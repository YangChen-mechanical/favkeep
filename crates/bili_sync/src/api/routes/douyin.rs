use axum::{
    Extension, Json, Router,
    extract::Path,
    routing::{get, post, put},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{
    error::InnerApiError,
    wrapper::{ApiError, ApiResponse},
};
use crate::config::VersionedConfig;
use crate::douyin::{self, Auth, runtime, store};

pub(super) fn router() -> Router {
    Router::new()
        .route("/douyin/status", get(status))
        .route("/douyin/resume", post(resume))
        .route("/config/douyin", put(interval))
        .route("/video-sources/douyin", get(folders))
        .route("/video-sources/douyin/refresh", post(refresh))
        .route("/video-sources/douyin/{id}", put(select))
        .route("/task/douyin", post(run))
}

fn bad(message: &str) -> ApiError {
    InnerApiError::BadRequest(message.into()).into()
}

fn saved() -> Result<Auth, ApiError> {
    Auth::read(&douyin::login::root().join("auth.json")).map_err(|_| bad("请先连接抖音账号"))
}

fn time(value: Option<i64>) -> Option<String> {
    value
        .and_then(|n| chrono::DateTime::from_timestamp(n, 0))
        .map(|v| v.to_rfc3339())
}

async fn status(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<Value>, ApiError> {
    let account = if let Ok(auth) = saved() {
        if store::bound_identity(&db).await?.as_deref() == Some(auth.identity.as_str()) {
            Some(store::account(&db, &auth.identity).await?)
        } else {
            None
        }
    } else {
        None
    };
    let paused = account.as_ref().and_then(|a| a.paused_reason.clone());
    let connected = account.is_some();
    let progress = douyin::progress::snapshot();
    Ok(ApiResponse::ok(json!({"connected":connected,"account_name":null,
        "auth_state": if paused.is_some() {"paused"} else if connected {"connected"} else {"disconnected"},
        "interval":VersionedConfig::get().read().douyin.interval,
        "paused_reason":paused,"rejections":account.as_ref().map(|a|a.rejections).unwrap_or(0),
        "task":{"is_running":douyin::RUNNING.try_lock().is_err(),
        "current":progress.current,"last_sync":progress.last_sync,
        "last_run":time(account.as_ref().and_then(|a|a.last_run)),
        "last_finish":time(account.as_ref().and_then(|a|a.last_finish)),
        "next_run": if paused.is_some() {None} else {runtime::next_run()},
        "last_error":account.as_ref().and_then(|a|a.last_error.clone())}})))
}

fn folder(row: store::StoredFolder) -> Value {
    json!({"id":row.id,"name":row.name,"enabled":row.selected,"available":row.available,
        "baseline_at":time(row.baseline_at),"last_sync_at":time(row.last_scan),
        "last_error":row.last_result.filter(|r|r!="complete"),"video_count":row.item_count})
}

async fn folders(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<Value>, ApiError> {
    let rows = match saved() {
        Ok(auth) => store::folders(&db, &auth.identity).await?,
        Err(_) => vec![],
    };
    Ok(ApiResponse::ok(
        json!({"folders":rows.into_iter().map(folder).collect::<Vec<_>>()}),
    ))
}

async fn refresh(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<Value>, ApiError> {
    runtime::refresh(&db, &VersionedConfig::get().snapshot()).await?;
    folders(Extension(db)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    enabled: bool,
}

async fn select(
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<String>,
    Json(input): Json<Selection>,
) -> Result<ApiResponse<Value>, ApiError> {
    info!(
        "[抖音] 收到更新收藏来源请求（{}）",
        if input.enabled { "启用" } else { "停用" }
    );
    let auth = saved().inspect_err(|_| warn!("[抖音] 更新收藏来源未执行：请先连接账号"))?;
    if !crate::platform::valid_id(&id) {
        warn!("[抖音] 更新收藏来源未执行：收藏夹标识无效");
        return Err(bad("无效收藏夹"));
    }
    let selected = store::set_selected(&db, &auth.identity, &id, input.enabled)
        .await
        .inspect_err(|_| error!("[抖音] 更新收藏来源失败，请刷新来源状态后重试"))?;
    info!(
        "[抖音] 收藏夹 {:?}（{}）已{}，{}",
        selected.name,
        selected.id,
        if selected.selected { "启用" } else { "停用" },
        if !selected.selected {
            "本地文件和观看记录保留"
        } else if selected.baseline_done {
            "沿用已有基线，保存后续识别到的新增视频"
        } else {
            "首次同步将建立历史基线，不下载已有历史内容"
        }
    );
    Ok(ApiResponse::ok(folder(selected)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Interval {
    interval: u64,
}

async fn interval(
    Extension(db): Extension<DatabaseConnection>,
    Json(input): Json<Interval>,
) -> Result<ApiResponse<Value>, ApiError> {
    info!("[抖音] 收到修改同步间隔请求：{} 秒", input.interval);
    if !(61..=31_536_000).contains(&input.interval) {
        warn!("[抖音] 同步间隔未修改：须为 61 至 31536000 秒");
        return Err(bad("扫描间隔须大于60秒且不超过一年"));
    }
    let config = VersionedConfig::get();
    let _lock = config.update_lock.lock().await;
    let mut next = (*config.snapshot()).clone();
    next.douyin.interval = input.interval;
    next.check()
        .inspect_err(|_| error!("[抖音] 同步间隔未保存：配置校验失败"))?;
    config
        .update_locked(next, &db)
        .await
        .inspect_err(|_| error!("[抖音] 同步间隔保存失败"))?;
    info!("[抖音] 同步间隔已保存：{} 秒", input.interval);
    drop(_lock);
    status(Extension(db)).await
}

async fn resume(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    runtime::resume(&db, &VersionedConfig::get().snapshot()).await?;
    Ok(ApiResponse::ok(true))
}

async fn run(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    info!("[抖音] 收到手动同步与重试失败项请求");
    let auth = saved().inspect_err(|_| warn!("[抖音] 手动同步未执行：请先连接账号"))?;
    let account = store::ensure_account(&db, &auth.identity)
        .await
        .inspect_err(|_| error!("[抖音] 手动同步未执行：账号状态读取失败"))?;
    if account.paused_reason.is_some()
        || account
            .next_allowed_at
            .is_some_and(|n| n > chrono::Utc::now().timestamp())
    {
        warn!("[抖音] 手动同步未执行：账号已暂停或正在冷却，请先处理后恢复");
        return Err(bad("抖音已暂停或正在冷却，请先处理后恢复"));
    }
    let guard = douyin::RUNNING.try_lock().map_err(|_| {
        warn!("[抖音] 手动同步未执行：账号正在处理其他操作");
        bad("抖音账号正在处理任务")
    })?;
    let retried = store::retry_failed(&db, &auth.identity, None)
        .await
        .inspect_err(|_| error!("[抖音] 重试失败项未完成：状态更新失败"))?;
    info!("[抖音] 已重新排队 {} 个符合当前来源规则的失败视频", retried);
    drop(guard);
    crate::task::DownloadTaskManager::get()
        .douyin_once()
        .await
        .inspect_err(|_| error!("[抖音] 手动同步调度失败"))?;
    info!("[抖音] 手动同步请求已提交，等待调度执行");
    Ok(ApiResponse::ok(true))
}

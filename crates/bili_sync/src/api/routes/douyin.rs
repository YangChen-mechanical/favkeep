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
use crate::douyin::{self, browser::Auth, runtime, store};

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
        Some(store::ensure_account(&db, &auth.identity).await?)
    } else {
        None
    };
    let paused = account.as_ref().and_then(|a| a.paused_reason.clone());
    let connected = account.is_some();
    Ok(ApiResponse::ok(json!({"connected":connected,"account_name":null,
        "auth_state": if paused.is_some() {"paused"} else if connected {"connected"} else {"disconnected"},
        "interval":VersionedConfig::get().read().douyin.interval,
        "paused_reason":paused,"rejections":account.as_ref().map(|a|a.rejections).unwrap_or(0),
        "task":{"is_running":douyin::RUNNING.try_lock().is_err(),
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
    let auth = saved()?;
    if !douyin::valid_id(&id) {
        return Err(bad("无效收藏夹"));
    }
    Ok(ApiResponse::ok(folder(
        store::set_selected(&db, &auth.identity, &id, input.enabled).await?,
    )))
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
    if !(61..=31_536_000).contains(&input.interval) {
        return Err(bad("扫描间隔须大于60秒且不超过一年"));
    }
    let config = VersionedConfig::get();
    let _lock = config.update_lock.lock().await;
    let mut next = (*config.snapshot()).clone();
    next.douyin.interval = input.interval;
    next.check()?;
    config.update_locked(next, &db).await?;
    drop(_lock);
    status(Extension(db)).await
}

async fn resume(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    let _lock = douyin::RUNNING.try_lock().map_err(|_| bad("抖音账号正在处理任务"))?;
    let auth = saved()?;
    let account = store::ensure_account(&db, &auth.identity).await?;
    if account
        .next_allowed_at
        .is_some_and(|n| n > chrono::Utc::now().timestamp())
    {
        return Err(bad("请等待本轮冷却结束"));
    }
    let config = VersionedConfig::get().snapshot();
    let (session, fresh) = match runtime::restore(&config).await {
        Ok(restored) => restored,
        Err(error) => {
            store::account_outcome(
                &db,
                &auth.identity,
                runtime::account_result(&Err(error.clone())),
                config.douyin.interval,
            )
            .await?;
            return Err(error.into());
        }
    };
    session.close().await?;
    store::resume_account(&db, &fresh.identity).await?;
    Ok(ApiResponse::ok(true))
}

async fn run(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    let auth = saved()?;
    let account = store::ensure_account(&db, &auth.identity).await?;
    if account.paused_reason.is_some()
        || account
            .next_allowed_at
            .is_some_and(|n| n > chrono::Utc::now().timestamp())
    {
        return Err(bad("抖音已暂停或正在冷却，请先处理后恢复"));
    }
    let guard = douyin::RUNNING.try_lock().map_err(|_| bad("抖音账号正在处理任务"))?;
    store::retry_failed(&db, &auth.identity).await?;
    drop(guard);
    crate::task::DownloadTaskManager::get().douyin_once().await?;
    Ok(ApiResponse::ok(true))
}

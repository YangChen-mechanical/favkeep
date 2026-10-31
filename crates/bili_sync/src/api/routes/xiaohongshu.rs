use crate::{
    api::{
        error::InnerApiError,
        wrapper::{ApiError, ApiResponse},
    },
    config::VersionedConfig,
    xiaohongshu::{self, client::Auth, runtime, store},
};
use axum::{
    Extension, Json, Router,
    routing::{get, post, put},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};
pub(super) fn router() -> Router {
    Router::new()
        .route("/xiaohongshu/status", get(status))
        .route("/xiaohongshu/resume", post(resume))
        .route("/video-sources/xiaohongshu", get(source).put(select))
        .route("/config/xiaohongshu", put(interval))
        .route("/task/xiaohongshu", post(run))
}
fn bad(message: &str) -> ApiError {
    InnerApiError::BadRequest(message.into()).into()
}
fn time(t: Option<i64>) -> Option<String> {
    t.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|t| t.to_rfc3339())
}
async fn status(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<Value>, ApiError> {
    let account = store::account(&db).await?;
    let source = store::source(&db).await?;
    let connected = account
        .as_ref()
        .and_then(|a| a.identity.as_deref())
        .is_some_and(|id| Auth::read(&xiaohongshu::login::root().join("auth.json")).is_ok_and(|a| a.identity == id));
    let paused = account.as_ref().and_then(|a| a.paused_reason.clone());
    let progress = xiaohongshu::progress::snapshot();
    Ok(ApiResponse::ok(
        json!({"connected":connected,"account_name":null,"auth_state":if paused.is_some(){"paused"}else if connected{"connected"}else{"disconnected"},"interval":VersionedConfig::get().read().xiaohongshu.interval,"window_size":50,"paused_reason":paused,"rejections":account.as_ref().map(|a|a.rejections).unwrap_or(0),"next_allowed_at":time(account.as_ref().and_then(|a|a.next_allowed_at)),"task":{"is_running":xiaohongshu::RUNNING.try_lock().is_err(),"current":progress.current,"last_sync":progress.last_sync,"last_run":time(account.as_ref().and_then(|a|a.last_run)),"last_finish":time(account.as_ref().and_then(|a|a.last_finish)),"next_run":if source.enabled&&paused.is_none(){runtime::next_run()}else{None},"last_error":account.as_ref().and_then(|a|a.last_error.clone())}}),
    ))
}
async fn source(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<store::Source>, ApiError> {
    Ok(ApiResponse::ok(store::source(&db).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    enabled: bool,
}
async fn select(
    Extension(db): Extension<DatabaseConnection>,
    Json(input): Json<Selection>,
) -> Result<ApiResponse<store::Source>, ApiError> {
    let selected = store::set_enabled(&db, input.enabled)
        .await
        .map_err(|_| bad("请先连接小红书账号"))?;
    info!(
        "[小红书] 全部收藏来源已{}，已有文件和观看记录保留",
        if selected.enabled { "启用" } else { "停用" }
    );
    Ok(ApiResponse::ok(selected))
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
    let lock = config.update_lock.lock().await;
    let mut next = (*config.snapshot()).clone();
    next.xiaohongshu.interval = input.interval;
    next.check()?;
    config.update_locked(next, &db).await?;
    drop(lock);
    info!("[小红书] 同步间隔已更新为 {} 秒", input.interval);
    status(Extension(db)).await
}
async fn resume(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    runtime::resume(&db, &VersionedConfig::get().snapshot())
        .await
        .map_err(|e| bad(e.message()))?;
    Ok(ApiResponse::ok(true))
}
async fn run(Extension(db): Extension<DatabaseConnection>) -> Result<ApiResponse<bool>, ApiError> {
    let guard = xiaohongshu::RUNNING
        .try_lock()
        .map_err(|_| bad("小红书账号正在处理任务"))?;
    let account = store::account(&db).await?.ok_or_else(|| bad("请先连接小红书"))?;
    if account.identity.is_none() {
        return Err(bad("请先连接小红书"));
    }
    if account.paused_reason.is_some()
        || account
            .next_allowed_at
            .is_some_and(|t| t > chrono::Utc::now().timestamp())
    {
        return Err(bad("小红书已暂停或正在冷却，请处理后恢复"));
    }
    if !store::source(&db).await?.enabled {
        return Err(bad("请先启用全部收藏来源"));
    }
    let retried = store::retry_failed(&db, None).await?;
    info!("[小红书] 手动同步已接收，明确重试 {} 个失败项", retried);
    drop(guard);
    crate::task::DownloadTaskManager::get().xiaohongshu_once().await?;
    Ok(ApiResponse::ok(true))
}

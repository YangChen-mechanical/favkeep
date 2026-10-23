use axum::extract::{OriginalUri, Request};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Router, middleware};
use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use reqwest::StatusCode;
use tower_sessions::Session;

use crate::api::wrapper::ApiResponse;
use crate::config::VersionedConfig;

mod config;
mod dashboard;
mod login;
mod me;
mod media;
mod media_cover;
mod media_session;
mod task;
mod video_sources;
mod videos;
mod watch_progress;
mod ws;

pub use ws::{LogHelper, MAX_HISTORY_LOGS};

pub fn router() -> Router {
    Router::new().nest(
        "/api",
        config::router()
            .merge(me::router())
            .merge(login::router())
            .merge(video_sources::router())
            .merge(videos::router())
            .merge(dashboard::router())
            .merge(ws::router())
            .merge(task::router())
            .merge(media::router())
            .merge(media_session::router())
            .merge(watch_progress::router())
            .layer(middleware::from_fn(auth)),
    )
}

/// 中间件：使用 auth token 对请求进行身份验证
pub async fn auth(
    session: Session,
    OriginalUri(uri): OriginalUri,
    mut headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let config = VersionedConfig::get().read();
    let token = config.auth_token.as_str();
    if headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s == token)
    {
        return Ok(next.run(request).await);
    }
    // Native video requests cannot set Authorization; this session grants media reads only.
    if matches!(*request.method(), axum::http::Method::GET | axum::http::Method::HEAD)
        && uri.path().starts_with("/api/media/")
        && session
            .get::<String>("media_auth")
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some_and(|value| value == token)
    {
        return Ok(next.run(request).await);
    }
    if let Some(protocol) = headers.remove("Sec-WebSocket-Protocol")
        && protocol
            .to_str()
            .ok()
            .and_then(|s| BASE64_URL_SAFE_NO_PAD.decode(s).ok())
            .is_some_and(|s| s == token.as_bytes())
    {
        let mut resp = next.run(request).await;
        resp.headers_mut().insert("Sec-WebSocket-Protocol", protocol);
        return Ok(resp);
    }
    Ok(ApiResponse::<()>::unauthorized("auth token does not match").into_response())
}

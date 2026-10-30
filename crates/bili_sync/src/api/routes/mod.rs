use axum::Router;

mod config;
mod dashboard;
mod douyin;
mod login;
mod me;
mod media;
mod media_cover;
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
            .route(
                "/platforms",
                axum::routing::get(|| async { crate::api::wrapper::ApiResponse::ok(crate::platform::platforms()) }),
            )
            .merge(douyin::router())
            .merge(me::router())
            .merge(login::router())
            .merge(video_sources::router())
            .merge(videos::router())
            .merge(dashboard::router())
            .merge(ws::router())
            .merge(task::router())
            .merge(media::router())
            .merge(watch_progress::router())
            .merge(crate::auth::router())
            .fallback(|| async { crate::api::wrapper::ApiResponse::<()>::not_found("API not found") })
            .layer(axum::middleware::from_fn(crate::auth::protect)),
    )
}

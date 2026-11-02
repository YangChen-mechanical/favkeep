use axum::{
    Router,
    routing::{get, post},
};

mod mutations;
mod query;
#[cfg(test)]
mod tests;
mod watch;

use mutations::{
    clear_and_reset_video_status, reset_filtered_video_status, reset_video_status, update_filtered_video_status,
    update_video_status,
};
use query::{get_video, get_videos};

pub(super) fn router() -> Router {
    Router::new()
        .route("/videos", get(get_videos))
        .route("/videos/{id}", get(get_video))
        .route(
            "/videos/{id}/clear-and-reset-status",
            post(clear_and_reset_video_status),
        )
        .route("/videos/{id}/reset-status", post(reset_video_status))
        .route("/videos/{id}/update-status", post(update_video_status))
        .route("/videos/reset-status", post(reset_filtered_video_status))
        .route("/videos/update-status", post(update_filtered_video_status))
}

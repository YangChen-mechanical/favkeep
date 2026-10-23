use axum::Router;
use axum::routing::post;
use tower_sessions::Session;

use crate::api::wrapper::{ApiError, ApiResponse};
use crate::config::VersionedConfig;

pub(super) fn router() -> Router {
    Router::new().route("/media-session", post(open).delete(close))
}

async fn open(session: Session) -> Result<ApiResponse<()>, ApiError> {
    session.cycle_id().await?;
    session
        .insert("media_auth", VersionedConfig::get().read().auth_token.clone())
        .await?;
    Ok(ApiResponse::ok(()))
}

async fn close(session: Session) -> Result<ApiResponse<()>, ApiError> {
    session.flush().await?;
    Ok(ApiResponse::ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Extension,
        body::Body,
        http::{Request, StatusCode, header},
    };
    use bili_sync_migration::{Migrator, MigratorTrait};
    use sea_orm::Database;
    use tower::ServiceExt;
    use tower_sessions::{MemoryStore, SessionManagerLayer};

    #[tokio::test]
    async fn playback_cookie_cannot_access_admin_routes_and_is_revoked_on_logout() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let app = crate::api::router()
            .layer(Extension(db))
            .layer(SessionManagerLayer::new(MemoryStore::default()).with_secure(false));
        let token = VersionedConfig::get().read().auth_token.clone();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/media/pages/999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media-session")
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/media/pages/999")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/videos")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/media-session")
                    .header(header::AUTHORIZATION, &token)
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/media/pages/999")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

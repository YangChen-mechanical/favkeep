use std::path::{Path as FilePath, PathBuf};

use axum::Router;
use axum::extract::{Extension, Path, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use favkeep_entity::{page, video};
use sea_orm::{DatabaseConnection, EntityTrait};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::api::wrapper::ApiResponse;

pub(super) fn router() -> Router {
    Router::new()
        .route("/media/pages/{id}", get(stream_page))
        .route("/media/videos/{id}/cover", get(super::media_cover::cover))
}

async fn stream_page(
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<i32>,
    request: Request,
) -> Result<Response, StatusCode> {
    let Some(page) = page::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Ok(ApiResponse::<()>::not_found("Media not found").into_response());
    };
    if !crate::media::page_ready(&db, page.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Ok(ApiResponse::<()>::not_found("Media has not finished downloading").into_response());
    }
    let Some(path) = page.path else {
        return Ok(ApiResponse::<()>::not_found("Media not found").into_response());
    };
    let Some(video) = video::Entity::find_by_id(page.video_id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Ok(ApiResponse::<()>::not_found("Media not found").into_response());
    };
    let path = checked_path(FilePath::new(&video.path), FilePath::new(&path)).await?;
    let mut response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    Ok(response)
}

async fn checked_path(root: &FilePath, file: &FilePath) -> Result<PathBuf, StatusCode> {
    let root = tokio::fs::canonicalize(root).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let path = tokio::fs::canonicalize(file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !path.starts_with(&root)
        || path
            .extension()
            .is_none_or(|ext| !ext.as_encoded_bytes().eq_ignore_ascii_case(b"mp4"))
    {
        return Err(StatusCode::NOT_FOUND);
    }
    if !tokio::fs::metadata(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .is_file()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;

    #[tokio::test]
    async fn only_serves_mp4_inside_the_work_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("media");
        tokio::fs::create_dir(&root).await.unwrap();
        let video = root.join("video.mp4");
        tokio::fs::write(&video, b"0123456789").await.unwrap();
        let outside = dir.path().join("private.mp4");
        tokio::fs::write(&outside, b"private").await.unwrap();
        let config = root.join("config.sqlite");
        tokio::fs::write(&config, b"secret").await.unwrap();
        assert!(checked_path(&root, &video).await.is_ok());
        assert_eq!(checked_path(&root, &outside).await, Err(StatusCode::NOT_FOUND));
        assert_eq!(checked_path(&root, &config).await, Err(StatusCode::NOT_FOUND));

        let response = ServeFile::new(checked_path(&root, &video).await.unwrap())
            .oneshot(
                Request::builder()
                    .header(header::RANGE, "bytes=2-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(to_bytes(response.into_body(), 100).await.unwrap().as_ref(), b"2345");

        let response = ServeFile::new(&video)
            .oneshot(Request::builder().method("HEAD").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
        assert!(to_bytes(response.into_body(), 100).await.unwrap().is_empty());

        let response = ServeFile::new(&video)
            .oneshot(
                Request::builder()
                    .header(header::RANGE, "bytes=100-200")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    }
}

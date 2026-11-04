use std::path::{Path as FilePath, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use axum::extract::{Extension, Path, Request};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use favkeep_entity::{page, video};
use reqwest::{Client, Url, redirect::Policy};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::sync::Semaphore;

use crate::config::CONFIG_DIR;

const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;
static FETCH_LIMIT: Semaphore = Semaphore::const_new(3);
static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent("Mozilla/5.0 favkeep")
        .referer(true)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || !allowed_url(attempt.url()) {
                attempt.error("cover redirect is not allowed")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .expect("cover HTTP client")
});

fn allowed_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url
            .host_str()
            .is_some_and(|host| host == "hdslb.com" || host.ends_with(".hdslb.com"))
}

fn source_url(source: &str) -> Result<Url, StatusCode> {
    let mut url = Url::parse(source).map_err(|_| StatusCode::NOT_FOUND)?;
    if url.scheme() == "http" {
        url.set_scheme("https").map_err(|_| StatusCode::NOT_FOUND)?;
    }
    allowed_url(&url).then_some(url).ok_or(StatusCode::NOT_FOUND)
}

fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_COVER_BYTES {
        return None;
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

async fn read_image(path: &FilePath) -> Option<Vec<u8>> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() > MAX_COVER_BYTES as u64 {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    image_type(&bytes)?;
    Some(bytes)
}

async fn local_image(root: &FilePath, candidates: Vec<PathBuf>) -> Option<Vec<u8>> {
    let root = tokio::fs::canonicalize(root).await.ok()?;
    for candidate in candidates {
        let Ok(path) = tokio::fs::canonicalize(candidate).await else {
            continue;
        };
        if path.starts_with(&root)
            && let Some(bytes) = read_image(&path).await
        {
            return Some(bytes);
        }
    }
    None
}

fn cache_path(cache: &FilePath, id: i32, url: &Url) -> PathBuf {
    cache.join(format!("{id}-{:x}.image", md5::compute(url.as_str())))
}

async fn load_cover(video: &video::Model, pages: &[page::Model], cache: &FilePath) -> Result<Vec<u8>, StatusCode> {
    let root = FilePath::new(&video.path);
    let mut candidates = vec![root.join("poster.jpg")];
    for p in pages {
        if let Some(path) = p.path.as_deref().map(FilePath::new)
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            candidates.push(path.with_file_name(format!("{stem}-poster.jpg")));
            candidates.push(path.with_file_name(format!("{stem}-thumb.jpg")));
        }
    }
    if let Some(bytes) = local_image(root, candidates).await {
        return Ok(bytes);
    }
    let url = source_url(&video.cover)?;
    let target = cache_path(cache, video.id, &url);
    if let Some(bytes) = read_image(&target).await {
        return Ok(bytes);
    }
    let _permit = FETCH_LIMIT
        .acquire()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if let Some(bytes) = read_image(&target).await {
        return Ok(bytes);
    }
    let mut response = CLIENT
        .get(url)
        .header(header::REFERER, "https://www.bilibili.com/")
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_COVER_BYTES as u64)
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|t| {
                matches!(
                    t.split(';').next().unwrap_or("").trim(),
                    "image/jpeg" | "image/png" | "image/webp"
                )
            })
    {
        return Err(StatusCode::BAD_GATEWAY);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| StatusCode::BAD_GATEWAY)? {
        if bytes.len() + chunk.len() > MAX_COVER_BYTES {
            return Err(StatusCode::BAD_GATEWAY);
        }
        bytes.extend_from_slice(&chunk);
    }
    image_type(&bytes).ok_or(StatusCode::BAD_GATEWAY)?;
    tokio::fs::create_dir_all(cache)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let temporary = cache.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, &bytes)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if tokio::fs::rename(&temporary, &target).await.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
        if read_image(&target).await.is_none() {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    Ok(bytes)
}

pub(super) async fn cover(
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<i32>,
    request: Request,
) -> Result<Response, StatusCode> {
    let video = video::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let pages = page::Entity::find()
        .filter(page::Column::VideoId.eq(id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bytes = load_cover(&video, &pages, &CONFIG_DIR.join("cover-cache")).await?;
    let mime = image_type(&bytes).ok_or(StatusCode::NOT_FOUND)?;
    let length = bytes.len().to_string();
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_LENGTH, length.as_str()),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        if request.method() == Method::HEAD {
            vec![]
        } else {
            bytes
        },
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn covers_use_safe_local_files_or_cached_images_without_network() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("work");
        let cache = temp.path().join("cache");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&cache).await.unwrap();
        let jpeg = b"\xff\xd8\xffsample";
        let outside = temp.path().join("private.jpg");
        tokio::fs::write(&outside, jpeg).await.unwrap();
        assert!(local_image(&root, vec![outside]).await.is_none());
        let local = root.join("poster.jpg");
        tokio::fs::write(&local, jpeg).await.unwrap();
        let mut video = video::Model {
            id: 42,
            path: root.to_string_lossy().into(),
            cover: "https://i0.hdslb.com/test.jpg".into(),
            ..Default::default()
        };
        assert_eq!(load_cover(&video, &[], &cache).await.unwrap(), jpeg);
        tokio::fs::remove_file(local).await.unwrap();
        let url = source_url(&video.cover).unwrap();
        tokio::fs::write(cache_path(&cache, video.id, &url), jpeg)
            .await
            .unwrap();
        assert_eq!(load_cover(&video, &[], &cache).await.unwrap(), jpeg);
        video.cover = "http://127.0.0.1/private.jpg".into();
        assert_eq!(load_cover(&video, &[], &cache).await, Err(StatusCode::NOT_FOUND));
        for url in [
            "https://i0.hdslb.com.evil.test/a",
            "https://user@i0.hdslb.com/a",
            "https://i0.hdslb.com:444/a",
            "file:///etc/passwd",
            "https://localhost/a",
        ] {
            assert!(source_url(url).is_err());
        }
        assert!(source_url("http://i1.hdslb.com/a").is_ok());
        assert!(image_type(b"<svg onload=alert(1) />").is_none());
        assert!(image_type(&vec![0; MAX_COVER_BYTES + 1]).is_none());
    }
}

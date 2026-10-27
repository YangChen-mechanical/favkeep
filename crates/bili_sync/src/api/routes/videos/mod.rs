use std::collections::HashSet;

use anyhow::{Context, Result};
use axum::extract::{Extension, Path, Query};
use axum::routing::{get, post};
use axum::{Json, Router};
use bili_sync_entity::*;
use chrono::NaiveDateTime;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, Select, TransactionTrait, TryIntoModel,
};

use crate::api::error::InnerApiError;
use crate::api::helper::{update_page_download_status, update_video_download_status};
mod watch;

use crate::api::request::{
    ResetFilteredVideoStatusRequest, ResetVideoStatusRequest, UpdateFilteredVideoStatusRequest,
    UpdateVideoStatusRequest, VideosRequest, WatchSort, WatchState,
};
use crate::api::response::{
    ClearAndResetVideoStatusResponse, PageInfo, ResetFilteredVideosResponse, ResetVideoResponse, SimplePageInfo,
    SimpleVideoInfo, UpdateFilteredVideoStatusResponse, UpdateVideoStatusResponse, VideoInfo, VideoResponse,
    VideosResponse,
};
use crate::api::wrapper::{ApiError, ApiResponse, ValidatedJson};
use crate::utils::status::{PageStatus, VideoStatus};

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

fn apply_created_at_filter(
    mut query: Select<video::Entity>,
    created_from: Option<NaiveDateTime>,
    created_to: Option<NaiveDateTime>,
) -> Select<video::Entity> {
    if let Some(created_from) = created_from {
        query = query.filter(
            Expr::cust_with_expr("datetime(?, 'localtime')", Expr::col(video::Column::CreatedAt))
                .gte(created_from.format("%Y-%m-%d %H:%M:%S").to_string()),
        );
    }
    if let Some(created_to) = created_to {
        query = query.filter(
            Expr::cust_with_expr("datetime(?, 'localtime')", Expr::col(video::Column::CreatedAt))
                .lte(created_to.format("%Y-%m-%d %H:%M:%S").to_string()),
        );
    }
    query
}

/// 列出视频的基本信息，支持根据视频来源筛选、名称查找和分页
pub async fn get_videos(
    Extension(db): Extension<DatabaseConnection>,
    Query(params): Query<VideosRequest>,
) -> Result<ApiResponse<VideosResponse>, ApiError> {
    let mut query = video::Entity::find();
    if let Some(platform) = &params.platform {
        if !matches!(platform.as_str(), "bilibili" | "douyin") {
            return Err(InnerApiError::BadRequest("无效的平台".into()).into());
        }
        query = query.filter(video::Column::Platform.eq(platform));
    }
    if let Some(state) = params.watch_state {
        let having = match state {
            WatchState::Watched => "SUM(COALESCE(w.completed,0)) = COUNT(*)",
            WatchState::Unwatched => "SUM(CASE WHEN w.completed OR w.position_ms > 0 THEN 1 ELSE 0 END) = 0",
            WatchState::InProgress => {
                "SUM(CASE WHEN w.completed OR w.position_ms > 0 THEN 1 ELSE 0 END) > 0 AND SUM(COALESCE(w.completed,0)) < COUNT(*)"
            }
        };
        query = query.filter(Expr::cust(format!(
            "video.id IN (SELECT p.video_id FROM page p LEFT JOIN watch_progress w ON w.page_id=p.id
             WHERE ((p.download_status >> 3) & 7)=7 AND lower(p.path) LIKE '%.mp4'
             GROUP BY p.video_id HAVING {having})"
        )));
    }
    if params.saved_only == Some(true) {
        query = query.filter(
            video::Column::Id.in_subquery(
                page::Entity::find()
                    .select_only()
                    .column(page::Column::VideoId)
                    .filter(PageStatus::query_builder().subtask_succeeded(1))
                    .filter(page::Column::Path.ends_with(".mp4"))
                    .into_query(),
            ),
        );
    }
    for (field, column) in [
        (params.collection, video::Column::CollectionId),
        (params.favorite, video::Column::FavoriteId),
        (params.submission, video::Column::SubmissionId),
        (params.watch_later, video::Column::WatchLaterId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = params.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(&query_word))
                .or(video::Column::UpperName.contains(query_word)),
        );
    }
    if let Some(status_filter) = params.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = params.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, params.created_from, params.created_to);
    let total_count = query.clone().count(&db).await?;
    let (page, page_size) = if let (Some(page), Some(page_size)) = (params.page, params.page_size) {
        (page, page_size)
    } else {
        (0, 10)
    };
    query = match params.sort {
        Some(WatchSort::Added) => query
            .order_by_desc(video::Column::CreatedAt)
            .order_by_desc(video::Column::Id),
        Some(WatchSort::Title) => query.order_by_asc(video::Column::Name).order_by_asc(video::Column::Id),
        None => query.order_by_desc(video::Column::Id),
    };
    let videos = query
        .into_partial_model::<VideoInfo>()
        .paginate(&db, page_size)
        .fetch_page(page)
        .await?;
    let watch_summaries = if params.saved_only == Some(true) {
        Some(watch::summaries(&db, &videos.iter().map(|v| v.id).collect::<Vec<_>>()).await?)
    } else {
        None
    };
    Ok(ApiResponse::ok(VideosResponse {
        videos,
        total_count,
        watch_summaries,
    }))
}

pub async fn get_video(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<ApiResponse<VideoResponse>, ApiError> {
    let (video_info, pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Pid)
            .order_by_asc(page::Column::Id)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    Ok(ApiResponse::ok(VideoResponse {
        video: video_info,
        watch_progress: watch_progress::Entity::find()
            .filter(watch_progress::Column::PageId.is_in(pages_info.iter().map(|p| p.id)))
            .all(&db)
            .await?,
        playable_page_ids: page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .filter(PageStatus::query_builder().subtask_succeeded(1))
            .filter(page::Column::Path.ends_with(".mp4"))
            .all(&db)
            .await?
            .iter()
            .map(|p| p.id)
            .collect(),
        watch_summary: watch::summaries(&db, &[id]).await?.pop(),
        pages: pages_info,
    }))
}

pub async fn reset_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    Json(request): Json<ResetVideoStatusRequest>,
) -> Result<ApiResponse<ResetVideoResponse>, ApiError> {
    let (video_info, pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Cid)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(mut video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    if video_info.platform == "douyin" && request.force {
        return Err(InnerApiError::BadRequest("抖音仅支持重试失败下载".into()).into());
    }
    let resetted_pages_info = pages_info
        .into_iter()
        .filter_map(|mut page_info| {
            let mut page_status = PageStatus::from(page_info.download_status);
            if (request.force && page_status.force_reset_failed()) || page_status.reset_failed() {
                page_info.download_status = page_status.into();
                Some(page_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut video_status = VideoStatus::from(video_info.download_status);
    let mut video_resetted = (request.force && video_status.force_reset_failed()) || video_status.reset_failed();
    if !resetted_pages_info.is_empty() {
        video_status.set(4, 0); //  将“分页下载”重置为 0
        video_resetted = true;
    }
    let resetted_videos_info = if video_resetted {
        video_info.download_status = video_status.into();
        vec![&video_info]
    } else {
        vec![]
    };
    let resetted = !resetted_videos_info.is_empty() || !resetted_pages_info.is_empty();
    if resetted {
        let txn = db.begin().await?;
        if !resetted_videos_info.is_empty() {
            // 只可能有 1 个元素，所以不用 batch
            update_video_download_status::<VideoInfo>(&txn, &resetted_videos_info, None).await?;
        }
        if !resetted_pages_info.is_empty() {
            update_page_download_status(&txn, &resetted_pages_info, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(ResetVideoResponse {
        resetted,
        video: video_info,
        pages: resetted_pages_info,
    }))
}

pub async fn clear_and_reset_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<ApiResponse<ClearAndResetVideoStatusResponse>, ApiError> {
    let video_info = video::Entity::find_by_id(id).one(&db).await?;
    let Some(video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    if video_info.platform == "douyin" {
        return Err(InnerApiError::BadRequest("抖音不支持清理重下".into()).into());
    }
    let txn = db.begin().await?;
    let mut video_info = video_info.into_active_model();
    video_info.single_page = Set(None);
    video_info.download_status = Set(0);
    video_info.valid = Set(true);
    let video_info = video_info.update(&txn).await?;
    page::Entity::delete_many()
        .filter(page::Column::VideoId.eq(id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    let video_info = video_info.try_into_model()?;
    let warning = if video_info.path.is_empty() {
        None
    } else {
        tokio::fs::remove_dir_all(&video_info.path)
            .await
            .context(format!("删除本地路径「{}」失败", video_info.path))
            .err()
            .map(|e| format!("{:#}", e))
    };
    Ok(ApiResponse::ok(ClearAndResetVideoStatusResponse {
        warning,
        video: VideoInfo {
            id: video_info.id,
            platform: video_info.platform,
            platform_id: video_info.platform_id,
            bvid: video_info.bvid,
            name: video_info.name,
            upper_name: video_info.upper_name,
            valid: video_info.valid,
            should_download: video_info.should_download,
            download_status: video_info.download_status,
            collection_id: video_info.collection_id,
            favorite_id: video_info.favorite_id,
            submission_id: video_info.submission_id,
            watch_later_id: video_info.watch_later_id,
        },
    }))
}

pub async fn reset_filtered_video_status(
    Extension(db): Extension<DatabaseConnection>,
    Json(request): Json<ResetFilteredVideoStatusRequest>,
) -> Result<ApiResponse<ResetFilteredVideosResponse>, ApiError> {
    let mut query = video::Entity::find().filter(video::Column::Platform.eq("bilibili"));
    for (field, column) in [
        (request.collection, video::Column::CollectionId),
        (request.favorite, video::Column::FavoriteId),
        (request.submission, video::Column::SubmissionId),
        (request.watch_later, video::Column::WatchLaterId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = request.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(&query_word))
                .or(video::Column::UpperName.contains(query_word)),
        );
    }
    if let Some(status_filter) = request.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = request.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, request.created_from, request.created_to);
    let all_videos = query.into_partial_model::<SimpleVideoInfo>().all(&db).await?;
    let all_pages = page::Entity::find()
        .filter(page::Column::VideoId.is_in(all_videos.iter().map(|v| v.id)))
        .into_partial_model::<SimplePageInfo>()
        .all(&db)
        .await?;
    let resetted_pages_info = all_pages
        .into_iter()
        .filter_map(|mut page_info| {
            let mut page_status = PageStatus::from(page_info.download_status);
            if (request.force && page_status.force_reset_failed()) || page_status.reset_failed() {
                page_info.download_status = page_status.into();
                Some(page_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let video_ids_with_resetted_pages: HashSet<i32> = resetted_pages_info.iter().map(|page| page.video_id).collect();
    let resetted_videos_info = all_videos
        .into_iter()
        .filter_map(|mut video_info| {
            let mut video_status = VideoStatus::from(video_info.download_status);
            let mut video_resetted =
                (request.force && video_status.force_reset_failed()) || video_status.reset_failed();
            if video_ids_with_resetted_pages.contains(&video_info.id) {
                video_status.set(4, 0); // 将"分页下载"重置为 0
                video_resetted = true;
            }
            if video_resetted {
                video_info.download_status = video_status.into();
                Some(video_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let has_video_updates = !resetted_videos_info.is_empty();
    let has_page_updates = !resetted_pages_info.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status(&txn, &resetted_videos_info, Some(500)).await?;
        }
        if has_page_updates {
            update_page_download_status(&txn, &resetted_pages_info, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(ResetFilteredVideosResponse {
        resetted: has_video_updates || has_page_updates,
        resetted_videos_count: resetted_videos_info.len(),
        resetted_pages_count: resetted_pages_info.len(),
    }))
}

pub async fn update_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    ValidatedJson(request): ValidatedJson<UpdateVideoStatusRequest>,
) -> Result<ApiResponse<UpdateVideoStatusResponse>, ApiError> {
    let (video_info, mut pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Cid)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(mut video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    if video_info.platform == "douyin" {
        return Err(InnerApiError::BadRequest("抖音状态由扫描与下载任务维护".into()).into());
    }
    let mut video_status = VideoStatus::from(video_info.download_status);
    for update in &request.video_updates {
        video_status.set(update.status_index, update.status_value);
    }
    video_info.download_status = video_status.into();
    let mut updated_pages_info = Vec::new();
    let mut page_id_map = pages_info
        .iter_mut()
        .map(|page| (page.id, page))
        .collect::<std::collections::HashMap<_, _>>();
    for page_update in &request.page_updates {
        if let Some(page_info) = page_id_map.remove(&page_update.page_id) {
            let mut page_status = PageStatus::from(page_info.download_status);
            for update in &page_update.updates {
                page_status.set(update.status_index, update.status_value);
            }
            page_info.download_status = page_status.into();
            updated_pages_info.push(page_info);
        }
    }
    let has_video_updates = !request.video_updates.is_empty();
    let has_page_updates = !updated_pages_info.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status::<VideoInfo>(&txn, &[&video_info], None).await?;
        }
        if has_page_updates {
            update_page_download_status::<PageInfo>(&txn, &updated_pages_info, None).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(UpdateVideoStatusResponse {
        success: has_video_updates || has_page_updates,
        video: video_info,
        pages: pages_info,
    }))
}

pub async fn update_filtered_video_status(
    Extension(db): Extension<DatabaseConnection>,
    ValidatedJson(request): ValidatedJson<UpdateFilteredVideoStatusRequest>,
) -> Result<ApiResponse<UpdateFilteredVideoStatusResponse>, ApiError> {
    let mut query = video::Entity::find().filter(video::Column::Platform.eq("bilibili"));
    for (field, column) in [
        (request.collection, video::Column::CollectionId),
        (request.favorite, video::Column::FavoriteId),
        (request.submission, video::Column::SubmissionId),
        (request.watch_later, video::Column::WatchLaterId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = request.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(&query_word))
                .or(video::Column::UpperName.contains(query_word)),
        );
    }
    if let Some(status_filter) = request.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = request.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, request.created_from, request.created_to);
    let mut all_videos = query.into_partial_model::<SimpleVideoInfo>().all(&db).await?;
    let mut all_pages = page::Entity::find()
        .filter(page::Column::VideoId.is_in(all_videos.iter().map(|v| v.id)))
        .into_partial_model::<SimplePageInfo>()
        .all(&db)
        .await?;
    for video_info in all_videos.iter_mut() {
        let mut video_status = VideoStatus::from(video_info.download_status);
        for update in &request.video_updates {
            video_status.set(update.status_index, update.status_value);
        }
        video_info.download_status = video_status.into();
    }
    for page_info in all_pages.iter_mut() {
        let mut page_status = PageStatus::from(page_info.download_status);
        for update in &request.page_updates {
            page_status.set(update.status_index, update.status_value);
        }
        page_info.download_status = page_status.into();
    }
    let has_video_updates = !all_videos.is_empty();
    let has_page_updates = !all_pages.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status(&txn, &all_videos, Some(500)).await?;
        }
        if has_page_updates {
            update_page_download_status(&txn, &all_pages, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(UpdateFilteredVideoStatusResponse {
        success: has_video_updates || has_page_updates,
        updated_videos_count: all_videos.len(),
        updated_pages_count: all_pages.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use bili_sync_migration::{Migrator, MigratorTrait};
    use sea_orm::Database;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn list(db: &DatabaseConnection, params: &str) -> Value {
        let response = router()
            .layer(Extension(db.clone()))
            .oneshot(
                Request::builder()
                    .uri(format!("/videos?{params}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()["data"].take()
    }

    #[tokio::test]
    async fn douyin_filters_and_bili_mutations_preserve_qualified_media() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        video::ActiveModel::from(video::Model {
            id: 1,
            platform: "douyin".into(),
            platform_id: Some("old-video-new-collection".into()),
            name: "saved video".into(),
            download_status: 1,
            ..Default::default()
        })
        .insert(&db)
        .await
        .unwrap();
        page::ActiveModel {
            id: Set(1),
            video_id: Set(1),
            pid: Set(1),
            name: Set("saved video".into()),
            duration: Set(25),
            download_status: Set(1 << 3),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let response = list(&db, "platform=douyin").await;
        assert_eq!(response["total_count"], 1);
        assert_eq!(response["videos"][0]["platform_id"], "old-video-new-collection");
        assert_eq!(response["videos"][0]["bvid"], Value::Null);
        assert_eq!(list(&db, "platform=bilibili").await["total_count"], 0);
        for (path, body, expected) in [
            ("/videos/1/clear-and-reset-status", json!({}), StatusCode::BAD_REQUEST),
            ("/videos/1/reset-status", json!({"force":true}), StatusCode::BAD_REQUEST),
            (
                "/videos/1/update-status",
                json!({"video_updates":[{"status_index":4,"status_value":7}]}),
                StatusCode::BAD_REQUEST,
            ),
            ("/videos/reset-status", json!({"force":true}), StatusCode::OK),
            (
                "/videos/update-status",
                json!({"video_updates":[{"status_index":4,"status_value":7}]}),
                StatusCode::OK,
            ),
        ] {
            let response = router()
                .layer(Extension(db.clone()))
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
            assert_eq!(
                video::Entity::find_by_id(1)
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .download_status,
                1
            );
            assert_eq!(
                page::Entity::find_by_id(1)
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .download_status,
                1 << 3
            );
        }
    }

    #[tokio::test]
    async fn saved_only_filters_before_pagination_without_changing_existing_queries() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        for id in 1..=8 {
            video::ActiveModel::from(video::Model {
                id,
                name: format!("Video {id}"),
                bvid: Some(format!("BV{id}")),
                upper_name: format!("Author{id}"),
                // Saved files remain watchable even if the source is now invalid or skipped.
                valid: false,
                should_download: false,
                ..Default::default()
            })
            .insert(&db)
            .await
            .unwrap();
        }
        for (id, video_id, path, status) in [
            (1, 1, Some("partial.mp4"), 0),
            (2, 1, Some("saved.mp4"), 1 | (7 << 3) | (1 << 12)),
            (3, 2, Some("saved.MP4"), 7 << 3),
            (4, 4, None, 7 << 3),
            (5, 5, Some("saved.mkv"), 7 << 3),
            (6, 6, Some("unfinished.mp4"), 0),
            (7, 7, Some(""), 7 << 3),
            (8, 8, Some("first.mp4"), 7 << 3),
            (9, 8, Some("second.mp4"), 7 << 3),
        ] {
            page::ActiveModel {
                id: Set(id),
                video_id: Set(video_id),
                cid: Set(Some(id.into())),
                pid: Set(id),
                name: Set(format!("Part {id}")),
                duration: Set(60),
                path: Set(path.map(str::to_owned)),
                download_status: Set(status),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let first = list(&db, "saved_only=true&page=0&page_size=2").await;
        assert_eq!(first["total_count"], 3);
        assert_eq!(first["videos"][0]["id"], 8);
        assert_eq!(first["videos"][1]["id"], 2);
        let second = list(&db, "saved_only=true&page=1&page_size=2").await;
        assert_eq!(second["total_count"], 3);
        assert_eq!(second["videos"].as_array().unwrap().len(), 1);
        assert_eq!(second["videos"][0]["id"], 1);
        assert_eq!(
            list(&db, "saved_only=true&page=2&page_size=2").await["videos"],
            json!([])
        );
        for query in ["Video%201", "BV1"] {
            let found = list(&db, &format!("saved_only=true&query={query}")).await;
            assert_eq!(found["total_count"], 1);
            assert_eq!(found["videos"][0]["id"], 1);
        }
        assert_eq!(list(&db, "saved_only=true&query=BV3").await["total_count"], 0);
        let existing = list(&db, "page=0&page_size=20").await;
        assert_eq!(existing["total_count"], 8);
        assert_eq!(existing, list(&db, "saved_only=false&page=0&page_size=20").await);
        assert_eq!(list(&db, "query=BV3").await["total_count"], 1);
        assert!(existing.get("watch_summaries").is_none());
        assert_eq!(list(&db, "saved_only=true&query=Author1").await["total_count"], 1);
        assert_eq!(list(&db, "saved_only=true&sort=title").await["videos"][0]["id"], 1);
        assert_eq!(first["watch_summaries"][0]["page_count"], 2);
        assert_eq!(first["watch_summaries"][0]["duration"], 120);
        assert_eq!(
            list(&db, "saved_only=true&watch_state=unwatched").await["total_count"],
            3
        );
        for (id, position, completed) in [(1, 60000, true), (2, 60000, true), (3, 12000, false)] {
            watch_progress::ActiveModel {
                page_id: Set(id),
                position_ms: Set(position),
                completed: Set(completed),
                revision: Set(1),
                updated_at: Set(chrono::Utc::now().naive_utc()),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        for (state, id) in [("unwatched", 8), ("in_progress", 2), ("watched", 1)] {
            let found = list(&db, &format!("saved_only=true&watch_state={state}&page=0&page_size=1")).await;
            assert_eq!(found["total_count"], 1);
            assert_eq!(found["videos"][0]["id"], id);
            assert_eq!(found["watch_summaries"][0]["watch_state"], state);
        }
        assert_eq!(
            list(&db, "saved_only=true&watch_state=in_progress").await["watch_summaries"][0]["resume_position_ms"],
            12000
        );
        watch_progress::ActiveModel {
            page_id: Set(8),
            position_ms: Set(60000),
            completed: Set(true),
            revision: Set(1),
            updated_at: Set(chrono::Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();
        let partial = list(&db, "saved_only=true&watch_state=in_progress&page=0&page_size=1").await;
        assert_eq!(partial["total_count"], 2);
        assert_eq!(partial["watch_summaries"][0]["resume_page_id"], 9);
        watch_progress::ActiveModel {
            page_id: Set(9),
            position_ms: Set(60000),
            completed: Set(true),
            revision: Set(1),
            updated_at: Set(chrono::Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(list(&db, "saved_only=true&watch_state=watched").await["total_count"], 2);
    }
}

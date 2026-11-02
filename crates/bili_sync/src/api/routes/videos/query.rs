use super::watch;
use crate::api::{
    error::InnerApiError,
    request::{VideosRequest, WatchSort, WatchState},
    response::{PageInfo, VideoInfo, VideoResponse, VideosResponse},
    wrapper::{ApiError, ApiResponse},
};
use crate::{media, platform::Platform, utils::status::PageStatus};
use anyhow::Result;
use axum::extract::{Extension, Path, Query};
use bili_sync_entity::{page, video, watch_progress};
use chrono::NaiveDateTime;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    Select,
    sea_query::{Expr, ExprTrait},
};
use std::collections::HashSet;

/// 列出视频的基本信息，支持根据视频来源筛选、名称查找和分页
pub(super) async fn get_videos(
    Extension(db): Extension<DatabaseConnection>,
    Query(params): Query<VideosRequest>,
) -> Result<ApiResponse<VideosResponse>, ApiError> {
    let mut query = video::Entity::find();
    if let Some(platform) = &params.platform {
        query = query.filter(video::Column::Platform.eq(platform.id()));
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
             WHERE p.id IN (SELECT id FROM playable_page)
             GROUP BY p.video_id HAVING {having})"
        )));
    }
    if params.saved_only == Some(true) {
        query = query.filter(
            video::Column::Id.in_subquery(
                page::Entity::find()
                    .select_only()
                    .column(page::Column::VideoId)
                    .filter(Expr::cust(media::PLAYABLE))
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
                .or(video::Column::PlatformId.contains(&query_word))
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
    let mut videos = query
        .into_partial_model::<VideoInfo>()
        .paginate(&db, page_size)
        .fetch_page(page)
        .await?;
    for video in &mut videos {
        enrich_video(&db, video).await?;
    }
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

pub(super) async fn get_video(
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
    let Some(mut video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    enrich_video(&db, &mut video_info).await?;
    let mut pages_info = pages_info;
    enrich_pages(&db, &video_info.platform, &mut pages_info).await?;
    Ok(ApiResponse::ok(VideoResponse {
        video: video_info,
        watch_progress: watch_progress::Entity::find()
            .filter(watch_progress::Column::PageId.is_in(pages_info.iter().map(|p| p.id)))
            .all(&db)
            .await?,
        playable_page_ids: page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .filter(Expr::cust(media::PLAYABLE))
            .all(&db)
            .await?
            .iter()
            .map(|p| p.id)
            .collect(),
        watch_summary: watch::summaries(&db, &[id]).await?.pop(),
        pages: pages_info,
    }))
}

pub(super) async fn enrich_video(db: &DatabaseConnection, info: &mut VideoInfo) -> Result<(), ApiError> {
    let model = video::Entity::find_by_id(info.id)
        .one(db)
        .await?
        .ok_or(InnerApiError::NotFound(info.id))?;
    info.media = media::video_info(db, &model, Platform::parse(&model.platform)?).await?;
    Ok(())
}

pub(super) async fn enrich_pages(
    db: &DatabaseConnection,
    platform: &str,
    pages: &mut [PageInfo],
) -> Result<(), ApiError> {
    let platform = Platform::parse(platform)?;
    let ready_ids = if let Some(first) = pages.first() {
        page::Entity::find()
            .filter(page::Column::VideoId.eq(first.video_id))
            .filter(Expr::cust(media::PLAYABLE))
            .all(db)
            .await?
            .into_iter()
            .map(|page| page.id)
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    for page in pages {
        page.media = match platform {
            Platform::Bilibili => media::MediaInfo {
                download_state: media::bili_state(page.download_status),
                media_ready: ready_ids.contains(&page.id),
                platform_details: Some(media::PlatformDetails::Bilibili {
                    download_status: PageStatus::from(page.download_status).into(),
                }),
                ..Default::default()
            },
            Platform::Douyin | Platform::Xiaohongshu => {
                media::page_info(db, platform, page.id, page.download_status).await?
            }
        };
    }
    Ok(())
}

pub(super) fn apply_created_at_filter(
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

use std::collections::HashMap;

use bili_sync_entity::{page, watch_progress};
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};

use crate::api::response::WatchSummary;
use crate::utils::status::PageStatus;

pub(super) async fn summaries(db: &DatabaseConnection, video_ids: &[i32]) -> Result<Vec<WatchSummary>, DbErr> {
    if video_ids.is_empty() {
        return Ok(vec![]);
    }
    let pages = page::Entity::find()
        .filter(page::Column::VideoId.is_in(video_ids.iter().copied()))
        .filter(PageStatus::query_builder().subtask_succeeded(1))
        .filter(page::Column::Path.ends_with(".mp4"))
        .order_by_asc(page::Column::Pid)
        .order_by_asc(page::Column::Id)
        .all(db)
        .await?;
    let records = watch_progress::Entity::find()
        .filter(watch_progress::Column::PageId.is_in(pages.iter().map(|p| p.id)))
        .all(db)
        .await?
        .into_iter()
        .map(|w| (w.page_id, w))
        .collect::<HashMap<_, _>>();
    let mut grouped: HashMap<i32, Vec<&page::Model>> = HashMap::new();
    for p in &pages {
        grouped.entry(p.video_id).or_default().push(p);
    }
    Ok(video_ids.iter().filter_map(|id| {
        let parts = grouped.get(id)?;
        let completed = parts.iter().filter(|p| records.get(&p.id).is_some_and(|w| w.completed)).count();
        let started = parts.iter().any(|p| records.get(&p.id).is_some_and(|w| w.completed || w.position_ms > 0));
        let resume = parts.iter()
            .filter_map(|p| records.get(&p.id))
            .filter(|w| !w.completed && w.position_ms > 0)
            .max_by_key(|w| (w.updated_at, w.page_id));
        Some(WatchSummary {
            video_id: *id,
            duration: parts.iter().map(|p| u64::from(p.duration)).sum(),
            page_count: parts.len(),
            watch_state: if completed == parts.len() { "watched" } else if started { "in_progress" } else { "unwatched" },
            resume_page_id: resume.map_or(parts[0].id, |w| w.page_id),
            resume_position_ms: resume.map_or(0, |w| w.position_ms),
        })
    }).collect())
}

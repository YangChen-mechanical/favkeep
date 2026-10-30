use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use bili_sync_entity::{page, watch_progress};
use chrono::Utc;
use sea_orm::{DatabaseConnection, DbBackend, EntityTrait, FromQueryResult, Statement};
use serde::Deserialize;
use serde::Serialize;

use crate::api::wrapper::ApiResponse;

pub(super) fn router() -> Router {
    Router::new()
        .route("/watch-progress/{id}", get(load).put(save))
        .route("/continue-watching", get(continue_watching))
}

#[derive(Serialize, FromQueryResult)]
struct ContinueWatchingItem {
    page_id: i32,
    video_id: i32,
    video_name: String,
    page_name: String,
    upper_name: String,
    pid: i32,
    position_ms: i64,
    duration: i64,
}

async fn continue_watching(
    Extension(db): Extension<DatabaseConnection>,
) -> Result<ApiResponse<Vec<ContinueWatchingItem>>, StatusCode> {
    let statement = Statement::from_string(
        DbBackend::Sqlite,
        "SELECT page_id, video_id, video_name, page_name, upper_name, pid, position_ms, duration
         FROM (SELECT p.id AS page_id, v.id AS video_id, v.name AS video_name,
                p.name AS page_name, v.upper_name, p.pid, w.position_ms, p.duration, w.updated_at,
                ROW_NUMBER() OVER (PARTITION BY v.id ORDER BY w.updated_at DESC, p.id DESC) AS rank
         FROM watch_progress w JOIN page p ON p.id=w.page_id JOIN video v ON v.id=p.video_id
         WHERE w.completed=FALSE AND w.position_ms>0 AND p.path IS NOT NULL
           AND p.id IN (SELECT id FROM playable_page))
         WHERE rank=1 ORDER BY updated_at DESC, page_id DESC LIMIT 12",
    );
    let items = ContinueWatchingItem::find_by_statement(statement)
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(ApiResponse::ok(items))
}

async fn load(
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<i32>,
) -> Result<ApiResponse<Option<watch_progress::Model>>, StatusCode> {
    if page::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let progress = watch_progress::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(ApiResponse::ok(progress))
}

#[derive(Deserialize)]
struct ProgressUpdate {
    position_ms: i64,
    completed: bool,
    revision: i64,
}

async fn save(
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<i32>,
    Json(update): Json<ProgressUpdate>,
) -> Result<ApiResponse<watch_progress::Model>, StatusCode> {
    Ok(ApiResponse::ok(save_progress(&db, id, update).await?))
}

async fn save_progress(
    db: &DatabaseConnection,
    id: i32,
    update: ProgressUpdate,
) -> Result<watch_progress::Model, StatusCode> {
    let page = page::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !crate::media::page_ready(db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let next_revision = update.revision.checked_add(1).ok_or(StatusCode::BAD_REQUEST)?;
    if update.revision < 0 || update.position_ms < 0 || update.position_ms > i64::from(page.duration) * 1000 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let statement = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO watch_progress (page_id, position_ms, completed, revision, updated_at)
         SELECT ?, ?, ?, ?, ? WHERE ? = 0 OR EXISTS (SELECT 1 FROM watch_progress WHERE page_id = ?)
         ON CONFLICT(page_id) DO UPDATE SET position_ms=excluded.position_ms,
           completed=excluded.completed, revision=excluded.revision, updated_at=excluded.updated_at
         WHERE watch_progress.revision = ? RETURNING *",
        [
            id.into(),
            update.position_ms.into(),
            update.completed.into(),
            next_revision.into(),
            Utc::now().naive_utc().into(),
            update.revision.into(),
            id.into(),
            update.revision.into(),
        ],
    );
    watch_progress::Entity::find()
        .from_raw_sql(statement)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::CONFLICT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bili_sync_migration::{Migrator, MigratorTrait};
    use sea_orm::{ActiveModelTrait, Database, Set};

    #[tokio::test]
    async fn progress_is_persistent_validated_and_rejects_stale_writes() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        page::ActiveModel {
            id: Set(1),
            video_id: Set(1),
            cid: Set(Some(1)),
            pid: Set(1),
            name: Set("Sample".into()),
            duration: Set(60),
            download_status: Set(0),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let video = bili_sync_entity::video::Model {
            id: 1,
            name: "Sample video".into(),
            ..Default::default()
        };
        bili_sync_entity::video::ActiveModel::from(video)
            .insert(&db)
            .await
            .unwrap();
        page::ActiveModel {
            id: sea_orm::Unchanged(1),
            path: Set(Some("sample.mp4".into())),
            download_status: Set(crate::utils::status::STATUS_OK << 3),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        let update = |position_ms, revision| ProgressUpdate {
            position_ms,
            completed: false,
            revision,
        };
        let first = save_progress(&db, 1, update(15_000, 0)).await.unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(
            watch_progress::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .position_ms,
            15_000
        );
        let rewind = save_progress(&db, 1, update(5_000, 1)).await.unwrap();
        assert_eq!(rewind.position_ms, 5_000);
        assert_eq!(
            save_progress(&db, 1, update(20_000, 1)).await,
            Err(StatusCode::CONFLICT)
        );
        assert_eq!(
            save_progress(&db, 1, update(61_000, 2)).await,
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(save_progress(&db, 999, update(0, 0)).await, Err(StatusCode::NOT_FOUND));

        let response = continue_watching(Extension(db.clone())).await.unwrap();
        let data = serde_json::to_value(response).unwrap();
        assert_eq!(data["data"].as_array().unwrap().len(), 1);
        assert_eq!(data["data"][0]["page_id"], 1);
        save_progress(
            &db,
            1,
            ProgressUpdate {
                position_ms: 60_000,
                completed: true,
                revision: 2,
            },
        )
        .await
        .unwrap();
        let data = serde_json::to_value(continue_watching(Extension(db)).await.unwrap()).unwrap();
        assert!(data["data"].as_array().unwrap().is_empty());
    }
}

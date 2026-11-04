use favkeep_entity::{page, video};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait, QueryFilter, Statement,
};
use serde::Serialize;

use crate::platform::Platform;

pub const PLAYABLE: &str = "page.id IN (SELECT id FROM playable_page)";

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    #[default]
    Waiting,
    Succeeded,
    Failed,
}

#[derive(Default, Serialize)]
pub struct MediaInfo {
    pub download_state: DownloadState,
    pub media_ready: bool,
    pub source_url: Option<String>,
    pub allowed_actions: Vec<&'static str>,
    pub platform_details: Option<PlatformDetails>,
}

#[derive(Serialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
pub enum PlatformDetails {
    Bilibili { download_status: [u32; 5] },
    Douyin { failures: u32 },
    Xiaohongshu { failures: u32 },
}

pub fn bili_state(status: u32) -> DownloadState {
    let parts: [u32; 5] = crate::utils::status::VideoStatus::from(status).into();
    if parts.iter().any(|s| !matches!(s, 0 | 7)) {
        DownloadState::Failed
    } else if parts.iter().all(|s| *s == 7) {
        DownloadState::Succeeded
    } else {
        DownloadState::Waiting
    }
}

pub async fn page_ready(db: &DatabaseConnection, id: i32) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM playable_page WHERE id=?",
            [id.into()],
        ))
        .await?
        .is_some())
}

pub async fn page_info(db: &DatabaseConnection, platform: Platform, id: i32, status: u32) -> Result<MediaInfo, DbErr> {
    let (state, details) = match platform {
        Platform::Bilibili => (
            bili_state(status),
            PlatformDetails::Bilibili {
                download_status: crate::utils::status::PageStatus::from(status).into(),
            },
        ),
        Platform::Douyin | Platform::Xiaohongshu => {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT state,failures FROM {} WHERE page_id=?",
                        if platform == Platform::Douyin {
                            "douyin_download"
                        } else {
                            "xiaohongshu_download"
                        }
                    ),
                    [id.into()],
                ))
                .await?
                .ok_or_else(|| DbErr::Custom("Missing Douyin download state".into()))?;
            let state: String = row.try_get("", "state")?;
            let state = match state.as_str() {
                "waiting" => DownloadState::Waiting,
                "succeeded" => DownloadState::Succeeded,
                "failed" => DownloadState::Failed,
                _ => return Err(DbErr::Custom("Invalid download state".into())),
            };
            (
                state,
                if platform == Platform::Douyin {
                    PlatformDetails::Douyin {
                        failures: row.try_get("", "failures")?,
                    }
                } else {
                    PlatformDetails::Xiaohongshu {
                        failures: row.try_get("", "failures")?,
                    }
                },
            )
        }
    };
    Ok(MediaInfo {
        download_state: state,
        media_ready: page_ready(db, id).await?,
        platform_details: Some(details),
        ..Default::default()
    })
}

pub async fn video_info(db: &DatabaseConnection, video: &video::Model, platform: Platform) -> Result<MediaInfo, DbErr> {
    let state;
    let ready = page::Entity::find()
        .filter(page::Column::VideoId.eq(video.id))
        .filter(sea_orm::sea_query::Expr::cust(PLAYABLE))
        .one(db)
        .await?
        .is_some();
    let details;
    let mut actions = Vec::new();
    match platform {
        Platform::Bilibili => {
            state = bili_state(video.download_status);
            details = Some(PlatformDetails::Bilibili {
                download_status: crate::utils::status::VideoStatus::from(video.download_status).into(),
            });
            actions.extend(["retry_failed", "force_retry", "status_edit", "clear_redownload"]);
        }
        Platform::Douyin | Platform::Xiaohongshu => {
            let page = page::Entity::find()
                .filter(page::Column::VideoId.eq(video.id))
                .one(db)
                .await?
                .ok_or_else(|| DbErr::Custom("Missing Douyin page".into()))?;
            let info = page_info(db, platform, page.id, page.download_status).await?;
            state = info.download_state;
            details = info.platform_details;
            if if platform == Platform::Douyin {
                crate::douyin::store::can_retry(db, video.id).await?
            } else {
                crate::xiaohongshu::store::can_retry(db, video.id).await?
            } {
                actions.push("retry_failed");
            }
        }
    }
    Ok(MediaInfo {
        download_state: state,
        media_ready: ready,
        source_url: platform.source_url(video.platform_id.as_deref()),
        allowed_actions: actions,
        platform_details: details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use favkeep_migration::{Migrator, MigratorTrait};
    use sea_orm::{ConnectionTrait, Database};

    async fn legacy() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, Some(17)).await.unwrap();
        db
    }

    async fn insert_video(db: &impl ConnectionTrait, id: i32) {
        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO video(id,platform,platform_id,bvid,name,upper_name,upper_face,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download)
             VALUES(?,'douyin',?,NULL,'Saved','Author','','/unchanged',2,'','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,1,1)",
            [id.into(),id.to_string().into()])).await.unwrap();
    }

    #[tokio::test]
    async fn preserves_identity_paths_progress_and_every_retry_state() {
        let db = legacy().await;
        for (id, status) in [(1, 0), (2, 1), (3, 2), (4, 3), (5, 4), (6, 7)] {
            insert_video(&db, id).await;
            db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
                "INSERT INTO page(id,video_id,pid,name,duration,path,download_status) VALUES(?,?,1,'Part',30,'/unchanged/video.mp4',?)",
                [id.into(),id.into(),((7 | (7<<6) | (7<<9) | (7<<12)) | (status<<3)).into()])).await.unwrap();
        }
        db.execute_unprepared("INSERT INTO watch_progress(page_id,position_ms,completed,revision,updated_at) VALUES(6,12000,0,3,CURRENT_TIMESTAMP);
            INSERT INTO douyin_account(identity) VALUES('account');
            INSERT INTO douyin_folder(identity,id,name,selected,baseline_done,baseline_at) VALUES('account','folder','Saved',1,1,123);
            INSERT INTO douyin_folder_item(identity,folder_id,platform_id,kind,qualified) VALUES('account','folder','6','video',1);").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        for (id, state, failures) in [
            (1, "waiting", 0),
            (2, "failed", 1),
            (3, "failed", 2),
            (4, "failed", 3),
            (5, "failed", 4),
            (6, "succeeded", 0),
        ] {
            let row=db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite,
                "SELECT d.state,d.failures,p.download_status,p.path FROM douyin_download d JOIN page p ON p.id=d.page_id WHERE p.id=?",[id.into()])).await.unwrap().unwrap();
            assert_eq!(row.try_get::<String>("", "state").unwrap(), state);
            assert_eq!(row.try_get::<i32>("", "failures").unwrap(), failures);
            assert_eq!(row.try_get::<i64>("", "download_status").unwrap(), 0);
            assert_eq!(row.try_get::<String>("", "path").unwrap(), "/unchanged/video.mp4");
        }
        for sql in [
            "SELECT page_id AS id FROM watch_progress WHERE position_ms=12000 AND revision=3",
            "SELECT baseline_at AS id FROM douyin_folder WHERE baseline_done=1 AND baseline_at=123",
            "SELECT id FROM playable_page WHERE id=6",
        ] {
            assert!(
                db.query_one(Statement::from_string(DbBackend::Sqlite, sql))
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        assert!(
            db.query_all(Statement::from_string(DbBackend::Sqlite, "PRAGMA foreign_key_check"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn missing_or_invalid_page_rolls_back_without_clearing_legacy_state() {
        for status in [None, Some(5), Some(6)] {
            let db = legacy().await;
            insert_video(&db, 1).await;
            if let Some(status) = status {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO page(id,video_id,pid,name,duration,download_status) VALUES(1,1,1,'Part',30,?)",
                    [(status << 3).into()],
                ))
                .await
                .unwrap();
            }
            assert!(Migrator::up(&db, None).await.is_err());
            assert!(
                db.query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='douyin_download'"
                ))
                .await
                .unwrap()
                .is_none()
            );
            if let Some(status) = status {
                let row = db
                    .query_one(Statement::from_string(
                        DbBackend::Sqlite,
                        "SELECT download_status FROM page WHERE id=1",
                    ))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(row.try_get::<i32>("", "download_status").unwrap(), status << 3);
            }
        }
    }
}

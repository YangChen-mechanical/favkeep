use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, QueryResult, Statement, TransactionTrait, Value};
use serde::Serialize;

use super::{
    client::{Folder, Item},
    valid_id,
};

#[derive(Debug, Serialize)]
pub struct Account {
    pub identity: String,
    pub paused_reason: Option<String>,
    pub rejections: u32,
    pub last_run: Option<i64>,
    pub last_finish: Option<i64>,
    pub next_allowed_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StoredFolder {
    pub id: String,
    pub name: String,
    pub total: Option<i64>,
    pub selected: bool,
    pub available: bool,
    pub baseline_done: bool,
    pub baseline_at: Option<i64>,
    pub revision: i64,
    pub last_scan: Option<i64>,
    pub last_result: Option<String>,
    pub item_count: i64,
}

#[derive(Debug)]
pub struct Pending {
    pub id: i32,
    pub platform_id: String,
}

pub enum Outcome {
    Succeeded {
        path: String,
        title: String,
        author: String,
        duration: u32,
        width: Option<u32>,
        height: Option<u32>,
    },
    Failed,
}

pub enum AccountOutcome {
    Success,
    Rejected,
    LoginExpired,
    IdentityUnverified,
    AccountChanged,
    Challenge,
    Network,
    Failed,
    DiskFull,
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn invalid() -> DbErr {
    DbErr::Custom("invalid_douyin_data".into())
}
fn conflict() -> DbErr {
    DbErr::Custom("douyin_selection_changed".into())
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn check_id(id: &str) -> Result<(), DbErr> {
    if valid_id(id) { Ok(()) } else { Err(invalid()) }
}

fn account_row(row: QueryResult) -> Result<Account, DbErr> {
    Ok(Account {
        identity: row.try_get("", "identity")?,
        paused_reason: row.try_get("", "paused_reason")?,
        rejections: row.try_get("", "rejections")?,
        last_run: row.try_get("", "last_run")?,
        last_finish: row.try_get("", "last_finish")?,
        next_allowed_at: row.try_get("", "next_allowed_at")?,
        last_error: row.try_get("", "last_error")?,
    })
}

fn folder_row(row: QueryResult) -> Result<StoredFolder, DbErr> {
    Ok(StoredFolder {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        total: row.try_get("", "total")?,
        selected: row.try_get("", "selected")?,
        available: row.try_get("", "available")?,
        baseline_done: row.try_get("", "baseline_done")?,
        baseline_at: row.try_get("", "baseline_at")?,
        revision: row.try_get("", "revision")?,
        last_scan: row.try_get("", "last_scan")?,
        last_result: row.try_get("", "last_result")?,
        item_count: row.try_get("", "item_count")?,
    })
}

pub async fn ensure_account(db: &DatabaseConnection, identity: &str) -> Result<Account, DbErr> {
    check_id(identity)?;
    let txn = db.begin().await?;
    txn.execute(statement(
        "INSERT INTO douyin_account(identity) VALUES (?) ON CONFLICT DO NOTHING",
        vec![identity.into()],
    ))
    .await?;
    let account = account_row(
        txn.query_one(statement("SELECT * FROM douyin_account", vec![]))
            .await?
            .ok_or_else(invalid)?,
    )?;
    if account.identity != identity {
        return Err(DbErr::Custom("douyin_account_changed".into()));
    }
    txn.commit().await?;
    Ok(account)
}

pub async fn account(db: &DatabaseConnection, identity: &str) -> Result<Account, DbErr> {
    account_row(
        db.query_one(statement(
            "SELECT * FROM douyin_account WHERE identity=?",
            vec![identity.into()],
        ))
        .await?
        .ok_or_else(invalid)?,
    )
}

pub async fn bound_identity(db: &DatabaseConnection) -> Result<Option<String>, DbErr> {
    db.query_one(statement("SELECT identity FROM douyin_account", vec![]))
        .await?
        .map(|row| row.try_get("", "identity"))
        .transpose()
}

pub async fn folders(db: &DatabaseConnection, identity: &str) -> Result<Vec<StoredFolder>, DbErr> {
    db.query_all(statement(
        "SELECT * FROM douyin_folder WHERE identity=? ORDER BY id",
        vec![identity.into()],
    ))
    .await?
    .into_iter()
    .map(folder_row)
    .collect()
}

pub async fn sync_folders(
    db: &DatabaseConnection,
    identity: &str,
    incoming: &[Folder],
) -> Result<Vec<StoredFolder>, DbErr> {
    check_id(identity)?;
    let mut ids = HashSet::new();
    for folder in incoming {
        check_id(&folder.id)?;
        if !ids.insert(folder.id.as_str()) || folder.total.is_some_and(|total| total > i64::MAX as u64) {
            return Err(invalid());
        }
    }
    let txn = db.begin().await?;
    txn.execute(statement(
        "UPDATE douyin_account SET identity=identity WHERE identity=?",
        vec![identity.into()],
    ))
    .await?;
    for row in txn
        .query_all(statement(
            "SELECT id FROM douyin_folder WHERE identity=? AND available=1",
            vec![identity.into()],
        ))
        .await?
    {
        let id: String = row.try_get("", "id")?;
        if !ids.contains(id.as_str()) {
            txn.execute(statement(
                "UPDATE douyin_folder SET available=0,revision=revision+1 WHERE identity=? AND id=?",
                vec![identity.into(), id.into()],
            ))
            .await?;
        }
    }
    for folder in incoming {
        txn.execute(statement("INSERT INTO douyin_folder(identity,id,name,total) VALUES (?,?,?,?) ON CONFLICT(identity,id) DO UPDATE SET name=excluded.name,total=excluded.total,available=1,revision=douyin_folder.revision+(douyin_folder.available=0)", vec![identity.into(),folder.id.clone().into(),folder.name.clone().into(),folder.total.map(|v| v as i64).into()])).await?;
    }
    txn.commit().await?;
    folders(db, identity).await
}

pub async fn set_selected(
    db: &DatabaseConnection,
    identity: &str,
    folder_id: &str,
    selected: bool,
) -> Result<StoredFolder, DbErr> {
    check_id(identity)?;
    check_id(folder_id)?;
    let txn = db.begin().await?;
    let updated = txn.execute(statement("UPDATE douyin_folder SET selected=?,revision=revision+(selected!=?) WHERE identity=? AND id=? AND (available=1 OR ?=0)", vec![selected.into(),selected.into(),identity.into(),folder_id.into(),selected.into()])).await?;
    if updated.rows_affected() != 1 {
        return Err(conflict());
    }
    let folder = folder_row(
        txn.query_one(statement(
            "SELECT * FROM douyin_folder WHERE identity=? AND id=?",
            vec![identity.into(), folder_id.into()],
        ))
        .await?
        .ok_or_else(conflict)?,
    )?;
    txn.commit().await?;
    Ok(folder)
}

pub async fn commit_scan(
    db: &DatabaseConnection,
    identity: &str,
    folder_id: &str,
    revision: i64,
    items: &[Item],
) -> Result<usize, DbErr> {
    check_id(identity)?;
    check_id(folder_id)?;
    let mut ids = HashSet::new();
    for item in items {
        check_id(&item.id)?;
        if !ids.insert(item.id.as_str()) || !matches!(item.kind.as_str(), "video" | "unsupported") {
            return Err(invalid());
        }
    }
    let txn = db.begin().await?;
    let updated = txn.execute(statement("UPDATE douyin_folder SET last_scan=? WHERE identity=? AND id=? AND revision=? AND selected=1 AND available=1 AND EXISTS(SELECT 1 FROM douyin_account a WHERE a.identity=douyin_folder.identity AND a.paused_reason IS NULL)", vec![now().into(),identity.into(),folder_id.into(),revision.into()])).await?;
    if updated.rows_affected() != 1 {
        return Err(conflict());
    }
    let folder = folder_row(
        txn.query_one(statement(
            "SELECT * FROM douyin_folder WHERE identity=? AND id=?",
            vec![identity.into(), folder_id.into()],
        ))
        .await?
        .ok_or_else(conflict)?,
    )?;
    if folder.total.is_some_and(|total| total != items.len() as i64) {
        return Err(invalid());
    }
    txn.execute(statement(
        "UPDATE douyin_folder_item SET present=0 WHERE identity=? AND folder_id=?",
        vec![identity.into(), folder_id.into()],
    ))
    .await?;
    let mut inserted = 0;
    for item in items {
        let seen = txn
            .query_one(statement(
                "SELECT 1 FROM douyin_folder_item WHERE identity=? AND folder_id=? AND platform_id=?",
                vec![identity.into(), folder_id.into(), item.id.clone().into()],
            ))
            .await?
            .is_some();
        let qualified = folder.baseline_done && !seen && item.kind == "video";
        if qualified {
            let video = txn.execute(statement("INSERT INTO video(platform,platform_id,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download,single_page) VALUES ('douyin',?,'','',?,'',0,'','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,1,1,1) ON CONFLICT(platform,platform_id) WHERE platform='douyin' DO NOTHING", vec![item.id.clone().into(),item.id.clone().into()])).await?;
            if video.rows_affected() == 1 {
                txn.execute(statement(
                    "INSERT INTO page(video_id,pid,name,duration,download_status) VALUES (?,1,?,0,?)",
                    vec![
                        (video.last_insert_id() as i64).into(),
                        item.id.clone().into(),
                        i64::from(initial_page_status()).into(),
                    ],
                ))
                .await?;
                inserted += 1;
            }
        }
        txn.execute(statement("INSERT INTO douyin_folder_item(identity,folder_id,platform_id,kind,qualified) VALUES (?,?,?,?,?) ON CONFLICT(identity,folder_id,platform_id) DO UPDATE SET present=1,kind=excluded.kind", vec![identity.into(),folder_id.into(),item.id.clone().into(),item.kind.clone().into(),qualified.into()])).await?;
    }
    txn.execute(statement("UPDATE douyin_folder SET baseline_done=1,baseline_at=coalesce(baseline_at,?),last_result='complete',item_count=? WHERE identity=? AND id=?", vec![now().into(),(items.len() as i64).into(),identity.into(),folder_id.into()])).await?;
    txn.commit().await?;
    Ok(inserted)
}

fn initial_page_status() -> u32 {
    7 | (7 << 6) | (7 << 9) | (7 << 12)
}

pub async fn folder_error(db: &DatabaseConnection, identity: &str, folder_id: &str, error: &str) -> Result<(), DbErr> {
    check_id(identity)?;
    check_id(folder_id)?;
    if !matches!(
        error,
        "invalid_platform_data"
            | "incomplete_pagination"
            | "platform_rejected"
            | "login_expired"
            | "verification_required"
            | "identity_unverified"
            | "account_changed"
            | "browser_failed"
            | "network_failed"
            | "private_file_failed"
            | "media_unavailable"
            | "browser_token_unavailable"
            | "disk_full"
            | "selection_changed_during_scan"
    ) {
        return Err(invalid());
    }
    db.execute(statement(
        "UPDATE douyin_folder SET last_scan=?,last_result=? WHERE identity=? AND id=?",
        vec![now().into(), error.into(), identity.into(), folder_id.into()],
    ))
    .await?;
    Ok(())
}

const ELIGIBLE_SQL: &str = "SELECT DISTINCT v.id,v.platform_id FROM video v JOIN page p ON p.video_id=v.id JOIN douyin_folder_item i ON i.platform_id=v.platform_id JOIN douyin_folder f ON f.identity=i.identity AND f.id=i.folder_id JOIN douyin_account a ON a.identity=f.identity WHERE v.platform='douyin' AND v.valid=1 AND v.should_download=1 AND p.pid=1 AND i.kind='video' AND i.present=1 AND i.qualified=1 AND f.identity=? AND f.selected=1 AND f.available=1 AND a.paused_reason IS NULL";

pub async fn pending(db: &DatabaseConnection, identity: &str) -> Result<Vec<Pending>, DbErr> {
    db.query_all(statement(
        &format!("{ELIGIBLE_SQL} AND ((p.download_status>>3)&7)<4 ORDER BY v.id"),
        vec![identity.into()],
    ))
    .await?
    .into_iter()
    .map(|row| {
        Ok(Pending {
            id: row.try_get("", "id")?,
            platform_id: row.try_get("", "platform_id")?,
        })
    })
    .collect()
}

pub async fn is_pending(db: &DatabaseConnection, identity: &str, video_id: i32) -> Result<bool, DbErr> {
    Ok(db
        .query_one(statement(
            &format!("{ELIGIBLE_SQL} AND ((p.download_status>>3)&7)<4 AND v.id=? LIMIT 1"),
            vec![identity.into(), video_id.into()],
        ))
        .await?
        .is_some())
}

pub async fn retry_failed(db: &DatabaseConnection, identity: &str) -> Result<usize, DbErr> {
    check_id(identity)?;
    let txn = db.begin().await?;
    let account=txn.execute(statement("UPDATE douyin_account SET identity=identity WHERE identity=? AND paused_reason IS NULL AND (next_allowed_at IS NULL OR next_allowed_at<=?)",vec![identity.into(),now().into()])).await?;
    if account.rows_affected() != 1 {
        return Err(DbErr::Custom("douyin_account_blocked".into()));
    }
    let rows = txn
        .query_all(statement(
            &format!("{ELIGIBLE_SQL} AND ((p.download_status>>3)&7) BETWEEN 1 AND 4"),
            vec![identity.into()],
        ))
        .await?;
    let page_mask = i64::from(!((1u32 << 31) | (7 << 3)));
    let video_mask = i64::from(!((1u32 << 31) | (7 << 12)));
    let mut count = 0;
    for row in rows {
        let id: i32 = row.try_get("", "id")?;
        count += txn
            .execute(statement(
                "UPDATE page SET download_status=download_status&? WHERE video_id=? AND pid=1",
                vec![page_mask.into(), id.into()],
            ))
            .await?
            .rows_affected() as usize;
        txn.execute(statement(
            "UPDATE video SET download_status=download_status&? WHERE id=?",
            vec![video_mask.into(), id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    Ok(count)
}

pub async fn mark_outcome(db: &DatabaseConnection, video_id: i32, outcome: Outcome) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let updated = txn
        .execute(statement(
            "UPDATE video SET id=id WHERE id=? AND platform='douyin'",
            vec![video_id.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(invalid());
    }
    let row = txn
        .query_one(statement(
            "SELECT p.id,p.download_status FROM page p WHERE p.video_id=? AND p.pid=1",
            vec![video_id.into()],
        ))
        .await?
        .ok_or_else(invalid)?;
    let page_id: i32 = row.try_get("", "id")?;
    let old_status: u32 = row.try_get("", "download_status")?;
    let state = (old_status >> 3) & 7;
    if state >= 4 {
        txn.commit().await?;
        return Ok(());
    }
    match outcome {
        Outcome::Succeeded {
            path,
            title,
            author,
            duration,
            width,
            height,
        } => {
            let file = std::path::Path::new(&path);
            if !file.is_absolute() {
                return Err(invalid());
            }
            let directory = file
                .parent()
                .and_then(std::path::Path::to_str)
                .ok_or_else(invalid)?
                .to_owned();
            let complete = i64::from((1u32 << 31) | 0x7fff);
            txn.execute(statement(
                "UPDATE page SET path=?,name=coalesce(nullif(?,''),name),duration=?,width=?,height=?,download_status=? WHERE id=?",
                vec![
                    path.clone().into(),
                    title.clone().into(),
                    duration.into(),
                    width.into(),
                    height.into(),
                    complete.into(),
                    page_id.into(),
                ],
            ))
            .await?;
            txn.execute(statement(
                "UPDATE video SET path=?,name=coalesce(nullif(?,''),name),upper_name=coalesce(nullif(?,''),upper_name),download_status=? WHERE id=?",
                vec![
                    directory.into(),
                    title.into(),
                    author.into(),
                    complete.into(),
                    video_id.into(),
                ],
            ))
            .await?;
        }
        Outcome::Failed => {
            let failures = state + 1;
            let status = initial_page_status() | (failures << 3) | if failures >= 4 { 1u32 << 31 } else { 0 };
            txn.execute(statement(
                "UPDATE page SET download_status=? WHERE id=?",
                vec![i64::from(status).into(), page_id.into()],
            ))
            .await?;
            let video_status = if failures >= 4 {
                (1u32 << 31) | 0xfff | (4 << 12)
            } else {
                0
            };
            txn.execute(statement(
                "UPDATE video SET download_status=? WHERE id=?",
                vec![i64::from(video_status).into(), video_id.into()],
            ))
            .await?;
        }
    }
    txn.commit().await
}

pub async fn begin_run(db: &DatabaseConnection, identity: &str) -> Result<bool, DbErr> {
    Ok(db.execute(statement("UPDATE douyin_account SET last_run=? WHERE identity=? AND paused_reason IS NULL AND (next_allowed_at IS NULL OR next_allowed_at<=?)", vec![now().into(),identity.into(),now().into()])).await?.rows_affected()==1)
}

pub async fn account_outcome(
    db: &DatabaseConnection,
    identity: &str,
    outcome: AccountOutcome,
    interval_seconds: u64,
) -> Result<Account, DbErr> {
    let txn = db.begin().await?;
    txn.execute(statement(
        "UPDATE douyin_account SET last_finish=? WHERE identity=?",
        vec![now().into(), identity.into()],
    ))
    .await?;
    let (error, pause) = match outcome {
        AccountOutcome::Success => {
            txn.execute(statement(
                "UPDATE douyin_account SET rejections=0,next_allowed_at=NULL,last_error=NULL WHERE identity=?",
                vec![identity.into()],
            ))
            .await?;
            (None, None)
        }
        AccountOutcome::Rejected => {
            let next = now().saturating_add(i64::try_from(interval_seconds).unwrap_or(i64::MAX));
            txn.execute(statement("UPDATE douyin_account SET rejections=min(rejections+1,3),next_allowed_at=?,last_error='rejected',paused_reason=CASE WHEN rejections+1>=3 THEN 'rejected' ELSE paused_reason END WHERE identity=?", vec![next.into(),identity.into()])).await?;
            (None, None)
        }
        AccountOutcome::LoginExpired => (Some("login_expired"), Some("login_expired")),
        AccountOutcome::IdentityUnverified => (Some("identity_unverified"), Some("identity_unverified")),
        AccountOutcome::AccountChanged => (Some("account_changed"), Some("account_changed")),
        AccountOutcome::Challenge => (Some("challenge"), Some("challenge")),
        AccountOutcome::DiskFull => (Some("disk_full"), Some("disk_full")),
        AccountOutcome::Network => (Some("network"), None),
        AccountOutcome::Failed => (Some("failed"), None),
    };
    if let Some(error) = error {
        txn.execute(statement(
            "UPDATE douyin_account SET last_error=?,paused_reason=coalesce(?,paused_reason) WHERE identity=?",
            vec![error.into(), pause.map(str::to_owned).into(), identity.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    account(db, identity).await
}

pub async fn resume_account(db: &DatabaseConnection, identity: &str) -> Result<Account, DbErr> {
    db.execute(statement("UPDATE douyin_account SET rejections=0,paused_reason=NULL,next_allowed_at=NULL,last_error=NULL WHERE identity=?", vec![identity.into()])).await?;
    account(db, identity).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bili_sync_migration::{Migrator, MigratorTrait};
    use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};

    async fn database() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        ensure_account(&db, "self_123").await.unwrap();
        sync_folders(
            &db,
            "self_123",
            &[
                Folder {
                    id: "f1".into(),
                    name: "First".into(),
                    total: None,
                },
                Folder {
                    id: "f2".into(),
                    name: "Second".into(),
                    total: None,
                },
            ],
        )
        .await
        .unwrap();
        db
    }

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            kind: "video".into(),
        }
    }

    async fn saved_page(db: &DatabaseConnection, video_id: i32) -> bili_sync_entity::page::Model {
        bili_sync_entity::page::Entity::find()
            .filter(bili_sync_entity::page::Column::VideoId.eq(video_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn migration_preserves_old_ids_paths_watch_progress_and_bili_uniqueness() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, Some(16)).await.unwrap();
        db.execute_unprepared("INSERT INTO video(id,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) VALUES (10,1,42,'Author','','Old','old/video',0,'BVold','','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,7,1,1);
            INSERT INTO page(id,video_id,cid,pid,name,duration,path,download_status) VALUES (20,10,99,1,'Part',60,'old/file.mp4',56);
            INSERT INTO watch_progress(page_id,position_ms,revision) VALUES (20,12000,3);").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let old = bili_sync_entity::video::Entity::find_by_id(10)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(old.platform, "bilibili");
        assert_eq!(old.platform_id.as_deref(), Some("BVold"));
        assert_eq!(old.bvid.as_deref(), Some("BVold"));
        assert_eq!(old.upper_id, Some(42));
        assert_eq!(old.path, "old/video");
        let page = bili_sync_entity::page::Entity::find_by_id(20)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(page.cid, Some(99));
        assert_eq!(page.path.as_deref(), Some("old/file.mp4"));
        assert_eq!(page.download_status, 56);
        let watch = db
            .query_one(statement(
                "SELECT page_id,position_ms,revision FROM watch_progress",
                vec![],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(watch.try_get::<i32>("", "page_id").unwrap(), 20);
        assert_eq!(watch.try_get::<i64>("", "position_ms").unwrap(), 12000);
        assert_eq!(watch.try_get::<i64>("", "revision").unwrap(), 3);
        let bili_copy = "INSERT INTO video(id,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) SELECT 11,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download FROM video WHERE id=10";
        assert!(db.execute_unprepared(bili_copy).await.is_err());
        db.execute_unprepared(&bili_copy.replace("SELECT 11,favorite_id", "SELECT 12,2"))
            .await
            .unwrap();
        let duplicate = "INSERT INTO video(platform,platform_id,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) VALUES ('douyin','dy_1','','','New','',0,'','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,1,1)";
        db.execute_unprepared(duplicate).await.unwrap();
        assert!(db.execute_unprepared(duplicate).await.is_err());
        let dy = db
            .query_one(statement(
                "SELECT bvid,upper_id FROM video WHERE platform='douyin'",
                vec![],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dy.try_get::<Option<String>>("", "bvid").unwrap(), None);
        assert_eq!(dy.try_get::<Option<i64>>("", "upper_id").unwrap(), None);
        assert!(
            db.query_all(statement("PRAGMA foreign_key_check", vec![]))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn disabling_a_qualifying_folder_cannot_download_via_another_folders_baseline() {
        let db = database().await;
        let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
        let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
        commit_scan(&db, "self_123", "f1", first.revision, &[]).await.unwrap();
        commit_scan(&db, "self_123", "f2", second.revision, &[item("shared")])
            .await
            .unwrap();
        commit_scan(&db, "self_123", "f1", first.revision, &[item("shared")])
            .await
            .unwrap();
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
        set_selected(&db, "self_123", "f1", false).await.unwrap();
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        assert_eq!(retry_failed(&db, "self_123").await.unwrap(), 0);
        set_selected(&db, "self_123", "f1", true).await.unwrap();
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn baseline_diffs_folder_history_and_selection_are_persistent() {
        let db = database().await;
        let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
        assert_eq!(
            commit_scan(&db, "self_123", "f1", first.revision, &[item("old")])
                .await
                .unwrap(),
            0
        );
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        assert_eq!(
            commit_scan(&db, "self_123", "f1", first.revision, &[item("old"), item("new")])
                .await
                .unwrap(),
            1
        );
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
        let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
        assert_eq!(
            commit_scan(
                &db,
                "self_123",
                "f2",
                second.revision,
                &[item("new"), item("historical")]
            )
            .await
            .unwrap(),
            0
        );
        commit_scan(&db, "self_123", "f1", first.revision, &[item("old")])
            .await
            .unwrap();
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        assert_eq!(
            commit_scan(&db, "self_123", "f1", first.revision, &[item("old"), item("new")])
                .await
                .unwrap(),
            0
        );
        let queued = pending(&db, "self_123").await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].platform_id, "new");
        assert!(is_pending(&db, "self_123", queued[0].id).await.unwrap());
        set_selected(&db, "self_123", "f1", false).await.unwrap();
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        assert!(!is_pending(&db, "self_123", queued[0].id).await.unwrap());
        let again = set_selected(&db, "self_123", "f1", true).await.unwrap();
        assert!(again.baseline_done);
        assert!(again.baseline_at.is_some());
        assert_eq!(again.item_count, 2);
        assert_eq!(again.last_result.as_deref(), Some("complete"));
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
        assert!(
            commit_scan(&db, "self_123", "f1", first.revision, &[item("should_not_exist")])
                .await
                .is_err()
        );
        assert_eq!(folders(&db, "self_123").await.unwrap()[0].item_count, 2);
        assert!(
            db.query_one(statement(
                "SELECT 1 FROM douyin_folder_item WHERE platform_id='should_not_exist'",
                vec![]
            ))
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn another_folder_baseline_does_not_suppress_a_later_new_collection() {
        let db = database().await;
        let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
        let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
        commit_scan(&db, "self_123", "f1", first.revision, &[]).await.unwrap();
        commit_scan(&db, "self_123", "f2", second.revision, &[item("historical_in_second")])
            .await
            .unwrap();
        assert_eq!(
            commit_scan(&db, "self_123", "f1", first.revision, &[item("historical_in_second")])
                .await
                .unwrap(),
            1
        );
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
        assert_eq!(
            commit_scan(
                &db,
                "self_123",
                "f1",
                first.revision,
                &[item("historical_in_second"), item("new_in_both")]
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            commit_scan(
                &db,
                "self_123",
                "f2",
                second.revision,
                &[item("historical_in_second"), item("new_in_both")]
            )
            .await
            .unwrap(),
            0
        );
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn only_complete_valid_scans_commit_and_disappeared_folders_stop_pending() {
        let db = database().await;
        let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
        let bad = Item {
            id: "bad".into(),
            kind: "unknown".into(),
        };
        assert!(
            commit_scan(&db, "self_123", "f1", folder.revision, &[item("x"), bad])
                .await
                .is_err()
        );
        assert!(!folders(&db, "self_123").await.unwrap()[0].baseline_done);
        folder_error(&db, "self_123", "f1", "incomplete_pagination")
            .await
            .unwrap();
        assert!(!folders(&db, "self_123").await.unwrap()[0].baseline_done);
        assert!(folder_error(&db, "self_123", "f1", "secret signed URL").await.is_err());
        assert_eq!(
            folders(&db, "self_123").await.unwrap()[0].last_result.as_deref(),
            Some("incomplete_pagination")
        );
        commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.unwrap();
        let unsupported = Item {
            id: "image".into(),
            kind: "unsupported".into(),
        };
        assert_eq!(
            commit_scan(&db, "self_123", "f1", folder.revision, &[unsupported, item("x")])
                .await
                .unwrap(),
            1
        );
        sync_folders(&db, "self_123", &[]).await.unwrap();
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        assert!(commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.is_err());
        let hidden = folders(&db, "self_123").await.unwrap();
        assert!(!hidden[0].available);
        assert!(hidden[0].baseline_done);
        assert!(!set_selected(&db, "self_123", "f1", false).await.unwrap().selected);
    }

    #[tokio::test]
    async fn media_success_and_four_failures_use_existing_status_bits() {
        let db = database().await;
        let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
        commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.unwrap();
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("good"), item("bad")])
            .await
            .unwrap();
        let queued = pending(&db, "self_123").await.unwrap();
        let good = queued.iter().find(|p| p.platform_id == "good").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("good.mp4");
        tokio::fs::write(&output, b"saved content").await.unwrap();
        let output = output.to_str().unwrap().to_owned();
        mark_outcome(
            &db,
            good.id,
            Outcome::Succeeded {
                path: output.clone(),
                title: "Title".into(),
                author: "Author".into(),
                duration: 60,
                width: Some(1920),
                height: Some(1080),
            },
        )
        .await
        .unwrap();
        let saved = saved_page(&db, good.id).await;
        assert_eq!((saved.download_status >> 3) & 7, 7);
        assert_eq!(saved.path.as_deref(), Some(output.as_str()));
        assert!(!is_pending(&db, "self_123", good.id).await.unwrap());
        let video = bili_sync_entity::video::Entity::find_by_id(good.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::path::Path::new(&video.path), directory.path());
        assert!(std::path::Path::new(&output).starts_with(&video.path));
        assert!(tokio::fs::canonicalize(&video.path).await.unwrap().is_dir());
        assert!(tokio::fs::canonicalize(&output).await.unwrap().is_file());
        db.execute(statement(
            "UPDATE page SET download_status=? WHERE video_id=?",
            vec![i64::from(initial_page_status()).into(), good.id.into()],
        ))
        .await
        .unwrap();
        mark_outcome(
            &db,
            good.id,
            Outcome::Succeeded {
                path: output.clone(),
                title: String::new(),
                author: String::new(),
                duration: 60,
                width: Some(1920),
                height: Some(1080),
            },
        )
        .await
        .unwrap();
        let recovered = bili_sync_entity::video::Entity::find_by_id(good.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (recovered.name.as_str(), recovered.upper_name.as_str()),
            ("Title", "Author")
        );
        assert_eq!(saved_page(&db, good.id).await.name, "Title");
        let bad = queued.iter().find(|p| p.platform_id == "bad").unwrap();
        for _ in 0..3 {
            mark_outcome(&db, bad.id, Outcome::Failed).await.unwrap();
            assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
        }
        mark_outcome(&db, bad.id, Outcome::Failed).await.unwrap();
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
        let failed = saved_page(&db, bad.id).await;
        assert_eq!((failed.download_status >> 3) & 7, 4);
        assert_eq!(failed.download_status >> 31, 1);
        mark_outcome(&db, good.id, Outcome::Failed).await.unwrap();
        assert_eq!(saved_page(&db, good.id).await.download_status, saved.download_status);
    }

    #[tokio::test]
    async fn account_binding_rejects_switching_without_changing_existing_state() {
        let empty = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&empty, None).await.unwrap();
        assert_eq!(bound_identity(&empty).await.unwrap(), None);
        let db = database().await;
        assert_eq!(bound_identity(&db).await.unwrap().as_deref(), Some("self_123"));
        account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        assert_eq!(
            ensure_account(&db, "other_account").await.unwrap_err().to_string(),
            "Custom Error: douyin_account_changed"
        );
        assert_eq!(ensure_account(&db, "self_123").await.unwrap().rejections, 1);
        assert_eq!(folders(&db, "self_123").await.unwrap().len(), 2);
        assert_eq!(
            db.query_all(statement("SELECT identity FROM douyin_account", vec![]))
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            db.execute(statement(
                "INSERT INTO douyin_account(identity) VALUES ('other_account')",
                vec![]
            ))
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn manual_retry_resets_only_eligible_failed_content_and_honors_account_blocks() {
        let db = database().await;
        let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
        let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
        commit_scan(&db, "self_123", "f1", first.revision, &[item("historical")])
            .await
            .unwrap();
        commit_scan(&db, "self_123", "f2", second.revision, &[]).await.unwrap();
        commit_scan(
            &db,
            "self_123",
            "f1",
            first.revision,
            &[item("historical"), item("bad"), item("partial"), item("good")],
        )
        .await
        .unwrap();
        commit_scan(&db, "self_123", "f2", second.revision, &[item("disabled")])
            .await
            .unwrap();
        let queued = pending(&db, "self_123").await.unwrap();
        let id = |name: &str| queued.iter().find(|p| p.platform_id == name).unwrap().id;
        for _ in 0..4 {
            mark_outcome(&db, id("bad"), Outcome::Failed).await.unwrap();
            mark_outcome(&db, id("disabled"), Outcome::Failed).await.unwrap();
        }
        mark_outcome(&db, id("partial"), Outcome::Failed).await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        mark_outcome(
            &db,
            id("good"),
            Outcome::Succeeded {
                path: directory.path().join("good.mp4").to_str().unwrap().into(),
                title: "Good".into(),
                author: "Author".into(),
                duration: 1,
                width: None,
                height: None,
            },
        )
        .await
        .unwrap();
        set_selected(&db, "self_123", "f2", false).await.unwrap();
        assert!(!is_pending(&db, "self_123", id("bad")).await.unwrap());
        let baseline = serde_json::to_value(folders(&db, "self_123").await.unwrap()).unwrap();
        let good_before = saved_page(&db, id("good")).await;
        assert_eq!(retry_failed(&db, "self_123").await.unwrap(), 2);
        assert!(is_pending(&db, "self_123", id("bad")).await.unwrap());
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 2);
        assert_eq!((saved_page(&db, id("bad")).await.download_status >> 3) & 7, 0);
        assert_eq!((saved_page(&db, id("partial")).await.download_status >> 3) & 7, 0);
        assert_eq!((saved_page(&db, id("disabled")).await.download_status >> 3) & 7, 4);
        assert_eq!(saved_page(&db, id("good")).await, good_before);
        let parent = bili_sync_entity::video::Entity::find_by_id(id("bad"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parent.download_status, 0xfff);
        assert_eq!(
            serde_json::to_value(folders(&db, "self_123").await.unwrap()).unwrap(),
            baseline
        );
        assert!(
            db.query_one(statement(
                "SELECT 1 FROM video WHERE platform='douyin' AND platform_id='historical'",
                vec![]
            ))
            .await
            .unwrap()
            .is_none()
        );
        mark_outcome(&db, id("bad"), Outcome::Failed).await.unwrap();
        account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        assert!(
            matches!(retry_failed(&db,"self_123").await,Err(DbErr::Custom(message)) if message=="douyin_account_blocked")
        );
        assert_eq!((saved_page(&db, id("bad")).await.download_status >> 3) & 7, 1);
        resume_account(&db, "self_123").await.unwrap();
        account_outcome(&db, "self_123", AccountOutcome::DiskFull, 3600)
            .await
            .unwrap();
        assert!(retry_failed(&db, "self_123").await.is_err());
        assert_eq!((saved_page(&db, id("bad")).await.download_status >> 3) & 7, 1);
    }

    #[tokio::test]
    async fn known_counts_and_duplicate_ids_cannot_commit_partial_baselines() {
        let db = database().await;
        sync_folders(
            &db,
            "self_123",
            &[Folder {
                id: "f1".into(),
                name: "First".into(),
                total: Some(2),
            }],
        )
        .await
        .unwrap();
        let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
        assert!(
            commit_scan(&db, "self_123", "f1", folder.revision, &[item("one")])
                .await
                .is_err()
        );
        assert!(
            commit_scan(&db, "self_123", "f1", folder.revision, &[item("one"), item("one")])
                .await
                .is_err()
        );
        let untouched = folders(&db, "self_123").await.unwrap();
        assert!(!untouched[0].baseline_done);
        assert_eq!(untouched[0].last_scan, None);
        assert_eq!(
            commit_scan(&db, "self_123", "f1", folder.revision, &[item("one"), item("two")])
                .await
                .unwrap(),
            0
        );
        assert!(pending(&db, "self_123").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejection_streak_cooldown_and_hard_pauses_survive_database_reads() {
        let db = database().await;
        assert!(begin_run(&db, "self_123").await.unwrap());
        let one = account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        assert_eq!(one.rejections, 1);
        assert!(!begin_run(&db, "self_123").await.unwrap());
        account_outcome(&db, "self_123", AccountOutcome::Network, 3600)
            .await
            .unwrap();
        assert_eq!(account(&db, "self_123").await.unwrap().rejections, 1);
        account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        let three = account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        assert_eq!(three.paused_reason.as_deref(), Some("rejected"));
        assert_eq!(three.rejections, 3);
        resume_account(&db, "self_123").await.unwrap();
        assert!(begin_run(&db, "self_123").await.unwrap());
        account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
            .await
            .unwrap();
        let reset = account_outcome(&db, "self_123", AccountOutcome::Success, 3600)
            .await
            .unwrap();
        assert_eq!(reset.rejections, 0);
        assert_eq!(reset.next_allowed_at, None);
        account_outcome(&db, "self_123", AccountOutcome::DiskFull, 3600)
            .await
            .unwrap();
        assert_eq!(
            account(&db, "self_123").await.unwrap().paused_reason.as_deref(),
            Some("disk_full")
        );
        assert!(!begin_run(&db, "self_123").await.unwrap());
    }
}

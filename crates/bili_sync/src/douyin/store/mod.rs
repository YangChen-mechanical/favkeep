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

#[derive(Debug, Default)]
pub struct ScanSummary {
    pub baseline: bool,
    pub scanned: usize,
    pub historical: usize,
    pub existing: usize,
    pub unsupported: usize,
    pub inserted: usize,
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
) -> Result<ScanSummary, DbErr> {
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
    let mut summary = ScanSummary {
        baseline: !folder.baseline_done,
        scanned: items.len(),
        ..Default::default()
    };
    for item in items {
        let seen = txn
            .query_one(statement(
                "SELECT qualified FROM douyin_folder_item WHERE identity=? AND folder_id=? AND platform_id=?",
                vec![identity.into(), folder_id.into(), item.id.clone().into()],
            ))
            .await?;
        let qualified = folder.baseline_done && seen.is_none() && item.kind == "video";
        let historical = !folder.baseline_done
            || seen
                .as_ref()
                .map(|row| row.try_get::<bool>("", "qualified"))
                .transpose()?
                .is_some_and(|q| !q);
        let mut inserted = false;
        if qualified {
            let video = txn.execute(statement("INSERT INTO video(platform,platform_id,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download,single_page) VALUES ('douyin',?,'','',?,'',0,'','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,1,1,1) ON CONFLICT(platform,platform_id) WHERE platform='douyin' DO NOTHING", vec![item.id.clone().into(),item.id.clone().into()])).await?;
            if video.rows_affected() == 1 {
                txn.execute(statement(
                    "INSERT INTO page(video_id,pid,name,duration,download_status) VALUES (?,1,?,0,?)",
                    vec![
                        (video.last_insert_id() as i64).into(),
                        item.id.clone().into(),
                        0i64.into(),
                    ],
                ))
                .await?;
                txn.execute(statement("INSERT INTO douyin_download(page_id,state,failures) SELECT id,'waiting',0 FROM page WHERE video_id=?", vec![(video.last_insert_id() as i64).into()])).await?;
                summary.inserted += 1;
                inserted = true;
            }
        }
        if item.kind != "video" {
            summary.unsupported += 1;
        } else if historical {
            summary.historical += 1;
        } else if !inserted {
            summary.existing += 1;
        }
        txn.execute(statement("INSERT INTO douyin_folder_item(identity,folder_id,platform_id,kind,qualified) VALUES (?,?,?,?,?) ON CONFLICT(identity,folder_id,platform_id) DO UPDATE SET present=1,kind=excluded.kind", vec![identity.into(),folder_id.into(),item.id.clone().into(),item.kind.clone().into(),qualified.into()])).await?;
    }
    txn.execute(statement("UPDATE douyin_folder SET baseline_done=1,baseline_at=coalesce(baseline_at,?),last_result='complete',item_count=? WHERE identity=? AND id=?", vec![now().into(),(items.len() as i64).into(),identity.into(),folder_id.into()])).await?;
    txn.commit().await?;
    Ok(summary)
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

const ELIGIBLE_SQL: &str = "SELECT DISTINCT v.id,v.platform_id FROM video v JOIN page p ON p.video_id=v.id JOIN douyin_download d ON d.page_id=p.id JOIN douyin_folder_item i ON i.platform_id=v.platform_id JOIN douyin_folder f ON f.identity=i.identity AND f.id=i.folder_id JOIN douyin_account a ON a.identity=f.identity WHERE v.platform='douyin' AND v.valid=1 AND v.should_download=1 AND p.pid=1 AND i.kind='video' AND i.present=1 AND i.qualified=1 AND f.identity=? AND f.selected=1 AND f.available=1 AND a.paused_reason IS NULL AND (a.next_allowed_at IS NULL OR a.next_allowed_at<=CAST(strftime('%s','now') AS INTEGER))";

pub async fn pending(db: &DatabaseConnection, identity: &str) -> Result<Vec<Pending>, DbErr> {
    db.query_all(statement(
        &format!("{ELIGIBLE_SQL} AND d.state!='succeeded' AND d.failures<4 ORDER BY v.id"),
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
            &format!("{ELIGIBLE_SQL} AND d.state!='succeeded' AND d.failures<4 AND v.id=? LIMIT 1"),
            vec![identity.into(), video_id.into()],
        ))
        .await?
        .is_some())
}

pub async fn can_retry(db: &DatabaseConnection, video_id: i32) -> Result<bool, DbErr> {
    let Some(identity) = bound_identity(db).await? else {
        return Ok(false);
    };
    Ok(db
        .query_one(statement(
            &format!("{ELIGIBLE_SQL} AND d.state='failed' AND v.id=?"),
            vec![identity.into(), video_id.into()],
        ))
        .await?
        .is_some())
}

pub async fn retry_failed(db: &DatabaseConnection, identity: &str, video_id: Option<i32>) -> Result<usize, DbErr> {
    check_id(identity)?;
    let txn = db.begin().await?;
    let account=txn.execute(statement("UPDATE douyin_account SET identity=identity WHERE identity=? AND paused_reason IS NULL AND (next_allowed_at IS NULL OR next_allowed_at<=?)",vec![identity.into(),now().into()])).await?;
    if account.rows_affected() != 1 {
        return Err(DbErr::Custom("douyin_account_blocked".into()));
    }
    let mut sql = format!("{ELIGIBLE_SQL} AND d.state='failed'");
    let mut values = vec![identity.into()];
    if let Some(id) = video_id {
        sql.push_str(" AND v.id=?");
        values.push(id.into());
    }
    let rows = txn.query_all(statement(&sql, values)).await?;
    let mut count = 0;
    for row in rows {
        let id: i32 = row.try_get("", "id")?;
        count += txn.execute(statement("UPDATE douyin_download SET state='waiting',failures=0 WHERE page_id IN (SELECT id FROM page WHERE video_id=?)",vec![id.into()])).await?.rows_affected() as usize;
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
            "SELECT p.id,d.state,d.failures FROM page p JOIN douyin_download d ON d.page_id=p.id WHERE p.video_id=? AND p.pid=1",
            vec![video_id.into()],
        ))
        .await?
        .ok_or_else(invalid)?;
    let page_id: i32 = row.try_get("", "id")?;
    let state: String = row.try_get("", "state")?;
    let failures: u32 = row.try_get("", "failures")?;
    if state == "succeeded" || failures >= 4 {
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

            txn.execute(statement(
                "UPDATE page SET path=?,name=coalesce(nullif(?,''),name),duration=?,width=?,height=? WHERE id=?",
                vec![
                    path.clone().into(),
                    title.clone().into(),
                    duration.into(),
                    width.into(),
                    height.into(),
                    page_id.into(),
                ],
            ))
            .await?;
            txn.execute(statement(
                "UPDATE video SET path=?,name=coalesce(nullif(?,''),name),upper_name=coalesce(nullif(?,''),upper_name) WHERE id=?",
                vec![
                    directory.into(),
                    title.into(),
                    author.into(),
                    video_id.into(),
                ],
            ))
            .await?;
            txn.execute(statement(
                "UPDATE douyin_download SET state='succeeded',failures=0 WHERE page_id=?",
                vec![page_id.into()],
            ))
            .await?;
        }
        Outcome::Failed => {
            txn.execute(statement(
                "UPDATE douyin_download SET state='failed',failures=failures+1 WHERE page_id=?",
                vec![page_id.into()],
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
mod tests;

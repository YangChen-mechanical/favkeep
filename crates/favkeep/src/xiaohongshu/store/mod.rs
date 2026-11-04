use super::{client::Item, valid_id};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, QueryResult, Statement, TransactionTrait, Value};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Serialize)]
pub struct Account {
    pub identity: Option<String>,
    pub paused_reason: Option<String>,
    pub rejections: u32,
    pub last_run: Option<i64>,
    pub last_finish: Option<i64>,
    pub next_allowed_at: Option<i64>,
    pub last_error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Source {
    pub name: &'static str,
    pub enabled: bool,
    pub revision: i64,
    pub baseline_done: bool,
    pub baseline_at: Option<i64>,
    pub last_scan: Option<i64>,
    pub last_result: Option<String>,
    pub item_count: i64,
    pub window_size: usize,
}
#[derive(Debug)]
pub struct Pending {
    pub id: i32,
    pub item: Item,
}
#[derive(Debug, Default)]
pub struct ScanSummary {
    pub baseline: bool,
    pub rebased: bool,
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
    DbErr::Custom("invalid_xiaohongshu_data".into())
}
fn conflict() -> DbErr {
    DbErr::Custom("xiaohongshu_selection_changed".into())
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
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
pub async fn account(db: &DatabaseConnection) -> Result<Option<Account>, DbErr> {
    db.query_one(statement("SELECT * FROM xiaohongshu_account WHERE id=1", vec![]))
        .await?
        .map(account_row)
        .transpose()
}
pub async fn ensure(db: &DatabaseConnection) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    tx.execute_unprepared("INSERT INTO xiaohongshu_account(id) VALUES(1) ON CONFLICT DO NOTHING; INSERT INTO xiaohongshu_source(id,account_id) VALUES(1,1) ON CONFLICT DO NOTHING;").await?;
    tx.commit().await
}
pub async fn bind(db: &DatabaseConnection, identity: &str) -> Result<(), DbErr> {
    if !valid_id(identity) {
        return Err(invalid());
    }
    ensure(db).await?;
    if db
        .execute(statement(
            "UPDATE xiaohongshu_account SET identity=? WHERE id=1 AND (identity IS NULL OR identity=?)",
            vec![identity.into(), identity.into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(DbErr::Custom("xiaohongshu_account_changed".into()));
    }
    Ok(())
}
pub async fn source(db: &DatabaseConnection) -> Result<Source, DbErr> {
    let row = db
        .query_one(statement("SELECT * FROM xiaohongshu_source WHERE id=1", vec![]))
        .await?;
    Ok(match row {
        Some(r) => Source {
            name: "全部收藏",
            enabled: r.try_get("", "enabled")?,
            revision: r.try_get("", "revision")?,
            baseline_done: r.try_get::<Option<String>>("", "window")?.is_some(),
            baseline_at: r.try_get("", "baseline_at")?,
            last_scan: r.try_get("", "last_scan")?,
            last_result: r.try_get("", "last_result")?,
            item_count: r.try_get("", "item_count")?,
            window_size: 50,
        },
        None => Source {
            name: "全部收藏",
            enabled: false,
            revision: 0,
            baseline_done: false,
            baseline_at: None,
            last_scan: None,
            last_result: None,
            item_count: 0,
            window_size: 50,
        },
    })
}
pub async fn set_enabled(db: &DatabaseConnection, enabled: bool) -> Result<Source, DbErr> {
    if db.execute(statement("UPDATE xiaohongshu_source SET enabled=?,revision=revision+(enabled!=?) WHERE id=1 AND EXISTS(SELECT 1 FROM xiaohongshu_account WHERE identity IS NOT NULL)",vec![enabled.into(),enabled.into()])).await?.rows_affected()!=1 {return Err(invalid());}
    source(db).await
}
pub async fn commit_scan(
    db: &DatabaseConnection,
    identity: &str,
    revision: i64,
    items: &[Item],
) -> Result<ScanSummary, DbErr> {
    let mut ids = HashSet::new();
    if !valid_id(identity) || items.len() > 50 {
        return Err(invalid());
    }
    for i in items {
        if !valid_id(&i.id)
            || !ids.insert(i.id.as_str())
            || !matches!(i.kind.as_str(), "video" | "unsupported")
            || i.token.len() > 4096
            || !i.token.is_ascii()
            || (i.kind == "video" && i.token.is_empty())
        {
            return Err(invalid());
        }
    }
    let tx = db.begin().await?;
    if tx.execute(statement("UPDATE xiaohongshu_source SET last_scan=? WHERE id=1 AND revision=? AND enabled=1 AND EXISTS(SELECT 1 FROM xiaohongshu_account WHERE identity=? AND paused_reason IS NULL AND (next_allowed_at IS NULL OR next_allowed_at<=?))",vec![now().into(),revision.into(),identity.into(),now().into()])).await?.rows_affected()!=1 {return Err(conflict());}
    let previous: Option<String> = tx
        .query_one(statement("SELECT window FROM xiaohongshu_source WHERE id=1", vec![]))
        .await?
        .ok_or_else(invalid)?
        .try_get("", "window")?;
    let previous: Option<Vec<String>> = previous
        .map(|s| serde_json::from_str(&s).map_err(|_| invalid()))
        .transpose()?;
    let baseline = previous.is_none();
    let overlap = previous
        .as_ref()
        .is_some_and(|old| items.iter().any(|i| old.contains(&i.id)));
    let rebased = !baseline && !overlap && !(items.is_empty() && previous.as_ref().is_some_and(Vec::is_empty));
    let mut prefix = !baseline && !rebased;
    let mut summary = ScanSummary {
        baseline,
        rebased,
        scanned: items.len(),
        ..Default::default()
    };
    for i in items {
        let old = tx
            .query_one(statement(
                "SELECT qualified FROM xiaohongshu_seen WHERE identity=? AND platform_id=?",
                vec![identity.into(), i.id.clone().into()],
            ))
            .await?;
        let old_qualified = old
            .as_ref()
            .map(|r| r.try_get::<bool>("", "qualified"))
            .transpose()?
            .unwrap_or(false);
        let candidate = prefix && old.is_none() && i.kind == "video";
        if old.is_some() {
            prefix = false;
        }
        let qualified = old_qualified || candidate;
        if candidate {
            // shortcut: legacy dates are non-null; keep unknown XHS dates as an unexposed epoch until those fields become nullable.
            let video=tx.execute(statement("INSERT INTO video(platform,platform_id,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download,single_page) VALUES ('xiaohongshu',?,'','',?,'',0,'','','1970-01-01 00:00:00','1970-01-01 00:00:00','1970-01-01 00:00:00',0,1,1,1) ON CONFLICT(platform,platform_id) WHERE platform='xiaohongshu' DO NOTHING",vec![i.id.clone().into(),i.id.clone().into()])).await?;
            if video.rows_affected() == 1 {
                tx.execute(statement(
                    "INSERT INTO page(video_id,pid,name,duration,download_status) VALUES(?,1,?,0,0)",
                    vec![(video.last_insert_id() as i64).into(), i.id.clone().into()],
                ))
                .await?;
                tx.execute(statement("INSERT INTO xiaohongshu_download(page_id,state,failures) SELECT id,'waiting',0 FROM page WHERE video_id=?",vec![(video.last_insert_id() as i64).into()])).await?;
                summary.inserted += 1;
            } else {
                summary.existing += 1;
            }
        } else if i.kind == "video" {
            if old_qualified {
                summary.existing += 1;
            } else {
                summary.historical += 1;
            }
        }
        if i.kind != "video" {
            summary.unsupported += 1;
        }
        tx.execute(statement("INSERT INTO xiaohongshu_seen(identity,platform_id,kind,token,qualified) VALUES(?,?,?,?,?) ON CONFLICT(identity,platform_id) DO UPDATE SET kind=excluded.kind,token=excluded.token,qualified=xiaohongshu_seen.qualified OR excluded.qualified",vec![identity.into(),i.id.clone().into(),i.kind.clone().into(),i.token.clone().into(),qualified.into()])).await?;
    }
    let window = serde_json::to_string(&items.iter().map(|i| &i.id).collect::<Vec<_>>()).map_err(|_| invalid())?;
    tx.execute(statement("UPDATE xiaohongshu_source SET window=?,baseline_at=coalesce(baseline_at,?),item_count=?,last_result=? WHERE id=1",vec![window.into(),now().into(),(items.len() as i64).into(),if rebased {"rebased"} else if baseline {"baseline"} else {"complete"}.into()])).await?;
    tx.commit().await?;
    Ok(summary)
}
const ELIGIBLE_SQL: &str = "SELECT v.id,i.platform_id,i.kind,i.token FROM video v JOIN page p ON p.video_id=v.id JOIN xiaohongshu_download d ON d.page_id=p.id JOIN xiaohongshu_seen i ON i.platform_id=v.platform_id JOIN xiaohongshu_account a ON a.identity=i.identity JOIN xiaohongshu_source s ON s.account_id=a.id WHERE v.platform='xiaohongshu' AND v.valid=1 AND v.should_download=1 AND p.pid=1 AND i.kind='video' AND i.qualified=1 AND a.identity=? AND s.enabled=1 AND a.paused_reason IS NULL AND (a.next_allowed_at IS NULL OR a.next_allowed_at<=CAST(strftime('%s','now') AS INTEGER))";
pub async fn pending(db: &DatabaseConnection, identity: &str) -> Result<Vec<Pending>, DbErr> {
    db.query_all(statement(
        &format!("{ELIGIBLE_SQL} AND d.state!='succeeded' AND d.failures<4 ORDER BY v.id"),
        vec![identity.into()],
    ))
    .await?
    .into_iter()
    .map(|r| {
        Ok(Pending {
            id: r.try_get("", "id")?,
            item: Item {
                id: r.try_get("", "platform_id")?,
                kind: r.try_get("", "kind")?,
                token: r.try_get("", "token")?,
            },
        })
    })
    .collect()
}
pub async fn is_pending(db: &DatabaseConnection, identity: &str, id: i32) -> Result<bool, DbErr> {
    Ok(db
        .query_one(statement(
            &format!("{ELIGIBLE_SQL} AND v.id=? AND d.state!='succeeded' AND d.failures<4"),
            vec![identity.into(), id.into()],
        ))
        .await?
        .is_some())
}
pub async fn can_retry(db: &DatabaseConnection, id: i32) -> Result<bool, DbErr> {
    let Some(identity) = account(db).await?.and_then(|a| a.identity) else {
        return Ok(false);
    };
    Ok(db
        .query_one(statement(
            &format!("{ELIGIBLE_SQL} AND v.id=? AND d.state='failed'"),
            vec![identity.into(), id.into()],
        ))
        .await?
        .is_some())
}
pub async fn retry_failed(db: &DatabaseConnection, id: Option<i32>) -> Result<usize, DbErr> {
    let Some(identity) = account(db).await?.and_then(|a| a.identity) else {
        return Err(invalid());
    };
    Ok(db.execute(statement(&format!("UPDATE xiaohongshu_download SET state='waiting',failures=0 WHERE page_id IN (SELECT p.id FROM page p WHERE p.video_id IN (SELECT v.id FROM ({ELIGIBLE_SQL} AND (? IS NULL OR v.id=?) AND d.state='failed') v))"),vec![identity.into(),id.into(),id.into()])).await?.rows_affected() as usize)
}
pub async fn mark_outcome(db: &DatabaseConnection, video_id: i32, outcome: Outcome) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let updated = txn
        .execute(statement(
            "UPDATE video SET id=id WHERE id=? AND platform='xiaohongshu'",
            vec![video_id.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(invalid());
    }
    let row = txn
        .query_one(statement(
            "SELECT p.id,d.state,d.failures FROM page p JOIN xiaohongshu_download d ON d.page_id=p.id WHERE p.video_id=? AND p.pid=1",
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
                "UPDATE xiaohongshu_download SET state='succeeded',failures=0 WHERE page_id=?",
                vec![page_id.into()],
            ))
            .await?;
        }
        Outcome::Failed => {
            txn.execute(statement(
                "UPDATE xiaohongshu_download SET state='failed',failures=failures+1 WHERE page_id=?",
                vec![page_id.into()],
            ))
            .await?;
        }
    }
    txn.commit().await
}

pub async fn begin_run(db: &DatabaseConnection, identity: &str) -> Result<bool, DbErr> {
    Ok(db.execute(statement("UPDATE xiaohongshu_account SET last_run=? WHERE identity=? AND paused_reason IS NULL AND (next_allowed_at IS NULL OR next_allowed_at<=?) AND EXISTS(SELECT 1 FROM xiaohongshu_source WHERE enabled=1)",vec![now().into(),identity.into(),now().into()])).await?.rows_affected()==1)
}
pub async fn account_outcome(db: &DatabaseConnection, outcome: AccountOutcome, interval: u64) -> Result<(), DbErr> {
    ensure(db).await?;
    let tx = db.begin().await?;
    tx.execute(statement(
        "UPDATE xiaohongshu_account SET last_finish=? WHERE id=1",
        vec![now().into()],
    ))
    .await?;
    let (error, pause) = match outcome {
        AccountOutcome::Success => {
            tx.execute_unprepared(
                "UPDATE xiaohongshu_account SET rejections=0,next_allowed_at=NULL,last_error=NULL WHERE id=1",
            )
            .await?;
            (None, None)
        }
        AccountOutcome::Rejected => {
            tx.execute(statement("UPDATE xiaohongshu_account SET rejections=min(rejections+1,3),next_allowed_at=?,last_error='rejected',paused_reason=CASE WHEN rejections+1>=3 THEN 'rejected' ELSE paused_reason END WHERE id=1",vec![now().saturating_add(i64::try_from(interval).unwrap_or(i64::MAX)).into()])).await?;
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
        tx.execute(statement(
            "UPDATE xiaohongshu_account SET last_error=?,paused_reason=coalesce(?,paused_reason) WHERE id=1",
            vec![error.into(), pause.map(str::to_owned).into()],
        ))
        .await?;
    }
    tx.commit().await
}
pub async fn resume_account(db: &DatabaseConnection, identity: &str) -> Result<bool, DbErr> {
    Ok(db.execute(statement("UPDATE xiaohongshu_account SET rejections=0,paused_reason=NULL,next_allowed_at=NULL,last_error=NULL WHERE identity=? AND (next_allowed_at IS NULL OR next_allowed_at<=?)",vec![identity.into(),now().into()])).await?.rows_affected()==1)
}

#[cfg(test)]
mod tests;

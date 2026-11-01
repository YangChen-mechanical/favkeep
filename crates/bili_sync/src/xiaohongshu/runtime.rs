use super::{
    Error, RUNNING, Result,
    client::{self, Auth, Item, Session},
    progress,
    store::{self, AccountOutcome, Outcome},
};
use crate::{
    config::Config,
    douyin::browser::{private_dir, write_private},
    media_file,
};
use sea_orm::DatabaseConnection;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
pub static NEXT_RUN: AtomicI64 = AtomicI64::new(0);
fn database(_: sea_orm::DbErr) -> Error {
    Error::File
}
pub fn next_run() -> Option<String> {
    chrono::DateTime::from_timestamp(NEXT_RUN.load(Ordering::Relaxed), 0)
        .filter(|t| t.timestamp() > 0)
        .map(|t| t.to_rfc3339())
}
pub async fn restore(identity: &str) -> Result<(Session, Auth)> {
    let root = super::login::root();
    let saved = Auth::read(&root.join("auth.json"))?;
    if saved.identity != identity {
        return Err(Error::AccountChanged);
    }
    let session = Session::new(Some(&saved))?;
    let auth = client::authenticate(&session, Some(&saved), Some(identity), None).await?;
    write_private(&root.join("auth.json"), &auth)?;
    Ok((session, auth))
}
pub async fn run(db: &DatabaseConnection, config: &Config) -> Result<()> {
    let Ok(_guard) = RUNNING.try_lock() else {
        info!("[小红书] 跳过重叠任务：账号忙碌");
        return Ok(());
    };
    let source = store::source(db).await.map_err(database)?;
    if !source.enabled {
        return Ok(());
    }
    let Some(account) = store::account(db).await.map_err(database)? else {
        return Ok(());
    };
    let Some(identity) = account.identity else {
        return Ok(());
    };
    if !store::begin_run(db, &identity).await.map_err(database)? {
        return Err(Error::Paused);
    }
    let mut activity = progress::start("sync");
    let result = async {
        activity.stage("restoring", None, None);
        let (session, auth) = restore(&identity).await?;
        async {
            activity.stage("scanning", None, None);
            let items = tokio::time::timeout(Duration::from_secs(900), client::scan(&session, &auth))
                .await
                .map_err(|_| Error::Incomplete)??;
            let fresh = client::authenticate(&session, Some(&auth), Some(&identity), None).await?;
            write_private(&super::login::root().join("auth.json"), &fresh)?;
            let scan = store::commit_scan(db, &identity, source.revision, &items)
                .await
                .map_err(|e| match e {
                    sea_orm::DbErr::Custom(c) if c == "xiaohongshu_selection_changed" => Error::SelectionChanged,
                    _ => Error::File,
                })?;
            activity.scanned(&scan);
            if scan.rebased {
                warn!("[小红书] 窗口断档，已重设起点，本批未下载");
                return Ok(());
            }
            if scan.baseline {
                info!("[小红书] 首次窗口已记录，本批不下载历史");
                return Ok(());
            }
            let pending = store::pending(db, &identity).await.map_err(database)?;
            activity.summary(|s| s.pending = Some(pending.len()));
            let mut failure = None;
            for item in pending {
                if !store::is_pending(db, &identity, item.id).await.map_err(database)? {
                    continue;
                }
                activity.stage("downloading", None, Some(&item.item.id));
                match save(
                    db,
                    &identity,
                    item.id,
                    &session,
                    &fresh,
                    &item.item,
                    Path::new(&config.xiaohongshu.path),
                )
                .await
                {
                    Ok((outcome, kind)) => {
                        store::mark_outcome(db, item.id, outcome).await.map_err(database)?;
                        activity.summary(|s| {
                            match kind {
                                SaveKind::Downloaded => s.downloaded += 1,
                                SaveKind::Reused => s.reused += 1,
                                SaveKind::Recovered => s.recovered += 1,
                            };
                            s.pending = s.pending.map(|n| n.saturating_sub(1));
                        });
                        info!("[小红书] 新发现视频已保存并登记");
                    }
                    Err(
                        e @ (Error::Rejected
                        | Error::LoginExpired
                        | Error::Challenge
                        | Error::DiskFull
                        | Error::SelectionChanged
                        | Error::AccountChanged
                        | Error::IdentityUnverified),
                    ) => return Err(e),
                    Err(e) => {
                        store::mark_outcome(db, item.id, Outcome::Failed)
                            .await
                            .map_err(database)?;
                        activity.summary(|s| s.failed += 1);
                        failure.get_or_insert(e);
                    }
                }
            }
            failure.map_or(Ok(()), Err)
        }
        .await
    }
    .await;
    if let Ok(pending) = store::pending(db, &identity).await {
        activity.summary(|s| s.pending = Some(pending.len()));
    }
    store::account_outcome(db, account_result(&result), config.xiaohongshu.interval)
        .await
        .map_err(database)?;
    activity.finish(&result);
    progress::report("同步", &result);
    result
}
pub async fn resume(db: &DatabaseConnection, config: &Config) -> Result<()> {
    let _guard = RUNNING.try_lock().map_err(|_| Error::Busy)?;
    let Some(a) = store::account(db).await.map_err(database)? else {
        return Err(Error::LoginExpired);
    };
    if a.next_allowed_at.is_some_and(|t| t > chrono::Utc::now().timestamp()) {
        return Err(Error::Paused);
    }
    let identity = a.identity.ok_or(Error::LoginExpired)?;
    let mut activity = progress::start("resume");
    let result = async {
        let _verified = restore(&identity).await?;
        if !store::resume_account(db, &identity).await.map_err(database)? {
            return Err(Error::Paused);
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        store::account_outcome(db, account_result(&result), config.xiaohongshu.interval)
            .await
            .map_err(database)?;
    }
    activity.finish(&result);
    progress::report("验证连接与恢复", &result);
    result
}
pub fn account_result(result: &Result<()>) -> AccountOutcome {
    match result {
        Ok(()) => AccountOutcome::Success,
        Err(Error::Rejected) => AccountOutcome::Rejected,
        Err(Error::LoginExpired) => AccountOutcome::LoginExpired,
        Err(Error::Challenge) => AccountOutcome::Challenge,
        Err(Error::IdentityUnverified) => AccountOutcome::IdentityUnverified,
        Err(Error::AccountChanged) => AccountOutcome::AccountChanged,
        Err(Error::DiskFull) => AccountOutcome::DiskFull,
        Err(Error::Network) => AccountOutcome::Network,
        Err(_) => AccountOutcome::Failed,
    }
}
#[derive(Debug)]
enum SaveKind {
    Downloaded,
    Reused,
    Recovered,
}
async fn inspect(path: &Path) -> Result<media_file::MediaInfo> {
    let info = media_file::inspect(path).await?;
    if !info.mp4 || info.video_codec.as_deref() != Some("h264") {
        return Err(Error::MediaUnavailable);
    }
    Ok(info)
}
async fn save(
    db: &DatabaseConnection,
    identity: &str,
    video_id: i32,
    session: &Session,
    auth: &Auth,
    item: &Item,
    root: &Path,
) -> Result<(Outcome, SaveKind)> {
    if !super::valid_id(&item.id) || !root.is_absolute() {
        return Err(Error::InvalidData);
    }
    let dir = root.join(&item.id);
    private_dir(&dir)?;
    let final_path = dir.join("video.mp4");
    let part = dir.join("video.part");
    let ready = dir.join("video.ready.json");
    let (mut title, mut author) = (String::new(), String::new());
    let mut kind = SaveKind::Downloaded;
    let info = if final_path.try_exists().map_err(Error::from)? {
        kind = SaveKind::Reused;
        inspect(&final_path).await?
    } else {
        let recovered = if media_file::ready_part(&part, &ready).await {
            inspect(&part).await.ok()
        } else {
            None
        };
        if recovered.is_some() {
            kind = SaveKind::Recovered;
        } else {
            let detail = match client::detail(session, auth, item).await {
                Ok(detail) => detail,
                Err(error @ (Error::Rejected | Error::LoginExpired | Error::Challenge)) => return Err(error),
                Err(error) => {
                    // A missing detail can mean a removed note or an expired account; confirm self before retrying it.
                    let fresh = client::authenticate(session, Some(auth), Some(identity), None).await?;
                    write_private(&super::login::root().join("auth.json"), &fresh)?;
                    return Err(error);
                }
            };
            title = detail.title;
            author = detail.author;
            crate::media_file::discard_partial(&part, &ready).await?;
            let _transfer = crate::resources::acquire(&crate::resources::TRANSFERS, "小红书视频传输").await;
            let response = client::media_response(auth, detail.url).await?;
            if media_file::available_space(&dir)?
                < response
                    .content_length()
                    .unwrap_or(64 * 1024 * 1024)
                    .saturating_add(64 * 1024 * 1024)
            {
                return Err(Error::DiskFull);
            }
            let mut file = tokio::fs::File::create_new(&part).await.map_err(Error::from)?;
            crate::downloader::copy_response(response, &mut file)
                .await
                .map_err(|e| {
                    if e.chain()
                        .filter_map(|e| e.downcast_ref::<std::io::Error>())
                        .any(|e| matches!(e.raw_os_error(), Some(28 | 112)))
                    {
                        Error::DiskFull
                    } else {
                        Error::Network
                    }
                })?;
            file.flush().await.map_err(Error::from)?;
            file.sync_all().await.map_err(Error::from)?;
            drop(file);
            inspect(&part).await?;
            write_private(
                &ready,
                &serde_json::json!({"bytes":tokio::fs::metadata(&part).await.map_err(Error::from)?.len()}),
            )?;
        }
        if !store::is_pending(db, identity, video_id).await.map_err(database)? {
            return Err(Error::SelectionChanged);
        }
        media_file::publish(&part, &ready, &final_path).await?;
        inspect(&final_path).await?
    };
    let poster = dir.join("poster.jpg");
    if !poster.exists() {
        let _permit = crate::resources::acquire(&crate::resources::MEDIA_PROCESSES, "视频封面").await;
        let _ = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(crate::config::ARGS.ffmpeg_path.as_deref().unwrap_or("ffmpeg"))
                .args(["-v", "error", "-i"])
                .arg(&final_path)
                .args(["-frames:v", "1", "-n"])
                .arg(poster)
                .kill_on_drop(true)
                .output(),
        )
        .await;
    }
    Ok((
        Outcome::Succeeded {
            path: final_path.to_string_lossy().into_owned(),
            title,
            author,
            duration: info.duration,
            width: Some(info.width),
            height: Some(info.height),
        },
        kind,
    ))
}

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::browser::{Auth, Session, private_dir, write_private};
use super::client::{Client, Folder};
use super::store::{self, AccountOutcome, Outcome};
use super::{Error, RUNNING, Result};
use crate::config::{ARGS, Config};

pub static NEXT_RUN: AtomicI64 = AtomicI64::new(0);

fn database(_: sea_orm::DbErr) -> Error {
    Error::File
}

pub async fn refresh(db: &DatabaseConnection, config: &Config) -> Result<()> {
    let _guard = RUNNING.try_lock().map_err(|_| Error::Busy)?;
    let saved = Auth::read(&super::login::root().join("auth.json"))?;
    let account = store::ensure_account(db, &saved.identity).await.map_err(database)?;
    if account.paused_reason.is_some()
        || account
            .next_allowed_at
            .is_some_and(|t| t > chrono::Utc::now().timestamp())
    {
        return Err(Error::Paused);
    }
    let result = async {
        let (session, auth) = restore(config).await?;
        let fetched = async {
            let folders = Client::new(&auth).await?.folders().await?;
            store::sync_folders(db, &auth.identity, &folders)
                .await
                .map_err(database)?;
            Ok(())
        }
        .await;
        let closed = session.close().await;
        fetched.and(closed)
    }
    .await;
    // Listing folders cannot reset rejection history; only a complete scan cycle can.
    if let Err(
        error @ (Error::Rejected
        | Error::LoginExpired
        | Error::Challenge
        | Error::IdentityUnverified
        | Error::AccountChanged),
    ) = &result
    {
        store::account_outcome(
            db,
            &saved.identity,
            account_result(&Err(error.clone())),
            config.douyin.interval,
        )
        .await
        .map_err(database)?;
    }
    result
}

pub async fn restore(config: &Config) -> Result<(Session, Auth)> {
    let root = super::login::root();
    let saved = Auth::read(&root.join("auth.json"))?;
    let session = Session::launch(&root, Path::new(&config.douyin.browser)).await?;
    match session.restore(&saved, false).await {
        Ok(auth) => {
            if let Err(error) = write_private(&root.join("auth.json"), &auth) {
                let _ = session.close().await;
                return Err(error);
            }
            Ok((session, auth))
        }
        Err(error) => {
            let _ = session.close().await;
            Err(error)
        }
    }
}

pub async fn run(db: &DatabaseConnection, config: &Config) -> Result<()> {
    let Ok(_guard) = RUNNING.try_lock() else {
        return Ok(());
    };
    let root = super::login::root();
    if !root.join("auth.json").exists() {
        return Ok(());
    }
    let saved = Auth::read(&root.join("auth.json"))?;
    store::ensure_account(db, &saved.identity).await.map_err(database)?;
    if !store::begin_run(db, &saved.identity).await.map_err(database)? {
        return Ok(());
    }
    let result = async {
        let (session, auth) = restore(config).await?;
        let round = scan_and_download(db, config, &auth).await;
        let closed = session.close().await;
        round.and(closed)
    }
    .await;
    store::account_outcome(db, &saved.identity, account_result(&result), config.douyin.interval)
        .await
        .map_err(database)?;
    result
}

pub fn account_result(result: &Result<()>) -> AccountOutcome {
    match result {
        Ok(()) => AccountOutcome::Success,
        Err(Error::Rejected) => AccountOutcome::Rejected,
        Err(Error::LoginExpired) => AccountOutcome::LoginExpired,
        Err(Error::IdentityUnverified) => AccountOutcome::IdentityUnverified,
        Err(Error::AccountChanged) => AccountOutcome::AccountChanged,
        Err(Error::Challenge) => AccountOutcome::Challenge,
        Err(Error::DiskFull) => AccountOutcome::DiskFull,
        Err(Error::Network) => AccountOutcome::Network,
        Err(_) => AccountOutcome::Failed,
    }
}

async fn scan_and_download(db: &DatabaseConnection, config: &Config, auth: &Auth) -> Result<()> {
    let client = Client::new(auth).await?;
    let folders = client.folders().await?;
    let stored = store::sync_folders(db, &auth.identity, &folders)
        .await
        .map_err(database)?;
    for folder in stored.iter().filter(|f| f.selected && f.available) {
        let remote = Folder {
            id: folder.id.clone(),
            name: folder.name.clone(),
            total: folder.total.map(|n| n as u64),
        };
        let items = match client.scan_folder(&remote).await {
            Ok(items) => items,
            Err(error) => {
                store::folder_error(db, &auth.identity, &folder.id, &error.to_string())
                    .await
                    .map_err(database)?;
                return Err(error);
            }
        };
        store::commit_scan(db, &auth.identity, &folder.id, folder.revision, &items)
            .await
            .map_err(|error| {
                if error.to_string().contains("douyin_selection_changed") {
                    Error::SelectionChanged
                } else {
                    Error::File
                }
            })?;
    }
    let mut failure = None;
    for item in store::pending(db, &auth.identity).await.map_err(database)? {
        // Recheck selection after each completed file; a disabled source must not start another download.
        if !store::is_pending(db, &auth.identity, item.id).await.map_err(database)? {
            continue;
        }
        match save(&client, &item.platform_id, Path::new(&config.douyin.path)).await {
            Ok(outcome) => store::mark_outcome(db, item.id, outcome).await.map_err(database)?,
            Err(error @ (Error::Rejected | Error::LoginExpired | Error::Challenge | Error::DiskFull)) => {
                return Err(error);
            }
            Err(error) => {
                store::mark_outcome(db, item.id, Outcome::Failed)
                    .await
                    .map_err(database)?;
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn save(client: &Client, id: &str, root: &Path) -> Result<Outcome> {
    if !super::valid_id(id) || !root.is_absolute() {
        return Err(Error::InvalidData);
    }
    let directory = root.join(id);
    tokio::fs::create_dir_all(&directory).await.map_err(Error::from)?;
    private_dir(&directory)?;
    let final_path = directory.join("video.mp4");
    let part = directory.join("video.part");
    let ready = directory.join("video.ready.json");
    let mut title = String::new();
    let mut author = String::new();
    let info = if final_path.try_exists().map_err(Error::from)? {
        inspect(&final_path).await?
    } else {
        // A valid partial file may have been synced just before a process crash.
        let recovered = if ready_part(&part, &ready).await {
            inspect(&part).await.ok()
        } else {
            None
        };
        let info = match recovered {
            Some(info) => info,
            None => {
                let details = client.detail(id).await?;
                title = details.title;
                author = details.author;
                if let Ok(metadata) = tokio::fs::symlink_metadata(&part).await {
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        return Err(Error::File);
                    }
                    tokio::fs::remove_file(&part).await.map_err(Error::from)?;
                }
                let response = client
                    .media_response(details.urls.first().ok_or(Error::MediaUnavailable)?)
                    .await?;
                let needed = response
                    .content_length()
                    .unwrap_or(64 * 1024 * 1024)
                    .saturating_add(64 * 1024 * 1024);
                if available_space(&directory)? < needed {
                    return Err(Error::DiskFull);
                }
                let mut file = tokio::fs::File::create_new(&part).await.map_err(Error::from)?;
                if let Err(error) = crate::downloader::copy_response(response, &mut file).await {
                    return Err(
                        if error
                            .chain()
                            .filter_map(|e| e.downcast_ref::<std::io::Error>())
                            .any(|e| matches!(e.raw_os_error(), Some(28 | 112)))
                        {
                            Error::DiskFull
                        } else {
                            Error::Network
                        },
                    );
                }
                file.flush().await.map_err(Error::from)?;
                file.sync_all().await.map_err(Error::from)?;
                drop(file);
                let info = inspect(&part).await?;
                let bytes = tokio::fs::metadata(&part).await.map_err(Error::from)?.len();
                write_private(&ready, &serde_json::json!({"bytes":bytes}))?;
                info
            }
        };
        // Hard-link publishes on the same disk without overwriting an existing finished file.
        match tokio::fs::hard_link(&part, &final_path).await {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                inspect(&final_path).await?;
            }
            Err(error) => return Err(Error::from(error)),
        }
        tokio::fs::remove_file(&part).await.map_err(Error::from)?;
        let _ = tokio::fs::remove_file(&ready).await;
        #[cfg(unix)]
        {
            let directory = directory.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .map_err(|_| Error::File)?
                .map_err(Error::from)?;
        }
        info
    };
    let poster = directory.join("poster.jpg");
    if !poster.exists() {
        let _ = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(ARGS.ffmpeg_path.as_deref().unwrap_or("ffmpeg"))
                .args(["-v", "error", "-i"])
                .arg(&final_path)
                .args(["-frames:v", "1", "-n"])
                .arg(&poster)
                .kill_on_drop(true)
                .output(),
        )
        .await;
    }
    Ok(Outcome::Succeeded {
        path: final_path.to_string_lossy().into_owned(),
        title,
        author,
        duration: info.duration,
        width: Some(info.width),
        height: Some(info.height),
    })
}

async fn ready_part(part: &Path, ready: &Path) -> bool {
    let Ok(metadata) = tokio::fs::symlink_metadata(part).await else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(marker) = tokio::fs::read(ready).await else {
        return false;
    };
    serde_json::from_slice::<Value>(&marker)
        .ok()
        .and_then(|v| v["bytes"].as_u64())
        == Some(metadata.len())
}

fn available_space(path: &Path) -> Result<u64> {
    let path = path.canonicalize().map_err(Error::from)?;
    sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| {
            d.mount_point()
                .canonicalize()
                .is_ok_and(|mount| path.starts_with(mount))
        })
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .ok_or(Error::File)
}

struct MediaInfo {
    duration: u32,
    width: u32,
    height: u32,
}

fn ffprobe() -> PathBuf {
    ARGS.ffmpeg_path
        .as_ref()
        .map(|p| Path::new(p).with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }))
        .unwrap_or_else(|| "ffprobe".into())
}

async fn inspect(path: &Path) -> Result<MediaInfo> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(Error::from)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(Error::MediaUnavailable);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(ffprobe())
            .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
            .arg(path)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::MediaUnavailable)?
    .map_err(|_| Error::MediaUnavailable)?;
    if !output.status.success() {
        return Err(Error::MediaUnavailable);
    }
    parse_media(&serde_json::from_slice::<Value>(&output.stdout).map_err(|_| Error::MediaUnavailable)?)
}

fn parse_media(info: &Value) -> Result<MediaInfo> {
    let video = info["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
        .ok_or(Error::MediaUnavailable)?;
    let duration = info["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .ok_or(Error::MediaUnavailable)?;
    let width = video["width"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or(Error::MediaUnavailable)?;
    let height = video["height"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or(Error::MediaUnavailable)?;
    if !duration.is_finite() || duration <= 0.0 || duration > u32::MAX as f64 {
        return Err(Error::MediaUnavailable);
    }
    Ok(MediaInfo {
        duration: duration.ceil() as u32,
        width,
        height,
    })
}

pub fn next_run() -> Option<String> {
    chrono::DateTime::from_timestamp(NEXT_RUN.load(Ordering::Relaxed), 0)
        .filter(|t| t.timestamp() > 0)
        .map(|t| t.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn refuses_empty_corrupt_and_unbounded_media() {
        assert!(parse_media(&json!({})).is_err());
        for duration in ["NaN", "inf", "0", "-1", "4294967296"] {
            assert!(
                parse_media(
                    &json!({"streams":[{"codec_type":"video","width":10,"height":10}],"format":{"duration":duration}})
                )
                .is_err()
            );
        }
        let info = parse_media(
            &json!({"streams":[{"codec_type":"video","width":1920,"height":1080}],"format":{"duration":"24.98"}}),
        )
        .unwrap();
        assert_eq!((info.duration, info.width, info.height), (25, 1920, 1080));
    }
    #[test]
    fn disk_full_pauses_and_ordinary_errors_do_not_count_as_rejections() {
        assert!(matches!(
            account_result(&Err(Error::DiskFull)),
            AccountOutcome::DiskFull
        ));
        assert!(matches!(account_result(&Err(Error::Network)), AccountOutcome::Network));
        assert!(matches!(
            account_result(&Err(Error::IdentityUnverified)),
            AccountOutcome::IdentityUnverified
        ));
        assert!(matches!(
            account_result(&Err(Error::AccountChanged)),
            AccountOutcome::AccountChanged
        ));
        assert!(matches!(
            account_result(&Err(Error::Rejected)),
            AccountOutcome::Rejected
        ));
        assert_eq!(Error::from(std::io::Error::from_raw_os_error(28)), Error::DiskFull);
    }
    #[test]
    fn disk_lookup_matches_the_canonical_windows_or_unix_volume_path() {
        let directory = tempfile::tempdir().unwrap();
        assert!(available_space(directory.path()).unwrap() > 0);
    }
    #[tokio::test]
    async fn interrupted_download_requires_a_synced_completion_marker_with_matching_length() {
        let directory = tempfile::tempdir().unwrap();
        let part = directory.path().join("video.part");
        let ready = directory.path().join("video.ready.json");
        tokio::fs::write(&part, b"incomplete").await.unwrap();
        assert!(!ready_part(&part, &ready).await);
        tokio::fs::write(&ready, b"{\"bytes\":999}").await.unwrap();
        assert!(!ready_part(&part, &ready).await);
        tokio::fs::write(&ready, b"{\"bytes\":10}").await.unwrap();
        assert!(ready_part(&part, &ready).await);
    }
}

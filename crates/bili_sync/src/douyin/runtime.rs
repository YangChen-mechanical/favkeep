use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::browser::{Auth, Session, private_dir, write_private};
use super::client::{Client, Folder};
use super::progress::{self, Activity};
use super::store::{self, AccountOutcome, Outcome};
use super::{Error, RUNNING, Result};
use crate::config::{ARGS, Config};

pub static NEXT_RUN: AtomicI64 = AtomicI64::new(0);

fn database(_: sea_orm::DbErr) -> Error {
    Error::File
}

pub async fn refresh(db: &DatabaseConnection, config: &Config) -> Result<()> {
    info!("[抖音] 收到刷新收藏夹请求");
    let result = async {
        let _guard = RUNNING.try_lock().map_err(|_| Error::Busy)?;
        let mut activity = progress::start("refresh");
        let result = async {
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
                activity.stage("restoring", None, None);
                let (session, auth) = restore(config).await?;
                let fetched = async {
                    activity.stage("folders", None, None);
                    let folders = Client::new(&auth).await?.folders().await?;
                    store::sync_folders(db, &auth.identity, &folders)
                        .await
                        .map_err(database)?;
                    info!("[抖音] 收藏夹列表已更新，共 {} 个", folders.len());
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
        .await;
        activity.finish(&result);
        result
    }
    .await;
    progress::report("刷新收藏夹", &result);
    result
}

pub async fn restore(config: &Config) -> Result<(Session, Auth)> {
    info!("[抖音] 正在恢复旧会话并验证账号");
    let root = super::login::root();
    let saved = Auth::read(&root.join("auth.json"))?;
    let session = Session::launch(&root, Path::new(&config.douyin.browser)).await?;
    match session.restore(&saved, false).await {
        Ok(auth) => {
            if let Err(error) = write_private(&root.join("auth.json"), &auth) {
                let _ = session.close().await;
                return Err(error);
            }
            info!("[抖音] 旧会话已恢复，刷新后的凭据已安全保存");
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
        info!("[抖音] 跳过本轮同步：账号正在处理其他操作");
        return Ok(());
    };
    let root = super::login::root();
    if !root.join("auth.json").exists() {
        info!("[抖音] 跳过本轮同步：尚未连接账号");
        return Ok(());
    }
    let mut activity = progress::start("sync");
    info!("[抖音] 本轮同步开始");
    let result = async {
        let saved = Auth::read(&root.join("auth.json"))?;
        store::ensure_account(db, &saved.identity).await.map_err(database)?;
        if !store::begin_run(db, &saved.identity).await.map_err(database)? {
            return Err(Error::Paused);
        }
        let round = async {
            activity.stage("restoring", None, None);
            let (session, auth) = restore(config).await?;
            let round = scan_and_download(db, config, &auth, &activity).await;
            let closed = session.close().await;
            round.and(closed)
        }
        .await;
        // Count remaining eligible work before account_outcome applies a pause or cooldown.
        if let Ok(pending) = store::pending(db, &saved.identity).await {
            activity.summary(|s| s.pending = Some(pending.len()));
        } else {
            activity.summary(|s| s.pending = None);
        }
        store::account_outcome(db, &saved.identity, account_result(&round), config.douyin.interval)
            .await
            .map_err(database)?;
        round
    }
    .await;
    activity.finish(&result);
    progress::report("同步下载", &result);
    result
}

pub async fn resume(db: &DatabaseConnection, config: &Config) -> Result<()> {
    info!("[抖音] 收到验证连接与恢复请求");
    let result = async {
        let _guard = RUNNING.try_lock().map_err(|_| Error::Busy)?;
        let mut activity = progress::start("resume");
        let result = async {
            let auth = Auth::read(&super::login::root().join("auth.json"))?;
            let account = store::ensure_account(db, &auth.identity).await.map_err(database)?;
            if account
                .next_allowed_at
                .is_some_and(|n| n > chrono::Utc::now().timestamp())
            {
                return Err(Error::Paused);
            }
            activity.stage("restoring", None, None);
            let (session, fresh) = match restore(config).await {
                Ok(restored) => restored,
                Err(error) => {
                    store::account_outcome(
                        db,
                        &auth.identity,
                        account_result(&Err(error.clone())),
                        config.douyin.interval,
                    )
                    .await
                    .map_err(database)?;
                    return Err(error);
                }
            };
            session.close().await?;
            store::resume_account(db, &fresh.identity).await.map_err(database)?;
            Ok(())
        }
        .await;
        activity.finish(&result);
        result
    }
    .await;
    progress::report("验证连接与恢复", &result);
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

async fn scan_and_download(
    db: &DatabaseConnection,
    config: &Config,
    auth: &Auth,
    activity: &Activity<'_>,
) -> Result<()> {
    activity.stage("folders", None, None);
    let client = Client::new(auth).await?;
    let folders = client.folders().await?;
    let stored = store::sync_folders(db, &auth.identity, &folders)
        .await
        .map_err(database)?;
    let selected = stored.iter().filter(|f| f.selected && f.available).count();
    info!(
        "[抖音] 获取 {} 个收藏夹，本轮扫描 {} 个已启用且可用的收藏夹",
        stored.len(),
        selected
    );
    if selected == 0 {
        info!("[抖音] 没有可扫描来源，请先启用收藏夹");
    }
    for folder in stored.iter().filter(|f| f.selected && f.available) {
        activity.stage("scanning", Some(&folder.name), None);
        info!("[抖音] 开始扫描收藏夹 {:?}（{}）", folder.name, folder.id);
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
        let committed = store::commit_scan(db, &auth.identity, &folder.id, folder.revision, &items)
            .await
            .map_err(|error| {
                if error.to_string().contains("douyin_selection_changed") {
                    Error::SelectionChanged
                } else {
                    Error::File
                }
            });
        let committed = match committed {
            Ok(committed) => committed,
            Err(error) => {
                store::folder_error(db, &auth.identity, &folder.id, &error.to_string())
                    .await
                    .map_err(database)?;
                return Err(error);
            }
        };
        activity.scanned(&committed);
        info!(
            "[抖音] 收藏夹 {:?} 扫描已提交：扫描 {}，历史跳过 {}，已有 {}，不支持 {}，新增入库 {}",
            folder.name,
            committed.scanned,
            committed.historical,
            committed.existing,
            committed.unsupported,
            committed.inserted
        );
        if committed.baseline {
            info!(
                "[抖音] 收藏夹 {:?} 首次历史基线已建立，本次不下载历史内容；后续保存新增视频",
                folder.name
            );
        } else if committed.scanned == 0 {
            info!("[抖音] 收藏夹 {:?} 为空，没有新增视频", folder.name);
        } else if committed.inserted == 0 {
            info!(
                "[抖音] 收藏夹 {:?} 没有新增入库视频：条目属于历史、已有或不支持类型",
                folder.name
            );
        }
    }
    let mut failure = None;
    let pending = store::pending(db, &auth.identity).await.map_err(database)?;
    activity.summary(|s| s.pending = Some(pending.len()));
    info!("[抖音] 扫描阶段结束，{} 个视频待处理", pending.len());
    if pending.is_empty() {
        info!("[抖音] 本轮无需下载：没有符合当前来源规则的待处理视频");
    }
    for item in pending {
        // Recheck selection after each completed file; a disabled source must not start another download.
        if !store::is_pending(db, &auth.identity, item.id).await.map_err(database)? {
            activity.summary(|s| s.pending = None);
            continue;
        }
        activity.stage("downloading", None, Some(&item.platform_id));
        info!("[抖音] 开始处理视频 {}", item.platform_id);
        match save(&client, &item.platform_id, Path::new(&config.douyin.path)).await {
            Ok((outcome, kind)) => {
                activity.stage("registering", None, Some(&item.platform_id));
                if let Err(error) = store::mark_outcome(db, item.id, outcome).await {
                    activity.summary(|s| s.failed += 1);
                    return Err(database(error));
                }
                activity.summary(|s| match kind {
                    SaveKind::Downloaded => s.downloaded += 1,
                    SaveKind::Reused => s.reused += 1,
                    SaveKind::Recovered => s.recovered += 1,
                });
                activity.summary(|s| s.pending = s.pending.map(|n| n.saturating_sub(1)));
                info!(
                    "[抖音] 视频 {} 保存成功：{}，文件已校验、发布并登记",
                    item.platform_id,
                    kind.message()
                );
            }
            Err(error @ (Error::Rejected | Error::LoginExpired | Error::Challenge | Error::DiskFull)) => {
                activity.summary(|s| s.failed += 1);
                return Err(error);
            }
            Err(error) => {
                activity.summary(|s| {
                    s.failed += 1;
                    s.pending = None;
                });
                store::mark_outcome(db, item.id, Outcome::Failed)
                    .await
                    .map_err(database)?;
                error!("[抖音] 视频 {} 下载失败：{}", item.platform_id, error.message());
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

#[derive(Debug, PartialEq, Eq)]
enum SaveKind {
    Downloaded,
    Reused,
    Recovered,
}

impl SaveKind {
    fn message(&self) -> &'static str {
        match self {
            Self::Downloaded => "新下载",
            Self::Reused => "复用已有成品，未重新下载",
            Self::Recovered => "恢复已完成的中断文件，未重新下载",
        }
    }
}

async fn save(client: &Client, id: &str, root: &Path) -> Result<(Outcome, SaveKind)> {
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
    let mut kind = SaveKind::Downloaded;
    let info = if final_path.try_exists().map_err(Error::from)? {
        kind = SaveKind::Reused;
        inspect(&final_path).await?
    } else {
        // A valid partial file may have been synced just before a process crash.
        let recovered = if ready_part(&part, &ready).await {
            inspect(&part).await.ok()
        } else {
            None
        };
        let info = match recovered {
            Some(info) => {
                kind = SaveKind::Recovered;
                info
            }
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
                let transfer = crate::resources::acquire(&crate::resources::TRANSFERS, "抖音视频传输").await;
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
                drop(transfer);
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
        let _process = crate::resources::acquire(&crate::resources::MEDIA_PROCESSES, "视频封面").await;
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
    let _process = crate::resources::acquire(&crate::resources::MEDIA_PROCESSES, "视频校验").await;
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

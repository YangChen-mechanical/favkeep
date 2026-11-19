use crate::{
    config::ARGS,
    platform::{Error, Result, valid_id},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn directory_name(title: &str, id: &str) -> String {
    let mut title = title
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .replace(|c: char| c.is_control() || "<>:\"/\\|?*".contains(c), "_");
    let stem = title
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|&prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if device {
        title.insert(0, '_');
    }
    title.truncate(title.floor_char_boundary(240 - id.len() - 3));
    let title = title.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let title = if title.is_empty() || title == id {
        "未命名"
    } else {
        title
    };
    format!("{title} [{id}]")
}

pub(crate) fn existing_directory(root: &Path, id: &str, saved: &str) -> Result<Option<PathBuf>> {
    if !valid_id(id) || !root.is_absolute() {
        return Err(Error::InvalidData);
    }
    let directory = if saved.is_empty() {
        root.join(id)
    } else {
        PathBuf::from(saved)
    };
    if !directory.is_absolute() {
        return Err(Error::InvalidData);
    }
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => return Err(Error::File),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::from(error)),
    }
    if !saved.is_empty() {
        return Ok(Some(directory));
    }
    for name in ["video.mp4", "video.part", "video.ready.json"] {
        match std::fs::symlink_metadata(directory.join(name)) {
            Ok(_) => return Ok(Some(directory)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::from(error)),
        }
    }
    Ok(None)
}

pub(crate) async fn record_directory(
    db: &DatabaseConnection,
    video_id: i32,
    root: &Path,
    id: &str,
    title: &str,
    author: &str,
) -> Result<PathBuf> {
    let directory = root.join(directory_name(title, id));
    match std::fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::from(error)),
        Ok(_) => return Err(Error::File),
    }
    let path = directory.to_str().ok_or(Error::InvalidData)?;
    let update = db.execute(Statement::from_sql_and_values(
        db.get_database_backend(),
        "UPDATE video SET path=?,name=coalesce(nullif(?,''),name),upper_name=coalesce(nullif(?,''),upper_name) WHERE id=? AND path='' AND NOT EXISTS (SELECT 1 FROM video WHERE path=?)",
        [path.into(), title.into(), author.into(), video_id.into(), path.into()],
    )).await.map_err(|_| Error::File)?;
    if update.rows_affected() != 1 {
        return Err(Error::File);
    }
    Ok(directory)
}

pub(crate) async fn ready_part(part: &Path, ready: &Path) -> bool {
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

pub(crate) fn available_space(path: &Path) -> Result<u64> {
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

pub(crate) struct MediaInfo {
    pub duration: u32,
    pub width: u32,
    pub height: u32,
    pub video_codec: Option<String>,
    pub mp4: bool,
}

fn ffprobe() -> PathBuf {
    ARGS.ffmpeg_path
        .as_ref()
        .map(|p| Path::new(p).with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }))
        .unwrap_or_else(|| "ffprobe".into())
}

pub(crate) async fn inspect(path: &Path) -> Result<MediaInfo> {
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

pub(crate) fn parse_media(info: &Value) -> Result<MediaInfo> {
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
        video_codec: video["codec_name"].as_str().map(str::to_owned),
        mp4: info["format"]["format_name"]
            .as_str()
            .is_some_and(|s| s.split(',').any(|x| x == "mp4")),
        duration: duration.ceil() as u32,
        width,
        height,
    })
}

pub(crate) async fn publish(part: &Path, ready: &Path, final_path: &Path) -> Result<()> {
    match tokio::fs::hard_link(part, final_path).await {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            inspect(final_path).await?;
        }
        Err(error) => return Err(Error::from(error)),
    }
    tokio::fs::remove_file(part).await.map_err(Error::from)?;
    let _ = tokio::fs::remove_file(ready).await;
    #[cfg(unix)]
    {
        let directory = final_path.parent().ok_or(Error::File)?.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
            .await
            .map_err(|_| Error::File)?
            .map_err(Error::from)?;
    }
    Ok(())
}

pub(crate) async fn discard_partial(part: &Path, ready: &Path) -> Result<()> {
    for path in [part, ready] {
        match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(Error::File);
                }
                tokio::fs::remove_file(path).await.map_err(Error::from)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::from(e)),
        }
    }
    Ok(())
}
#[cfg(test)]
pub(crate) async fn check_named_download(title: &str, id: &str, expected: &str) {
    let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared(
        "CREATE TABLE video(id INTEGER, path TEXT, name TEXT, upper_name TEXT);
         INSERT INTO video VALUES (1,'','','')",
    )
    .await
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    assert_eq!(existing_directory(root.path(), id, "").unwrap(), None);
    let directory = record_directory(&db, 1, root.path(), id, title, "").await.unwrap();
    assert_eq!(directory, root.path().join(expected));
    let saved: String = db
        .query_one(Statement::from_string(
            db.get_database_backend(),
            "SELECT path FROM video",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "path")
        .unwrap();
    assert_eq!(
        existing_directory(root.path(), id, &saved).unwrap().as_ref(),
        Some(&directory)
    );
    tokio::fs::create_dir(&directory).await.unwrap();
    let part = directory.join("video.part");
    let final_path = directory.join("video.mp4");
    tokio::fs::write(&part, b"sample").await.unwrap();
    publish(&part, &directory.join("video.ready.json"), &final_path)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(final_path).await.unwrap(), b"sample");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn restarted_transfer_cannot_inherit_an_old_completion_marker() {
        let root = tempfile::tempdir().unwrap();
        let part = root.path().join("video.part");
        let ready = root.path().join("video.ready.json");
        tokio::fs::write(&part, b"old").await.unwrap();
        tokio::fs::write(&ready, b"{\"bytes\":3}").await.unwrap();
        assert!(ready_part(&part, &ready).await);
        discard_partial(&part, &ready).await.unwrap();
        tokio::fs::write(&part, b"new").await.unwrap();
        assert!(!ready_part(&part, &ready).await);
    }
}

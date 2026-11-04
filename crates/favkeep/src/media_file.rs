use crate::{
    config::ARGS,
    platform::{Error, Result},
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
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

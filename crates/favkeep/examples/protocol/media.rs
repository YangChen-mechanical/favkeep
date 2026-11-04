use super::write;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

pub async fn publish(mut response: reqwest::Response, root: &Path, index: usize, ffprobe: &Path) -> Result<Value> {
    let expected = response
        .content_length()
        .filter(|n| *n > 0)
        .context("media content length missing")?;
    ensure!(expected < 2 * 1024 * 1024 * 1024, "probe media exceeds 2 GiB limit");
    let disk = sysinfo::Disks::new_with_refreshed_list();
    let canonical = root.canonicalize()?;
    let free = disk
        .iter()
        .filter_map(|d| {
            d.mount_point()
                .canonicalize()
                .ok()
                .filter(|m| canonical.starts_with(m))
                .map(|m| (m.as_os_str().len(), d.available_space()))
        })
        .max_by_key(|v| v.0)
        .map(|v| v.1)
        .context("disk space unavailable")?;
    ensure!(free > expected + 64 * 1024 * 1024, "insufficient disk space");
    let part = root.join(format!("sample-{:02}.part", index));
    let final_path = root.join(format!("sample-{:02}.mp4", index));
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&part).await?;
    let mut bytes = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("media read failed"))?
    {
        bytes += chunk.len() as u64;
        ensure!(bytes <= expected, "media exceeds declared length");
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    drop(file);
    ensure!(bytes == expected, "media length mismatch");
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(ffprobe)
            .args(["-v", "error", "-show_format", "-show_streams", "-of", "json"])
            .arg(&part)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(output.status.success(), "ffprobe failed");
    let info: Value = serde_json::from_slice(&output.stdout)?;
    let video = info["streams"]
        .as_array()
        .and_then(|v| v.iter().find(|s| s["codec_type"] == "video"))
        .context("video stream missing")?;
    let duration = info["format"]["duration"]
        .as_str()
        .and_then(|n| n.parse::<f64>().ok())
        .context("duration missing")?;
    ensure!(
        video["codec_name"] == "h264"
            && video["width"].as_u64().is_some_and(|n| n > 0)
            && video["height"].as_u64().is_some_and(|n| n > 0)
            && duration.is_finite()
            && duration > 0.0
            && info["format"]["format_name"]
                .as_str()
                .is_some_and(|s| s.split(',').any(|f| f == "mp4")),
        "media validation failed"
    );
    write(
        &root.join(format!("sample-{:02}-ready.json", index)),
        &serde_json::to_vec_pretty(&json!({"bytes":bytes,"validated":true}))?,
    )?;
    std::fs::hard_link(&part, &final_path)?;
    std::fs::remove_file(&part)?;
    #[cfg(unix)]
    {
        std::fs::File::open(root)?.sync_all()?;
    }
    let report = json!({"sample":index,"bytes":bytes,"duration":duration,"codec":"h264","width":video["width"],"height":video["height"],"audio_codecs":info["streams"].as_array().unwrap().iter().filter(|s|s["codec_type"]=="audio").map(|s|s["codec_name"].clone()).collect::<Vec<_>>()});

    Ok(report)
}

use super::xhs::Session;
use super::{Platform, UA, request, trusted, write};
use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url, header, redirect::Policy};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

fn initial_state(html: &str) -> Result<Value> {
    let marker = regex::Regex::new(r"window\.__INITIAL_STATE__\s*=\s*")?;
    let bytes = &html.as_bytes()[marker.find(html).context("initial state missing")?.end()..];
    ensure!(bytes.first() == Some(&b'{'), "initial state is not an object");
    let (mut quoted, mut escape, mut depth, mut i) = (false, false, 0usize, 0usize);
    let mut json = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        // shortcut: normalize only observed state literals; add fixture-backed cases if serialization changes.
        if !quoted && bytes[i..].starts_with(b"new Set([])") {
            json.extend(b"[]");
            i += 11;
            continue;
        }
        if !quoted
            && bytes[i..].starts_with(b"undefined")
            && bytes.get(i + 9).is_some_and(|c| b",}] \r\n\t".contains(c))
        {
            json.extend(b"null");
            i += 9;
            continue;
        }
        json.push(b);
        if quoted {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(serde_json::from_slice(&json)?);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    anyhow::bail!("initial state incomplete")
}

fn media_url(raw: &str) -> Result<Url> {
    let mut url = Url::parse(raw)?;
    if url.scheme() == "http" {
        url.set_scheme("https")
            .map_err(|_| anyhow::anyhow!("invalid media scheme"))?;
    }
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && url.host_str().is_some_and(|h| [
                "sns-video-bd.xhscdn.com",
                "sns-v11.rednotecdn.com",
                "sns-v14.rednotecdn.com",
                "sns-v27.rednotecdn.com",
                "sns-v28.rednotecdn.com"
            ]
            .contains(&h)),
        "untrusted media URL"
    );
    Ok(url)
}

fn best_video(note: &Value) -> Result<Url> {
    let streams = note["video"]["media"]["stream"]
        .as_object()
        .context("video streams missing")?;
    let selected = streams
        .iter()
        .flat_map(|(codec, values)| {
            values.as_array().into_iter().flatten().filter_map(move |s| {
                let kind = s["videoCodec"]
                    .as_str()
                    .or_else(|| s["video_codec"].as_str())
                    .unwrap_or(codec);
                if !["h264", "avc", "avc1", "EF4"].contains(&kind) || (kind == "EF4" && s["format"] != "mp4") {
                    return None;
                }
                let raw = s["masterUrl"].as_str().or_else(|| s["master_url"].as_str())?;
                Some(((s["height"].as_u64()?, s["width"].as_u64()?), raw))
            })
        })
        .max_by_key(|(rank, _)| *rank)
        .context("H264 MP4 stream missing")?;
    media_url(selected.1)
}

pub async fn download(session: &Session, samples_file: Option<&Path>, ffprobe: &Path) -> Result<()> {
    let root = &session.root;
    let window: Value = serde_json::from_slice(&std::fs::read(root.join("window-private.json"))?)?;
    ensure!(window["window_complete"] == true, "verified window required");
    let all = window["items"].as_array().context("window items missing")?;
    let ids: Vec<String> = if let Some(path) = samples_file {
        serde_json::from_slice(&std::fs::read(path)?)?
    } else {
        all.iter()
            .filter(|v| v["kind"] == "video")
            .take(3)
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .collect()
    };
    ensure!(
        ids.len() == 3 && ids.iter().collect::<HashSet<_>>().len() == 3,
        "three distinct sample IDs required"
    );
    ensure!(
        ids.iter()
            .all(|id| all.iter().any(|v| v["id"] == *id && v["kind"] == "video")),
        "sample must be a video in the current verified window"
    );
    write(&root.join("sample-ids-private.json"), &serde_json::to_vec_pretty(&ids)?)?;
    let media = Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(20))
        .redirect(Policy::custom(|a| {
            if a.previous().len() < 3 && media_url(a.url().as_str()).is_ok() {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()?;
    let mut reports = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let token = all
            .iter()
            .find(|v| v["id"] == *id)
            .and_then(|v| v["token"].as_str())
            .context("sample access token missing")?;
        let mut url = Url::parse(&format!("https://www.rednote.com/explore/{id}"))?;
        url.query_pairs_mut()
            .append_pair("xsec_token", token)
            .append_pair("xsec_source", "pc_user");
        let expected = id.clone();
        let document = Client::builder()
            .user_agent(UA)
            .cookie_provider(session.jar.clone())
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .redirect(Policy::custom(move |a| {
                if a.previous().len() < 3
                    && trusted(Platform::Xiaohongshu, a.url())
                    && a.url().host_str() == Some("www.rednote.com")
                    && [format!("/explore/{expected}"), format!("/discovery/item/{expected}")]
                        .contains(&a.url().path().to_owned())
                {
                    a.follow()
                } else {
                    a.stop()
                }
            }))
            .build()?;
        let stage = format!("document-{:02}", index + 1);
        request(
            &document,
            Platform::Xiaohongshu,
            document.get(url).header(header::REFERER, "https://www.rednote.com/"),
            &stage,
            root,
        )
        .await?;
        let html = std::fs::read_to_string(root.join(format!("{stage}-body.bin")))?;
        let state = initial_state(&html)?;
        let note = &state["note"]["noteDetailMap"][id]["note"];
        ensure!(
            note["noteId"] == *id || note["note_id"] == *id,
            "detail identity mismatch"
        );
        let url = best_video(note)?;
        let mut response = media
            .get(url)
            .header(header::REFERER, "https://www.rednote.com/")
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("media transport failed"))?;
        ensure!(response.status().as_u16() == 200, "media HTTP failed");
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
        let part = root.join(format!("sample-{:02}.part", index + 1));
        let final_path = root.join(format!("sample-{:02}.mp4", index + 1));
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
            &root.join(format!("sample-{:02}-ready.json", index + 1)),
            &serde_json::to_vec_pretty(&json!({"bytes":bytes,"validated":true}))?,
        )?;
        std::fs::hard_link(&part, &final_path)?;
        std::fs::remove_file(&part)?;
        #[cfg(unix)]
        {
            std::fs::File::open(root)?.sync_all()?;
        }
        let report = json!({"sample":index+1,"bytes":bytes,"duration":duration,"codec":"h264","width":video["width"],"height":video["height"],"audio_codecs":info["streams"].as_array().unwrap().iter().filter(|s|s["codec_type"]=="audio").map(|s|s["codec_name"].clone()).collect::<Vec<_>>()});
        println!("{}", report);
        reports.push(report);
    }
    write(&root.join("downloads.json"), &serde_json::to_vec_pretty(&reports)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_state_is_parsed_without_executing_scripts_or_altering_strings() {
        let html = r#"<script>window.__INITIAL_STATE__ = {"note":{"x":undefined},"unused":new Set([]),"title":"undefined } and \" braces"};alert('ignored')</script>"#;
        let v = initial_state(html).unwrap();
        assert!(v["note"]["x"].is_null());
        assert_eq!(v["unused"], json!([]));
        assert_eq!(v["title"], "undefined } and \" braces");
        assert!(initial_state("window.__INITIAL_STATE__={broken").is_err());
        for url in [
            "https://sns-v11.rednotecdn.com.evil.test/video",
            "http://user@sns-v11.rednotecdn.com/video",
            "https://sns-v11.rednotecdn.com:444/video",
        ] {
            assert!(media_url(url).is_err());
        }
    }
}

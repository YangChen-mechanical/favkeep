use super::{Passport, sign};
use crate::{douyin_media, media, write};
use anyhow::{Context, Result, ensure};
use reqwest::{Url, header, redirect::Policy};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, time::Duration};

fn sample_membership(ids: &[String], collections: &[Value]) -> Result<()> {
    ensure!(
        ids.len() == 3 && ids.iter().collect::<HashSet<_>>().len() == 3,
        "three_distinct_sample_IDs_required"
    );
    ensure!(
        ids.iter().all(|id| collections
            .iter()
            .any(|folder| folder["items"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["aweme_id"].as_str() == Some(id) && !douyin_media::unsupported(item))
            }))),
        "sample_not_in_verified_selected_folders"
    );
    Ok(())
}

fn sample_urls(id: &str, payload: &Value) -> Result<Vec<String>> {
    let detail = &payload["aweme_detail"];
    ensure!(
        (detail["aweme_id"].as_str() == Some(id)
            || detail["aweme_id"].as_u64().is_some_and(|value| value.to_string() == id))
            && !douyin_media::unsupported(detail),
        "sample_detail_identity_or_kind_mismatch"
    );
    let urls = douyin_media::video_urls(&detail["video"]);
    ensure!(!urls.is_empty(), "sample_h264_source_missing");
    Ok(urls)
}

fn media_url(raw: &str) -> Result<Url> {
    let url = douyin_media::media_url(raw).context("untrusted_media_url")?;
    ensure!(
        url.host_str() == Some("v26-web.douyinvod.com"),
        "unconfirmed_media_host"
    );
    Ok(url)
}

async fn media_response(client: &reqwest::Client, raw: &str, root: &Path, index: usize) -> Result<reqwest::Response> {
    let mut url = media_url(raw)?;
    for hop in 0..6 {
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("media_transport_failed"))?;
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok());
        write(
            &root.join(format!("media-{index:02}-{hop:02}-private.json")),
            &serde_json::to_vec_pretty(
                &json!({"url":url.as_str(),"http_status":response.status().as_u16(),"location":location}),
            )?,
        )?;
        if response.status().is_redirection() {
            let next = url.join(location.context("media_redirect_missing")?)?;
            url = media_url(next.as_str())?;
            continue;
        }
        ensure!(response.status() == reqwest::StatusCode::OK, "media_http_rejected");
        return Ok(response);
    }
    anyhow::bail!("media_redirect_limit")
}

impl Passport<'_> {
    pub(super) async fn download_three(&mut self, args: &crate::Args, collections: &[Value]) -> Result<()> {
        let ids: Vec<String> = if let Some(path) = &args.sample_ids_file {
            let bytes = std::fs::read(path)?;
            ensure!(bytes.len() <= 16 * 1024, "sample_ID_file_too_large");
            serde_json::from_slice(&bytes)?
        } else {
            let mut known = HashSet::new();
            collections
                .iter()
                .flat_map(|folder| folder["items"].as_array().into_iter().flatten())
                .filter(|item| !douyin_media::unsupported(item))
                .filter_map(|item| item["aweme_id"].as_str())
                .filter(|id| known.insert(*id))
                .take(3)
                .map(str::to_owned)
                .collect()
        };
        sample_membership(&ids, collections)?;
        write(
            &self.root.join("sample-ids-private.json"),
            &serde_json::to_vec_pretty(&ids)?,
        )?;
        let media_client = reqwest::Client::builder()
            .cookie_store(false)
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .redirect(Policy::none())
            .user_agent(sign::UA)
            .default_headers(header::HeaderMap::from_iter([(
                header::REFERER,
                header::HeaderValue::from_static("https://www.douyin.com/"),
            )]))
            .build()?;
        let mut reports = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let detail = self
                .main_get(
                    "/aweme/v1/web/aweme/detail/",
                    &[("aweme_id", id.clone())],
                    &format!("detail-sample-{:02}", index + 1),
                )
                .await?;
            let urls = sample_urls(id, &detail)?;
            let response = media_response(&media_client, &urls[0], self.root, index + 1).await?;
            let host = response.url().host_str().unwrap_or_default().to_owned();
            let mut report = media::publish(response, self.root, index + 1, &args.ffprobe).await?;
            report["source_host"] = json!(host);
            println!("{}", report);
            reports.push(report);
        }
        write(&self.root.join("downloads.json"), &serde_json::to_vec_pretty(&reports)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_must_be_distinct_videos_in_selected_folders() {
        let folders =
            [json!({"items":[{"aweme_id":"1"},{"aweme_id":"2"},{"aweme_id":"3"},{"aweme_id":"4","aweme_type":68}]})];
        assert!(sample_membership(&["1".into(), "2".into(), "3".into()], &folders).is_ok());
        for ids in [["1", "1", "3"], ["1", "2", "missing"], ["1", "2", "4"]] {
            assert!(sample_membership(&ids.map(str::to_owned), &folders).is_err());
        }
        assert!(sample_urls("1", &json!({"aweme_detail":{"aweme_id":"2","video":{}}})).is_err());
        assert!(sample_urls("1", &json!({"aweme_detail":{"aweme_id":"1","images":[{}]}})).is_err());
    }
    #[test]
    fn media_hosts_are_confirmed_https_targets_without_credentials() {
        assert!(media_url("https://v26-web.douyinvod.com/video").is_ok());
        for url in [
            "http://v26-web.douyinvod.com/video",
            "https://v26-web.douyinvod.com.evil.test/video",
            "https://127.0.0.1/video",
            "https://user@v26-web.douyinvod.com/video",
            "https://v26-web.douyinvod.com:444/video",
            "https://unconfirmed.douyinvod.com/video",
        ] {
            assert!(media_url(url).is_err());
        }
    }
}

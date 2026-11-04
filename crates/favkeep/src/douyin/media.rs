use std::collections::HashSet;

use reqwest::Url;
use serde_json::Value;

pub(crate) fn unsupported(row: &Value) -> bool {
    row["aweme_type"].as_u64() == Some(68) || row["images"].as_array().is_some_and(|images| !images.is_empty())
}

pub(crate) fn media_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let host = url.domain()?;
    let official = [
        "douyin.com",
        "douyinvod.com",
        "zjcdn.com",
        "pstatp.com",
        "snssdk.com",
        "bytecdn.cn",
        "bytedance.com",
        "byteimg.com",
    ]
    .iter()
    .any(|domain| host == *domain || host.strip_suffix(domain).is_some_and(|prefix| prefix.ends_with('.')));
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && official)
        .then_some(url)
}

pub fn video_urls(video: &Value) -> Vec<String> {
    let best = video["bit_rate"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|rate| {
            rate["is_h265"].as_u64() == Some(0)
                && rate["play_addr"]["url_list"]
                    .as_array()
                    .is_some_and(|urls| !urls.is_empty())
        })
        .max_by_key(|rate| {
            let addr = &rate["play_addr"];
            (
                addr["width"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_mul(addr["height"].as_u64().unwrap_or(0)),
                rate["bit_rate"].as_u64().unwrap_or(0),
            )
        });
    let addr = best.map(|rate| &rate["play_addr"]).unwrap_or_else(|| {
        if video["play_addr_h264"]["url_list"]
            .as_array()
            .is_some_and(|urls| !urls.is_empty())
        {
            &video["play_addr_h264"]
        } else {
            &video["play_addr"]
        }
    });
    let mut seen = HashSet::new();
    addr["url_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|url| media_url(url).is_some() && seen.insert(*url))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_posts_never_become_video_samples() {
        assert!(unsupported(&serde_json::json!({"aweme_type":68})));
        assert!(unsupported(&serde_json::json!({"aweme_type":0,"images":[{}]})));
        assert!(!unsupported(&serde_json::json!({"aweme_type":0,"images":null})));
    }
}

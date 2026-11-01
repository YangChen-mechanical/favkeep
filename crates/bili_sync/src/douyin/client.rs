use std::{collections::HashSet, time::Duration};

use reqwest::{header, redirect::Policy};
use serde::Serialize;
use serde_json::Value;

pub use super::media::video_urls;
use super::{
    Error, Result,
    browser::Auth,
    media::{media_url, unsupported},
    valid_id,
};

#[derive(Debug, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub total: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
}

#[derive(Serialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub author: String,
    pub urls: Vec<String>,
}

pub struct Client {
    api: reqwest::Client,
    media: reqwest::Client,
    ua: String,
    ms_token: String,
    fingerprint: String,
}

impl Client {
    pub async fn new(auth: &Auth) -> Result<Self> {
        if !valid_id(&auth.identity)
            || auth.origin != "https://www.douyin.com"
            || !bounded_ascii(&auth.ua, 1024)
            || !bounded_ascii(&auth.ms_token, 4096)
            || !bounded_ascii(&auth.fingerprint, 1024)
        {
            return Err(Error::InvalidData);
        }
        let mut headers = header::HeaderMap::new();
        headers.insert(header::USER_AGENT, auth.ua.parse().map_err(|_| Error::InvalidData)?);
        headers.insert(
            header::REFERER,
            header::HeaderValue::from_static("https://www.douyin.com/"),
        );
        let media = reqwest::Client::builder()
            .default_headers(headers.clone())
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .redirect(Policy::none())
            .build()
            .map_err(|_| Error::Network)?;
        let cookie_header = auth.cookie_header()?;
        // shortcut: pinned F2 uses an Argus placeholder, capture real browser headers if the gateway starts validating it.
        headers.insert("x-tt-argus", header::HeaderValue::from_static("1"));
        let uifid = |name| {
            cookie_header
                .split("; ")
                .filter_map(|part| part.split_once('='))
                .find_map(|(key, value)| (key == name && !value.is_empty()).then_some(value))
        };
        if let Some(value) = uifid("UIFID").or_else(|| uifid("UIFID_TEMP")) {
            let mut value = header::HeaderValue::from_str(value).map_err(|_| Error::InvalidData)?;
            value.set_sensitive(true);
            headers.insert("uifid", value);
        }
        let mut cookie = header::HeaderValue::from_str(&cookie_header).map_err(|_| Error::InvalidData)?;
        cookie.set_sensitive(true);
        headers.insert(header::COOKIE, cookie);
        let api = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .redirect(Policy::none())
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            api,
            media,
            ua: auth.ua.clone(),
            ms_token: auth.ms_token.clone(),
            fingerprint: auth.fingerprint.clone(),
        })
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value> {
        let query = base_query(&self.ms_token, params)?;
        let signature = super::sign::sign(&query, "", &self.ua, &self.fingerprint);
        let encoded_signature =
            serde_urlencoded::to_string([("a_bogus", signature)]).map_err(|_| Error::InvalidData)?;
        let mut response = self
            .api
            .get(format!("https://www.douyin.com{path}?{query}&{encoded_signature}"))
            .send()
            .await
            .map_err(|_| Error::Network)?;
        http_status(response.status())?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
            if chunk.len() > 8 * 1024 * 1024 - body.len() {
                return Err(Error::InvalidData);
            }
            body.extend_from_slice(&chunk);
        }
        let payload: Value = serde_json::from_slice(&body).map_err(|_| Error::InvalidData)?;
        tracing::debug!(endpoint = path, keys = ?payload.as_object().map(|p| p.keys().collect::<Vec<_>>()), "received API shape");
        business_status(&payload)?;
        Ok(payload)
    }

    async fn pages(&self, path: &str, key: &str, folder: Option<&Folder>) -> Result<Vec<Value>> {
        let mut scan = Scan::default();
        for _ in 0..10_000 {
            let mut params = vec![("cursor", scan.cursor.to_string()), ("count", "20".into())];
            if let Some(folder) = folder {
                params.push(("collects_id", folder.id.clone()));
            }
            let payload = self.get(path, &params).await?;
            let done = scan.push(&payload, key, folder.is_some_and(|f| f.total == Some(0)))?;
            if done {
                return scan.finish(folder.and_then(|f| f.total));
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        Err(Error::Incomplete)
    }

    pub async fn folders(&self) -> Result<Vec<Folder>> {
        self.pages("/aweme/v1/web/collects/list/", "collects_list", None)
            .await?
            .into_iter()
            .map(|row| {
                Ok(Folder {
                    id: record_id(&row, "collects_id")?,
                    name: row["collects_name"].as_str().ok_or(Error::InvalidData)?.to_owned(),
                    total: match row.get("total_number") {
                        Some(Value::Null) | None => None,
                        Some(value) => Some(value.as_u64().ok_or(Error::InvalidData)?),
                    },
                })
            })
            .collect()
    }

    pub async fn scan_folder(&self, folder: &Folder) -> Result<Vec<Item>> {
        if !valid_id(&folder.id) {
            return Err(Error::InvalidData);
        }
        self.pages("/aweme/v1/web/collects/video/list/", "aweme_list", Some(folder))
            .await?
            .into_iter()
            .map(|row| {
                Ok(Item {
                    id: record_id(&row, "aweme_id")?,
                    kind: if unsupported(&row) { "unsupported" } else { "video" }.into(),
                })
            })
            .collect()
    }

    pub async fn detail(&self, id: &str) -> Result<Video> {
        if !valid_id(id) {
            return Err(Error::InvalidData);
        }
        let payload = self
            .get("/aweme/v1/web/aweme/detail/", &[("aweme_id", id.to_owned())])
            .await?;
        let detail = &payload["aweme_detail"];
        if record_id(detail, "aweme_id").ok().as_deref() != Some(id) || unsupported(detail) {
            return Err(Error::MediaUnavailable);
        }
        let urls = video_urls(&detail["video"]);
        if urls.is_empty() {
            return Err(Error::MediaUnavailable);
        }
        Ok(Video {
            id: id.to_owned(),
            title: detail["desc"].as_str().unwrap_or_default().to_owned(),
            author: detail["author"]["nickname"].as_str().unwrap_or_default().to_owned(),
            urls,
        })
    }

    pub async fn media_response(&self, url: &str) -> Result<reqwest::Response> {
        let mut url = media_url(url).ok_or(Error::InvalidData)?;
        for _ in 0..6 {
            let response = self.media.get(url.clone()).send().await.map_err(|_| Error::Network)?;
            if response.status().is_redirection() {
                let next = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|location| url.join(location).ok())
                    .ok_or(Error::Rejected)?;
                url = media_url(next.as_str()).ok_or(Error::Rejected)?;
                continue;
            }
            http_status(response.status())?;
            return Ok(response);
        }
        Err(Error::Rejected)
    }
}

fn base_query(ms_token: &str, params: &[(&str, String)]) -> Result<String> {
    // Keep the pinned BaseRequestModel order: these exact encoded bytes are signed and sent.
    let mut query: Vec<(&str, &str)> = vec![
        ("device_platform", "webapp"),
        ("aid", "6383"),
        ("channel", "channel_pc_web"),
        ("pc_client_type", "1"),
        ("publish_video_strategy_type", "2"),
        ("pc_libra_divert", "Windows"),
        ("version_code", "290100"),
        ("version_name", "29.1.0"),
        ("cookie_enabled", "true"),
        ("screen_width", "1920"),
        ("screen_height", "1080"),
        ("browser_language", "zh-CN"),
        ("browser_platform", "Win32"),
        ("browser_name", "Edge"),
        ("browser_version", "130.0.0.0"),
        ("browser_online", "true"),
        ("engine_name", "Blink"),
        ("engine_version", "130.0.0.0"),
        ("os_name", "Windows"),
        ("os_version", "10"),
        ("cpu_core_num", "12"),
        ("device_memory", "8"),
        ("platform", "PC"),
        ("downlink", "10"),
        ("effective_type", "4g"),
        ("round_trip_time", "100"),
        ("msToken", ms_token),
    ];
    query.extend(params.iter().map(|(key, value)| (*key, value.as_str())));
    serde_urlencoded::to_string(query).map_err(|_| Error::InvalidData)
}

fn http_status(status: reqwest::StatusCode) -> Result<()> {
    match status.as_u16() {
        200..=299 => Ok(()),
        500..=599 => Err(Error::Network),
        _ => Err(Error::Rejected),
    }
}

fn bounded_ascii(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| (32..127).contains(&b))
}

fn business_status(payload: &Value) -> Result<()> {
    match payload.get("status_code").and_then(Value::as_i64) {
        Some(0) => Ok(()),
        Some(_) => Err(Error::Rejected),
        None => Err(Error::InvalidData),
    }
}

pub fn page_data(payload: &Value, cursor: u64, key: &str, verified_empty: bool) -> Result<(Vec<Value>, bool, u64)> {
    business_status(payload)?;
    let more = match payload.get("has_more") {
        Some(Value::Bool(more)) => *more,
        Some(value) if value.as_u64() == Some(0) => false,
        Some(value) if value.as_u64() == Some(1) => true,
        _ => return Err(Error::InvalidData),
    };
    let next = payload
        .get("cursor")
        .and_then(Value::as_u64)
        .ok_or(Error::InvalidData)?;
    let rows = match payload.get(key) {
        Some(Value::Array(rows)) => rows.clone(),
        Some(Value::Null) if verified_empty && !more => vec![],
        _ => return Err(Error::InvalidData),
    };
    if more && (next <= cursor || rows.is_empty()) {
        return Err(Error::Incomplete);
    }
    Ok((rows, more, next))
}

#[derive(Default)]
struct Scan {
    cursor: u64,
    known: HashSet<String>,
    items: Vec<Value>,
    reported_total: Option<u64>,
}

impl Scan {
    fn finish(self, total: Option<u64>) -> Result<Vec<Value>> {
        if [total, self.reported_total]
            .into_iter()
            .flatten()
            .any(|total| total != self.items.len() as u64)
        {
            return Err(Error::Incomplete);
        }
        Ok(self.items)
    }

    fn push(&mut self, payload: &Value, key: &str, verified_empty: bool) -> Result<bool> {
        let (rows, more, next) = page_data(payload, self.cursor, key, verified_empty)?;
        if key == "collects_list"
            && let Some(value) = payload.get("total_number")
        {
            let total = value.as_u64().ok_or(Error::InvalidData)?;
            if self.reported_total.is_some_and(|previous| previous != total) {
                return Err(Error::Incomplete);
            }
            self.reported_total = Some(total);
        }
        let id_key = if key == "aweme_list" { "aweme_id" } else { "collects_id" };
        let ids: Vec<String> = rows.iter().map(|row| record_id(row, id_key)).collect::<Result<_>>()?;
        if !ids.is_empty() && ids.iter().all(|id| self.known.contains(id)) {
            return Err(Error::Incomplete);
        }
        for (row, id) in rows.into_iter().zip(ids) {
            if self.known.insert(id) {
                self.items.push(row);
            }
        }
        self.cursor = next;
        Ok(!more)
    }
}

fn record_id(row: &Value, key: &str) -> Result<String> {
    let id = match row.get(key) {
        Some(Value::String(id)) => id.clone(),
        Some(value) => value.as_u64().ok_or(Error::InvalidData)?.to_string(),
        None => return Err(Error::InvalidData),
    };
    if !valid_id(&id) {
        return Err(Error::InvalidData);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn page_requires_business_success_and_a_real_terminal_signal() {
        let page = json!({"status_code":0,"aweme_list":[{"aweme_id":"12"}],"has_more":0,"cursor":10});
        assert_eq!(page_data(&page, 0, "aweme_list", false).unwrap().0.len(), 1);
        assert!(page_data(&json!({"status_code":0,"aweme_list":[]}), 0, "aweme_list", false).is_err());
        assert_eq!(
            page_data(&json!({"status_code":8}), 0, "aweme_list", false),
            Err(Error::Rejected)
        );
    }

    #[test]
    fn ambiguous_http_statuses_do_not_claim_login_expiry_or_captcha() {
        for status in [301, 401, 403, 406, 429, 461] {
            assert_eq!(
                http_status(reqwest::StatusCode::from_u16(status).unwrap()),
                Err(Error::Rejected)
            );
        }
        assert_eq!(
            http_status(reqwest::StatusCode::SERVICE_UNAVAILABLE),
            Err(Error::Network)
        );
        assert!(http_status(reqwest::StatusCode::OK).is_ok());
    }

    #[tokio::test]
    async fn client_rejects_unsafe_signer_inputs_before_building_requests() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let auth = Auth {
            identity: "self_123".into(),
            ua: "test-agent".into(),
            origin: "https://www.douyin.com".into(),
            ms_token: "token".into(),
            fingerprint: "1920|1080".into(),
            cookies: vec![json!({"domain":".douyin.com","name":"sessionid","value":"secret"})],
        };
        assert!(Client::new(&auth).await.is_ok());
        for bad in [
            "".to_owned(),
            "非ASCII".to_owned(),
            "x".repeat(4097),
            "bad\nheader".to_owned(),
        ] {
            for field in ["ua", "ms_token", "fingerprint"] {
                let mut auth = auth.clone();
                match field {
                    "ua" => auth.ua = bad.clone(),
                    "ms_token" => auth.ms_token = bad.clone(),
                    _ => auth.fingerprint = bad.clone(),
                }
                assert!(matches!(Client::new(&auth).await, Err(Error::InvalidData)));
            }
        }
    }

    #[test]
    fn stalled_cursor_or_unproven_empty_is_not_a_complete_scan() {
        let page = json!({"status_code":0,"aweme_list":[{"aweme_id":"12"}],"has_more":1,"cursor":0});
        assert_eq!(page_data(&page, 0, "aweme_list", false), Err(Error::Incomplete));
        let empty = json!({"status_code":0,"aweme_list":null,"has_more":0,"cursor":0});
        assert!(page_data(&empty, 0, "aweme_list", false).is_err());
        assert!(page_data(&empty, 0, "aweme_list", true).unwrap().0.is_empty());
    }

    #[test]
    fn prefers_best_avc_source_and_never_returns_insecure_or_local_urls() {
        let v = json!({"bit_rate":[
            {"is_h265":1,"bit_rate":9000,"play_addr":{"width":3840,"height":2160,"url_list":["https://video.douyin.com/hevc"]}},
            {"is_h265":0,"bit_rate":6000,"play_addr":{"width":1920,"height":1080,"url_list":["https://video.douyin.com/avc","file:///secret","http://video.douyin.com/plain","https://127.0.0.1/x"]}}
        ]});
        assert_eq!(video_urls(&v), vec!["https://video.douyin.com/avc"]);
    }

    #[test]
    fn pages_deduplicate_overlap_and_check_the_unique_folder_total() {
        let mut scan = Scan::default();
        assert!(
            !scan
                .push(
                    &json!({"status_code":0,"aweme_list":[{"aweme_id":"1"},{"aweme_id":"2"}],"has_more":1,"cursor":20}),
                    "aweme_list",
                    false
                )
                .unwrap()
        );
        assert!(
            scan.push(
                &json!({"status_code":0,"aweme_list":[{"aweme_id":"2"},{"aweme_id":"3"}],"has_more":0,"cursor":20}),
                "aweme_list",
                false
            )
            .unwrap()
        );
        assert_eq!(scan.finish(Some(3)).unwrap().len(), 3);

        let mut truncated = Scan::default();
        truncated
            .push(
                &json!({"status_code":0,"aweme_list":[{"aweme_id":"1"}],"has_more":0,"cursor":0}),
                "aweme_list",
                false,
            )
            .unwrap();
        assert_eq!(truncated.finish(Some(2)), Err(Error::Incomplete));
        assert!(Scan::default().finish(Some(0)).unwrap().is_empty());
    }

    #[test]
    fn folder_list_checks_reported_total_even_when_only_earlier_pages_supply_it() {
        let first =
            json!({"status_code":0,"collects_list":[{"collects_id":"1"}],"has_more":1,"cursor":20,"total_number":3});
        let last = json!({"status_code":0,"collects_list":[{"collects_id":"2"}],"has_more":0,"cursor":20});
        let mut scan = Scan::default();
        assert!(!scan.push(&first, "collects_list", false).unwrap());
        assert!(scan.push(&last, "collects_list", false).unwrap());
        assert_eq!(scan.finish(None), Err(Error::Incomplete));

        let mut scan = Scan::default();
        let mut complete_first = first.clone();
        complete_first["total_number"] = json!(2);
        scan.push(&complete_first, "collects_list", false).unwrap();
        scan.push(&last, "collects_list", false).unwrap();
        assert_eq!(scan.finish(None).unwrap().len(), 2);
    }

    #[test]
    fn folder_list_rejects_malformed_or_changing_totals() {
        for total in [json!(null), json!("1"), json!(-1), json!(1.5), json!(true)] {
            let page = json!({"status_code":0,"collects_list":[{"collects_id":"1"}],"has_more":0,"cursor":0,"total_number":total});
            assert_eq!(
                Scan::default().push(&page, "collects_list", false),
                Err(Error::InvalidData)
            );
        }
        let mut scan = Scan::default();
        scan.push(
            &json!({"status_code":0,"collects_list":[{"collects_id":"1"}],"has_more":1,"cursor":20,"total_number":2}),
            "collects_list",
            false,
        )
        .unwrap();
        assert_eq!(scan.push(&json!({"status_code":0,"collects_list":[{"collects_id":"2"}],"has_more":0,"cursor":20,"total_number":3}), "collects_list", false), Err(Error::Incomplete));
    }

    #[test]
    fn external_integer_ids_keep_exact_decimal_digits_and_reject_lossy_shapes() {
        let numeric: Value = serde_json::from_str(r#"{"collects_id":9007199254740993}"#).unwrap();
        assert_eq!(record_id(&numeric, "collects_id").unwrap(), "9007199254740993");
        assert_eq!(
            record_id(&json!({"collects_id":"self_123"}), "collects_id").unwrap(),
            "self_123"
        );
        for id in [json!(-1), json!(1.5), json!(true), json!(null)] {
            assert!(record_id(&json!({"collects_id":id}), "collects_id").is_err());
        }
    }

    #[test]
    fn repeats_are_rejected_even_if_the_cursor_advances_or_the_page_claims_terminal() {
        for more in [0, 1] {
            let mut scan = Scan::default();
            scan.push(
                &json!({"status_code":0,"aweme_list":[{"aweme_id":"1"}],"has_more":1,"cursor":20}),
                "aweme_list",
                false,
            )
            .unwrap();
            assert_eq!(
                scan.push(
                    &json!({"status_code":0,"aweme_list":[{"aweme_id":"1"}],"has_more":more,"cursor":40}),
                    "aweme_list",
                    false
                ),
                Err(Error::Incomplete)
            );
        }
        let backwards = json!({"status_code":0,"aweme_list":[{"aweme_id":"2"}],"has_more":1,"cursor":10});
        assert_eq!(page_data(&backwards, 20, "aweme_list", false), Err(Error::Incomplete));
    }

    #[test]
    fn malformed_signals_and_ids_never_finish_a_scan() {
        for page in [
            json!({"status_code":"0","aweme_list":[],"has_more":0,"cursor":0}),
            json!({"status_code":0,"aweme_list":[],"has_more":"0","cursor":0}),
            json!({"status_code":0,"aweme_list":[],"has_more":2,"cursor":0}),
            json!({"status_code":0,"aweme_list":[],"has_more":0,"cursor":-1}),
            json!({"status_code":0,"aweme_list":[{"aweme_id":"../secret"}],"has_more":0,"cursor":0}),
        ] {
            assert!(Scan::default().push(&page, "aweme_list", false).is_err());
        }
    }

    #[test]
    fn query_keeps_pinned_order_and_encodes_token_and_ids_once() {
        let query = base_query(
            "a+b/c=",
            &[
                ("cursor", "0".into()),
                ("count", "20".into()),
                ("collects_id", "id_1".into()),
            ],
        )
        .unwrap();
        assert!(query.starts_with("device_platform=webapp&aid=6383&channel=channel_pc_web&pc_client_type=1&publish_video_strategy_type=2&pc_libra_divert=Windows&"));
        assert!(query.ends_with("&msToken=a%2Bb%2Fc%3D&cursor=0&count=20&collects_id=id_1"));
        assert_eq!(query.matches("msToken=").count(), 1);
    }

    #[test]
    fn media_hosts_are_exact_suffixes_with_https_and_no_credentials_or_other_ports() {
        for url in [
            "https://v3-web.douyinvod.com/x",
            "https://v5-dy-o.zjcdn.com/x",
            "https://video.douyin.com/x",
        ] {
            assert!(media_url(url).is_some());
        }
        for url in [
            "https://douyin.com.evil.test/x",
            "https://evildouyin.com/x",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "http://video.douyin.com/x",
            "file:///x",
            "https://user@video.douyin.com/x",
            "https://video.douyin.com:444/x",
        ] {
            assert!(media_url(url).is_none());
        }
    }

    #[test]
    fn avc_quality_uses_area_before_bitrate_and_uses_official_fallback() {
        let video = json!({"bit_rate":[
            {"is_h265":0,"bit_rate":9000,"play_addr":{"width":1920,"height":1080,"url_list":["https://video.douyin.com/1080"]}},
            {"is_h265":0,"bit_rate":3000,"play_addr":{"width":2560,"height":1440,"url_list":["https://video.douyin.com/1440"]}},
            {"is_h265":0,"bit_rate":5000,"play_addr":{"width":2560,"height":1440,"url_list":["https://video.douyin.com/best"]}}
        ]});
        assert_eq!(video_urls(&video), vec!["https://video.douyin.com/best"]);
        assert_eq!(
            video_urls(&json!({"play_addr":{"url_list":["https://video.douyin.com/fallback"]}})),
            vec!["https://video.douyin.com/fallback"]
        );
    }
}

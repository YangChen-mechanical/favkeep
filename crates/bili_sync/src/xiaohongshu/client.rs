use super::{Error, Result, cookies::Jar, sign, state::initial_state, valid_id};
use reqwest::{Client, Url, header, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

const WEB_HOSTS: [&str; 2] = ["www.xiaohongshu.com", "www.rednote.com"];
const API_HOSTS: [&str; 4] = [
    "www.xiaohongshu.com",
    "edith.xiaohongshu.com",
    "www.rednote.com",
    "webapi.rednote.com",
];
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0";
const ME: &str = "/api/sns/web/v2/user/me";
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Auth {
    pub identity: String,
    pub origin: String,
    pub ua: String,
    pub cookies: Vec<Value>,
}

impl Auth {
    pub fn read(path: &Path) -> Result<Self> {
        if std::fs::metadata(path).map_err(Error::from)?.len() > 1024 * 1024 {
            return Err(Error::InvalidData);
        }
        let auth: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(Error::from)?).map_err(|_| Error::InvalidData)?;
        if !valid_id(&auth.identity)
            || auth.identity == "0"
            || !["https://www.rednote.com", "https://www.xiaohongshu.com"].contains(&auth.origin.as_str())
            || auth.ua.is_empty()
            || auth.ua.len() > 1024
            || !auth.ua.bytes().all(|b| (32..127).contains(&b))
        {
            return Err(Error::InvalidData);
        }
        let jar = Jar::default();
        jar.load(&auth.cookies)?;
        if jar
            .value(
                "web_session",
                &Url::parse(api_origin(&auth.origin)).map_err(|_| Error::InvalidData)?,
            )?
            .is_none_or(|value| value.is_empty())
        {
            return Err(Error::LoginExpired);
        }
        Ok(auth)
    }
}

fn api_origin(origin: &str) -> &'static str {
    if origin == "https://www.xiaohongshu.com" {
        "https://edith.xiaohongshu.com"
    } else {
        "https://webapi.rednote.com"
    }
}

pub struct Session {
    client: Client,
    jar: Arc<Jar>,
    origin: String,
    api: &'static str,
    ua: String,
    page_load: u64,
    sequence: AtomicU32,
}

impl Session {
    pub fn new(saved: Option<&Auth>) -> Result<Self> {
        let origin = saved.map(|a| a.origin.as_str()).unwrap_or("https://www.rednote.com");
        if !["https://www.rednote.com", "https://www.xiaohongshu.com"].contains(&origin) {
            return Err(Error::InvalidData);
        }
        let ua = saved.map(|a| a.ua.as_str()).unwrap_or(UA);
        if ua.is_empty() || ua.len() > 1024 || !ua.bytes().all(|b| (32..127).contains(&b)) {
            return Err(Error::InvalidData);
        }
        let jar = Arc::new(Jar::default());
        if let Some(saved) = saved {
            jar.load(&saved.cookies)?;
        }
        let client = Client::builder()
            .user_agent(ua)
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(45))
            .redirect(Policy::none())
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            client,
            jar,
            origin: origin.to_owned(),
            api: api_origin(origin),
            ua: ua.to_owned(),
            page_load: (chrono::Utc::now().timestamp_millis() as u64).saturating_sub(20_000),
            sequence: AtomicU32::new(17),
        })
    }

    async fn bootstrap(&self) -> Result<()> {
        let client = Client::builder()
            .user_agent(&self.ua)
            .cookie_provider(self.jar.clone())
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(45))
            .redirect(Policy::custom(|a| {
                if a.previous().len() < 3
                    && trusted_api(a.url().as_str()).is_ok()
                    && a.url().host_str().is_some_and(|h| WEB_HOSTS.contains(&h))
                    && ["/", "/login"].contains(&a.url().path())
                {
                    a.follow()
                } else {
                    a.stop()
                }
            }))
            .build()
            .map_err(|_| Error::Network)?;
        let response = client.get(&self.origin).send().await.map_err(|_| Error::Network)?;
        let bytes = read_response(response).await?;
        html_challenge(&bytes)?;
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        let random = format!("{:032x}", rand::random::<u128>());
        let prefix = format!("{timestamp:x}{}50000", &random[..30]);
        let full = format!("{prefix}{}", sign::crc32(prefix.as_bytes()));
        let a1 = &full[..52];
        let url = Url::parse(&self.origin).map_err(|_| Error::InvalidData)?;
        let base = url.host_str().ok_or(Error::InvalidData)?.trim_start_matches("www.");
        for text in [
            format!("a1={a1}; Domain=.{base}; Path=/; Secure"),
            format!("webId={:x}; Domain=.{base}; Path=/; Secure", md5::compute(a1)),
            format!("webBuild=4.86.0; Domain=.{base}; Path=/; Secure"),
            format!("xsecappid=xhs-pc-web; Domain=.{base}; Path=/; Secure"),
        ] {
            self.jar.set(&text, &url)?;
        }
        Ok(())
    }

    async fn api(&self, path: &str, body: Option<&str>) -> Result<Value> {
        let url = trusted_api(&format!("{}{path}", self.api))?;
        if !url.path().starts_with("/api/") {
            return Err(Error::InvalidData);
        }
        let path = &url.as_str()[self.api.len()..];
        let a1 = self.jar.value("a1", &url)?.ok_or(Error::LoginExpired)?;
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let xs = sign::sign(
            path,
            body,
            &a1,
            &sign::Inputs {
                timestamp,
                page_load: self.page_load,
                seed: rand::random(),
                sequence,
                window: 1500 + sequence,
            },
        )
        .map_err(|_| Error::InvalidData)?;
        let common = sign::common(&a1, timestamp, 7).map_err(|_| Error::InvalidData)?;
        let mut request = self
            .client
            .request(
                if body.is_some() {
                    reqwest::Method::POST
                } else {
                    reqwest::Method::GET
                },
                url,
            )
            .header(header::ORIGIN, &self.origin)
            .header(header::REFERER, format!("{}/", self.origin))
            .header(header::CONTENT_TYPE, "application/json;charset=UTF-8")
            .header("x-s", xs)
            .header("x-s-common", common)
            .header("x-t", timestamp.to_string())
            .header("x-b3-traceid", format!("{:016x}", rand::random::<u64>()))
            .header("x-mns", "unload");
        if let Some(body) = body {
            request = request.body(body.to_owned());
        }
        let bytes = read_response(request.send().await.map_err(|_| Error::Network)?).await?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| html_challenge(&bytes).err().unwrap_or(Error::InvalidData))?;
        business_status(&value)?;
        Ok(value)
    }

    fn export(&self, id: String) -> Result<Auth> {
        if self
            .jar
            .value("web_session", &Url::parse(self.api).map_err(|_| Error::InvalidData)?)?
            .is_none_or(|value| value.is_empty())
        {
            return Err(Error::LoginExpired);
        }
        Ok(Auth {
            identity: id,
            origin: self.origin.clone(),
            ua: self.ua.clone(),
            cookies: self.jar.snapshot(),
        })
    }
}

async fn read_response(mut response: reqwest::Response) -> Result<Vec<u8>> {
    if let Some(error) = response_error(i64::from(response.status().as_u16())) {
        return Err(error);
    }
    if response.status().as_u16() != 200 {
        return Err(Error::Network);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
        if chunk.len() > MAX_BODY.saturating_sub(bytes.len()) {
            return Err(Error::InvalidData);
        }
        bytes.extend(chunk);
    }
    Ok(bytes)
}

fn business_status(value: &Value) -> Result<()> {
    if value["success"] == true && value["code"].as_i64() == Some(0) {
        return Ok(());
    }
    if value["code"].as_i64() == Some(-100) {
        Err(Error::LoginExpired)
    } else {
        Err(Error::Rejected)
    }
}

fn html_challenge(bytes: &[u8]) -> Result<()> {
    let raw = String::from_utf8_lossy(bytes);
    let scripts = regex::Regex::new(r"(?is)<script\b[^>]*>.*?</script>|<style\b[^>]*>.*?</style>")
        .map_err(|_| Error::InvalidData)?;
    let clean = scripts.replace_all(&raw, " ");
    let tags = regex::Regex::new(r"(?s)<[^>]*>").map_err(|_| Error::InvalidData)?;
    let visible = tags.replace_all(&clean, " ").to_ascii_lowercase();
    if [
        "ip at risk",
        "请完成验证",
        "安全验证",
        "安全限制",
        "访问频繁",
        "访问异常",
        "验证后继续",
    ]
    .iter()
    .any(|s| visible.contains(s))
    {
        Err(Error::Challenge)
    } else {
        Ok(())
    }
}

pub async fn authenticate(
    session: &Session,
    saved: Option<&Auth>,
    expected: Option<&str>,
    qr: Option<&tokio::sync::watch::Sender<Option<String>>>,
) -> Result<Auth> {
    let expected = expected.or(saved.map(|a| a.identity.as_str()));
    if let Some(qr) = qr {
        session.bootstrap().await?;
        let value = session.api("/api/sns/web/v1/login/qrcode/create", Some("{}")).await?;
        let data = &value["data"];
        let qr_id = data["qr_id"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .ok_or(Error::InvalidData)?;
        let code = data["code"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .ok_or(Error::InvalidData)?;
        let target = data["url"]
            .as_str()
            .filter(|v| v.len() <= 8192)
            .ok_or(Error::InvalidData)?;
        let target_url = Url::parse(target).map_err(|_| Error::InvalidData)?;
        if target_url.scheme() != "xhsdiscover" && trusted_api(target).is_err() {
            return Err(Error::InvalidData);
        }
        qr.send_replace(Some(target.to_owned()));
        let mut poll = Url::parse(&format!("{}/api/sns/web/v1/login/qrcode/status", session.api))
            .map_err(|_| Error::InvalidData)?;
        poll.query_pairs_mut()
            .append_pair("qr_id", qr_id)
            .append_pair("code", code);
        let path = &poll.as_str()[session.api.len()..];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let result = session.api(path, None).await?;
            match result["data"]["code_status"].as_i64().ok_or(Error::InvalidData)? {
                0 | 1 => {}
                2 => {
                    let info = &result["data"]["login_info"];
                    if let Some(value) = info["session"].as_str().or_else(|| info["web_session"].as_str()) {
                        if value.is_empty()
                            || value.len() > 4096
                            || !value.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b',')
                        {
                            return Err(Error::InvalidData);
                        }
                        let url = Url::parse(&session.origin).map_err(|_| Error::InvalidData)?;
                        let base = url.host_str().ok_or(Error::InvalidData)?.trim_start_matches("www.");
                        session.jar.set(
                            &format!("web_session={value}; Domain=.{base}; Path=/; Secure; HttpOnly"),
                            &url,
                        )?;
                    }
                    let me = session.api(ME, None).await?;
                    let id =
                        checked_identity(&format!("{}{ME}", session.api), &me, expected, info["user_id"].as_str())?;
                    return session.export(id);
                }
                _ => return Err(Error::LoginExpired),
            }
        }
        return Err(Error::LoginExpired);
    }
    let value = session.api(ME, None).await?;
    let id = checked_identity(&format!("{}{ME}", session.api), &value, expected, None)?;
    session.export(id)
}

pub async fn scan(session: &Session, auth: &Auth) -> Result<Vec<Item>> {
    let mut scan = Scan::default();
    for _ in 0..20 {
        let mut url =
            Url::parse(&format!("{}/api/sns/web/v2/note/collect/page", session.api)).map_err(|_| Error::InvalidData)?;
        url.query_pairs_mut()
            .append_pair("num", "30")
            .append_pair("cursor", &scan.cursor)
            .append_pair("user_id", &auth.identity)
            .append_pair("image_formats", "jpg,webp,avif");
        let value = session.api(&url.as_str()[session.api.len()..], None).await?;
        let cursor = scan.cursor.clone();
        scan.accept(&cursor, &value)?;
        if scan.finish_window(None)? {
            return Ok(scan.items);
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Err(Error::Incomplete)
}

pub async fn detail(session: &Session, auth: &Auth, item: &Item) -> Result<Video> {
    if !valid_id(&item.id)
        || item.kind != "video"
        || item.token.is_empty()
        || item.token.len() > 4096
        || !item.token.is_ascii()
    {
        return Err(Error::InvalidData);
    }
    let mut target = Url::parse(&format!("{}/explore/{}", auth.origin, item.id)).map_err(|_| Error::InvalidData)?;
    target
        .query_pairs_mut()
        .append_pair("xsec_token", &item.token)
        .append_pair("xsec_source", "pc_user");
    let note_url = target.clone();
    let client = Client::builder()
        .user_agent(&session.ua)
        .cookie_provider(session.jar.clone())
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(45))
        .redirect(Policy::custom(move |a| {
            if a.previous().len() < 3 && navigation_matches(a.url(), &note_url) {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()
        .map_err(|_| Error::Network)?;
    let bytes = read_response(
        client
            .get(target)
            .header(header::REFERER, format!("{}/", auth.origin))
            .send()
            .await
            .map_err(|_| Error::Network)?,
    )
    .await?;
    html_challenge(&bytes)?;
    let state = initial_state(std::str::from_utf8(&bytes).map_err(|_| Error::InvalidData)?)
        .map_err(|_| Error::MediaUnavailable)?;
    let note = &state["note"]["noteDetailMap"][&item.id]["note"];
    if note["noteId"] != item.id && note["note_id"] != item.id {
        return Err(Error::InvalidData);
    }
    Ok(Video {
        url: video_url(note)?,
        title: note["title"].as_str().unwrap_or("").chars().take(1024).collect(),
        author: note["user"]["nickname"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(256)
            .collect(),
    })
}

fn trusted_api(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::InvalidData)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !url.host_str().is_some_and(|host| API_HOSTS.contains(&host))
    {
        return Err(Error::InvalidData);
    }
    Ok(url)
}

fn navigation_matches(actual: &Url, target: &Url) -> bool {
    actual.host_str().is_some_and(|h| WEB_HOSTS.contains(&h))
        && (actual.path() == target.path()
            || (target.path() == "/explore" && matches!(actual.path(), "/" | "/login"))
            || target
                .path()
                .strip_prefix("/explore/")
                .is_some_and(|id| valid_id(id) && actual.path() == format!("/discovery/item/{id}")))
}

fn identity(url: &str, body: &Value) -> Option<String> {
    let url = trusted_api(url).ok()?;
    if !["/api/sns/web/v1/user/self", "/api/sns/web/v2/user/me"].contains(&url.path().trim_end_matches('/'))
        || body["success"] != true
        || body["code"].as_i64() != Some(0)
        || body["data"]["guest"] != false
    {
        return None;
    }
    let id = body["data"]["user_id"]
        .as_str()
        .or_else(|| body["data"]["id"].as_str())?;
    (valid_id(id) && id != "0").then(|| id.to_owned())
}

fn checked_identity(url: &str, body: &Value, expected: Option<&str>, qr_user: Option<&str>) -> Result<String> {
    if body["data"]["guest"] == true {
        return Err(Error::LoginExpired);
    }
    let id = identity(url, body).ok_or(Error::IdentityUnverified)?;
    if [expected, qr_user].into_iter().flatten().any(|value| value != id) {
        return Err(Error::AccountChanged);
    }
    Ok(id)
}

fn response_error(status: i64) -> Option<Error> {
    match status {
        401 => Some(Error::LoginExpired),
        403 | 406 | 429 => Some(Error::Rejected),
        461 | 471 => Some(Error::Challenge),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub token: String,
}

fn page_items(body: &Value) -> Result<(Vec<Item>, bool, String)> {
    if body["success"] != true || body["code"].as_i64() != Some(0) {
        return Err(Error::Rejected);
    }
    let data = &body["data"];
    let more = data["has_more"].as_bool().ok_or(Error::InvalidData)?;
    let cursor = data["cursor"].as_str().unwrap_or("").to_owned();
    let notes = data["notes"].as_array().ok_or(Error::InvalidData)?;
    if more && (cursor.is_empty() || notes.is_empty()) {
        return Err(Error::Incomplete);
    }
    let mut ids = HashSet::new();
    let mut items = Vec::new();
    for raw in notes {
        let id = raw["note_id"]
            .as_str()
            .or_else(|| raw["id"].as_str())
            .filter(|id| valid_id(id))
            .ok_or(Error::InvalidData)?;
        if !ids.insert(id) {
            return Err(Error::Incomplete);
        }
        let card = raw.get("note_card").or_else(|| raw.get("noteCard")).unwrap_or(raw);
        let item_type = card["type"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or(Error::InvalidData)?;
        let kind = if item_type == "video" { "video" } else { "unsupported" };
        let token = raw["xsec_token"]
            .as_str()
            .or_else(|| raw["xsecToken"].as_str())
            .unwrap_or("");
        if token.len() > 4096 || !token.is_ascii() || (kind == "video" && token.is_empty()) {
            return Err(Error::InvalidData);
        }
        items.push(Item {
            id: id.into(),
            kind: kind.into(),
            token: token.into(),
        });
    }
    Ok((items, more, cursor))
}

#[derive(Default)]
struct Scan {
    cursor: String,
    visited: HashSet<String>,
    items: Vec<Item>,
    ids: HashSet<String>,
    complete: bool,
}

impl Scan {
    fn accept(&mut self, request_cursor: &str, body: &Value) -> Result<()> {
        if self.complete || request_cursor != self.cursor || !self.visited.insert(request_cursor.into()) {
            return Err(Error::Incomplete);
        }
        let (items, more, next) = page_items(body)?;
        let nonempty = !items.is_empty();
        let before = self.items.len();
        for item in items {
            if self.ids.insert(item.id.clone()) {
                self.items.push(item);
            }
        }
        if nonempty && self.items.len() == before {
            return Err(Error::Incomplete);
        }
        if more && (next == self.cursor || self.visited.contains(&next)) {
            return Err(Error::Incomplete);
        }
        self.cursor = next;
        self.complete = !more;
        Ok(())
    }

    fn finish(&self, expected: Option<u64>) -> Result<()> {
        if !self.complete || expected.is_some_and(|n| n != self.items.len() as u64) {
            Err(Error::Incomplete)
        } else {
            Ok(())
        }
    }

    fn finish_window(&mut self, expected: Option<u64>) -> Result<bool> {
        if self.items.len() >= 50 {
            self.items.truncate(50);
            return Ok(true);
        }
        if self.complete {
            self.finish(expected)?;
            return Ok(true);
        }
        Ok(false)
    }
}

fn media_url(raw: &str) -> Result<Url> {
    let mut url = Url::parse(raw).map_err(|_| Error::MediaUnavailable)?;
    if url.scheme() == "http" {
        url.set_scheme("https").map_err(|_| Error::MediaUnavailable)?;
    }
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !url.host_str().is_some_and(|host| {
            [
                "sns-video-bd.xhscdn.com",
                "sns-v11.rednotecdn.com",
                "sns-v14.rednotecdn.com",
                "sns-v27.rednotecdn.com",
                "sns-v28.rednotecdn.com",
            ]
            .contains(&host)
        })
    {
        return Err(Error::MediaUnavailable);
    }
    Ok(url)
}

fn video_url(note: &Value) -> Result<Url> {
    let streams = note["video"]["media"]["stream"]
        .as_object()
        .ok_or(Error::MediaUnavailable)?;
    let best = streams
        .iter()
        .flat_map(|(codec, values)| {
            values.as_array().into_iter().flatten().filter_map(move |stream| {
                let declared = stream["videoCodec"]
                    .as_str()
                    .or_else(|| stream["video_codec"].as_str())
                    .unwrap_or(codec);
                let h264 = ["h264", "avc", "avc1", "EF4"].contains(&declared);
                if !h264 || (declared == "EF4" && stream["format"] != "mp4") {
                    return None;
                }
                let raw = stream["masterUrl"].as_str().or_else(|| stream["master_url"].as_str())?;
                Some(((h264, stream["height"].as_u64()?, stream["width"].as_u64()?), raw))
            })
        })
        .max_by_key(|(rank, _)| *rank)
        .ok_or(Error::MediaUnavailable)?;
    // EF4 samples decode as H.264; ffprobe still verifies every file before publishing.
    media_url(best.1)
}

pub struct Video {
    pub url: Url,
    pub title: String,
    pub author: String,
}

pub async fn media_response(auth: &Auth, url: Url) -> Result<reqwest::Response> {
    media_url(url.as_str())?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3
                || attempt.url().scheme() != "https"
                || media_url(attempt.url().as_str()).is_err()
            {
                attempt.error("media redirect rejected")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| Error::Network)?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, &auth.ua)
        .header(reqwest::header::REFERER, format!("{}/", auth.origin))
        .send()
        .await
        .map_err(|_| Error::Network)?;
    match response.status().as_u16() {
        200 => Ok(response),
        401 | 403 | 406 | 429 | 461 | 471 => Err(Error::Rejected),
        _ => Err(Error::MediaUnavailable),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn guest_wrong_account_and_platform_rejection_cannot_replace_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let empty = Auth {
            identity: "self123".into(),
            origin: "https://www.rednote.com".into(),
            ua: "fixture".into(),
            cookies: vec![json!({"name":"web_session","value":"","domain":".rednote.com","path":"/","secure":true})],
        };
        std::fs::write(&path, serde_json::to_vec(&empty).unwrap()).unwrap();
        assert!(matches!(Auth::read(&path), Err(Error::LoginExpired)));
        let url = "https://webapi.rednote.com/api/sns/web/v2/user/me";
        let body = json!({"success":true,"code":0,"data":{"guest":false,"user_id":"self123"}});
        assert_eq!(
            checked_identity(url, &body, Some("other"), None),
            Err(Error::AccountChanged)
        );
        assert_eq!(
            checked_identity(url, &body, None, Some("other")),
            Err(Error::AccountChanged)
        );
        assert_eq!(
            checked_identity(url, &body, Some("self123"), Some("self123")).unwrap(),
            "self123"
        );
        let guest = json!({"success":true,"code":0,"data":{"guest":true}});
        assert_eq!(checked_identity(url, &guest, None, None), Err(Error::LoginExpired));
        assert_eq!(
            business_status(&json!({"success":false,"code":-100})),
            Err(Error::LoginExpired)
        );
        assert_eq!(
            business_status(&json!({"success":false,"code":300012})),
            Err(Error::Rejected)
        );
        assert_eq!(html_challenge(b"<html>IP at risk</html>"), Err(Error::Challenge));
        assert!(html_challenge(b"<script>const message='IP at risk'</script><title>Note</title>").is_ok());
    }
    fn body(id: &str, more: bool, cursor: &str) -> Value {
        json!({"success":true,"code":0,"data":{"has_more":more,"cursor":cursor,"notes":[{"note_id":id,"type":"video","xsec_token":"synthetic-token"}]}})
    }

    #[test]
    fn identity_requires_official_self_success_and_non_guest() {
        let data = json!({"success":true,"code":0,"data":{"guest":false,"user_id":"self123"}});
        assert_eq!(
            identity("https://webapi.rednote.com/api/sns/web/v2/user/me", &data).as_deref(),
            Some("self123")
        );
        for url in [
            "https://webapi.rednote.com.evil.test/api/sns/web/v2/user/me",
            "http://webapi.rednote.com/api/sns/web/v2/user/me",
            "https://webapi.rednote.com:444/api/sns/web/v2/user/me",
            "https://webapi.rednote.com/api/sns/web/v1/user/other",
        ] {
            assert!(identity(url, &data).is_none());
        }
        let mut guest = data;
        guest["data"]["guest"] = json!(true);
        assert!(identity("https://webapi.rednote.com/api/sns/web/v2/user/me", &guest).is_none());
    }

    #[test]
    fn note_redirect_must_keep_official_host_and_exact_note_id() {
        let target = Url::parse("https://www.xiaohongshu.com/explore/fixture123").unwrap();
        let login = Url::parse("https://www.rednote.com/login").unwrap();
        let home = Url::parse("https://www.rednote.com/explore").unwrap();
        assert!(navigation_matches(&login, &home));
        assert!(!navigation_matches(&login, &target));
        assert!(navigation_matches(
            &Url::parse("https://www.rednote.com/discovery/item/fixture123").unwrap(),
            &target
        ));
        for url in [
            "https://www.rednote.com/discovery/item/other",
            "https://www.rednote.com.evil.test/discovery/item/fixture123",
            "https://www.rednote.com/user/profile/fixture123",
        ] {
            assert!(!navigation_matches(&Url::parse(url).unwrap(), &target));
        }
    }

    #[test]
    fn complete_scan_needs_terminal_response_and_matching_total() {
        let mut scan = Scan::default();
        scan.accept("", &body("a", true, "next")).unwrap();
        assert_eq!(scan.finish(None), Err(Error::Incomplete));
        scan.accept("next", &body("b", false, "")).unwrap();
        scan.finish(Some(2)).unwrap();
        assert_eq!(scan.finish(Some(3)), Err(Error::Incomplete));
    }

    #[test]
    fn window_stops_at_fifty_unique_items_without_claiming_full_catalog() {
        let mut scan = Scan::default();
        let mut first = body("item0", true, "next");
        first["data"]["notes"] = json!(
            (0..45)
                .map(|n| json!({"note_id":format!("item{n}"),"type":"normal"}))
                .collect::<Vec<_>>()
        );
        scan.accept("", &first).unwrap();
        assert!(!scan.finish_window(Some(895)).unwrap());
        let mut second = body("item44", true, "later");
        second["data"]["notes"] = json!(
            (44..55)
                .map(|n| json!({"note_id":format!("item{n}"),"type":"video","xsec_token":"fixture"}))
                .collect::<Vec<_>>()
        );
        scan.accept("next", &second).unwrap();
        assert!(scan.finish_window(Some(895)).unwrap());
        assert_eq!(scan.items.len(), 50);
        assert_eq!(scan.items[44].id, "item44");
        assert_eq!(scan.items[49].id, "item49");
        assert_eq!(scan.items.iter().filter(|item| item.kind == "video").count(), 5);
        assert!(!scan.complete);
        assert_eq!(scan.finish(None), Err(Error::Incomplete));
    }

    #[test]
    fn short_window_requires_verified_end_and_matching_count() {
        let mut scan = Scan::default();
        scan.accept("", &body("a", true, "next")).unwrap();
        assert!(!scan.finish_window(None).unwrap());
        scan.accept("next", &body("b", false, "")).unwrap();
        assert_eq!(scan.finish_window(Some(3)), Err(Error::Incomplete));
        assert!(scan.finish_window(Some(2)).unwrap());
        let mut empty = Scan::default();
        empty
            .accept(
                "",
                &json!({"success":true,"code":0,"data":{"notes":[],"has_more":false,"cursor":""}}),
            )
            .unwrap();
        assert_eq!(empty.finish_window(Some(1)), Err(Error::Incomplete));
        assert!(empty.finish_window(Some(0)).unwrap());
    }

    #[test]
    fn scan_rejects_repeats_cycles_wrong_cursor_and_partial_responses() {
        let mut scan = Scan::default();
        scan.accept("", &body("a", true, "next")).unwrap();
        assert_eq!(scan.accept("next", &body("a", false, "")), Err(Error::Incomplete));
        assert_eq!(
            Scan::default().accept("other", &body("a", false, "")),
            Err(Error::Incomplete)
        );
        assert_eq!(Scan::default().accept("", &body("a", true, "")), Err(Error::Incomplete));
        assert_eq!(page_items(&json!({"success":false,"code":-1})), Err(Error::Rejected));
        assert_eq!(
            page_items(&json!({"success":true,"code":0,"data":{"notes":[]}})),
            Err(Error::InvalidData)
        );
    }

    #[test]
    fn confirmed_empty_and_unsupported_are_distinct_from_failure() {
        let empty = json!({"success":true,"code":0,"data":{"notes":[],"has_more":false,"cursor":""}});
        let mut scan = Scan::default();
        scan.accept("", &empty).unwrap();
        scan.finish(Some(0)).unwrap();
        let mut image = body("image", false, "");
        image["data"]["notes"][0]["type"] = json!("normal");
        assert_eq!(page_items(&image).unwrap().0[0].kind, "unsupported");
        image["data"]["notes"][0]["type"] = json!("video");
        image["data"]["notes"][0]["xsec_token"] = Value::Null;
        assert_eq!(page_items(&image), Err(Error::InvalidData));
        image["data"]["notes"][0]["xsec_token"] = json!("fixture");
        image["data"]["notes"][0]["type"] = Value::Null;
        assert_eq!(page_items(&image), Err(Error::InvalidData));
    }

    #[test]
    fn media_hosts_and_h264_selection_are_bounded() {
        for url in [
            "https://xhscdn.com.evil.test/a.mp4",
            "https://localhost/a.mp4",
            "file:///a.mp4",
            "https://user:pass@sns-video-bd.xhscdn.com/a.mp4",
            "https://sns-video-bd.xhscdn.com:444/a.mp4",
        ] {
            assert!(media_url(url).is_err());
        }
        assert_eq!(
            media_url("http://sns-video-bd.xhscdn.com/a.mp4").unwrap().scheme(),
            "https"
        );
        let note = json!({"video":{"media":{"stream":{"h264":[{"width":640,"height":360,"masterUrl":"https://sns-video-bd.xhscdn.com/low.mp4"},{"width":1920,"height":1080,"masterUrl":"https://sns-video-bd.xhscdn.com/high.mp4"}]}}}});
        assert_eq!(video_url(&note).unwrap().path(), "/high.mp4");
        assert!(video_url(&json!({})).is_err());
        let opaque = json!({"video":{"media":{"stream":{"EF4":[{"width":720,"height":1280,"format":"mp4","videoCodec":"EF4","masterUrl":"https://sns-v28.rednotecdn.com/test.mp4"}]}}}});
        assert_eq!(video_url(&opaque).unwrap().host_str(), Some("sns-v28.rednotecdn.com"));
        assert!(video_url(&json!({"video":{"consumer":{"originVideoKey":"stream/fixture.mp4"}}})).is_err());
    }
    #[test]
    fn official_unauthorized_requires_login_while_rejection_requires_cooldown() {
        assert_eq!(response_error(401), Some(Error::LoginExpired));
        for status in [403, 406, 429] {
            assert_eq!(response_error(status), Some(Error::Rejected));
        }
        assert_eq!(response_error(461), Some(Error::Challenge));
        assert_eq!(response_error(471), Some(Error::Challenge));
        assert_eq!(response_error(200), None);
        assert_eq!(response_error(500), None);
    }
}

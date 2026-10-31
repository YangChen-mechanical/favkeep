use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::{
    emulation::{SetLocaleOverrideParams, SetUserAgentOverrideParams},
    input::{DispatchMouseEventParams, DispatchMouseEventType},
    network::{
        BlockPattern, CookieParam, DeleteCookiesParams, EnableParams, EventLoadingFailed, EventLoadingFinished,
        EventRequestWillBeSent, EventResponseReceived, EventResponseReceivedExtraInfo, GetCookiesParams,
        GetResponseBodyParams, RequestId, SetBlockedUrLsParams, SetCookiesParams,
    },
    page::CaptureScreenshotFormat,
};
use futures::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Error, Result, valid_id};
pub use crate::douyin::browser::Session;

const WEB_HOSTS: [&str; 2] = ["www.xiaohongshu.com", "www.rednote.com"];
const API_HOSTS: [&str; 4] = [
    "www.xiaohongshu.com",
    "edith.xiaohongshu.com",
    "www.rednote.com",
    "webapi.rednote.com",
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Auth {
    pub identity: String,
    pub origin: String,
    pub ua: String,
    pub cookies: Vec<Value>,
}

impl Auth {
    pub fn read(path: &Path) -> Result<Self> {
        if std::fs::metadata(path).map_err(|_| Error::File)?.len() > 1024 * 1024 {
            return Err(Error::InvalidData);
        }
        let auth: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|_| Error::File)?).map_err(|_| Error::InvalidData)?;
        if !valid_id(&auth.identity)
            || !WEB_HOSTS.iter().any(|host| auth.origin == format!("https://{host}"))
            || !auth.ua.is_ascii()
            || auth.ua.len() > 1024
        {
            return Err(Error::InvalidData);
        }
        auth.cookie_params()?;
        Ok(auth)
    }

    fn cookie_params(&self) -> Result<Vec<CookieParam>> {
        let mut cookies = Vec::new();
        for value in &self.cookies {
            let domain = value["domain"].as_str().unwrap_or("").trim_start_matches('.');
            if !["xiaohongshu.com", "rednote.com"]
                .iter()
                .any(|base| domain == *base || domain.ends_with(&format!(".{base}")))
            {
                continue;
            }
            let mut value = value.clone();
            let object = value.as_object_mut().ok_or(Error::InvalidData)?;
            if object.get("expires").and_then(Value::as_f64).is_some_and(|n| n <= 0.0) {
                object.remove("expires");
            }
            let cookie: CookieParam = serde_json::from_value(value).map_err(|_| Error::InvalidData)?;
            if cookie.name.is_empty()
                || !cookie
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !cookie.value.is_ascii()
                || cookie.value.bytes().any(|b| b < 32 || b == 127 || b == b';')
            {
                return Err(Error::InvalidData);
            }
            cookies.push(cookie);
        }
        if !cookies.iter().any(|c| c.name == "web_session" && !c.value.is_empty()) {
            return Err(Error::LoginExpired);
        }
        Ok(cookies)
    }
}

fn trusted_api(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::InvalidData)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
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

async fn navigate(page: &Page, target: &str) -> Result<()> {
    match page.goto(target).await {
        Ok(_) | Err(chromiumoxide::error::CdpError::Timeout) => {}
        Err(error) => {
            let code = match &error {
                chromiumoxide::error::CdpError::ChromeMessage(message) => message
                    .split("net::")
                    .nth(1)
                    .map(|v| {
                        v.chars()
                            .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| "chrome_message".into()),
                chromiumoxide::error::CdpError::Chrome(_) => "cdp_protocol".into(),
                _ => "browser_transport".into(),
            };
            eprintln!("navigation_failed: {code}");
            return Err(Error::Browser);
        }
    }
    // A loaded document can outlive a timed-out asset; identity and pagination still need their own evidence.
    let actual = page
        .evaluate("location.href")
        .await
        .map_err(|_| Error::Browser)?
        .into_value::<String>()
        .map_err(|_| Error::Browser)?;
    let actual = trusted_api(&actual)?;
    let target = trusted_api(target)?;
    if actual.path() == "/website-login/error" {
        return Err(Error::Rejected);
    }
    if !navigation_matches(&actual, &target) {
        eprintln!("navigation_target_mismatch");
        return Err(Error::Browser);
    }
    Ok(())
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

fn response_error(status: i64) -> Option<Error> {
    match status {
        401 => Some(Error::LoginExpired),
        403 | 406 | 429 | 461 | 471 => Some(Error::Rejected),
        _ => None,
    }
}

async fn response_body(page: &Page, event: &EventResponseReceived) -> Result<Value> {
    if let Some(error) = response_error(event.response.status) {
        return Err(error);
    }
    if event.response.status != 200 {
        return Err(Error::Network);
    }
    for _ in 0..20 {
        if let Ok(result) = page.execute(GetResponseBodyParams::new(event.request_id.clone())).await {
            if result.result.body.len() > 12 * 1024 * 1024 {
                return Err(Error::InvalidData);
            }
            let bytes = if result.result.base64_encoded {
                base64::engine::general_purpose::STANDARD
                    .decode(result.result.body)
                    .map_err(|_| Error::InvalidData)?
            } else {
                result.result.body.into_bytes()
            };
            return serde_json::from_slice(&bytes).map_err(|_| Error::InvalidData);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(Error::Incomplete)
}

async fn challenge(page: &Page) -> Result<()> {
    let blocked = page
        .evaluate(
            r#"!!document.body && /请完成验证|安全验证|安全限制|访问频繁|访问异常|验证后继续|IP at risk/.test(document.body.innerText)"#,
        )
        .await
        .map_err(|_| Error::Browser)?
        .into_value::<bool>()
        .map_err(|_| Error::Browser)?;
    if blocked { Err(Error::Challenge) } else { Ok(()) }
}

const QR: &str = r#"(() => {
    const visible = e => { const r=e.getBoundingClientRect(); return r.width>0 && r.height>0; };
    const context = e => {
        if (e.matches('img[alt*="二维码"]') || e.closest('[class*="qrcode"],[class*="qr-code"]')) return true;
        let p=e.parentElement;
        for(let i=0;p && i<4;i++,p=p.parentElement) {
            const r=p.getBoundingClientRect();
            if(r.width<650 && r.height<800 && /扫码|二维码|Scan.*log in|QR code/i.test(p.innerText || '')) return true;
        }
        return false;
    };
    const qr=[...document.querySelectorAll('img,canvas')].find(e=>{
        const r=e.getBoundingClientRect();
        return visible(e) && r.width>=100 && r.width<=512 && r.height>=100 && r.height<=512 && r.width/r.height>.8 && r.width/r.height<1.2 && context(e);
    });
    if(qr) { qr.setAttribute('data-favkeep-xhs-qr',''); return true; }
    const button=[...document.querySelectorAll('button,a,div,span')].find(e=>visible(e) && e.childElementCount===0 && ['登录','扫码登录','登录/注册','Log in','Login','Sign in'].includes(e.textContent.trim()) && !e.hasAttribute('data-favkeep-clicked'));
    if(button) { button.setAttribute('data-favkeep-clicked',''); button.click(); }
    return false;
})()"#;

const SELECT_COLLECTION: &str = r#"(() => {
    const tabs=[...document.querySelectorAll('span,div,button,a')].filter(e=>
        e.childElementCount===0 && ['收藏','Save','Saved'].includes(e.textContent.trim()) && e.getBoundingClientRect().width>0);
    if(!tabs.length)return false;
    tabs[tabs.length-1].click();return true;
})()"#;

pub async fn authenticate(
    session: &Session,
    saved: Option<&Auth>,
    expected: Option<&str>,
    qr: Option<&tokio::sync::watch::Sender<Option<String>>>,
) -> Result<Auth> {
    let login = qr.is_some();
    if !login && saved.is_none() {
        return Err(Error::LoginExpired);
    }
    let origin = saved.as_ref().map_or("https://www.rednote.com", |a| a.origin.as_str());
    if !login && let Some(auth) = &saved {
        let existing = session
            .page
            .execute(
                GetCookiesParams::builder()
                    .urls(["https://www.xiaohongshu.com", "https://www.rednote.com"])
                    .build(),
            )
            .await
            .map_err(|_| Error::Browser)?
            .result
            .cookies;
        if !existing.iter().any(|c| c.name == "web_session" && !c.value.is_empty()) {
            session
                .page
                .execute(SetCookiesParams::new(auth.cookie_params()?))
                .await
                .map_err(|_| Error::Browser)?;
        }
    }
    let mut responses = session
        .page
        .event_listener::<EventResponseReceived>()
        .await
        .map_err(|_| Error::Browser)?;
    navigate(&session.page, &format!("{origin}/explore")).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(if login { 180 } else { 45 });
    let mut qr_saved = false;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), responses.next()).await
            && event.r#type != chromiumoxide::cdp::browser_protocol::network::ResourceType::Preflight
            && let Ok(url) = trusted_api(&event.response.url)
            && ["/api/sns/web/v1/user/self", "/api/sns/web/v2/user/me"].contains(&url.path().trim_end_matches('/'))
        {
            let body = response_body(&session.page, &event).await?;
            if !login && body["success"] == true && body["code"] == 0 && body["data"]["guest"] == true {
                return Err(Error::LoginExpired);
            }
            if let Some(id) = identity(&event.response.url, &body) {
                if expected.or(saved.map(|s| s.identity.as_str())).is_some_and(|s| s != id) {
                    return Err(Error::AccountChanged);
                }
                let origin = if url.host_str().is_some_and(|h| h.ends_with("rednote.com")) {
                    "https://www.rednote.com"
                } else {
                    "https://www.xiaohongshu.com"
                };
                let ua = session
                    .page
                    .evaluate("navigator.userAgent")
                    .await
                    .map_err(|_| Error::Browser)?
                    .into_value::<String>()
                    .map_err(|_| Error::Browser)?;
                let cookies = session
                    .page
                    .execute(
                        GetCookiesParams::builder()
                            .urls([
                                "https://www.xiaohongshu.com",
                                "https://edith.xiaohongshu.com",
                                "https://www.rednote.com",
                                "https://webapi.rednote.com",
                            ])
                            .build(),
                    )
                    .await
                    .map_err(|_| Error::Browser)?
                    .result
                    .cookies;
                let auth = Auth {
                    identity: id,
                    origin: origin.into(),
                    ua,
                    cookies: cookies
                        .into_iter()
                        .map(|c| serde_json::to_value(c).map_err(|_| Error::InvalidData))
                        .collect::<Result<_>>()?,
                };
                auth.cookie_params()?;
                return Ok(auth);
            }
        }
        challenge(&session.page).await?;
        if login
            && !qr_saved
            && session
                .page
                .evaluate(QR)
                .await
                .map_err(|_| Error::Browser)?
                .into_value::<bool>()
                .map_err(|_| Error::Browser)?
        {
            let bytes = session
                .page
                .find_element("[data-favkeep-xhs-qr]")
                .await
                .map_err(|_| Error::Browser)?
                .screenshot(CaptureScreenshotFormat::Png)
                .await
                .map_err(|_| Error::Browser)?;
            if bytes.len() > 1024 * 1024 {
                return Err(Error::InvalidData);
            }
            qr.ok_or(Error::InvalidData)?.send_replace(Some(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )));
            qr_saved = true;
        }
    }
    Err(if login {
        Error::LoginExpired
    } else {
        Error::IdentityUnverified
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub token: String,
}

fn collection_url(raw: &str) -> Option<Url> {
    let url = trusted_api(raw).ok()?;
    [
        "/api/sns/web/v1/note/collect/page",
        "/api/sns/web/v2/note/collect/page",
        "/api/sns/web/v1/user/collect/page",
    ]
    .contains(&url.path().trim_end_matches('/'))
    .then_some(url)
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

async fn scroll(page: &Page) -> Result<()> {
    page.evaluate("window.scrollTo(0, document.documentElement.scrollHeight); window.scrollBy(0,-900)")
        .await
        .map_err(|_| Error::Browser)?;
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseWheel)
            .x(640.0)
            .y(700.0)
            .delta_x(0.0)
            .delta_y(1600.0)
            .build()
            .map_err(|_| Error::Browser)?,
    )
    .await
    .map_err(|_| Error::Browser)?;
    Ok(())
}

pub async fn scan(session: &Session, auth: &Auth) -> Result<Vec<Item>> {
    let page = &session.page;
    page.execute(
        EnableParams::builder()
            .max_total_buffer_size(64 * 1024 * 1024)
            .max_resource_buffer_size(8 * 1024 * 1024)
            .enable_durable_messages(true)
            .build(),
    )
    .await
    .map_err(|_| Error::Browser)?;
    page.execute(
        SetBlockedUrLsParams::builder()
            .url_patterns(
                ["jpg", "jpeg", "png", "webp", "mp4", "woff"]
                    .map(|extension| BlockPattern::new(format!("*://*:*/*.{extension}*"), true)),
            )
            .build(),
    )
    .await
    .map_err(|_| Error::Browser)?;
    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .map_err(|_| Error::Browser)?;
    let mut finished = page
        .event_listener::<EventLoadingFinished>()
        .await
        .map_err(|_| Error::Browser)?;
    let mut failures = page
        .event_listener::<EventLoadingFailed>()
        .await
        .map_err(|_| Error::Browser)?;
    let mut requests = page
        .event_listener::<EventRequestWillBeSent>()
        .await
        .map_err(|_| Error::Browser)?;
    let mut extras = page
        .event_listener::<EventResponseReceivedExtraInfo>()
        .await
        .map_err(|_| Error::Browser)?;
    navigate(page, &format!("{}/user/profile/{}", auth.origin, auth.identity)).await?;
    for attempt in 0..45 {
        challenge(page).await?;
        if page
            .evaluate("Boolean(document.querySelector('#app')?.__vue_app__)")
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<bool>()
            .map_err(|_| Error::Browser)?
        {
            break;
        }
        if attempt == 44 {
            return Err(Error::Browser);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if !page
        .evaluate(SELECT_COLLECTION)
        .await
        .map_err(|_| Error::Browser)?
        .into_value::<bool>()
        .map_err(|_| Error::Browser)?
    {
        return Err(Error::Browser);
    }
    for attempt in 0..20 {
        if page
            .evaluate("window.__INITIAL_STATE__?.user?.activeTab?._rawValue?.key === 1")
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<bool>()
            .map_err(|_| Error::Browser)?
        {
            break;
        }
        if attempt == 19 {
            return Err(Error::Browser);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut scan = Scan::default();
    let mut pending = HashMap::<String, Value>::new();
    let mut bodies = HashMap::<RequestId, std::sync::Arc<EventResponseReceived>>::new();
    let mut ended = HashSet::<RequestId>::new();
    let mut failed = HashSet::<RequestId>::new();
    let mut rejected = HashMap::<RequestId, Error>::new();
    let mut scoped = HashSet::<RequestId>::new();
    let mut last_progress = tokio::time::Instant::now();
    let mut tick = tokio::time::interval_at(last_progress + Duration::from_secs(5), Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // shortcut: bound this diagnostic run to 600 pages/15 minutes; remeasure before supporting larger catalogs.
    while scan.visited.len() < 600 {
        if let Some(id) = bodies.keys().find(|id| ended.contains(*id)).cloned() {
            let event = bodies.remove(&id).ok_or(Error::Incomplete)?;
            let url = collection_url(&event.response.url).ok_or(Error::InvalidData)?;
            let query = url.query_pairs().collect::<HashMap<_, _>>();
            if query.get("user_id").map(|v| v.as_ref()) != Some(auth.identity.as_str()) {
                return Err(Error::IdentityUnverified);
            }
            let cursor = query.get("cursor").map_or("", |v| v.as_ref()).to_owned();
            let body = response_body(page, &event).await?;
            if scan.visited.contains(&cursor) || pending.insert(cursor, body).is_some() {
                return Err(Error::Incomplete);
            }
            while let Some(body) = pending.remove(&scan.cursor) {
                let cursor = scan.cursor.clone();
                scan.accept(&cursor, &body)?;
                last_progress = tokio::time::Instant::now();
                let expected = page
                    .evaluate("window.__INITIAL_STATE__?.user?.activeSubTab?._rawValue?.count ?? null")
                    .await
                    .map_err(|_| Error::Browser)?
                    .into_value::<Option<u64>>()
                    .map_err(|_| Error::InvalidData)?;
                if scan.finish_window(expected)? {
                    return Ok(scan.items);
                }
            }
            continue;
        }
        tokio::select! {
            biased;
            event=extras.next()=>{
                let event=event.ok_or(Error::Browser)?;
                if let Some(error)=response_error(event.status_code) {
                    if scoped.contains(&event.request_id) {return Err(error);}
                    rejected.insert(event.request_id.clone(),error);
                }
            }
            event=requests.next()=>{
                let event=event.ok_or(Error::Browser)?;
                if event.request.method=="GET" && collection_url(&event.request.url).is_some() {
                    if let Some(error)=rejected.remove(&event.request_id) {return Err(error);}
                    if failed.contains(&event.request_id) {return Err(Error::Network);}
                    scoped.insert(event.request_id.clone());
                }
            }
            event=responses.next()=>{
                let event=event.ok_or(Error::Browser)?;
                if event.r#type==chromiumoxide::cdp::browser_protocol::network::ResourceType::Preflight {continue;}
                if collection_url(&event.response.url).is_some() {
                    bodies.insert(event.request_id.clone(),event);
                }
            }
            event=finished.next()=>{ended.insert(event.ok_or(Error::Browser)?.request_id.clone());}
            event=failures.next()=>{
                let id=event.ok_or(Error::Browser)?.request_id.clone();
                if scoped.contains(&id) {return Err(Error::Network);}
                failed.insert(id);
            }
            _=tick.tick()=>{
                challenge(page).await?;
                if last_progress.elapsed()>Duration::from_secs(60) {return Err(Error::Incomplete);}
                scroll(page).await?;
            }
        }
        if ended.len() > 10000 || failed.len() > 10000 {
            return Err(Error::Incomplete);
        }
    }
    Err(Error::Incomplete)
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

pub async fn detail(session: &Session, auth: &Auth, item: &Item) -> Result<Video> {
    if !valid_id(&item.id) {
        return Err(Error::InvalidData);
    }
    let mut url = Url::parse(&format!("{}/explore/{}", auth.origin, item.id)).map_err(|_| Error::InvalidData)?;
    url.query_pairs_mut()
        .append_pair("xsec_token", &item.token)
        .append_pair("xsec_source", "pc_user");
    navigate(&session.page, url.as_str()).await?;
    let expression = format!(
        "JSON.stringify(window.__INITIAL_STATE__?.note?.noteDetailMap?.[{}]?.note ?? null)",
        serde_json::to_string(&item.id).map_err(|_| Error::InvalidData)?
    );
    let mut note = Value::Null;
    for _ in 0..30 {
        challenge(&session.page).await?;
        let text = session
            .page
            .evaluate(expression.clone())
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<String>()
            .map_err(|_| Error::Browser)?;
        note = serde_json::from_str(&text).map_err(|_| Error::InvalidData)?;
        if !note.is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if note["noteId"] != item.id && note["note_id"] != item.id {
        return Err(Error::InvalidData);
    }
    Ok(Video {
        url: video_url(&note)?,
        title: note["title"].as_str().unwrap_or("").chars().take(1024).collect(),
        author: note["user"]["nickname"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(256)
            .collect(),
    })
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
pub async fn launch(root: &Path, browser: &Path) -> Result<Session> {
    let session = Session::launch(root, browser).await?;
    let configured = async {
        let ua = session
            .page
            .evaluate("navigator.userAgent")
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<String>()
            .map_err(|_| Error::Browser)?;
        session
            .page
            .execute(
                SetUserAgentOverrideParams::builder()
                    .user_agent(ua)
                    .accept_language("zh-CN,zh;q=0.9")
                    .build()
                    .map_err(|_| Error::Browser)?,
            )
            .await
            .map_err(|_| Error::Browser)?;
        session
            .page
            .execute(SetLocaleOverrideParams::builder().locale("zh_CN").build())
            .await
            .map_err(|_| Error::Browser)?;
        // RedNote redirects can replace the renderer before the self response is read.
        session
            .page
            .execute(
                EnableParams::builder()
                    .max_total_buffer_size(64 * 1024 * 1024)
                    .max_resource_buffer_size(8 * 1024 * 1024)
                    .enable_durable_messages(true)
                    .build(),
            )
            .await
            .map_err(|_| Error::Browser)?;
        Ok(())
    }
    .await;
    if let Err(error) = configured {
        let _ = session.close().await;
        return Err(error);
    }
    Ok(session)
}

pub async fn replace_cookies(session: &Session, auth: &Auth) -> Result<Vec<CookieParam>> {
    let current = session
        .page
        .execute(
            GetCookiesParams::builder()
                .urls([
                    "https://www.xiaohongshu.com",
                    "https://www.rednote.com",
                    "https://edith.xiaohongshu.com",
                    "https://webapi.rednote.com",
                ])
                .build(),
        )
        .await
        .map_err(|_| Error::Browser)?
        .result
        .cookies;
    let mut backup = Vec::new();
    for cookie in current {
        let mut value = serde_json::to_value(cookie).map_err(|_| Error::InvalidData)?;
        if value["expires"].as_f64().is_some_and(|v| v <= 0.) {
            value.as_object_mut().ok_or(Error::InvalidData)?.remove("expires");
        }
        backup.push(serde_json::from_value::<CookieParam>(value).map_err(|_| Error::InvalidData)?);
    }
    if let Err(error) = reset_cookies(session, auth.cookie_params()?).await {
        let _ = reset_cookies(session, backup).await;
        return Err(error);
    }
    Ok(backup)
}
pub async fn reset_cookies(session: &Session, cookies: Vec<CookieParam>) -> Result<()> {
    let current = session
        .page
        .execute(
            GetCookiesParams::builder()
                .urls([
                    "https://www.xiaohongshu.com",
                    "https://www.rednote.com",
                    "https://edith.xiaohongshu.com",
                    "https://webapi.rednote.com",
                ])
                .build(),
        )
        .await
        .map_err(|_| Error::Browser)?
        .result
        .cookies;
    for c in current {
        let domain = c.domain.trim_start_matches('.');
        if !["xiaohongshu.com", "rednote.com"]
            .iter()
            .any(|b| domain == *b || domain.ends_with(&format!(".{b}")))
        {
            continue;
        }
        session
            .page
            .execute(
                DeleteCookiesParams::builder()
                    .name(c.name)
                    .domain(c.domain)
                    .path(c.path)
                    .build()
                    .map_err(|_| Error::Browser)?,
            )
            .await
            .map_err(|_| Error::Browser)?;
    }
    session
        .page
        .execute(SetCookiesParams::new(cookies))
        .await
        .map_err(|_| Error::Browser)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
    #[tokio::test]
    #[ignore = "requires FAVKEEP_TEST_BROWSER; about:blank and synthetic cookies only"]
    async fn fresh_login_cookie_swap_can_restore_the_previous_session() {
        let browser = std::env::var_os("FAVKEEP_TEST_BROWSER").expect("set system Chromium");
        let dir = tempfile::tempdir().unwrap();
        let session = launch(dir.path(), Path::new(&browser)).await.unwrap();
        let auth = |value: &str| Auth {
            identity: "self".into(),
            origin: "https://www.rednote.com".into(),
            ua: "fixture".into(),
            cookies: vec![json!({"name":"web_session","value":value,"domain":".rednote.com","path":"/","secure":true})],
        };
        session
            .page
            .execute(SetCookiesParams::new(vec![
                CookieParam::builder()
                    .name("sessionid")
                    .value("unrelated")
                    .domain(".douyin.com")
                    .path("/")
                    .build()
                    .unwrap(),
            ]))
            .await
            .unwrap();
        replace_cookies(&session, &auth("old")).await.unwrap();
        let backup = replace_cookies(&session, &auth("new")).await.unwrap();
        let cookies = session
            .page
            .execute(GetCookiesParams::builder().urls(["https://www.rednote.com"]).build())
            .await
            .unwrap()
            .result
            .cookies;
        assert!(cookies.iter().any(|c| c.name == "web_session" && c.value == "new"));
        reset_cookies(&session, backup).await.unwrap();
        let cookies = session
            .page
            .execute(
                GetCookiesParams::builder()
                    .urls(["https://www.rednote.com", "https://www.douyin.com"])
                    .build(),
            )
            .await
            .unwrap()
            .result
            .cookies;
        assert!(cookies.iter().any(|c| c.name == "web_session" && c.value == "old"));
        assert!(cookies.iter().any(|c| c.name == "sessionid" && c.value == "unrelated"));
        session.close().await.unwrap();
    }
    #[test]
    fn official_unauthorized_requires_login_while_rejection_requires_cooldown() {
        assert_eq!(response_error(401), Some(Error::LoginExpired));
        for status in [403, 406, 429, 461, 471] {
            assert_eq!(response_error(status), Some(Error::Rejected));
        }
        assert_eq!(response_error(200), None);
        assert_eq!(response_error(500), None);
    }
}

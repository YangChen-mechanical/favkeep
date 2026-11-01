// SPDX-License-Identifier: MIT
// Passport/profile reference: cv-cat/catbus bcc72f81549b94090b340fb87f29927529056e49; see catbus-LICENSE.
pub(crate) mod ac_sign;
use super::codec;
pub(crate) mod passport_sign;
use super::cookies::Jar;
use super::{Error, Result, valid_id};
use parking_lot::Mutex;
use passport_sign as sign;
pub use passport_sign::UA;
use reqwest::{Client, Url, header, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;

const WWW: &str = "https://www.douyin.com";
const DOMAINS: &[&str] = &[
    "douyin.com",
    "www.douyin.com",
    "login.douyin.com",
    "sso.douyin.com",
    "bytedance.com",
    "ttwid.bytedance.com",
    "mssdk.bytedance.com",
];
const HOSTS: &[&str] = &[
    "www.douyin.com",
    "login.douyin.com",
    "sso.douyin.com",
    "ttwid.bytedance.com",
    "mssdk.bytedance.com",
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Auth {
    pub identity: String,
    pub ua: String,
    pub cookies: Vec<Value>,
    pub origin: String,
    #[serde(default)]
    pub ms_token: String,
    #[serde(default)]
    pub fingerprint: String,
}
impl Auth {
    pub fn read(path: &Path) -> Result<Self> {
        if std::fs::metadata(path).map_err(Error::from)?.len() > 1024 * 1024 {
            return Err(Error::InvalidData);
        }
        let auth: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(Error::from)?).map_err(|_| Error::InvalidData)?;
        auth.validate()?;
        Ok(auth)
    }
    fn validate(&self) -> Result<()> {
        if !valid_id(&self.identity)
            || self.identity == "0"
            || self.origin != WWW
            || self.ua.is_empty()
            || self.ua.len() > 1024
            || !self.ua.bytes().all(|b| (32..127).contains(&b))
        {
            return Err(Error::InvalidData);
        }
        let jar = Jar::new(DOMAINS);
        jar.load(&self.cookies)?;
        Ok(())
    }
}

pub struct Session {
    client: Client,
    jar: Arc<Jar>,
    token: Mutex<String>,
    counter: AtomicU32,
}
impl Session {
    pub fn new(saved: Option<&Auth>) -> Result<Self> {
        let jar = Arc::new(Jar::new(DOMAINS));
        if let Some(saved) = saved {
            saved.validate()?;
            jar.load(&saved.cookies)?;
        }
        let client = Client::builder()
            .user_agent(sign::UA)
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
            token: Mutex::new(saved.map(|a| a.ms_token.clone()).unwrap_or_default()),
            counter: AtomicU32::new(2),
        })
    }
    async fn response(&self, builder: reqwest::RequestBuilder) -> Result<Response> {
        let request = builder.build().map_err(|_| Error::InvalidData)?;
        if !trusted(request.url()) {
            return Err(Error::InvalidData);
        }
        let mut response = self.client.execute(request).await.map_err(|_| Error::Network)?;
        let status = response.status();
        if status.is_server_error() {
            return Err(Error::Network);
        }
        if !status.is_success() && !status.is_redirection() {
            return Err(Error::Rejected);
        }
        let headers = response.headers().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
            if chunk.len() > (8 * 1024 * 1024usize).saturating_sub(bytes.len()) {
                return Err(Error::InvalidData);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes).ok();
        if value.is_none() {
            html_state(&bytes)?;
        }
        Ok(Response {
            status: status.as_u16(),
            headers,
            bytes,
            value,
        })
    }
    async fn json(&self, builder: reqwest::RequestBuilder) -> Result<Value> {
        let response = self.response(builder).await?;
        if response.status != 200 {
            return Err(Error::Rejected);
        }
        let value = response.value.ok_or(Error::InvalidData)?;
        if value["status_code"].as_i64().is_some_and(|n| n != 0)
            || value["data"]["error_code"].as_i64().is_some_and(|n| n != 0)
            || value["message"] == "error"
        {
            return Err(Error::Rejected);
        }
        Ok(value)
    }
    fn cookie(&self, name: &str, origin: &str) -> Result<Option<String>> {
        self.jar
            .value(name, &Url::parse(origin).map_err(|_| Error::InvalidData)?)
    }
    fn visitor_cookie(&self) -> Result<Option<String>> {
        for name in ["UIFID", "UIFID_TEMP"] {
            if let Some(value) = self.cookie(name, WWW)?
                && (100..=512).contains(&value.len())
                && value.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    async fn mint_token(&self) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let response = self
            .response(
                self.client
                    .post("https://mssdk.bytedance.com/web/r/token?ms_appid=6383")
                    .header(header::REFERER, format!("{WWW}/"))
                    .header(header::ORIGIN, WWW)
                    .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
                    .header("sec-ch-ua-platform", "\"Windows\"")
                    .header("sec-ch-ua", sign::CH_UA)
                    .header("sec-ch-ua-mobile", "?0")
                    .header(header::ACCEPT, "*/*")
                    .header(header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
                    .header(header::CACHE_CONTROL, "no-cache")
                    .header("pragma", "no-cache")
                    .header("priority", "u=1, i")
                    .header("sec-fetch-dest", "empty")
                    .header("sec-fetch-mode", "cors")
                    .header("sec-fetch-site", "cross-site")
                    .body(
                        report(
                            now,
                            &uuid::Uuid::new_v4().to_string(),
                            rand::random_range(40.0..160.0),
                            rand::random(),
                        )
                        .map_err(|_| Error::InvalidData)?,
                    ),
            )
            .await?;
        if response.status != 200 {
            return Err(Error::Rejected);
        }
        let token = response
            .headers
            .get("x-ms-token")
            .and_then(|h| h.to_str().ok())
            .or_else(|| {
                response
                    .headers
                    .get_all(header::SET_COOKIE)
                    .iter()
                    .filter_map(|h| h.to_str().ok())
                    .find_map(|h| h.strip_prefix("msToken=").and_then(|v| v.split(';').next()))
            })
            .filter(|v| valid_token(v))
            .ok_or(Error::TokenUnavailable)?;
        *self.token.lock() = token.to_owned();
        Ok(())
    }
    async fn visitor_document(&self) -> Result<Response> {
        let response = self
            .response(
                self.client
                    .get(format!("{WWW}/jingxuan"))
                    .header(
                        header::ACCEPT,
                        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                    )
                    .header(header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
                    .header(header::REFERER, format!("{WWW}/?recommend=1"))
                    .header("sec-ch-ua", sign::CH_UA)
                    .header("sec-ch-ua-platform", "\"Windows\"")
                    .header("sec-ch-ua-mobile", "?0")
                    .header("sec-fetch-dest", "document")
                    .header("sec-fetch-mode", "navigate")
                    .header("sec-fetch-site", "same-origin")
                    .header("upgrade-insecure-requests", "1"),
            )
            .await?;
        if response.status != 200 {
            return Err(Error::Rejected);
        }
        Ok(response)
    }
    async fn prepare_visitor(&self, fresh: bool) -> Result<()> {
        if self.visitor_cookie()?.is_some() {
            return Ok(());
        }
        let document = self.visitor_document().await?;
        if fresh && html_state(&document.bytes)? {
            let nonce = self
                .cookie("__ac_nonce", WWW)?
                .filter(|n| (20..=64).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or(Error::TokenUnavailable)?;
            let signature = ac_sign::signature(
                &nonce,
                &format!("{WWW}/jingxuan"),
                sign::UA,
                chrono::Utc::now().timestamp() as u64,
            );
            let url = Url::parse(WWW).map_err(|_| Error::InvalidData)?;
            self.jar.set(
                &format!("__ac_signature={signature}; Max-Age=31536000; Path=/; Secure; SameSite=None"),
                &url,
            )?;
            self.jar.set(
                &format!("__ac_referer={WWW}/?recommend=1; Max-Age=31536000; Path=/; Secure; SameSite=None"),
                &url,
            )?;
            self.visitor_document().await?;
        }
        self.visitor_cookie()?.ok_or(Error::TokenUnavailable)?;
        Ok(())
    }
    pub async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value> {
        if !path.starts_with("/aweme/v1/web/") || path.contains(['?', '#']) {
            return Err(Error::InvalidData);
        }
        let mut pairs: Vec<(&str, String)> = [
            ("device_platform", "webapp"),
            ("aid", "6383"),
            ("channel", "channel_pc_web"),
            ("pc_client_type", "1"),
            ("version_code", "290100"),
            ("version_name", "29.1.0"),
            ("cookie_enabled", "true"),
            ("screen_width", "2560"),
            ("screen_height", "1440"),
            ("browser_language", "zh-CN"),
            ("browser_platform", "Win32"),
            ("browser_name", "Chrome"),
            ("browser_version", "155.0.0.0"),
            ("browser_online", "true"),
            ("engine_name", "Blink"),
            ("engine_version", "155.0.0.0"),
            ("os_name", "Windows"),
            ("os_version", "10"),
            ("cpu_core_num", "20"),
            ("device_memory", "32"),
            ("platform", "PC"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_owned()))
        .collect();
        let ms = self.token.lock().clone();
        pairs.push(("msToken", ms));
        pairs.extend_from_slice(params);
        let query = sign::query(&pairs);
        let counter = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let bogus = sign::abogus(&query, "", counter, sign::Target::Main);
        let query = format!("{query}&{}", sign::query(&[("a_bogus", bogus)]));
        let uifid = self.visitor_cookie()?;
        let query = uifid
            .as_ref()
            .map(|v| sign::web_sign(&query, v, chrono::Utc::now().timestamp()))
            .unwrap_or(query);
        let mut builder = self.client.get(format!("{WWW}{path}?{query}"));
        if let Some(value) = uifid {
            builder = builder.header("uifid", secret_header(&value)?);
        }

        self.json(
            builder
                .header(header::REFERER, format!("{WWW}/"))
                .header(header::ACCEPT, "application/json, text/plain, */*"),
        )
        .await
    }
    pub async fn verify(&self, expected: Option<&str>) -> Result<Auth> {
        let session = self
            .cookie("sessionid", WWW)?
            .filter(|s| !s.is_empty())
            .or(self.cookie("sessionid_ss", WWW)?.filter(|s| !s.is_empty()));
        if session.is_none() {
            return Err(Error::LoginExpired);
        }
        let payload = self.get("/aweme/v1/web/user/profile/self/", &[]).await?;
        let identity = self_identity(&payload)?;
        if expected.is_some_and(|e| e != identity) {
            return Err(Error::AccountChanged);
        }
        Ok(Auth {
            identity: identity.to_owned(),
            ua: sign::UA.to_owned(),
            cookies: self.jar.snapshot(),
            origin: WWW.to_owned(),
            ms_token: self.token.lock().clone(),
            fingerprint: String::new(),
        })
    }
    pub async fn restore(&self, saved: &Auth) -> Result<Auth> {
        self.mint_token().await?;
        let auth = self.verify(Some(&saved.identity)).await?;
        self.prepare_visitor(false).await?;
        Ok(Auth {
            cookies: self.jar.snapshot(),
            ..auth
        })
    }
    pub async fn login(&self, expected: Option<&str>, updates: &watch::Sender<Option<String>>) -> Result<Auth> {
        self.json(self.client.post("https://ttwid.bytedance.com/ttwid/union/register/")
            .header(header::CONTENT_TYPE,"application/json")
            .body(serde_json::to_string(&json!({"region":"cn","aid":1768,"needFid":false,"service":"www.douyin.com","migrate_info":{"ticket":"","source":"node"},"cbUrlProtocol":"https","union":true})).map_err(|_|Error::InvalidData)?)).await?;
        let ttwid = self
            .cookie("ttwid", "https://ttwid.bytedance.com")?
            .filter(|s| !s.is_empty() && s.len() <= 1024)
            .ok_or(Error::TokenUnavailable)?;
        self.jar.set(
            &format!("ttwid={ttwid}; Domain=.douyin.com; Path=/; Secure"),
            &Url::parse(WWW).map_err(|_| Error::InvalidData)?,
        )?;
        self.mint_token().await?;
        self.prepare_visitor(true).await?;
        let now = chrono::Utc::now().timestamp_millis();
        let ts = sign::sdk_ts(now).map_err(|_| Error::InvalidData)?;
        let trace = hex::encode(rand::random::<[u8; 4]>());
        self.jar.set(
            &format!("biz_trace_id={trace}; Domain=.douyin.com; Path=/; Secure"),
            &Url::parse(WWW).map_err(|_| Error::InvalidData)?,
        )?;
        let mut pairs: Vec<(&str, String)> = [
            ("passport_jssdk_version", "3.4.9"),
            ("passport_jssdk_type", "normal"),
            ("is_from_ttaccountsdk", "1"),
            ("aid", "6383"),
            ("language", "zh"),
            ("account_app_language", "zh-CN"),
            ("next", WWW),
            ("need_short_url", "true"),
            ("need_logo", "false"),
            ("is_new_login", "1"),
            ("is_from_iesaccountsaas", "1"),
            ("p_ui", "2.4.10"),
            ("account_sdk_source", "web"),
            ("p_js_v", "3.4.9"),
            ("p_js_t", "pro"),
            ("p_zt", "3.3.30"),
            ("p_ver", "1.1.4-beta.5"),
            ("p_ver_real", "0"),
            ("request_host", "https%3A%2F%2Fwww.douyin.com"),
            ("p_bd", "1.0.1.19-fix.01"),
            ("device_platform", "web_app"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_owned()))
        .collect();
        pairs.extend([
            ("ts", ts.clone()),
            (
                "account_sdk_source_info",
                source_info(now).map_err(|_| Error::InvalidData)?,
            ),
            ("p_ts", now.to_string()),
            ("p_no", hex::encode(rand::random::<[u8; 16]>())),
            ("biz_trace_id", trace.clone()),
        ]);
        pairs.retain(|(k, _)| {
            ![
                "next",
                "need_short_url",
                "need_logo",
                "is_new_login",
                "is_from_iesaccountsaas",
                "ts",
                "p_ts",
                "p_no",
            ]
            .contains(k)
        });
        pairs.push(("msToken", self.token.lock().clone()));

        let portrait = format!("{}.login", uuid::Uuid::new_v4());
        let login = Login {
            session: self,
            base: pairs,
            trace,
            portrait,
        };
        let qr = login
            .call(
                "/passport/web/get_qrcode/",
                &[
                    ("next", WWW.to_owned()),
                    ("need_short_url", "true".into()),
                    ("need_logo", "false".into()),
                    ("is_new_login", "1".into()),
                    ("is_from_iesaccountsaas", "1".into()),
                ],
                &[],
            )
            .await?;
        let url = qr["data"]["qrcode_index_url"].as_str().ok_or(Error::InvalidData)?;
        if !qr_url(url) {
            return Err(Error::InvalidData);
        }
        updates.send_replace(Some(url.to_owned()));
        let token = qr["data"]["token"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or(Error::InvalidData)?;
        let fp = verify_fp();
        let extra = [
            ("is_from_iesaccountsaas", "1".to_owned()),
            ("p_ca", "4.0.26".to_owned()),
            ("p_ca_real", "1.0.0.938".to_owned()),
            ("fp", fp.clone()),
            ("verifyFp", fp),
        ];
        let body = [
            ("need_logo", "false".to_owned()),
            ("is_frontier", "true".into()),
            ("token", token.to_owned()),
            ("is_new_login", "1".into()),
            ("next", WWW.to_owned()),
            ("need_short_url", "true".into()),
        ];
        for _ in 0..36 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let poll = login.call("/passport/web/check_qrconnect/", &extra, &body).await?;
            match qr_state(&poll)? {
                "confirmed" => {
                    self.finish(poll["data"]["redirect_url"].as_str().ok_or(Error::InvalidData)?)
                        .await?;
                    return self.verify(expected).await;
                }
                "new" | "scanned" => {}
                _ => return Err(Error::InvalidData),
            }
        }
        Err(Error::LoginExpired)
    }
    async fn finish(&self, redirect: &str) -> Result<()> {
        let mut url = Url::parse(redirect).map_err(|_| Error::InvalidData)?;
        for _ in 0..5 {
            if !login_redirect(&url) {
                return Err(Error::InvalidData);
            }
            let response = self
                .response(self.client.get(url.clone()).header(header::REFERER, format!("{WWW}/")))
                .await?;
            if [301, 302, 303, 307, 308].contains(&response.status) {
                let location = response
                    .headers
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(Error::Rejected)?;
                url = url.join(location).map_err(|_| Error::InvalidData)?;
            } else if response.status == 200 {
                return Ok(());
            } else {
                return Err(Error::Rejected);
            }
        }
        Err(Error::Rejected)
    }
}
struct Login<'a> {
    session: &'a Session,
    base: Vec<(&'static str, String)>,
    trace: String,
    portrait: String,
}
impl Login<'_> {
    async fn call(&self, path: &str, extra: &[(&str, String)], body: &[(&str, String)]) -> Result<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        let ts = sign::sdk_ts(now).map_err(|_| Error::InvalidData)?;
        let mut pairs = self.base.clone();
        pairs.extend_from_slice(extra);
        pairs.extend([
            ("ts", ts.clone()),
            ("p_ts", now.to_string()),
            ("p_no", hex::encode(rand::random::<[u8; 16]>())),
        ]);
        let (seal, qs) = sign::seal(&pairs, body);
        pairs.extend([("sign", seal), ("qs", qs)]);
        let query = sign::query(&pairs);
        let encoded = sign::query(body);
        let counter = self.session.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let bogus = sign::abogus(&query, &encoded, counter, sign::Target::Login);
        let url = format!(
            "https://login.douyin.com{path}?{query}&{}",
            sign::query(&[("a_bogus", bogus)])
        );
        let client = &self.session.client;
        let builder = if body.is_empty() {
            client.get(url)
        } else {
            client
                .post(url)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(encoded)
        };
        self.session
            .json(
                builder
                    .header("web-sdk-version", "1")
                    .header("sec-ch-ua-platform", "\"Windows\"")
                    .header("sec-ch-ua", sign::CH_UA)
                    .header("sec-ch-ua-mobile", "?0")
                    .header(header::REFERER, format!("{WWW}/"))
                    .header(header::ORIGIN, WWW)
                    .header("x-tt-passport-aid-sign", secret_header(&sign::aid_sign(path, &ts))?)
                    .header(
                        "x-tt-passport-csrf-token",
                        secret_header(
                            &self
                                .session
                                .cookie("passport_csrf_token", "https://login.douyin.com")?
                                .or(self
                                    .session
                                    .cookie("passport_csrf_token_default", "https://login.douyin.com")?)
                                .unwrap_or_default(),
                        )?,
                    )
                    .header("x-tt-passport-trace-id", secret_header(&self.trace)?)
                    .header("x-tt-passport-verify-portrait", secret_header(&self.portrait)?)
                    .header(header::ACCEPT, "application/json, text/javascript")
                    .header(header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
                    .header("priority", "u=1, i")
                    .header("sec-fetch-dest", "empty")
                    .header("sec-fetch-mode", "cors")
                    .header("sec-fetch-site", "same-site"),
            )
            .await
    }
}
struct Response {
    status: u16,
    headers: header::HeaderMap,
    bytes: Vec<u8>,
    value: Option<Value>,
}
fn secret_header(value: &str) -> Result<header::HeaderValue> {
    let mut h = header::HeaderValue::from_str(value).map_err(|_| Error::InvalidData)?;
    h.set_sensitive(true);
    Ok(h)
}
fn trusted(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && HOSTS.contains(&url.host_str().unwrap_or(""))
}
fn login_redirect(url: &Url) -> bool {
    trusted(url)
        && ["www.douyin.com", "login.douyin.com", "sso.douyin.com"].contains(&url.host_str().unwrap_or(""))
        && (url.path() == "/" || url.path().starts_with("/passport/"))
}
fn qr_url(raw: &str) -> bool {
    Url::parse(raw).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("api.amemv.com")
            && u.username().is_empty()
            && u.password().is_none()
            && u.port_or_known_default() == Some(443)
            && u.path() == "/ucenter_web/app/aweme/scan_login/index/douyin_scan_code_login/cn/app/index.html"
    })
}
pub(super) fn valid_token(value: &str) -> bool {
    (100..=512).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b',')
}
fn qr_state(value: &Value) -> Result<&str> {
    if value["data"]["error_code"] != 0 || value["message"] != "success" {
        return Err(Error::Rejected);
    }
    match value["data"]["status"].as_str() {
        Some(s @ ("new" | "scanned" | "confirmed")) => Ok(s),
        Some("expired" | "cancelled") => Err(Error::LoginExpired),
        _ => Err(Error::InvalidData),
    }
}
fn self_identity(value: &Value) -> Result<&str> {
    if value["status_code"] != 0 {
        return Err(Error::IdentityUnverified);
    }
    value["user"]["sec_uid"]
        .as_str()
        .filter(|s| valid_id(s) && *s != "0")
        .ok_or(Error::IdentityUnverified)
}
fn html_state(bytes: &[u8]) -> Result<bool> {
    use std::sync::LazyLock;
    static SCRIPT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?is)<script\b[^>]*>.*?</script>|<style\b[^>]*>.*?</style>").unwrap());
    static TAG: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?s)<[^>]*>").unwrap());
    let raw = String::from_utf8_lossy(bytes);
    let clean = SCRIPT.replace_all(&raw, " ");
    let visible = TAG.replace_all(&clean, " ").to_ascii_lowercase();
    if ["ip at risk", "请完成安全验证", "安全限制", "验证码中间页"]
        .iter()
        .any(|s| visible.contains(s))
    {
        return Err(Error::Challenge);
    }
    Ok(raw.contains("__ac_nonce") && raw.contains("__ac_signature") && visible.trim().is_empty())
}
fn report(now: i64, uuid: &str, collect_time: f64, nonce: u8) -> anyhow::Result<String> {
    // The vendored public device profile contains no account, Cookie, or minted token.
    let mut profile: Value = serde_json::from_str(include_str!("mstoken-profile.json"))?;
    let n = &mut profile["nWID"];
    n["navigator"]["userAgent"] = json!(sign::UA);
    n["audio"]["audioContext"]["state"] = json!("running");
    n["custom"] = json!(serde_json::to_string(&json!({
        "version": n["ms_version"], "fxgDid":"", "uuid":uuid,
        "collectTime":collect_time,"aid":6383,"pageId":6241,
    }))?);
    let plaintext = serde_json::to_string(&json!({"nWID":profile["nWID"],"wID":profile["wID"]}))?;
    Ok(serde_json::to_string(
        &json!({"magic":538969122,"version":1,"dataType":8,
        "strData":sign::str_data(&plaintext, nonce),"tspFromClient":now,"ulr":0}),
    )?)
}

fn source_info(now: i64) -> anyhow::Result<String> {
    Ok(sign::encrypt(&serde_json::to_string(&json!({
        "hardwareConcurrency":20,"webdriver":false,"chromedriver":false,"shelldriver":false,"plugins":5,
        "innerHeight":1215,"innerWidth":2560,"outerHeight":1392,"outerWidth":2560,
        "webgl":{"vendor":"Google Inc. (NVIDIA)","renderer":"ANGLE (NVIDIA, NVIDIA GeForce RTX 5060 Ti (0x00002D04) Direct3D11 vs_5_0 ps_5_0, D3D11)"},
        "automation":{"s":"00000000","c":"0000","p":"0000000","s1":"00000000","c1":"0000","p1":"0"},
        "performance":{"timeOrigin":(now-3000) as f64,"usedJSHeapSize":472537551,
            "navigationTiming":{"decodedBodySize":968454,"entryType":"navigation","initiatorType":"navigation",
                "name":"https://www.douyin.com/?recommend=1","renderBlockingStatus":"non-blocking",
                "serverTiming":"cdn-cache,edge,origin,inner,tt_agw","guleStart":610.8999999761581,"guleDuration":"none"}},
        "browser":{"t":format!("{:013}",rand::random_range(0..10_000_000_000_000u64)),"bit_protocol":"false","bit_helper":false}
    }))?))
}

fn verify_fp() -> String {
    let mut n = chrono::Utc::now().timestamp_millis() as u64;
    let mut time = Vec::new();
    while n > 0 {
        time.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    time.reverse();
    let random = format!("{:032x}", rand::random::<u128>());
    format!(
        "verify_{}_{}_{}_4{}_a{}_{}",
        String::from_utf8(time).unwrap(),
        &random[..8],
        &random[8..12],
        &random[13..16],
        &random[17..20],
        &random[20..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_identity_qr_and_redirect_boundaries() {
        assert_eq!(
            self_identity(&json!({"status_code":0,"user":{"sec_uid":"self_123"}})).unwrap(),
            "self_123"
        );
        for value in [
            json!({"status_code":1,"user":{"sec_uid":"self_123"}}),
            json!({"status_code":0,"user":{"sec_uid":"0"}}),
            json!({"status_code":0,"user":{"uid":"123"}}),
        ] {
            assert_eq!(self_identity(&value), Err(Error::IdentityUnverified));
        }
        for state in ["new", "scanned", "confirmed"] {
            assert_eq!(
                qr_state(&json!({"message":"success","data":{"error_code":0,"status":state}})).unwrap(),
                state
            );
        }
        for state in ["expired", "cancelled"] {
            assert_eq!(
                qr_state(&json!({"message":"success","data":{"error_code":0,"status":state}})),
                Err(Error::LoginExpired)
            );
        }
        assert_eq!(
            qr_state(&json!({"message":"error","data":{"error_code":4031,"status":"confirmed"}})),
            Err(Error::Rejected)
        );
        assert!(login_redirect(
            &Url::parse("https://www.douyin.com/passport/sso/login/callback/?ticket=synthetic").unwrap()
        ));
        for raw in [
            "http://www.douyin.com/",
            "https://www.douyin.com.evil.test/",
            "https://user@login.douyin.com/",
            "https://login.douyin.com:444/",
            "https://mssdk.bytedance.com/",
            "https://www.douyin.com/user/self",
        ] {
            assert!(!login_redirect(&Url::parse(raw).unwrap()));
        }
        assert!(!qr_url("https://evil.test/?token=synthetic"));
    }

    #[test]
    fn ordinary_initialization_is_distinct_from_visible_security_verification() {
        assert_eq!(html_state(b"<script>__ac_nonce;__ac_signature;</script>"), Ok(true));
        assert_eq!(
            html_state("<script>const text='请完成安全验证';</script><p>正常页面</p>".as_bytes()),
            Ok(false)
        );
        assert_eq!(html_state("<p>请完成安全验证</p>".as_bytes()), Err(Error::Challenge));
        assert_eq!(html_state(b"<p>IP at risk</p>"), Err(Error::Challenge));
    }

    #[test]
    fn cookie_scope_and_metadata_survive_native_restore_without_cross_platform_leaks() {
        use reqwest::cookie::CookieStore;
        let jar = Jar::new(DOMAINS);
        let www = Url::parse(WWW).unwrap();
        let login = Url::parse("https://login.douyin.com/passport/test").unwrap();
        jar.set(
            "sessionid=synthetic; Domain=.douyin.com; Path=/; Secure; HttpOnly; Max-Age=600; SameSite=Lax",
            &www,
        )
        .unwrap();
        jar.set("passport_csrf_token=private; Path=/passport; Secure", &login)
            .unwrap();
        let restored = Jar::new(DOMAINS);
        restored.load(&jar.snapshot()).unwrap();
        assert_eq!(
            restored.value("sessionid", &login).unwrap().as_deref(),
            Some("synthetic")
        );
        assert!(restored.value("passport_csrf_token", &www).unwrap().is_none());
        for raw in [
            "http://www.douyin.com/",
            "https://www.rednote.com/",
            "https://mssdk.bytedance.com/",
            "https://v26-web.douyinvod.com/",
        ] {
            assert!(restored.cookies(&Url::parse(raw).unwrap()).is_none());
        }
        assert!(jar.set("sessionid=foreign; Domain=.rednote.com; Path=/", &www).is_err());
        assert!(jar.snapshot().iter().any(|c| c["domain"] == ".douyin.com"
            && c["httpOnly"] == true
            && c["sameSite"] == "Lax"
            && c["expires"].as_i64().unwrap() > 0));
    }
}

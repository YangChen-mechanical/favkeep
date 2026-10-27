use std::path::Path;
use std::time::Duration;

use base64::Engine;
use chromiumoxide::cdp::browser_protocol::network::{
    CookieParam, DeleteCookiesParams, EventResponseReceived, GetResponseBodyParams,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinHandle;

use super::{Error, Result, valid_id};

#[derive(Serialize, Deserialize, Clone)]
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
        let bytes = std::fs::read(path).map_err(|_| Error::File)?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::InvalidData);
        }
        let auth: Self = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidData)?;
        if !valid_id(&auth.identity)
            || auth.origin != "https://www.douyin.com"
            || !auth.ua.is_ascii()
            || auth.ua.len() > 1024
        {
            return Err(Error::InvalidData);
        }
        auth.cookie_header()?;
        Ok(auth)
    }

    pub fn cookie_header(&self) -> Result<String> {
        let mut parts = Vec::new();
        for cookie in &self.cookies {
            let domain = cookie["domain"].as_str().unwrap_or("").trim_start_matches('.');
            if domain != "douyin.com" && domain != "www.douyin.com" {
                continue;
            }
            let name = cookie["name"].as_str().unwrap_or("");
            let value = cookie["value"].as_str().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !value.is_ascii()
                || value.bytes().any(|b| b < 32 || b == 127 || b == b';')
            {
                return Err(Error::InvalidData);
            }
            parts.push(format!("{name}={value}"));
        }
        if parts.is_empty() {
            return Err(Error::LoginExpired);
        }
        Ok(parts.join("; "))
    }
}

pub fn private_dir(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root).map_err(Error::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).map_err(|_| Error::File)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let account = format!(
            "{}\\{}:(OI)(CI)F",
            std::env::var("USERDOMAIN").map_err(|_| Error::File)?,
            std::env::var("USERNAME").map_err(|_| Error::File)?
        );
        let result = std::process::Command::new("icacls")
            .arg(root)
            .args(["/inheritance:r", "/grant:r", &account])
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| Error::File)?;
        if !result.status.success() {
            return Err(Error::File);
        }
        // icacls preserves other explicit ACEs; reject shared directories before any secret write.
        let check = "$ErrorActionPreference='Stop'; $a=[System.IO.Directory]::GetAccessControl($env:FAVKEEP_PRIVATE_DIR); $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; if(!$a.AreAccessRulesProtected){exit 1}; foreach($r in $a.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($me,'S-1-5-18','S-1-5-32-544')){exit 1}}";
        let bytes: Vec<u8> = check.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let checked = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .env("FAVKEEP_PRIVATE_DIR", root)
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| Error::File)?;
        if !checked.status.success() {
            return Err(Error::File);
        }
    }
    Ok(())
}

pub fn write_private(path: &Path, data: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or(Error::File)?;
    private_dir(parent)?;
    let bytes = serde_json::to_vec_pretty(data).map_err(|_| Error::InvalidData)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(Error::from)?;
    file.write_all(&bytes).map_err(Error::from)?;
    file.flush().map_err(Error::from)?;
    file.as_file().sync_all().map_err(Error::from)?;
    file.persist(path).map_err(|e| Error::from(e.error))?;
    Ok(())
}

pub struct Session {
    browser: Browser,
    pub page: Page,
    handler: JoinHandle<()>,
}

impl Session {
    pub async fn launch(root: &Path, executable: &Path) -> Result<Self> {
        private_dir(root)?;
        let profile = root.join("browser");
        private_dir(&profile)?;
        let config = BrowserConfig::builder()
            .chrome_executable(executable)
            .user_data_dir(profile)
            .port(0)
            .new_headless_mode()
            .respect_https_errors()
            .window_size(1280, 900)
            .launch_timeout(Duration::from_secs(45))
            .request_timeout(Duration::from_secs(25))
            .arg("--lang=zh-CN")
            .build()
            .map_err(|_| Error::Browser)?;
        let (mut browser, mut handler) = Browser::launch(config).await.map_err(|_| Error::Browser)?;
        let handler = tokio::spawn(async move {
            while let Some(result) = handler.next().await {
                if result.is_err() {
                    break;
                }
            }
        });
        match browser.new_page("about:blank").await {
            Ok(page) => Ok(Self { browser, page, handler }),
            Err(_) => {
                let _ = browser.kill().await;
                handler.abort();
                Err(Error::Browser)
            }
        }
    }

    async fn clear_cookies(&self) -> Result<()> {
        for cookie in self.browser.get_cookies().await.map_err(|_| Error::Browser)? {
            if matches!(cookie.domain.trim_start_matches('.'), "douyin.com" | "www.douyin.com") {
                self.page
                    .execute(
                        DeleteCookiesParams::builder()
                            .name(cookie.name)
                            .domain(cookie.domain)
                            .path(cookie.path)
                            .build()
                            .map_err(|_| Error::Browser)?,
                    )
                    .await
                    .map_err(|_| Error::Browser)?;
            }
        }
        Ok(())
    }

    async fn seed(&self, saved: &Auth, explicit: bool) -> Result<Option<Vec<CookieParam>>> {
        let current = self.browser.get_cookies().await.map_err(|_| Error::Browser)?;
        if !explicit && current.iter().any(|c| c.name == "sessionid" && !c.value.is_empty()) {
            return Ok(None);
        }
        fn params(values: &[Value]) -> Result<Vec<CookieParam>> {
            let mut cookies = Vec::new();
            for value in values {
                let domain = value["domain"].as_str().unwrap_or("").trim_start_matches('.');
                if !matches!(domain, "douyin.com" | "www.douyin.com") || value["name"].as_str().unwrap_or("").is_empty()
                {
                    continue;
                }
                let mut value = value.clone();
                if value["expires"].as_f64().is_some_and(|n| n <= 0.0) {
                    value.as_object_mut().ok_or(Error::InvalidData)?.remove("expires");
                }
                cookies.push(serde_json::from_value::<CookieParam>(value).map_err(|_| Error::InvalidData)?);
            }
            Ok(cookies)
        }
        saved.cookie_header()?;
        let cookies = params(&saved.cookies)?;
        let backup = params(
            &current
                .into_iter()
                .map(|c| serde_json::to_value(c).map_err(|_| Error::InvalidData))
                .collect::<Result<Vec<_>>>()?,
        )?;
        self.clear_cookies().await?;
        if self.browser.set_cookies(cookies).await.is_err() {
            self.clear_cookies().await?;
            self.browser.set_cookies(backup).await.map_err(|_| Error::Browser)?;
            return Err(Error::Browser);
        }
        Ok(Some(backup))
    }

    pub async fn restore(&self, saved: &Auth, explicit: bool) -> Result<Auth> {
        let backup = self.seed(saved, explicit).await?;
        let result = self.observe_identity(Some(&saved.identity), None).await;
        if result.is_err()
            && let Some(cookies) = backup
        {
            self.clear_cookies().await?;
            self.browser.set_cookies(cookies).await.map_err(|_| Error::Browser)?;
        }
        result
    }

    pub async fn login(&self, identity: Option<&str>, qr: &tokio::sync::watch::Sender<Option<String>>) -> Result<Auth> {
        self.observe_identity(identity, Some(qr)).await
    }

    async fn login_image(&self) -> Result<Option<String>> {
        let state = self
            .page
            .evaluate(LOGIN_QR)
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<String>()
            .map_err(|_| Error::Browser)?;
        match state.as_str() {
            "challenge" => Err(Error::Challenge),
            "expired" => Err(Error::LoginExpired),
            "qr" => {
                let Ok(element) = self.page.find_element("[data-favkeep-login-qr]").await else {
                    return Ok(None);
                };
                let Ok(image) = element.screenshot(CaptureScreenshotFormat::Png).await else {
                    return Ok(None);
                };
                if image.len() > 1024 * 1024 {
                    return Err(Error::InvalidData);
                }
                Ok(Some(format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(image)
                )))
            }
            _ => Ok(None),
        }
    }

    async fn observe_identity(
        &self,
        expected_identity: Option<&str>,
        qr: Option<&tokio::sync::watch::Sender<Option<String>>>,
    ) -> Result<Auth> {
        let mut responses = self
            .page
            .event_listener::<EventResponseReceived>()
            .await
            .map_err(|_| Error::Browser)?;
        self.page
            .goto("https://www.douyin.com/user/self")
            .await
            .map_err(|_| Error::Browser)?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(if qr.is_some() { 180 } else { 45 });
        let mut identity = None;
        let mut ms_token = String::new();
        let mut login_cookie = self.login_cookie().await?;
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(1), responses.next()).await {
                let url = &event.response.url;
                if let Ok(parsed) = reqwest::Url::parse(url)
                    && parsed.scheme() == "https"
                    && parsed.host_str() == Some("www.douyin.com")
                    && parsed.port_or_known_default() == Some(443)
                {
                    for (name, value) in parsed.query_pairs() {
                        if name == "msToken" && value.len() >= 20 && value.len() <= 1024 && value.is_ascii() {
                            ms_token = value.into_owned();
                        }
                    }
                    if parsed.path().trim_end_matches('/') == "/aweme/v1/web/user/profile/self" {
                        if event.response.status == 401 && qr.is_none() {
                            return Err(Error::LoginExpired);
                        }
                        if matches!(event.response.status, 403 | 406 | 429 | 461) {
                            return Err(Error::Rejected);
                        }
                        for _ in 0..5 {
                            if let Ok(body) = self
                                .page
                                .execute(GetResponseBodyParams::new(event.request_id.clone()))
                                .await
                            {
                                let bytes = if body.result.base64_encoded {
                                    base64::engine::general_purpose::STANDARD
                                        .decode(&body.result.body)
                                        .map_err(|_| Error::InvalidData)?
                                } else {
                                    body.result.body.into_bytes()
                                };
                                if bytes.len() > 2 * 1024 * 1024 {
                                    return Err(Error::InvalidData);
                                }
                                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                                    identity = identity_from_payload(url, &value);
                                }
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
            }
            if qr.is_some() {
                let cookie = self.login_cookie().await?;
                if cookie.is_some() && cookie != login_cookie {
                    login_cookie = cookie;
                    identity = None;
                    ms_token.clear();
                    responses = self
                        .page
                        .event_listener::<EventResponseReceived>()
                        .await
                        .map_err(|_| Error::Browser)?;
                    self.page
                        .goto("https://www.douyin.com/user/self")
                        .await
                        .map_err(|_| Error::Browser)?;
                    continue;
                }
            }
            if let Some(identity) = &identity {
                if expected_identity.is_some_and(|expected| identity != expected) {
                    return Err(Error::AccountChanged);
                }
                if !ms_token.is_empty() {
                    let ua = self
                        .page
                        .evaluate("navigator.userAgent")
                        .await
                        .map_err(|_| Error::Browser)?
                        .into_value::<String>()
                        .map_err(|_| Error::Browser)?;
                    let fingerprint = self.page.evaluate("[innerWidth,innerHeight,outerWidth,outerHeight,screenX,screenY,0,0,screen.width,screen.height,screen.availWidth,screen.availHeight,innerWidth,innerHeight,screen.colorDepth,screen.pixelDepth,navigator.platform].join('|')")
                        .await.map_err(|_| Error::Browser)?.into_value::<String>().map_err(|_| Error::Browser)?;
                    if !ua.is_ascii() || ua.len() > 1024 || !fingerprint.is_ascii() || fingerprint.len() > 512 {
                        return Err(Error::InvalidData);
                    }
                    let cookies = self.browser.get_cookies().await.map_err(|_| Error::Browser)?;
                    return Ok(Auth {
                        identity: identity.clone(),
                        ua,
                        cookies: cookies
                            .into_iter()
                            .map(|c| serde_json::to_value(c).map_err(|_| Error::InvalidData))
                            .collect::<Result<_>>()?,
                        origin: "https://www.douyin.com".into(),
                        ms_token,
                        fingerprint,
                    });
                }
            }
            let challenge = self
                .page
                .evaluate("!!document.body && document.body.innerText.includes('请完成验证')")
                .await
                .map_err(|_| Error::Browser)?
                .into_value::<bool>()
                .map_err(|_| Error::Browser)?;
            if challenge {
                return Err(Error::Challenge);
            }
            if let Some(qr) = qr
                && let Some(image) = self.login_image().await?
            {
                qr.send_if_modified(|previous| {
                    if previous.as_ref() == Some(&image) {
                        return false;
                    }
                    *previous = Some(image);
                    true
                });
            }
        }
        Err(if identity.is_some() {
            Error::TokenUnavailable
        } else if qr.is_some() {
            Error::LoginExpired
        } else {
            Error::IdentityUnverified
        })
    }

    async fn login_cookie(&self) -> Result<Option<String>> {
        Ok(self
            .browser
            .get_cookies()
            .await
            .map_err(|_| Error::Browser)?
            .into_iter()
            .find(|c| {
                c.name == "sessionid"
                    && !c.value.is_empty()
                    && matches!(c.domain.trim_start_matches('.'), "douyin.com" | "www.douyin.com")
            })
            .map(|c| c.value))
    }

    pub async fn close(mut self) -> Result<()> {
        let closed = tokio::time::timeout(Duration::from_secs(10), self.browser.close()).await;
        if !matches!(closed, Ok(Ok(_))) {
            let _ = self.browser.kill().await;
        }
        if tokio::time::timeout(Duration::from_secs(5), self.browser.wait())
            .await
            .is_err()
        {
            let _ = self.browser.kill().await;
        }
        self.handler.abort();
        if matches!(closed, Ok(Ok(_))) {
            Ok(())
        } else {
            Err(Error::Browser)
        }
    }
}

const LOGIN_QR: &str = r#"(() => {
    const text = document.body?.innerText || '';
    if (text.includes('请完成验证') || text.includes('请完成下列验证')) return 'challenge';
    if (text.includes('二维码已失效') || text.includes('二维码已过期')) return 'expired';
    const visible = e => { const r=e.getBoundingClientRect(); return r.width>0 && r.height>0; };
    const qrContext = e => {
        if (e.matches('img[alt*="二维码"],img[src*="qrcode"]') || e.closest('[class*="qr"],[class*="QRCode"]')) return true;
        let parent=e.parentElement;
        for(let i=0;parent && i<4;i++,parent=parent.parentElement) {
            const r=parent.getBoundingClientRect(), text=parent.innerText || '';
            if(r.width<=600 && r.height<=800 && text.length<=300 && /扫码|二维码/.test(text)) return true;
        }
        return false;
    };
    const qr = [...document.querySelectorAll('img,canvas')].find(e => { const r=e.getBoundingClientRect(); return visible(e) && r.width>=100 && r.width<=512 && r.height>=100 && r.height<=512 && r.width/r.height>0.8 && r.width/r.height<1.2 && qrContext(e); });
    if (qr) { document.querySelectorAll('[data-favkeep-login-qr]').forEach(e => e.removeAttribute('data-favkeep-login-qr')); qr.setAttribute('data-favkeep-login-qr',''); return 'qr'; }
    const buttons = [...document.querySelectorAll('button,a,[role="button"],div,span')].filter(e => visible(e) && e.childElementCount===0 && !e.hasAttribute('data-favkeep-login-clicked'));
    const login = buttons.find(e => e.textContent.trim()==='扫码登录') || buttons.find(e => ['登录','登录/注册'].includes(e.textContent.trim()));
    if (login) { login.setAttribute('data-favkeep-login-clicked',''); login.click(); }
    return 'waiting';
})()"#;

impl Drop for Session {
    fn drop(&mut self) {
        self.handler.abort();
        // Browser owns its child and kills it on drop, including cancelled restore operations.
    }
}

pub fn identity_from_payload(url: &str, body: &Value) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.douyin.com")
        || url.port_or_known_default() != Some(443)
        || url.path().trim_end_matches('/') != "/aweme/v1/web/user/profile/self"
        || body["status_code"].as_i64() != Some(0)
    {
        return None;
    }
    let id = body["user"]["sec_uid"].as_str()?;
    (valid_id(id) && id != "0").then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn private_storage_restricts_directory_and_export() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("private");
        private_dir(&root).unwrap();
        write_private(&root.join("auth.json"), &json!({"synthetic":"fixture"})).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(
                std::fs::metadata(root.join("auth.json")).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        {
            let result = std::process::Command::new("icacls").arg(&root).output().unwrap();
            let text = String::from_utf8_lossy(&result.stdout);
            assert!(result.status.success());
            assert!(
                !text.contains("(I)"),
                "private directory must not inherit parent access"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn existing_explicit_foreign_access_is_rejected_before_writing_secrets() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("private");
        private_dir(&root).unwrap();
        let result = std::process::Command::new("icacls")
            .arg(&root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)R"])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(private_dir(&root), Err(Error::File));
        assert!(!root.join("auth.json").exists());
    }

    #[tokio::test]
    #[ignore = "requires FAVKEEP_TEST_BROWSER; uses only about:blank and synthetic cookies"]
    async fn explicit_import_replaces_stale_cookies_without_contacting_platform() {
        let executable = std::env::var_os("FAVKEEP_TEST_BROWSER").expect("set the system Chromium path");
        let root = tempfile::tempdir().unwrap();
        let session = Session::launch(root.path(), Path::new(&executable)).await.unwrap();
        let mut auth = Auth {
            identity: "fixture".into(),
            ua: "fixture".into(),
            origin: "https://www.douyin.com".into(),
            ms_token: String::new(),
            fingerprint: String::new(),
            cookies: vec![
                json!({"domain":".douyin.com","path":"/","name":"sessionid","value":"stale-fixture"}),
                json!({"domain":".douyin.com","path":"/","name":"UIFID","value":"old-fixture"}),
            ],
        };
        session.seed(&auth, true).await.unwrap();
        auth.cookies = vec![json!({"domain":".douyin.com","path":"/","name":"sessionid","value":"new-fixture"})];
        assert!(session.seed(&auth, false).await.unwrap().is_none());
        let backup = session
            .seed(&auth, true)
            .await
            .unwrap()
            .expect("explicit import must replace existing cookies");
        let cookies = session.browser.get_cookies().await.unwrap();
        assert!(
            cookies
                .iter()
                .any(|c| c.name == "sessionid" && c.value == "new-fixture")
        );
        assert!(!cookies.iter().any(|c| c.name == "UIFID"));
        session.clear_cookies().await.unwrap();
        session.browser.set_cookies(backup).await.unwrap();
        assert!(
            session
                .browser
                .get_cookies()
                .await
                .unwrap()
                .iter()
                .any(|c| c.name == "sessionid" && c.value == "stale-fixture")
        );
        session.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires FAVKEEP_TEST_BROWSER; uses only synthetic about:blank DOM"]
    async fn qr_capture_opens_login_and_stops_at_challenge_or_expiry() {
        let executable = std::env::var_os("FAVKEEP_TEST_BROWSER").expect("set the system Chromium path");
        let root = tempfile::tempdir().unwrap();
        let session = Session::launch(root.path(), Path::new(&executable)).await.unwrap();
        session.page.set_content(r#"<button onclick="document.querySelector('.qrcode').style.display='block'">登录</button><div class="qrcode" style="display:none"><canvas width="160" height="160"></canvas></div>"#).await.unwrap();
        assert!(session.login_image().await.unwrap().is_none());
        let image = session
            .login_image()
            .await
            .unwrap()
            .expect("visible QR canvas must be captured");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        session
            .page
            .set_content("<canvas class='hashed-logo' width='160' height='160'></canvas>")
            .await
            .unwrap();
        assert!(session.login_image().await.unwrap().is_none());
        session.page.set_content("<div style='width:300px'><p>请使用抖音扫码登录</p><canvas class='hashed-platform-class' width='178' height='178'></canvas></div>").await.unwrap();
        assert!(session.login_image().await.unwrap().is_some());
        session.page.set_content("<p>请完成验证</p>").await.unwrap();
        assert_eq!(session.login_image().await, Err(Error::Challenge));
        session.page.set_content("<p>二维码已过期</p>").await.unwrap();
        assert_eq!(session.login_image().await, Err(Error::LoginExpired));
        session.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires FAVKEEP_TEST_BROWSER and internet; fresh profile, no scan or saved auth"]
    async fn live_login_qr_is_captured_from_official_page() {
        let executable = std::env::var_os("FAVKEEP_TEST_BROWSER").expect("set the system Chromium path");
        let root = tempfile::tempdir().unwrap();
        let session = Session::launch(root.path(), Path::new(&executable)).await.unwrap();
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        let result = {
            let login = session.login(None, &tx);
            tokio::pin!(login);
            tokio::select! {
                result = &mut login => result.map(|_| ()),
                result = tokio::time::timeout(Duration::from_secs(90), rx.changed()) => {
                    result.map_err(|_| Error::Network).and_then(|r| r.map_err(|_| Error::Browser))
                }
            }
        };
        if result.is_err() {
            let shape = session.page.evaluate(r#"JSON.stringify({title:document.title,ready:document.readyState,elements:[...document.querySelectorAll('img,canvas,button,[role="button"]')].filter(e=>e.getBoundingClientRect().width>0).slice(0,50).map(e=>({tag:e.tagName,cls:String(e.className),width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height,text:e.tagName==='BUTTON'?e.textContent.trim().slice(0,30):null}))})"#).await.unwrap().into_value::<String>().unwrap();
            eprintln!("fresh-profile DOM shape without URLs or credentials: {shape}");
        }
        session.close().await.unwrap();
        assert_eq!(result, Ok(()));
        assert!(
            rx.borrow()
                .as_deref()
                .is_some_and(|image| image.starts_with("data:image/png;base64,"))
        );
        assert!(!root.path().join("auth.json").exists());
    }

    #[test]
    fn only_official_self_response_proves_identity() {
        let body = json!({"status_code":0,"user":{"sec_uid":"self_123"}});
        assert_eq!(
            identity_from_payload("https://www.douyin.com/aweme/v1/web/user/profile/self/?x=1", &body).as_deref(),
            Some("self_123")
        );
        assert!(
            identity_from_payload(
                "https://www.douyin.com.evil.test/aweme/v1/web/user/profile/self/",
                &body
            )
            .is_none()
        );
        assert!(identity_from_payload("http://www.douyin.com/aweme/v1/web/user/profile/self/", &body).is_none());
        assert!(identity_from_payload("https://www.douyin.com:444/aweme/v1/web/user/profile/self/", &body).is_none());
        assert!(
            identity_from_payload(
                "https://www.douyin.com/aweme/v1/web/user/profile/self/",
                &json!({"status_code":1,"user":{"sec_uid":"self_123"}})
            )
            .is_none()
        );
    }
}

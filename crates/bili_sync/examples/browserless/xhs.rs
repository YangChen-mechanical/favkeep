use super::{Platform, UA, request, write, xhs_sign};
use anyhow::{Context, Result, ensure};
use reqwest::{
    Client, Method, Url,
    cookie::{CookieStore, Jar},
    header,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const ORIGIN: &str = "https://www.rednote.com";
const API: &str = "https://webapi.rednote.com";

#[derive(Serialize, Deserialize)]
pub struct Auth {
    pub identity: String,
    pub origin: String,
    pub ua: String,
    pub cookies: Vec<Value>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn cookie_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b',')
}

fn identity(value: &Value) -> Result<String> {
    ensure!(
        value["success"] == true && value["code"] == 0 && value["data"]["guest"] == false,
        "self_identity_unverified"
    );
    let id = value["data"]["user_id"]
        .as_str()
        .or_else(|| value["data"]["id"].as_str())
        .context("self_identity_missing")?;
    ensure!(valid_id(id) && id != "0", "invalid_self_identity");
    Ok(id.into())
}

pub struct Session {
    client: Client,
    pub(super) jar: Arc<Jar>,
    a1: String,
    pub(super) root: PathBuf,
    page_load: u64,
    sequence: u32,
}

impl Session {
    pub fn new(client: Client, jar: Arc<Jar>, a1: String, root: PathBuf) -> Self {
        Self {
            client,
            jar,
            a1,
            root,
            page_load: chrono::Utc::now().timestamp_millis() as u64 - 20_000,
            sequence: 17,
        }
    }

    pub fn guest(&self) {
        let url = Url::parse(ORIGIN).unwrap();
        for cookie in [
            format!("a1={}; Domain=.rednote.com; Path=/; Secure", self.a1),
            format!(
                "webId={:x}; Domain=.rednote.com; Path=/; Secure",
                md5::compute(self.a1.as_bytes())
            ),
            "webBuild=4.86.0; Domain=.rednote.com; Path=/; Secure".into(),
            "xsecappid=xhs-pc-web; Domain=.rednote.com; Path=/; Secure".into(),
        ] {
            self.jar.add_cookie_str(&cookie, &url);
        }
    }

    pub async fn restore(client: Client, jar: Arc<Jar>, root: PathBuf, source: &Path) -> Result<Self> {
        let marker: Value = serde_json::from_slice(&std::fs::read(
            source.parent().context("auth parent missing")?.join("result.json"),
        )?)?;
        ensure!(
            marker["browser_used"] == false && marker["identity_verified"] == true,
            "only verified browserless probe credentials may be restored"
        );
        let bytes = std::fs::read(source)?;
        ensure!(bytes.len() <= 1024 * 1024, "auth file too large");
        let auth: Auth = serde_json::from_slice(&bytes)?;
        ensure!(
            auth.origin == ORIGIN && auth.ua == UA && valid_id(&auth.identity),
            "invalid saved auth"
        );
        for c in &auth.cookies {
            let host = c["domain"].as_str().context("cookie domain missing")?;
            ensure!(
                ["www.rednote.com", "webapi.rednote.com"].contains(&host),
                "untrusted cookie domain"
            );
            let name = c["name"].as_str().context("cookie name missing")?;
            let value = c["value"].as_str().context("cookie value missing")?;
            ensure!(
                !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    && cookie_value(value),
                "invalid saved cookie"
            );
            jar.add_cookie_str(
                &format!("{name}={value}; Domain={host}; Path=/; Secure"),
                &Url::parse(&format!("https://{host}"))?,
            );
        }
        let a1 = auth
            .cookies
            .iter()
            .find(|c| c["name"] == "a1")
            .and_then(|c| c["value"].as_str())
            .context("a1 missing")?
            .to_owned();
        let mut session = Self::new(client, jar, a1, root);
        let current = session.api("/api/sns/web/v2/user/me", None, "self-restored").await?;
        ensure!(identity(&current)? == auth.identity, "restored account differs");
        let verified = session.export(auth.identity)?;
        write(&session.root.join("auth.json"), &serde_json::to_vec_pretty(&verified)?)?;
        println!(
            "{}",
            json!({"stage":"restored","same_identity":true,"browser_used":false})
        );
        Ok(session)
    }

    pub async fn scan(&mut self) -> Result<()> {
        let auth: Auth = serde_json::from_slice(&std::fs::read(self.root.join("auth.json"))?)?;
        let mut cursor = String::new();
        let mut cursors = HashSet::new();
        let mut ids = HashSet::new();
        let mut items = Vec::new();
        for page in 0..20 {
            let mut url = Url::parse(&format!("{API}/api/sns/web/v2/note/collect/page"))?;
            url.query_pairs_mut()
                .append_pair("num", "30")
                .append_pair("cursor", &cursor)
                .append_pair("user_id", &auth.identity)
                .append_pair("image_formats", "jpg,webp,avif");
            let value = self
                .api(&url.as_str()[API.len()..], None, &format!("collection-{page:02}"))
                .await?;
            let data = &value["data"];
            let more = data["has_more"].as_bool().context("pagination terminal flag missing")?;
            let next = data["cursor"].as_str().context("pagination cursor missing")?;
            let notes = data["notes"].as_array().context("notes missing")?;
            ensure!(
                !more || (!notes.is_empty() && !next.is_empty() && next != cursor && cursors.insert(next.to_owned())),
                "incomplete pagination"
            );
            let before = items.len();
            let mut page_ids = HashSet::new();
            for note in notes {
                let id = note["note_id"]
                    .as_str()
                    .or_else(|| note["id"].as_str())
                    .context("note identity missing")?;
                ensure!(
                    valid_id(id) && page_ids.insert(id.to_owned()),
                    "invalid or duplicate note identity"
                );
                if !ids.insert(id.to_owned()) {
                    continue;
                }
                let card = note.get("note_card").or_else(|| note.get("noteCard")).unwrap_or(note);
                let kind = card["type"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("note type missing")?;
                let token = note["xsec_token"]
                    .as_str()
                    .or_else(|| note["xsecToken"].as_str())
                    .unwrap_or("");
                ensure!(
                    token.len() <= 4096 && token.is_ascii() && (kind != "video" || !token.is_empty()),
                    "video access token missing"
                );
                items.push(json!({"id":id,"kind":if kind=="video"{"video"}else{"unsupported"},"token":token}));
            }
            ensure!(notes.is_empty() || items.len() > before, "pagination made no progress");
            if items.len() >= 50 || !more {
                items.truncate(50);
                let videos = items.iter().filter(|n| n["kind"] == "video").count();
                write(
                    &self.root.join("window-private.json"),
                    &serde_json::to_vec_pretty(
                        &json!({"identity":auth.identity,"items":items,"window_complete":true,"full_collection_complete":!more}),
                    )?,
                )?;
                println!(
                    "{}",
                    json!({"stage":"window_read","unique_items":items.len(),"videos":videos,"unsupported":items.len()-videos,"window_complete":true,"browser_used":false})
                );
                return Ok(());
            }
            cursor = next.into();
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        anyhow::bail!("window scan limit reached")
    }

    pub async fn api(&mut self, path: &str, body: Option<&str>, stage: &str) -> Result<Value> {
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        self.sequence += 1;
        let xs = xhs_sign::sign(
            path,
            body,
            &self.a1,
            &xhs_sign::Inputs {
                timestamp,
                page_load: self.page_load,
                seed: rand::random(),
                sequence: self.sequence,
                window: 1500 + self.sequence,
            },
        )?;
        let common = xhs_sign::common(&self.a1, timestamp, 7)?;
        let mut req = self
            .client
            .request(
                if body.is_some() { Method::POST } else { Method::GET },
                format!("{API}{path}"),
            )
            .header(header::ORIGIN, ORIGIN)
            .header(header::REFERER, format!("{ORIGIN}/"))
            .header(header::CONTENT_TYPE, "application/json;charset=UTF-8")
            .header("x-s", xs)
            .header("x-s-common", common)
            .header("x-t", timestamp.to_string())
            .header("x-b3-traceid", format!("{:016x}", rand::random::<u64>()))
            .header("x-mns", "unload");
        if let Some(body) = body {
            req = req.body(body.to_owned());
        }
        request(&self.client, Platform::Xiaohongshu, req, stage, &self.root).await
    }

    fn export(&self, identity: String) -> Result<Auth> {
        let mut cookies = Vec::new();
        for origin in [ORIGIN, API] {
            let url = Url::parse(origin)?;
            if let Some(header) = self.jar.cookies(&url) {
                for part in header.to_str()?.split(';') {
                    let (name, value) = part.trim().split_once('=').context("invalid_cookie_header")?;
                    cookies.push(json!({"name":name,"value":value,"domain":url.host_str().unwrap(),"path":"/","secure":true,"httpOnly":true}));
                }
            }
        }
        ensure!(
            cookies.iter().any(|c| c["name"] == "web_session"),
            "authenticated_cookie_missing"
        );
        Ok(Auth {
            identity,
            origin: ORIGIN.into(),
            ua: UA.into(),
            cookies,
        })
    }

    pub async fn login(&mut self, qr: &Value) -> Result<()> {
        let qr_id = qr["qr_id"].as_str().context("QR ID missing")?;
        let code = qr["code"].as_str().context("QR code missing")?;
        let mut url = Url::parse(&format!("{API}/api/sns/web/v1/login/qrcode/status"))?;
        url.query_pairs_mut()
            .append_pair("qr_id", qr_id)
            .append_pair("code", code);
        let path = &url.as_str()[API.len()..];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
        let mut attempt = 0;
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let result = self.api(path, None, &format!("poll-{attempt:03}")).await?;
            attempt += 1;
            match result["data"]["code_status"].as_i64().context("QR status missing")? {
                0 | 1 => {}
                2 => {
                    let info = &result["data"]["login_info"];
                    if let Some(session) = info["session"].as_str().or_else(|| info["web_session"].as_str()) {
                        ensure!(cookie_value(session), "invalid_login_cookie");
                        self.jar.add_cookie_str(
                            &format!("web_session={session}; Domain=.rednote.com; Path=/; Secure; HttpOnly"),
                            &Url::parse(ORIGIN)?,
                        );
                    }
                    let candidate = self.export(String::new())?;
                    write(
                        &self.root.join("candidate-private.json"),
                        &serde_json::to_vec_pretty(&candidate)?,
                    )?;
                    let me = self.api("/api/sns/web/v2/user/me", None, "self").await?;
                    let id = identity(&me)?;
                    if let Some(qr_user) = info["user_id"].as_str() {
                        ensure!(qr_user == id, "QR and self identities differ");
                    }
                    let auth = self.export(id)?;
                    write(&self.root.join("auth.json"), &serde_json::to_vec_pretty(&auth)?)?;
                    println!(
                        "{}",
                        json!({"stage":"identity_verified","credentials_saved":true,"browser_used":false})
                    );
                    return Ok(());
                }
                _ => anyhow::bail!("QR expired or refused"),
            }
        }
        anyhow::bail!("QR confirmation timed out")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_self_identity_and_safe_cookies_are_accepted() {
        assert_eq!(
            identity(&json!({"success":true,"code":0,"data":{"guest":false,"user_id":"fixture_123"}})).unwrap(),
            "fixture_123"
        );
        for v in [
            json!({"success":true,"code":0,"data":{"guest":true,"user_id":"fixture"}}),
            json!({"success":false,"code":-1,"data":{"guest":false,"user_id":"fixture"}}),
            json!({"success":true,"code":0,"data":{"user_id":"fixture"}}),
        ] {
            assert!(identity(&v).is_err());
        }
        assert!(!cookie_value("abc; injected=value"));
        assert!(!cookie_value("abc\r\n"));
        assert!(cookie_value("fixture_123="));
    }
}

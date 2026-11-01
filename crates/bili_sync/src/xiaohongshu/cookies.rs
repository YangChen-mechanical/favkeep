use super::{Error, Result};
use cookie::{Cookie, SameSite, time::OffsetDateTime};
use cookie_store::{CookieDomain, CookieExpiration, CookieStore};
use parking_lot::Mutex;
use reqwest::{Url, cookie::CookieStore as HttpCookies, header::HeaderValue};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Jar(Mutex<CookieStore>);

fn allowed_domain(domain: &str) -> bool {
    [
        "rednote.com",
        "www.rednote.com",
        "webapi.rednote.com",
        "xiaohongshu.com",
        "www.xiaohongshu.com",
        "edith.xiaohongshu.com",
    ]
    .contains(&domain.trim_start_matches('.'))
}

fn valid_value(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        && value.len() <= 8192
        && value.bytes().all(|b| b.is_ascii_graphic() && b != b';')
}

impl Jar {
    pub fn load(&self, values: &[Value]) -> Result<()> {
        if values.len() > 512 {
            return Err(Error::InvalidData);
        }
        let mut store = CookieStore::default();
        for value in values {
            let domain = value["domain"].as_str().ok_or(Error::InvalidData)?;
            if !allowed_domain(domain) {
                return Err(Error::InvalidData);
            }
            let name = value["name"].as_str().ok_or(Error::InvalidData)?;
            let text = value["value"].as_str().ok_or(Error::InvalidData)?;
            let path = value["path"].as_str().unwrap_or("/");
            if !valid_value(name, text)
                || !path.starts_with('/')
                || path.bytes().any(|b| b <= 32 || b == b';' || b >= 127)
            {
                return Err(Error::InvalidData);
            }
            let mut cookie = Cookie::build((name.to_owned(), text.to_owned()))
                .path(path.to_owned())
                .secure(value["secure"].as_bool().unwrap_or(false))
                .http_only(value["httpOnly"].as_bool().unwrap_or(false));
            if domain.starts_with('.') {
                cookie = cookie.domain(domain.to_owned());
            }
            if let Some(expires) = value.get("expires") {
                let seconds = expires.as_f64().filter(|n| n.is_finite()).ok_or(Error::InvalidData)?;
                if seconds > 0.0 {
                    if seconds <= chrono::Utc::now().timestamp() as f64 {
                        continue;
                    }
                    cookie = cookie
                        .expires(OffsetDateTime::from_unix_timestamp(seconds as i64).map_err(|_| Error::InvalidData)?);
                }
            }
            if let Some(site) = value["sameSite"].as_str() {
                cookie = cookie.same_site(match site {
                    "Strict" => SameSite::Strict,
                    "Lax" => SameSite::Lax,
                    "None" => SameSite::None,
                    _ => return Err(Error::InvalidData),
                });
            }
            let url =
                Url::parse(&format!("https://{}", domain.trim_start_matches('.'))).map_err(|_| Error::InvalidData)?;
            store
                .insert_raw(&cookie.build(), &url)
                .map_err(|_| Error::InvalidData)?;
        }
        *self.0.lock() = store;
        Ok(())
    }

    pub fn set(&self, text: &str, url: &Url) -> Result<()> {
        let raw = Cookie::parse(text.to_owned()).map_err(|_| Error::InvalidData)?;
        if !allowed_domain(url.host_str().unwrap_or(""))
            || !valid_value(raw.name(), raw.value())
            || raw.domain().is_some_and(|d| !allowed_domain(d))
        {
            return Err(Error::InvalidData);
        }
        self.0.lock().insert_raw(&raw, url).map_err(|_| Error::InvalidData)?;
        Ok(())
    }

    pub fn value(&self, name: &str, url: &Url) -> Result<Option<String>> {
        let store = self.0.lock();
        let mut matches = store.matches(url).into_iter().filter(|c| c.name() == name);
        let Some(first) = matches.next() else {
            return Ok(None);
        };
        if matches.any(|c| c.value() != first.value()) {
            return Err(Error::InvalidData);
        }
        Ok(Some(first.value().to_owned()))
    }

    pub fn snapshot(&self) -> Vec<Value> {
        let store = self.0.lock();
        let mut values: Vec<_> = store
            .iter_unexpired()
            .map(|c| {
                let domain = match &c.domain {
                    CookieDomain::HostOnly(d) => d.clone(),
                    CookieDomain::Suffix(d) => format!(".{d}"),
                    _ => unreachable!("stored cookies have resolved domains"),
                };
                let expires = match c.expires {
                    CookieExpiration::AtUtc(t) => t.unix_timestamp(),
                    CookieExpiration::SessionEnd => -1,
                };
                let mut value = json!({"name":c.name(),"value":c.value(),"domain":domain,"path":c.path.to_string(),
                "secure":c.secure().unwrap_or(false),"httpOnly":c.http_only().unwrap_or(false),"expires":expires});
                if let Some(site) = c.same_site() {
                    value["sameSite"] = json!(site.to_string());
                }
                value
            })
            .collect();
        values.sort_by_key(|c| format!("{}\0{}\0{}", c["domain"], c["path"], c["name"]));
        values
    }
}

impl HttpCookies for Jar {
    fn set_cookies(&self, values: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        for header in values {
            if let Ok(text) = header.to_str()
                && text.len() <= 8192
            {
                // Invalid response cookies are ignored as in a browser; verified identity remains mandatory.
                let _ = self.set(text, url);
            }
        }
    }
    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let store = self.0.lock();
        let mut cookies = store.matches(url);
        cookies.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let text = cookies
            .iter()
            .map(|c| format!("{}={}", c.name(), c.value()))
            .collect::<Vec<_>>()
            .join("; ");
        if text.is_empty() {
            return None;
        }
        let mut header = HeaderValue::from_str(&text).ok()?;
        header.set_sensitive(true);
        Some(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_expiration_and_refresh_survive_restart_without_cross_platform_leaks() {
        let www = Url::parse("https://www.rednote.com/").unwrap();
        let api = Url::parse("https://webapi.rednote.com/api/test").unwrap();
        let jar = Jar::default();
        jar.set(
            "web_session=old; Domain=.rednote.com; Path=/; Secure; HttpOnly; Max-Age=600; SameSite=Lax",
            &www,
        )
        .unwrap();
        jar.set("host_only=private; Path=/api; Secure", &api).unwrap();
        let restored = Jar::default();
        restored.load(&jar.snapshot()).unwrap();
        assert_eq!(restored.value("web_session", &api).unwrap().as_deref(), Some("old"));
        assert!(restored.value("host_only", &www).unwrap().is_none());
        assert!(
            restored
                .cookies(&Url::parse("http://webapi.rednote.com/api/test").unwrap())
                .is_none()
        );
        for target in [
            "https://www.douyin.com/",
            "https://sns-v14.rednotecdn.com/",
            "https://webapi.rednote.com.evil.test/",
        ] {
            assert!(restored.cookies(&Url::parse(target).unwrap()).is_none());
        }
        restored
            .set(
                "web_session=new; Domain=.rednote.com; Path=/; Secure; HttpOnly; Max-Age=600",
                &www,
            )
            .unwrap();
        assert_eq!(restored.value("web_session", &api).unwrap().as_deref(), Some("new"));
        restored
            .set(
                "web_session=deleted; Domain=.rednote.com; Path=/; Secure; Max-Age=0",
                &www,
            )
            .unwrap();
        assert!(restored.value("web_session", &api).unwrap().is_none());
        let mut invalid = jar.snapshot();
        invalid[0]["domain"] = json!(".com");
        assert!(Jar::default().load(&invalid).is_err());
        assert!(
            jar.set("web_session=foreign; Domain=.douyin.com; Path=/", &www)
                .is_err()
        );
    }
}

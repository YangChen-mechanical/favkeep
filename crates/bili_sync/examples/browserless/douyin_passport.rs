// SPDX-License-Identifier: MIT
// Protocol/profile reference: cv-cat/catbus bcc72f81549b94090b340fb87f29927529056e49; see catbus-LICENSE.
#[path = "douyin_download.rs"]
mod download;
use super::{Platform, capture, passport_sign as sign, write};
use anyhow::{Context, Result, ensure};
use reqwest::{
    Client, Url,
    cookie::{CookieStore, Jar},
    header,
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

const WWW: &str = "https://www.douyin.com";
const QR_PATH: &str = "/passport/web/get_qrcode/";

pub async fn restore(client: &Client, jar: &Jar, args: &super::Args, source: &Path) -> Result<()> {
    let marker: Value = serde_json::from_slice(&std::fs::read(
        source.parent().context("auth_parent_missing")?.join("result.json"),
    )?)?;
    ensure!(
        marker["platform"] == "douyin" && marker["browser_used"] == false && marker["identity_verified"] == true,
        "verified_probe_auth_required"
    );
    let bytes = std::fs::read(source)?;
    ensure!(bytes.len() <= 1024 * 1024, "auth_file_too_large");
    let auth: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        auth["origin"] == WWW && auth["ua"] == sign::UA && auth["identity_verified"] == true,
        "invalid_saved_auth"
    );
    let expected = auth["identity"].as_str().context("saved_identity_missing")?;
    ensure!(valid_id(expected), "invalid_saved_identity");
    for c in auth["cookies"].as_array().context("saved_cookies_missing")? {
        ensure!(
            c["domain"] == "www.douyin.com" && c["path"] == "/" && c["secure"] == true,
            "untrusted_saved_cookie_scope"
        );
        let name = c["name"].as_str().context("saved_cookie_name_missing")?;
        let value = c["value"].as_str().context("saved_cookie_value_missing")?;
        ensure!(
            valid_id(name)
                && !value.is_empty()
                && value.len() <= 8192
                && value.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b','),
            "invalid_saved_cookie"
        );
        jar.add_cookie_str(&format!("{name}={value}; Path=/; Secure"), &Url::parse(WWW)?);
    }
    let token = mint_token(client, &args.output_dir).await?;
    let mut session = Passport {
        client,
        jar,
        root: &args.output_dir,
        base: vec![("msToken", token)],
        trace: String::new(),
        portrait: String::new(),
        counter: 2,
    };
    session.verify_self(Some(expected)).await?;
    println!(
        "{}",
        json!({"stage":"restored","same_identity":true,"fresh_visitor_token":true,"browser_used":false})
    );
    if args.scan {
        session.scan(args).await?;
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn report(now: i64, uuid: &str, collect_time: f64, nonce: u8) -> Result<String> {
    // The vendored public device profile contains no account, Cookie, or minted token.
    let mut profile: Value = serde_json::from_str(include_str!("../../src/douyin/native/mstoken-profile.json"))?;
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

fn source_info(now: i64) -> Result<String> {
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

fn cookie(jar: &Jar, name: &str) -> Option<String> {
    jar.cookies(&Url::parse("https://login.douyin.com/").unwrap())?
        .to_str()
        .ok()?
        .split("; ")
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

fn site_cookie(jar: &Jar, name: &str) -> Option<String> {
    let header = jar.cookies(&Url::parse(WWW).ok()?)?;
    let mut values = header.to_str().ok()?.split("; ").filter_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    });
    let value = values.next()?;
    values.all(|other| other == value).then(|| value.to_owned())
}

fn valid_ac_nonce(value: &str) -> bool {
    (20..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn private_headers(root: &Path, stage: &str, headers: &header::HeaderMap) -> Result<()> {
    let selected: Vec<_> = headers
        .iter()
        .filter(|(key, _)| {
            [
                "set-cookie",
                "x-ms-token",
                "x-tt-logid",
                "bd-ticket-guard-server-data",
                "bd-ticket-guard-sec-ts",
            ]
            .contains(&key.as_str())
        })
        .map(|(key, value)| (key.as_str(), value.to_str().unwrap_or("non-text")))
        .collect();
    write(
        &root.join(format!("{stage}-headers-private.json")),
        &serde_json::to_vec_pretty(&selected)?,
    )
}

fn valid_token(value: &str) -> bool {
    (100..=512).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic() && b != b';' && b != b',')
}

async fn mint_token(client: &Client, root: &Path) -> Result<String> {
    let now = chrono::Utc::now().timestamp_millis();
    let token_response = capture(
        client,
        Platform::Douyin,
        client
            .post("https://mssdk.bytedance.com/web/r/token?ms_appid=6383")
            .header(header::USER_AGENT, sign::UA)
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
            .body(report(
                now,
                &uuid::Uuid::new_v4().to_string(),
                rand::random_range(40.0..160.0),
                rand::random(),
            )?),
        "mint-mstoken",
        root,
    )
    .await?;
    private_headers(root, "mint-mstoken", &token_response.headers)?;
    ensure!(
        token_response.status == 200
            && !["platform_rejected", "business_rejected", "verification_required"]
                .contains(&token_response.classification),
        "visitor_token_rejected"
    );
    let token = token_response
        .headers
        .get("x-ms-token")
        .and_then(|h| h.to_str().ok())
        .or_else(|| {
            token_response
                .headers
                .get_all(header::SET_COOKIE)
                .iter()
                .filter_map(|h| h.to_str().ok())
                .find_map(|h| h.strip_prefix("msToken=").and_then(|v| v.split(';').next()))
        })
        .filter(|v| valid_token(v))
        .context("genuine_visitor_token_missing")?;
    Ok(token.to_owned())
}

pub async fn qrcode(client: &Client, jar: &Jar, args: &super::Args) -> Result<()> {
    let root = &args.output_dir;
    let response = capture(client,Platform::Douyin,
        client.post("https://ttwid.bytedance.com/ttwid/union/register/")
            .header(header::USER_AGENT,sign::UA).header(header::CONTENT_TYPE,"application/json")
            .body(serde_json::to_string(&json!({"region":"cn","aid":1768,"needFid":false,
                "service":"www.douyin.com","migrate_info":{"ticket":"","source":"node"},"cbUrlProtocol":"https","union":true}))?),
        "register-ttwid",root).await?;
    private_headers(root, "register-ttwid", &response.headers)?;
    ensure!(
        response.classification == "json_received",
        "visitor_registration_rejected"
    );
    let ttwid = response
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find_map(|h| h.strip_prefix("ttwid=").and_then(|v| v.split(';').next()))
        .context("visitor_id_missing")?;
    ensure!(!ttwid.is_empty() && ttwid.len() <= 1024, "invalid_visitor_id");
    jar.add_cookie_str(
        &format!("ttwid={ttwid}; Domain=.douyin.com; Path=/; Secure"),
        &Url::parse(WWW)?,
    );
    let token = mint_token(client, root).await?;
    println!(
        "{}",
        json!({"stage":"visitor_tokens","ttwid_received":true,"genuine_ms_token_received":true,"ms_token_bytes":token.len()})
    );
    let now = chrono::Utc::now().timestamp_millis();
    let ts = sign::sdk_ts(now)?;
    let trace = hex::encode(rand::random::<[u8; 4]>());
    jar.add_cookie_str(
        &format!("biz_trace_id={trace}; Domain=.douyin.com; Path=/; Secure"),
        &Url::parse(WWW)?,
    );
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
        ("account_sdk_source_info", source_info(now)?),
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
    pairs.push(("msToken", token.to_owned()));
    let mut passport = Passport {
        client,
        jar,
        root,
        base: pairs,
        trace,
        portrait: format!("{}.login", uuid::Uuid::new_v4()),
        counter: 2,
    };
    if args.douyin_guest_bootstrap {
        passport.prepare_visitor(true).await?;
        println!(
            "{}",
            json!({"stage":"guest_visitor_initialized","official_cookie_received":true})
        );
    }
    let value = passport
        .call(
            QR_PATH,
            &[
                ("next", WWW.to_owned()),
                ("need_short_url", "true".to_owned()),
                ("need_logo", "false".to_owned()),
                ("is_new_login", "1".to_owned()),
                ("is_from_iesaccountsaas", "1".to_owned()),
            ],
            &[],
            "qrcode-passport",
        )
        .await?;
    ensure!(
        value["data"]["token"].as_str().is_some_and(|s| !s.is_empty())
            && ["qrcode", "qrcode_index_url"]
                .iter()
                .any(|k| value["data"][k].as_str().is_some_and(|s| !s.is_empty())),
        "QR_payload_missing"
    );
    let qr_url = Url::parse(value["data"]["qrcode_index_url"].as_str().context("QR_url_missing")?)?;
    ensure!(
        qr_url.scheme() == "https"
            && qr_url.host_str() == Some("api.amemv.com")
            && qr_url.port_or_known_default() == Some(443)
            && qr_url.username().is_empty()
            && qr_url.password().is_none()
            && qr_url.path() == "/ucenter_web/app/aweme/scan_login/index/douyin_scan_code_login/cn/app/index.html",
        "untrusted_QR_url"
    );
    write(
        &root.join("qr-private.json"),
        &serde_json::to_vec_pretty(&value["data"])?,
    )?;
    if args.login {
        passport.poll(&value["data"]).await?;
    }
    if args.scan {
        passport.scan(args).await?;
    }
    Ok(())
}

struct Passport<'a> {
    client: &'a Client,
    jar: &'a Jar,
    root: &'a Path,
    base: Vec<(&'static str, String)>,
    trace: String,
    portrait: String,
    counter: u32,
}

impl Passport<'_> {
    async fn pages(
        &mut self,
        path: &str,
        key: &str,
        extra: &[(&str, String)],
        total: Option<u64>,
        stage: &str,
    ) -> Result<Vec<Value>> {
        let mut scan = Scan::default();
        for page in 0..100 {
            let mut params = extra.to_vec();
            params.extend([("cursor", scan.cursor.to_string()), ("count", "20".to_owned())]);
            let value = self.main_get(path, &params, &format!("{stage}-{page:03}")).await?;
            if scan.push(&value, key, total == Some(0))? {
                ensure!(
                    [total, scan.total]
                        .into_iter()
                        .flatten()
                        .all(|n| n == scan.rows.len() as u64),
                    "pagination_total_mismatch"
                );
                return Ok(scan.rows);
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        anyhow::bail!("pagination_limit_incomplete")
    }

    async fn scan(&mut self, args: &super::Args) -> Result<()> {
        self.prepare_visitor(false).await?;
        let folders = self
            .pages("/aweme/v1/web/collects/list/", "collects_list", &[], None, "folders")
            .await?;
        write(
            &self.root.join("folders-private.json"),
            &serde_json::to_vec_pretty(&folders)?,
        )?;
        let selected: Vec<String> = serde_json::from_slice(&std::fs::read(
            args.folder_ids_file.as_ref().context("selected_folders_required")?,
        )?)?;
        ensure!(
            !selected.is_empty()
                && selected.len() == selected.iter().collect::<HashSet<_>>().len()
                && selected.iter().all(|id| valid_id(id)),
            "invalid_selected_folders"
        );
        let mut collections = Vec::new();
        for (index, id) in selected.iter().enumerate() {
            let folder = folders
                .iter()
                .find(|f| {
                    f["collects_id"].as_str() == Some(id)
                        || f["collects_id"].as_u64().is_some_and(|n| n.to_string() == *id)
                })
                .context("selected_folder_unavailable")?;
            let total = match folder.get("total_number") {
                Some(Value::Null) | None => None,
                Some(v) => Some(v.as_u64().context("invalid_folder_total")?),
            };
            let rows = self
                .pages(
                    "/aweme/v1/web/collects/video/list/",
                    "aweme_list",
                    &[("collects_id", id.clone())],
                    total,
                    &format!("collection-{index:02}"),
                )
                .await?;
            println!(
                "{}",
                json!({"stage":"collection_complete","folder_number":index+1,"items":rows.len(),"real_terminal":true})
            );
            collections.push(json!({"folder_id":id,"items":rows}));
        }
        write(
            &self.root.join("collections-private.json"),
            &serde_json::to_vec_pretty(&json!({"collection_complete":true,"folders":collections}))?,
        )?;
        if args.download_three {
            self.download_three(args, &collections).await?;
        }
        Ok(())
    }
    async fn call(
        &mut self,
        path: &str,
        extra: &[(&str, String)],
        body: &[(&str, String)],
        stage: &str,
    ) -> Result<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        let ts = sign::sdk_ts(now)?;
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
        let encoded_body = sign::query(body);
        self.counter += 1;
        let bogus = sign::abogus(&query, &encoded_body, self.counter, sign::Target::Login);
        let url = Url::parse(&format!(
            "https://login.douyin.com{path}?{query}&{}",
            sign::query(&[("a_bogus", bogus)])
        ))?;
        let builder = if body.is_empty() {
            self.client.get(url)
        } else {
            self.client
                .post(url)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(encoded_body)
        };
        let response = capture(
            self.client,
            Platform::Douyin,
            builder
                .header(header::USER_AGENT, sign::UA)
                .header("web-sdk-version", "1")
                .header("sec-ch-ua-platform", "\"Windows\"")
                .header("sec-ch-ua", sign::CH_UA)
                .header("sec-ch-ua-mobile", "?0")
                .header(header::REFERER, format!("{WWW}/"))
                .header(header::ORIGIN, WWW)
                .header("x-tt-passport-aid-sign", sign::aid_sign(path, &ts))
                .header(
                    "x-tt-passport-csrf-token",
                    cookie(self.jar, "passport_csrf_token")
                        .or_else(|| cookie(self.jar, "passport_csrf_token_default"))
                        .unwrap_or_default(),
                )
                .header("x-tt-passport-trace-id", &self.trace)
                .header("x-tt-passport-verify-portrait", &self.portrait)
                .header(header::ACCEPT, "application/json, text/javascript")
                .header(header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
                .header("priority", "u=1, i")
                .header("sec-fetch-dest", "empty")
                .header("sec-fetch-mode", "cors")
                .header("sec-fetch-site", "same-site"),
            stage,
            self.root,
        )
        .await?;
        private_headers(self.root, stage, &response.headers)?;
        ensure!(
            response.classification == "json_received",
            "{}",
            response.classification
        );
        response.value.context("missing_JSON_response")
    }

    async fn poll(&mut self, qr: &Value) -> Result<()> {
        let token = qr["token"].as_str().context("QR_token_missing")?;
        let fp = super::verify_fp();
        let extra = [
            ("is_from_iesaccountsaas", "1".to_owned()),
            ("p_ca", "4.0.26".to_owned()),
            ("p_ca_real", "1.0.0.938".to_owned()),
            ("fp", fp.clone()),
            ("verifyFp", fp),
        ];
        let body = [
            ("need_logo", "false".to_owned()),
            ("is_frontier", "true".to_owned()),
            ("token", token.to_owned()),
            ("is_new_login", "1".to_owned()),
            ("next", WWW.to_owned()),
            ("need_short_url", "true".to_owned()),
        ];
        println!(
            "{}",
            json!({"stage":"awaiting_phone_confirmation","timeout_seconds":180})
        );
        for n in 1..=36 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let value = self
                .call(
                    "/passport/web/check_qrconnect/",
                    &extra,
                    &body,
                    &format!("qr-poll-{n:02}"),
                )
                .await?;
            let state = qr_state(&value)?;
            println!("{}", json!({"stage":"qr_state","state":state}));
            if state == "confirmed" {
                let redirect = value["data"]["redirect_url"]
                    .as_str()
                    .context("login_callback_missing")?;
                self.finish(redirect).await?;
                return self.verify_self(None).await;
            }
        }
        anyhow::bail!("QR_login_timeout")
    }

    async fn finish(&self, redirect: &str) -> Result<()> {
        let mut url = Url::parse(redirect).map_err(|_| anyhow::anyhow!("invalid_login_callback"))?;
        for hop in 0..5 {
            ensure!(login_redirect(&url), "untrusted_login_callback");
            let response = capture(
                self.client,
                Platform::Douyin,
                self.client
                    .get(url.clone())
                    .header(header::USER_AGENT, sign::UA)
                    .header(header::REFERER, format!("{WWW}/")),
                &format!("callback-{hop}"),
                self.root,
            )
            .await?;
            private_headers(self.root, &format!("callback-{hop}"), &response.headers)?;
            if [301, 302, 303, 307, 308].contains(&response.status) {
                let location = response
                    .headers
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .context("callback_location_missing")?;
                url = url
                    .join(location)
                    .map_err(|_| anyhow::anyhow!("invalid_callback_location"))?;
            } else {
                ensure!(
                    response.status == 200
                        && !["platform_rejected", "business_rejected", "verification_required"]
                            .contains(&response.classification),
                    "login_callback_rejected"
                );
                return Ok(());
            }
        }
        anyhow::bail!("login_callback_redirect_limit")
    }

    async fn main_get(&mut self, path: &str, params: &[(&str, String)], stage: &str) -> Result<Value> {
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
        let ms = self
            .base
            .iter()
            .find(|(k, _)| *k == "msToken")
            .context("visitor_token_missing")?
            .1
            .clone();
        pairs.push(("msToken", ms));
        pairs.extend_from_slice(params);
        let query = sign::query(&pairs);
        self.counter += 1;
        let bogus = sign::abogus(&query, "", self.counter, sign::Target::Main);
        let query = format!("{query}&{}", sign::query(&[("a_bogus", bogus)]));
        let uifid = self.visitor_cookie();
        let query = uifid
            .as_ref()
            .map(|v| sign::web_sign(&query, v, chrono::Utc::now().timestamp()))
            .unwrap_or(query);
        let mut builder = self.client.get(format!("{WWW}{path}?{query}"));
        if let Some(value) = uifid {
            builder = builder.header("uifid", value);
        }
        let response = capture(
            self.client,
            Platform::Douyin,
            builder
                .header(header::USER_AGENT, sign::UA)
                .header(header::REFERER, format!("{WWW}/"))
                .header(header::ACCEPT, "application/json, text/plain, */*"),
            stage,
            self.root,
        )
        .await?;
        private_headers(self.root, stage, &response.headers)?;
        ensure!(
            response.classification == "json_received",
            "{}",
            response.classification
        );
        response.value.context("missing_JSON_response")
    }

    fn visitor_cookie(&self) -> Option<String> {
        ["UIFID", "UIFID_TEMP"].into_iter().find_map(|name| {
            site_cookie(self.jar, name)
                .filter(|v| (100..=512).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    }

    async fn prepare_visitor(&self, fresh_guest: bool) -> Result<()> {
        if self.visitor_cookie().is_some() {
            return Ok(());
        }
        let response = self.visitor_document("visitor-document").await?;
        if fresh_guest && response.classification == "javascript_initialization_required" {
            let nonce = site_cookie(self.jar, "__ac_nonce")
                .filter(|v| valid_ac_nonce(v))
                .context("official_ac_nonce_missing")?;
            let signature = super::ac_sign::signature(
                &nonce,
                &format!("{WWW}/jingxuan"),
                sign::UA,
                chrono::Utc::now().timestamp() as u64,
            );
            self.jar.add_cookie_str(
                &format!("__ac_signature={signature}; Max-Age=31536000; Path=/; Secure; SameSite=None"),
                &Url::parse(WWW)?,
            );
            self.jar.add_cookie_str(
                &format!("__ac_referer={WWW}/?recommend=1; Max-Age=31536000; Path=/; Secure; SameSite=None"),
                &Url::parse(WWW)?,
            );
            self.visitor_document("visitor-after-ac").await?;
        }
        ensure!(self.visitor_cookie().is_some(), "official_visitor_cookie_missing");
        Ok(())
    }

    async fn visitor_document(&self, stage: &str) -> Result<super::Captured> {
        let response = capture(
            self.client,
            Platform::Douyin,
            self.client
                .get(format!("{WWW}/jingxuan"))
                .header(header::USER_AGENT, sign::UA)
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
            stage,
            self.root,
        )
        .await?;
        private_headers(self.root, stage, &response.headers)?;
        ensure!(
            response.status == 200
                && !["platform_rejected", "business_rejected", "verification_required"]
                    .contains(&response.classification),
            "visitor_initialization_rejected"
        );
        Ok(response)
    }

    async fn verify_self(&mut self, expected: Option<&str>) -> Result<()> {
        let exported = cookie_snapshot(self.jar)?;
        ensure!(
            exported.iter().any(
                |c| ["sessionid", "sessionid_ss"].contains(&c["name"].as_str().unwrap_or(""))
                    && c["value"].as_str().is_some_and(|s| !s.is_empty())
            ),
            "authenticated_session_cookie_missing"
        );
        let mut auth = json!({"origin":WWW,"ua":sign::UA,"cookies":exported,"identity_verified":false});
        write(
            &self.root.join("candidate-private.json"),
            &serde_json::to_vec_pretty(&auth)?,
        )?;
        let value = self
            .main_get("/aweme/v1/web/user/profile/self/", &[], "official-self")
            .await?;
        let id = self_identity(&value)?;
        ensure!(expected.is_none_or(|e| e == id), "restored_account_differs");
        auth["cookies"] = json!(cookie_snapshot(self.jar)?);
        auth["identity"] = json!(id);
        auth["identity_verified"] = json!(true);
        auth["ms_token"] = json!(
            self.base
                .iter()
                .find(|(k, _)| *k == "msToken")
                .context("visitor_token_missing")?
                .1
        );
        write(&self.root.join("auth.json"), &serde_json::to_vec_pretty(&auth)?)?;
        println!(
            "{}",
            json!({"stage":"identity","official_self_verified":true,"auth_saved":true})
        );
        Ok(())
    }
}

fn cookie_snapshot(jar: &Jar) -> Result<Vec<Value>> {
    let cookies = jar.cookies(&Url::parse(WWW)?).context("login_cookies_missing")?;
    // shortcut: probe exports only www-scoped cookies; preserve original scopes before migrating application credentials.
    Ok(cookies.to_str()?.split("; ").filter_map(|s|s.split_once('=')).map(|(name,value)|
        json!({"name":name,"value":value,"domain":"www.douyin.com","path":"/","secure":true,"httpOnly":true})).collect())
}

fn qr_state(value: &Value) -> Result<&str> {
    ensure!(
        value["data"]["error_code"] == 0 && value["message"] == "success",
        "QR_poll_rejected"
    );
    match value["data"]["status"].as_str() {
        Some(state @ ("new" | "scanned" | "confirmed")) => Ok(state),
        Some("expired") => anyhow::bail!("QR_expired"),
        _ => anyhow::bail!("QR_state_unknown"),
    }
}

fn login_redirect(url: &Url) -> bool {
    super::trusted(Platform::Douyin, url)
        && ["www.douyin.com", "login.douyin.com", "sso.douyin.com"].contains(&url.host_str().unwrap_or(""))
        && (url.path() == "/" || url.path().starts_with("/passport/"))
}

fn self_identity(value: &Value) -> Result<&str> {
    ensure!(value["status_code"] == 0, "self_identity_unverified");
    value["user"]["sec_uid"]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && *id != "0"
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .context("self_identity_missing")
}

#[derive(Default)]
struct Scan {
    cursor: u64,
    known: HashSet<String>,
    rows: Vec<Value>,
    total: Option<u64>,
}

impl Scan {
    fn push(&mut self, value: &Value, key: &str, verified_empty: bool) -> Result<bool> {
        ensure!(value["status_code"] == 0, "pagination_rejected");
        let more = match &value["has_more"] {
            Value::Bool(b) => *b,
            v if v.as_u64() == Some(0) => false,
            v if v.as_u64() == Some(1) => true,
            _ => anyhow::bail!("terminal_signal_missing"),
        };
        let next = value["cursor"].as_u64().context("cursor_missing")?;
        let rows = match &value[key] {
            Value::Array(a) => a.as_slice(),
            Value::Null if verified_empty && !more => &[],
            _ => anyhow::bail!("page_rows_missing"),
        };
        ensure!(
            !more || (next > self.cursor && !rows.is_empty()),
            "cursor_stalled_or_empty_page"
        );
        if key == "collects_list"
            && let Some(total) = value.get("total_number")
        {
            let total = total.as_u64().context("invalid_pagination_total")?;
            ensure!(self.total.is_none_or(|n| n == total), "pagination_total_changed");
            self.total = Some(total);
        }
        let id_key = if key == "collects_list" {
            "collects_id"
        } else {
            "aweme_id"
        };
        let mut added = 0;
        let mut page_ids = HashSet::new();
        for row in rows {
            let id = row[id_key]
                .as_str()
                .map(str::to_owned)
                .or_else(|| row[id_key].as_u64().map(|n| n.to_string()))
                .context("item_id_missing")?;
            ensure!(
                valid_id(&id) && page_ids.insert(id.clone()),
                "invalid_or_duplicate_item_id"
            );
            if self.known.insert(id) {
                self.rows.push(row.clone());
                added += 1;
            }
        }
        ensure!(rows.is_empty() || added > 0, "repeated_page");
        self.cursor = next;
        Ok(!more)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_requires_terminal_progress_and_distinguishes_missing_rows() {
        let mut scan = Scan::default();
        assert!(
            !scan
                .push(
                    &json!({"status_code":0,"has_more":true,"cursor":20,"aweme_list":[{"aweme_id":"1"}]}),
                    "aweme_list",
                    false
                )
                .unwrap()
        );
        assert!(
            scan.push(
                &json!({"status_code":0,"has_more":true,"cursor":20,"aweme_list":[{"aweme_id":"2"}]}),
                "aweme_list",
                false
            )
            .is_err()
        );
        assert!(
            scan.push(
                &json!({"status_code":0,"has_more":true,"cursor":40,"aweme_list":[]}),
                "aweme_list",
                false
            )
            .is_err()
        );
        assert!(
            scan.push(
                &json!({"status_code":0,"has_more":false,"cursor":40,"aweme_list":[{"aweme_id":"1"}]}),
                "aweme_list",
                false
            )
            .is_err()
        );
        assert!(
            scan.push(
                &json!({"status_code":0,"has_more":false,"cursor":40,"aweme_list":[{"aweme_id":"2"}]}),
                "aweme_list",
                false
            )
            .unwrap()
        );
        assert_eq!(scan.rows.len(), 2);
        assert!(
            Scan::default()
                .push(
                    &json!({"status_code":0,"has_more":false,"cursor":0}),
                    "aweme_list",
                    false
                )
                .is_err()
        );
        assert!(
            Scan::default()
                .push(
                    &json!({"status_code":0,"has_more":false,"cursor":0}),
                    "aweme_list",
                    true
                )
                .unwrap()
        );
    }
    #[test]
    fn qr_identity_redirect_and_cookie_isolation() {
        assert_eq!(
            qr_state(&json!({"message":"success","data":{"error_code":0,"status":"scanned"}})).unwrap(),
            "scanned"
        );
        for state in ["expired", "cancelled", "unknown"] {
            assert!(qr_state(&json!({"message":"success","data":{"error_code":0,"status":state}})).is_err());
        }
        assert!(qr_state(&json!({"message":"error","data":{"error_code":7,"status":"new"}})).is_err());
        assert!(self_identity(&json!({"status_code":0,"user":{"sec_uid":"self_123"}})).is_ok());
        assert!(self_identity(&json!({"status_code":1,"user":{"sec_uid":"self_123"}})).is_err());
        assert!(self_identity(&json!({"status_code":0,"user":{"sec_uid":"0"}})).is_err());
        assert!(login_redirect(
            &Url::parse("https://www.douyin.com/passport/sso/login/callback/?ticket=synthetic").unwrap()
        ));
        for url in [
            "http://www.douyin.com/",
            "https://www.douyin.com.evil.test/",
            "https://mssdk.bytedance.com/",
            "https://login.douyin.com:444/",
            "https://user@login.douyin.com/",
            "https://www.douyin.com/user/self",
        ] {
            assert!(!login_redirect(&Url::parse(url).unwrap()));
        }
        let jar = Jar::default();
        jar.add_cookie_str(
            "sessionid=synthetic; Domain=.douyin.com; Path=/; Secure",
            &Url::parse(WWW).unwrap(),
        );
        assert!(cookie(&jar, "sessionid").is_some());
        assert!(
            jar.cookies(&Url::parse("https://mssdk.bytedance.com/").unwrap())
                .is_none()
        );
        assert!(jar.cookies(&Url::parse("https://www.rednote.com/").unwrap()).is_none());
    }
    #[test]
    fn visitor_payload_and_token_boundaries() {
        let nonce = "0123456789abcdef01234";
        assert!(valid_ac_nonce(nonce));
        for invalid in ["", "short", "0123456789abcdef0123;injected", "0123456789abcdef0123\r\n"] {
            assert!(!valid_ac_nonce(invalid));
        }
        let jar = Jar::default();
        jar.add_cookie_str(
            &format!("__ac_nonce={nonce}; Path=/; Secure"),
            &Url::parse(WWW).unwrap(),
        );
        assert_eq!(site_cookie(&jar, "__ac_nonce").as_deref(), Some(nonce));
        assert!(cookie(&jar, "__ac_nonce").is_none());
        jar.add_cookie_str(
            "__ac_nonce=abcdef0123456789abcde; Domain=.douyin.com; Path=/; Secure",
            &Url::parse(WWW).unwrap(),
        );
        assert!(site_cookie(&jar, "__ac_nonce").is_none());
        let payload: Value = serde_json::from_str(&report(1793448101123, "synthetic-uuid", 75.5, 91).unwrap()).unwrap();
        assert_eq!(payload["magic"], 538969122);
        assert_eq!(payload["tspFromClient"], 1793448101123i64);
        let bytes = {
            use base64::Engine;
            base64::engine::GeneralPurpose::new(
                &base64::alphabet::Alphabet::new("Dkdpgh4ZKsQB80/Mfvw36XI1R25+WUAlEi7NLboqYTOPuzmFjJnryx9HVGcaStCe")
                    .unwrap(),
                base64::engine::general_purpose::PAD,
            )
            .decode(payload["strData"].as_str().unwrap())
            .unwrap()
        };
        assert_eq!(&bytes[..2], &[0x41, 91]);
        let profile: Value = serde_json::from_slice(&super::super::codec::rc4(&bytes[2..], &[91])).unwrap();
        let custom: Value = serde_json::from_str(profile["nWID"]["custom"].as_str().unwrap()).unwrap();
        assert_eq!(custom["uuid"], "synthetic-uuid");
        assert_eq!(custom["collectTime"], 75.5);
        assert_eq!(profile["wID"]["msgType"], 3);
        assert_eq!(profile["nWID"]["ms_accid"], "");
        assert!(valid_token(&"A".repeat(184)));
        assert!(!valid_token(""));
        assert!(!valid_token(&format!("{};bad", "A".repeat(180))));
        assert_eq!(super::super::classification(403, b"", None), "platform_rejected");
    }
}

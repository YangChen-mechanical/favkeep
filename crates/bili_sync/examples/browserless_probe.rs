//! Browser-free protocol gate. Only requests a fresh QR; never opens an application database or imports a session.
#[path = "../src/douyin/sign.rs"]
mod douyin_sign;
#[path = "browserless/xhs.rs"]
mod xhs;
#[path = "browserless/xhs_media.rs"]
mod xhs_media;
#[path = "browserless/xhs_sign.rs"]
mod xhs_sign;

use anyhow::{Context, Result, bail, ensure};
#[cfg(windows)]
use base64::Engine;
use clap::{Parser, ValueEnum};
use reqwest::{Client, Url, cookie::Jar, header, redirect::Policy};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0";
const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Platform {
    Douyin,
    Xiaohongshu,
}

impl Platform {
    fn name(self) -> &'static str {
        match self {
            Self::Douyin => "douyin",
            Self::Xiaohongshu => "xiaohongshu",
        }
    }
    fn allows(self, host: &str) -> bool {
        match self {
            Self::Douyin => ["www.douyin.com", "login.douyin.com", "sso.douyin.com"].contains(&host),
            Self::Xiaohongshu => [
                "www.rednote.com",
                "webapi.rednote.com",
                "www.xiaohongshu.com",
                "edith.xiaohongshu.com",
            ]
            .contains(&host),
        }
    }
}

#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    platform: Platform,
    /// Must be a new absolute directory. Existing directories are never overwritten.
    #[arg(long)]
    output_dir: PathBuf,
    /// Explicitly allow one official bootstrap followed by one QR request; no automatic retries.
    #[arg(long)]
    live: bool,
    /// Wait for phone confirmation and verify official self identity (Xiaohongshu only).
    #[arg(long)]
    login: bool,
    /// Restore only a verified credential file produced by this browserless probe.
    #[arg(long, conflicts_with = "login")]
    restore_auth: Option<PathBuf>,
    /// Read the fixed 50-item window after login or restoration.
    #[arg(long)]
    scan: bool,
    /// Download and ffprobe-check exactly three videos from the verified window.
    #[arg(long, requires = "scan")]
    download_three: bool,
    #[arg(long, requires = "download_three")]
    sample_ids_file: Option<PathBuf>,
    #[arg(long, default_value = "ffprobe")]
    ffprobe: PathBuf,
    /// Explicit experiment with the official SSO QR protocol used by Rust SDKs; never an automatic fallback.
    #[arg(long)]
    douyin_sso: bool,
}

fn trusted(platform: Platform, url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| platform.allows(host))
}

fn bootstrap_redirect(platform: Platform, initial: &Url, next: &Url, previous: usize) -> bool {
    initial.path() == "/" && previous < 3 && trusted(platform, next)
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

fn private_dir(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "output directory must be absolute");
    // create_dir prevents using an existing application, probe, symlink or credential directory.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .context("new private output directory required")?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::fs::create_dir(path).context("new private output directory required")?;
        let account = format!(
            "{}\\{}:(OI)(CI)F",
            std::env::var("USERDOMAIN")?,
            std::env::var("USERNAME")?
        );
        let status = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &account])
            .creation_flags(0x08000000)
            .output()?;
        ensure!(status.status.success(), "private ACL setup failed");
        let check = "$ErrorActionPreference='Stop'; $a=[System.IO.Directory]::GetAccessControl($env:FAVKEEP_PRIVATE_DIR); $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; if(!$a.AreAccessRulesProtected){exit 1}; foreach($r in $a.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($me,'S-1-5-18','S-1-5-32-544')){exit 1}}";
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(check.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .env("FAVKEEP_PRIVATE_DIR", path)
            .creation_flags(0x08000000)
            .output()?;
        ensure!(result.status.success(), "private ACL verification failed");
    }
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn classification(status: u16, body: &[u8], json: Option<&Value>) -> &'static str {
    if [401, 403, 406, 412, 429, 451, 461, 471].contains(&status) {
        return "platform_rejected";
    }
    if status != 200 {
        return "http_failed";
    }
    match json {
        None => {
            let raw = String::from_utf8_lossy(body);
            let scripts = regex::Regex::new(r"(?is)<script\b[^>]*>.*?</script>|<style\b[^>]*>.*?</style>").unwrap();
            let clean = scripts.replace_all(&raw, " ");
            let tags = regex::Regex::new(r"(?s)<[^>]*>").unwrap();
            let visible = tags.replace_all(&clean, " ").to_ascii_lowercase();
            if ["ip at risk", "请完成安全验证", "安全限制"]
                .iter()
                .any(|s| visible.contains(s))
            {
                return "verification_required";
            }
            if raw.contains("__ac_nonce") && raw.contains("__ac_signature") && visible.trim().is_empty() {
                return "javascript_initialization_required";
            }
            "unexpected_non_json"
        }
        Some(v)
            if v.get("success") == Some(&Value::Bool(false))
                || v["status_code"].as_i64().is_some_and(|n| n != 0)
                || v["data"]["error_code"].as_i64().is_some_and(|n| n != 0)
                || v["message"] == "error"
                || v["code"].as_i64().is_some_and(|n| n != 0) =>
        {
            "business_rejected"
        }
        Some(_) => "json_received",
    }
}

async fn request(
    client: &Client,
    platform: Platform,
    request: reqwest::RequestBuilder,
    stage: &str,
    root: &Path,
) -> Result<Value> {
    let request = request.build()?;
    ensure!(trusted(platform, request.url()), "untrusted request target");
    let mut response = client
        .execute(request)
        .await
        .map_err(|_| anyhow::anyhow!("transport_failed"))?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("response_read_failed"))?
    {
        ensure!(
            chunk.len() <= MAX_BODY.saturating_sub(bytes.len()),
            "response_too_large"
        );
        bytes.extend(chunk);
    }
    let value: Option<Value> = serde_json::from_slice(&bytes).ok();
    let result = classification(status, &bytes, value.as_ref());
    // Raw bodies and QR tokens are private evidence only; never print or return them to ordinary logs.
    write(&root.join(format!("{stage}-body.bin")), &bytes)?;
    let report = json!({"stage":stage,"http_status":status,"classification":result,"content_type":content_type,"body_bytes":bytes.len(),"body_md5":format!("{:x}",md5::compute(&bytes)),"business_code":value.as_ref().and_then(|v|v.get("code").or_else(||v.get("status_code")).or_else(||v.get("data").and_then(|d|d.get("error_code")))).and_then(Value::as_i64)});
    write(
        &root.join(format!("{stage}-report.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    if (stage == "bootstrap" || stage.starts_with("document-"))
        && status == 200
        && ["unexpected_non_json", "json_received"].contains(&result)
    {
        return Ok(value.unwrap_or(Value::Null));
    }
    ensure!(result == "json_received", "{result}");
    value.context("missing JSON response")
}

async fn run(args: &Args) -> Result<()> {
    let platform = args.platform;
    let jar = Arc::new(Jar::default());
    let client =
        Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(45))
            .user_agent(UA)
            .cookie_provider(jar.clone())
            .redirect(Policy::custom(move |attempt| {
                if attempt.previous().first().is_some_and(|initial| {
                    bootstrap_redirect(platform, initial, attempt.url(), attempt.previous().len())
                }) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()?;
    match platform {
        Platform::Douyin => {
            if args.douyin_sso {
                let fp = verify_fp();
                jar.add_cookie_str(
                    &format!("s_v_web_id={fp}; Domain=.douyin.com; Path=/; Secure"),
                    &Url::parse("https://www.douyin.com/")?,
                );
                let mut url = Url::parse("https://sso.douyin.com/get_qrcode/")?;
                url.query_pairs_mut()
                    .append_pair("aid", "6383")
                    .append_pair("verifyFp", &fp)
                    .append_pair("fp", &fp);
                let query = url.query().unwrap().to_owned();
                let signature = douyin_sign::sign(
                    &query,
                    "",
                    UA,
                    "1280|900|1280|900|0|0|0|0|1280|900|1280|900|1280|900|24|24|Win32",
                );
                url.query_pairs_mut().append_pair("a_bogus", &signature);
                let value = request(
                    &client,
                    platform,
                    client
                        .get(url)
                        .header(header::ORIGIN, "https://www.douyin.com")
                        .header(header::REFERER, "https://www.douyin.com/")
                        .header(header::ACCEPT, "application/json, text/plain, */*"),
                    "qrcode-sso",
                    &args.output_dir,
                )
                .await?;
                let data = &value["data"];
                ensure!(
                    data["token"].as_str().is_some_and(|s| !s.is_empty())
                        && data["qrcode_index_url"].as_str().is_some_and(|s| !s.is_empty()),
                    "SSO QR fields missing"
                );
                write(
                    &args.output_dir.join("qr-private.json"),
                    &serde_json::to_vec_pretty(data)?,
                )?;
                return Ok(());
            }
            request(
                &client,
                platform,
                client.get("https://www.douyin.com/"),
                "bootstrap",
                &args.output_dir,
            )
            .await?;
            let url = "https://login.douyin.com/passport/web/get_qrcode/";
            let query = [
                ("aid", "6383"),
                ("language", "zh"),
                ("is_new_login", "1"),
                ("need_logo", "false"),
            ];
            let value = request(
                &client,
                platform,
                client
                    .get(url)
                    .query(&query)
                    .header(header::REFERER, "https://www.douyin.com/"),
                "qrcode",
                &args.output_dir,
            )
            .await?;
            let data = &value["data"];
            ensure!(
                data["token"].as_str().is_some_and(|s| !s.is_empty()),
                "QR token missing"
            );
            ensure!(
                data["qrcode"].as_str().is_some_and(|s| !s.is_empty())
                    || data["qrcode_index_url"].as_str().is_some_and(|s| !s.is_empty()),
                "QR payload missing"
            );
            write(
                &args.output_dir.join("qr-private.json"),
                &serde_json::to_vec_pretty(data)?,
            )?;
        }
        Platform::Xiaohongshu => {
            if let Some(source) = &args.restore_auth {
                let mut session = xhs::Session::restore(client, jar, args.output_dir.clone(), source).await?;
                if args.scan {
                    session.scan().await?;
                }
                if args.download_three {
                    xhs_media::download(&session, args.sample_ids_file.as_deref(), &args.ffprobe).await?;
                }
                return Ok(());
            }
            request(
                &client,
                platform,
                client.get("https://www.rednote.com/"),
                "bootstrap",
                &args.output_dir,
            )
            .await?;
            let timestamp = chrono::Utc::now().timestamp_millis() as u64;
            let random = format!("{:032x}", rand::random::<u128>());
            let prefix = format!("{timestamp:x}{}50000", &random[..30]);
            let full = format!("{prefix}{}", xhs_sign::crc32(prefix.as_bytes()));
            let a1 = &full[..52];
            let mut session = xhs::Session::new(client, jar, a1.into(), args.output_dir.clone());
            session.guest();
            let value = session
                .api("/api/sns/web/v1/login/qrcode/create", Some("{}"), "qrcode")
                .await?;
            let data = &value["data"];
            ensure!(
                data["qr_id"].as_str().is_some_and(|s| !s.is_empty())
                    && data["code"].as_str().is_some_and(|s| !s.is_empty()),
                "QR identity missing"
            );
            let url = Url::parse(data["url"].as_str().context("QR URL missing")?)?;
            ensure!(
                url.scheme() == "xhsdiscover" || trusted(platform, &url),
                "untrusted QR target"
            );
            write(
                &args.output_dir.join("qr-private.json"),
                &serde_json::to_vec_pretty(data)?,
            )?;
            if args.login {
                session.login(data).await?;
                if args.scan {
                    session.scan().await?;
                }
                if args.download_three {
                    xhs_media::download(&session, args.sample_ids_file.as_deref(), &args.ffprobe).await?;
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Args::parse();
    ensure!(args.live, "explicit --live is required; no request sent");
    ensure!(
        !args.douyin_sso || matches!(args.platform, Platform::Douyin),
        "SSO experiment is Douyin-only"
    );
    ensure!(
        !args.login || matches!(args.platform, Platform::Xiaohongshu),
        "login currently only supported in the Xiaohongshu probe"
    );
    ensure!(
        !args.scan || args.login || args.restore_auth.is_some(),
        "scan requires login or verified probe restoration"
    );
    ensure!(
        args.restore_auth.is_none() || matches!(args.platform, Platform::Xiaohongshu),
        "restoration currently only supported in the Xiaohongshu probe"
    );
    private_dir(&args.output_dir)?;
    let result = run(&args).await;
    let summary = json!({"platform":args.platform.name(),"arch":std::env::consts::ARCH,"os":std::env::consts::OS,"browser_used":false,"application_database_opened":false,"qr_obtained":args.output_dir.join("qr-private.json").is_file(),"identity_verified":args.output_dir.join("auth.json").is_file(),"window_complete":args.output_dir.join("window-private.json").is_file(),"gate_passed":false,"scope":if args.scan{"authenticated_window_probe"}else if args.restore_auth.is_some(){"restore_probe"}else if args.login{"fresh_qr_login_probe"}else{"fresh_qr_preflight_only"},"failure":result.as_ref().err().map(|e|e.to_string()),"timestamp":chrono::Utc::now().to_rfc3339()});
    write(
        &args.output_dir.join("result.json"),
        &serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string(&summary)?);
    if result.is_err() {
        bail!("protocol preflight failed; see private evidence");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_hosts_and_response_boundaries() {
        let fp = verify_fp();
        let pieces: Vec<_> = fp.split('_').collect();
        assert_eq!(pieces.len(), 7);
        assert_eq!(pieces[0], "verify");
        assert!(pieces[4].starts_with('4') && pieces[5].starts_with('a'));
        for host in [
            "https://www.rednote.com.evil.test/api",
            "http://webapi.rednote.com/api",
            "https://webapi.rednote.com:444/api",
            "https://user@webapi.rednote.com/api",
        ] {
            assert!(!trusted(Platform::Xiaohongshu, &Url::parse(host).unwrap()));
        }
        assert!(!trusted(
            Platform::Douyin,
            &Url::parse("https://www.rednote.com/").unwrap()
        ));
        assert_eq!(classification(406, b"{}", Some(&json!({}))), "platform_rejected");
        assert_eq!(
            classification(200, b"{}", Some(&json!({"data":{"error_code":4031},"message":"error"}))),
            "business_rejected"
        );
        let initial = Url::parse("https://www.rednote.com/").unwrap();
        let target = Url::parse("https://www.rednote.com/login").unwrap();
        assert!(bootstrap_redirect(Platform::Xiaohongshu, &initial, &target, 1));
        assert!(!bootstrap_redirect(Platform::Xiaohongshu, &initial, &target, 3));
        assert!(!bootstrap_redirect(Platform::Xiaohongshu, &target, &initial, 1));
        assert!(!bootstrap_redirect(Platform::Douyin, &initial, &target, 1));
        assert_eq!(
            classification(200, b"<html>IP at risk</html>", None),
            "verification_required"
        );
        assert_eq!(
            classification(
                200,
                b"<title>Login</title><script>const gfkadpd='captcha';</script>",
                None
            ),
            "unexpected_non_json"
        );
        assert_eq!(
            classification(
                200,
                b"<script>const __ac_nonce='fixture', __ac_signature='';</script>",
                None
            ),
            "javascript_initialization_required"
        );
        assert_eq!(
            classification(200, b"{}", Some(&json!({"success":true,"data":{"title":"IP at risk"}}))),
            "json_received"
        );
        assert_eq!(
            classification(200, b"{}", Some(&json!({"success":false,"code":-1}))),
            "business_rejected"
        );
        assert_eq!(classification(200, b"<html>hello</html>", None), "unexpected_non_json");
        assert_eq!(
            classification(200, b"<script src='captcha-sdk.js'></script>", None),
            "unexpected_non_json"
        );
        assert_eq!(
            classification(200, b"{}", Some(&json!({"success":true,"code":0}))),
            "json_received"
        );
    }
    #[test]
    fn existing_output_directory_is_never_changed() {
        let temp = tempfile::tempdir().unwrap();
        let sentinel = temp.path().join("data.sqlite");
        std::fs::write(&sentinel, b"untouched").unwrap();
        assert!(private_dir(temp.path()).is_err());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
    }
}

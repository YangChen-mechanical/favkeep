//! Isolated validation entry point; never opens the application database.
#[path = "../src/douyin/browser.rs"]
mod browser;
#[path = "../src/douyin/client.rs"]
mod client;
#[path = "../src/douyin/core.rs"]
#[allow(dead_code)]
mod core;
#[path = "../src/douyin/sign.rs"]
mod sign;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use browser::{Auth, Session, private_dir, write_private};
use core::{Error, Result, valid_id};

#[derive(Clone, ValueEnum)]
enum Operation {
    Runtime,
    Restore,
    Folders,
    Scan,
    Download,
}

#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    operation: Operation,
    #[arg(long)]
    data_dir: PathBuf,
    #[arg(long)]
    browser: PathBuf,
    /// Import only from the administrator's dedicated private session export.
    #[arg(long)]
    import_session: Option<PathBuf>,
    #[arg(long)]
    folder: Option<String>,
    #[arg(long)]
    id: Option<String>,
    #[arg(long, default_value = "ffprobe")]
    ffprobe: PathBuf,
}

async fn inspect(path: &Path, ffprobe: &Path) -> Result<Value> {
    if std::fs::symlink_metadata(path)
        .map_err(|_| Error::File)?
        .file_type()
        .is_symlink()
    {
        return Err(Error::InvalidData);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(ffprobe)
            .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
            .arg(path)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::MediaUnavailable)?
    .map_err(|_| Error::MediaUnavailable)?;
    if !output.status.success() {
        return Err(Error::MediaUnavailable);
    }
    let info: Value = serde_json::from_slice(&output.stdout).map_err(|_| Error::MediaUnavailable)?;
    let video = info["streams"]
        .as_array()
        .and_then(|streams| streams.iter().find(|s| s["codec_type"] == "video"))
        .ok_or(Error::MediaUnavailable)?;
    let duration: f64 = info["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::MediaUnavailable)?;
    let bytes = std::fs::metadata(path).map_err(|_| Error::File)?.len();
    if !duration.is_finite() || duration <= 0.0 || bytes == 0 {
        return Err(Error::MediaUnavailable);
    }
    Ok(
        json!({"bytes":bytes,"duration":duration,"video_codec":video["codec_name"],
        "audio_codecs":info["streams"].as_array().unwrap().iter().filter(|s| s["codec_type"] == "audio")
            .map(|s| s["codec_name"].clone()).collect::<Vec<_>>()}),
    )
}

async fn operation(args: &Args, session: &Session, root: &Path) -> Result<Value> {
    if matches!(args.operation, Operation::Runtime) {
        let v = session
            .page
            .evaluate("document.title='rust-cdp-runtime'; navigator.userAgent")
            .await
            .map_err(|_| Error::Browser)?
            .into_value::<String>()
            .map_err(|_| Error::Browser)?;
        if !v.contains("Chrome/") {
            return Err(Error::Browser);
        }
        return Ok(json!({"browser_started":true}));
    }
    let source = args.import_session.clone().unwrap_or_else(|| root.join("auth.json"));
    let saved = Auth::read(&source)?;
    let auth = session.restore(&saved, args.import_session.is_some()).await?;
    write_private(&root.join("auth.json"), &auth)?;
    if matches!(args.operation, Operation::Restore) {
        return Ok(json!({"authenticated":true,"same_account":true,"browser_token_observed":true}));
    }
    let client = client::Client::new(&auth).await?;
    if matches!(args.operation, Operation::Folders | Operation::Scan) {
        let folders = client.folders().await?;
        write_private(&root.join("folders.json"), &folders)?;
        if matches!(args.operation, Operation::Folders) {
            return Ok(json!({"folder_count":folders.len(),"folders":folders.iter()
                .map(|f| json!({"id":f.id,"count":f.total})).collect::<Vec<_>>()}));
        }
        let id = args.folder.as_deref().ok_or(Error::InvalidData)?;
        let folder = folders.iter().find(|f| f.id == id).ok_or(Error::InvalidData)?;
        let items = client.scan_folder(folder).await?;
        write_private(
            &root.join("verified-folder.json"),
            &json!({"folder":folder.id,"identity":auth.identity,"items":items}),
        )?;
        return Ok(
            json!({"count":items.len(),"videos":items.iter().filter(|i| i.kind == "video").count(),"complete":true}),
        );
    }
    let id = args.id.as_deref().filter(|id| valid_id(id)).ok_or(Error::InvalidData)?;
    let snapshot: Value =
        serde_json::from_slice(&std::fs::read(root.join("verified-folder.json")).map_err(|_| Error::File)?)
            .map_err(|_| Error::InvalidData)?;
    if snapshot["identity"] != auth.identity
        || !snapshot["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["id"] == id && i["kind"] == "video"))
    {
        return Err(Error::InvalidData);
    }
    let media = root.join("media");
    private_dir(&media)?;
    let final_path = media.join(format!("{id}.mp4"));
    if final_path.exists() {
        return Ok(json!({"already_completed":true,"media":inspect(&final_path,&args.ffprobe).await?}));
    }
    let video = client.detail(id).await?;
    write_private(&root.join("detail.json"), &video)?;
    let url = video.urls.first().ok_or(Error::MediaUnavailable)?;
    let mut response = client.media_response(url).await?;
    let expected = response.content_length();
    let temporary = media.join(format!("{}.part", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create_new(&temporary).await.map_err(|_| Error::File)?;
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
        file.write_all(&chunk).await.map_err(|_| Error::File)?;
        received += chunk.len() as u64;
    }
    file.sync_all().await.map_err(|_| Error::File)?;
    drop(file);
    if expected.is_some_and(|n| n != received) {
        return Err(Error::MediaUnavailable);
    }
    let info = inspect(&temporary, &args.ffprobe).await?;
    tokio::fs::rename(&temporary, &final_path)
        .await
        .map_err(|_| Error::File)?;
    Ok(json!({"completed":true,"media":info}))
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("douyin_probe=debug")
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let args = Args::parse();
    let start = Instant::now();
    let result = async {
        private_dir(&args.data_dir)?;
        let root = args.data_dir.canonicalize().map_err(|_| Error::File)?;
        let session = Session::launch(&root, &args.browser).await?;
        let result = tokio::time::timeout(Duration::from_secs(900), operation(&args, &session, &root))
            .await
            .map_err(|_| Error::Network)
            .and_then(|r| r);
        let closed = session.close().await;
        closed?;
        result
    }
    .await;
    let success = result.is_ok();
    let summary = match result {
        Ok(data) => json!({"status":"passed","data":data,"elapsed_seconds":start.elapsed().as_secs_f64()}),
        Err(error) => {
            json!({"status":"failed","error":error.to_string(),"elapsed_seconds":start.elapsed().as_secs_f64()})
        }
    };
    let _ = write_private(&args.data_dir.join("last-result.json"), &summary);
    println!("{summary}");
    if !success {
        std::process::exit(1);
    }
}

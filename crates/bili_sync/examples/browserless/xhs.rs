// The gate executes the application's HTTP client; no duplicated login or collection protocol.
#[path = "../../src/douyin/core.rs"]
#[allow(dead_code)]
mod core;
pub use core::{Error, Result, valid_id};
#[path = "../../src/xiaohongshu/client.rs"]
mod client;
use super::codec;
#[path = "../../src/xiaohongshu/cookies.rs"]
mod cookies;
#[path = "../../src/xiaohongshu/sign.rs"]
mod sign;
#[path = "../../src/xiaohongshu/state.rs"]
mod state;

use super::{Args, media, write};
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;

pub async fn run(args: &Args) -> anyhow::Result<()> {
    let root = &args.output_dir;
    let (session, mut auth) = if let Some(source) = &args.restore_auth {
        let marker: Value = serde_json::from_slice(&std::fs::read(
            source.parent().context("auth_parent_missing")?.join("result.json"),
        )?)?;
        ensure!(
            marker["platform"] == "xiaohongshu"
                && marker["browser_used"] == false
                && marker["identity_verified"] == true,
            "verified_browserless_probe_auth_required"
        );
        let saved = client::Auth::read(source)?;
        let session = client::Session::new(Some(&saved))?;
        let auth = client::authenticate(&session, Some(&saved), Some(&saved.identity), None).await?;
        println!(
            "{}",
            json!({"stage":"restored","same_identity":true,"application_http_client":true,"browser_used":false})
        );
        (session, auth)
    } else {
        let session = client::Session::new(None)?;
        let auth = {
            let (tx, mut rx) = tokio::sync::watch::channel(None);
            let future = client::authenticate(&session, None, None, Some(&tx));
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result=&mut future=>break result?,
                    changed=rx.changed()=>{
                        changed.context("QR_channel_closed")?;
                        if let Some(url)=rx.borrow_and_update().clone() {
                            write(&root.join("qr-private.json"),&serde_json::to_vec_pretty(&json!({"url":url}))?)?;
                            println!("{}",json!({"stage":"qr_ready","application_http_client":true,"browser_used":false}));
                            if !args.login {return Ok(());}
                        }
                    }
                }
            }
        };
        (session, auth)
    };
    println!(
        "{}",
        json!({"stage":"identity_verified","application_http_client":true,"browser_used":false})
    );
    let items = if args.scan {
        let items = client::scan(&session, &auth).await?;
        auth = client::authenticate(&session, Some(&auth), Some(&auth.identity), None).await?;
        write(
            &root.join("window-private.json"),
            &serde_json::to_vec_pretty(&json!({"window_complete":true,"items":items}))?,
        )?;
        let videos = items.iter().filter(|i| i.kind == "video").count();
        println!(
            "{}",
            json!({"stage":"window_read","unique_items":items.len(),"videos":videos,"unsupported":items.len()-videos,"window_complete":true,"application_http_client":true,"browser_used":false})
        );
        items
    } else {
        Vec::new()
    };
    write(&root.join("auth.json"), &serde_json::to_vec_pretty(&auth)?)?;
    if args.download_three {
        let ids: Vec<String> = if let Some(path) = &args.sample_ids_file {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            items
                .iter()
                .filter(|i| i.kind == "video")
                .take(3)
                .map(|i| i.id.clone())
                .collect()
        };
        ensure!(
            ids.len() == 3 && ids.iter().collect::<HashSet<_>>().len() == 3,
            "three_distinct_sample_IDs_required"
        );
        ensure!(
            ids.iter()
                .all(|id| items.iter().any(|i| i.id == *id && i.kind == "video")),
            "sample_not_in_verified_window"
        );
        write(&root.join("sample-ids-private.json"), &serde_json::to_vec_pretty(&ids)?)?;
        let mut reports = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let item = items.iter().find(|i| i.id == *id).context("sample_missing")?;
            let detail = client::detail(&session, &auth, item).await?;
            let response = client::media_response(&auth, detail.url).await?;
            let mut report = media::publish(response, root, index + 1, &args.ffprobe).await?;
            report["title_chars"] = json!(detail.title.chars().count());
            report["author_chars"] = json!(detail.author.chars().count());
            println!("{}", report);
            reports.push(report);
        }
        write(&root.join("downloads.json"), &serde_json::to_vec_pretty(&reports)?)?;
    }
    Ok(())
}

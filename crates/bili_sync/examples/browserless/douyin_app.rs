//! Runs the production native client without opening the application database.
#[path = "../../src/douyin/core.rs"]
#[allow(dead_code)]
pub(crate) mod core;
pub use core::{Error, Result, valid_id};
#[path = "../../src/douyin/client.rs"]
mod client;
#[path = "../../src/utils/cookies.rs"]
pub(crate) mod cookies;
#[path = "../../src/douyin/media.rs"]
pub(crate) mod media;
#[path = "../../src/douyin/native/mod.rs"]
pub(crate) mod native;

use super::{Args, codec, write};
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;

pub async fn run(args: &Args) -> anyhow::Result<()> {
    let saved = args.restore_auth.as_deref().map(native::Auth::read).transpose()?;
    if let Some(source) = &args.restore_auth {
        let marker: Value = serde_json::from_slice(&std::fs::read(
            source.parent().context("auth_parent_missing")?.join("result.json"),
        )?)?;
        ensure!(
            marker["platform"] == "douyin" && marker["identity_verified"] == true && marker["browser_used"] == false,
            "verified_native_probe_auth_required"
        );
    }
    let session = native::Session::new(saved.as_ref())?;
    let auth = if let Some(saved) = &saved {
        session.restore(saved).await?
    } else {
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        let login = session.login(None, &tx);
        tokio::pin!(login);
        loop {
            tokio::select! {
                result = &mut login => break result?,
                changed = rx.changed() => {
                    changed?;
                    if let Some(url) = rx.borrow().as_ref() {
                        write(&args.output_dir.join("qr-private.json"), &serde_json::to_vec_pretty(&json!({"qrcode_index_url":url}))?)?;
                        println!("{}", json!({"stage":"application_qr_ready","browser_used":false}));
                    }
                }
            }
        }
    };
    let mut exported = serde_json::to_value(&auth)?;
    exported["identity_verified"] = json!(true);
    write(
        &args.output_dir.join("verified-candidate-private.json"),
        &serde_json::to_vec_pretty(&exported)?,
    )?;
    println!(
        "{}",
        json!({"stage":"application_identity_verified","restored":saved.is_some(),"browser_used":false})
    );
    if !args.scan {
        write(
            &args.output_dir.join("auth.json"),
            &serde_json::to_vec_pretty(&exported)?,
        )?;
        return Ok(());
    }
    let client = client::Client::new(&auth).await?;
    let folders = client.folders().await?;
    let selected_bytes = std::fs::read(args.folder_ids_file.as_ref().context("selected_folders_required")?)?;
    ensure!(selected_bytes.len() <= 16 * 1024, "selected_folder_file_too_large");
    let selected: Vec<String> = serde_json::from_slice(&selected_bytes)?;
    ensure!(
        !selected.is_empty()
            && selected.iter().all(|id| valid_id(id))
            && selected.iter().collect::<HashSet<_>>().len() == selected.len(),
        "invalid_selected_folders"
    );
    let mut video_ids = HashSet::new();
    let mut records = Vec::new();
    for id in selected {
        let folder = folders
            .iter()
            .find(|f| f.id == id)
            .context("selected_folder_unavailable")?;
        let items = client.scan_folder(folder).await?;
        client.verify(&auth.identity).await?;
        video_ids.extend(items.iter().filter(|i| i.kind == "video").map(|i| i.id.clone()));
        println!(
            "{}",
            json!({"stage":"application_collection_complete","items":items.len(),"real_terminal":true})
        );
        records.push(json!({"folder_id":id,"items":items}));
    }
    write(
        &args.output_dir.join("collections-private.json"),
        &serde_json::to_vec_pretty(&json!({"collection_complete":true,"folders":records}))?,
    )?;
    if args.download_three {
        let bytes = std::fs::read(args.sample_ids_file.as_ref().context("fixed_sample_ids_required")?)?;
        ensure!(bytes.len() <= 16 * 1024, "sample_id_file_too_large");
        let ids: Vec<String> = serde_json::from_slice(&bytes)?;
        ensure!(
            ids.len() == 3
                && ids.iter().collect::<HashSet<_>>().len() == 3
                && ids.iter().all(|id| video_ids.contains(id)),
            "three_distinct_selected_video_samples_required"
        );
        let mut reports = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let detail = client.detail(id).await?;
            let response = client
                .media_response(detail.urls.first().context("sample_url_missing")?)
                .await?;
            let report = super::media::publish(response, &args.output_dir, index + 1, &args.ffprobe).await?;
            println!("{}", report);
            reports.push(report);
        }
        write(
            &args.output_dir.join("downloads.json"),
            &serde_json::to_vec_pretty(&reports)?,
        )?;
    }
    let mut fresh = serde_json::to_value(client.verify(&auth.identity).await?)?;
    fresh["identity_verified"] = json!(true);
    write(&args.output_dir.join("auth.json"), &serde_json::to_vec_pretty(&fresh)?)?;
    Ok(())
}

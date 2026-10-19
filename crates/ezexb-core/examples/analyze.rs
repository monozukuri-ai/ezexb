//! cargo run -p ezexb-core --example analyze -- drawing.exb
//!
//! Emit bounded metadata, preview information and decoded-stream hashes as JSON.
//! Each optional decoder reports its own error; unknown record schemas remain
//! explicit stopped inspections rather than silently omitted geometry.

use ezexb_core::{Container, EntryKind, ParseOptions};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env};

fn reported<T: Serialize>(result: Result<T, ezexb_core::Error>) -> Value {
    match result {
        Ok(value) => json!({"status": "ok", "value": value}),
        Err(error) => json!({"status": "error", "error": error.to_string()}),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments.next().ok_or("usage: analyze FILE")?;
    if arguments.next().is_some() {
        return Err("usage: analyze FILE".into());
    }
    let mut container = Container::open(path, ParseOptions::default())?;
    let inspection = container.info().clone();
    let file_info = reported(container.file_info());
    let app_info = reported(container.app_info());
    let preview_info = reported(container.preview_info());
    let preview_bmp = reported(container.preview_bmp().map(|bytes| {
        json!({"size_bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))})
    }));
    let mut streams = BTreeMap::new();
    for entry in &inspection.entries {
        if entry.kind != EntryKind::Stream {
            continue;
        }
        let payload = reported(container.stream_payload(&entry.path).map(|decoded| {
            json!({
                "info": decoded.info,
                "sha256": format!("{:x}", Sha256::digest(&decoded.data)),
            })
        }));
        let records =
            if entry.path.starts_with("/*BlockStg/*Blk_") || entry.path == "/*BlockStg/*GBlock" {
                reported(container.record_headers(&entry.path))
            } else {
                json!({"status": "not_applicable"})
            };
        streams.insert(
            entry.path.clone(),
            json!({"payload": payload, "records": records}),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "inspection": inspection,
            "file_info": file_info,
            "app_info": app_info,
            "preview_info": preview_info,
            "preview_bmp": preview_bmp,
            "streams": streams,
        }))?
    );
    Ok(())
}

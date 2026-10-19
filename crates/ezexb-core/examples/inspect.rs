//! cargo run -p ezexb-core --example inspect -- drawing.exb
use ezexb_core::{Container, EntryKind, ParseOptions};
use std::{collections::BTreeMap, env};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("usage: inspect FILE")?;
    let mut container = Container::open(path, ParseOptions::default())?;
    let info = container.info().clone();
    let mut hashes = BTreeMap::new();
    for entry in &info.entries {
        if entry.kind == EntryKind::Stream {
            hashes.insert(entry.path.clone(), container.stream_sha256(&entry.path)?);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "inspection": info, "stream_sha256": hashes
        }))?
    );
    Ok(())
}

//! Generate original synthetic entity-extension fixtures without overwriting.
#[path = "support/entity_fixture.rs"]
mod fixture;
use std::{fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: make_entity_fixture OUTPUT [MAJOR]")?;
    let major = args.next().map(|s| s.parse()).transpose()?.unwrap_or(10);
    if ![10, 11, 15, 16, 18].contains(&major) || args.next().is_some() {
        return Err("expected a supported major: 10, 11, 15, 16 or 18".into());
    }
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(&fixture::fixture(major))?;
    Ok(())
}

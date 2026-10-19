//! Create an original synthetic complete stored-entity fixture; never overwrite.
#[path = "support/document_fixture.rs"]
mod fixture;
use std::{fs::OpenOptions, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: make_document_fixture OUTPUT [MAJOR]")?;
    let major = args.next().map(|s| s.parse()).transpose()?.unwrap_or(10);
    let mut bytes = fixture::fixture(major);
    if args.next().as_deref() == Some("--unknown") {
        bytes = fixture::mutate_stream(&bytes, "/*BlockStg/*GBlock", |data| {
            data.extend(fixture::record(
                99,
                -5,
                "11111111-1111-1111-1111-111111111111",
                1,
                &[9; 7],
            ))
        });
    }
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(&bytes)?;
    Ok(())
}

//! Print a bounded stored-entity document. --partial retains unsupported spans.
use ezexb_core::{read_file, Error, ParseOptions};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("usage: read FILE [--partial]")?;
    let partial = args.next().is_some_and(|value| value == "--partial");
    match read_file(path, ParseOptions::default(), !partial) {
        Ok(document) => println!("{}", serde_json::to_string(&document)?),
        Err(Error::IncompleteRead(report)) => {
            eprintln!("{}", serde_json::to_string(&report)?);
            std::process::exit(2);
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

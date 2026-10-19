//! Print world instances or a portable SVG with explicit diagnostics.
use ezexb_core::{expand_world, read_file, render_svg, ParseOptions};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: world FILE SPACE_ID [--svg]")?;
    let space_id = args.next().ok_or("space ID required")?.parse()?;
    let options = ParseOptions::default();
    let document = read_file(path, options, false)?;
    if args.next().as_deref() == Some("--svg") {
        println!(
            "{}",
            serde_json::to_string(&render_svg(&document, space_id, options)?)?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string(&expand_world(&document, space_id, options)?)?
        );
    }
    Ok(())
}

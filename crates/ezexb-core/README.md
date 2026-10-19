# ezexb-core

An experimental, read-only CAXA EXB parser in pure Rust. No Python or native CAD
runtime is required. The crate supports selected stored-entity layouts, metadata,
embedded BMP previews and CFB v3/v4 stream inspection.

## Read entities

```rust,no_run
use ezexb_core::{read_file, ParseOptions};

fn main() -> Result<(), ezexb_core::Error> {
    let document = read_file("drawing.exb", ParseOptions::default(), false)?;
    println!("Complete: {}", document.report.complete);
    println!("Entities: {}", document.entities.len());
    for diagnostic in &document.report.diagnostics {
        println!("{}", diagnostic.code);
    }
    Ok(())
}
```

The final argument selects strictness. Strict reads return
`Error::IncompleteRead(report)` for unsupported required content. Partial reads
return the supported subset with diagnostics; input and resource errors still
fail. `read_bytes` and `Container::document` expose the same reader.

Supported data includes LINE, CIRCLE, ARC, POLYLINE, TEXT, INSERT attributes,
attribute definitions, HATCH/SOLID boundaries, stored NURBS/ELLIPSE parameters,
partial dimension data and layer/style/space/block context. Coordinates remain
in the stored block coordinate system, and physical units are not inferred.
Exact supported builds and limitations are documented in the
[reader guide](https://github.com/monozukuri-ai/ezexb/blob/main/docs/reading.md).

## Inspect streams

```rust,no_run
use ezexb_core::{Container, ParseOptions};

fn main() -> Result<(), ezexb_core::Error> {
    let mut file = Container::open("drawing.exb", ParseOptions::default())?;
    println!("{}", file.info().classification);
    let bytes = file.read_stream("/*FileInfoStream", 0, Some(64))?;
    println!("Read {} bytes", bytes.len());
    Ok(())
}
```

Containers are owned snapshots. Stream paths use forward slashes on every
platform and must match the listing. Listing validates the directory; read or
hash streams to validate their contents. Metadata, preview extraction,
decompression and record inspection have separate bounded APIs.

## World coordinates and SVG

`expand_world(&document, space_id, options)` expands INSERTs and resolves instance
attributes. Check source, transformation, geometry and style completeness
separately. `render_svg(&document, space_id, options)` creates an approximate
world-XY preview; native font layout and full rendering are not supported.
`Container::world_scene` and `Container::svg_preview` provide the same operations
from a snapshot.

From the workspace checkout:

```sh
cargo run -p ezexb-core --example read -- drawing.exb --partial
cargo run -p ezexb-core --example inspect -- drawing.exb
cargo run -p ezexb-core --example analyze -- drawing.exb
cargo run -p ezexb-core --example world -- drawing.exb -7
```

MIT; see [LICENSE](LICENSE). Tests use original synthetic fixtures and do not
require vendor drawings or software.

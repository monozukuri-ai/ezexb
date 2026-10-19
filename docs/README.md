# Documentation

ezexb provides an experimental stored-data reader and lower-level inspection
tools. All APIs are read-only and run without a native CAD installation.

| Task | Guide |
| --- | --- |
| Install and extract your first entities | [Quick start](../README.md) |
| Read geometry, attributes, blocks and styles | [Stored-entity reader](reading.md) |
| Expand INSERTs or create an approximate SVG | [World coordinates and SVG](reading.md#world-coordinates-and-portable-svg) |
| List, hash and extract CFB streams | [Container inspection](inspection.md) |
| Read metadata, extract previews or inspect records | [Stream analysis](stream-analysis.md) |
| Use the pure Rust library | [Rust core](../crates/ezexb-core/README.md) |
| Build, test and contribute | [Development](development.md) |
| Prepare and publish a version | [Releasing](releasing.md) |

Start with `read(..., strict=False)` when exploring a drawing, and inspect
`document.report`. Strict mode is the default for callers that require every
required object in the supported profile to be decoded. Resource limits apply
in both modes.

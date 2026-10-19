# ezexb

A read-only CAXA EXB parser with a Rust core, Python API and command-line tools.
No CAXA installation is required.

**Status: experimental.** The reader supports a limited set of EXB layouts and
returns explicit diagnostics for unsupported content. APIs may change before
1.0. Use it to extract supported data; check the read report before relying on a
complete drawing.

## Install

Requires GIL-enabled CPython 3.10 or newer. For a published release:

```sh
python -m pip install ezexb
```

To build from a source checkout, install Rust 1.83 or newer and run:

```sh
python -m pip install .
```

Prebuilt release wheels target Linux x86_64 and aarch64 (glibc 2.28+), Windows x86_64, and
macOS Intel / Apple silicon. A source install requires a Rust toolchain.
See [development](https://github.com/monozukuri-ai/ezexb/blob/main/docs/development.md) for an editable installation and tests.

## Read a drawing

```python
import ezexb

document = ezexb.read("drawing.exb", strict=False)
print("Complete:", document.report.complete)

for diagnostic in document.report.diagnostics:
    print(diagnostic.code, diagnostic.stream_path, diagnostic.span)

for entity in document.entities:
    if entity.kind == "Line":
        print(entity.geometry.start, entity.geometry.end)
```

`strict=False` requests a partial document. The default, `strict=True`, raises
`IncompleteReadError` when required content cannot be read; the exception carries
the same structured report. Unknown build profiles raise `UnsupportedFeatureError`
in both modes. Input and resource errors always raise.

Coordinates are stored in each block's coordinate system, and physical units
are not inferred. Use `read_world()` to expand a selected space. A complete report
means completeness within the [supported reader profile](https://github.com/monozukuri-ai/ezexb/blob/main/docs/reading.md), not
full EXB compatibility or CAXA display equivalence.

## Command line

```sh
ezexb capabilities --json
ezexb read drawing.exb --partial --json
ezexb inspect drawing.exb --json
ezexb streams drawing.exb --sha256 --json
ezexb metadata drawing.exb --json
ezexb preview drawing.exb --output preview.bmp
```

List `document.spaces` to choose a space ID for world coordinates or an approximate
SVG. For example, if the desired space has ID `-7`:

```sh
ezexb world drawing.exb --space=-7 --partial --json
ezexb svg drawing.exb --space=-7 --partial --output drawing.svg --json
```

`read --partial` can exit successfully with `report.complete=false`; inspect the
JSON report in automation. Extraction commands refuse to overwrite existing files.
`preview` extracts a cached BMP; `svg` generates an approximate drawing preview.

## Supported data

| Area | Available data |
| --- | --- |
| Geometry | LINE, CIRCLE, ARC, POLYLINE, stored NURBS and ELLIPSE parameters |
| Text and blocks | TEXT, INSERT, attached attributes, attribute definitions |
| Fills and dimensions | HATCH/SOLID boundaries and parameters; stored data and presentation for five dimension types |
| Context | Layers, linetypes, text styles, spaces and blocks |
| Inspection | CFB streams, metadata, embedded BMP, decoded payloads and record headers |

Support depends on the exact layout and build profile. Dimension construction
and associativity, physical units, external references, native font layout and
complete rendering remain unsupported. SVG uses approximate text and strokes;
it does not regenerate hatch fills. Writing and DXF export are not provided.

## Documentation

- [Documentation index](https://github.com/monozukuri-ai/ezexb/blob/main/docs/README.md)
- [Reading, supported profiles and diagnostics](https://github.com/monozukuri-ai/ezexb/blob/main/docs/reading.md)
- [Container and stream inspection](https://github.com/monozukuri-ai/ezexb/blob/main/docs/inspection.md)
- [Metadata, previews and stream analysis](https://github.com/monozukuri-ai/ezexb/blob/main/docs/stream-analysis.md)
- [Rust core](https://github.com/monozukuri-ai/ezexb/blob/main/crates/ezexb-core/README.md)
- [Development](https://github.com/monozukuri-ai/ezexb/blob/main/docs/development.md) and [releasing](https://github.com/monozukuri-ai/ezexb/blob/main/docs/releasing.md)

## License

MIT; see [LICENSE](https://github.com/monozukuri-ai/ezexb/blob/main/LICENSE). This license covers ezexb's own code. Third-party
drawings and vendor materials are not included in source or binary distributions.

# Stream analysis

This API reads observed EXB metadata, stored BMP previews, compressed stream
payloads, record headers with nested display caches, and experimental LINE coordinates
and raw attributes. The separate [stored-entity reader](reading.md) implements
`ezexb.read()` under a limited profile. No CAXA installation is required.

## Python

```python
from pathlib import Path
import ezexb
from ezexb import raw

metadata = ezexb.metadata(Path("drawing.exb"))
print(metadata.file_info.product_build.value)
for item in metadata.file_info.properties:
    print(item.key.value, item.value.value)
for record in metadata.app_info.records:
    print([field.value for field in record.strings])

with raw.open("drawing.exb") as container:
    preview = container.preview_info()
    print(preview.width, preview.height, preview.bits_per_pixel)
    with Path("preview.bmp").open("xb") as destination:
        destination.write(container.preview_bmp())

    decoded = container.stream_payload("/*SpaceStream")
    print(decoded.info.encoding, decoded.info.logical_size, decoded.info.blocks)
    headers = container.record_headers("/*BlockStg/*Blk_fffffffb")
    for record in headers.records:
        print(record.class_name, record.type_id, record.span)
        if record.extension is not None:
            print(record.extension.relations)
            for child in record.extension.display_cache.records:
                print("display child", child.type_id, child.span)
    print(headers.profile)
    print(headers.framing_complete, headers.stop_reason, headers.unparsed_span)
    result = container.lines("/*BlockStg/*Blk_fffffffb")
    for line in result.lines:
        header = result.framing.records[line.record_index]
        print(header.id_raw, line.start, line.end, line.attributes)
    print(result.unsupported_lines, result.framing.stop_reason)
```

`metadata_bytes(data)` and `preview_bytes(data)` accept file contents in memory.
`preview(path)` returns `PreviewInfo`; use `raw.Container.preview_bmp()` for the
original BMP bytes. `file_info()` and `app_info()` also work independently when
only one metadata stream is needed. Typed results are frozen dataclasses in
`ezexb.content`; `dataclasses.asdict()` converts them to JSON-compatible mappings
except for the `bytes` member of `DecodedStream`.

FileInfo exposes length-prefixed UTF-16 strings and key/value properties.
AppInfo preserves ordered records and string fields, including empty strings.
Uninterpreted bytes are retained as `UnknownField(span, hex)`. Neither build
strings nor the `ProductVersion` property establish complete format support.
Opaque date-shaped fields are not presented as creation or modification times.

## Rust

```rust,no_run
use ezexb_core::{Container, ParseOptions};

fn main() -> Result<(), ezexb_core::Error> {
    let mut container = Container::open("drawing.exb", ParseOptions::default())?;
    let file = container.file_info()?;
    println!("{}", file.product_build.value);
    let app = container.app_info()?;
    let bmp = container.preview_bmp()?;
    let decoded = container.stream_payload("/*SpaceStream")?;
    let records = container.record_headers("/*BlockStg/*Blk_fffffffb")?;
    let lines = container.lines("/*BlockStg/*Blk_fffffffb")?;
    Ok(())
}
```

The standalone `decode_file_info`, `decode_app_info`, `decode_preview`,
`extract_preview_bmp`, `decode_stream_payload`, `inspect_record_headers`, and `inspect_lines`
functions accept byte slices and `ParseOptions`. AppInfo and record inspection
expect a decompressed logical payload. Other metadata and preview decoders
expect the original uncompressed stream. The `analyze` example emits JSON with
per-stream payload hashes and explicit decoder statuses:

```sh
cargo run -p ezexb-core --example analyze -- drawing.exb
```

Standalone record functions have no FileInfo context and stop at object schema 3.
Use `inspect_record_headers_with_profile(data, options, Some(profile))` or
`inspect_lines_with_profile` with `RecordProfile::Schema3Attributes33` or
`RecordProfile::Schema3Attributes37` when the caller has established the profile.
Passing an explicit profile is a caller assertion, not automatic format detection.

## CLI

```sh
ezexb metadata drawing.exb --json
ezexb preview drawing.exb --json
ezexb preview drawing.exb --output preview.bmp
ezexb payload drawing.exb '/*SpaceStream' --json
ezexb payload drawing.exb '/*SpaceStream' --hex
ezexb payload drawing.exb '/*SpaceStream' --output decoded-space.bin
ezexb records drawing.exb '/*BlockStg/*Blk_fffffffb' --json
ezexb lines drawing.exb '/*BlockStg/*Blk_fffffffb' --json
```

`stream` returns stored bytes. `payload` validates and decompresses all blocks
of the observed EXB zlib envelope, checks checksums and final zero padding, and
returns only the logical payload. Unwrapped streams pass through unchanged;
unknown wrappers are not inferred from content. A recognized but invalid or
unsupported envelope raises `InvalidStreamError`. Extraction creates a new
file and refuses to overwrite an existing one.

## Supported layouts and bounds

Decoded metadata reports `evidence="corpus_consistent"`, not official format
certification. The supported preview profile is the observed EXB wrapper with a
24-bit BI_RGB BMP and a 40-byte DIB header. Its image dimensions, row padding,
pixel extent, and enclosing lengths are validated. A preview can be blank or
stale and does not establish whether a drawing contains entities.

Record inspection accepts only block stream paths and the observed version-0.0
header. It walks supported frames in order, retaining unknown type IDs as opaque
payload spans. The legacy `schema` field preserves the original two bytes as a
little-endian integer. These bytes are separately exposed as `object_schema` and
`properties_present`; supported pairs are `(1, false)`, `(1, true)`, `(100, true)`,
and, under the experimental profiles below, `(3, false)` and `(3, true)`.
Property groups retain GUIDs, typed value tags, keys, and exact value spans. To
inspect an uninterpreted value, slice `stream_payload().data` using `value_span`.
Unknown schemas or property tags stop inspection before the entire record, without
searching for another familiar GUID. `framing_complete` means all bytes were
framed; `record_headers().geometry_decoded` remains false.
A stopped inspection is a successful diagnostic result, so the
CLI returns zero; check `framing_complete`, `stop_reason`, and `unparsed_span`.

### Schema-3 relations and display caches

`Container.record_headers()` and `Container.lines()` select a framing profile
from these exact FileInfo `product_build` values:

| FileInfo product build | `RecordInspection.profile` | Attribute bytes |
| --- | --- | ---: |
| `10.0.0.0000`, `15.0.0.5681` | `schema3-attributes33-experimental` | 33 |
| `18.0.0.18159` | `schema3-attributes37-experimental` | 37 |

An absent, unsupported, or malformed FileInfo leaves the profile unset. Ordinary
supported frames remain readable, but schema 3 then stops before its first
record. Resource-limit, I/O and container errors still propagate. Build strings
select an observed layout; they do not certify all files produced by that build.

For schema 3, `RecordHeader.payload_span` covers only the initial length-prefixed
body. `extension` holds the following relation section, raw header values,
attribute span, display cache and tail span. `RecordHeader.span` covers the
complete record including children. Relations currently frame
`CxDbSingleRelation`, `CxDbRefPt` and `CxDbLoopRelation`; their value semantics
remain opaque and their original bytes are accessible through `span`.

`extension.display_cache.records` contains recursive `RecordHeader` objects for
`CxGrpDispData`. These cached display objects are not ordinary top-level entities
and are never flattened into `RecordInspection.records` or `LineInspection.lines`.
All nested spans use the same decoded-stream origin. An absent cache has
`present=false`, `type_id=None`, and an empty records tuple.

The profiles require observed header values, attributes matching the initial
body prefix, recognized relation/cache types, and a zero tail flag. Other values,
including the unimplemented tail flag 1, stop before the entire enclosing
top-level record. No partially parsed tree is returned. Truncated known fields
raise an error. No GUID scanning or inferred record boundaries are used.

## Experimental LINE inspection

`lines()` selects consecutively framed top-level `CxDbLine` records with object schema 1
and an 81-byte body. It decodes a 33-byte attribute prefix and six little-endian
float64 values as stored start/end XYZ coordinates. Each line references its
header through `record_index`. Other LINE layouts and non-finite numbers are
reported in `unsupported_lines`; they are never substituted with zero coordinates.
Other record types are counted by `other_record_count`.
The attribute fields include raw layer/linetype relation IDs and status words,
stored color and width, three inheritance flags, the raw hide flag, and a float32
linetype scale. Stored float32 values are promoted exactly to float64 so Rust,
Python, and JSON report the same numeric value. Raw stored color/width/linetype values may differ from rendered
values when inherited. Layer names, inheritance, spaces, units, block transforms,
and other entity types are not resolved. `drawing_complete` is always false;
`supported_profiles` in capabilities remains the list of qualified full-drawing
profiles (currently empty).

| Offset | Coordinate space |
| --- | --- |
| FileInfo text/unknown spans | Original `*FileInfoStream` bytes |
| Preview BMP and unknown spans | Original `*PreviewStream` bytes |
| AppInfo, record, property, relation, display-cache and LINE spans | Decompressed logical stream bytes |
| `StreamPayloadBlock.stream_offset` | Compressed bytes after the block length in the original stream |
| `StreamPayloadBlock.logical_offset` | Decompressed logical stream bytes |

None of these are absolute offsets in the CFB file. The compressed block index
locates the source block; there is no byte-for-byte compressed offset mapping.

`ParseOptions` adds per-operation budgets:

| Option | Default | Applies to |
| --- | ---: | --- |
| `max_decoded_stream_bytes` | 64 MiB | Entire decoded allocation, including padding |
| `max_string_bytes` | 64 KiB | Each stored UTF-16 string, including its terminator |
| `max_preview_pixels` | 16,777,216 | Width × absolute height |
| `max_records` | 100,000 | Cumulative top-level/nested records, property groups/values, relations and relation-array entries; metadata/compression have their own per-operation counts |
| `max_record_depth` | 32 | Nested display-record depth, with top-level depth 0 and an unconditional safety ceiling of 64; zero allows top-level records only |

Existing container/file budgets continue to apply. Limits are not a process-wide
memory cap: inputs, decoded bytes, typed results and Python copies can coexist.
No external references are loaded. Missing streams raise `StreamNotFoundError`;
malformed fields raise `InvalidStreamError`; exceeded budgets raise
`InputLimitError`. See [reading](reading.md) for the separate limited `read()` API.

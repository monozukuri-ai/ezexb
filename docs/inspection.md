# Container and stream inspection

`ezexb.inspect(path, *, options=None)` and `inspect_bytes(bytes, *, options=None)`
return an immutable `Inspection` containing:

- `signature`, `classification`, `byte_length`, `cfb_version`, `sector_size`.
- `entries`: sorted non-root `EntryInfo(path, kind, size_bytes)` objects.
  `kind` is `storage` or `stream`; storage sizes are `None`.
- `streams` and `storages`: filtered views of `entries`.
- `total_stream_bytes`: sum of declared lengths of the listed streams.
- `matched_markers`, `missing_markers`, `diagnostics`, and `exb_confirmed=False`.

| Classification | Meaning |
| --- | --- |
| `unknown` | No recognized header; no container traversal |
| `legacy_caxa_candidate` | Legacy header only; stream access is unsupported |
| `cfb` | Parsed CFB directory without the required EXB marker layout |
| `cfb_exb_candidate` | Parsed CFB directory containing the expected marker kinds |

Malformed input starting with the CFB signature raises `InvalidContainerError`.
It is never downgraded to `unknown` or returned as a partial successful directory.

The current candidate rule requires root streams `*FileInfoStream`,
`*AppInfoStream`, `*SpaceStream`, `*StyleStream`, `*DBSelfData`,
`*DictionaryStream`, storage `*BlockStg`, and its stream `*BlockMgrStream`.
`*XrefInfoStream` is optional: its absence remains visible in `missing_markers`
but does not prevent candidate classification. Marker spelling is matched case-insensitively for ASCII.
This heuristic is not a complete EXB specification: a synthetic lookalike can
match, and an unrecognized EXB variant can fail to match. Neither a positive
match nor a successful stream read validates entities or the native save version.

## Raw access

```python
from ezexb import ParseOptions, raw

with raw.open("drawing.exb", options=ParseOptions(max_file_bytes=16 * 1024**2)) as file:
    info = file.info
    for entry in file.streams:
        payload = file.read_stream(entry.path)
        digest = file.stream_sha256(entry.path)
    header = file.read_stream("/*FileInfoStream", offset=0, length=32)
```

`raw.open_bytes(data)` accepts an immutable `bytes` object. Both open functions
create an owned snapshot; changes to the original file cannot affect subsequent
reads. `close()` releases it and is idempotent. Access after closing raises
`InvalidInputError`. Python operations on a shared container are serialized and
release the GIL during native work.

Selectors must exactly match a path from the listing, including case. Paths are
absolute within the CFB and use `/` even on Windows. Relative paths, `..`, `.`,
empty components, backslashes, NUL and drive prefixes are rejected. They are
never interpreted as external files. A missing stream raises `StreamNotFoundError`;
selecting a storage raises `InvalidInputError`.

`read_stream(path, *, offset=0, length=None)` returns `bytes`. Omitted length
means the remainder of the stream. Out-of-range requests are errors, rather
than silent truncation. An empty read at the stream end is allowed. A range
read does not certify bytes outside the range. `stream_sha256(path)` reads all
declared bytes in chunks and returns a lowercase hexadecimal digest.

## Resource limits

All `ParseOptions` fields are inclusive unsigned 64-bit integer limits.
Exceeding one raises `InputLimitError`; no partial result is returned.

| Field | Default | Applied to |
| --- | ---: | --- |
| `max_file_bytes` | 256 MiB | File snapshot or bytes input, before CFB parsing |
| `max_entries` | 10,000 | Non-root entries visited after the CFB directory is parsed |
| `max_stream_bytes` | 64 MiB | Each declared stream length, even for a listing |
| `max_total_stream_bytes` | 256 MiB | Sum of declared stream lengths |
| `max_storage_depth` | 64 | Number of path components, including the stream name |
| `max_container_read_bytes` | 64 MiB | Low-level bytes read during each open/read/hash operation |

The last budget also bounds reads during CFB allocation-table parsing, before
the entry-count check. Repeated reads and buffering count toward it, so it can
exceed the returned payload size. It resets for each operation. Entry limits
do not cap the CFB library's initial directory allocation; file and low-level
read budgets bound that earlier stage. These are operation limits, not a
process-wide memory quota. `read_stream` returns stored bytes without decompression.
Separate [stream analysis APIs](stream-analysis.md) add bounded decompression,
metadata, preview extraction and limited record-header inspection.

## CLI

```sh
ezexb inspect drawing.exb --json
ezexb streams drawing.exb --sha256 --json
ezexb stream drawing.exb '/*FileInfoStream' --hex --offset 0 --length 64
ezexb stream drawing.exb '/*SpaceStream' --output space.bin
```

The default hex range is at most 256 bytes from the requested offset. Explicit
`--length` controls the range; `--json` returns the hex string and range metadata.
`--output` extracts all remaining bytes unless a length is specified and refuses
to overwrite an existing destination. Listing paths are JSON-escaped so CFB
property-stream control characters are visible without affecting the terminal.

Limits also have CLI flags, such as `--max-entries 1000` and
`--max-container-read-bytes 8388608`. Success exits 0; parse/I/O/limit failures
exit 1 with diagnostics on stderr; invalid command-line syntax exits 2.

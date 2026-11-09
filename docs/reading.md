# Stored-entity reader

`ezexb.read(path)` and `ezexb.read_bytes(bytes)` return an immutable Python
`Document` for the experimental `exb-stored-planar-v1` profile. The Rust core
provides `read_file(path, options, strict)`, `read_bytes(data, options, strict)`,
and `Container::document(strict)`. No CAXA installation is required.

The stored-entity layouts were verified against native samples of these product
builds: `10.0.0.0000`, `11.7.0.5203`, `15.0.0.5681`, `16.4.0.13104`, and
`18.0.0.18159`. The decoders branch on the major number, so any other build whose
major is between 10 and 19 (field files carry builds such as `12.5.0.5302`,
`16.0.0.6227`, `17.1.0.15581`, `18.0.0.17670` or `19.0.0.20385`) is read with the
rules of its major (19 with those of 18, its nearest sample) and reports one
`unverified_product_build` diagnostic naming the verified neighbours; strict
reads therefore raise `IncompleteReadError` for them. A platform note CAXA
appends after the build (`15.2.0.5869 (x64)`) is ignored. These strings are not
an official EXB version specification. Builds outside that major range, or
without a leading dotted numeric form, raise `UnsupportedFeatureError`.
`ezexb.capabilities()` lists both the verified builds and the qualified major range.

```python
import ezexb

# Strict mode is the default. An incomplete read raises with a structured report.
try:
    document = ezexb.read("drawing.exb")
except ezexb.IncompleteReadError as error:
    for diagnostic in error.report.diagnostics:
        print(diagnostic.code, diagnostic.stream_path, diagnostic.span)

# Explicitly request the supported subset of a more complex drawing.
document = ezexb.read("drawing.exb", strict=False)
print(document.report.complete, document.report.unsupported_entity_count)
for space in document.spaces:
    print(space.id, space.name.value, space.block_id)
for entity in document.entities:
    print(entity.id, entity.block_id, entity.kind, entity.style.layer_name)
    if entity.kind == "Line":
        print(entity.geometry.start, entity.geometry.end)
```

The `raw.Container.document(strict=...)` method reads the same profile from an
owned snapshot. Existing `lines()` remains the lower-level 81-byte LINE API;
use `read()` to access the additional supported entity layouts.

## Profile boundary

| Object | Returned data | Boundary |
| --- | --- | --- |
| POINT | Stored XYZ position and attributes | 57/61-byte bodies; finite planar coordinates |
| LINE | Stored endpoints, 33/37-byte attributes | 81/85-byte bodies; planar finite coordinates |
| CIRCLE / ARC | Center, radius, basis; ARC sweep and endpoints | Positive radius, orthonormal XY basis; sweep in radians, basis preserves handedness |
| DegenerateArc | Stored center, zero radius/basis and full-turn sweep | Only the qualified zero-radius/zero-basis pattern; `degenerate_geometry` diagnostic, no inferred curve or point |
| POLYLINE | Vertices, bulges, widths, closure and normal | Version-dependent vertex encoding; zero discarded legacy Z and default normal |
| TEXT | Unicode, insertion, stored dimensions, rotation/scale, raw formatting; decoded font/spacing styles; save major >= 15 column layout kept raw (`column_*`) | Stored values remain distinct from native layout; a nonzero column count is reported as `unsupported_text_semantics` and not applied |
| INSERT / AttributeDefinition | Block reference, components/matrix, attached tag/value/TEXT; definition tag/prompt/default/TEXT | Visibility uncertainty is explicit; clipping and unusual embedded objects remain unsupported |
| HATCH / SOLID | Boundary loops, native curve parameters, pattern/dash data or gradient fields; a hatch's cached line points when the file stores them before the boundary (`line_cache_points`, field files of builds 16.0–18.0) | Nonempty association maps remain unsupported; no fill regeneration |
| Dimension | Linear, diameter, radius, leader, two-line angle, chamfer, coordinate list, half, large arc, origin and pitch; stored value/text/locations and typed cached presentation | Construction, overrides and associativity remain in `unresolved_semantics_span`; read remains incomplete |
| Symbol | Roughness, geometric tolerance, section marker and datum; selected captions, raw modes, leader/cut points and cached presentation | Build 10/15 layouts only; reference objects and remaining semantics retain spans; read remains incomplete |
| PartNumber | Style/table references, raw modes, reference-point data, leader points, scale, item indices and cached presentations | Build 10 only, empty override map; table/style resolution and number regeneration remain unsupported |
| Table | General/hole-chart table columns, sparse rows/cells, text/block contents, typed raw properties, placement, break data and hole-chart fields | Build 18 only; raw modes, styles, linked data and regeneration remain unresolved |
| BomTable | External data-table reference, placement, break/rewind points and typed header/row presentations | Build 10 only; semantic cell assignments and external table lookup are unresolved |
| View2D | Inherited block-reference fields, name, view type and absent/circular clip | Build 10 only, no attached attributes; raw clip reference bytes preserved; no view expansion or clip evaluation |
| NURBS / ELLIPSE | Stored control points/knots or axes/radii/angular fields; save major >= 15 adds `fit_point_count_raw`/`fit_tolerance` | Only an empty fit-point section (count -1) is qualified; no fit-point reconstruction or native tessellation |
| Layer / linetype / text style | Names, stored color/width, dash data, Western/CJK fonts and width/spacing factors | Font files are not embedded or loaded; advanced line styles remain unsupported |
| Space / block | Names, IDs, membership, paper scale, optional SOLID attachment | Xrefs and unsupported optional objects remain explicit |

`Document.entities` contains ordinary top-level records from all blocks, including
unreferenced definitions. Attached attributes, optional block SOLIDs and annotation
presentation records remain nested. They never become ordinary model entities.
`cached_record_count` counts the existing schema-3 display caches; inline dimension
presentation is available separately in `Dimension.presentation_records`.
`entities_in_block(id)` and `entities_in_space(id)` return direct members only.

`Symbol.data` is a typed `Roughness`, `GeometricTolerance`, `SectionMarker` or
`Datum` value. Only the fields before `unresolved_semantics_span` are interpreted;
for example, geometric-tolerance notes and leaders do not constitute a decoded
tolerance-frame definition. Inspect `presentation_records` for stored display
primitives and text. These records are caches, not regenerated annotation geometry.

`PartNumber.items` holds semantic item records; `presentation_records` holds their
separate display copies. Both can contain nested `PartItem` presentations. An item's
`index_raw` is a stored lookup index, not an inferred displayed number. Table and
style references retain their IDs and flags. Reference-point optional fields keep
their stored positions without assigning unverified geometric roles. Symbols and
part numbers emit `unsupported_symbol_semantics` and
`unsupported_part_number_semantics` respectively, so strict reading still raises.

`Table` types are available from `ezexb.tables`. `table_type` distinguishes `"Table"`
and `"HoleChart"`. Rows, cells, contents and properties retain their stored `index`;
these can be sparse and are not positions in a dense matrix. Text is accessible at
`cell.contents[n].value.text` when its `kind` is `"Text"`. Block content retains a
reference, scale and rotation. Numeric properties preserve their native tag and
`U16`/`U32`/`F64` value type without assigning unqualified meanings. Formatting,
style evaluation, linked data and regeneration emit `unsupported_table_semantics`.
`unresolved_semantics_span` covers the interspersed raw fields in `data_span`.

`BomTable` is available from `ezexb.bom`. Its `items` are typed `Header`/`Row`
display objects, with their own attributes and `presentation_records`. Saved TEXT
records preserve captions and positions, but their order or coordinates do not
establish semantic column/cell assignments. The external data-table reference
remains unresolved and emits `unsupported_bom_table_semantics`.

`View2D` and `ViewClip` are available from `ezexb.views`. `block_reference` retains
INSERT-shaped fields while `clip` separately retains its circular curve, raw
32-byte reference header and raw border flag. The header has no inferred coordinate
meaning. `unsupported_view_semantics` keeps strict reading incomplete even when all
stored fields decode. Block references are still checked for missing targets/cycles.
`unsupported_entity_count == 0` does not imply that these semantics are complete.

Coordinates in `read()` remain **stored block coordinates**. `document.units` is
`None`; paper scale is not a physical-unit conversion. Raw attribute flags and color
encodings are preserved. `entity.style` distinguishes ByLayer, ByBlock, explicit
values and symbolic device selections (width flags 0/1/7, colour and linetype
flag 7 keep the draw helper's current selection). ByBlock values require an INSERT instance.
A layer width flag of 0 means keeping the device width; flag 2 selects the stored float.

`document.text_style_context` is a separate optional display-context result with its
own `complete` and diagnostics. Text-style records remain in `styles.other_records`
for source tracing. An incomplete optional text-style decode does not redefine the
stored geometry profile's strictness. Rust `TextGeometry::presentation(styles)`
resolves font names, width factors and the -1 line-spacing sentinel without changing
saved values. Spacing factors are not asserted to be native glyph distances.

`report.complete=True` means all required geometry, context and references passed
**this stored-data profile**. It does not mean visual equivalence to CAXA or full
EXB support. Application properties, annotation style bodies and selected context
fields retain spans. Other streams, including drawing settings, dictionary,
block-manager metadata and previews, are listed in `uninterpreted_streams`.
The profile does not interpret their application-specific semantics.

## Legacy CaxaEbForWindows files

Drawings saved before the compound-document format are one byte stream that
starts with the counted string `CaxaEbForWindows`. `read()`, `read_bytes()` and
the Rust `read_file` / `read_bytes` recognise the signature and return a
`Document` whose `profile` is `exb-legacy-planar-v1`
(`capabilities()["legacy_profile"]`); `read_legacy_file` and `read_legacy_bytes`
read it directly. The `ezexb read` command accepts these files; the container,
stream, metadata, preview, world and SVG functions apply to compound documents
only.

The layouts of the container versions 1050, 1060 and 1070 were checked against
field files. Another version between 1000 and 1099 is read with the nearest of
those layouts and reports `unverified_legacy_version`; any other value raises
`UnsupportedFeatureError`. The version is in `file_info.stream_version` and
`file_info.product_build` is `CaxaEbForWindows <version>`.

A legacy document has one space and one block (`*Model_Space`) that holds every
entity. Blocks, dimensions, tables and hatches store their members as ordinary
records in drawing coordinates, so the reader returns them flattened: the
members are plain entities whose `owner_id` is the id of their group record,
and a block reference is not expanded because its geometry is already there.

| Record | Returned as | Notes |
| --- | --- | --- |
| Point, line | `Point`, `Line` | `type_id` `legacy:point`, `legacy:line` |
| Circle, arc | `Circle`, `Arc` | Centre, unit axes, radius; an arc with a negative sweep is returned with a mirrored second axis and a positive sweep |
| Polyline | `Polyline` | Straight segments |
| Ellipse, spline | `Polyline` (`legacy:ellipse_samples`, `legacy:spline_samples`) | The stored sample points; the curve definition is skipped (`legacy_curve_approximated`) |
| Text | `Text` | See below |
| Block, dimension, table, annotation and hatch groups | Their members | Dimension values, table data and hatch parameters are not returned |

Styles use the vocabulary of the compound-document profile, so both profiles can
be consumed alike:

- **Colors.** A record stores one byte: an index into the 48 basic colors of the
  Windows color dialog, then the sixteen custom colors the file stores. The
  reader returns the color value (`0x00BBGGRR`) in `Layer.color_raw` and in
  `style.color_raw`.
- **Line styles.** One byte selects a dash pattern and a width together. The
  program defines the first 24 styles itself and the file stores the others
  (name, a 16-bit pattern and a width). `style.linetype` holds the index, and
  `Document.linetypes` has an entry for the five standard styles (0 thick, 1
  thin, 2 dashed, 3 centre, 4 phantom, named `LEGACY_*`), for every stored
  style (ids from 24, with `pattern` and one dash unit per pattern bit) and a
  continuous placeholder `LEGACY_STYLE_<n>` for any other index in use
  (`legacy_line_style_unidentified`). The file stores no dash lengths for the
  standard styles; theirs are conventional values in millimetres. The width is
  the `wide` preset for style 0, `thin` for styles 1 to 4 and the stored width
  for a stored style.
- **ByLayer and ByBlock.** ByLayer values come from the layer table. A ByBlock
  value is taken from the group the entity was flattened out of and is returned
  with the source `by_block` and that value; outside a group it has no value.

Text maps onto the fields of the compound-document text record: `alignment_raw`
is the 3×3 code (0 top-left … 8 bottom-right; the file stores one more),
`mode_flags_raw`, `fit_mode` and `writing_direction` use the same bits,
`rotation` is in radians, and `insertion` is the alignment point (the point
inside the stored area when the text has one, `area_width_stored` and
`area_height_stored`). `style_id_raw` refers to `Document.text_styles`: the
text-style table of the 1060 and 1070 layouts (ids from 10000), or, in the
1050 layout, one style per distinct combination of the font names and factors
that each text record carries (ids from 1). The two width factors of a style are
taken in the order of the two font names.

| Diagnostic | Meaning |
| --- | --- |
| `legacy_record_tail_skipped` | Fields behind the members of dimension, table and annotation groups, behind curve points or behind 1050 text are not decoded; the geometry before them is returned |
| `legacy_curve_approximated` | Ellipses or splines are returned as their stored sample polylines |
| `legacy_records_skipped` | Bytes that are not a readable record were skipped up to the next record header |
| `legacy_trailing_data_skipped` | Data behind the last entity record (for example table rows) is not decoded |
| `legacy_line_style_unidentified` | A built-in line style other than the five standard ones is in use and is returned as a continuous line without a width |
| `unresolved_layer` | Entities reference a layer index outside the layer table |
| `legacy_color_unresolved` | A color byte lies outside the palette |
| `legacy_text_alignment_unknown` | A text stores an alignment code outside 1 to 9 and is aligned at the top-left corner |
| `legacy_layer_table_missing`, `legacy_style_tables_missing`, `legacy_entity_section_missing` | A table or the entity section was not found |
| `unverified_legacy_version` | The container version is not one of the checked ones |

Drawing settings, dimension styles and the paper are not read, and a legacy
document has no drawing scale: `Space.scale` is 1.

## Diagnostics and limits

Strict mode raises `IncompleteReadError` with a typed `.report` and the original
`.report_json`; Rust returns `Error::IncompleteRead(Box<ReadReport>)`.
Partial mode returns decoded entities and diagnostics for unsupported types,
unframed ranges, unsupported attributes, missing references and cyclic block
references. Every unsupported entity includes its ID, type GUID, stream and span.
Unknown nested record framing stops before the containing top-level record;
there is no GUID search to resume parsing through unknown bytes.

Resource failures remain errors in partial mode. `max_records` applies
cumulatively to records, properties, relations, compression blocks, context
entries, vertices and text items processed by one document read.
`max_total_decoded_bytes` defaults to 256 MiB and bounds cumulative decoded
allocations, including zlib padding, before allocation. Existing per-stream,
file, string and recursion limits continue to apply. Source spans always refer
to decoded stream bytes, not CFB file offsets.

```sh
ezexb read drawing.exb --json
ezexb read drawing.exb --partial --json
cargo run -p ezexb-core --example read -- drawing.exb --partial
```

The Python CLI exits with 0 for a returned document, 2 for an incomplete strict
read, and 1 for input or resource errors. A successful `--partial` command can
contain `report.complete=false`; automation must inspect that field.

## World coordinates and portable SVG

Choose a space ID from `document.spaces`; names do not imply model/paper semantics.

```python
scene = ezexb.read_world("drawing.exb", -7, strict=False)
print(scene.source_document_complete, scene.transforms_complete)
for instance in scene.instances:
    print(instance.entity_id, instance.insert_path, instance.transform.matrix)

preview = ezexb.svg_preview("drawing.exb", -7, strict=False)
with open("drawing.svg", "x", encoding="utf-8") as output:
    output.write(preview.svg)
print(preview.native_display_equivalent, preview.diagnostics)
```

The same operations are `raw.Container.world(space_id, strict=...)` and
`raw.Container.svg(...)`, including on `raw.open_bytes()` snapshots.
Rust provides `expand_world(&document, space_id, options)` and
`render_svg(&document, space_id, options)`, plus `Container::world_scene` and
`Container::svg_preview`.

```sh
ezexb world drawing.exb --space=-7 --partial --json
ezexb svg drawing.exb --space=-7 --partial --output drawing.svg --json
cargo run -p ezexb-core --example world -- drawing.exb -7
```

Matrices use row vectors: `x' = x*m[0] + y*m[4] + z*m[8] + m[12]`.
Child-to-root composition is `child * parent`. INSERT components must agree with
the saved matrix under the observed native tolerance/branches. No extra base-point
subtraction is added. Attached Attribute TEXT uses the outer container transform:
applying its own INSERT's matrix again would move it twice. Layer-zero inheritance
and ByBlock selection are resolved per instance, without modifying the definition.

POINT positions and LINE/CIRCLE/ARC and POLYLINE curves are transformed. Nonuniform/sheared circular
curves become `EllipticCurve` with radius-inclusive vectors:
`p(t) = center + cos(t)*axis_u + sin(t)*axis_v`. Other entities retain their source
index and instance transform; missing curve conversion has a diagnostic.
View2D referenced blocks are not expanded, even without a clip, and produce
`unsupported_world_clip`. DegenerateArc produces no world curve or inferred point.

`source_document_complete`, `transforms_complete`, `geometry_complete`,
`styles_complete`, and `render_complete` are independent. `WorldScene.complete`
combines transform/geometry/style completeness of the decoded subset; always check
`source_document_complete` too. `render_complete` remains false. A generated SVG
always reports `native_display_equivalent=false`.

SVG is a world-XY preview with neutral strokes and system font fallback. It renders
supported curves, POINT markers, fill boundaries, cached dimension/symbol/part-number/table/BOM presentation and supported plain
TEXT/attached text. It does not regenerate hatching, native line styles, gradients,
CAD stroke fonts, wrapping, rich formatting or every text alignment. Unsupported
text modes and attribute-definition display are omitted with diagnostics. Nominal
text size, width and line spacing produce an approximation, not native glyph metrics.
No external assets or font URLs are embedded. Output files are never overwritten.
POINT markers use neutral round caps rather than native point styles and count
toward `rendered_curve_count`. Part-number semantic item copies are not drawn a
second time alongside their cached presentations. Table layout and BOM placement are not
applied a second time to their saved display records; semantic cells are not rendered
again. BOM item visibility and nested presentation visibility are checked separately.

World visits, recursion and output items are bounded by `ParseOptions`; SVG byte
output is capped by `max_decoded_stream_bytes`. Partial mode never bypasses budgets.
The CLI retains the same strict-read exit codes. World/preview completeness must
be inspected independently even when the command succeeds.

## Compatibility limits

The reader is experimental. A supported build string does not guarantee that every
object in a drawing is supported. Inspect diagnostics for each file, even when
other drawings from the same application version have worked.

Stored TEXT fields can differ from the values displayed by CAD software after
layout or regeneration. Complete dimension semantics, physical units, native font
layout and pixel-equivalent rendering are not supported. No editing or writing
API is provided.

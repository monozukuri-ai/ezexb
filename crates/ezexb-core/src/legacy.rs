//! Reader for the `CaxaEbForWindows` container written by CAXA electronic
//! drawing board releases before the compound-document format (2005 and
//! earlier). Field names describe observations, not an official specification.
//!
//! A legacy file is one little-endian byte stream: a counted signature string,
//! an internal version, a property string, an index table, settings and style
//! tables, the entity section and a short trailer. Strings are counted GBK.
//! Every entity record has a header (type, layer, color, linetype, width and a
//! tag count), tagged fields (the tag byte written twice, then a value whose
//! type follows from the tag) and a type-specific body. Group records carry
//! their members inline in drawing coordinates, so the reader returns one flat
//! model space.
//!
//! Colors are one byte: an index into the 48 basic colors of the Windows color
//! dialog followed by the sixteen custom colors stored in the file. Line
//! styles are one byte as well and select a dash pattern and a width together;
//! the program defines the first 24 itself and the file stores the rest. Both
//! are returned in the vocabulary of the compound-document reader (a stored
//! color value, a linetype id and a width selection), so the two profiles can
//! be consumed alike.
//!
//! Text stores the fields of the compound-document text record under other
//! encodings: an alignment code (one more than the later 3×3 code), the same
//! mode flags, a rotation, an optional area and a style. The reader returns
//! the alignment point as the insertion, as the later format stores it.
//!
//! Block references are not expanded because their geometry is already
//! present, and the members of dimensions and tables are returned as plain
//! lines, arcs and text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    resolve_entity_style, AnnotationGeometry, Block, Diagnostic, Document, DrawingBlock, Entity,
    EntityAttributes, EntitySource, Error, FileInfo, Geometry, Layer, Linetype, ParseOptions,
    PolylineGeometry, PolylineVertex, PrimitiveGeometry, ReadReport, ResolvedStyle, SourceSpan,
    Space, SpaceContext, StyleContext, TextField, TextGeometry, TextStyle, TextStyleContext,
};

/// Profile name reported by documents read from the legacy container.
pub const LEGACY_PROFILE: &str = "exb-legacy-planar-v1";
/// Internal container versions whose layouts were checked against field files.
pub const LEGACY_CONTAINER_VERSIONS: &[u32] = &[1050, 1060, 1070];

pub(crate) const LEGACY_MAGIC: &[u8] = b"\x10CaxaEbForWindows";
const STREAM: &str = "/legacy";
const SPACE_ID: i32 = 1;
const MODEL_BLOCK_ID: i32 = 1;
const BY_LAYER: u8 = 0xFB;
const BY_BLOCK: u8 = 0xFC;
/// Bytes searched for the next record header after an undecoded tail.
const RESYNC_WINDOW: usize = 8192;
/// Longest undecoded tail accepted behind a group's members.
const GROUP_TAIL_LIMIT: usize = 2048;
const LAYER_SEARCH_WINDOW: usize = 16 * 1024;
const TRAILER: &[u8] = b"\x00\x00\x00\x00\xff\xff\x00\x00\x00\x00";
const TAU: f64 = std::f64::consts::TAU;
/// Line styles the program defines itself; the file stores the ones after them.
const BUILT_IN_LINE_STYLES: usize = 24;
/// Stored widths below this (the "0 mm" styles) select the thin preset.
const MIN_STORED_WIDTH: f64 = 0.01;
const WHITE: u32 = 0x00FF_FFFF;
/// The 48 basic colors of the Windows color dialog as `0x00BBGGRR`, row by
/// row. Color bytes 0 to 47 index this table and 48 to 63 the sixteen custom
/// colors stored in the file.
const BASIC_COLORS: [u32; 48] = [
    0x8080FF, 0x80FFFF, 0x80FF80, 0x80FF00, 0xFFFF80, 0xFF8000, 0xC080FF, 0xFF80FF, //
    0x0000FF, 0x00FFFF, 0x00FF80, 0x40FF00, 0xFFFF00, 0xC08000, 0xC08080, 0xFF00FF, //
    0x404080, 0x4080FF, 0x00FF00, 0x808000, 0x804000, 0xFF8080, 0x400080, 0x8000FF, //
    0x000080, 0x0080FF, 0x008000, 0x408000, 0xFF0000, 0xA00000, 0x800080, 0xFF0080, //
    0x000040, 0x004080, 0x004000, 0x404000, 0x800000, 0x400000, 0x400040, 0x800040, //
    0x000000, 0x008080, 0x408080, 0x808080, 0x808040, 0xC0C0C0, 0x400040, 0xFFFFFF, //
];
const CUSTOM_COLORS: usize = 16;
// Width and selection flags of the compound-document attribute layout.
const SELECT_BY_LAYER: u8 = 0;
const SELECT_BY_BLOCK: u8 = 1;
const SELECT_STORED: u8 = 2;
/// Leaves the consumer's default in place.
const SELECT_NONE: u8 = 7;
const WIDTH_STORED: u8 = 2;
const WIDTH_THIN: u8 = 3;
const WIDTH_THICK: u8 = 4;
const WIDTH_UNKNOWN: u8 = 7;

/// Read a legacy `CaxaEbForWindows` file. See [`read_legacy_bytes`].
pub fn read_legacy_file(
    path: impl AsRef<Path>,
    options: ParseOptions,
    strict: bool,
) -> Result<Document, Error> {
    let path = path.as_ref();
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput("read requires a regular file"));
    }
    options.check_size(metadata.len())?;
    read_legacy_bytes(&std::fs::read(path)?, options, strict)
}

/// Read the stored entities of a legacy `CaxaEbForWindows` stream.
///
/// Records whose trailing fields are not decoded (dimension text fields, table
/// rows, curve definitions) keep the geometry that precedes them and are
/// listed in the report; `strict` turns any such diagnostic into
/// [`Error::IncompleteRead`].
pub fn read_legacy_bytes(
    data: &[u8],
    options: ParseOptions,
    strict: bool,
) -> Result<Document, Error> {
    options.check_size(data.len() as u64)?;
    let prelude = parse_prelude(data, options)?;
    let mut report = ReadReport {
        profile: LEGACY_PROFILE.into(),
        complete: false,
        decoded_entity_count: 0,
        unsupported_entity_count: 0,
        top_level_record_count: 0,
        cached_record_count: 0,
        decoded_bytes: data.len() as u64,
        budget_used: 0,
        diagnostics: Vec::new(),
    };
    if !LEGACY_CONTAINER_VERSIONS.contains(&prelude.version) {
        report.diagnostics.push(diagnostic(
            "unverified_legacy_version",
            format!(
                "Legacy container version {} was not checked against a sample; it was read with \
                 the layout of the verified versions.",
                prelude.version
            ),
            Some(span(17, 4)),
            None,
        ));
    }
    if prelude.layers.is_empty() {
        report.diagnostics.push(diagnostic(
            "legacy_layer_table_missing",
            "The layer table and the style tables behind it were not found; entities keep their \
             layer indexes, and stored line styles and custom colors are unresolved.",
            None,
            None,
        ));
    } else if prelude.tables.is_none() {
        report.diagnostics.push(diagnostic(
            "legacy_style_tables_missing",
            "The line-style and color tables behind the layer table were not found; stored line \
             styles and custom colors are unresolved.",
            None,
            None,
        ));
    }

    let version = prelude.version;
    let search_from = prelude.search_from;
    let styles = Styles::new(prelude);
    let mut parser = Parser {
        data,
        options,
        version,
        styles: &styles,
        pos: 0,
        entities: Vec::new(),
        records: 0,
        next_id: 1,
        tails: BTreeMap::new(),
        approximated_curves: 0,
        resync_count: 0,
        resync_bytes: 0,
        first_failure: None,
        used_line_styles: BTreeSet::new(),
        unresolved_layers: BTreeMap::new(),
        unresolved_colors: styles.unresolved_layer_colors,
        record_styles: BTreeMap::new(),
        record_text_styles: Vec::new(),
        unknown_alignments: 0,
        end: data.len(),
        trailing_bytes: 0,
    };
    let top_level = match find_entity_section(data, search_from) {
        Some(start) => parser.read_section(start)?,
        None => {
            report.diagnostics.push(diagnostic(
                "legacy_entity_section_missing",
                "No entity section was found behind the style tables.",
                None,
                None,
            ));
            0
        }
    };
    parser.finish(&mut report);
    report.top_level_record_count = top_level;
    report.decoded_entity_count = parser.entities.len() as u64;
    report.budget_used = parser.records;
    report.complete = report.diagnostics.is_empty();
    if strict && !report.complete {
        return Err(Error::IncompleteRead(Box::new(report)));
    }

    let linetypes = linetype_table(&styles, &parser.used_line_styles);
    let mut text_styles = parser.record_text_styles;
    let entities = parser.entities;
    let text_style_notes = if styles.text_styles.is_some() {
        Vec::new()
    } else if version < 1060 {
        // The 1050 layout repeats the style values on every text record.
        vec![
            "The text-style table of this layout is not decoded; styles are gathered from \
              the text records."
                .to_owned(),
        ]
    } else {
        vec!["The text-style table was not found.".to_owned()]
    };
    let text_styles_complete = styles.text_styles.is_some() || version < 1060;
    text_styles.extend(styles.text_styles.unwrap_or_default());
    text_styles.sort_by_key(|style| style.id);
    let styles_complete = !styles.layers.is_empty() && styles.tables_found;
    let layers = styles.layers;
    let whole = span(0, data.len());
    let empty_text = || TextField {
        value: String::new(),
        span: span(0, 0),
    };
    Ok(Document {
        profile: LEGACY_PROFILE.into(),
        evidence: "legacy_container_grammar_consistent_across_field_files".into(),
        file_info: FileInfo {
            evidence: "legacy_container_header",
            stream_version: TextField {
                value: version.to_string(),
                span: span(17, 4),
            },
            document_type: TextField {
                value: "CaxaEbForWindows".into(),
                span: span(0, LEGACY_MAGIC.len()),
            },
            product_build: TextField {
                value: format!("CaxaEbForWindows {version}"),
                span: span(17, 4),
            },
            copyright: empty_text(),
            properties: Vec::new(),
            unknown_fields: Vec::new(),
        },
        coordinate_system: "stored_block_coordinates".into(),
        units: None,
        styles: StyleContext {
            budget_used: (layers.len() + linetypes.len()) as u64,
            evidence: "legacy_layer_and_line_style_tables",
            complete: styles_complete,
            layers,
            linetypes,
            other_records: Vec::new(),
            diagnostics: Vec::new(),
            unparsed_span: None,
        },
        text_style_context: TextStyleContext {
            complete: text_styles_complete,
            styles: text_styles,
            diagnostics: text_style_notes,
        },
        space_context: SpaceContext {
            budget_used: 1,
            evidence: "legacy_single_drawing_space",
            complete: true,
            spaces: vec![Space {
                id: SPACE_ID,
                owner: 0,
                span: whole.clone(),
                payload_span: whole.clone(),
                name: TextField {
                    value: "Model".into(),
                    span: span(0, 0),
                },
                scale: 1.0,
                block_id: MODEL_BLOCK_ID,
                block_status: 0,
                current_ucs_id: 0,
                unknown_fields: Vec::new(),
            }],
            other_records: Vec::new(),
            diagnostics: Vec::new(),
            unparsed_span: None,
        },
        blocks: vec![DrawingBlock {
            definition: Block {
                solid_attachment: None,
                solid_geometry: None,
                solid_attributes: None,
                complete: true,
                diagnostics: Vec::new(),
                current_ucs_id: 0,
                id: MODEL_BLOCK_ID,
                owner: 0,
                span: whole.clone(),
                payload_span: whole,
                name: TextField {
                    value: "*Model_Space".into(),
                    span: span(0, 0),
                },
                unknown_fields: Vec::new(),
            },
            stream_path: STREAM.into(),
            entity_indices: (0..entities.len()).collect(),
        }],
        entities,
        uninterpreted_streams: Vec::new(),
        report,
    })
}

fn span(offset: usize, length: usize) -> SourceSpan {
    SourceSpan {
        offset: offset as u64,
        length: length as u64,
    }
}

fn diagnostic(
    code: &str,
    message: impl Into<String>,
    span: Option<SourceSpan>,
    type_id: Option<String>,
) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        stream_path: Some(STREAM.into()),
        span,
        record_id: None,
        type_id,
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidStream(message.into())
}

fn decode_gbk(bytes: &[u8]) -> String {
    encoding_rs::GBK.decode(bytes).0.into_owned()
}

/// Run lengths of the sixteen pattern bits, least significant bit first and
/// starting with a drawn run. One bit is one unit. A pattern without gaps (or
/// without marks) is continuous.
fn pattern_dashes(pattern: u16) -> Vec<f64> {
    if pattern == 0 || pattern == u16::MAX {
        return Vec::new();
    }
    let bit = |index: u32| (pattern >> (index % 16)) & 1 == 1;
    let start = (0..16)
        .find(|index| bit(*index) && !bit(index + 15))
        .expect("a pattern with marks and gaps has a run start");
    let mut dashes = Vec::new();
    let mut drawn = true;
    let mut length = 0.0;
    for offset in 0..16 {
        if bit(start + offset) == drawn {
            length += 1.0;
        } else {
            dashes.push(length);
            drawn = !drawn;
            length = 1.0;
        }
    }
    dashes.push(length);
    dashes
}

/// The linetype table of a legacy document: the five standard line styles,
/// the stored ones, and a continuous placeholder for every other index in use.
///
/// The file stores neither names nor dash lengths for the built-in styles.
/// Their order (thick, thin, dashed, centre, phantom) follows from the default
/// layers; the dash lengths are conventional drawing values in millimetres.
fn linetype_table(styles: &Styles, in_use: &BTreeSet<u8>) -> Vec<Linetype> {
    let linetype =
        |id: usize, name: String, dashes: Vec<f64>, pattern: Option<u16>, at: SourceSpan| {
            let empty_text = || TextField {
                value: String::new(),
                span: span(0, 0),
            };
            Linetype {
                id: id as i32,
                owner: 0,
                span: at.clone(),
                payload_span: at.clone(),
                name: TextField {
                    value: name,
                    span: at,
                },
                description: empty_text(),
                alternate_name: empty_text(),
                scale: 1.0,
                dashes,
                pattern,
                advanced_present: false,
            }
        };
    let standard: [(&str, &[f64]); 5] = [
        ("LEGACY_THICK", &[]),
        ("LEGACY_THIN", &[]),
        ("LEGACY_DASHED", &[6.0, 2.0]),
        ("LEGACY_CENTER", &[12.0, 2.0, 2.0, 2.0]),
        ("LEGACY_PHANTOM", &[12.0, 2.0, 2.0, 2.0, 2.0, 2.0]),
    ];
    let mut table: Vec<Linetype> = standard
        .iter()
        .enumerate()
        .map(|(id, (name, dashes))| linetype(id, (*name).into(), dashes.to_vec(), None, span(0, 0)))
        .collect();
    for (position, style) in styles.line_styles.iter().enumerate() {
        let id = BUILT_IN_LINE_STYLES + position;
        let name = if style.name.is_empty() {
            format!("LEGACY_STYLE_{id}")
        } else {
            style.name.clone()
        };
        table.push(linetype(
            id,
            name,
            pattern_dashes(style.pattern),
            Some(style.pattern),
            style.span.clone(),
        ));
    }
    for index in in_use {
        if !styles.identifies(*index) {
            let id = usize::from(*index);
            table.push(linetype(
                id,
                format!("LEGACY_STYLE_{id}"),
                Vec::new(),
                None,
                span(0, 0),
            ));
        }
    }
    table.sort_by_key(|linetype| linetype.id);
    table
}

struct LegacyLayer {
    key: String,
    description: String,
    visible: bool,
    color: u8,
    linetype: u8,
    span: SourceSpan,
}

/// A line style stored behind the built-in ones.
struct LegacyLineStyle {
    name: String,
    /// Sixteen pattern bits, drawn from the least significant bit.
    pattern: u16,
    /// Line width in millimetres.
    width: f64,
    span: SourceSpan,
}

/// The tables behind the layer table.
struct StyleTables {
    line_styles: Vec<LegacyLineStyle>,
    custom_colors: Vec<u32>,
    /// The text styles of the 1060 and 1070 layouts.
    text_styles: Option<Vec<TextStyle>>,
    end: usize,
}

struct Prelude {
    version: u32,
    layers: Vec<LegacyLayer>,
    tables: Option<StyleTables>,
    /// Where the search for the entity section starts.
    search_from: usize,
}

/// Layers, stored line styles and custom colors in resolved form.
struct Styles {
    layers: Vec<Layer>,
    line_styles: Vec<LegacyLineStyle>,
    custom_colors: Vec<u32>,
    text_styles: Option<Vec<TextStyle>>,
    tables_found: bool,
    unresolved_layer_colors: u64,
}

impl Styles {
    fn new(prelude: Prelude) -> Self {
        let (line_styles, custom_colors, text_styles, tables_found) = match prelude.tables {
            Some(tables) => (
                tables.line_styles,
                tables.custom_colors,
                tables.text_styles,
                true,
            ),
            None => (Vec::new(), Vec::new(), None, false),
        };
        let mut styles = Self {
            layers: Vec::new(),
            line_styles,
            custom_colors,
            text_styles,
            tables_found,
            unresolved_layer_colors: 0,
        };
        let empty_text = || TextField {
            value: String::new(),
            span: span(0, 0),
        };
        let mut layers = Vec::with_capacity(prelude.layers.len());
        for (index, layer) in prelude.layers.into_iter().enumerate() {
            let color = styles.color(layer.color);
            if color.is_none() {
                styles.unresolved_layer_colors += 1;
            }
            let (width_flag, width) = styles.width(layer.linetype);
            layers.push(Layer {
                id: index as i32,
                owner: 0,
                span: layer.span.clone(),
                payload_span: layer.span.clone(),
                name: TextField {
                    value: if layer.description.is_empty() {
                        layer.key.clone()
                    } else {
                        layer.description
                    },
                    span: layer.span.clone(),
                },
                alternate_name: empty_text(),
                key_name: TextField {
                    value: layer.key,
                    span: layer.span,
                },
                description: empty_text(),
                color_raw: color.unwrap_or(WHITE),
                status_raw: u32::from(layer.visible),
                width,
                // An unidentified line style leaves the width to the consumer.
                width_flag: if width_flag == WIDTH_UNKNOWN {
                    0
                } else {
                    u32::from(width_flag)
                },
                linetype_id: i32::from(layer.linetype),
                linetype_status: 0,
                on: layer.visible,
                locked: false,
                printable: true,
                frozen: false,
                unknown_fields: Vec::new(),
            });
        }
        styles.layers = layers;
        styles
    }

    /// The stored color value (`0x00BBGGRR`) of a palette index.
    fn color(&self, index: u8) -> Option<u32> {
        let index = usize::from(index);
        BASIC_COLORS.get(index).copied().or_else(|| {
            self.custom_colors
                .get(index.checked_sub(BASIC_COLORS.len())?)
                .copied()
        })
    }

    fn stored_line_style(&self, index: u8) -> Option<&LegacyLineStyle> {
        self.line_styles
            .get(usize::from(index).checked_sub(BUILT_IN_LINE_STYLES)?)
    }

    /// Whether the reader knows the pattern and width of a line style.
    fn identifies(&self, index: u8) -> bool {
        index <= 4 || self.stored_line_style(index).is_some()
    }

    /// The width a line style selects, as a width flag of the
    /// compound-document attributes and a width in millimetres: the thick
    /// preset for style 0, the thin preset for styles 1 to 4, the stored width
    /// for a stored style.
    fn width(&self, index: u8) -> (u8, f64) {
        match index {
            0 => (WIDTH_THICK, 0.0),
            1..=4 => (WIDTH_THIN, 0.0),
            _ => match self.stored_line_style(index) {
                Some(style) if style.width >= MIN_STORED_WIDTH => (WIDTH_STORED, style.width),
                Some(_) => (WIDTH_THIN, 0.0),
                None => (WIDTH_UNKNOWN, 0.0),
            },
        }
    }
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

/// Signature, version, property string and index table, then the layer table.
fn parse_prelude(data: &[u8], options: ParseOptions) -> Result<Prelude, Error> {
    if !data.starts_with(LEGACY_MAGIC) {
        return Err(Error::InvalidInput(
            "the legacy CaxaEbForWindows signature is missing",
        ));
    }
    let truncated = || invalid("truncated legacy container header");
    let mut offset = LEGACY_MAGIC.len();
    let version = read_u32(data, offset).ok_or_else(truncated)?;
    if !(1000..1100).contains(&version) {
        return Err(Error::UnsupportedFeature(
            "reader for this legacy container version",
        ));
    }
    // version, 1, 0
    offset += 12;
    let mut length = usize::from(*data.get(offset).ok_or_else(truncated)?);
    offset += 1;
    if length == 0xFF {
        let bytes = data.get(offset..offset + 2).ok_or_else(truncated)?;
        length = usize::from(u16::from_le_bytes(bytes.try_into().expect("two bytes")));
        offset += 2;
    }
    ParseOptions::check_limit("max_string_bytes", length as u64, options.max_string_bytes)?;
    offset = offset
        .checked_add(length)
        .filter(|end| *end <= data.len())
        .ok_or_else(truncated)?;
    // 3, 0, 50, index count, 256, then the 16-bit index table
    let index_count = read_u32(data, offset + 12).ok_or_else(truncated)? as usize;
    let table_end = index_count
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(offset + 20))
        .filter(|end| *end <= data.len())
        .ok_or_else(|| invalid("truncated legacy index table"))?;

    let widths: [usize; 2] = if version >= 1070 { [6, 4] } else { [4, 6] };
    let search_end = data
        .len()
        .min(table_end.saturating_add(LAYER_SEARCH_WINDOW));
    for start in table_end..search_end {
        for width in widths {
            if let Some((layers, end)) = layers_at(data, start, width) {
                let tables = style_tables_at(data, end, version);
                let search_from = tables.as_ref().map_or(end, |tables| tables.end);
                return Ok(Prelude {
                    version,
                    layers,
                    tables,
                    search_from,
                });
            }
        }
    }
    Ok(Prelude {
        version,
        layers: Vec::new(),
        tables: None,
        search_from: table_end,
    })
}

/// The tables behind the layer table: one byte, the number of line styles
/// including the built-in ones, the stored line styles (name, a `u32`, the
/// 16-bit pattern, the width and a flag byte), one byte, sixteen custom colors
/// and 64 flag bytes.
fn style_tables_at(data: &[u8], start: usize, version: u32) -> Option<StyleTables> {
    let declared = read_u32(data, start.checked_add(1)?)? as usize;
    if !(BUILT_IN_LINE_STYLES..=255).contains(&declared) {
        return None;
    }
    let mut offset = start + 5;
    let mut line_styles = Vec::with_capacity(declared - BUILT_IN_LINE_STYLES);
    for _ in BUILT_IN_LINE_STYLES..declared {
        let entry = offset;
        let name = counted_name(data, &mut offset, 1)?;
        let fields = data.get(offset..offset + 15)?;
        offset += 15;
        let pattern = u16::from_le_bytes(fields[4..6].try_into().expect("two bytes"));
        let width = f64::from_le_bytes(fields[6..14].try_into().expect("eight bytes"));
        if !(0.0..100.0).contains(&width) || fields[14] > 1 {
            return None;
        }
        line_styles.push(LegacyLineStyle {
            name: decode_gbk(name),
            pattern,
            width,
            span: span(entry, offset - entry),
        });
    }
    offset += 1;
    let mut custom_colors = Vec::with_capacity(CUSTOM_COLORS);
    for _ in 0..CUSTOM_COLORS {
        let color = read_u32(data, offset)?;
        if color > WHITE {
            return None;
        }
        custom_colors.push(color);
        offset += 4;
    }
    data.get(offset..offset + 64)?;
    let mut end = offset + 64;
    let mut text_styles = None;
    if version >= 1060 {
        if let Some((styles, after)) = text_styles_at(data, end) {
            text_styles = Some(styles);
            end = after;
        }
    }
    Some(StyleTables {
        line_styles,
        custom_colors,
        text_styles,
        end,
    })
}

/// The text-style table of the 1060 and 1070 layouts: a count, the current
/// style id, and per style an id, a name, the Western and the CJK font name
/// and seven numbers (height, an unidentified one, character spacing, line
/// spacing, two width factors and another unidentified one).
///
/// The two width factors are equal in every sample; they are taken in the
/// order of the font names.
fn text_styles_at(data: &[u8], start: usize) -> Option<(Vec<TextStyle>, usize)> {
    let count = read_u32(data, start)?;
    if !(1..=4096).contains(&count) {
        return None;
    }
    let mut offset = start + 8;
    let mut styles = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let entry = offset;
        let id = i32::try_from(read_u32(data, offset)?).ok()?;
        offset += 4;
        let name = counted_name(data, &mut offset, 0)?;
        let western_font = counted_name(data, &mut offset, 0)?;
        let cjk_font = counted_name(data, &mut offset, 0)?;
        let mut numbers = [0.0; 7];
        for number in &mut numbers {
            let bytes = data.get(offset..offset + 8)?;
            *number = f64::from_le_bytes(bytes.try_into().expect("eight bytes"));
            if !number.is_finite() {
                return None;
            }
            offset += 8;
        }
        styles.push(text_style(
            id,
            decode_gbk(name),
            [decode_gbk(western_font), decode_gbk(cjk_font)],
            [numbers[4], numbers[5]],
            numbers[2],
            numbers[3],
            numbers[0],
            span(entry, offset - entry),
        ));
    }
    Some((styles, offset))
}

#[allow(clippy::too_many_arguments)]
fn text_style(
    id: i32,
    name: String,
    fonts: [String; 2],
    width_factors: [f64; 2],
    character_spacing_factor: f64,
    line_spacing_factor: f64,
    height: f64,
    at: SourceSpan,
) -> TextStyle {
    let field = |value: String| TextField {
        value,
        span: at.clone(),
    };
    let [western_font, cjk_font] = fonts;
    TextStyle {
        id,
        owner: 0,
        span: at.clone(),
        name: field(name),
        alternate_name: field(String::new()),
        western_font: field(western_font),
        cjk_font: field(cjk_font),
        western_width_factor: width_factors[0],
        cjk_width_factor: width_factors[1],
        character_spacing_factor,
        line_spacing_factor,
        height,
        oblique_angle: 0.0,
        indent: 0.0,
        shape_file_flag_raw: None,
        shape_file_path: None,
        extra_flags_raw: None,
    }
}

/// A layer table: `u32` count, then per layer a key string, a description
/// string and the flag bytes: `visible color linetype 1` in the 1050 and 1060
/// layouts, `visible color linetype 0 1 1` in the 1070 layout.
fn layers_at(data: &[u8], start: usize, width: usize) -> Option<(Vec<LegacyLayer>, usize)> {
    let count = read_u32(data, start)?;
    if !(1..=512).contains(&count) {
        return None;
    }
    let mut offset = start + 4;
    let mut layers = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let entry = offset;
        let key = counted_name(data, &mut offset, 1)?;
        let description = counted_name(data, &mut offset, 0)?;
        let flags = data.get(offset..offset + width)?;
        offset += width;
        let closed = match width {
            4 => flags[3] == 1,
            _ => flags[3..] == [0, 1, 1],
        };
        if flags[0] > 1 || !closed {
            return None;
        }
        layers.push(LegacyLayer {
            key: decode_gbk(key),
            description: decode_gbk(description),
            visible: flags[0] == 1,
            color: flags[1],
            linetype: flags[2],
            span: span(entry, offset - entry),
        });
    }
    Some((layers, offset))
}

fn counted_name<'a>(data: &'a [u8], offset: &mut usize, minimum: usize) -> Option<&'a [u8]> {
    let length = usize::from(*data.get(*offset)?);
    if length < minimum || length > 64 {
        return None;
    }
    let bytes = data.get(*offset + 1..*offset + 1 + length)?;
    if bytes.iter().any(|byte| *byte < 0x20) {
        return None;
    }
    *offset += 1 + length;
    Some(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// `type type layer color linetype 00 width(u16) tags(u32)`
    Doubled,
    /// `type layer color linetype 00 width(u16) tags(u32)`
    Plain,
    /// `type` and a 32-bit word below 256 (zero in most records) followed by
    /// the plain form.
    Wrapped,
}

#[derive(Clone, Copy, Debug)]
struct Header {
    form: Form,
    kind: u8,
    layer: u8,
    color: u8,
    linetype: u8,
    tags: u32,
    length: usize,
}

fn is_record_kind(kind: u8) -> bool {
    matches!(
        kind,
        0x01..=0x09 | 0x0E | 0x0F | 0x10 | 0x12 | 0x13 | 0x14 | 0x32 | 0x33 | 0x34
    )
}

fn plain_header(data: &[u8], offset: usize) -> Option<(u8, u8, u8, u32)> {
    let bytes = data.get(offset..offset + 11)?;
    if bytes[4] != 0 || bytes[5..7] != [0xFF, 0xFF] {
        return None;
    }
    let tags = u32::from_le_bytes(bytes[7..11].try_into().expect("four bytes"));
    (tags < 64).then_some((bytes[1], bytes[2], bytes[3], tags))
}

fn header_at(data: &[u8], offset: usize) -> Option<Header> {
    let kind = *data.get(offset)?;
    if !is_record_kind(kind) {
        return None;
    }
    if let Some(bytes) = data.get(offset..offset + 12) {
        if bytes[1] == kind && bytes[5] == 0 && bytes[6..8] == [0xFF, 0xFF] {
            let tags = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
            if tags < 64 {
                return Some(Header {
                    form: Form::Doubled,
                    kind,
                    layer: bytes[2],
                    color: bytes[3],
                    linetype: bytes[4],
                    tags,
                    length: 12,
                });
            }
        }
    }
    if let Some((layer, color, linetype, tags)) = plain_header(data, offset) {
        return Some(Header {
            form: Form::Plain,
            kind,
            layer,
            color,
            linetype,
            tags,
            length: 11,
        });
    }
    if data.get(offset + 2..offset + 5) == Some(&[0_u8; 3][..])
        && data.get(offset + 5) == Some(&kind)
    {
        if let Some((layer, color, linetype, tags)) = plain_header(data, offset + 5) {
            return Some(Header {
                form: Form::Wrapped,
                kind,
                layer,
                color,
                linetype,
                tags,
                length: 16,
            });
        }
    }
    None
}

fn next_header(data: &[u8], from: usize, window: usize) -> Option<usize> {
    let end = data.len().min(from.saturating_add(window));
    (from..end).find(|offset| header_at(data, *offset).is_some())
}

/// The entity section starts with a `u32` record count directly followed by a
/// record header in the doubled form.
fn find_entity_section(data: &[u8], from: usize) -> Option<usize> {
    (from..data.len().saturating_sub(16)).find(|offset| {
        read_u32(data, *offset).is_some_and(|count| (1..=1_000_000).contains(&count))
            && header_at(data, offset + 4).is_some_and(|header| header.form == Form::Doubled)
    })
}

/// A record-level failure: `Soft` loses the record (the reader resynchronises
/// on the next header), `Hard` ends the read.
enum Fail {
    Soft(String),
    Hard(Error),
}

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        Self::Hard(error)
    }
}

fn soft(message: impl Into<String>) -> Fail {
    Fail::Soft(message.into())
}

/// The tagged fields a text record uses. The 1050 layout stores what the
/// later layouts keep in the record body and in the text-style table.
#[derive(Default)]
struct Tags {
    /// Tag 0x15: the far corner of a text area in the 1050 layout.
    corner: Option<[f64; 2]>,
    /// Tag 0x29: the width of the text area; absent when the text has none.
    area_width: Option<f64>,
    /// Tags 0x2B and 0x2C: the width factors, in the order of the font names.
    width_factors: [Option<f64>; 2],
    /// Tag 0x2D: character spacing factor.
    character_spacing: Option<f64>,
    /// Tag 0x2E: line spacing factor.
    line_spacing: Option<f64>,
    /// Tag 0x32: rotation in radians.
    rotation: Option<f64>,
    /// Tag 0x50: alignment code.
    alignment: Option<u8>,
    /// Tag 0x51: mode flags.
    mode_flags: Option<u8>,
    /// Tag 0x64: style name.
    style_name: Option<String>,
    /// Tags 0x65 and 0x66: the font names.
    fonts: [Option<String>; 2],
}

struct Parser<'a> {
    data: &'a [u8],
    options: ParseOptions,
    version: u32,
    styles: &'a Styles,
    pos: usize,
    entities: Vec<Entity>,
    records: u64,
    next_id: i32,
    /// Record kind → (count, bytes, first offset) of undecoded tails.
    tails: BTreeMap<u8, (u64, u64, usize)>,
    approximated_curves: u64,
    resync_count: u64,
    resync_bytes: u64,
    first_failure: Option<(usize, String)>,
    /// Line styles the returned entities end up with.
    used_line_styles: BTreeSet<u8>,
    /// Layer index outside the layer table → number of entities.
    unresolved_layers: BTreeMap<u8, u64>,
    /// Color bytes outside the palette, layers included.
    unresolved_colors: u64,
    /// Style values stored on text records → the id of the gathered style.
    record_styles: BTreeMap<RecordStyleKey, i32>,
    record_text_styles: Vec<TextStyle>,
    /// Text records whose alignment code is outside the 3×3 grid.
    unknown_alignments: u64,
    /// Where the entity section ends.
    end: usize,
    /// Bytes behind the last record that are not records.
    trailing_bytes: u64,
}

/// Name, fonts and the bit patterns of the width factors and the two spacing
/// factors of a style stored on a text record.
type RecordStyleKey = (String, [String; 2], [u64; 4]);

impl<'a> Parser<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], Fail> {
        let end = self
            .pos
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| soft(format!("truncated field at offset {}", self.pos)))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, Fail> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Fail> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, Fail> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn f64(&mut self) -> Result<f64, Fail> {
        let value = f64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes"));
        if value.is_finite() {
            Ok(value)
        } else {
            Err(soft(format!(
                "non-finite number at offset {}",
                self.pos - 8
            )))
        }
    }

    fn point(&mut self) -> Result<[f64; 2], Fail> {
        Ok([self.f64()?, self.f64()?])
    }

    /// A counted GBK string: one length byte, or `FF` and a 16-bit length.
    fn string(&mut self) -> Result<(String, SourceSpan), Fail> {
        let start = self.pos;
        let mut length = usize::from(self.u8()?);
        if length == 0xFF {
            length = usize::from(self.u16()?);
        }
        ParseOptions::check_limit(
            "max_string_bytes",
            length as u64,
            self.options.max_string_bytes,
        )?;
        let bytes = self.take(length)?;
        Ok((decode_gbk(bytes), span(start, self.pos - start)))
    }

    fn charge(&mut self) -> Result<(), Fail> {
        self.records += 1;
        ParseOptions::check_limit("max_records", self.records, self.options.max_records)?;
        Ok(())
    }

    fn read_tags(&mut self, header: &Header) -> Result<Tags, Fail> {
        let mut tags = Tags::default();
        for _ in 0..header.tags {
            let pair = self.take(2)?;
            let tag = pair[0];
            if pair[1] != tag {
                return Err(soft(format!(
                    "field tag is not doubled at offset {}",
                    self.pos - 2
                )));
            }
            match tag {
                0x0A..=0x27 => {
                    let point = self.point()?;
                    if tag == 0x15 {
                        tags.corner = Some(point);
                    }
                }
                0x28..=0x3B => {
                    let value = self.f64()?;
                    match tag {
                        0x29 => tags.area_width = Some(value),
                        0x2B => tags.width_factors[0] = Some(value),
                        0x2C => tags.width_factors[1] = Some(value),
                        0x2D => tags.character_spacing = Some(value),
                        0x2E => tags.line_spacing = Some(value),
                        0x32 => tags.rotation = Some(value),
                        _ => {}
                    }
                }
                0x3C..=0x4F => {
                    self.u32()?;
                }
                0x50..=0x63 => {
                    let value = self.u8()?;
                    match tag {
                        0x50 => tags.alignment = Some(value),
                        0x51 => tags.mode_flags = Some(value),
                        _ => {}
                    }
                }
                0x00..=0x09 | 0x64..=0x9F => {
                    let (value, _) = self.string()?;
                    match tag {
                        0x64 => tags.style_name = Some(value),
                        0x65 => tags.fonts[0] = Some(value),
                        0x66 => tags.fonts[1] = Some(value),
                        _ => {}
                    }
                }
                0xA0..=0xB3 => {
                    self.u16()?;
                }
                _ => {
                    return Err(soft(format!(
                        "unknown field tag {tag:#04x} at offset {}",
                        self.pos - 2
                    )))
                }
            }
        }
        Ok(tags)
    }

    fn allocate_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Attributes and resolved style of a record header, in the vocabulary of
    /// the compound-document reader. ByBlock selections take the value of
    /// `parent`, the style of the enclosing group: groups are returned
    /// flattened, so the group is the only block context a member has.
    fn style_of(
        &mut self,
        start: usize,
        header: &Header,
        parent: Option<&ResolvedStyle>,
    ) -> (EntityAttributes, ResolvedStyle) {
        let (color_flag, color) = match header.color {
            BY_LAYER => (SELECT_BY_LAYER, 0),
            BY_BLOCK => (SELECT_BY_BLOCK, 0),
            index => match self.styles.color(index) {
                Some(color) => (SELECT_STORED, color),
                None => {
                    self.unresolved_colors += 1;
                    (SELECT_NONE, 0)
                }
            },
        };
        // One byte selects the dash pattern and the width together.
        let (linetype_flag, linetype_id, width_flag, width) = match header.linetype {
            BY_LAYER => (SELECT_BY_LAYER, 0, SELECT_BY_LAYER, 0.0),
            BY_BLOCK => (SELECT_BY_BLOCK, 0, SELECT_BY_BLOCK, 0.0),
            index => {
                let (width_flag, width) = self.styles.width(index);
                (SELECT_STORED, i32::from(index), width_flag, width)
            }
        };
        let attributes = EntityAttributes {
            span: span(start, header.length),
            layer_status_raw: 0,
            layer_id_raw: i32::from(header.layer),
            linetype_status_raw: 0,
            linetype_id_raw: linetype_id,
            color_raw: color,
            line_width_raw: width,
            linetype_flag_raw: linetype_flag,
            line_width_flag_raw: width_flag,
            color_flag_raw: color_flag,
            hide_flag_raw: 0,
            linetype_scale: 1.0,
            extension_raw: None,
        };
        let layer = self.styles.layers.get(usize::from(header.layer));
        let style = resolve_entity_style(&attributes, layer, parent).style;
        (attributes, style)
    }

    fn push(&mut self, record: &Record, name: &str, geometry: Geometry) {
        let layer = usize::from(record.header.layer);
        if layer >= self.styles.layers.len() && !self.styles.layers.is_empty() {
            *self
                .unresolved_layers
                .entry(record.header.layer)
                .or_insert(0) += 1;
        }
        if let Some(Ok(line_style)) = record.style.linetype.value.map(u8::try_from) {
            self.used_line_styles.insert(line_style);
        }
        self.entities.push(Entity {
            id: record.id,
            owner_id: record.owner,
            block_id: Some(MODEL_BLOCK_ID),
            type_id: format!("legacy:{name}"),
            object_schema: 0,
            source: EntitySource {
                stream_path: STREAM.into(),
                span: span(record.start, self.pos - record.start),
                payload_span: span(record.body, self.pos - record.body),
            },
            attributes: record.attributes.clone(),
            style: record.style.clone(),
            geometry,
            properties: Vec::new(),
        });
    }

    /// Skip an undecoded tail up to the next record header. The data rows
    /// behind a table (0x14) have no length limit and may close the section.
    fn skip_tail(&mut self, kind: u8, limit: usize) -> bool {
        let table = kind == 0x14;
        let window = if table { self.end } else { limit };
        let next = match next_header(self.data, self.pos, window) {
            Some(next) if next <= self.end => next,
            _ if table => self.end.max(self.pos),
            _ => return false,
        };
        let entry = self.tails.entry(kind).or_insert((0, 0, self.pos));
        entry.0 += 1;
        entry.1 += (next - self.pos) as u64;
        self.pos = next;
        true
    }

    fn at_header(&self) -> bool {
        header_at(self.data, self.pos).is_some()
    }

    fn read_record(
        &mut self,
        depth: u64,
        owner: i32,
        parent: Option<&ResolvedStyle>,
    ) -> Result<(), Fail> {
        if depth > self.options.max_record_depth.min(64) {
            return Err(Fail::Hard(Error::ResourceLimit {
                name: "max_record_depth",
                actual: depth,
                limit: self.options.max_record_depth.min(64),
            }));
        }
        let start = self.pos;
        let header = header_at(self.data, start)
            .ok_or_else(|| soft(format!("no record header at offset {start}")))?;
        self.charge()?;
        self.pos = start + header.length;
        let tags = self.read_tags(&header)?;
        let (attributes, style) = self.style_of(start, &header, parent);
        let record = Record {
            id: self.allocate_id(),
            owner,
            start,
            body: self.pos,
            header,
            attributes,
            style,
        };
        match header.kind {
            0x01 => {
                let point = self.point()?;
                self.push(
                    &record,
                    "point",
                    Geometry::Primitive(PrimitiveGeometry::Point {
                        point: [point[0], point[1], 0.0],
                    }),
                );
            }
            0x02 | 0x32 => {
                let start_point = self.point()?;
                let end_point = self.point()?;
                if header.kind == 0x32 {
                    self.u16()?;
                }
                self.push(
                    &record,
                    "line",
                    Geometry::Primitive(PrimitiveGeometry::Line {
                        start: [start_point[0], start_point[1], 0.0],
                        end: [end_point[0], end_point[1], 0.0],
                    }),
                );
            }
            0x03 | 0x04 | 0x33 => self.read_circular(&record)?,
            0x07 | 0x0F | 0x10 => self.read_curve(&record)?,
            0x06 => self.read_text(&record, &tags)?,
            0x09 => {
                // Hatch parameters: the pick point, a byte, the spacing, the
                // angle in degrees and a pattern word. The hatch lines are
                // ordinary line records, so nothing is drawn from this one.
                self.point()?;
                self.u8()?;
                self.f64()?;
                self.f64()?;
                self.u32()?;
            }
            _ => self.read_group(&record, depth)?,
        }
        Ok(())
    }

    /// Circle (0x03) and arc (0x04, 0x33): centre, the end points of two unit
    /// axes, the sweep angle and the radius.
    fn read_circular(&mut self, record: &Record) -> Result<(), Fail> {
        let center = self.point()?;
        let x_end = self.point()?;
        let y_end = self.point()?;
        let mut sweep = self.f64()?;
        let radius = self.f64()?;
        if record.header.kind == 0x33 {
            self.u16()?;
        }
        let x_axis = [x_end[0] - center[0], x_end[1] - center[1], 0.0];
        let mut y_axis = [y_end[0] - center[0], y_end[1] - center[1], 0.0];
        let center = [center[0], center[1], 0.0];
        if record.header.kind == 0x03 {
            self.push(
                record,
                "circle",
                Geometry::Primitive(PrimitiveGeometry::Circle {
                    center,
                    radius,
                    x_axis,
                    y_axis,
                }),
            );
            return Ok(());
        }
        if sweep < 0.0 {
            // The same curve with a positive parameter sweep.
            sweep = -sweep;
            y_axis = [-y_axis[0], -y_axis[1], 0.0];
        }
        let sweep = sweep.min(TAU);
        let (sin, cos) = sweep.sin_cos();
        let start = [
            center[0] + radius * x_axis[0],
            center[1] + radius * x_axis[1],
            0.0,
        ];
        let end = [
            center[0] + radius * (x_axis[0] * cos + y_axis[0] * sin),
            center[1] + radius * (x_axis[1] * cos + y_axis[1] * sin),
            0.0,
        ];
        self.push(
            record,
            "arc",
            Geometry::Primitive(PrimitiveGeometry::Arc {
                center,
                radius,
                x_axis,
                y_axis,
                sweep_angle: sweep,
                start,
                end,
            }),
        );
        Ok(())
    }

    /// Polyline (0x07), ellipse (0x0F) and spline (0x10): a segment count and
    /// that many plus one points. For ellipses and splines the points are the
    /// stored sample polyline; the curve definition behind them is skipped.
    fn read_curve(&mut self, record: &Record) -> Result<(), Fail> {
        let kind = record.header.kind;
        let segments = self.u32()? as usize;
        let remaining = self.data.len() - self.pos;
        if segments >= remaining / 16 {
            return Err(soft(format!(
                "implausible vertex count {segments} at offset {}",
                self.pos - 4
            )));
        }
        self.records += segments as u64;
        ParseOptions::check_limit("max_records", self.records, self.options.max_records)?;
        let mut vertices = Vec::with_capacity(segments + 1);
        for _ in 0..=segments {
            let at = self.pos;
            let point = self.point()?;
            vertices.push(PolylineVertex {
                point,
                legacy_z: None,
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                span: span(at, 16),
            });
        }
        let points_end = self.pos;
        let name = match kind {
            0x07 => "polyline",
            0x0F => "ellipse_samples",
            _ => "spline_samples",
        };
        if kind != 0x07 {
            if !self.at_header() && !self.skip_tail(kind, RESYNC_WINDOW) {
                return Err(soft(format!(
                    "no record follows the curve at offset {}",
                    record.start
                )));
            }
            self.approximated_curves += 1;
        }
        let end = self.pos;
        // The entity spans its points; an undecoded definition stays outside.
        self.pos = points_end;
        self.push(
            record,
            name,
            Geometry::Annotation(AnnotationGeometry::Polyline(PolylineGeometry {
                vertices,
                closed: false,
                closed_raw: 0,
                normal: None,
                span: span(record.body, points_end - record.body),
            })),
        );
        self.pos = end;
        Ok(())
    }

    /// Text (0x06). The 1060 and 1070 layouts store a point, the far corner
    /// of the text area, the height, the contents, a style id, the rotation,
    /// a zero word, the alignment code and the mode flags. The 1050 layout
    /// stores a point, the height and the contents, and keeps the other
    /// values in tags; fields behind the contents are skipped.
    ///
    /// With an area (tag 0x29) the point is its top-left corner and the far
    /// corner its bottom-right one. Without an area the point is the
    /// alignment point itself and the second point of the record is unused.
    fn read_text(&mut self, record: &Record, tags: &Tags) -> Result<(), Fail> {
        let boxed = self.version >= 1060;
        let point = self.point()?;
        let far_corner = if boxed {
            Some(self.point()?)
        } else {
            tags.corner
        };
        let height = self.f64()?;
        let (contents, contents_span) = self.string()?;
        let mut rotation = tags.rotation.unwrap_or(0.0);
        let mut alignment = tags.alignment;
        let mut mode_flags = tags.mode_flags.unwrap_or(0);
        let mut style_id = -1;
        if boxed {
            style_id = self.u32()? as i32;
            rotation = self.f64()?;
            self.u32()?;
            alignment = Some(self.u8()?);
            mode_flags = self.u8()?;
        } else if let Some(id) = self.record_style(tags, record.start) {
            style_id = id;
        }
        let fields_end = self.pos;
        let followed = boxed || self.at_header() || self.skip_tail(0x06, RESYNC_WINDOW);
        let end = self.pos;

        // 1 top-left … 5 centre … 9 bottom-right; the later format stores one less.
        let alignment_raw = match alignment {
            Some(code @ 1..=9) => u32::from(code - 1),
            Some(_) => {
                self.unknown_alignments += 1;
                0
            }
            None => 0,
        };
        let area = tags
            .area_width
            .zip(far_corner)
            .filter(|(width, _)| *width > 0.0);
        let (area_width, area_height) = area.map_or((0.0, 0.0), |(width, corner)| {
            (width, (point[1] - corner[1]).abs())
        });
        // The alignment point inside the area; rotation turns the text about it.
        let insertion = [
            point[0] + area_width * f64::from(alignment_raw % 3) / 2.0,
            point[1] - area_height * f64::from(alignment_raw / 3) / 2.0,
            0.0,
        ];
        self.pos = fields_end;
        self.push(
            record,
            "text",
            Geometry::Annotation(AnnotationGeometry::Text(TextGeometry {
                contents,
                contents_span,
                insertion,
                text_height_stored: height,
                area_height_stored: area_height,
                area_width_stored: area_width,
                rotation,
                drawing_scale: 1.0,
                mode_flags_raw: mode_flags,
                alignment_raw,
                // The same bits as in the compound-document text record.
                writing_direction: (mode_flags >> 3) & 1,
                fit_mode: if mode_flags & 0x40 != 0 {
                    2
                } else {
                    (mode_flags >> 5) & 1
                },
                style_status_raw: 0,
                style_id_raw: style_id,
                legacy_format_raw: None,
                line_spacing_raw: 0.0,
                line_spacing_style_raw: 0,
                format_type_id: None,
                column_flags_raw: None,
                column_width: None,
                column_count: None,
                column_entries_raw: None,
                formatting_span: span(fields_end, 0),
                span: span(record.body, fields_end - record.body),
            })),
        );
        self.pos = end;
        if followed {
            Ok(())
        } else {
            Err(soft(format!(
                "no record follows the text at offset {}",
                record.start
            )))
        }
    }

    /// The style stored on a text record of the 1050 layout, gathered into
    /// one style per distinct combination. Their ids count from 1; the ids of
    /// the text-style table start at 10000.
    fn record_style(&mut self, tags: &Tags, at: usize) -> Option<i32> {
        tags.fonts[0].as_ref()?;
        let fonts = [
            tags.fonts[0].clone().unwrap_or_default(),
            tags.fonts[1].clone().unwrap_or_default(),
        ];
        let numbers = [
            tags.width_factors[0].unwrap_or(0.0),
            tags.width_factors[1].unwrap_or(0.0),
            tags.character_spacing.unwrap_or(0.0),
            tags.line_spacing.unwrap_or(0.0),
        ];
        let name = tags.style_name.clone().unwrap_or_default();
        let key = (name.clone(), fonts.clone(), numbers.map(f64::to_bits));
        if let Some(id) = self.record_styles.get(&key) {
            return Some(*id);
        }
        let id = i32::try_from(self.record_text_styles.len() + 1).ok()?;
        let name = if name.is_empty() {
            format!("LEGACY_TEXT_STYLE_{id}")
        } else {
            name
        };
        self.record_text_styles.push(text_style(
            id,
            name,
            fonts,
            [numbers[0], numbers[1]],
            numbers[2],
            numbers[3],
            0.0,
            span(at, 0),
        ));
        self.record_styles.insert(key, id);
        Some(id)
    }

    /// Groups: a member count and the members. A plain-form 0x0E record
    /// without members names a block (five numbers, a name, a property string
    /// and a byte); one may follow a 0x08 group outside its member count. A
    /// 0x12 group without tags ends with a base point unless the next record
    /// follows its members directly, and with no members also names a
    /// definition. Dimension (0x05 plain or wrapped), annotation (0x13) and
    /// table (0x14, 0x34) groups end with fields that are skipped.
    fn read_group(&mut self, record: &Record, depth: u64) -> Result<(), Fail> {
        let header = record.header;
        let count = self.u32()?;
        if count as usize > self.data.len() - self.pos {
            return Err(soft(format!(
                "implausible member count {count} at offset {}",
                self.pos - 4
            )));
        }
        for _ in 0..count {
            self.read_record(depth + 1, record.id, Some(&record.style))?;
        }
        if header.kind == 0x12 && header.tags == 0 && !self.at_header() {
            self.point()?;
            if count == 0 {
                self.point()?;
                self.f64()?;
                self.read_reference_name()?;
            }
        }
        if header.kind == 0x0E && header.form == Form::Plain && count == 0 {
            for _ in 0..5 {
                self.f64()?;
            }
            self.read_reference_name()?;
        }
        if header.kind == 0x08 {
            let follower = header_at(self.data, self.pos).filter(|next| {
                next.kind == 0x0E
                    && next.form == Form::Plain
                    && next.tags == 0
                    && read_u32(self.data, self.pos + next.length) == Some(0)
            });
            if follower.is_some() {
                self.read_record(depth + 1, record.id, Some(&record.style))?;
            }
        }
        let tailed = (header.kind == 0x05 && header.form != Form::Doubled)
            || matches!(header.kind, 0x13 | 0x14 | 0x34)
            || (header.kind == 0x12 && header.tags == 0);
        if tailed && self.pos + 12 < self.end && !self.at_header() {
            // Without a header inside the limit the next read fails and resynchronises.
            self.skip_tail(header.kind, GROUP_TAIL_LIMIT);
        }
        Ok(())
    }

    fn read_reference_name(&mut self) -> Result<(), Fail> {
        self.string()?;
        self.string()?;
        self.u8()?;
        Ok(())
    }

    /// Read records from the section start to the trailer. The declared count
    /// is not a stop condition: some drawings store more top-level records
    /// than it says.
    fn read_section(&mut self, start: usize) -> Result<u64, Error> {
        let data = self.data;
        let tail_from = data.len().saturating_sub(96);
        let end_limit = data[tail_from..]
            .windows(TRAILER.len())
            .rposition(|window| window == TRAILER)
            .map(|index| tail_from + index)
            .filter(|tail| *tail > start)
            .unwrap_or(data.len());
        self.end = end_limit;
        self.pos = start + 4;
        let mut top_level = 0_u64;
        while self.pos + 11 < end_limit {
            let record_start = self.pos;
            match self.read_record(0, 0, None) {
                Ok(()) => top_level += 1,
                Err(Fail::Hard(error)) => return Err(error),
                Err(Fail::Soft(reason)) => {
                    let failed_at = self.pos;
                    let next = next_header(data, failed_at + 1, RESYNC_WINDOW)
                        .filter(|next| *next < end_limit);
                    if next.is_none() && header_at(data, record_start).is_none() {
                        // Not a record and none follows: data behind the last record.
                        self.trailing_bytes = (end_limit - record_start.min(end_limit)) as u64;
                        self.pos = end_limit;
                        break;
                    }
                    if self.first_failure.is_none() {
                        self.first_failure = Some((failed_at, reason));
                    }
                    let Some(next) = next else {
                        self.resync_count += 1;
                        self.resync_bytes += (end_limit - failed_at.min(end_limit)) as u64;
                        self.pos = end_limit;
                        break;
                    };
                    self.resync_count += 1;
                    self.resync_bytes += (next - failed_at) as u64;
                    self.pos = next;
                    top_level += 1;
                }
            }
        }
        Ok(top_level)
    }

    fn finish(&self, report: &mut ReadReport) {
        const NAMES: &[(u8, &str)] = &[
            (0x05, "dimension"),
            (0x06, "text"),
            (0x0F, "ellipse"),
            (0x10, "spline"),
            (0x12, "group"),
            (0x13, "annotation"),
            (0x14, "table"),
            (0x34, "composite_curve"),
        ];
        let name = |kind: u8| {
            NAMES
                .iter()
                .find(|(candidate, _)| *candidate == kind)
                .map_or_else(
                    || format!("legacy:{kind:#04x}"),
                    |(_, name)| format!("legacy:{name}"),
                )
        };
        for (kind, (count, bytes, first)) in &self.tails {
            report.diagnostics.push(diagnostic(
                "legacy_record_tail_skipped",
                format!(
                    "{count} {} record(s) end with fields that are not decoded ({bytes} bytes); \
                     the geometry stored before them is kept.",
                    name(*kind).trim_start_matches("legacy:")
                ),
                Some(span(*first, 0)),
                Some(name(*kind)),
            ));
        }
        if self.approximated_curves > 0 {
            report.diagnostics.push(diagnostic(
                "legacy_curve_approximated",
                format!(
                    "{} ellipse or spline record(s) are returned as their stored sample \
                     polylines; the curve definitions are not decoded.",
                    self.approximated_curves
                ),
                None,
                None,
            ));
        }
        let unidentified: Vec<String> = self
            .used_line_styles
            .iter()
            .filter(|index| !self.styles.identifies(**index))
            .map(|index| index.to_string())
            .collect();
        if !unidentified.is_empty() {
            report.diagnostics.push(diagnostic(
                "legacy_line_style_unidentified",
                format!(
                    "Line style index(es) {} are in use but are neither one of the five standard \
                     styles nor stored in the file; they are returned as continuous lines \
                     without a width.",
                    unidentified.join(", ")
                ),
                None,
                None,
            ));
        }
        if self.unknown_alignments > 0 {
            report.diagnostics.push(diagnostic(
                "legacy_text_alignment_unknown",
                format!(
                    "{} text record(s) store an alignment code outside 1 to 9; they are aligned \
                     at the top-left corner.",
                    self.unknown_alignments
                ),
                None,
                None,
            ));
        }
        if !self.unresolved_layers.is_empty() {
            let count: u64 = self.unresolved_layers.values().sum();
            let indexes: Vec<String> = self
                .unresolved_layers
                .keys()
                .map(|index| index.to_string())
                .collect();
            report.diagnostics.push(diagnostic(
                "unresolved_layer",
                format!(
                    "{count} entity record(s) reference layer index(es) {} outside the layer \
                     table.",
                    indexes.join(", ")
                ),
                None,
                None,
            ));
        }
        if self.unresolved_colors > 0 {
            report.diagnostics.push(diagnostic(
                "legacy_color_unresolved",
                format!(
                    "{} color byte(s) lie outside the palette (48 basic and {} custom colors) \
                     and carry no color.",
                    self.unresolved_colors,
                    self.styles.custom_colors.len()
                ),
                None,
                None,
            ));
        }
        if self.trailing_bytes > 0 {
            report.diagnostics.push(diagnostic(
                "legacy_trailing_data_skipped",
                format!(
                    "{} bytes behind the last entity record are not records and are not decoded.",
                    self.trailing_bytes
                ),
                Some(span(
                    self.end.saturating_sub(self.trailing_bytes as usize),
                    self.trailing_bytes as usize,
                )),
                None,
            ));
        }
        if self.resync_count > 0 {
            let (offset, reason) = self.first_failure.clone().unwrap_or((0, String::new()));
            report.diagnostics.push(diagnostic(
                "legacy_records_skipped",
                format!(
                    "{} span(s) ({} bytes) could not be decoded and were skipped up to the next \
                     record header; first: {reason}.",
                    self.resync_count, self.resync_bytes
                ),
                Some(span(offset, 0)),
                None,
            ));
        }
    }
}

struct Record {
    id: i32,
    owner: i32,
    start: usize,
    body: usize,
    header: Header,
    attributes: EntityAttributes,
    style: ResolvedStyle,
}

#[cfg(test)]
mod pattern_tests {
    use super::pattern_dashes;

    #[test]
    fn pattern_bits_become_run_lengths() {
        assert!(pattern_dashes(0xFFFF).is_empty());
        assert!(pattern_dashes(0).is_empty());
        assert_eq!(pattern_dashes(0x5FFF), [13.0, 1.0, 1.0, 1.0]);
        assert_eq!(pattern_dashes(0x57FF), [11.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(pattern_dashes(0x00FF), [8.0, 8.0]);
        // A pattern that begins with a gap starts at its first drawn run.
        assert_eq!(pattern_dashes(0xFF00), [8.0, 8.0]);
        assert_eq!(pattern_dashes(0x5555), [1.0; 16]);
    }
}

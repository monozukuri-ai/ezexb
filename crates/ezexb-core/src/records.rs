//! Bounded framing of an observed block-record prefix, not drawing entity decoding.
use serde::Serialize;

use crate::metadata::Reader;
use crate::{Error, ParseOptions, SourceSpan, TextField};

const PREFIX: &[u8] = &[8, 0, 0, 0, b'0', 0, b'.', 0, b'0', 0, 0, 0];
const LINE_GUID: [u8; 16] = [
    0x2d, 0xf7, 0xec, 0x17, 0x46, 0xc0, 0x9a, 0x4e, 0x84, 0x16, 0xfd, 0x82, 0x20, 0xfd, 0x33, 0xb5,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordHeader {
    /// Windows GUID byte order, rendered as a canonical UUID string.
    pub type_id: String,
    /// A type identity observation, not qualification of its geometry or properties.
    pub class_name: Option<String>,
    pub id_raw: i32,
    pub owner_raw: i32,
    /// Original adjacent bytes: object schema in the low byte and property
    /// presence in the high byte. Retained for compatibility, not one version.
    pub schema: u16,
    pub object_schema: u8,
    pub properties_present: bool,
    pub property_groups: Vec<RecordPropertyGroup>,
    /// Positions refer to the decompressed logical stream, never the CFB file.
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    /// Schema-3 post-body structure; nested display records are not top-level entities.
    pub extension: Option<RecordExtension>,
}

/// A framed property group. The GUID's application-specific meaning is unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordPropertyGroup {
    pub type_id: String,
    pub span: SourceSpan,
    pub values: Vec<RecordPropertyValue>,
}

/// A property value whose bytes are available through the decoded stream.
/// Tags establish framing only; numeric types and CAD semantics are not inferred.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordPropertyValue {
    pub tag: u8,
    pub key: Option<TextField>,
    /// Present only for tag 12, which has an object GUID and opaque blob.
    pub object_type_id: Option<String>,
    pub span: SourceSpan,
    /// Tag 7 includes its u32 length and UTF-16 terminator. Tag 12 includes
    /// only the blob bytes; other tags contain the fixed-width value bytes.
    pub value_span: SourceSpan,
}

/// Explicit experimental schema-3 framing profiles. A matching build string is
/// a corpus gate, not an official file-format version declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordProfile {
    Schema3Attributes33,
    Schema3Attributes37,
}

impl RecordProfile {
    pub fn from_product_build(build: &str) -> Option<Self> {
        match build {
            "10.0.0.0000" | "15.0.0.5681" => Some(Self::Schema3Attributes33),
            "18.0.0.18159" => Some(Self::Schema3Attributes37),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schema3Attributes33 => "schema3-attributes33-experimental",
            Self::Schema3Attributes37 => "schema3-attributes37-experimental",
        }
    }

    fn attribute_width(self) -> usize {
        match self {
            Self::Schema3Attributes33 => 33,
            Self::Schema3Attributes37 => 37,
        }
    }
}

/// A known relation grammar. Its contents retain their original stream span;
/// referenced identifiers are not resolved to drawing objects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordRelation {
    pub type_id: String,
    pub class_name: String,
    pub span: SourceSpan,
}

/// Framed display records, kept separate from the stream's top-level records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordDisplayCache {
    pub span: SourceSpan,
    pub presence_span: SourceSpan,
    pub present: bool,
    pub type_id: Option<String>,
    pub records: Vec<RecordHeader>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordExtension {
    pub span: SourceSpan,
    pub relation_section_span: SourceSpan,
    pub relation_type_id: String,
    pub relations: Vec<RecordRelation>,
    pub header_span: SourceSpan,
    pub header_u16_raw: u16,
    pub header_u32_raw: [u32; 2],
    /// Repeated opaque attributes, required to equal the initial body prefix.
    pub attributes_span: SourceSpan,
    pub display_cache: RecordDisplayCache,
    /// The separate trailing-object presence byte. Only absence is supported.
    pub tail_span: SourceSpan,
    pub tail_raw: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordInspection {
    pub evidence: String,
    pub profile: Option<String>,
    pub offset_space: String,
    pub stream_version: String,
    pub records: Vec<RecordHeader>,
    pub framing_complete: bool,
    pub unparsed_span: Option<SourceSpan>,
    pub stop_reason: Option<String>,
    pub geometry_decoded: bool,
}

fn span(start: usize, end: usize) -> SourceSpan {
    SourceSpan {
        offset: start as u64,
        length: (end - start) as u64,
    }
}

fn consume_budget(used: &mut u64, count: u64, options: ParseOptions) -> Result<(), Error> {
    *used = used.saturating_add(count);
    ParseOptions::check_limit("max_records", *used, options.max_records)
}

fn property_text(reader: &mut Reader<'_>) -> Result<TextField, Error> {
    let start = reader.offset;
    if reader.u32()? == 0 {
        return Ok(TextField {
            value: String::new(),
            span: span(start, reader.offset),
        });
    }
    reader.offset = start;
    reader.text()
}

pub(crate) enum FrameError {
    Invalid(Error),
    Unsupported(String),
}

/// Frame an inline native presentation record inside an enclosing body.
/// The original stream origin is retained and the enclosing end is a hard bound.
pub(crate) fn embedded_record(
    data: &[u8],
    start: usize,
    end: usize,
    options: ParseOptions,
    budget: &mut u64,
    profile: RecordProfile,
    depth: u64,
) -> Result<RecordHeader, FrameError> {
    if start > end || end > data.len() {
        return Err(Error::InvalidStream("invalid embedded record bounds".into()).into());
    }
    parse_record(&data[..end], start, options, budget, Some(profile), depth)
}

impl From<Error> for FrameError {
    fn from(error: Error) -> Self {
        Self::Invalid(error)
    }
}

/// Recursive parsing has a fixed stack-safety ceiling even if the caller raises
/// the configurable budget. Top-level records have depth zero.
const MAX_SAFE_RECORD_DEPTH: u64 = 64;
const RELATION_TYPE: &str = "0fbb54e7-fc10-4b09-9499-924097c3bf94";
const DISPLAY_TYPE: &str = "0045f336-69f0-4e1a-b117-6f062840b2ab";

fn read_guid(reader: &mut Reader<'_>) -> Result<String, Error> {
    Ok(uuid::Uuid::from_bytes_le(reader.take(16)?.try_into().unwrap()).to_string())
}

fn presence(reader: &mut Reader<'_>, context: &str) -> Result<bool, FrameError> {
    let offset = reader.offset;
    match reader.take(1)?[0] {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(FrameError::Unsupported(format!(
            "Unsupported {context} presence {value} at decoded offset {offset}; no resynchronization attempted."
        ))),
    }
}

fn bounded_count(
    reader: &mut Reader<'_>,
    budget: &mut u64,
    minimum_bytes: usize,
) -> Result<u64, Error> {
    let count = reader.u32()? as u64;
    consume_budget(budget, count, reader.options)?;
    if count > (reader.data.len() - reader.offset) as u64 / minimum_bytes as u64 {
        return Err(Error::InvalidStream(format!(
            "array count at decoded offset {} exceeds remaining stream bytes",
            reader.offset - 4
        )));
    }
    Ok(count)
}

fn relation(reader: &mut Reader<'_>, budget: &mut u64) -> Result<RecordRelation, FrameError> {
    let start = reader.offset;
    let type_id = read_guid(reader)?;
    let class_name = match type_id.as_str() {
        "62691bae-8d34-4142-a32f-b5e5ba30d444" => {
            reader.take(8)?; // Flags and referenced identifier.
            "CxDbSingleRelation"
        }
        "77e8ac27-a318-44dc-bed7-edaac7fadf6e" => {
            reader.take(24)?; // Flags, type, and sixteen opaque parameter bytes.
            for _ in 0..2 {
                let count = bounded_count(reader, budget, 4)?;
                reader.take(count as usize * 4)?;
            }
            for width in [24, 24, 8, 8, 24] {
                if presence(reader, "relation optional field")? {
                    reader.take(width)?;
                }
            }
            "CxDbRefPt"
        }
        "fa486143-85fe-4cb7-b5e2-c437d21f18a1" => {
            reader.take(4)?; // Flags.
            let groups = bounded_count(reader, budget, 4)?;
            for _ in 0..groups {
                let entries = bounded_count(reader, budget, 5)?;
                for _ in 0..entries {
                    reader.take(4)?; // Referenced identifier.
                    if presence(reader, "loop relation identifier list")? {
                        let identifiers = bounded_count(reader, budget, 4)?;
                        reader.take(identifiers as usize * 4)?;
                        reader.take(4)?; // Terminal identifier.
                    }
                }
            }
            let groups = bounded_count(reader, budget, 4)?;
            for _ in 0..groups {
                let pairs = bounded_count(reader, budget, 8)?;
                reader.take(pairs as usize * 8)?;
            }
            reader.take(4)?; // Final opaque word.
            "CxDbLoopRelation"
        }
        _ => {
            return Err(FrameError::Unsupported(format!(
                "Unsupported relation type {type_id} at decoded offset {start}; no resynchronization attempted."
            )));
        }
    };
    Ok(RecordRelation {
        type_id,
        class_name: class_name.into(),
        span: span(start, reader.offset),
    })
}

fn record_extension(
    reader: &mut Reader<'_>,
    body: SourceSpan,
    profile: RecordProfile,
    budget: &mut u64,
    depth: u64,
) -> Result<RecordExtension, FrameError> {
    let start = reader.offset;
    let relation_type_id = read_guid(reader)?;
    if relation_type_id != RELATION_TYPE {
        return Err(FrameError::Unsupported(format!(
            "Unsupported relation-list type {relation_type_id} at decoded offset {start}; no resynchronization attempted."
        )));
    }
    let count = bounded_count(reader, budget, 16)?;
    let mut relations = Vec::new();
    for _ in 0..count {
        relations.push(relation(reader, budget)?);
    }
    let relation_section_span = span(start, reader.offset);
    let header_start = reader.offset;
    let header_u16_raw = reader.u16()?;
    let header_u32_raw = [reader.u32()?, reader.u32()?];
    if header_u16_raw != 0
        || header_u32_raw[0] != 1
        || !matches!(header_u32_raw[1], 0xf000fe00 | 0xf000fe40)
    {
        return Err(FrameError::Unsupported(format!(
            "Unsupported schema-3 extension header at decoded offset {header_start}; no resynchronization attempted."
        )));
    }
    let header_span = span(header_start, reader.offset);
    let attributes_start = reader.offset;
    let width = profile.attribute_width();
    if body.length < width as u64 {
        return Err(FrameError::Unsupported(format!(
            "Schema-3 repeated attributes do not match the short initial body for profile {}; no resynchronization attempted.",
            profile.as_str()
        )));
    }
    let attributes = reader.take(width)?;
    let body_start = body.offset as usize;
    if reader.data[body_start..body_start + attributes.len()] != *attributes {
        return Err(FrameError::Unsupported(format!(
            "Schema-3 repeated attributes at decoded offset {attributes_start} do not match the initial body prefix for profile {}; no resynchronization attempted.",
            profile.as_str()
        )));
    }
    let attributes_span = span(attributes_start, reader.offset);
    let display_start = reader.offset;
    let present = presence(reader, "display cache")?;
    let presence_span = span(display_start, reader.offset);
    let mut records = Vec::new();
    let type_id = if present {
        let type_id = read_guid(reader)?;
        if type_id != DISPLAY_TYPE {
            return Err(FrameError::Unsupported(format!(
                "Unsupported display cache type {type_id} at decoded offset {}; no resynchronization attempted.",
                display_start + 1
            )));
        }
        let count = reader.u32()? as u64;
        // Each child consumes the cumulative budget in parse_record, not twice.
        ParseOptions::check_limit(
            "max_records",
            budget.saturating_add(count),
            reader.options.max_records,
        )?;
        if count > (reader.data.len() - reader.offset) as u64 / 26 {
            return Err(Error::InvalidStream(format!(
                "display record count at decoded offset {} exceeds remaining stream bytes",
                reader.offset - 4
            ))
            .into());
        }
        for _ in 0..count {
            let record = parse_record(
                reader.data,
                reader.offset,
                reader.options,
                budget,
                Some(profile),
                depth + 1,
            )?;
            reader.offset = (record.span.offset + record.span.length) as usize;
            records.push(record);
        }
        Some(type_id)
    } else {
        None
    };
    let display_cache = RecordDisplayCache {
        span: span(display_start, reader.offset),
        presence_span,
        present,
        type_id,
        records,
    };
    let tail_start = reader.offset;
    let tail_raw = reader.take(1)?[0];
    if tail_raw != 0 {
        return Err(FrameError::Unsupported(format!(
            "Unsupported trailing-object presence {tail_raw} at decoded offset {tail_start}; no resynchronization attempted."
        )));
    }
    Ok(RecordExtension {
        span: span(start, reader.offset),
        relation_section_span,
        relation_type_id,
        relations,
        header_span,
        header_u16_raw,
        header_u32_raw,
        attributes_span,
        display_cache,
        tail_span: span(tail_start, reader.offset),
        tail_raw,
    })
}

fn parse_record(
    data: &[u8],
    start: usize,
    options: ParseOptions,
    budget: &mut u64,
    profile: Option<RecordProfile>,
    depth: u64,
) -> Result<RecordHeader, FrameError> {
    ParseOptions::check_limit(
        "max_record_depth",
        depth,
        options.max_record_depth.min(MAX_SAFE_RECORD_DEPTH),
    )?;
    if data.len() - start < 26 {
        return Err(Error::InvalidStream(format!(
            "truncated nested record header at decoded offset {start}"
        ))
        .into());
    }
    let object_schema = data[start + 24];
    let property_presence = data[start + 25];
    let schema = u16::from_le_bytes([object_schema, property_presence]);
    if !matches!(
        (object_schema, property_presence),
        (1, 0) | (1, 1) | (100, 1) | (3, 0) | (3, 1)
    ) {
        return Err(FrameError::Unsupported(format!(
            "Unsupported record schema {schema:#06x} (object schema {object_schema}, property presence {property_presence}); no resynchronization attempted."
        )));
    }
    if object_schema == 3 && profile.is_none() {
        return Err(FrameError::Unsupported(format!(
            "Unsupported record schema {schema:#06x}: schema 3 requires an explicit recognized framing profile; no resynchronization attempted."
        )));
    }
    consume_budget(budget, 1, options)?;
    let mut reader = Reader::new(data, options)?;
    reader.offset = start + 26;
    let mut property_groups = Vec::new();
    if property_presence == 1 {
        let count = reader.u32()? as u64;
        consume_budget(budget, count, options)?;
        // Each group requires its GUID and value count, even if it is empty.
        if count > (data.len() - reader.offset) as u64 / 20 {
            return Err(Error::InvalidStream(format!(
                "property group count at decoded offset {} exceeds stream",
                reader.offset - 4
            ))
            .into());
        }
        for _ in 0..count {
            let group_start = reader.offset;
            let guid = reader.take(16)?.try_into().unwrap();
            let value_count = reader.u32()? as u64;
            consume_budget(budget, value_count, options)?;
            // Unknown tags can be diagnosed from one byte, before reading a key.
            if value_count > (data.len() - reader.offset) as u64 {
                return Err(Error::InvalidStream(format!(
                    "property value count at decoded offset {} exceeds stream",
                    reader.offset - 4
                ))
                .into());
            }
            let mut values = Vec::new();
            for _ in 0..value_count {
                let value_start = reader.offset;
                let tag = reader.take(1)?[0];
                let (key, object_type_id, value_span) = if tag == 12 {
                    let object_guid = reader.take(16)?.try_into().unwrap();
                    let size = reader.u32()? as usize;
                    let body_start = reader.offset;
                    reader.take(size)?;
                    (
                        None,
                        Some(uuid::Uuid::from_bytes_le(object_guid).to_string()),
                        span(body_start, reader.offset),
                    )
                } else {
                    let width = match tag {
                        0 => 0,
                        1 => 1,
                        2 | 4 | 5 => 4,
                        6 => 8,
                        7 => 0, // A validated length-prefixed UTF-16 field below.
                        8 => 2,
                        9 => 24,
                        _ => {
                            return Err(FrameError::Unsupported(format!(
                                "Unsupported property value tag {tag} at decoded offset {value_start}; record at offset {start} remains unparsed; no resynchronization attempted."
                            )));
                        }
                    };
                    let key = property_text(&mut reader)?;
                    let body_start = reader.offset;
                    if tag == 7 {
                        property_text(&mut reader)?;
                    } else {
                        reader.take(width)?;
                    }
                    (Some(key), None, span(body_start, reader.offset))
                };
                values.push(RecordPropertyValue {
                    tag,
                    key,
                    object_type_id,
                    span: span(value_start, reader.offset),
                    value_span,
                });
            }
            property_groups.push(RecordPropertyGroup {
                type_id: uuid::Uuid::from_bytes_le(guid).to_string(),
                span: span(group_start, reader.offset),
                values,
            });
        }
    }
    let size = reader.u32()? as usize;
    let payload_start = reader.offset;
    reader.take(size)?;
    let payload_span = span(payload_start, reader.offset);
    let extension = if object_schema == 3 {
        Some(record_extension(
            &mut reader,
            payload_span.clone(),
            profile.unwrap(),
            budget,
            depth,
        )?)
    } else {
        None
    };
    let guid: [u8; 16] = data[start..start + 16].try_into().unwrap();
    Ok(RecordHeader {
        type_id: uuid::Uuid::from_bytes_le(guid).to_string(),
        class_name: (guid == LINE_GUID).then(|| "CxDbLine".into()),
        id_raw: i32::from_le_bytes(data[start + 16..start + 20].try_into().unwrap()),
        owner_raw: i32::from_le_bytes(data[start + 20..start + 24].try_into().unwrap()),
        schema,
        object_schema,
        properties_present: property_presence == 1,
        property_groups,
        span: span(start, reader.offset),
        payload_span,
        extension,
    })
}

/// Walk supported object-schema/property-presence pairs from the known header.
/// Unknown schemas or property tags stop before the entire unsupported record.
/// No GUID scanning or resynchronization is performed. `max_records` limits the
/// cumulative number of records, property groups, and property values.
pub fn inspect_record_headers(
    data: &[u8],
    options: ParseOptions,
) -> Result<RecordInspection, Error> {
    inspect_record_headers_with_profile(data, options, None)
}

/// Frame schema-3 records only with an explicit experimental attribute profile.
/// The profile must come from a caller's known format context or the exact
/// product-build gate. Unknown nested structures stop before the enclosing
/// top-level record, so incomplete subtrees are never returned.
pub fn inspect_record_headers_with_profile(
    data: &[u8],
    options: ParseOptions,
    profile: Option<RecordProfile>,
) -> Result<RecordInspection, Error> {
    inspect_record_headers_with_budget(data, options, profile).map(|(result, _)| result)
}

pub(crate) fn inspect_record_headers_with_budget(
    data: &[u8],
    options: ParseOptions,
    profile: Option<RecordProfile>,
) -> Result<(RecordInspection, u64), Error> {
    ParseOptions::check_limit(
        "max_decoded_stream_bytes",
        data.len() as u64,
        options.max_decoded_stream_bytes,
    )?;
    if !data.starts_with(PREFIX) {
        return Err(Error::UnsupportedFeature("block record stream header"));
    }
    if data.len() < 172 {
        return Err(Error::InvalidStream("truncated block stream header".into()));
    }
    if data[12..172].iter().any(|byte| *byte != 0) {
        return Err(Error::UnsupportedFeature(
            "nonzero block stream header extension",
        ));
    }
    let mut result = RecordInspection {
        evidence: "corpus_consistent".into(),
        profile: profile.map(|value| value.as_str().into()),
        offset_space: "decoded_stream".into(),
        stream_version: "0.0".into(),
        records: Vec::new(),
        framing_complete: false,
        unparsed_span: None,
        stop_reason: None,
        geometry_decoded: false,
    };
    let mut offset = 172usize;
    let mut budget = 0u64;
    while offset < data.len() {
        if data.len() - offset < 26 {
            result.stop_reason = Some("Remaining bytes do not form a known record header.".into());
            break;
        }
        match parse_record(data, offset, options, &mut budget, profile, 0) {
            Ok(record) => {
                offset = (record.span.offset + record.span.length) as usize;
                result.records.push(record);
            }
            Err(FrameError::Unsupported(reason)) => {
                result.stop_reason = Some(reason);
                break;
            }
            Err(FrameError::Invalid(error)) => return Err(error),
        }
    }
    result.framing_complete = offset == data.len();
    if !result.framing_complete {
        result.unparsed_span = Some(span(offset, data.len()));
    }
    Ok((result, budget))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Vec<u8> {
        let mut data = PREFIX.to_vec();
        data.resize(172, 0);
        data
    }
    fn append_record(data: &mut Vec<u8>, schema: u16, payload: &[u8]) {
        data.extend(LINE_GUID);
        data.extend(7i32.to_le_bytes());
        data.extend((-5i32).to_le_bytes());
        data.extend(schema.to_le_bytes());
        data.extend((payload.len() as u32).to_le_bytes());
        data.extend(payload);
    }

    #[test]
    fn sequential_boundaries_and_source_spans() {
        let mut data = header();
        append_record(&mut data, 1, &[0; 81]);
        append_record(&mut data, 1, &LINE_GUID); // Embedded GUID must not become another record.
        let result = inspect_record_headers(&data, ParseOptions::default()).unwrap();
        assert_eq!(result.records.len(), 2);
        assert!(result.framing_complete);
        assert!(!result.geometry_decoded);
        assert_eq!(
            result.records[0].span,
            SourceSpan {
                offset: 172,
                length: 111
            }
        );
        assert_eq!(result.records[1].span.offset, 283);
        assert_eq!(result.records[0].class_name.as_deref(), Some("CxDbLine"));
        assert_eq!(
            result.records[0].type_id,
            "17ecf72d-c046-4e9a-8416-fd8220fd33b5"
        );
    }

    #[test]
    fn unknown_schema_stops_without_skipping() {
        let mut data = header();
        append_record(&mut data, 0x7777, &[0; 32]);
        append_record(&mut data, 1, &[0; 81]);
        let result = inspect_record_headers(&data, ParseOptions::default()).unwrap();
        assert!(result.records.is_empty());
        assert!(!result.framing_complete);
        assert_eq!(result.unparsed_span.unwrap().offset, 172);
    }

    #[test]
    fn corrupt_lengths_and_budgets_fail() {
        let mut data = header();
        append_record(&mut data, 1, &[0; 81]);
        assert!(matches!(
            inspect_record_headers(&data[..data.len() - 1], ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        assert!(matches!(
            inspect_record_headers(
                &data,
                ParseOptions {
                    max_records: 0,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit { .. })
        ));
        assert!(matches!(
            inspect_record_headers(
                &data,
                ParseOptions {
                    max_decoded_stream_bytes: 4,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit { .. })
        ));
    }

    fn append_text(data: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().chain([0]).collect();
        data.extend((units.len() as u32 * 2).to_le_bytes());
        for unit in units {
            data.extend(unit.to_le_bytes());
        }
    }

    fn append_extended_record(
        data: &mut Vec<u8>,
        object_schema: u8,
        properties: &[u8],
        payload: &[u8],
    ) {
        data.extend(LINE_GUID);
        data.extend(8i32.to_le_bytes());
        data.extend((-5i32).to_le_bytes());
        data.extend([object_schema, 1]);
        data.extend(properties);
        data.extend((payload.len() as u32).to_le_bytes());
        data.extend(payload);
    }

    fn one_group(values: &[u8], count: u32) -> Vec<u8> {
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend([0x11; 16]);
        data.extend(count.to_le_bytes());
        data.extend(values);
        data
    }

    #[test]
    fn extended_records_frame_all_observed_tags_and_preserve_spans() {
        let mut properties = Vec::new();
        let tags = [
            (0, 0),
            (1, 1),
            (2, 4),
            (4, 4),
            (5, 4),
            (6, 8),
            (8, 2),
            (9, 24),
        ];
        for (tag, width) in tags {
            properties.push(tag);
            if tag == 0 {
                // A zero-length encoded key is valid only in this property grammar.
                properties.extend(0u32.to_le_bytes());
            } else {
                append_text(&mut properties, "key");
            }
            properties.extend(vec![tag; width]);
        }
        properties.push(7);
        append_text(&mut properties, "text");
        append_text(&mut properties, "値");
        properties.push(12);
        properties.extend([0x22; 16]);
        properties.extend(3u32.to_le_bytes());
        properties.extend([17, 18, 19]);
        let mut data = header();
        append_record(&mut data, 1, &[0; 81]);
        append_extended_record(&mut data, 1, &one_group(&properties, 10), &[5, 6]);
        append_extended_record(&mut data, 100, &0u32.to_le_bytes(), &[7]);
        let result = inspect_record_headers(&data, ParseOptions::default()).unwrap();
        assert!(result.framing_complete);
        assert_eq!(result.records.len(), 3);
        assert!(result.records[0].property_groups.is_empty());
        assert_eq!(result.records[0].object_schema, 1);
        assert!(!result.records[0].properties_present);
        let record = &result.records[1];
        assert_eq!(record.span.offset, 283);
        assert_eq!(record.schema, 0x0101);
        assert_eq!(record.object_schema, 1);
        assert!(record.properties_present);
        assert_eq!(record.property_groups.len(), 1);
        let group = &record.property_groups[0];
        assert_eq!(group.type_id, "11111111-1111-1111-1111-111111111111");
        assert_eq!(group.span.offset, record.span.offset + 30);
        assert_eq!(group.span.length, 20 + properties.len() as u64);
        assert_eq!(group.values.len(), 10);
        for (value, (tag, width)) in group.values.iter().zip(tags) {
            assert_eq!(value.tag, tag);
            assert_eq!(value.value_span.length, width as u64);
            assert!(value.object_type_id.is_none());
            assert_eq!(
                value.key.as_ref().unwrap().value,
                if tag == 0 { "" } else { "key" }
            );
            let start = value.value_span.offset as usize;
            assert_eq!(&data[start..start + width], vec![tag; width]);
        }
        let text = &group.values[8];
        assert_eq!(text.tag, 7);
        assert_eq!(text.value_span.length, 8);
        let start = text.value_span.offset as usize;
        assert_eq!(&data[start..start + 8], &[4, 0, 0, 0, 0x24, 0x50, 0, 0]);
        let blob = &group.values[9];
        assert!(blob.key.is_none());
        assert_eq!(
            blob.object_type_id.as_deref(),
            Some("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(blob.value_span.length, 3);
        assert_eq!(blob.span.length, 24);
        let start = blob.value_span.offset as usize;
        assert_eq!(&data[start..start + 3], &[17, 18, 19]);
        assert_eq!(
            record.payload_span.offset,
            group.span.offset + group.span.length + 4
        );
        assert_eq!(record.payload_span.length, 2);
        let last = &result.records[2];
        assert_eq!(last.object_schema, 100);
        assert_eq!(last.schema, 0x0164);
        assert!(last.property_groups.is_empty());
        assert_eq!(last.span.offset + last.span.length, data.len() as u64);
    }

    #[test]
    fn unsupported_schema_pairs_and_tags_stop_before_the_whole_record() {
        for schema in [0x0003, 0x0103, 0x0064, 0x0201, 0x7777] {
            let mut data = header();
            append_record(&mut data, 1, &[]);
            append_record(&mut data, schema, &LINE_GUID);
            append_record(&mut data, 1, &[0; 81]);
            let result = inspect_record_headers(&data, ParseOptions::default()).unwrap();
            assert_eq!(result.records.len(), 1, "schema {schema:#06x}");
            assert_eq!(result.unparsed_span.unwrap().offset, 202);
            assert!(result
                .stop_reason
                .unwrap()
                .contains("Unsupported record schema"));
        }
        let mut data = header();
        append_record(&mut data, 1, &[]);
        // An unknown tag is enough to stop; do not assume it has a key or length.
        append_extended_record(&mut data, 1, &one_group(&[255], 1), &[]);
        append_record(&mut data, 1, &[0; 81]);
        let result = inspect_record_headers(&data, ParseOptions::default()).unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.unparsed_span.unwrap().offset, 202);
        assert!(result
            .stop_reason
            .unwrap()
            .contains("Unsupported property value tag 255"));
    }

    #[test]
    fn extended_known_field_truncations_fail() {
        let mut values = vec![12];
        values.extend([0x22; 16]);
        values.extend(3u32.to_le_bytes());
        values.extend([1, 2, 3]);
        let mut data = header();
        append_extended_record(&mut data, 1, &one_group(&values, 1), &[4, 5]);
        // Every truncation after the schema pair is a malformed known frame.
        for length in 198..data.len() {
            assert!(
                matches!(
                    inspect_record_headers(&data[..length], ParseOptions::default()),
                    Err(Error::InvalidStream(_))
                ),
                "length {length}"
            );
        }
    }

    #[test]
    fn extended_counts_and_cumulative_budget_are_bounded() {
        let mut data = header();
        append_extended_record(&mut data, 1, &100u32.to_le_bytes(), &[]);
        assert!(matches!(
            inspect_record_headers(&data, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        let mut data = header();
        append_extended_record(&mut data, 1, &one_group(&[], 100), &[]);
        assert!(matches!(
            inspect_record_headers(&data, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        let mut data = header();
        append_record(&mut data, 1, &[]);
        append_extended_record(&mut data, 1, &one_group(&[0, 0, 0, 0, 0], 1), &[]);
        assert!(matches!(
            inspect_record_headers(
                &data,
                ParseOptions {
                    max_records: 3,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit {
                name: "max_records",
                actual: 4,
                limit: 3
            })
        ));
        assert!(
            inspect_record_headers(
                &data,
                ParseOptions {
                    max_records: 4,
                    ..ParseOptions::default()
                }
            )
            .unwrap()
            .framing_complete
        );
    }

    #[test]
    fn property_strings_are_validated_and_bounded() {
        let invalid_texts: &[&[u8]] = &[
            &[1, 0, 0, 0, 0],                   // Odd byte length.
            &[2, 0, 0, 0, 1, 0],                // Missing terminal NUL.
            &[4, 0, 0, 0, 0, 0xd8, 0, 0],       // Unpaired high surrogate.
            &[6, 0, 0, 0, 0, 0, 0x61, 0, 0, 0], // Embedded NUL.
        ];
        for encoded in invalid_texts {
            for is_value in [false, true] {
                let mut value = if is_value {
                    vec![7, 0, 0, 0, 0]
                } else {
                    vec![0]
                };
                value.extend_from_slice(encoded);
                let mut data = header();
                append_extended_record(&mut data, 1, &one_group(&value, 1), &[]);
                assert!(matches!(
                    inspect_record_headers(&data, ParseOptions::default()),
                    Err(Error::InvalidStream(_))
                ));
            }
        }
        let mut value = vec![7, 0, 0, 0, 0];
        append_text(&mut value, "abc");
        let mut data = header();
        append_extended_record(&mut data, 1, &one_group(&value, 1), &[]);
        assert!(matches!(
            inspect_record_headers(
                &data,
                ParseOptions {
                    max_string_bytes: 6,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit {
                name: "max_string_bytes",
                actual: 8,
                limit: 6
            })
        ));
    }

    fn guid_bytes(value: &str) -> [u8; 16] {
        uuid::Uuid::parse_str(value).unwrap().to_bytes_le()
    }

    fn simple_record(schema: u16, payload: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        append_record(&mut result, schema, payload);
        result
    }

    fn schema3_record(
        width: usize,
        children: Option<&[Vec<u8>]>,
        relations: &[Vec<u8>],
        with_properties: bool,
    ) -> Vec<u8> {
        let attributes = vec![0x5a; width];
        let mut body = attributes.clone();
        body.extend([0xaa, 0xbb]);
        let mut bytes = Vec::new();
        if with_properties {
            append_extended_record(&mut bytes, 3, &0u32.to_le_bytes(), &body);
        } else {
            append_record(&mut bytes, 3, &body);
        }
        bytes.extend(guid_bytes(RELATION_TYPE));
        bytes.extend((relations.len() as u32).to_le_bytes());
        for relation in relations {
            bytes.extend(relation);
        }
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0xf000fe00u32.to_le_bytes());
        bytes.extend(attributes);
        if let Some(children) = children {
            bytes.push(1);
            bytes.extend(guid_bytes(DISPLAY_TYPE));
            bytes.extend((children.len() as u32).to_le_bytes());
            for child in children {
                bytes.extend(child);
            }
        } else {
            bytes.push(0);
        }
        bytes.push(0); // A different trailing object's absence, not a second cache.
        bytes
    }

    fn relation_fixtures() -> Vec<Vec<u8>> {
        let mut single = guid_bytes("62691bae-8d34-4142-a32f-b5e5ba30d444").to_vec();
        single.extend(0x11223344u32.to_le_bytes());
        single.extend((-7i32).to_le_bytes());
        let mut reference = guid_bytes("77e8ac27-a318-44dc-bed7-edaac7fadf6e").to_vec();
        reference.extend(3u32.to_le_bytes());
        reference.extend(4u32.to_le_bytes());
        reference.extend(LINE_GUID); // A GUID inside opaque parameters is not a record.
        reference.extend(2u32.to_le_bytes());
        reference.extend((-7i32).to_le_bytes());
        reference.extend((-8i32).to_le_bytes());
        reference.extend(1u32.to_le_bytes());
        reference.extend((-9i32).to_le_bytes());
        for (present, width) in [(true, 24), (false, 24), (true, 8), (false, 8), (true, 24)] {
            reference.push(u8::from(present));
            if present {
                reference.extend(vec![0x6b; width]);
            }
        }
        let mut loop_relation = guid_bytes("fa486143-85fe-4cb7-b5e2-c437d21f18a1").to_vec();
        loop_relation.extend(5u32.to_le_bytes());
        loop_relation.extend(2u32.to_le_bytes()); // First outer array.
        loop_relation.extend(2u32.to_le_bytes()); // First inner array.
        loop_relation.extend((-7i32).to_le_bytes());
        loop_relation.push(0);
        loop_relation.extend((-8i32).to_le_bytes());
        loop_relation.push(1);
        loop_relation.extend(2u32.to_le_bytes());
        loop_relation.extend((-10i32).to_le_bytes());
        loop_relation.extend((-11i32).to_le_bytes());
        loop_relation.extend((-12i32).to_le_bytes());
        loop_relation.extend(0u32.to_le_bytes()); // Second inner array.
        loop_relation.extend(1u32.to_le_bytes()); // Second outer array.
        loop_relation.extend(2u32.to_le_bytes()); // Identifier-pair array.
        for identifier in [-20i32, -21, 22, 23] {
            loop_relation.extend(identifier.to_le_bytes());
        }
        loop_relation.extend(6u32.to_le_bytes());
        vec![single, reference, loop_relation]
    }

    fn profile33() -> Option<RecordProfile> {
        Some(RecordProfile::Schema3Attributes33)
    }

    #[test]
    fn schema3_requires_an_exact_or_explicit_profile() {
        assert_eq!(
            RecordProfile::from_product_build("10.0.0.0000"),
            profile33()
        );
        assert_eq!(
            RecordProfile::from_product_build("15.0.0.5681"),
            profile33()
        );
        assert_eq!(
            RecordProfile::from_product_build("18.0.0.18159"),
            Some(RecordProfile::Schema3Attributes37)
        );
        for build in [
            "10",
            "10.0.0.0001",
            "18.0.0.18159 ",
            "Synthetic Build",
            "27.0.0.21423",
        ] {
            assert_eq!(RecordProfile::from_product_build(build), None);
        }
        let mut data = header();
        data.extend(schema3_record(33, None, &[], false));
        let stopped = inspect_record_headers(&data, ParseOptions::default()).unwrap();
        assert_eq!(stopped.profile, None);
        assert!(stopped.records.is_empty());
        assert!(stopped
            .stop_reason
            .unwrap()
            .contains("requires an explicit recognized"));
        let result =
            inspect_record_headers_with_profile(&data, ParseOptions::default(), profile33())
                .unwrap();
        assert!(result.framing_complete);
        assert_eq!(
            result.profile.as_deref(),
            Some("schema3-attributes33-experimental")
        );
        let mismatched = inspect_record_headers_with_profile(
            &data,
            ParseOptions::default(),
            Some(RecordProfile::Schema3Attributes37),
        )
        .unwrap();
        assert!(mismatched.records.is_empty());
        assert!(mismatched.stop_reason.unwrap().contains("do not match"));
        let mut newer = header();
        newer.extend(schema3_record(37, None, &[], true));
        let result = inspect_record_headers_with_profile(
            &newer,
            ParseOptions::default(),
            Some(RecordProfile::Schema3Attributes37),
        )
        .unwrap();
        assert!(result.framing_complete);
        assert_eq!(result.records[0].schema, 0x0103);
        assert_eq!(
            result.records[0]
                .extension
                .as_ref()
                .unwrap()
                .attributes_span
                .length,
            37
        );
    }

    #[test]
    fn schema3_recursive_display_records_stay_separate_and_preserve_spans() {
        let deepest = simple_record(1, &[0; 81]);
        let inner = schema3_record(33, Some(&[deepest]), &[], true);
        let children = [simple_record(1, &[0; 81]), inner];
        let relation_bytes = relation_fixtures();
        let outer = schema3_record(33, Some(&children), &relation_bytes, false);
        let mut data = header();
        data.extend(&outer);
        data.extend(simple_record(1, &[0; 81]));
        let result =
            inspect_record_headers_with_profile(&data, ParseOptions::default(), profile33())
                .unwrap();
        assert!(result.framing_complete);
        assert_eq!(result.records.len(), 2);
        let record = &result.records[0];
        assert_eq!(record.span, span(172, 172 + outer.len()));
        assert_eq!(record.payload_span, span(202, 237));
        let extension = record.extension.as_ref().unwrap();
        assert_eq!(extension.span.offset, 237);
        assert_eq!(
            extension.span.offset + extension.span.length,
            record.span.offset + record.span.length
        );
        assert_eq!(extension.relations.len(), 3);
        assert_eq!(extension.relations[0].class_name, "CxDbSingleRelation");
        assert_eq!(extension.relations[1].class_name, "CxDbRefPt");
        assert_eq!(extension.relations[2].class_name, "CxDbLoopRelation");
        for (relation, original) in extension.relations.iter().zip(relation_bytes) {
            let offset = relation.span.offset as usize;
            assert_eq!(relation.span.length, original.len() as u64);
            assert_eq!(&data[offset..offset + original.len()], original);
        }
        assert_eq!(extension.header_span.length, 10);
        assert_eq!(extension.header_u16_raw, 0);
        assert_eq!(extension.header_u32_raw, [1, 0xf000fe00]);
        assert_eq!(extension.attributes_span.length, 33);
        let cache = &extension.display_cache;
        assert!(cache.present);
        assert_eq!(cache.type_id.as_deref(), Some(DISPLAY_TYPE));
        assert_eq!(cache.presence_span.length, 1);
        assert_eq!(cache.records.len(), 2);
        assert_eq!(cache.records[0].span.offset, cache.span.offset + 21);
        assert_eq!(
            cache.records[1]
                .extension
                .as_ref()
                .unwrap()
                .display_cache
                .records
                .len(),
            1
        );
        assert_eq!(
            extension.tail_span,
            span(172 + outer.len() - 1, 172 + outer.len())
        );
        assert_eq!(extension.tail_raw, 0);
        assert_eq!(
            result.records[1].span.offset,
            record.span.offset + record.span.length
        );
        let lines =
            crate::inspect_lines_with_profile(&data, ParseOptions::default(), profile33()).unwrap();
        assert_eq!(lines.lines.len(), 1); // Cached LINEs are deliberately not returned.
        assert_eq!(lines.lines[0].record_index, 1);
        assert_eq!(lines.unsupported_lines.len(), 1); // Synthetic outer LINE has schema 3.
        assert!(!lines.drawing_complete);
    }

    #[test]
    fn nested_unknowns_never_return_a_partial_enclosing_record() {
        let children = [
            simple_record(1, &[0; 81]),
            simple_record(0x7777, &[0; 81]),
            simple_record(1, &[0; 81]),
        ];
        let unknown_child = schema3_record(33, Some(&children), &[], false);
        let mut unknown_tail = schema3_record(33, None, &[], false);
        *unknown_tail.last_mut().unwrap() = 1;
        let mut unknown_relation = schema3_record(33, None, &[vec![0x77; 16]], false);
        // An additional following LINE must never become a synchronization point.
        unknown_relation.extend(simple_record(1, &[0; 81]));
        for unknown in [unknown_child, unknown_tail, unknown_relation] {
            let mut data = header();
            data.extend(simple_record(1, &[0; 81]));
            let start = data.len();
            data.extend(unknown);
            data.extend(simple_record(1, &[0; 81]));
            let result =
                inspect_record_headers_with_profile(&data, ParseOptions::default(), profile33())
                    .unwrap();
            assert_eq!(result.records.len(), 1);
            assert!(!result.framing_complete);
            assert_eq!(result.unparsed_span.unwrap(), span(start, data.len()));
            assert!(result.stop_reason.is_some());
        }
    }

    #[test]
    fn schema3_known_field_truncation_is_an_error() {
        let children = [schema3_record(
            33,
            Some(&[simple_record(1, &[0; 81])]),
            &[],
            true,
        )];
        let mut data = header();
        data.extend(schema3_record(
            33,
            Some(&children),
            &relation_fixtures(),
            true,
        ));
        for length in 198..data.len() {
            assert!(
                matches!(
                    inspect_record_headers_with_profile(
                        &data[..length],
                        ParseOptions::default(),
                        profile33()
                    ),
                    Err(Error::InvalidStream(_))
                ),
                "truncation length {length}"
            );
        }
    }

    #[test]
    fn schema3_depth_has_configurable_and_hard_limits() {
        let mut nested = simple_record(1, &[]);
        for _ in 0..3 {
            nested = schema3_record(33, Some(&[nested]), &[], false);
        }
        let mut data = header();
        data.extend(nested.clone());
        let options = ParseOptions {
            max_record_depth: 2,
            ..ParseOptions::default()
        };
        assert!(matches!(
            inspect_record_headers_with_profile(&data, options, profile33()),
            Err(Error::ResourceLimit {
                name: "max_record_depth",
                actual: 3,
                limit: 2
            })
        ));
        let options = ParseOptions {
            max_record_depth: 3,
            ..ParseOptions::default()
        };
        assert!(
            inspect_record_headers_with_profile(&data, options, profile33())
                .unwrap()
                .framing_complete
        );
        for _ in 0..63 {
            nested = schema3_record(33, Some(&[nested]), &[], false);
        }
        let mut data = header();
        data.extend(nested);
        let options = ParseOptions {
            max_record_depth: u64::MAX,
            ..ParseOptions::default()
        };
        assert!(matches!(
            inspect_record_headers_with_profile(&data, options, profile33()),
            Err(Error::ResourceLimit {
                name: "max_record_depth",
                actual: 65,
                limit: 64
            })
        ));
    }

    #[test]
    fn schema3_relation_arrays_share_the_record_budget() {
        let mut data = header();
        data.extend(schema3_record(33, None, &relation_fixtures(), false));
        // 1 record + 3 relations + 3 RefPt entries + 9 Loop array entries.
        let options = ParseOptions {
            max_records: 15,
            ..ParseOptions::default()
        };
        assert!(matches!(
            inspect_record_headers_with_profile(&data, options, profile33()),
            Err(Error::ResourceLimit {
                name: "max_records",
                actual: 16,
                limit: 15
            })
        ));
        let options = ParseOptions {
            max_records: 16,
            ..ParseOptions::default()
        };
        assert!(
            inspect_record_headers_with_profile(&data, options, profile33())
                .unwrap()
                .framing_complete
        );
        let mut data = header();
        data.extend(schema3_record(
            33,
            Some(&[simple_record(1, &[]), simple_record(1, &[])]),
            &[],
            false,
        ));
        let options = ParseOptions {
            max_records: 2,
            ..ParseOptions::default()
        };
        assert!(matches!(
            inspect_record_headers_with_profile(&data, options, profile33()),
            Err(Error::ResourceLimit {
                name: "max_records",
                actual: 3,
                limit: 2
            })
        ));
    }

    #[test]
    fn schema3_unknown_presence_header_and_changed_attributes_stop_safely() {
        let mut data = header();
        data.extend(schema3_record(33, Some(&[]), &[], false));
        let valid =
            inspect_record_headers_with_profile(&data, ParseOptions::default(), profile33())
                .unwrap();
        let extension = valid.records[0].extension.as_ref().unwrap();
        for offset in [
            extension.span.offset,
            extension.header_span.offset,
            extension.attributes_span.offset,
            extension.display_cache.span.offset,
            extension.display_cache.span.offset + 1,
        ] {
            let mut changed = data.clone();
            changed[offset as usize] ^= 0xff;
            let result =
                inspect_record_headers_with_profile(&changed, ParseOptions::default(), profile33())
                    .unwrap();
            assert!(result.records.is_empty(), "mutation offset {offset}");
            assert!(!result.framing_complete);
            assert_eq!(result.unparsed_span.unwrap().offset, 172);
        }
    }
}

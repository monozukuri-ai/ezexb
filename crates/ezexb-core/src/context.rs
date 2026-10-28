//! Bounded layer, linetype, paper-space, and block context for observed profiles.
//! Unused annotation styles retain their source spans without claiming semantics.
use std::collections::HashSet;

use serde::Serialize;

use crate::metadata::Reader;
use crate::{Error, ParseOptions, RecordHeader, SourceSpan, TextField};

const LAYER: &str = "6d0ae65f-a804-4cf3-9d92-07831c336a7a";
const LINETYPE: &str = "c8f1ff60-a882-4639-93cb-87921b4f5d0b";
const PAPER: &str = "979f2fd6-ca62-42b8-9cb8-d47716dd1dd7";
const BLOCK: &str = "b65f3081-5c0e-4d00-97b4-0c5231051af1";
const RELATIONS: &str = "0fbb54e7-fc10-4b09-9499-924097c3bf94";
const SINGLE_RELATION: &str = "62691bae-8d34-4142-a32f-b5e5ba30d444";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContextRecord {
    pub class_name: Option<String>,
    pub type_id: String,
    pub id: i32,
    pub owner: i32,
    pub object_schema: u8,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    pub property_span: Option<SourceSpan>,
    pub extension_span: Option<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Layer {
    pub id: i32,
    pub owner: i32,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    pub name: TextField,
    /// Stored alternate strings; locale selection is not applied.
    pub alternate_name: TextField,
    pub key_name: TextField,
    pub description: TextField,
    pub color_raw: u32,
    pub status_raw: u32,
    pub width: f64,
    pub width_flag: u32,
    pub linetype_id: i32,
    pub linetype_status: u32,
    pub on: bool,
    pub locked: bool,
    pub printable: bool,
    pub frozen: bool,
    pub unknown_fields: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Linetype {
    pub id: i32,
    pub owner: i32,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    pub name: TextField,
    pub description: TextField,
    pub alternate_name: TextField,
    pub scale: f64,
    /// Native floating-point dash values; no display-unit conversion is applied.
    pub dashes: Vec<f64>,
    pub pattern: Option<u16>,
    pub advanced_present: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Space {
    pub id: i32,
    pub owner: i32,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    pub name: TextField,
    /// Native PaperScale, not a coordinate-unit conversion.
    pub scale: f64,
    pub block_id: i32,
    pub block_status: u32,
    pub current_ucs_id: i32,
    pub unknown_fields: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Block {
    /// Bounded native CxDbSolid attachment, separate from normal block children.
    pub solid_attachment: Option<ContextRecord>,
    pub solid_geometry: Option<crate::complex::SolidGeometry>,
    pub solid_attributes: Option<crate::EntityAttributes>,
    /// The qualified definition fields consumed the whole bounded body.
    pub complete: bool,
    pub diagnostics: Vec<String>,
    pub current_ucs_id: i32,
    pub id: i32,
    pub owner: i32,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
    pub name: TextField,
    pub unknown_fields: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StyleContext {
    /// Cumulative records and nested property/relation/dash entries consumed.
    pub budget_used: u64,
    pub evidence: &'static str,
    /// Required layer/linetype layouts and the whole context framing are known.
    /// Annotation-style bodies in other_records remain opaque.
    pub complete: bool,
    pub layers: Vec<Layer>,
    pub linetypes: Vec<Linetype>,
    pub other_records: Vec<ContextRecord>,
    pub diagnostics: Vec<String>,
    pub unparsed_span: Option<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpaceContext {
    /// Cumulative records and nested property/relation entries consumed.
    pub budget_used: u64,
    pub evidence: &'static str,
    /// Space names, scales, references, and framing are known; this does not
    /// qualify coordinates, view transforms, or opaque attached tables.
    pub complete: bool,
    pub spaces: Vec<Space>,
    pub other_records: Vec<ContextRecord>,
    pub diagnostics: Vec<String>,
    pub unparsed_span: Option<SourceSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlockContext {
    /// Embedded records and properties; caller already charged outer records.
    pub budget_used: u64,
    pub evidence: &'static str,
    pub complete: bool,
    pub blocks: Vec<Block>,
    pub other_records: Vec<ContextRecord>,
    pub diagnostics: Vec<String>,
    pub unparsed_span: Option<SourceSpan>,
}

fn span(start: usize, end: usize) -> SourceSpan {
    SourceSpan {
        offset: start as u64,
        length: (end - start) as u64,
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidStream(message.into())
}

enum ContextError {
    Invalid(Error),
    Unsupported(String),
}
impl From<Error> for ContextError {
    fn from(value: Error) -> Self {
        Self::Invalid(value)
    }
}
type ContextResult<T> = Result<T, ContextError>;

/// Major of a build inside the qualified range (see `crate::build`); the
/// decoders branch on the major only.
fn major(build: &str) -> Option<u32> {
    crate::build::qualified_major(build)
}

fn guid(r: &mut Reader<'_>) -> Result<String, Error> {
    Ok(uuid::Uuid::from_bytes_le(r.take(16)?.try_into().unwrap()).to_string())
}
fn text(r: &mut Reader<'_>) -> Result<TextField, Error> {
    let start = r.offset;
    if r.u32()? == 0 {
        return Ok(TextField {
            value: String::new(),
            span: span(start, r.offset),
        });
    }
    r.offset = start;
    r.text()
}
fn i32_value(r: &mut Reader<'_>) -> Result<i32, Error> {
    Ok(r.u32()? as i32)
}
fn f32_value(r: &mut Reader<'_>) -> Result<f64, Error> {
    let value = f32::from_le_bytes(r.take(4)?.try_into().unwrap());
    if !value.is_finite() {
        return Err(invalid("non-finite context float"));
    }
    Ok(f64::from(value))
}
fn f64_value(r: &mut Reader<'_>) -> Result<f64, Error> {
    let value = f64::from_le_bytes(r.take(8)?.try_into().unwrap());
    if !value.is_finite() {
        return Err(invalid("non-finite context double"));
    }
    Ok(value)
}
fn budget(used: &mut u64, count: u64, options: ParseOptions) -> Result<(), Error> {
    *used = used.saturating_add(count);
    ParseOptions::check_limit("max_records", *used, options.max_records)
}
fn count(r: &mut Reader<'_>, used: &mut u64, minimum: usize) -> Result<usize, Error> {
    let count = r.u32()? as usize;
    // An impossible count is a stream error, not budget exhaustion
    if count > (r.data.len() - r.offset) / minimum {
        return Err(invalid("context count exceeds remaining stream bytes"));
    }
    budget(used, count as u64, r.options)?;
    Ok(count)
}

fn skip_properties(r: &mut Reader<'_>, used: &mut u64) -> ContextResult<bool> {
    let groups = count(r, used, 20)?;
    let mut values_present = false;
    for _ in 0..groups {
        r.take(16)?;
        let values = count(r, used, 1)?;
        values_present |= values != 0;
        for _ in 0..values {
            skip_property_value(r, used, 0)?;
        }
    }
    Ok(values_present)
}

/// Same value encoding as `records::property_value`: tag 10 is a counted list of
/// nested values (CX_LAYOUTDATA, CX_SPECLIB_TXTINFO), tag 3 a four-byte flag.
fn skip_property_value(r: &mut Reader<'_>, used: &mut u64, depth: u64) -> ContextResult<()> {
    ParseOptions::check_limit(
        "max_record_depth",
        depth,
        r.options.max_record_depth.min(64),
    )?;
    let tag = r.take(1)?[0];
    if tag == 12 {
        r.take(16)?;
        let size = r.u32()? as usize;
        r.take(size)?;
        return Ok(());
    }
    let width = match tag {
        0 | 7 | 10 => 0,
        1 => 1,
        2..=5 => 4,
        6 => 8,
        8 => 2,
        9 => 24,
        _ => {
            return Err(ContextError::Unsupported(format!(
                "Unknown context property tag {tag}"
            )))
        }
    };
    text(r)?;
    match tag {
        7 => {
            text(r)?;
        }
        10 => {
            let nested = count(r, used, 1)?;
            for _ in 0..nested {
                skip_property_value(r, used, depth + 1)?;
            }
        }
        _ => {
            r.take(width)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    Style,
    Space,
}

fn known_schema(type_id: &str, kind: Kind) -> Option<u8> {
    match (kind, type_id) {
        (
            Kind::Style,
            LAYER
            | LINETYPE
            | "92216e22-2bbd-4d6d-ab34-24a69cdfb91e"
            | "c3c14722-1319-4c35-86bd-7a8b6ac22a0b"
            | "36e1dccd-333c-4007-b0fb-49bc6329205c"
            | "2eacf156-507a-45d7-8ad9-618b08d63dbc"
            | "b9bd09d2-0eaa-4fe7-9ff6-a23f19fe8954"
            | "5ccbf74a-358c-4fd1-b99a-7185dd522d7d"
            | "cf537287-481b-4a61-9ca5-6d420816ae17"
            | "d597a38c-016f-425e-9c4d-a7dc77a8d3a6"
            | "28f12394-cff0-42d7-bd3c-421b721579e5",
        ) => Some(1),
        (
            Kind::Style,
            "a7b11825-b974-4383-86ed-7cea3cdf557c"
            | "5393af7b-632a-4246-91e1-54cbc07551a4"
            | "f90a6c40-fa1d-438c-b899-a98a2d6fd6b7"
            | "07e0b92b-f65b-400a-a364-58bc8eab1efe"
            | "96feff0f-cac9-4fce-ac15-d80dc8bfb8de"
            | "a88cbe34-acb2-47f4-abae-fc7e078adb93"
            | "ead57a5b-f923-4f00-a54a-b2a0e3392f74",
        ) => Some(2),
        (
            Kind::Space,
            PAPER
            | "611180b4-0e2d-4848-a63e-bacb8a983d19"
            | "7e69e4bc-7e0c-4756-b841-f2d6711ad5bb"
            | "38083a18-c6a9-4cb1-a106-71bd42038a90"
            | "f284eab1-3ee0-4b16-9b7a-9e10350567e2"
            | "af6cf296-0189-40af-9381-2efa20ce502d"
            | "d4b4d3b4-89ac-40c3-a68a-16d17a01721b",
        ) => Some(1),
        (
            Kind::Space,
            "38c95f9b-4ccf-44bb-ad94-fad4b5944ae9"
            | "8363832b-518a-475c-8f09-34ba4454a7e4"
            | "0faf080b-f77d-4f9c-beba-03fbfe07d731"
            | "f851e69e-befe-4429-80b1-3ec80d6fa9a6"
            | "9a2d0f51-8162-4303-8395-1ba57baa6077",
        ) => Some(2),
        _ => None,
    }
}

fn frame(r: &mut Reader<'_>, used: &mut u64, kind: Kind) -> ContextResult<ContextRecord> {
    let start = r.offset;
    budget(used, 1, r.options)?;
    let type_id = guid(r)?;
    let id = i32_value(r)?;
    let owner = i32_value(r)?;
    let object_schema = r.take(1)?[0];
    let presence = r.take(1)?[0];
    if known_schema(&type_id, kind) != Some(object_schema) || presence > 1 {
        return Err(ContextError::Unsupported(format!("Unknown context type/schema {type_id}/{object_schema}/{presence} at offset {start}; no resynchronization attempted")));
    }
    let property_span = if presence == 1 {
        let p = r.offset;
        skip_properties(r, used)?;
        Some(span(p, r.offset))
    } else {
        None
    };
    let size = r.u32()? as usize;
    let body = r.offset;
    r.take(size)?;
    let payload_span = span(body, r.offset);
    let extension_span = if object_schema == 2 {
        let p = r.offset;
        if guid(r)? != RELATIONS {
            return Err(ContextError::Unsupported(format!(
                "Unknown context relation marker at offset {p}"
            )));
        }
        let relations = count(r, used, 24)?;
        for _ in 0..relations {
            if guid(r)? != SINGLE_RELATION {
                return Err(ContextError::Unsupported(format!(
                    "Unsupported context relation at offset {}",
                    r.offset - 16
                )));
            }
            r.take(8)?;
        }
        if r.u16()? != 0 || r.u32()? != 1 || r.u32()? != 0xf0000000 {
            return Err(ContextError::Unsupported(format!(
                "Unqualified context extension header at offset {p}"
            )));
        }
        Some(span(p, r.offset))
    } else {
        None
    };
    Ok(ContextRecord {
        class_name: None,
        type_id,
        id,
        owner,
        object_schema,
        span: span(start, r.offset),
        payload_span,
        property_span,
        extension_span,
    })
}

fn stream_reader(data: &[u8], options: ParseOptions) -> Result<Reader<'_>, Error> {
    let mut r = Reader::new(data, options)?;
    if r.version()?.value != "0.0" || r.take(160)?.iter().any(|v| *v != 0) {
        return Err(Error::UnsupportedFeature("context stream header"));
    }
    Ok(r)
}

fn body_reader<'a>(
    data: &'a [u8],
    field: &SourceSpan,
    options: ParseOptions,
) -> Result<Reader<'a>, Error> {
    let start =
        usize::try_from(field.offset).map_err(|_| invalid("context span exceeds address space"))?;
    let length = usize::try_from(field.length)
        .map_err(|_| invalid("context length exceeds address space"))?;
    let end = start
        .checked_add(length)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| invalid("context span exceeds stream"))?;
    let mut r = Reader::new(&data[..end], options)?;
    r.offset = start;
    Ok(r)
}
fn end_body(r: &Reader<'_>) -> ContextResult<()> {
    if r.offset != r.data.len() {
        return Err(ContextError::Unsupported(format!(
            "Unqualified context body suffix at offset {} ({} bytes)",
            r.offset,
            r.data.len() - r.offset
        )));
    }
    Ok(())
}

fn decode_layer(
    data: &[u8],
    record: &ContextRecord,
    major: u32,
    options: ParseOptions,
) -> ContextResult<Layer> {
    let mut r = body_reader(data, &record.payload_span, options)?;
    let name = text(&mut r)?;
    let alternate_name = text(&mut r)?;
    let key_name = text(&mut r)?;
    let color_raw = r.u32()?;
    let status_raw = r.u32()?;
    let description = text(&mut r)?;
    let width = f32_value(&mut r)?;
    let width_flag = r.u32()?;
    let linetype_status = r.u32()?;
    let linetype_id = i32_value(&mut r)?;
    let mut unknown_fields = Vec::new();
    if major >= 18 {
        let start = r.offset;
        r.take(4)?;
        unknown_fields.push(span(start, r.offset));
    }
    end_body(&r)?;
    Ok(Layer {
        id: record.id,
        owner: record.owner,
        span: record.span.clone(),
        payload_span: record.payload_span.clone(),
        name,
        alternate_name,
        key_name,
        description,
        color_raw,
        status_raw,
        width,
        width_flag,
        linetype_id,
        linetype_status,
        on: status_raw & 0x40000000 != 0,
        locked: status_raw & 0x20000000 != 0,
        printable: status_raw & 0x10000000 != 0,
        frozen: status_raw & 0x04000000 != 0,
        unknown_fields,
    })
}

fn decode_linetype(
    data: &[u8],
    record: &ContextRecord,
    major: u32,
    options: ParseOptions,
    used: &mut u64,
) -> ContextResult<Linetype> {
    let mut r = body_reader(data, &record.payload_span, options)?;
    let name = text(&mut r)?;
    let description = text(&mut r)?;
    let scale = f32_value(&mut r)?;
    let count = count(&mut r, used, 4)?;
    let pattern = if count > 0 { Some(r.u16()?) } else { None };
    let mut dashes = Vec::new();
    for _ in 0..count {
        dashes.push(f32_value(&mut r)?);
    }
    let alternate_name = text(&mut r)?;
    let advanced_present = if major > 10 {
        let flag = r.take(1)?[0];
        if flag != 0 {
            return Err(ContextError::Unsupported(format!(
                "Advanced linetype at offset {} is not decoded",
                record.span.offset
            )));
        }
        false
    } else {
        false
    };
    end_body(&r)?;
    Ok(Linetype {
        id: record.id,
        owner: record.owner,
        span: record.span.clone(),
        payload_span: record.payload_span.clone(),
        name,
        description,
        alternate_name,
        scale,
        dashes,
        pattern,
        advanced_present,
    })
}

fn decode_space(
    data: &[u8],
    record: &ContextRecord,
    major: u32,
    options: ParseOptions,
) -> ContextResult<Space> {
    let mut r = body_reader(data, &record.payload_span, options)?;
    let prefix = r.offset;
    r.take(120)?; // Native reader: ten u32 values, ten f64 values.
    for _ in 0..10 {
        text(&mut r)?;
    }
    let mut unknown_fields = vec![span(prefix, r.offset)];
    let name = text(&mut r)?;
    let current_ucs_id = i32_value(&mut r)?;
    let scale = f64_value(&mut r)?;
    let view = r.offset;
    r.take(32)?;
    unknown_fields.push(span(view, r.offset));
    let block_status = r.u32()?;
    let block_id = i32_value(&mut r)?;
    let suffix = r.offset;
    r.take(16 + 32 + 12)?; // Related IDs, grid/snap settings, opaque flags.
    if major > 10 {
        r.take(4)?;
    }
    if major >= 17 {
        r.take(16)?;
    }
    unknown_fields.push(span(suffix, r.offset));
    end_body(&r)?;
    Ok(Space {
        id: record.id,
        owner: record.owner,
        span: record.span.clone(),
        payload_span: record.payload_span.clone(),
        name,
        scale,
        block_id,
        block_status,
        current_ucs_id,
        unknown_fields,
    })
}

/// Decode qualified layer and linetype bodies, retaining other bounded styles.
pub fn decode_style_context(
    data: &[u8],
    product_build: &str,
    options: ParseOptions,
) -> Result<StyleContext, Error> {
    let mut r = stream_reader(data, options)?;
    let mut result = StyleContext {
        budget_used: 0,
        evidence: "native_static_and_corpus_consistent",
        complete: false,
        layers: Vec::new(),
        linetypes: Vec::new(),
        other_records: Vec::new(),
        diagnostics: Vec::new(),
        unparsed_span: None,
    };
    let Some(major) = major(product_build) else {
        result
            .diagnostics
            .push(format!("Unqualified style product build {product_build}"));
        result.unparsed_span = Some(span(r.offset, data.len()));
        return Ok(result);
    };
    let mut used = 0;
    let mut ids = HashSet::new();
    while r.offset < data.len() {
        let start = r.offset;
        let parsed = (|| -> ContextResult<()> {
            let record = frame(&mut r, &mut used, Kind::Style)?;
            if !ids.insert(record.id) {
                return Err(invalid("duplicate style context ID").into());
            }
            match record.type_id.as_str() {
                LAYER => result
                    .layers
                    .push(decode_layer(data, &record, major, options)?),
                LINETYPE => result
                    .linetypes
                    .push(decode_linetype(data, &record, major, options, &mut used)?),
                _ => result.other_records.push(record),
            }
            Ok(())
        })();
        result.budget_used = used;
        match parsed {
            Ok(()) => (),
            Err(ContextError::Invalid(error)) => return Err(error),
            Err(ContextError::Unsupported(reason)) => {
                result.diagnostics.push(reason);
                result.unparsed_span = Some(span(start, data.len()));
                return Ok(result);
            }
        }
    }
    result.complete = true;
    Ok(result)
}

/// Decode paper-space names/scales and explicit referenced block identifiers.
/// Coordinates, units, UCS, view transforms, and attached tables are not inferred.
pub fn decode_space_context(
    data: &[u8],
    product_build: &str,
    options: ParseOptions,
) -> Result<SpaceContext, Error> {
    let mut r = stream_reader(data, options)?;
    let mut result = SpaceContext {
        budget_used: 0,
        evidence: "native_static_and_corpus_consistent",
        complete: false,
        spaces: Vec::new(),
        other_records: Vec::new(),
        diagnostics: Vec::new(),
        unparsed_span: None,
    };
    let Some(major) = major(product_build) else {
        result
            .diagnostics
            .push(format!("Unqualified space product build {product_build}"));
        result.unparsed_span = Some(span(r.offset, data.len()));
        return Ok(result);
    };
    let mut used = 0;
    let mut ids = HashSet::new();
    while r.offset < data.len() {
        let start = r.offset;
        let parsed = (|| -> ContextResult<()> {
            let record = frame(&mut r, &mut used, Kind::Space)?;
            if !ids.insert(record.id) {
                return Err(invalid("duplicate space context ID").into());
            }
            if record.type_id == PAPER {
                result
                    .spaces
                    .push(decode_space(data, &record, major, options)?);
            } else {
                result.other_records.push(record);
            }
            Ok(())
        })();
        result.budget_used = used;
        match parsed {
            Ok(()) => (),
            Err(ContextError::Invalid(error)) => return Err(error),
            Err(ContextError::Unsupported(reason)) => {
                result.diagnostics.push(reason);
                result.unparsed_span = Some(span(start, data.len()));
                return Ok(result);
            }
        }
    }
    result.complete = true;
    Ok(result)
}

/// Decode block definitions from an already framed stream. The caller retains
/// the record-walk completion status; this function never infers omitted blocks.
pub fn decode_block_context(
    data: &[u8],
    records: &[RecordHeader],
    product_build: &str,
    options: ParseOptions,
) -> Result<BlockContext, Error> {
    ParseOptions::check_limit("max_records", records.len() as u64, options.max_records)?;
    decode_block_context_with_budget(data, records, product_build, options)
}

/// The document assembler has already charged the outer record walk. Only
/// embedded records and their contents consume this remaining record budget.
pub(crate) fn decode_block_context_with_budget(
    data: &[u8],
    records: &[RecordHeader],
    product_build: &str,
    options: ParseOptions,
) -> Result<BlockContext, Error> {
    ParseOptions::check_limit(
        "max_decoded_stream_bytes",
        data.len() as u64,
        options.max_decoded_stream_bytes,
    )?;
    let mut result = BlockContext {
        budget_used: 0,
        evidence: "native_static_and_corpus_consistent",
        complete: false,
        blocks: Vec::new(),
        other_records: Vec::new(),
        diagnostics: Vec::new(),
        unparsed_span: None,
    };
    let Some(major) = major(product_build) else {
        result
            .diagnostics
            .push(format!("Unqualified block product build {product_build}"));
        result.unparsed_span = Some(span(0, data.len()));
        return Ok(result);
    };
    let mut ids = HashSet::new();
    for record in records.iter().filter(|record| record.type_id == BLOCK) {
        let parsed = (|| -> ContextResult<Block> {
            if !matches!(record.object_schema, 1 | 100) {
                return Err(ContextError::Unsupported("Unqualified block schema".into()));
            }
            let mut r = body_reader(data, &record.payload_span, options)?;
            let name = text(&mut r)?;
            let unknown_start = r.offset;
            let current_ucs_id = i32_value(&mut r)?;
            let mut block = Block {
                solid_attachment: None,
                solid_geometry: None,
                solid_attributes: None,
                complete: false,
                diagnostics: Vec::new(),
                current_ucs_id,
                id: record.id_raw,
                owner: record.owner_raw,
                span: record.span.clone(),
                payload_span: record.payload_span.clone(),
                name,
                unknown_fields: vec![span(unknown_start, r.data.len())],
            };
            // A referenced UCS is retained; no coordinate transform is applied.
            let rest = (|| -> ContextResult<()> {
                r.take(4)?; // AllowExplode and UnionScale VARIANT_BOOL fields.
                match r.take(1)?[0] {
                    0 => (),
                    1 => {
                        block.solid_attachment =
                            Some(embedded_record(&mut r, &mut result.budget_used, SOLID, 1)?.0);
                        let record = block.solid_attachment.as_ref().unwrap();
                        let header = embedded_header(record);
                        let profile = crate::RecordProfile::for_major(major);
                        let remaining = ParseOptions {
                            max_records: options.max_records.saturating_sub(result.budget_used),
                            max_record_depth: options.max_record_depth.saturating_sub(1),
                            ..options
                        };
                        match crate::complex::decode_complex(
                            data, &header, profile, major, remaining,
                        )
                        .expect("qualified embedded SOLID class")
                        {
                            Ok(entity) => {
                                budget(&mut result.budget_used, entity.budget_used, options)?;
                                block.solid_attributes = Some(entity.attributes);
                                if let crate::complex::ComplexGeometry::Solid(geometry) =
                                    entity.geometry
                                {
                                    block.solid_geometry = Some(geometry);
                                }
                            }
                            Err(crate::complex::ComplexError::Limit(error)) => {
                                return Err(error.into())
                            }
                            Err(crate::complex::ComplexError::Unsupported(reason)) => {
                                block
                                    .diagnostics
                                    .push(format!("Embedded CxDbSolid: {reason}"));
                            }
                        }
                    }
                    _ => {
                        return Err(ContextError::Unsupported(
                            "Unknown block object presence flag".into(),
                        ))
                    }
                }
                if major > 10 {
                    r.take(10)?;
                    let xref_path = text(&mut r)?;
                    text(&mut r)?; // Comments.
                    if !xref_path.value.is_empty() {
                        return Err(ContextError::Unsupported(
                            "External block reference is not followed".into(),
                        ));
                    }
                    if r.take(1)?[0] != 0 {
                        return Err(ContextError::Unsupported(
                            "Optional block suffix object is not decoded".into(),
                        ));
                    }
                }
                end_body(&r)
            })();
            match rest {
                Ok(()) => {
                    block.complete =
                        block.solid_attachment.is_none() || block.solid_geometry.is_some();
                }
                Err(ContextError::Invalid(error)) => return Err(error.into()),
                Err(ContextError::Unsupported(reason)) => block.diagnostics.push(reason),
            }
            Ok(block)
        })();
        match parsed {
            Ok(block) => {
                if !ids.insert(block.id) {
                    return Err(invalid("duplicate block definition ID"));
                }
                if !block.complete {
                    result.diagnostics.extend(
                        block
                            .diagnostics
                            .iter()
                            .map(|reason| format!("Block {}: {reason}", block.id)),
                    );
                    if result.unparsed_span.is_none() {
                        result.unparsed_span = Some(block.payload_span.clone());
                    }
                }
                result.blocks.push(block);
            }
            Err(ContextError::Invalid(error)) => return Err(error),
            Err(ContextError::Unsupported(reason)) => {
                result
                    .diagnostics
                    .push(format!("Block {}: {reason}", record.id_raw));
                if result.unparsed_span.is_none() {
                    result.unparsed_span = Some(record.span.clone());
                }
            }
        }
    }
    result.complete = result.diagnostics.is_empty();
    Ok(result)
}

/// Stored block insertion data. Matrices are kept in serialized order; this
/// reader does not apply a matrix or infer a block base origin.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "Insert")]
pub struct Insert {
    pub block_id: i32,
    pub block_status: u32,
    pub insertion: [f64; 3],
    pub rotation: f64,
    pub scales: [f64; 3],
    pub matrix_stored: [f64; 16],
    pub attached_attributes: Vec<AttachedAttribute>,
    pub nickname: TextField,
    pub hide_flag_raw: u16,
    pub clip_flag_raw: Option<u16>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InsertEntity {
    pub attributes: crate::EntityAttributes,
    pub geometry: Insert,
    pub budget_used: u64,
}
#[derive(Debug)]
pub enum InsertError {
    Unsupported(String),
    Limit(Error),
}
impl From<Error> for InsertError {
    fn from(error: Error) -> Self {
        if matches!(error, Error::ResourceLimit { .. }) {
            Self::Limit(error)
        } else {
            Self::Unsupported(error.to_string())
        }
    }
}
impl From<String> for InsertError {
    fn from(reason: String) -> Self {
        Self::Unsupported(reason)
    }
}
impl From<&str> for InsertError {
    fn from(reason: &str) -> Self {
        Self::Unsupported(reason.into())
    }
}

/// Decode plain INSERTs without embedded display objects, attached attribute
/// entities, or clipping curves. Unsupported variants remain bounded records.
pub fn decode_insert(
    data: &[u8],
    header: &RecordHeader,
    profile: crate::RecordProfile,
    product_build: &str,
    options: ParseOptions,
) -> Option<Result<InsertEntity, InsertError>> {
    if header.type_id != "d38b4eba-a81f-4b28-aa19-71a60d884004" {
        return None;
    }
    Some((|| {
        let major = major(product_build).ok_or("Unqualified INSERT product build")?;
        let width = match (profile, major) {
            (crate::RecordProfile::Schema3Attributes33, 10..=17) => 33,
            (crate::RecordProfile::Schema3Attributes37, 18) => 37,
            _ => return Err("Unqualified INSERT attribute profile".into()),
        };
        if !matches!(header.object_schema, 1 | 100) || header.extension.is_some() {
            return Err("Unqualified INSERT schema/extension".into());
        }
        let attributes = crate::geometry::decode_attributes(data, &header.payload_span, profile)?;
        let mut r = body_reader(data, &header.payload_span, options)?;
        r.take(width)?;
        let start = r.offset;
        let mut used = 0;
        let displays = r.u32()?;
        budget(&mut used, u64::from(displays), options)?;
        if displays != 0 {
            return Err("INSERT contains embedded display objects".into());
        }
        let rotation = f64_value(&mut r)?;
        let mut scales = [0.; 3];
        for value in &mut scales {
            *value = f64_value(&mut r)?;
        }
        let mut insertion = [0.; 3];
        for value in &mut insertion {
            *value = f64_value(&mut r)?;
        }
        let mut matrix_stored = [0.; 16];
        for value in &mut matrix_stored {
            *value = f64_value(&mut r)?;
        }
        let block_status = r.u32()?;
        let block_id = i32_value(&mut r)?;
        let attribute_count = r.u32()?;
        budget(&mut used, u64::from(attribute_count), options)?;
        if attribute_count as usize > (r.data.len() - r.offset) / 30 {
            return Err("INSERT attribute count exceeds bounded payload".into());
        }
        // Each child record is charged by embedded_record, not twice by count.
        used -= u64::from(attribute_count);
        let mut attached_attributes = Vec::new();
        for _ in 0..attribute_count {
            let (record, properties_have_values) =
                embedded_record(&mut r, &mut used, ATTRIBUTE, 1)?;
            attached_attributes.push(decode_attached_attribute(
                data,
                record,
                properties_have_values,
                profile,
                major,
                options,
                &mut used,
            )?);
        }
        let nickname = text(&mut r)?;
        let hide_flag_raw = r.u16()?;
        let clip_flag_raw = if major > 10 {
            let flag = r.u16()?;
            let clips = r.u32()?;
            budget(&mut used, u64::from(clips), options)?;
            if flag != 0 || clips != 0 {
                return Err("INSERT clipping is not decoded".into());
            }
            Some(flag)
        } else {
            None
        };
        if r.offset != r.data.len() {
            return Err("Unqualified INSERT suffix".into());
        }
        Ok(InsertEntity {
            attributes,
            budget_used: used,
            geometry: Insert {
                block_id,
                block_status,
                insertion,
                rotation,
                scales,
                matrix_stored,
                attached_attributes,
                nickname,
                hide_flag_raw,
                clip_flag_raw,
                span: span(start, r.offset),
            },
        })
    })())
}

const ATTRIBUTE: &str = "60d85baf-4d2e-4000-84f0-e77d42fb999c";
const ATTRIBUTE_DEFINITION: &str = "e218226d-dd73-4a9d-aa95-764b4d3bc072";
const TEXT: &str = "76d76221-4358-465d-bc53-4449b3004657";
const SOLID: &str = "d7676d4e-b9e0-46c3-894d-e9ef7fb66fb2";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttachedAttribute {
    pub record: ContextRecord,
    pub attributes: crate::EntityAttributes,
    pub tag: TextField,
    pub value: TextField,
    pub text: crate::annotation::TextGeometry,
    pub text_attributes: crate::EntityAttributes,
    pub text_record: ContextRecord,
    /// Stored word (major >10). Native post-load normalization is not applied.
    pub mode_raw: Option<u32>,
    /// Native mode bit 30 inverted. Legacy records with property values remain
    /// unknown because CAXA_AttrOfMode can override the constructor default.
    pub visibility: Option<bool>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "AttributeDefinition")]
pub struct AttributeDefinition {
    pub tag: TextField,
    pub prompt: TextField,
    pub default_value: TextField,
    pub text: crate::annotation::TextGeometry,
    pub text_attributes: crate::EntityAttributes,
    pub text_record: ContextRecord,
    pub mode_raw: Option<u32>,
    /// Native mode bit 30 inverted, or the native visible constructor default
    /// when no mode is stored and the property groups contain no values.
    /// Property-bearing definitions remain unknown (native post-load override).
    pub visibility: Option<bool>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttributeDefinitionEntity {
    pub attributes: crate::EntityAttributes,
    pub geometry: AttributeDefinition,
    pub budget_used: u64,
}
impl From<ContextError> for InsertError {
    fn from(error: ContextError) -> Self {
        match error {
            ContextError::Invalid(e) => e.into(),
            ContextError::Unsupported(s) => Self::Unsupported(s),
        }
    }
}
fn embedded_record(
    r: &mut Reader<'_>,
    used: &mut u64,
    expected: &str,
    depth: u64,
) -> ContextResult<(ContextRecord, bool)> {
    ParseOptions::check_limit(
        "max_record_depth",
        depth,
        r.options.max_record_depth.min(64),
    )?;
    budget(used, 1, r.options)?;
    let start = r.offset;
    let type_id = guid(r)?;
    let id = i32_value(r)?;
    let owner = i32_value(r)?;
    let object_schema = r.take(1)?[0];
    let presence = r.take(1)?[0];
    if type_id != expected || !matches!(object_schema, 1 | 100) || presence > 1 {
        return Err(ContextError::Unsupported(format!(
            "Unqualified embedded record {type_id}/{object_schema}/{presence}"
        )));
    }
    let mut properties_have_values = false;
    let property_span = if presence == 1 {
        let p = r.offset;
        properties_have_values = skip_properties(r, used)?;
        Some(span(p, r.offset))
    } else {
        None
    };
    let size = r.u32()? as usize;
    let start_body = r.offset;
    r.take(size)?;
    let class_name = match expected {
        ATTRIBUTE => "CxAttribute",
        ATTRIBUTE_DEFINITION => "CxAttributeDefine",
        TEXT => "CxDbText",
        SOLID => "CxDbSolid",
        _ => unreachable!(),
    };
    Ok((
        ContextRecord {
            class_name: Some(class_name.into()),
            type_id,
            id,
            owner,
            object_schema,
            span: span(start, r.offset),
            payload_span: span(start_body, r.offset),
            property_span,
            extension_span: None,
        },
        properties_have_values,
    ))
}
fn embedded_text(
    data: &[u8],
    r: &mut Reader<'_>,
    used: &mut u64,
    profile: crate::RecordProfile,
    major: u32,
    depth: u64,
) -> Result<
    (
        ContextRecord,
        crate::EntityAttributes,
        crate::annotation::TextGeometry,
    ),
    InsertError,
> {
    let (record, _) = embedded_record(r, used, TEXT, depth)?;
    let header = embedded_header(&record);
    let options = ParseOptions {
        max_records: r.options.max_records.saturating_sub(*used),
        ..r.options
    };
    let result = crate::annotation::decode_annotation(data, &header, profile, major, options)
        .expect("qualified embedded TEXT class");
    let entity = match result {
        Ok(entity) => entity,
        Err(crate::annotation::AnnotationError::Limit(e)) => return Err(InsertError::Limit(e)),
        Err(crate::annotation::AnnotationError::Unsupported(s)) => {
            return Err(InsertError::Unsupported(format!("Embedded TEXT: {s}")))
        }
    };
    budget(used, entity.budget_used, r.options)?;
    match entity.geometry {
        crate::AnnotationGeometry::Text(text) => Ok((record, entity.attributes, text)),
        _ => unreachable!("qualified embedded TEXT class"),
    }
}
fn embedded_header(record: &ContextRecord) -> RecordHeader {
    RecordHeader {
        type_id: record.type_id.clone(),
        class_name: record.class_name.clone(),
        id_raw: record.id,
        owner_raw: record.owner,
        schema: u16::from(record.object_schema) | (u16::from(record.property_span.is_some()) << 8),
        object_schema: record.object_schema,
        properties_present: record.property_span.is_some(),
        property_groups: Vec::new(),
        span: record.span.clone(),
        payload_span: record.payload_span.clone(),
        extension: None,
    }
}
fn decode_attached_attribute(
    data: &[u8],
    record: ContextRecord,
    properties_have_values: bool,
    profile: crate::RecordProfile,
    major: u32,
    options: ParseOptions,
    used: &mut u64,
) -> Result<AttachedAttribute, InsertError> {
    let attributes = crate::geometry::decode_attributes(data, &record.payload_span, profile)?;
    let mut r = body_reader(data, &record.payload_span, options)?;
    r.take(attributes.span.length as usize)?;
    let start = r.offset;
    let tag = text(&mut r)?;
    let value = text(&mut r)?;
    let (text_record, text_attributes, text) =
        embedded_text(data, &mut r, used, profile, major, 2)?;
    let mode_raw = if major > 10 { Some(r.u32()?) } else { None };
    let visibility = attribute_visibility(mode_raw, properties_have_values, false);
    if r.offset != r.data.len() {
        return Err("Unqualified attached attribute suffix".into());
    }
    Ok(AttachedAttribute {
        record,
        attributes,
        tag,
        value,
        text,
        text_attributes,
        text_record,
        mode_raw,
        visibility,
        span: span(start, r.offset),
    })
}
fn attribute_visibility(
    mode_raw: Option<u32>,
    properties_have_values: bool,
    definition: bool,
) -> Option<bool> {
    if properties_have_values && (definition || mode_raw.is_none()) {
        None
    } else {
        // Native constructors use 0x80000001. A stored zero is normalized to
        // that same value, which does not change its visibility bit.
        Some(mode_raw.unwrap_or(0x80000001) & (1 << 30) == 0)
    }
}
/// Decode a bounded definition, retaining tag/prompt/default and its embedded
/// stored TEXT. A definition is not automatically a displayed INSERT attribute.
pub fn decode_attribute_definition(
    data: &[u8],
    header: &RecordHeader,
    profile: crate::RecordProfile,
    product_build: &str,
    options: ParseOptions,
) -> Option<Result<AttributeDefinitionEntity, InsertError>> {
    if header.type_id != ATTRIBUTE_DEFINITION {
        return None;
    }
    Some((|| {
        let major = major(product_build).ok_or("Unqualified attribute-definition product build")?;
        let qualified = matches!(
            (profile, major),
            (crate::RecordProfile::Schema3Attributes33, 10..=17)
                | (crate::RecordProfile::Schema3Attributes37, 18)
        );
        if !qualified || !matches!(header.object_schema, 1 | 100) || header.extension.is_some() {
            return Err("Unqualified attribute-definition profile/schema".into());
        }
        let attributes = crate::geometry::decode_attributes(data, &header.payload_span, profile)?;
        let mut r = body_reader(data, &header.payload_span, options)?;
        r.take(attributes.span.length as usize)?;
        let start = r.offset;
        let mut used = 0;
        let tag = text(&mut r)?;
        let prompt = text(&mut r)?;
        let default_value = text(&mut r)?;
        let (text_record, text_attributes, text) =
            embedded_text(data, &mut r, &mut used, profile, major, 1)?;
        let mode_raw = if major > 10 { Some(r.u32()?) } else { None };
        let visibility = attribute_visibility(
            mode_raw,
            header.property_groups.iter().any(|g| !g.values.is_empty()),
            true,
        );
        if r.offset != r.data.len() {
            return Err("Unqualified attribute-definition suffix".into());
        }
        Ok(AttributeDefinitionEntity {
            attributes,
            budget_used: used,
            geometry: AttributeDefinition {
                tag,
                prompt,
                default_value,
                text,
                text_attributes,
                text_record,
                mode_raw,
                visibility,
                span: span(start, r.offset),
            },
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(data: &mut Vec<u8>, value: &str) {
        let words: Vec<_> = value.encode_utf16().chain([0]).collect();
        data.extend(((words.len() * 2) as u32).to_le_bytes());
        for word in words {
            data.extend(word.to_le_bytes());
        }
    }
    fn header() -> Vec<u8> {
        let mut data = Vec::new();
        string(&mut data, "0.0");
        data.resize(172, 0);
        data
    }
    fn append(data: &mut Vec<u8>, type_id: &str, id: i32, schema: u8, body: &[u8]) {
        data.extend(uuid::Uuid::parse_str(type_id).unwrap().to_bytes_le());
        data.extend(id.to_le_bytes());
        data.extend((-23i32).to_le_bytes());
        data.extend([schema, 0]);
        data.extend((body.len() as u32).to_le_bytes());
        data.extend(body);
    }
    fn layer_body(major: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for value in ["Visible", "Visible alternate", "visible-key"] {
            string(&mut data, value);
        }
        data.extend(0x00123456u32.to_le_bytes());
        data.extend(0x50000000u32.to_le_bytes());
        string(&mut data, "description");
        data.extend(0.25f32.to_le_bytes());
        data.extend(3u32.to_le_bytes());
        data.extend(0x800000u32.to_le_bytes());
        data.extend((-19i32).to_le_bytes());
        if major >= 18 {
            data.extend(123u32.to_le_bytes());
        }
        data
    }
    #[test]
    fn layer_native_fields_and_profile_width() {
        for (build, major) in [("10.0.0.0000", 10), ("18.0.0.18159", 18)] {
            let mut data = header();
            append(&mut data, LAYER, -24, 1, &layer_body(major));
            let context = decode_style_context(&data, build, ParseOptions::default()).unwrap();
            assert!(context.complete);
            let layer = &context.layers[0];
            assert_eq!(layer.name.value, "Visible");
            assert_eq!(layer.color_raw, 0x123456);
            assert_eq!(layer.width, 0.25);
            assert_eq!(layer.linetype_id, -19);
            assert!(layer.on && layer.printable);
            assert!(!layer.locked && !layer.frozen);
            assert_eq!(layer.unknown_fields.len(), usize::from(major >= 18));
            assert_eq!(layer.span.offset, 172);
        }
    }
    #[test]
    fn unknown_record_and_advanced_linetype_stop_before_whole_record() {
        let mut data = header();
        append(&mut data, LAYER, -24, 1, &layer_body(10));
        let offset = data.len();
        append(&mut data, "00000000-0000-0000-0000-000000000000", 1, 1, &[]);
        let result = decode_style_context(&data, "10.0.0.0000", ParseOptions::default()).unwrap();
        assert!(!result.complete);
        assert_eq!(result.layers.len(), 1);
        assert_eq!(result.unparsed_span.unwrap().offset, offset as u64);
        let mut body = Vec::new();
        string(&mut body, "Advanced");
        string(&mut body, "description");
        body.extend(1f32.to_le_bytes());
        body.extend(0u32.to_le_bytes());
        string(&mut body, "Advanced");
        body.push(1);
        let mut data = header();
        append(&mut data, LINETYPE, -19, 1, &body);
        let result = decode_style_context(&data, "18.0.0.18159", ParseOptions::default()).unwrap();
        assert!(!result.complete);
        assert!(result.linetypes.is_empty());
        assert_eq!(result.unparsed_span.unwrap().offset, 172);
    }
    #[test]
    fn bounded_body_does_not_borrow_next_record_and_counts_are_limited() {
        let mut data = header();
        let mut body = layer_body(10);
        body.pop();
        append(&mut data, LAYER, -24, 1, &body);
        append(&mut data, LAYER, -25, 1, &layer_body(10));
        assert!(matches!(
            decode_style_context(&data, "10.0.0.0000", ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        let mut data = header();
        append(&mut data, LAYER, -24, 1, &layer_body(10));
        let options = ParseOptions {
            max_records: 0,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_style_context(&data, "10.0.0.0000", options),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
    }
    #[test]
    fn schema_two_extension_is_not_a_display_cache() {
        let mut data = header();
        append(
            &mut data,
            "a7b11825-b974-4383-86ed-7cea3cdf557c",
            1,
            2,
            &[42],
        );
        data.extend(uuid::Uuid::parse_str(RELATIONS).unwrap().to_bytes_le());
        data.extend(1u32.to_le_bytes());
        data.extend(
            uuid::Uuid::parse_str(SINGLE_RELATION)
                .unwrap()
                .to_bytes_le(),
        );
        data.extend([0; 8]);
        data.extend(0u16.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(0xf0000000u32.to_le_bytes());
        append(&mut data, LAYER, -24, 1, &layer_body(10));
        let result = decode_style_context(&data, "10.0.0.0000", ParseOptions::default()).unwrap();
        assert!(result.complete);
        assert_eq!(result.other_records.len(), 1);
        assert_eq!(result.layers.len(), 1);
        assert_eq!(
            result.other_records[0]
                .extension_span
                .as_ref()
                .unwrap()
                .length,
            54
        );
    }
    #[test]
    fn explicit_space_block_reference_and_variable_prefix_strings() {
        let mut body = vec![0; 120];
        for i in 0..10 {
            if i == 2 {
                string(&mut body, "prefix value");
            } else {
                body.extend(0u32.to_le_bytes());
            }
        }
        string(&mut body, "Sheet");
        body.extend(0i32.to_le_bytes());
        body.extend(2f64.to_le_bytes());
        body.extend([0; 32]);
        body.extend(0x800000u32.to_le_bytes());
        body.extend((-5i32).to_le_bytes());
        body.extend([0; 60]);
        let mut data = header();
        append(&mut data, PAPER, -7, 1, &body);
        let result = decode_space_context(&data, "10.0.0.0000", ParseOptions::default()).unwrap();
        assert!(result.complete);
        assert_eq!(result.spaces[0].block_id, -5);
        assert_eq!(result.spaces[0].scale, 2.0);
        assert_eq!(result.spaces[0].name.value, "Sheet");
    }
    #[test]
    fn partial_block_keeps_identity_and_explicit_ucs() {
        let mut body = Vec::new();
        string(&mut body, "Named block");
        body.extend(42i32.to_le_bytes());
        body.extend([0; 4]);
        body.push(1);
        body.extend([0; 24]);
        body.extend([1, 0]);
        body.extend(0u32.to_le_bytes());
        let mut data = header();
        append(&mut data, BLOCK, 10, 1, &body);
        let walk = crate::inspect_record_headers(&data, ParseOptions::default()).unwrap();
        let result =
            decode_block_context(&data, &walk.records, "10.0.0.0000", ParseOptions::default())
                .unwrap();
        assert!(!result.complete);
        assert_eq!(result.blocks.len(), 1);
        let block = &result.blocks[0];
        assert_eq!((block.id, block.current_ucs_id), (10, 42));
        assert_eq!(block.name.value, "Named block");
        assert!(!block.complete && !block.diagnostics.is_empty());
    }
    fn insert_body(major: u32) -> Vec<u8> {
        let mut body = vec![0; if major == 18 { 37 } else { 33 }];
        body.extend(0u32.to_le_bytes());
        for value in [0.125f64, 2., 3., 4., 5., 6., 7.] {
            body.extend(value.to_le_bytes());
        }
        for i in 0..16 {
            body.extend((i as f64 + 0.25).to_le_bytes());
        }
        body.extend(0x800000u32.to_le_bytes());
        body.extend(10i32.to_le_bytes());
        body.extend(0u32.to_le_bytes());
        string(&mut body, "Nick");
        body.extend(0u16.to_le_bytes());
        if major > 10 {
            body.extend([0; 6]);
        }
        body
    }
    #[test]
    fn insert_native_layout_keeps_stored_matrix_and_profile() {
        for (build, major, profile) in [
            ("10.0.0.0000", 10, crate::RecordProfile::Schema3Attributes33),
            (
                "18.0.0.18159",
                18,
                crate::RecordProfile::Schema3Attributes37,
            ),
        ] {
            let mut data = header();
            append(
                &mut data,
                "d38b4eba-a81f-4b28-aa19-71a60d884004",
                20,
                1,
                &insert_body(major),
            );
            let record = &crate::inspect_record_headers(&data, ParseOptions::default())
                .unwrap()
                .records[0];
            let decoded = decode_insert(&data, record, profile, build, ParseOptions::default())
                .unwrap()
                .unwrap();
            assert_eq!(decoded.geometry.insertion, [5., 6., 7.]);
            assert_eq!(decoded.geometry.scales, [2., 3., 4.]);
            assert_eq!(decoded.geometry.matrix_stored[15], 15.25);
            assert_eq!(decoded.geometry.rotation, 0.125);
            assert_eq!(decoded.geometry.nickname.value, "Nick");
            assert_eq!(decoded.geometry.block_id, 10);
            assert_eq!(
                decoded.geometry.clip_flag_raw,
                if major > 10 { Some(0) } else { None }
            );
        }
    }
    #[test]
    fn insert_embedded_count_limit_is_a_hard_error() {
        let mut body = insert_body(10);
        body[33..37].copy_from_slice(&2u32.to_le_bytes());
        let mut data = header();
        append(
            &mut data,
            "d38b4eba-a81f-4b28-aa19-71a60d884004",
            20,
            1,
            &body,
        );
        let record = &crate::inspect_record_headers(&data, ParseOptions::default())
            .unwrap()
            .records[0];
        let options = ParseOptions {
            max_records: 1,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_insert(
                &data,
                record,
                crate::RecordProfile::Schema3Attributes33,
                "10.0.0.0000",
                options
            ),
            Some(Err(InsertError::Limit(Error::ResourceLimit { .. })))
        ));
        assert!(matches!(
            decode_insert(
                &data,
                record,
                crate::RecordProfile::Schema3Attributes33,
                "10.0.0.0000",
                ParseOptions::default()
            ),
            Some(Err(InsertError::Unsupported(_)))
        ));
    }
    fn embedded_text_body(major: u32) -> Vec<u8> {
        let mut body = vec![0; if major == 18 { 37 } else { 33 }];
        string(&mut body, "displayed value");
        for value in [2.5f64, 0., 0., 0.] {
            body.extend(value.to_le_bytes());
        }
        body.push(0);
        body.extend(0u32.to_le_bytes());
        body.extend(1f64.to_le_bytes());
        body.extend(0x800000u32.to_le_bytes());
        body.extend((-20i32).to_le_bytes());
        for value in [40f64, -25., 0.] {
            body.extend(value.to_le_bytes());
        }
        if major <= 10 {
            body.extend(0u32.to_le_bytes());
        }
        body.extend(1f64.to_le_bytes());
        body.extend(0u32.to_le_bytes());
        if major >= 15 {
            body.extend([0; 14]);
        }
        if major > 10 {
            body.extend([0; 20]);
        }
        if major >= 15 {
            body.extend([0; 24]);
        }
        body
    }
    fn definition_body(major: u32, is_definition: bool) -> Vec<u8> {
        let mut body = vec![0; if major == 18 { 37 } else { 33 }];
        string(&mut body, "TAG");
        string(
            &mut body,
            if is_definition {
                "Prompt"
            } else {
                "saved value"
            },
        );
        if is_definition {
            string(&mut body, "Default");
        }
        append(&mut body, TEXT, 0, 1, &embedded_text_body(major));
        if major > 10 {
            body.extend(0x80000001u32.to_le_bytes());
        }
        body
    }
    #[test]
    fn definition_and_insert_attributes_keep_distinct_stored_text() {
        for (build, major, profile) in [
            ("10.0.0.0000", 10, crate::RecordProfile::Schema3Attributes33),
            (
                "18.0.0.18159",
                18,
                crate::RecordProfile::Schema3Attributes37,
            ),
        ] {
            let mut data = header();
            append(
                &mut data,
                ATTRIBUTE_DEFINITION,
                30,
                1,
                &definition_body(major, true),
            );
            let record = &crate::inspect_record_headers(&data, ParseOptions::default())
                .unwrap()
                .records[0];
            let d =
                decode_attribute_definition(&data, record, profile, build, ParseOptions::default())
                    .unwrap()
                    .unwrap();
            assert_eq!(d.geometry.tag.value, "TAG");
            assert_eq!(d.geometry.prompt.value, "Prompt");
            assert_eq!(d.geometry.default_value.value, "Default");
            assert_eq!(d.geometry.text.contents, "displayed value");
            assert_eq!(d.geometry.text.insertion, [40., -25., 0.]);
            assert_eq!(d.geometry.visibility, Some(true));
            assert_eq!(d.budget_used, 2);
            let mut body = insert_body(major);
            let count_offset = if major == 18 { 233 } else { 229 };
            body[count_offset..count_offset + 4].copy_from_slice(&1u32.to_le_bytes());
            let mut child = Vec::new();
            append(&mut child, ATTRIBUTE, 0, 1, &definition_body(major, false));
            body.splice(count_offset + 4..count_offset + 4, child);
            let mut data = header();
            append(
                &mut data,
                "d38b4eba-a81f-4b28-aa19-71a60d884004",
                20,
                1,
                &body,
            );
            let record = &crate::inspect_record_headers(&data, ParseOptions::default())
                .unwrap()
                .records[0];
            let i = decode_insert(&data, record, profile, build, ParseOptions::default())
                .unwrap()
                .unwrap();
            assert_eq!(i.geometry.attached_attributes.len(), 1);
            assert_eq!(i.budget_used, 3);
            let a = &i.geometry.attached_attributes[0];
            assert_eq!(a.value.value, "saved value");
            assert_eq!(a.text.contents, "displayed value");
            assert_eq!(a.text.insertion, [40., -25., 0.]); // No insertion matrix applied.
            assert_eq!(a.visibility, Some(true));
            let options = ParseOptions {
                max_record_depth: 1,
                ..ParseOptions::default()
            };
            assert!(matches!(
                decode_insert(&data, record, profile, build, options),
                Some(Err(InsertError::Limit(Error::ResourceLimit {
                    name: "max_record_depth",
                    ..
                })))
            ));
            let options = ParseOptions {
                max_records: 2,
                ..ParseOptions::default()
            };
            assert!(matches!(
                decode_insert(&data, record, profile, build, options),
                Some(Err(InsertError::Limit(Error::ResourceLimit {
                    name: "max_records",
                    ..
                })))
            ));
        }
    }
    #[test]
    fn attribute_visibility_keeps_native_property_overrides_unknown() {
        assert_eq!(attribute_visibility(None, false, false), Some(true));
        assert_eq!(attribute_visibility(None, true, false), None);
        assert_eq!(attribute_visibility(Some(0), false, false), Some(true));
        assert_eq!(
            attribute_visibility(Some(0xc0000001), false, false),
            Some(false)
        );
        assert_eq!(
            attribute_visibility(Some(0xc0000001), true, false),
            Some(false)
        );
        // Definition Load49 consults CAXA_AttrOfMode for every stream version.
        assert_eq!(attribute_visibility(Some(0x80000001), true, true), None);
        assert_eq!(attribute_visibility(None, false, true), Some(true));
    }
    #[test]
    fn solid_attachment_is_framed_but_not_claimed_as_geometry() {
        let mut body = Vec::new();
        string(&mut body, "Block");
        body.extend([0; 8]);
        body.push(1);
        append(&mut body, SOLID, 0, 1, &[1, 2, 3]);
        let mut data = header();
        append(&mut data, BLOCK, 10, 1, &body);
        let walk = crate::inspect_record_headers(&data, ParseOptions::default()).unwrap();
        let result =
            decode_block_context(&data, &walk.records, "10.0.0.0000", ParseOptions::default())
                .unwrap();
        assert_eq!(result.budget_used, 1);
        assert!(!result.complete);
        assert_eq!(
            result.blocks[0]
                .solid_attachment
                .as_ref()
                .unwrap()
                .class_name
                .as_deref(),
            Some("CxDbSolid")
        );
        assert_eq!(
            result.blocks[0]
                .solid_attachment
                .as_ref()
                .unwrap()
                .payload_span
                .length,
            3
        );
    }
    #[test]
    fn block_solid_geometry_and_nested_budget_are_separate_from_children() {
        let mut solid = vec![0; 33];
        solid.extend([0; 24]); // Stored pick point.
        for value in [1u32, 1, 1011] {
            solid.extend(value.to_le_bytes());
        }
        for value in [1.0f64, 2., 0., 3., 4., 0.] {
            solid.extend(value.to_le_bytes());
        }
        let mut body = Vec::new();
        string(&mut body, "Solid border");
        body.extend([0; 8]);
        body.push(1);
        append(&mut body, SOLID, 0, 1, &solid);
        let mut data = header();
        append(&mut data, BLOCK, 10, 1, &body);
        let walk = crate::inspect_record_headers(&data, ParseOptions::default()).unwrap();
        let result =
            decode_block_context(&data, &walk.records, "10.0.0.0000", ParseOptions::default())
                .unwrap();
        assert!(result.complete);
        assert_eq!(result.budget_used, 3); // One child record, one loop, one line.
        assert_eq!(
            result.blocks[0]
                .solid_geometry
                .as_ref()
                .unwrap()
                .boundary
                .loops[0]
                .curves
                .len(),
            1
        );
        let options = ParseOptions {
            max_records: 2,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_block_context(&data, &walk.records, "10.0.0.0000", options),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
    }
}

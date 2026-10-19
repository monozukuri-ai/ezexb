//! A limited reader for stored entities and their context, not a CAXA renderer.
use crate::{
    annotation::decode_annotation, geometry::decode_primitive,
    records::inspect_record_headers_with_budget, *,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub const READ_PROFILE: &str = "exb-stored-planar-v1";
pub const READ_PRODUCT_BUILDS: &[&str] = &[
    "10.0.0.0000",
    "11.7.0.5203",
    "15.0.0.5681",
    "16.4.0.13104",
    "18.0.0.18159",
];
const BLOCK_TYPE: &str = "b65f3081-5c0e-4d00-97b4-0c5231051af1";

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub stream_path: Option<String>,
    /// Relative to the decoded stream identified by stream_path.
    pub span: Option<SourceSpan>,
    pub record_id: Option<i32>,
    pub type_id: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReadReport {
    pub profile: String,
    /// Complete only within the documented stored-entity profile.
    pub complete: bool,
    pub decoded_entity_count: u64,
    pub unsupported_entity_count: u64,
    pub top_level_record_count: u64,
    pub cached_record_count: u64,
    pub decoded_bytes: u64,
    pub budget_used: u64,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Serialize)]
pub struct EntitySource {
    pub stream_path: String,
    pub span: SourceSpan,
    pub payload_span: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct StyleValue<T> {
    /// by_layer or unresolved; other inheritance modes retain raw flags.
    pub source: String,
    pub value: Option<T>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedStyle {
    pub layer_id: i32,
    pub layer_name: Option<String>,
    pub linetype: StyleValue<i32>,
    /// Stored native color encoding, not an RGB conversion.
    pub color_raw: StyleValue<u32>,
    pub line_width: StyleValue<f64>,
    /// Preserved entity scale; no unit conversion or dash rendering.
    pub linetype_scale: f64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Geometry {
    Primitive(PrimitiveGeometry),
    Annotation(AnnotationGeometry),
    Insert(Insert),
    AttributeDefinition(Box<AttributeDefinition>),
    Complex(ComplexGeometry),
    PartNumber(PartNumberGeometry),
    BomTable(Box<BomTableGeometry>),
    View2D(Box<View2DGeometry>),
    Table(Box<TableGeometry>),
}
#[derive(Clone, Debug, Serialize)]
pub struct Entity {
    pub id: i32,
    pub owner_id: i32,
    pub block_id: Option<i32>,
    pub type_id: String,
    pub object_schema: u8,
    pub source: EntitySource,
    pub attributes: EntityAttributes,
    pub style: ResolvedStyle,
    pub geometry: Geometry,
    /// Bounded application properties remain uninterpreted, with source spans.
    pub properties: Vec<RecordPropertyGroup>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DrawingBlock {
    pub definition: Block,
    pub stream_path: String,
    /// Indexes in Document.entities; display caches are never included.
    pub entity_indices: Vec<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Document {
    pub profile: String,
    pub evidence: String,
    pub file_info: FileInfo,
    pub coordinate_system: String,
    /// None means no physical-unit interpretation has been established.
    pub units: Option<String>,
    pub styles: StyleContext,
    pub text_style_context: TextStyleContext,
    pub space_context: SpaceContext,
    pub blocks: Vec<DrawingBlock>,
    pub entities: Vec<Entity>,
    /// Streams outside this profile, listed rather than silently discarded.
    pub uninterpreted_streams: Vec<EntryInfo>,
    pub report: ReadReport,
}
impl Document {
    pub fn entities_in_block(&self, id: i32) -> impl Iterator<Item = &Entity> {
        self.entities
            .iter()
            .filter(move |entity| entity.block_id == Some(id))
    }
    pub fn entities_in_space(&self, id: i32) -> Option<impl Iterator<Item = &Entity>> {
        self.space_context
            .spaces
            .iter()
            .find(|space| space.id == id)
            .map(|space| self.entities_in_block(space.block_id))
    }
}
pub fn read_file(
    path: impl AsRef<Path>,
    options: ParseOptions,
    strict: bool,
) -> Result<Document, Error> {
    Container::open(path, options)?.document(strict)
}
pub fn read_bytes(data: &[u8], options: ParseOptions, strict: bool) -> Result<Document, Error> {
    Container::from_bytes(data, options)?.document(strict)
}
struct Budget {
    options: ParseOptions,
    decoded: u64,
    items: u64,
}
impl Budget {
    fn remaining(&self) -> ParseOptions {
        ParseOptions {
            max_records: self.options.max_records.saturating_sub(self.items),
            ..self.options
        }
    }
    fn charge(&mut self, count: u64) -> Result<(), Error> {
        self.items = self.items.saturating_add(count);
        ParseOptions::check_limit("max_records", self.items, self.options.max_records)
    }
    fn payload(&mut self, container: &mut Container, path: &str) -> Result<Vec<u8>, Error> {
        let bytes = container.read_stream(path, 0, None)?;
        // Inspect only the bounded envelope header before any decompression allocation.
        let required = if bytes.starts_with(b"\x00\xde\xe3\x3d") && bytes.len() >= 16 {
            let length = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
            length.div_ceil(262144).saturating_mul(262144)
        } else {
            bytes.len() as u64
        };
        ParseOptions::check_limit(
            "max_total_decoded_bytes",
            self.decoded.saturating_add(required),
            self.options.max_total_decoded_bytes,
        )?;
        let payload = decode_stream_payload(&bytes, self.remaining())?;
        self.decoded = self.decoded.saturating_add(payload.info.allocated_size);
        self.charge(payload.info.blocks.len() as u64)?;
        Ok(payload.data)
    }
}
fn issue(
    report: &mut ReadReport,
    code: &str,
    message: impl Into<String>,
    path: Option<&str>,
    span: Option<SourceSpan>,
    record: Option<&RecordHeader>,
) {
    report.diagnostics.push(Diagnostic {
        code: code.into(),
        message: message.into(),
        stream_path: path.map(Into::into),
        span,
        record_id: record.map(|r| r.id_raw),
        type_id: record.map(|r| r.type_id.clone()),
    });
}
fn cached_count(records: &[RecordHeader]) -> u64 {
    records
        .iter()
        .filter_map(|r| r.extension.as_ref())
        .map(|ext| {
            ext.display_cache.records.len() as u64 + cached_count(&ext.display_cache.records)
        })
        .sum()
}
fn presentation_diagnostics(
    report: &mut ReadReport,
    records: &[PresentationRecord],
    path: &str,
    owner: &RecordHeader,
    code: &str,
) {
    for child in records {
        if matches!(
            child.geometry,
            Some(PresentationGeometry::Primitive(
                PrimitiveGeometry::DegenerateArc { .. }
            ))
        ) {
            issue(
                report,
                "degenerate_geometry",
                "Stored ARC has zero radius and zero basis; no valid curve is inferred.",
                Some(path),
                Some(child.header.payload_span.clone()),
                Some(owner),
            );
        }
        if let Some(reason) = &child.unsupported_reason {
            issue(
                report,
                code,
                reason,
                Some(path),
                Some(child.header.span.clone()),
                Some(owner),
            );
        }
        if let Some(PresentationGeometry::PartItem(item)) = &child.geometry {
            presentation_diagnostics(report, &item.presentation_records, path, owner, code);
        }
    }
}
struct StyleIndex<'a> {
    layers: HashMap<i32, &'a Layer>,
    linetypes: HashSet<i32>,
    text_styles: HashSet<i32>,
}
impl<'a> StyleIndex<'a> {
    fn new(styles: &'a StyleContext) -> Self {
        Self {
            layers: styles
                .layers
                .iter()
                .map(|layer| (layer.id, layer))
                .collect(),
            linetypes: styles.linetypes.iter().map(|value| value.id).collect(),
            text_styles: styles
                .other_records
                .iter()
                .filter(|record| record.type_id == "b9bd09d2-0eaa-4fe7-9ff6-a23f19fe8954")
                .map(|record| record.id)
                .collect(),
        }
    }
}
fn resolve_style(
    attributes: &EntityAttributes,
    styles: &StyleIndex<'_>,
    report: &mut ReadReport,
    path: &str,
    record: &RecordHeader,
) -> ResolvedStyle {
    let layer = styles.layers.get(&attributes.layer_id_raw).copied();
    let resolution = resolve_entity_style(attributes, layer, None);
    for diagnostic in resolution.diagnostics {
        issue(
            report,
            &diagnostic.code,
            diagnostic.message,
            Some(path),
            Some(attributes.span.clone()),
            Some(record),
        );
    }
    if let Some(id) = resolution.style.linetype.value {
        if !styles.linetypes.contains(&id) {
            issue(
                report,
                "unresolved_linetype",
                format!("Linetype {id} is unavailable."),
                Some(path),
                Some(attributes.span.clone()),
                Some(record),
            );
        }
    }
    resolution.style
}

pub(crate) fn read_container(container: &mut Container, strict: bool) -> Result<Document, Error> {
    let options = container.options();
    let mut budget = Budget {
        options,
        decoded: 0,
        items: 0,
    };
    let file_data = budget.payload(container, "/*FileInfoStream")?;
    let file_info = decode_file_info(&file_data, budget.remaining())?;
    budget.charge(file_info.properties.len() as u64)?;
    let build = file_info.product_build.value.clone();
    if !READ_PRODUCT_BUILDS.contains(&build.as_str()) {
        return Err(Error::UnsupportedFeature(
            "stored-entity reader for this product build",
        ));
    }
    let major: u32 = build.split('.').next().unwrap().parse().unwrap();
    let profile = if major >= 18 {
        RecordProfile::Schema3Attributes37
    } else {
        RecordProfile::Schema3Attributes33
    };
    let mut report = ReadReport {
        profile: READ_PROFILE.into(),
        complete: false,
        decoded_entity_count: 0,
        unsupported_entity_count: 0,
        top_level_record_count: 0,
        cached_record_count: 0,
        decoded_bytes: 0,
        budget_used: 0,
        diagnostics: Vec::new(),
    };
    let data = budget.payload(container, "/*StyleStream")?;
    let styles = decode_style_context(&data, &build, budget.remaining())?;
    budget.charge(styles.budget_used)?;
    let text_style_context =
        decode_text_styles(&data, &styles.other_records, budget.remaining(), major)?;
    for message in &styles.diagnostics {
        issue(
            &mut report,
            "unsupported_style_context",
            message,
            Some("/*StyleStream"),
            styles.unparsed_span.clone(),
            None,
        );
    }
    let data = budget.payload(container, "/*SpaceStream")?;
    let space_context = decode_space_context(&data, &build, budget.remaining())?;
    budget.charge(space_context.budget_used)?;
    for message in &space_context.diagnostics {
        issue(
            &mut report,
            "unsupported_space_context",
            message,
            Some("/*SpaceStream"),
            space_context.unparsed_span.clone(),
            None,
        );
    }
    let paths: Vec<String> = container
        .info()
        .entries
        .iter()
        .filter(|e| {
            e.kind == EntryKind::Stream
                && (e.path.starts_with("/*BlockStg/*Blk_") || e.path == "/*BlockStg/*GBlock")
        })
        .map(|e| e.path.clone())
        .collect();
    if paths.is_empty() {
        issue(
            &mut report,
            "missing_blocks",
            "No supported block streams were found.",
            None,
            None,
            None,
        );
    }
    let style_index = StyleIndex::new(&styles);
    let mut blocks = Vec::new();
    let mut entities = Vec::new();
    let mut block_ids = HashSet::new();
    let mut entity_ids = HashSet::new();
    for path in &paths {
        let data = budget.payload(container, path)?;
        let (framing, consumed) = inspect_record_headers_with_budget(
            &data,
            budget.remaining(),
            RecordProfile::from_product_build(&build),
        )?;
        budget.charge(consumed)?;
        report.top_level_record_count += framing.records.len() as u64;
        report.cached_record_count += cached_count(&framing.records);
        if !framing.framing_complete {
            issue(
                &mut report,
                "unframed_records",
                framing.stop_reason.clone().unwrap_or_default(),
                Some(path),
                framing.unparsed_span.clone(),
                None,
            );
        }
        // The record walk already charged these records. Body decoding checks its own bounds.
        let context = crate::context::decode_block_context_with_budget(
            &data,
            &framing.records,
            &build,
            budget.remaining(),
        )?;
        budget.charge(context.budget_used)?;
        let mut explained_messages = HashSet::new();
        for block in &context.blocks {
            for message in &block.diagnostics {
                let message = format!("Block {}: {message}", block.id);
                explained_messages.insert(message.clone());
                report.diagnostics.push(Diagnostic {
                    code: "unsupported_block_context".into(),
                    message,
                    stream_path: Some(path.clone()),
                    span: Some(block.payload_span.clone()),
                    record_id: Some(block.id),
                    type_id: Some(BLOCK_TYPE.into()),
                });
            }
        }
        for message in &context.diagnostics {
            if !explained_messages.contains(message) {
                issue(
                    &mut report,
                    "unsupported_block_context",
                    message,
                    Some(path),
                    context.unparsed_span.clone(),
                    None,
                );
            }
        }
        let local_blocks: HashSet<_> = framing
            .records
            .iter()
            .filter(|r| r.type_id == BLOCK_TYPE)
            .map(|r| r.id_raw)
            .collect();
        let first_entity = entities.len();
        for record in &framing.records {
            if record.type_id == BLOCK_TYPE {
                continue;
            }
            if !entity_ids.insert(record.id_raw) {
                issue(
                    &mut report,
                    "duplicate_entity_id",
                    format!("Duplicate entity identifier {}.", record.id_raw),
                    Some(path),
                    Some(record.span.clone()),
                    Some(record),
                );
            }
            let local_block = local_blocks
                .contains(&record.owner_raw)
                .then_some(record.owner_raw);
            if local_block.is_none() {
                issue(
                    &mut report,
                    "entity_owner_mismatch",
                    "Entity owner does not match the containing block definition.",
                    Some(path),
                    Some(record.span.clone()),
                    Some(record),
                );
            }
            let decoded = if let Some(result) = decode_primitive(&data, record, profile) {
                result.map(|entity| (entity.attributes, Geometry::Primitive(entity.geometry), 0))
            } else if let Some(result) =
                decode_annotation(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::Annotation(entity.geometry),
                        entity.budget_used,
                    )),
                    Err(crate::annotation::AnnotationError::Unsupported(reason)) => Err(reason),
                    Err(crate::annotation::AnnotationError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                decode_insert(&data, record, profile, &build, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::Insert(entity.geometry),
                        entity.budget_used,
                    )),
                    Err(InsertError::Unsupported(reason)) => Err(reason),
                    Err(InsertError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                crate::complex::decode_complex(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::Complex(entity.geometry),
                        entity.budget_used,
                    )),
                    Err(crate::complex::ComplexError::Unsupported(reason)) => Err(reason),
                    Err(crate::complex::ComplexError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                crate::partno::decode_part_number(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::PartNumber(entity.geometry),
                        entity.budget_used,
                    )),
                    Err(crate::complex::ComplexError::Unsupported(reason)) => Err(reason),
                    Err(crate::complex::ComplexError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                crate::bom::decode_bom_table(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::BomTable(Box::new(entity.geometry)),
                        entity.budget_used,
                    )),
                    Err(crate::complex::ComplexError::Unsupported(reason)) => Err(reason),
                    Err(crate::complex::ComplexError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                crate::view2d::decode_view2d(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::View2D(Box::new(entity.geometry)),
                        entity.budget_used,
                    )),
                    Err(crate::complex::ComplexError::Unsupported(reason)) => Err(reason),
                    Err(crate::complex::ComplexError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                crate::table::decode_table(&data, record, profile, major, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::Table(Box::new(entity.geometry)),
                        entity.budget_used,
                    )),
                    Err(crate::complex::ComplexError::Unsupported(reason)) => Err(reason),
                    Err(crate::complex::ComplexError::Limit(error)) => return Err(error),
                }
            } else if let Some(result) =
                decode_attribute_definition(&data, record, profile, &build, budget.remaining())
            {
                match result {
                    Ok(entity) => Ok((
                        entity.attributes,
                        Geometry::AttributeDefinition(Box::new(entity.geometry)),
                        entity.budget_used,
                    )),
                    Err(InsertError::Unsupported(reason)) => Err(reason),
                    Err(InsertError::Limit(error)) => return Err(error),
                }
            } else {
                Err(format!("Unsupported entity type {}.", record.type_id))
            };
            match decoded {
                Ok((attributes, geometry, extra_budget)) => {
                    budget.charge(extra_budget)?;
                    if matches!(
                        geometry,
                        Geometry::Primitive(PrimitiveGeometry::DegenerateArc { .. })
                    ) {
                        issue(&mut report, "degenerate_geometry", "Stored ARC has zero radius and zero basis; no valid curve is inferred.", Some(path), Some(record.payload_span.clone()), Some(record));
                    }
                    if let Geometry::Complex(ComplexGeometry::Dimension(dimension)) = &geometry {
                        issue(&mut report, "unsupported_dimension_semantics", "Stored dimension values and cached presentation are decoded; construction, overrides and associativity remain unresolved.", Some(path), Some(dimension.unresolved_semantics_span.clone()), Some(record));
                        presentation_diagnostics(
                            &mut report,
                            &dimension.presentation_records,
                            path,
                            record,
                            "unsupported_dimension_presentation",
                        );
                    }
                    if let Geometry::Complex(ComplexGeometry::Symbol(symbol)) = &geometry {
                        issue(&mut report, "unsupported_symbol_semantics", "Stored annotation fields and cached presentation are decoded; reference objects, style evaluation and regeneration remain unresolved.", Some(path), Some(symbol.unresolved_semantics_span.clone()), Some(record));
                        presentation_diagnostics(
                            &mut report,
                            &symbol.presentation_records,
                            path,
                            record,
                            "unsupported_symbol_presentation",
                        );
                    }
                    if let Geometry::PartNumber(part) = &geometry {
                        issue(&mut report, "unsupported_part_number_semantics", "Stored part-number fields and item presentations are decoded; annotation style, table references and number regeneration remain unresolved.", Some(path), Some(part.span.clone()), Some(record));
                        presentation_diagnostics(
                            &mut report,
                            &part.presentation_records,
                            path,
                            record,
                            "unsupported_part_number_presentation",
                        );
                        for item in &part.items {
                            presentation_diagnostics(
                                &mut report,
                                &item.geometry.presentation_records,
                                path,
                                record,
                                "unsupported_part_number_presentation",
                            );
                        }
                    }
                    if let Geometry::BomTable(table) = &geometry {
                        issue(&mut report, "unsupported_bom_table_semantics", "Stored BOM placement and header/row presentations are decoded; external data-table cells, associations and regeneration remain unresolved.", Some(path), Some(table.span.clone()), Some(record));
                        for item in &table.items {
                            presentation_diagnostics(
                                &mut report,
                                &item.presentation_records,
                                path,
                                record,
                                "unsupported_bom_table_presentation",
                            );
                        }
                    }
                    if let Geometry::View2D(view) = &geometry {
                        issue(&mut report, "unsupported_view_semantics", "Stored view block reference and clip geometry are decoded; view evaluation and clipping remain unresolved. The view is not expanded as an ordinary INSERT.", Some(path), Some(view.span.clone()), Some(record));
                    }
                    if let Geometry::Table(table) = &geometry {
                        issue(&mut report, "unsupported_table_semantics", "Stored table cells, layout and presentation are decoded; raw property meanings, style evaluation, linked data and regeneration remain unresolved.", Some(path), Some(table.unresolved_semantics_span.clone()), Some(record));
                        presentation_diagnostics(
                            &mut report,
                            &table.presentation_records,
                            path,
                            record,
                            "unsupported_table_presentation",
                        );
                    }
                    let style = resolve_style(&attributes, &style_index, &mut report, path, record);
                    if let Geometry::Annotation(AnnotationGeometry::Text(text)) = &geometry {
                        if !style_index.text_styles.contains(&text.style_id_raw) {
                            issue(
                                &mut report,
                                "unresolved_text_style",
                                format!("Text style {} is unavailable.", text.style_id_raw),
                                Some(path),
                                Some(text.span.clone()),
                                Some(record),
                            );
                        }
                    }
                    entities.push(Entity {
                        id: record.id_raw,
                        owner_id: record.owner_raw,
                        block_id: local_block,
                        type_id: record.type_id.clone(),
                        object_schema: record.object_schema,
                        source: EntitySource {
                            stream_path: path.clone(),
                            span: record.span.clone(),
                            payload_span: record.payload_span.clone(),
                        },
                        attributes,
                        style,
                        geometry,
                        properties: record.property_groups.clone(),
                    });
                }
                Err(reason) => {
                    report.unsupported_entity_count += 1;
                    issue(
                        &mut report,
                        "unsupported_entity",
                        reason,
                        Some(path),
                        Some(record.span.clone()),
                        Some(record),
                    );
                }
            }
        }
        let mut members: HashMap<i32, Vec<usize>> = HashMap::new();
        for (index, entity) in entities.iter().enumerate().skip(first_entity) {
            if let Some(id) = entity.block_id {
                members.entry(id).or_default().push(index);
            }
        }
        for definition in context.blocks {
            if !block_ids.insert(definition.id) {
                issue(
                    &mut report,
                    "duplicate_block_id",
                    format!("Duplicate block identifier {}.", definition.id),
                    Some(path),
                    Some(definition.span.clone()),
                    None,
                );
            }
            let entity_indices = members.remove(&definition.id).unwrap_or_default();
            blocks.push(DrawingBlock {
                definition,
                stream_path: path.clone(),
                entity_indices,
            });
        }
    }
    for entity in &entities {
        if block_ids.contains(&entity.id) {
            report.diagnostics.push(Diagnostic {
                code: "identifier_collision".into(),
                message: format!(
                    "Entity {} has the same identifier as a block definition.",
                    entity.id
                ),
                stream_path: Some(entity.source.stream_path.clone()),
                span: Some(entity.source.span.clone()),
                record_id: Some(entity.id),
                type_id: Some(entity.type_id.clone()),
            });
        }
    }
    validate_block_references(&entities, &block_ids, &mut report);
    let mut space_ids = HashSet::new();
    for space in &space_context.spaces {
        if !space_ids.insert(space.id) {
            issue(
                &mut report,
                "duplicate_space_id",
                format!("Duplicate space identifier {}.", space.id),
                Some("/*SpaceStream"),
                Some(space.span.clone()),
                None,
            );
        }
        if !block_ids.contains(&space.block_id) {
            issue(
                &mut report,
                "unresolved_space_block",
                format!(
                    "Space {} references unavailable block {}.",
                    space.id, space.block_id
                ),
                Some("/*SpaceStream"),
                Some(space.payload_span.clone()),
                None,
            );
        }
    }
    let interpreted: HashSet<_> = paths
        .iter()
        .map(String::as_str)
        .chain(["/*FileInfoStream", "/*StyleStream", "/*SpaceStream"])
        .collect();
    let uninterpreted_streams = container
        .info()
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Stream && !interpreted.contains(e.path.as_str()))
        .cloned()
        .collect();
    report.decoded_entity_count = entities.len() as u64;
    report.decoded_bytes = budget.decoded;
    report.budget_used = budget.items;
    report.complete = report.diagnostics.is_empty();
    if strict && !report.complete {
        return Err(Error::IncompleteRead(Box::new(report)));
    }
    Ok(Document {
        profile: READ_PROFILE.into(),
        evidence: "native_static_and_corpus_consistent_with_numeric_spot_checks".into(),
        file_info,
        coordinate_system: "stored_block_coordinates".into(),
        units: None,
        styles,
        text_style_context,
        space_context,
        blocks,
        entities,
        uninterpreted_streams,
        report,
    })
}

fn validate_block_references(entities: &[Entity], blocks: &HashSet<i32>, report: &mut ReadReport) {
    let mut adjacency: HashMap<i32, Vec<i32>> = HashMap::new();
    for entity in entities {
        let reference = match &entity.geometry {
            Geometry::Insert(insert) => Some((insert, "INSERT", "unresolved_insert_block")),
            Geometry::View2D(view) => {
                Some((&view.block_reference, "View2D", "unresolved_view_block"))
            }
            _ => None,
        };
        if let Some((insert, label, code)) = reference {
            if !blocks.contains(&insert.block_id) {
                report.diagnostics.push(Diagnostic {
                    code: code.into(),
                    message: format!(
                        "{label} {} references unavailable block {}.",
                        entity.id, insert.block_id
                    ),
                    stream_path: Some(entity.source.stream_path.clone()),
                    span: Some(insert.span.clone()),
                    record_id: Some(entity.id),
                    type_id: Some(entity.type_id.clone()),
                });
            }
            if let Some(owner) = entity.block_id {
                adjacency.entry(owner).or_default().push(insert.block_id);
            }
        }
    }
    // Iterative DFS has O(records) memory and does not recurse on attacker data.
    let mut state: HashMap<i32, u8> = HashMap::new();
    let mut roots: Vec<_> = blocks.iter().copied().collect();
    roots.sort_unstable();
    for root in roots {
        if state.contains_key(&root) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state.insert(root, 1);
        while let Some((node, next)) = stack.last_mut() {
            let children = adjacency.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if *next == children.len() {
                state.insert(*node, 2);
                stack.pop();
                continue;
            }
            let child = children[*next];
            *next += 1;
            match state.get(&child) {
                Some(1) => issue(
                    report,
                    "cyclic_block_reference",
                    format!("Block {node} references active ancestor block {child}."),
                    None,
                    None,
                    None,
                ),
                Some(2) => (),
                _ => {
                    state.insert(child, 1);
                    stack.push((child, 0));
                }
            }
        }
    }
}

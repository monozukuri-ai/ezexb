//! Tests of the legacy `CaxaEbForWindows` reader on synthetic streams.

use std::f64::consts::{FRAC_PI_2, PI};

use crate::legacy::LEGACY_MAGIC;
use crate::{
    read_bytes, read_file, read_legacy_bytes, read_legacy_file, AnnotationGeometry, Document,
    Entity, Error, Geometry, ParseOptions, PrimitiveGeometry, TextGeometry, LEGACY_PROFILE,
};

const BY_LAYER: u8 = 0xFB;
const BY_BLOCK: u8 = 0xFC;
const TRAILER: &[u8] = b"\x00\x00\x00\x00\xff\xff\x00\x00\x00\x00";
/// GBK for 中心线层 (centre line layer).
const CENTRE_LAYER: &[u8] = b"\xd6\xd0\xd0\xc4\xcf\xdf\xb2\xe3";
/// GBK for 宋体.
const SONG: &[u8] = b"\xcb\xce\xcc\xe5";

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn f64s(out: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn string(out: &mut Vec<u8>, value: &[u8]) {
    out.push(u8::try_from(value.len()).expect("short test string"));
    out.extend_from_slice(value);
}

/// `(key, description, visible, color, line style)`
type LayerSpec<'a> = (&'a [u8], &'a [u8], bool, u8, u8);
/// `(name, pattern, width)`
type LineStyleSpec<'a> = (&'a [u8], u16, f64);

const LAYERS: &[LayerSpec] = &[
    (b"0", b"", true, 47, 0),
    (b"1", CENTRE_LAYER, true, 8, 3),
    (b"2", b"hidden", false, 15, 24),
];
const LINE_STYLES: &[LineStyleSpec] = &[(b"Medium", 0xFFFF, 0.35), (b"Imported", 0x5FFF, 0.001)];

/// Everything in front of the entity section.
fn prelude(version: u32, layers: &[LayerSpec], line_styles: &[LineStyleSpec]) -> Vec<u8> {
    let mut out = LEGACY_MAGIC.to_vec();
    for value in [version, 1, 0] {
        u32le(&mut out, value);
    }
    string(&mut out, b"properties");
    for value in [3, 0, 50, 2, 256] {
        u32le(&mut out, value);
    }
    out.extend_from_slice(&[0; 4]);
    u32le(&mut out, layers.len() as u32);
    for (key, description, visible, color, line_style) in layers {
        string(&mut out, key);
        string(&mut out, description);
        out.extend_from_slice(&[u8::from(*visible), *color, *line_style]);
        out.extend_from_slice(if version >= 1070 { &[0, 1, 1] } else { &[1] });
    }
    out.push(2);
    u32le(&mut out, 24 + line_styles.len() as u32);
    for (name, pattern, width) in line_styles {
        string(&mut out, name);
        u32le(&mut out, 5);
        out.extend_from_slice(&pattern.to_le_bytes());
        f64s(&mut out, &[*width]);
        out.push(1);
    }
    out.push(0);
    u32le(&mut out, 0x0011_2233);
    for _ in 1..16 {
        u32le(&mut out, 0x00FF_FFFF);
    }
    out.extend_from_slice(&[1; 64]);
    if version >= 1060 {
        u32le(&mut out, 1);
        u32le(&mut out, 10_000);
        u32le(&mut out, 10_000);
        string(&mut out, b"Standard");
        string(&mut out, b"Arial");
        string(&mut out, SONG);
        f64s(&mut out, &[3.5, 0.0, 0.1, 0.4, 0.667, 0.7, 0.0]);
    }
    out
}

fn file_with(
    version: u32,
    layers: &[LayerSpec],
    line_styles: &[LineStyleSpec],
    count: u32,
    records: &[u8],
) -> Vec<u8> {
    let mut out = prelude(version, layers, line_styles);
    u32le(&mut out, count);
    out.extend_from_slice(records);
    out.extend_from_slice(TRAILER);
    out
}

fn file(version: u32, count: u32, records: &[u8]) -> Vec<u8> {
    file_with(version, LAYERS, LINE_STYLES, count, records)
}

/// `type type layer color style 00 FFFF tags`
fn doubled(out: &mut Vec<u8>, kind: u8, layer: u8, color: u8, line_style: u8, tags: u32) {
    out.extend_from_slice(&[kind, kind, layer, color, line_style, 0, 0xFF, 0xFF]);
    u32le(out, tags);
}

/// `type layer color style 00 FFFF tags`
fn plain(out: &mut Vec<u8>, kind: u8, layer: u8, color: u8, line_style: u8, tags: u32) {
    out.extend_from_slice(&[kind, layer, color, line_style, 0, 0xFF, 0xFF]);
    u32le(out, tags);
}

fn line(out: &mut Vec<u8>, layer: u8, color: u8, line_style: u8, ends: [f64; 4]) {
    doubled(out, 0x02, layer, color, line_style, 0);
    f64s(out, &ends);
}

fn by_layer_line(out: &mut Vec<u8>, ends: [f64; 4]) {
    line(out, 0, BY_LAYER, BY_LAYER, ends);
}

fn tag_f64(out: &mut Vec<u8>, tag: u8, value: f64) {
    out.extend_from_slice(&[tag, tag]);
    f64s(out, &[value]);
}

fn tag_u8(out: &mut Vec<u8>, tag: u8, value: u8) {
    out.extend_from_slice(&[tag, tag, value]);
}

fn tag_string(out: &mut Vec<u8>, tag: u8, value: &[u8]) {
    out.extend_from_slice(&[tag, tag]);
    string(out, value);
}

fn read(data: &[u8]) -> Document {
    read_legacy_bytes(data, ParseOptions::default(), false).expect("partial read")
}

fn codes(document: &Document) -> Vec<&str> {
    document
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn line_ends(entity: &Entity) -> ([f64; 3], [f64; 3]) {
    match &entity.geometry {
        Geometry::Primitive(PrimitiveGeometry::Line { start, end }) => (*start, *end),
        other => panic!("not a line: {other:?}"),
    }
}

fn text(entity: &Entity) -> &TextGeometry {
    match &entity.geometry {
        Geometry::Annotation(AnnotationGeometry::Text(text)) => text,
        other => panic!("not a text: {other:?}"),
    }
}

fn kinds(document: &Document) -> Vec<&str> {
    document
        .entities
        .iter()
        .map(|entity| entity.type_id.as_str())
        .collect()
}

#[test]
fn reads_primitives_layers_colors_and_line_styles() {
    let mut records = Vec::new();
    line(&mut records, 1, BY_LAYER, BY_LAYER, [1.0, 2.0, 4.0, 6.0]);
    doubled(&mut records, 0x03, 0, 9, 0, 0);
    f64s(
        &mut records,
        &[10.0, 10.0, 11.0, 10.0, 10.0, 11.0, 2.0 * PI, 5.0],
    );
    doubled(&mut records, 0x04, 0, 48, 25, 0);
    f64s(
        &mut records,
        &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, -FRAC_PI_2, 2.0],
    );
    doubled(&mut records, 0x07, 2, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 2);
    f64s(&mut records, &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    doubled(&mut records, 0x01, 0, BY_LAYER, BY_LAYER, 0);
    f64s(&mut records, &[7.0, 8.0]);
    let data = file(1070, 5, &records);

    let document = read_legacy_bytes(&data, ParseOptions::default(), true).expect("strict read");
    assert_eq!(document.profile, LEGACY_PROFILE);
    assert_eq!(
        document.file_info.product_build.value,
        "CaxaEbForWindows 1070"
    );
    assert_eq!(document.file_info.stream_version.value, "1070");
    assert!(document.report.complete && document.report.diagnostics.is_empty());
    assert_eq!(document.report.top_level_record_count, 5);
    assert_eq!(document.report.decoded_entity_count, 5);
    assert_eq!(document.space_context.spaces.len(), 1);
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].entity_indices, vec![0, 1, 2, 3, 4]);
    assert_eq!(
        kinds(&document),
        [
            "legacy:line",
            "legacy:circle",
            "legacy:arc",
            "legacy:polyline",
            "legacy:point"
        ]
    );

    // Layers: the description names the layer, the palette gives its color and
    // the line style its linetype and width.
    let layers = &document.styles.layers;
    assert!(document.styles.complete);
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer.name.value.as_str())
            .collect::<Vec<_>>(),
        ["0", "中心线层", "hidden"]
    );
    assert_eq!(layers[1].key_name.value, "1");
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer.color_raw)
            .collect::<Vec<_>>(),
        [0x00FF_FFFF, 0x0000_00FF, 0x00FF_00FF]
    );
    assert_eq!(
        layers
            .iter()
            .map(|layer| (layer.linetype_id, layer.width_flag, layer.width))
            .collect::<Vec<_>>(),
        [(0, 4, 0.0), (3, 3, 0.0), (24, 2, 0.35)]
    );
    assert_eq!(
        layers.iter().map(|layer| layer.on).collect::<Vec<_>>(),
        [true, true, false]
    );

    // Linetypes: the five standard styles and the stored ones.
    let linetypes = &document.styles.linetypes;
    assert_eq!(
        linetypes
            .iter()
            .map(|linetype| (linetype.id, linetype.name.value.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, "LEGACY_THICK"),
            (1, "LEGACY_THIN"),
            (2, "LEGACY_DASHED"),
            (3, "LEGACY_CENTER"),
            (4, "LEGACY_PHANTOM"),
            (24, "Medium"),
            (25, "Imported"),
        ]
    );
    assert!(linetypes[0].dashes.is_empty() && linetypes[1].dashes.is_empty());
    assert_eq!(linetypes[3].dashes, [12.0, 2.0, 2.0, 2.0]);
    assert_eq!(
        (linetypes[5].pattern, linetypes[5].dashes.len()),
        (Some(0xFFFF), 0)
    );
    assert_eq!(linetypes[6].dashes, [13.0, 1.0, 1.0, 1.0]);

    let entities = &document.entities;
    assert_eq!(line_ends(&entities[0]), ([1.0, 2.0, 0.0], [4.0, 6.0, 0.0]));
    let style = &entities[0].style;
    assert_eq!(style.layer_name.as_deref(), Some("中心线层"));
    assert_eq!(
        (style.color_raw.source.as_str(), style.color_raw.value),
        ("by_layer", Some(0xFF))
    );
    assert_eq!(
        (style.linetype.source.as_str(), style.linetype.value),
        ("by_layer", Some(3))
    );
    assert_eq!(style.line_width.source, "by_layer_thin");

    match &entities[1].geometry {
        Geometry::Primitive(PrimitiveGeometry::Circle {
            center,
            radius,
            x_axis,
            y_axis,
        }) => {
            assert_eq!((*center, *radius), ([10.0, 10.0, 0.0], 5.0));
            assert_eq!((*x_axis, *y_axis), ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
        }
        other => panic!("not a circle: {other:?}"),
    }
    let style = &entities[1].style;
    assert_eq!(
        (style.color_raw.source.as_str(), style.color_raw.value),
        ("explicit", Some(0x0000_FFFF))
    );
    assert_eq!(
        (style.linetype.source.as_str(), style.linetype.value),
        ("explicit", Some(0))
    );
    assert_eq!(style.line_width.source, "wide");

    // A negative sweep becomes the same arc with a mirrored second axis.
    match &entities[2].geometry {
        Geometry::Primitive(PrimitiveGeometry::Arc {
            radius,
            y_axis,
            sweep_angle,
            start,
            end,
            ..
        }) => {
            assert_eq!((*radius, *y_axis), (2.0, [0.0, -1.0, 0.0]));
            assert!((sweep_angle - FRAC_PI_2).abs() < 1e-12);
            assert_eq!(*start, [2.0, 0.0, 0.0]);
            assert!(end[0].abs() < 1e-12 && (end[1] + 2.0).abs() < 1e-12);
        }
        other => panic!("not an arc: {other:?}"),
    }
    // A custom color and a stored line style whose width is below the threshold.
    let style = &entities[2].style;
    assert_eq!(style.color_raw.value, Some(0x0011_2233));
    assert_eq!(
        (style.linetype.value, style.line_width.source.as_str()),
        (Some(25), "thin")
    );

    match &entities[3].geometry {
        Geometry::Annotation(AnnotationGeometry::Polyline(polyline)) => {
            let points: Vec<_> = polyline
                .vertices
                .iter()
                .map(|vertex| vertex.point)
                .collect();
            assert_eq!(points, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
            assert!(!polyline.closed);
        }
        other => panic!("not a polyline: {other:?}"),
    }
    let style = &entities[3].style;
    assert_eq!(
        (style.linetype.value, style.line_width.value),
        (Some(24), Some(0.35))
    );
    assert_eq!(style.line_width.source, "by_layer");

    match &entities[4].geometry {
        Geometry::Primitive(PrimitiveGeometry::Point { point }) => {
            assert_eq!(*point, [7.0, 8.0, 0.0]);
        }
        other => panic!("not a point: {other:?}"),
    }
}

#[test]
fn text_of_the_later_layouts_is_placed_at_its_alignment_point() {
    let mut records = Vec::new();
    // Centred in a 40 × 10 area, compressed to fit, rotated.
    doubled(&mut records, 0x06, 0, BY_LAYER, BY_LAYER, 1);
    tag_f64(&mut records, 0x29, 40.0);
    f64s(&mut records, &[10.0, 20.0, 50.0, 10.0, 3.5]);
    string(&mut records, b"ABC");
    u32le(&mut records, 10_000);
    f64s(&mut records, &[0.5]);
    u32le(&mut records, 0);
    records.extend_from_slice(&[5, 0x20]);
    // No area: the point is the alignment point and the second point is unused.
    doubled(&mut records, 0x06, 0, BY_LAYER, BY_LAYER, 0);
    f64s(&mut records, &[5.0, 5.0, 999.0, 999.0, 2.0]);
    string(&mut records, b"x");
    u32le(&mut records, 10_000);
    f64s(&mut records, &[0.0]);
    u32le(&mut records, 0);
    records.extend_from_slice(&[9, 0]);
    // An alignment code outside the grid.
    doubled(&mut records, 0x06, 0, BY_LAYER, BY_LAYER, 0);
    f64s(&mut records, &[1.0, 1.0, 0.0, 0.0, 2.0]);
    string(&mut records, b"y");
    u32le(&mut records, 10_000);
    f64s(&mut records, &[0.0]);
    u32le(&mut records, 0);
    records.extend_from_slice(&[10, 0]);
    let document = read(&file(1070, 3, &records));

    assert_eq!(codes(&document), ["legacy_text_alignment_unknown"]);
    let first = text(&document.entities[0]);
    assert_eq!(first.contents, "ABC");
    assert_eq!(first.insertion, [30.0, 15.0, 0.0]);
    assert_eq!(
        (first.area_width_stored, first.area_height_stored),
        (40.0, 10.0)
    );
    assert_eq!(
        (first.alignment_raw, first.fit_mode, first.mode_flags_raw),
        (4, 1, 0x20)
    );
    assert_eq!((first.text_height_stored, first.rotation), (3.5, 0.5));
    assert_eq!((first.style_id_raw, first.drawing_scale), (10_000, 1.0));

    let second = text(&document.entities[1]);
    assert_eq!(second.insertion, [5.0, 5.0, 0.0]);
    assert_eq!(
        (second.area_width_stored, second.area_height_stored),
        (0.0, 0.0)
    );
    assert_eq!((second.alignment_raw, second.fit_mode), (8, 0));
    assert_eq!(text(&document.entities[2]).alignment_raw, 0);

    let styles = &document.text_style_context;
    assert!(styles.complete && styles.diagnostics.is_empty());
    assert_eq!(styles.styles.len(), 1);
    let style = &styles.styles[0];
    assert_eq!((style.id, style.name.value.as_str()), (10_000, "Standard"));
    assert_eq!(
        (
            style.western_font.value.as_str(),
            style.cjk_font.value.as_str()
        ),
        ("Arial", "宋体")
    );
    assert_eq!(
        (style.western_width_factor, style.cjk_width_factor),
        (0.667, 0.7)
    );
    assert_eq!(
        (
            style.height,
            style.character_spacing_factor,
            style.line_spacing_factor
        ),
        (3.5, 0.1, 0.4)
    );
}

/// The tags a text record of the 1050 layout carries.
fn text_1050(out: &mut Vec<u8>, font: &[u8], area: Option<[f64; 3]>, contents: &[u8]) {
    let tags = if area.is_some() { 12 } else { 10 };
    doubled(out, 0x06, 0, BY_LAYER, BY_LAYER, tags);
    if let Some([x, y, width]) = area {
        out.extend_from_slice(&[0x15, 0x15]);
        f64s(out, &[x, y]);
        tag_f64(out, 0x29, width);
    }
    tag_f64(out, 0x2B, 0.667);
    tag_f64(out, 0x2C, 0.8);
    tag_f64(out, 0x2D, 0.1);
    tag_f64(out, 0x2E, 0.4);
    tag_f64(out, 0x32, 1.0);
    tag_u8(out, 0x50, 5);
    tag_u8(out, 0x51, 0x50);
    tag_string(out, 0x64, b"Notes");
    tag_string(out, 0x65, font);
    tag_string(out, 0x66, SONG);
    f64s(out, &[10.0, 6.0, 1.5]);
    string(out, contents);
}

#[test]
fn text_of_the_1050_layout_takes_its_values_from_tags() {
    let mut records = Vec::new();
    text_1050(&mut records, b"FontA", Some([14.0, 2.0, 4.0]), b"hi");
    text_1050(&mut records, b"FontA", None, b"again");
    text_1050(&mut records, b"FontB", None, b"other");
    // Fields behind the contents are not decoded.
    records.extend_from_slice(&[0xAA; 7]);
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 1.0]);
    let document = read(&file(1050, 4, &records));

    assert_eq!(codes(&document), ["legacy_record_tail_skipped"]);
    assert_eq!(
        document.report.diagnostics[0].type_id.as_deref(),
        Some("legacy:text")
    );
    assert_eq!(
        kinds(&document),
        ["legacy:text", "legacy:text", "legacy:text", "legacy:line"]
    );
    // Centred in the area between the point and the far corner.
    let first = text(&document.entities[0]);
    assert_eq!(first.insertion, [12.0, 4.0, 0.0]);
    assert_eq!(
        (first.area_width_stored, first.area_height_stored),
        (4.0, 4.0)
    );
    assert_eq!((first.alignment_raw, first.rotation), (4, 1.0));
    assert_eq!((first.mode_flags_raw, first.fit_mode), (0x50, 2));
    // Without an area the point is the alignment point.
    let second = text(&document.entities[1]);
    assert_eq!(second.insertion, [10.0, 6.0, 0.0]);
    assert_eq!(second.area_width_stored, 0.0);

    // One style per distinct combination of tag values.
    let ids: Vec<_> = document.entities[..3]
        .iter()
        .map(|entity| text(entity).style_id_raw)
        .collect();
    assert_eq!(ids, [1, 1, 2]);
    let styles = &document.text_style_context;
    assert!(styles.complete);
    assert_eq!(styles.diagnostics.len(), 1);
    assert_eq!(styles.styles.len(), 2);
    let style = &styles.styles[0];
    assert_eq!((style.id, style.name.value.as_str()), (1, "Notes"));
    assert_eq!(
        (
            style.western_font.value.as_str(),
            style.cjk_font.value.as_str()
        ),
        ("FontA", "宋体")
    );
    assert_eq!(
        (style.western_width_factor, style.cjk_width_factor),
        (0.667, 0.8)
    );
    assert_eq!(
        (style.character_spacing_factor, style.line_spacing_factor),
        (0.1, 0.4)
    );
    assert_eq!(styles.styles[1].western_font.value, "FontB");
}

#[test]
fn group_members_inherit_by_block_values_from_their_group() {
    let mut records = Vec::new();
    // A block on the centre layer with its own color and line style.
    doubled(&mut records, 0x08, 1, 9, 3, 0);
    u32le(&mut records, 2);
    line(&mut records, 0, BY_BLOCK, BY_BLOCK, [0.0, 0.0, 1.0, 0.0]);
    doubled(&mut records, 0x08, 0, BY_BLOCK, 1, 0);
    u32le(&mut records, 1);
    doubled(&mut records, 0x03, 0, BY_BLOCK, BY_BLOCK, 0);
    f64s(&mut records, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 2.0 * PI, 1.0]);
    // The block reference that follows a block outside its member count.
    plain(&mut records, 0x0E, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 0);
    f64s(&mut records, &[0.0, 0.0, 1.0, 1.0, 0.0]);
    string(&mut records, b"PART");
    string(&mut records, b"");
    records.push(0);
    by_layer_line(&mut records, [5.0, 5.0, 6.0, 6.0]);
    let data = file(1070, 2, &records);

    let document = read_legacy_bytes(&data, ParseOptions::default(), true).expect("strict read");
    assert_eq!(
        kinds(&document),
        ["legacy:line", "legacy:circle", "legacy:line"]
    );
    assert_eq!(document.report.top_level_record_count, 2);
    let ids: Vec<_> = document
        .entities
        .iter()
        .map(|entity| (entity.id, entity.owner_id))
        .collect();
    assert_eq!(ids, [(2, 1), (4, 3), (6, 0)]);

    let member = &document.entities[0].style;
    assert_eq!(
        (member.color_raw.source.as_str(), member.color_raw.value),
        ("by_block", Some(0x0000_FFFF))
    );
    assert_eq!(
        (member.linetype.source.as_str(), member.linetype.value),
        ("by_block", Some(3))
    );
    assert_eq!(member.line_width.source, "by_block_thin");
    assert_eq!(document.entities[0].attributes.color_flag_raw, 1);
    // Through a nested group that passes the color on and sets the line style.
    let nested = &document.entities[1].style;
    assert_eq!(nested.color_raw.value, Some(0x0000_FFFF));
    assert_eq!(
        (nested.linetype.source.as_str(), nested.linetype.value),
        ("by_block", Some(1))
    );
    // Outside a group nothing is inherited.
    let top = &document.entities[2].style;
    assert_eq!(
        (top.color_raw.source.as_str(), top.color_raw.value),
        ("by_layer", Some(0x00FF_FFFF))
    );
}

#[test]
fn hatch_groups_and_base_points() {
    let mut records = Vec::new();
    // The lines of a hatch, then its parameters.
    doubled(&mut records, 0x12, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 2);
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 1.0]);
    by_layer_line(&mut records, [0.0, 1.0, 1.0, 2.0]);
    doubled(&mut records, 0x09, 0, BY_LAYER, BY_LAYER, 0);
    f64s(&mut records, &[0.5, 0.5]);
    records.push(0);
    f64s(&mut records, &[3.0, 45.0]);
    u32le(&mut records, 1);
    // A group that ends with its base point.
    doubled(&mut records, 0x12, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 1);
    by_layer_line(&mut records, [2.0, 2.0, 3.0, 3.0]);
    f64s(&mut records, &[2.0, 2.0]);
    // A group without members names a definition.
    doubled(&mut records, 0x12, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 0);
    f64s(&mut records, &[0.0, 0.0, 1.0, 1.0, 0.0]);
    string(&mut records, b"NAME");
    string(&mut records, b"");
    records.push(0);
    by_layer_line(&mut records, [4.0, 4.0, 5.0, 5.0]);
    let data = file(1070, 5, &records);

    let document = read_legacy_bytes(&data, ParseOptions::default(), true).expect("strict read");
    assert_eq!(document.entities.len(), 4);
    assert_eq!(document.report.top_level_record_count, 5);
    assert_eq!(line_ends(&document.entities[3]).0, [4.0, 4.0, 0.0]);
}

#[test]
fn undecoded_tails_keep_the_geometry_and_are_reported() {
    // A dimension in the plain header form, and in the wrapped form with a zero
    // and with a nonzero word.
    for wrapper in [None, Some(0_u8), Some(7)] {
        let mut records = Vec::new();
        by_layer_line(&mut records, [5.0, 5.0, 6.0, 6.0]);
        if let Some(word) = wrapper {
            records.extend_from_slice(&[0x05, word, 0, 0, 0]);
        }
        plain(&mut records, 0x05, 0, BY_LAYER, BY_LAYER, 0);
        u32le(&mut records, 1);
        by_layer_line(&mut records, [0.0, 0.0, 10.0, 0.0]);
        records.extend_from_slice(&[0xAA; 9]);
        by_layer_line(&mut records, [1.0, 1.0, 2.0, 2.0]);
        let data = file(1070, 3, &records);

        let document = read(&data);
        assert_eq!(kinds(&document), ["legacy:line"; 3]);
        assert_eq!(codes(&document), ["legacy_record_tail_skipped"]);
        let diagnostic = &document.report.diagnostics[0];
        assert_eq!(diagnostic.type_id.as_deref(), Some("legacy:dimension"));
        assert!(
            diagnostic.message.contains("(9 bytes)"),
            "{}",
            diagnostic.message
        );
        assert!(!document.report.complete);
        // The member belongs to the dimension record.
        assert_eq!(document.entities[1].owner_id, 2);

        match read_legacy_bytes(&data, ParseOptions::default(), true) {
            Err(Error::IncompleteRead(report)) => {
                assert_eq!(report.diagnostics.len(), 1);
                assert_eq!(report.decoded_entity_count, 3);
            }
            other => panic!("expected an incomplete read: {:?}", other.map(|_| ())),
        }
    }
}

#[test]
fn ellipses_and_splines_are_their_stored_sample_polylines() {
    let mut records = Vec::new();
    doubled(&mut records, 0x10, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 2);
    f64s(&mut records, &[0.0, 0.0, 1.0, 1.0, 2.0, 0.0]);
    records.extend_from_slice(&[0xAA; 12]);
    doubled(&mut records, 0x0F, 0, BY_LAYER, BY_LAYER, 0);
    u32le(&mut records, 1);
    f64s(&mut records, &[5.0, 5.0, 6.0, 5.0]);
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 0.0]);

    for version in [1050, 1060, 1070] {
        let document = read(&file(version, 3, &records));
        assert_eq!(
            kinds(&document),
            [
                "legacy:spline_samples",
                "legacy:ellipse_samples",
                "legacy:line"
            ]
        );
        // The spline has a definition behind its points; the ellipse here has none.
        assert_eq!(
            codes(&document),
            ["legacy_record_tail_skipped", "legacy_curve_approximated"]
        );
        assert_eq!(
            document.report.diagnostics[0].type_id.as_deref(),
            Some("legacy:spline")
        );
        assert_eq!(document.report.unsupported_entity_count, 0);
        match &document.entities[0].geometry {
            Geometry::Annotation(AnnotationGeometry::Polyline(polyline)) => {
                let points: Vec<_> = polyline
                    .vertices
                    .iter()
                    .map(|vertex| vertex.point)
                    .collect();
                assert_eq!(points, [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]);
            }
            other => panic!("not a polyline: {other:?}"),
        }
    }
}

#[test]
fn undecodable_bytes_are_skipped_up_to_the_next_record() {
    let mut records = Vec::new();
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 0.0]);
    records.extend_from_slice(&[0xEE; 6]);
    by_layer_line(&mut records, [1.0, 0.0, 2.0, 0.0]);
    let document = read(&file(1070, 2, &records));
    assert_eq!(document.entities.len(), 2);
    assert_eq!(codes(&document), ["legacy_records_skipped"]);
    let message = &document.report.diagnostics[0].message;
    assert!(message.contains("1 span(s) (6 bytes)"), "{message}");

    // Data behind the last record is not a lost record.
    let mut records = Vec::new();
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 0.0]);
    records.extend_from_slice(&[0x77; 20]);
    let document = read(&file(1070, 1, &records));
    assert_eq!(document.entities.len(), 1);
    assert_eq!(codes(&document), ["legacy_trailing_data_skipped"]);
    let diagnostic = &document.report.diagnostics[0];
    assert!(
        diagnostic.message.starts_with("20 bytes"),
        "{}",
        diagnostic.message
    );
    assert_eq!(diagnostic.span.as_ref().map(|span| span.length), Some(20));
}

#[test]
fn unidentified_styles_are_reported_once() {
    let mut records = Vec::new();
    line(&mut records, 0, BY_LAYER, 13, [0.0, 0.0, 1.0, 0.0]);
    line(&mut records, 9, BY_LAYER, BY_LAYER, [0.0, 1.0, 1.0, 1.0]);
    line(&mut records, 0, 200, BY_LAYER, [0.0, 2.0, 1.0, 2.0]);
    let document = read(&file(1070, 3, &records));
    assert_eq!(
        codes(&document),
        [
            "legacy_line_style_unidentified",
            "unresolved_layer",
            "legacy_color_unresolved"
        ]
    );

    // A built-in style the reader does not identify is a continuous placeholder.
    let style = &document.entities[0].style;
    assert_eq!(
        (style.linetype.value, style.line_width.source.as_str()),
        (Some(13), "device")
    );
    let placeholder = document
        .styles
        .linetypes
        .iter()
        .find(|linetype| linetype.id == 13)
        .expect("placeholder linetype");
    assert_eq!(placeholder.name.value, "LEGACY_STYLE_13");
    assert!(placeholder.dashes.is_empty());

    let style = &document.entities[1].style;
    assert_eq!((style.layer_id, style.layer_name.as_deref()), (9, None));
    assert_eq!(style.color_raw.value, None);

    let style = &document.entities[2].style;
    assert_eq!(
        (style.color_raw.source.as_str(), style.color_raw.value),
        ("device", None)
    );
}

#[test]
fn the_1050_layer_layout_and_missing_tables() {
    let mut records = Vec::new();
    line(&mut records, 1, BY_LAYER, BY_LAYER, [0.0, 0.0, 1.0, 0.0]);
    let document = read(&file(1050, 1, &records));
    assert!(document.report.complete);
    assert_eq!(document.styles.layers.len(), 3);
    assert_eq!(
        document.entities[0].style.layer_name.as_deref(),
        Some("中心线层")
    );
    // The 1050 layout has no text-style table the reader decodes.
    assert!(document.text_style_context.styles.is_empty());

    // Without a layer table the entities keep their indexes.
    let mut data = LEGACY_MAGIC.to_vec();
    for value in [1070, 1, 0] {
        u32le(&mut data, value);
    }
    string(&mut data, b"");
    for value in [3, 0, 50, 0, 256] {
        u32le(&mut data, value);
    }
    data.extend_from_slice(&[0x33; 40]);
    u32le(&mut data, 1);
    data.extend_from_slice(&records);
    data.extend_from_slice(TRAILER);
    let document = read(&data);
    assert_eq!(codes(&document), ["legacy_layer_table_missing"]);
    assert!(document.styles.layers.is_empty() && !document.styles.complete);
    let style = &document.entities[0].style;
    assert_eq!((style.layer_id, style.layer_name.as_deref()), (1, None));

    // And without an entity section there is nothing to return.
    let mut data = prelude(1070, LAYERS, LINE_STYLES);
    data.extend_from_slice(TRAILER);
    let document = read(&data);
    assert_eq!(codes(&document), ["legacy_entity_section_missing"]);
    assert!(document.entities.is_empty());
}

#[test]
fn versions_limits_and_dispatch() {
    let mut records = Vec::new();
    by_layer_line(&mut records, [0.0, 0.0, 1.0, 0.0]);
    by_layer_line(&mut records, [1.0, 0.0, 2.0, 0.0]);
    let data = file(1070, 2, &records);
    let options = ParseOptions::default();

    // The common entry points recognise the signature.
    let document = read_bytes(&data, options, true).expect("read_bytes");
    assert_eq!(document.profile, LEGACY_PROFILE);
    let path = std::env::temp_dir().join(format!("ezexb-legacy-{}.exb", std::process::id()));
    std::fs::write(&path, &data).expect("write");
    let from_file = read_file(&path, options, true);
    let direct = read_legacy_file(&path, options, true);
    std::fs::remove_file(&path).expect("remove");
    assert_eq!(from_file.expect("read_file").entities.len(), 2);
    assert_eq!(direct.expect("read_legacy_file").entities.len(), 2);

    // A version near the verified ones is read with their layout and reported.
    let document = read(&file(1080, 2, &records));
    assert_eq!(codes(&document), ["unverified_legacy_version"]);
    assert_eq!(document.entities.len(), 2);

    assert!(matches!(
        read_legacy_bytes(&file(2000, 2, &records), options, false),
        Err(Error::UnsupportedFeature(_))
    ));
    assert!(matches!(
        read_legacy_bytes(b"not a legacy file", options, false),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        read_legacy_bytes(&data[..24], options, false),
        Err(Error::InvalidStream(_))
    ));
    let limited = ParseOptions {
        max_records: 1,
        ..options
    };
    assert!(matches!(
        read_legacy_bytes(&data, limited, false),
        Err(Error::ResourceLimit {
            name: "max_records",
            ..
        })
    ));
    let limited = ParseOptions {
        max_file_bytes: 16,
        ..options
    };
    assert!(matches!(
        read_legacy_bytes(&data, limited, false),
        Err(Error::InputLimit { .. })
    ));
    // Group nesting is bounded.
    let mut nested = Vec::new();
    for _ in 0..3 {
        doubled(&mut nested, 0x08, 0, BY_LAYER, BY_LAYER, 0);
        u32le(&mut nested, 1);
    }
    by_layer_line(&mut nested, [0.0, 0.0, 1.0, 0.0]);
    let limited = ParseOptions {
        max_record_depth: 2,
        ..options
    };
    assert!(matches!(
        read_legacy_bytes(&file(1070, 1, &nested), limited, false),
        Err(Error::ResourceLimit {
            name: "max_record_depth",
            ..
        })
    ));
    assert_eq!(read(&file(1070, 1, &nested)).entities.len(), 1);
}

//! Bounded decoding of stored 2D POLYLINE and TEXT data. Text layout is not rendered.
use serde::Serialize;

use crate::geometry::{decode_attributes, EntityAttributes};
use crate::{Error, ParseOptions, RecordHeader, RecordProfile, SourceSpan};

const POLYLINE: &str = "56d8e732-ad1f-426d-a4b0-6c5998434583";
const TEXT: &str = "76d76221-4358-465d-bc53-4449b3004657";

#[derive(Debug)]
pub(crate) enum AnnotationError {
    Unsupported(String),
    Limit(Error),
}

impl From<String> for AnnotationError {
    fn from(value: String) -> Self {
        Self::Unsupported(value)
    }
}
impl From<&str> for AnnotationError {
    fn from(value: &str) -> Self {
        Self::Unsupported(value.into())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum AnnotationGeometry {
    Polyline(PolylineGeometry),
    Text(TextGeometry),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationEntity {
    pub attributes: EntityAttributes,
    pub geometry: AnnotationGeometry,
    /// Additional items consumed from the caller's cumulative record budget:
    /// one per vertex, or one TEXT content item.
    pub budget_used: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PolylineVertex {
    /// Coordinates in the stored polyline plane. Only the default normal is qualified.
    pub point: [f64; 2],
    /// Older saves include this value, which the native 2D loader discards.
    /// Nonzero values are rejected by this limited reader.
    pub legacy_z: Option<f64>,
    pub bulge: f64,
    pub start_width: f64,
    pub end_width: f64,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PolylineGeometry {
    pub vertices: Vec<PolylineVertex>,
    pub closed: bool,
    pub closed_raw: u32,
    /// Absent in legacy saves; otherwise restricted to the observed [0, 0, 1].
    pub normal: Option<[f64; 3]>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextGeometry {
    /// UTF-16 contents without the terminator; formatting escapes remain literal.
    pub contents: String,
    /// Includes the byte count and UTF-16 terminator.
    pub contents_span: SourceSpan,
    /// Stored IXYZ coordinates, without block/space transforms.
    pub insertion: [f64; 3],
    /// Saved values, before native font/layout conversion and inherited styles.
    pub text_height_stored: f64,
    pub area_height_stored: f64,
    pub area_width_stored: f64,
    /// Native rotation value in radians.
    pub rotation: f64,
    pub drawing_scale: f64,
    pub mode_flags_raw: u8,
    pub alignment_raw: u32,
    /// Native GetTextDirection result: bit 3 of mode_flags_raw.
    pub writing_direction: u8,
    /// Native GetFitMode result; other mode bits retain their raw values.
    pub fit_mode: u8,
    pub style_status_raw: u32,
    pub style_id_raw: i32,
    pub legacy_format_raw: Option<u32>,
    /// Saved spacing sentinel/value, not the rendered line separation.
    pub line_spacing_raw: f64,
    pub line_spacing_style_raw: u32,
    pub format_type_id: Option<String>,
    /// Save major >= 15 only: the stored column layout (flags, width, count and
    /// one raw 16-byte entry per column). A nonzero count is not applied.
    pub column_flags_raw: Option<[u32; 3]>,
    pub column_width: Option<f64>,
    pub column_count: Option<u32>,
    pub column_entries_raw: Option<Vec<[f64; 2]>>,
    /// Version-specific formatting, color, and column fields. Their raw bytes
    /// remain available in the logical stream; layout is not reconstructed.
    pub formatting_span: SourceSpan,
    pub span: SourceSpan,
}

pub(crate) fn decode_annotation(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    save_major: u32,
    options: ParseOptions,
) -> Option<Result<AnnotationEntity, AnnotationError>> {
    if !matches!(header.type_id.as_str(), POLYLINE | TEXT) {
        return None;
    }
    Some((|| {
        if !matches!(header.object_schema, 1 | 100) || header.extension.is_some() {
            return Err("Unsupported annotation object schema or extension.".into());
        }
        let width = match (profile, save_major) {
            (RecordProfile::Schema3Attributes33, 10..=17) => 33,
            (RecordProfile::Schema3Attributes37, 18) => 37,
            _ => return Err("Unsupported annotation save-version/profile combination.".into()),
        };
        let start = usize::try_from(header.payload_span.offset)
            .map_err(|_| "Annotation source offset exceeds address space.")?;
        let length = usize::try_from(header.payload_span.length)
            .map_err(|_| "Annotation source length exceeds address space.")?;
        let end = start
            .checked_add(length)
            .ok_or("Annotation source range overflow.")?;
        let body = data
            .get(start..end)
            .ok_or("Truncated annotation payload.")?;
        if body.len() < width {
            return Err("Truncated annotation attributes.".into());
        }
        let attributes = decode_attributes(data, &header.payload_span, profile)?;
        let mut cursor = Cursor {
            data: body,
            offset: width,
            base: header.payload_span.offset,
        };
        let geometry = if header.type_id == POLYLINE {
            AnnotationGeometry::Polyline(decode_polyline(&mut cursor, save_major, options)?)
        } else {
            AnnotationGeometry::Text(decode_text(&mut cursor, save_major, options)?)
        };
        if cursor.offset != body.len() {
            return Err("Unsupported trailing annotation payload bytes.".into());
        }
        let budget_used = match &geometry {
            AnnotationGeometry::Polyline(polyline) => polyline.vertices.len() as u64,
            AnnotationGeometry::Text(_) => 1,
        };
        Ok(AnnotationEntity {
            attributes,
            geometry,
            budget_used,
        })
    })())
}

fn decode_polyline(
    c: &mut Cursor<'_>,
    major: u32,
    options: ParseOptions,
) -> Result<PolylineGeometry, AnnotationError> {
    let start = c.offset;
    let count = c.u32()?;
    ParseOptions::check_limit("max_records", u64::from(count), options.max_records)
        .map_err(AnnotationError::Limit)?;
    let stride = if major <= 10 { 48_u64 } else { 40 };
    let required = u64::from(count) * stride + 4 + if major > 10 { 24 } else { 0 };
    if required != (c.data.len() - c.offset) as u64 {
        return Err("POLYLINE vertex count does not match the versioned payload length.".into());
    }
    let mut vertices = Vec::new();
    for _ in 0..count {
        let offset = c.offset;
        let point = [c.f64()?, c.f64()?];
        let legacy_z = if major <= 10 { Some(c.f64()?) } else { None };
        if legacy_z.is_some_and(|z| z != 0.0) {
            return Err("Nonzero POLYLINE legacy Z is outside the qualified 2D profile.".into());
        }
        let bulge = c.f64()?;
        let start_width = c.f64()?;
        let end_width = c.f64()?;
        if start_width < 0.0 || end_width < 0.0 {
            return Err("Negative POLYLINE width is not qualified.".into());
        }
        vertices.push(PolylineVertex {
            point,
            legacy_z,
            bulge,
            start_width,
            end_width,
            span: c.span(offset),
        });
    }
    let closed_raw = c.u32()?;
    if closed_raw > 1 {
        return Err("Unsupported POLYLINE closed flag.".into());
    }
    let normal = if major > 10 {
        Some([c.f64()?, c.f64()?, c.f64()?])
    } else {
        None
    };
    if normal.is_some_and(|v| v != [0.0, 0.0, 1.0]) {
        return Err("Nondefault POLYLINE normal requires an unqualified plane transform.".into());
    }
    Ok(PolylineGeometry {
        vertices,
        closed: closed_raw != 0,
        closed_raw,
        normal,
        span: c.span(start),
    })
}

fn decode_text(
    c: &mut Cursor<'_>,
    major: u32,
    options: ParseOptions,
) -> Result<TextGeometry, AnnotationError> {
    ParseOptions::check_limit("max_records", 1, options.max_records)
        .map_err(AnnotationError::Limit)?;
    let start = c.offset;
    let size = c.u32()?;
    ParseOptions::check_limit(
        "max_string_bytes",
        u64::from(size),
        options.max_string_bytes,
    )
    .map_err(AnnotationError::Limit)?;
    if size < 2 || size % 2 != 0 {
        return Err("Invalid TEXT UTF-16 byte count.".into());
    }
    let raw = c.take(size as usize)?;
    if raw[raw.len() - 2..] != [0, 0] {
        return Err("Missing TEXT UTF-16 terminator.".into());
    }
    let units: Vec<u16> = raw[..raw.len() - 2]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.contains(&0) {
        return Err("Embedded TEXT UTF-16 terminator is not qualified.".into());
    }
    let contents = String::from_utf16(&units).map_err(|_| "Invalid TEXT UTF-16 contents.")?;
    let contents_span = c.span(start);
    let text_height_stored = c.f64()?;
    let area_height_stored = c.f64()?;
    let area_width_stored = c.f64()?;
    let rotation = c.f64()?;
    let mode_flags_raw = c.u8()?;
    let alignment_raw = c.u32()?;
    let drawing_scale = c.f64()?;
    let style_status_raw = c.u32()?;
    let style_id_raw = c.u32()? as i32;
    let insertion = [c.f64()?, c.f64()?, c.f64()?];
    let legacy_format_raw = if major <= 10 { Some(c.u32()?) } else { None };
    let line_spacing_raw = c.f64()?;
    let line_spacing_style_raw = c.u32()?;
    let tail_start = c.offset;
    if major >= 15 {
        let flag1 = c.u8()?;
        let value = c.f64()?;
        let flag2 = c.u8()?;
        let _color_raw = c.u32()?;
        if flag1 != 0 || value != 0.0 || flag2 != 0 {
            return Err("Nondefault TEXT formatting extension is not qualified.".into());
        }
    }
    let format_type_id = if major > 10 {
        let id = c.guid()?;
        // Native layouts use a GUID followed by typed override values. Only the
        // empty override list has been qualified against the corpus.
        if c.u32()? != 0 {
            return Err("Nonempty TEXT format override list is not qualified.".into());
        }
        Some(id)
    } else {
        None
    };
    let mut column_flags_raw = None;
    let mut column_width = None;
    let mut column_count = None;
    let mut column_entries_raw = None;
    if major >= 15 {
        // Field files of builds 15.0–17.1 (2026-10, 402 records) store a one-column
        // layout as flags [2, 0, 0], a width, count 1 and one 16-byte entry (a
        // double and eight zero bytes). The layout is kept raw and not applied.
        let flags = [c.u32()?, c.u32()?, c.u32()?];
        let width = c.f64()?;
        let count = c.u32()?;
        ParseOptions::check_limit("max_records", u64::from(count), options.max_records)
            .map_err(AnnotationError::Limit)?;
        if u64::from(count) > (c.data.len() - c.offset) as u64 / 16 {
            return Err("TEXT column count exceeds the stored body.".into());
        }
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push([c.f64()?, c.f64()?]);
        }
        column_flags_raw = Some(flags);
        column_width = Some(width);
        column_count = Some(count);
        column_entries_raw = Some(entries);
    }
    Ok(TextGeometry {
        contents,
        contents_span,
        insertion,
        text_height_stored,
        area_height_stored,
        area_width_stored,
        rotation,
        drawing_scale,
        mode_flags_raw,
        alignment_raw,
        writing_direction: (mode_flags_raw >> 3) & 1,
        fit_mode: if mode_flags_raw & 0x40 != 0 {
            2
        } else {
            (mode_flags_raw >> 5) & 1
        },
        style_status_raw,
        style_id_raw,
        legacy_format_raw,
        line_spacing_raw,
        line_spacing_style_raw,
        format_type_id,
        column_flags_raw,
        column_width,
        column_count,
        column_entries_raw,
        formatting_span: c.span(tail_start),
        span: c.span(start),
    })
}

struct Cursor<'a> {
    data: &'a [u8],
    offset: usize,
    base: u64,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("Annotation length overflow.")?;
        let bytes = self
            .data
            .get(self.offset..end)
            .ok_or("Truncated annotation field.")?;
        self.offset = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        let value = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !value.is_finite() {
            return Err("Non-finite annotation coordinate or value.".into());
        }
        Ok(value)
    }
    fn guid(&mut self) -> Result<String, String> {
        let b = self.take(16)?;
        Ok(format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u32::from_le_bytes(b[0..4].try_into().unwrap()),
            u16::from_le_bytes(b[4..6].try_into().unwrap()),
            u16::from_le_bytes(b[6..8].try_into().unwrap()),
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        ))
    }
    fn span(&self, start: usize) -> SourceSpan {
        SourceSpan {
            offset: self.base + start as u64,
            length: (self.offset - start) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polyline(major: u32) -> Vec<u8> {
        let mut b = 2_u32.to_le_bytes().to_vec();
        for xy in [[1.25_f64, -2.5], [3.75, 4.5]] {
            for value in xy {
                b.extend(value.to_le_bytes());
            }
            if major <= 10 {
                b.extend(0_f64.to_le_bytes());
            }
            for value in [0.5_f64, 0.25, 0.75] {
                b.extend(value.to_le_bytes());
            }
        }
        b.extend(1_u32.to_le_bytes());
        if major > 10 {
            for value in [0_f64, 0., 1.] {
                b.extend(value.to_le_bytes());
            }
        }
        b
    }
    fn cursor(data: &[u8]) -> Cursor<'_> {
        Cursor {
            data,
            offset: 0,
            base: 100,
        }
    }
    fn text(major: u32) -> Vec<u8> {
        let mut b = 8_u32.to_le_bytes().to_vec();
        for value in "寸法Ω".encode_utf16().chain([0]) {
            b.extend(value.to_le_bytes());
        }
        for value in [3.5_f64, 4., 8., 0.25] {
            b.extend(value.to_le_bytes());
        }
        b.push(0x48);
        b.extend(2_u32.to_le_bytes());
        b.extend(0.5_f64.to_le_bytes());
        b.extend(0x800000_u32.to_le_bytes());
        b.extend((-33_i32).to_le_bytes());
        for value in [10_f64, 20., 1e-28] {
            b.extend(value.to_le_bytes());
        }
        if major <= 10 {
            b.extend(0_u32.to_le_bytes());
        }
        b.extend((-1_f64).to_le_bytes());
        b.extend(0_u32.to_le_bytes());
        if major >= 15 {
            b.extend([0_u8; 10]);
            b.extend(0x10ffffff_u32.to_le_bytes());
        }
        if major > 10 {
            b.extend([0_u8; 20]);
        }
        if major >= 15 {
            b.extend([0_u8; 24]);
        }
        b
    }

    #[test]
    fn text_keeps_a_stored_column_layout_raw() {
        // A build 17.1 field record: flags [2, 0, 0], width 12.5, one column entry.
        let mut b = text(17);
        b.truncate(b.len() - 24);
        for value in [2_u32, 0, 0] {
            b.extend(value.to_le_bytes());
        }
        b.extend(12.5_f64.to_le_bytes());
        b.extend(1_u32.to_le_bytes());
        b.extend(9692.5_f64.to_le_bytes());
        b.extend(0_f64.to_le_bytes());
        let mut c = cursor(&b);
        let t = decode_text(&mut c, 17, ParseOptions::default()).unwrap();
        assert_eq!(c.offset, b.len());
        assert_eq!(t.contents, "寸法Ω");
        assert_eq!(t.column_flags_raw, Some([2, 0, 0]));
        assert_eq!(t.column_width, Some(12.5));
        assert_eq!(t.column_count, Some(1));
        assert_eq!(t.column_entries_raw, Some(vec![[9692.5, 0.]]));
        for end in 0..b.len() {
            assert!(decode_text(&mut cursor(&b[..end]), 17, ParseOptions::default()).is_err());
        }
        let plain = decode_text(&mut cursor(&text(17)), 17, ParseOptions::default()).unwrap();
        assert_eq!(
            (plain.column_count, plain.column_width),
            (Some(0), Some(0.))
        );
        assert_eq!(plain.column_entries_raw, Some(Vec::new()));
        let legacy = decode_text(&mut cursor(&text(11)), 11, ParseOptions::default()).unwrap();
        assert_eq!(legacy.column_count, None);
        let huge = {
            let mut bad = b.clone();
            let at = bad.len() - 16 - 4;
            bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            bad
        };
        assert!(decode_text(&mut cursor(&huge), 17, ParseOptions::default()).is_err());
    }

    #[test]
    fn polyline_versioned_coordinates_and_bulges() {
        for major in [10, 11, 15, 16, 18] {
            let b = polyline(major);
            let mut c = cursor(&b);
            let p = decode_polyline(&mut c, major, ParseOptions::default()).unwrap();
            assert_eq!(p.vertices[1].point, [3.75, 4.5]);
            assert_eq!(p.vertices[0].bulge, 0.5);
            assert_eq!(p.vertices[0].start_width, 0.25);
            assert_eq!(p.vertices[0].end_width, 0.75);
            assert!(p.closed);
            assert_eq!(c.offset, b.len());
            assert_eq!(p.vertices[0].span.offset, 104);
        }
    }

    #[test]
    fn polyline_rejects_truncation_count_nonfinite_z_and_normal() {
        for major in [10, 18] {
            let b = polyline(major);
            for end in 0..b.len() {
                assert!(
                    decode_polyline(&mut cursor(&b[..end]), major, ParseOptions::default())
                        .is_err()
                );
            }
            let mut bad = b.clone();
            bad[..4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(decode_polyline(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            let mut bad = b.clone();
            bad[4..12].copy_from_slice(&f64::NAN.to_le_bytes());
            assert!(decode_polyline(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            let mut bad = b.clone();
            let offset = if major <= 10 { 20 } else { bad.len() - 8 };
            bad[offset..offset + 8].copy_from_slice(&2_f64.to_le_bytes());
            assert!(decode_polyline(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            assert!(decode_polyline(
                &mut cursor(&b),
                major,
                ParseOptions {
                    max_records: 1,
                    ..ParseOptions::default()
                }
            )
            .is_err());
        }
    }

    #[test]
    fn text_keeps_stored_values_literal_contents_and_xyz() {
        for major in [10, 11, 15, 16, 18] {
            let b = text(major);
            let mut c = cursor(&b);
            let t = decode_text(&mut c, major, ParseOptions::default()).unwrap();
            assert_eq!(t.contents, "寸法Ω");
            assert_eq!(t.insertion, [10., 20., 1e-28]);
            assert_eq!(t.text_height_stored, 3.5);
            assert_eq!(t.drawing_scale, 0.5);
            assert_eq!(t.style_id_raw, -33);
            assert_eq!(t.line_spacing_raw, -1.);
            assert_eq!((t.writing_direction, t.fit_mode), (1, 2));
            assert_eq!(
                t.formatting_span.length,
                if major <= 10 {
                    0
                } else if major < 15 {
                    20
                } else {
                    58
                }
            );
            assert_eq!(c.offset, b.len());
        }
    }

    #[test]
    fn text_rejects_truncation_invalid_utf16_and_unsupported_extensions() {
        for major in [10, 18] {
            let b = text(major);
            for end in 0..b.len() {
                assert!(
                    decode_text(&mut cursor(&b[..end]), major, ParseOptions::default()).is_err()
                );
            }
            assert!(decode_text(
                &mut cursor(&b),
                major,
                ParseOptions {
                    max_string_bytes: 6,
                    ..ParseOptions::default()
                }
            )
            .is_err());
            let mut bad = b.clone();
            bad[4..6].copy_from_slice(&0xd800_u16.to_le_bytes());
            assert!(decode_text(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            let mut bad = b.clone();
            bad[10] = 1;
            assert!(decode_text(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            let mut bad = b.clone();
            bad[12..20].copy_from_slice(&f64::INFINITY.to_le_bytes());
            assert!(decode_text(&mut cursor(&bad), major, ParseOptions::default()).is_err());
            if major >= 15 {
                for offset in [b.len() - 58, b.len() - 28, b.len() - 4] {
                    let mut bad = b.clone();
                    bad[offset] = 1;
                    assert!(
                        decode_text(&mut cursor(&bad), major, ParseOptions::default()).is_err()
                    );
                }
            }
        }
    }

    #[test]
    fn annotation_resource_limits_remain_hard_errors() {
        let polyline = polyline(18);
        let text = text(18);
        assert!(matches!(
            decode_polyline(
                &mut cursor(&polyline),
                18,
                ParseOptions {
                    max_records: 1,
                    ..ParseOptions::default()
                }
            ),
            Err(AnnotationError::Limit(Error::ResourceLimit {
                name: "max_records",
                ..
            }))
        ));
        assert!(matches!(
            decode_text(
                &mut cursor(&text),
                18,
                ParseOptions {
                    max_records: 0,
                    ..ParseOptions::default()
                }
            ),
            Err(AnnotationError::Limit(Error::ResourceLimit {
                name: "max_records",
                ..
            }))
        ));
        assert!(matches!(
            decode_text(
                &mut cursor(&text),
                18,
                ParseOptions {
                    max_string_bytes: 2,
                    ..ParseOptions::default()
                }
            ),
            Err(AnnotationError::Limit(Error::ResourceLimit {
                name: "max_string_bytes",
                ..
            }))
        ));
    }
}

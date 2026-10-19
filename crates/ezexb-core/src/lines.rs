//! Experimental decoding of one observed LINE payload layout.
use serde::Serialize;

use crate::{
    inspect_record_headers_with_profile, Error, ParseOptions, RecordInspection, RecordProfile,
    SourceSpan,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineAttributes {
    pub span: SourceSpan,
    pub layer_status_raw: u32,
    pub layer_id_raw: i32,
    pub linetype_status_raw: u32,
    pub linetype_id_raw: i32,
    /// Stored value; resolving ByLayer/ByBlock requires drawing context.
    pub color_raw: u32,
    /// Stored value, not the resolved display width or a qualified unit.
    /// Stored float32 promoted exactly to float64 for Rust/Python/JSON agreement.
    pub line_width_raw: f64,
    pub linetype_flag_raw: u8,
    pub line_width_flag_raw: u8,
    pub color_flag_raw: u8,
    pub hide_flag_raw: u16,
    /// Stored float32 promoted exactly to float64.
    pub linetype_scale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineRecord {
    /// Index in `LineInspection.framing.records` for identity and all source spans.
    pub record_index: usize,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub coordinates_span: SourceSpan,
    pub attributes: LineAttributes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnsupportedLine {
    pub record_index: usize,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineInspection {
    pub profile: String,
    pub evidence: String,
    /// Always false: other entities, spaces, units, and inherited attributes are unresolved.
    pub drawing_complete: bool,
    pub framing: RecordInspection,
    pub lines: Vec<LineRecord>,
    pub unsupported_lines: Vec<UnsupportedLine>,
    pub other_record_count: usize,
}

/// Decode only 81-byte CxDbLine bodies in the consecutively framed stream prefix.
/// Unknown schemas stop framing; unknown LINE payload layouts remain explicit issues.
/// Coordinates are stored XYZ values, without unit or space transformations.
pub fn inspect_lines(data: &[u8], options: ParseOptions) -> Result<LineInspection, Error> {
    inspect_lines_with_profile(data, options, None)
}

/// Decode supported top-level LINE bodies using the selected framing profile.
/// Nested display-cache records remain in `framing` and are not drawing LINEs.
pub fn inspect_lines_with_profile(
    data: &[u8],
    options: ParseOptions,
    profile: Option<RecordProfile>,
) -> Result<LineInspection, Error> {
    let framing = inspect_record_headers_with_profile(data, options, profile)?;
    let mut lines = Vec::new();
    let mut unsupported_lines = Vec::new();
    let mut other_record_count = 0;
    for (record_index, record) in framing.records.iter().enumerate() {
        if record.class_name.as_deref() != Some("CxDbLine") {
            other_record_count += 1;
            continue;
        }
        let issue = |reason: &str| UnsupportedLine {
            record_index,
            reason: reason.into(),
        };
        if record.object_schema != 1 || record.payload_span.length != 81 {
            let reason = if record.object_schema == 100 && record.payload_span.length == 85 {
                "Unsupported LINE object schema 100 with 85-byte body; the additional attribute field is outside the 81-byte profile."
            } else {
                "Unsupported LINE payload layout; expected object schema 1 and 81 bytes."
            };
            unsupported_lines.push(issue(reason));
            continue;
        }
        // The framing parser has already validated the source range.
        let offset = record.payload_span.offset as usize;
        let body = &data[offset..offset + 81];
        let u32_at = |i| u32::from_le_bytes(body[i..i + 4].try_into().unwrap());
        let f32_at = |i| f32::from_bits(u32_at(i));
        let f64_at = |i| f64::from_le_bytes(body[i..i + 8].try_into().unwrap());
        let start = [f64_at(33), f64_at(41), f64_at(49)];
        let end = [f64_at(57), f64_at(65), f64_at(73)];
        if start.iter().chain(end.iter()).any(|x| !x.is_finite())
            || !f32_at(20).is_finite()
            || !f32_at(29).is_finite()
        {
            unsupported_lines.push(issue("Non-finite LINE coordinate or attribute."));
            continue;
        }
        lines.push(LineRecord {
            record_index,
            start,
            end,
            coordinates_span: SourceSpan {
                offset: offset as u64 + 33,
                length: 48,
            },
            attributes: LineAttributes {
                span: SourceSpan {
                    offset: offset as u64,
                    length: 33,
                },
                layer_status_raw: u32_at(0),
                layer_id_raw: u32_at(4) as i32,
                linetype_status_raw: u32_at(8),
                linetype_id_raw: u32_at(12) as i32,
                color_raw: u32_at(16),
                line_width_raw: f64::from(f32_at(20)),
                linetype_flag_raw: body[24],
                line_width_flag_raw: body[25],
                color_flag_raw: body[26],
                hide_flag_raw: u16::from_le_bytes(body[27..29].try_into().unwrap()),
                linetype_scale: f64::from(f32_at(29)),
            },
        });
    }
    Ok(LineInspection {
        profile: "cxdbline-81-experimental".into(),
        evidence: "corpus_consistent_with_native_xy_spot_checks".into(),
        drawing_complete: false,
        framing,
        lines,
        unsupported_lines,
        other_record_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![8, 0, 0, 0, b'0', 0, b'.', 0, b'0', 0, 0, 0];
        bytes.resize(172, 0);
        bytes.extend(
            uuid::Uuid::parse_str("17ecf72d-c046-4e9a-8416-fd8220fd33b5")
                .unwrap()
                .to_bytes_le(),
        );
        bytes.extend(17i32.to_le_bytes());
        bytes.extend((-5i32).to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend((payload.len() as u32).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn body() -> Vec<u8> {
        let mut bytes = vec![0; 81];
        bytes[4..8].copy_from_slice(&(-24i32).to_le_bytes());
        bytes[12..16].copy_from_slice(&(-19i32).to_le_bytes());
        bytes[16..20].copy_from_slice(&0x80ffffffu32.to_le_bytes());
        bytes[20..24].copy_from_slice(&0.5f32.to_le_bytes());
        bytes[24..29].copy_from_slice(&[2, 1, 0, 0x34, 0x12]);
        bytes[29..33].copy_from_slice(&1.25f32.to_le_bytes());
        for (i, value) in [1.25f64, -2.5, 3.0, 4.75, 5.5, -6.0].iter().enumerate() {
            bytes[33 + i * 8..41 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn coordinates_attributes_and_original_source_offsets() {
        let result = inspect_lines(&input(&body()), ParseOptions::default()).unwrap();
        assert!(!result.drawing_complete);
        assert!(result.framing.framing_complete);
        assert!(!result.framing.geometry_decoded);
        let line = &result.lines[0];
        assert_eq!(line.start, [1.25, -2.5, 3.0]);
        assert_eq!(line.end, [4.75, 5.5, -6.0]);
        assert_eq!(
            line.coordinates_span,
            SourceSpan {
                offset: 235,
                length: 48
            }
        );
        assert_eq!(line.attributes.layer_id_raw, -24);
        assert_eq!(line.attributes.linetype_id_raw, -19);
        assert_eq!(line.attributes.color_raw, 0x80ffffff);
        assert_eq!(line.attributes.line_width_raw, 0.5);
        assert_eq!(line.attributes.linetype_scale, 1.25);
        assert_eq!(line.attributes.linetype_flag_raw, 2);
        assert_eq!(line.attributes.line_width_flag_raw, 1);
        assert_eq!(line.attributes.hide_flag_raw, 0x1234);
    }

    #[test]
    fn unsupported_layouts_and_non_finite_values_are_not_geometry() {
        for payload in [vec![0; 80], vec![0; 85]] {
            let result = inspect_lines(&input(&payload), ParseOptions::default()).unwrap();
            assert!(result.lines.is_empty());
            assert_eq!(result.unsupported_lines.len(), 1);
        }
        for offset in [33, 65] {
            let mut payload = body();
            payload[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
            let result = inspect_lines(&input(&payload), ParseOptions::default()).unwrap();
            assert!(result.lines.is_empty());
            assert!(result.unsupported_lines[0].reason.contains("Non-finite"));
        }
        let mut payload = body();
        payload[29..33].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(inspect_lines(&input(&payload), ParseOptions::default())
            .unwrap()
            .lines
            .is_empty());
    }

    #[test]
    fn schema100_line_body_has_an_explicit_unsupported_reason() {
        let original = input(&[0; 85]);
        let mut data = original[..196].to_vec();
        data.extend([100, 1]);
        data.extend(0u32.to_le_bytes()); // No property groups.
        data.extend_from_slice(&original[198..]);
        let result = inspect_lines(&data, ParseOptions::default()).unwrap();
        assert!(result.framing.framing_complete);
        assert!(result.lines.is_empty());
        assert_eq!(result.unsupported_lines.len(), 1);
        assert!(result.unsupported_lines[0]
            .reason
            .contains("schema 100 with 85-byte body"));
        assert!(!result.drawing_complete);
    }
}

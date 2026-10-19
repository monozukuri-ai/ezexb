//! Bounded planar primitive decoding from the observed native serializer layouts.
//! Stored coordinates are not transformed into a drawing's model or paper space.
use serde::Serialize;

use crate::{RecordHeader, RecordProfile, SourceSpan};

const LINE: &str = "17ecf72d-c046-4e9a-8416-fd8220fd33b5";
const CIRCLE: &str = "7587733c-3df6-4c9d-8b08-20aeffe18291";
const ARC: &str = "91d6556b-0fb8-4e13-998f-bdacbfd31025";
const POINT: &str = "d6da372e-0f4f-4bfb-9e92-3b3363ac6c22";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EntityAttributes {
    pub span: SourceSpan,
    pub layer_status_raw: u32,
    pub layer_id_raw: i32,
    pub linetype_status_raw: u32,
    pub linetype_id_raw: i32,
    pub color_raw: u32,
    /// Stored float32 promoted exactly to float64, without resolving inheritance.
    pub line_width_raw: f64,
    pub linetype_flag_raw: u8,
    pub line_width_flag_raw: u8,
    pub color_flag_raw: u8,
    pub hide_flag_raw: u16,
    pub linetype_scale: f64,
    /// The additional four bytes in the 37-byte attribute layout. Meaning unknown.
    pub extension_raw: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum PrimitiveGeometry {
    /// The qualified zero-radius/zero-basis ARC storage pattern. This preserves
    /// saved values without declaring a valid curve or inventing endpoints.
    DegenerateArc {
        center: [f64; 3],
        radius: f64,
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        sweep_angle: f64,
    },
    Point {
        point: [f64; 3],
    },
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    Circle {
        center: [f64; 3],
        radius: f64,
        x_axis: [f64; 3],
        y_axis: [f64; 3],
    },
    Arc {
        center: [f64; 3],
        radius: f64,
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        /// Positive parameter sweep in radians. The axes retain handedness;
        /// a left-handed XY basis therefore describes a clockwise arc.
        sweep_angle: f64,
        start: [f64; 3],
        end: [f64; 3],
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrimitiveEntity {
    pub attributes: EntityAttributes,
    pub geometry: PrimitiveGeometry,
}

pub(crate) fn attribute_width(profile: RecordProfile) -> usize {
    match profile {
        RecordProfile::Schema3Attributes33 => 33,
        RecordProfile::Schema3Attributes37 => 37,
    }
}

fn payload<'a>(data: &'a [u8], span: &SourceSpan) -> Result<&'a [u8], String> {
    let start =
        usize::try_from(span.offset).map_err(|_| "Payload offset exceeds address space.")?;
    let size = usize::try_from(span.length).map_err(|_| "Payload size exceeds address space.")?;
    let end = start.checked_add(size).ok_or("Payload range overflow.")?;
    data.get(start..end)
        .ok_or_else(|| "Payload range exceeds the decoded stream.".into())
}

/// Decode only the attribute prefix of a fully bounded entity payload.
/// The extension word and all relation/style flags remain uninterpreted values.
pub(crate) fn decode_attributes(
    data: &[u8],
    payload_span: &SourceSpan,
    profile: RecordProfile,
) -> Result<EntityAttributes, String> {
    let width = attribute_width(profile);
    let body = payload(data, payload_span)?;
    let body = body
        .get(..width)
        .ok_or("Truncated entity attribute prefix.")?;
    let word = |offset| u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
    let line_width_raw = f64::from(f32::from_bits(word(20)));
    let linetype_scale = f64::from(f32::from_bits(word(29)));
    if !line_width_raw.is_finite() || !linetype_scale.is_finite() {
        return Err("Non-finite entity attribute value.".into());
    }
    Ok(EntityAttributes {
        span: SourceSpan {
            offset: payload_span.offset,
            length: width as u64,
        },
        layer_status_raw: word(0),
        layer_id_raw: word(4) as i32,
        linetype_status_raw: word(8),
        linetype_id_raw: word(12) as i32,
        color_raw: word(16),
        line_width_raw,
        linetype_flag_raw: body[24],
        line_width_flag_raw: body[25],
        color_flag_raw: body[26],
        hide_flag_raw: u16::from_le_bytes(body[27..29].try_into().unwrap()),
        linetype_scale,
        extension_raw: (width == 37).then(|| word(33)),
    })
}

/// Return None for other classes, and an explicit diagnostic for a recognized
/// primitive outside the observed planar layout. Never infer width from length.
pub(crate) fn decode_primitive(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
) -> Option<Result<PrimitiveEntity, String>> {
    let kind = header.type_id.as_str();
    if !matches!(kind, LINE | CIRCLE | ARC | POINT) {
        return None;
    }
    Some((|| {
        if !matches!(header.object_schema, 1 | 100) {
            return Err(format!(
                "Unsupported primitive object schema {}.",
                header.object_schema
            ));
        }
        let width = attribute_width(profile);
        let geometry_size = match kind {
            POINT => 24,
            LINE => 48,
            _ => 88,
        };
        if header.payload_span.length != (width + geometry_size) as u64 {
            return Err(format!(
                "Unsupported primitive body length {} for {} (expected {}).",
                header.payload_span.length,
                profile.as_str(),
                width + geometry_size
            ));
        }
        let body = payload(data, &header.payload_span)?;
        let attributes = decode_attributes(data, &header.payload_span, profile)?;
        let number = |offset| {
            f64::from_le_bytes(body[width + offset..width + offset + 8].try_into().unwrap())
        };
        let vector = |offset| [number(offset), number(offset + 8), number(offset + 16)];
        let geometry = if kind == POINT {
            let point = vector(0);
            if point.iter().any(|v| !v.is_finite()) {
                return Err("Non-finite POINT coordinate.".into());
            }
            // The native point loader reads XYZ, then discards Z.
            if point[2] != 0.0 {
                return Err(
                    "Non-planar POINT coordinates are outside the supported profile.".into(),
                );
            }
            PrimitiveGeometry::Point { point }
        } else if kind == LINE {
            let start = vector(0);
            let end = vector(24);
            if start.iter().chain(&end).any(|value| !value.is_finite()) {
                return Err("Non-finite LINE coordinate.".into());
            }
            // The native planar loader discards Z. Refuse to silently do so.
            if start[2] != 0.0 || end[2] != 0.0 {
                return Err(
                    "Non-planar LINE coordinates are outside the supported profile.".into(),
                );
            }
            PrimitiveGeometry::Line { start, end }
        } else {
            let center = vector(0);
            let radius = number(24);
            let x_axis = vector(32);
            let y_axis = vector(56);
            let sweep_angle = number(80);
            if center
                .iter()
                .chain(&x_axis)
                .chain(&y_axis)
                .chain([&radius, &sweep_angle])
                .any(|value| !value.is_finite())
            {
                return Err("Non-finite circular primitive parameter.".into());
            }
            if radius <= 0.0 {
                if kind == ARC
                    && radius == 0.0
                    && center[2] == 0.0
                    && x_axis == [0.0; 3]
                    && y_axis == [0.0; 3]
                    && sweep_angle == std::f64::consts::TAU
                {
                    return Ok(PrimitiveEntity {
                        attributes,
                        geometry: PrimitiveGeometry::DegenerateArc {
                            center,
                            radius,
                            x_axis,
                            y_axis,
                            sweep_angle,
                        },
                    });
                }
                return Err("Degenerate circular primitive: radius must be positive.".into());
            }
            if center[2] != 0.0 || x_axis[2] != 0.0 || y_axis[2] != 0.0 {
                return Err(
                    "Non-planar circular primitive is outside the supported profile.".into(),
                );
            }
            let x_norm = x_axis[0].hypot(x_axis[1]);
            let y_norm = y_axis[0].hypot(y_axis[1]);
            let dot = x_axis[0] * y_axis[0] + x_axis[1] * y_axis[1];
            if (x_norm - 1.0).abs() > 1e-9 || (y_norm - 1.0).abs() > 1e-9 || dot.abs() > 1e-9 {
                return Err("Circular primitive axes are not an orthonormal XY basis.".into());
            }
            if !(0.0 < sweep_angle && sweep_angle <= std::f64::consts::TAU) {
                return Err("Circular primitive sweep is outside (0, 2*pi] radians.".into());
            }
            if kind == CIRCLE {
                if sweep_angle != std::f64::consts::TAU {
                    return Err("CIRCLE does not contain the observed full-turn sweep.".into());
                }
                PrimitiveGeometry::Circle {
                    center,
                    radius,
                    x_axis,
                    y_axis,
                }
            } else {
                let (sin, cos) = sweep_angle.sin_cos();
                let start = std::array::from_fn(|i| center[i] + radius * x_axis[i]);
                let end = std::array::from_fn(|i| {
                    center[i] + radius * (cos * x_axis[i] + sin * y_axis[i])
                });
                if start
                    .iter()
                    .chain(&end)
                    .any(|value: &f64| !value.is_finite())
                {
                    return Err("Circular primitive endpoint computation overflowed.".into());
                }
                PrimitiveGeometry::Arc {
                    center,
                    radius,
                    x_axis,
                    y_axis,
                    sweep_angle,
                    start,
                    end,
                }
            }
        };
        Ok(PrimitiveEntity {
            attributes,
            geometry,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inspect_record_headers, ParseOptions};

    fn fixture(kind: &str, schema: u8, width: usize, values: &[f64]) -> (Vec<u8>, RecordHeader) {
        let mut data = vec![8, 0, 0, 0, b'0', 0, b'.', 0, b'0', 0, 0, 0];
        data.resize(172, 0);
        data.extend(uuid::Uuid::parse_str(kind).unwrap().to_bytes_le());
        data.extend(17i32.to_le_bytes());
        data.extend((-5i32).to_le_bytes());
        data.extend([schema, 1]);
        data.extend(0u32.to_le_bytes());
        data.extend(((width + values.len() * 8) as u32).to_le_bytes());
        let start = data.len();
        data.resize(start + width, 0);
        data[start + 4..start + 8].copy_from_slice(&(-24i32).to_le_bytes());
        data[start + 20..start + 24].copy_from_slice(&0.6f32.to_le_bytes());
        data[start + 29..start + 33].copy_from_slice(&1.25f32.to_le_bytes());
        if width == 37 {
            data[start + 33..start + 37].copy_from_slice(&0x12345678u32.to_le_bytes());
        }
        for value in values {
            data.extend(value.to_le_bytes());
        }
        let header = inspect_record_headers(&data, ParseOptions::default())
            .unwrap()
            .records
            .remove(0);
        (data, header)
    }

    #[test]
    fn degenerate_arc_storage_is_preserved_without_becoming_a_normal_arc() {
        let values = [
            2.,
            -3.5,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            std::f64::consts::TAU,
        ];
        for (width, profile) in [
            (33, RecordProfile::Schema3Attributes33),
            (37, RecordProfile::Schema3Attributes37),
        ] {
            let (data, header) = fixture(ARC, 1, width, &values);
            let entity = decode_primitive(&data, &header, profile).unwrap().unwrap();
            assert_eq!(
                entity.geometry,
                PrimitiveGeometry::DegenerateArc {
                    center: [2., -3.5, 0.],
                    radius: 0.,
                    x_axis: [0.; 3],
                    y_axis: [0.; 3],
                    sweep_angle: std::f64::consts::TAU,
                }
            );
            assert!(crate::transform_primitive(
                &entity.geometry,
                &crate::WorldTransform::identity()
            )
            .is_err());
            for end in 0..data.len() {
                assert!(decode_primitive(&data[..end], &header, profile)
                    .unwrap()
                    .is_err());
            }
            for (index, value) in [
                (0, f64::NAN),
                (2, 1.),
                (3, -1.),
                (4, 1.),
                (10, 0.),
                (10, 1.),
            ] {
                let mut altered = values;
                altered[index] = value;
                let (data, header) = fixture(ARC, 1, width, &altered);
                assert!(decode_primitive(&data, &header, profile).unwrap().is_err());
            }
            let (data, header) = fixture(CIRCLE, 1, width, &values);
            assert!(decode_primitive(&data, &header, profile).unwrap().is_err());
        }
    }

    #[test]
    fn point_profiles_are_exact_bounded_and_planar() {
        for (width, profile) in [
            (33, RecordProfile::Schema3Attributes33),
            (37, RecordProfile::Schema3Attributes37),
        ] {
            let (data, header) = fixture(POINT, 100, width, &[1.25, -2.5, 0.]);
            let point = decode_primitive(&data, &header, profile).unwrap().unwrap();
            assert_eq!(
                point.geometry,
                PrimitiveGeometry::Point {
                    point: [1.25, -2.5, 0.]
                }
            );
            assert_eq!(point.attributes.layer_id_raw, -24);
            for n in 0..data.len() {
                assert!(decode_primitive(&data[..n], &header, profile)
                    .unwrap()
                    .is_err());
            }
            for values in [
                &[1., 2.][..],
                &[1., 2., 0., 4.],
                &[1., 2., 3.],
                &[f64::NAN, 2., 0.],
                &[1., f64::INFINITY, 0.],
            ] {
                let (data, header) = fixture(POINT, 1, width, values);
                assert!(decode_primitive(&data, &header, profile).unwrap().is_err());
            }
            let (data, mut header) = fixture(POINT, 1, width, &[1., 2., 0.]);
            header.object_schema = 2;
            assert!(decode_primitive(&data, &header, profile).unwrap().is_err());
        }
    }

    #[test]
    fn line_85_preserves_extra_attribute_and_exact_float_promotion() {
        let (data, header) = fixture(LINE, 100, 37, &[1.25, -2.5, 0.0, 4.25, 1.5, 0.0]);
        let entity = decode_primitive(&data, &header, RecordProfile::Schema3Attributes37)
            .unwrap()
            .unwrap();
        assert_eq!(entity.attributes.extension_raw, Some(0x12345678));
        assert_eq!(entity.attributes.line_width_raw, f64::from(0.6f32));
        assert_eq!(entity.attributes.span.length, 37);
        assert_eq!(
            entity.geometry,
            PrimitiveGeometry::Line {
                start: [1.25, -2.5, 0.0],
                end: [4.25, 1.5, 0.0]
            }
        );
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn circle_and_clockwise_arc_preserve_basis_and_radian_sweep() {
        let values = [
            1.0,
            2.0,
            0.0,
            5.0,
            0.0,
            1.0,
            0.0,
            1.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ];
        let (data, header) = fixture(ARC, 1, 33, &values);
        let entity = decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
            .unwrap()
            .unwrap();
        let PrimitiveGeometry::Arc {
            start,
            end,
            sweep_angle,
            ..
        } = entity.geometry
        else {
            panic!("ARC expected")
        };
        assert_eq!(start, [1.0, 7.0, 0.0]);
        assert!((end[0] - 6.0).abs() < 1e-14 && (end[1] - 2.0).abs() < 1e-14);
        assert_eq!(sweep_angle, std::f64::consts::FRAC_PI_2);
        let mut circle_values = values;
        circle_values[10] = std::f64::consts::TAU;
        let (data, header) = fixture(CIRCLE, 100, 37, &circle_values);
        let entity = decode_primitive(&data, &header, RecordProfile::Schema3Attributes37)
            .unwrap()
            .unwrap();
        assert!(matches!(
            entity.geometry,
            PrimitiveGeometry::Circle { radius: 5.0, .. }
        ));
        assert_eq!(
            serde_json::to_value(&entity.geometry).unwrap()["kind"],
            "Circle"
        );
    }

    #[test]
    fn invalid_planarity_basis_sweep_and_numeric_values_are_explicit() {
        let baseline = [
            1.0,
            2.0,
            0.0,
            5.0,
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ];
        for (index, value) in [
            (0, f64::NAN),
            (2, 1.0),
            (3, 0.0),
            (3, -1.0),
            (4, 2.0),
            (7, 1.0),
            (10, 0.0),
            (10, 7.0),
        ] {
            let mut values = baseline;
            values[index] = value;
            let (data, header) = fixture(ARC, 1, 33, &values);
            assert!(
                decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                    .unwrap()
                    .is_err()
            );
        }
        let (data, header) = fixture(CIRCLE, 1, 33, &baseline);
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .unwrap_err()
                .contains("full-turn")
        );
        let (data, header) = fixture(LINE, 1, 33, &[0.0, 0.0, 1.0, 1.0, 2.0, 0.0]);
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .unwrap_err()
                .contains("Non-planar")
        );
    }

    #[test]
    fn caller_supplied_spans_and_schema_cannot_bypass_bounds() {
        let (data, mut header) = fixture(LINE, 1, 33, &[0.0; 6]);
        header.object_schema = 3;
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .is_err()
        );
        header.object_schema = 1;
        header.payload_span.offset = u64::MAX;
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .is_err()
        );
        let (data, header) = fixture(LINE, 1, 33, &[0.0; 6]);
        assert!(decode_primitive(
            &data[..data.len() - 1],
            &header,
            RecordProfile::Schema3Attributes33
        )
        .unwrap()
        .is_err());
        let mut unknown = header;
        unknown.type_id = "00000000-0000-0000-0000-000000000000".into();
        assert!(decode_primitive(&data, &unknown, RecordProfile::Schema3Attributes33).is_none());
    }

    #[test]
    fn finite_storage_does_not_allow_derived_overflow_or_nonfinite_attributes() {
        let (mut data, header) = fixture(LINE, 1, 33, &[0.0; 6]);
        let attribute_start = header.payload_span.offset as usize;
        for offset in [20, 29] {
            let original = data[attribute_start + offset..attribute_start + offset + 4].to_vec();
            data[attribute_start + offset..attribute_start + offset + 4]
                .copy_from_slice(&f32::INFINITY.to_le_bytes());
            assert!(
                decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                    .unwrap()
                    .unwrap_err()
                    .contains("Non-finite entity attribute")
            );
            data[attribute_start + offset..attribute_start + offset + 4].copy_from_slice(&original);
        }
        let values = [
            f64::MAX,
            0.0,
            0.0,
            f64::MAX,
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ];
        let (data, header) = fixture(ARC, 1, 33, &values);
        assert!(
            decode_primitive(&data, &header, RecordProfile::Schema3Attributes33)
                .unwrap()
                .unwrap_err()
                .contains("overflowed")
        );
    }
}

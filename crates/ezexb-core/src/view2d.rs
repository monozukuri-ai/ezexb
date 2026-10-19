//! Stored two-dimensional views. Clipping and view placement are retained
//! independently of ordinary INSERT expansion.
use crate::complex::{BoundaryCurve, BoundaryGeometry, ComplexError};
use crate::context::Insert;
use crate::geometry::{decode_attributes, EntityAttributes};
use crate::{ParseOptions, RecordHeader, RecordProfile, SourceSpan, TextField};
use serde::Serialize;

pub(crate) const VIEW2D: &str = "bdf700da-e09c-4ade-a1e4-0eadf201ff16";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViewClip {
    /// Two stored 16-byte fields whose coordinate roles are not qualified.
    pub reference_bytes: [u8; 32],
    pub reference_span: SourceSpan,
    pub type_raw: u32,
    /// Native ClipRgn show-border flag, preserved without boolean normalization.
    pub flag_raw: u16,
    /// The qualified circular clip contains one native 1012 curve.
    pub curves: Vec<BoundaryCurve>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "View2D")]
pub struct View2DGeometry {
    /// The inherited block-reference fields, not an instruction to expand the
    /// view as an ordinary INSERT. View clipping remains separate below.
    pub block_reference: Insert,
    pub name: TextField,
    pub view_type_raw: u32,
    pub clip: Option<ViewClip>,
    pub span: SourceSpan,
}

pub(crate) struct View2DEntity {
    pub attributes: EntityAttributes,
    pub geometry: View2DGeometry,
    pub budget_used: u64,
}

pub(crate) fn decode_view2d(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Option<Result<View2DEntity, ComplexError>> {
    if header.type_id != VIEW2D {
        return None;
    }
    Some((|| {
        if major != 10 || profile != RecordProfile::Schema3Attributes33 {
            return Err("Unqualified View2D save-version profile.".into());
        }
        if !matches!(header.object_schema, 1 | 3 | 100)
            || (header.object_schema == 3) != header.extension.is_some()
        {
            return Err("Unqualified View2D record schema.".into());
        }
        let start = usize::try_from(header.payload_span.offset)
            .map_err(|_| "View2D body offset overflow.")?;
        let end = usize::try_from(
            header
                .payload_span
                .offset
                .checked_add(header.payload_span.length)
                .ok_or("View2D body range overflow.")?,
        )
        .map_err(|_| "View2D body range overflow.")?;
        if end > data.len() || end < start || end - start < 33 {
            return Err("Truncated View2D body.".into());
        }
        let attributes = decode_attributes(data, &header.payload_span, profile)?;
        let mut c = Cursor {
            data: &data[..end],
            pos: start + 33,
            options,
            used: 0,
        };
        let start = c.pos;
        // CCxDbView2D has no counted presentation prefix before these fields.
        let rotation = c.f64()?;
        let scales = c.values()?;
        let insertion = c.values()?;
        let matrix_stored = c.values()?;
        let block_status = c.u32()?;
        let block_id = c.i32()?;
        let attribute_count = c.u32()?;
        c.budget(u64::from(attribute_count))?;
        if attribute_count != 0 {
            return Err("View2D attached attributes are not qualified.".into());
        }
        let nickname = c.text()?;
        let hide_flag_raw = c.u16()?;
        let block_reference = Insert {
            block_id,
            block_status,
            insertion,
            rotation,
            scales,
            matrix_stored,
            attached_attributes: Vec::new(),
            nickname,
            hide_flag_raw,
            clip_flag_raw: None,
            span: c.span(start),
        };
        let name = c.text()?;
        let view_type_raw = c.u32()?;
        let clip = match c.take(1)?[0] {
            0 => None,
            1 => Some(c.clip()?),
            _ => return Err("Unqualified View2D clip presence flag.".into()),
        };
        if c.pos != c.data.len() {
            return Err("Unsupported trailing View2D body bytes.".into());
        }
        Ok(View2DEntity {
            attributes,
            geometry: View2DGeometry {
                block_reference,
                name,
                view_type_raw,
                clip,
                span: c.span(start),
            },
            budget_used: c.used,
        })
    })())
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    options: ParseOptions,
    used: u64,
}
impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], ComplexError> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or("View2D field range overflow.")?;
        let value = self
            .data
            .get(self.pos..end)
            .ok_or("Truncated View2D field.")?;
        self.pos = end;
        Ok(value)
    }
    fn span(&self, start: usize) -> SourceSpan {
        SourceSpan {
            offset: start as u64,
            length: (self.pos - start) as u64,
        }
    }
    fn budget(&mut self, count: u64) -> Result<(), ComplexError> {
        let total = self.used.saturating_add(count);
        ParseOptions::check_limit("max_records", total, self.options.max_records)
            .map_err(ComplexError::Limit)?;
        self.used = total;
        Ok(())
    }
    fn u32(&mut self) -> Result<u32, ComplexError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, ComplexError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16, ComplexError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, ComplexError> {
        let value = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !value.is_finite() {
            return Err("Non-finite View2D value.".into());
        }
        Ok(value)
    }
    fn values<const N: usize>(&mut self) -> Result<[f64; N], ComplexError> {
        let mut result = [0.; N];
        for value in &mut result {
            *value = self.f64()?;
        }
        Ok(result)
    }
    fn text(&mut self) -> Result<TextField, ComplexError> {
        let start = self.pos;
        let length = self.u32()? as usize;
        ParseOptions::check_limit(
            "max_string_bytes",
            length as u64,
            self.options.max_string_bytes,
        )
        .map_err(ComplexError::Limit)?;
        if length == 0 {
            return Ok(TextField {
                value: String::new(),
                span: self.span(start),
            });
        }
        if length < 2 || length % 2 != 0 {
            return Err("Invalid View2D UTF-16 byte count.".into());
        }
        let bytes = self.take(length)?;
        if bytes[length - 2..] != [0, 0] {
            return Err("Unterminated View2D UTF-16 text.".into());
        }
        let units: Vec<_> = bytes[..length - 2]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let value = String::from_utf16(&units).map_err(|_| "Invalid View2D UTF-16 text.")?;
        Ok(TextField {
            value,
            span: self.span(start),
        })
    }
    fn clip(&mut self) -> Result<ViewClip, ComplexError> {
        let start = self.pos;
        self.budget(1)?;
        let reference_bytes = self.take(32)?.try_into().unwrap();
        let reference_span = self.span(start);
        let type_raw = self.u32()?;
        if type_raw != 2 {
            return Err("View2D clip type is not qualified.".into());
        }
        let flag_raw = self.u16()?;
        let count = self.u32()?;
        self.budget(u64::from(count))?;
        if count != 1 {
            return Err("View2D circular clip requires one saved curve.".into());
        }
        let native_type = self.u32()?;
        if native_type != 1012 {
            return Err("View2D clipping curve type is not qualified.".into());
        }
        let curve_start = self.pos;
        let center = self.values()?;
        let radius = self.f64()?;
        let x_axis = self.values()?;
        let y_axis = self.values()?;
        let sweep_angle = self.f64()?;
        if radius <= 0. {
            return Err("Nonpositive View2D clip radius.".into());
        }
        if center[2] != 0. || x_axis[2] != 0. || y_axis[2] != 0. {
            return Err("Nonplanar View2D circular clip is not qualified.".into());
        }
        let curve = BoundaryCurve {
            native_type,
            geometry: BoundaryGeometry::Circular {
                center,
                radius,
                x_axis,
                y_axis,
                sweep_angle,
            },
            span: self.span(curve_start),
        };
        Ok(ViewClip {
            reference_bytes,
            reference_span,
            type_raw,
            flag_raw,
            curves: vec![curve],
            span: self.span(start),
        })
    }
}

#[cfg(test)]
use crate::remaining_test_fixture::view as fixtures;
#[cfg(test)]
mod tests {
    use super::*;
    fn header(len: usize) -> RecordHeader {
        RecordHeader {
            type_id: VIEW2D.into(),
            class_name: None,
            id_raw: 7,
            owner_raw: -5,
            schema: 1,
            object_schema: 1,
            properties_present: false,
            property_groups: vec![],
            span: SourceSpan {
                offset: 0,
                length: len as u64,
            },
            payload_span: SourceSpan {
                offset: 0,
                length: len as u64,
            },
            extension: None,
        }
    }
    fn decode(body: &[u8], options: ParseOptions) -> Result<View2DEntity, ComplexError> {
        decode_view2d(
            body,
            &header(body.len()),
            RecordProfile::Schema3Attributes33,
            10,
            options,
        )
        .unwrap()
    }
    #[test]
    fn view_transform_and_clip_are_retained_separately() {
        let body = fixtures::view2d_body(42, true);
        let entity = decode(&body, ParseOptions::default()).unwrap();
        assert_eq!(entity.budget_used, 2);
        let geometry = entity.geometry;
        let insert = &geometry.block_reference;
        assert_eq!(insert.block_id, 42);
        assert_eq!(insert.rotation, 0.5);
        assert_eq!(insert.insertion, [20., 30., 0.]);
        assert_eq!(insert.scales, [2., 3., 1.]);
        assert_eq!(insert.nickname.value, "detail-ref");
        assert_eq!(insert.hide_flag_raw, 1);
        assert!(insert.attached_attributes.is_empty());
        assert_eq!(geometry.name.value, "Detail view A");
        assert_eq!(geometry.view_type_raw, 19);
        let clip = geometry.clip.as_ref().unwrap();
        assert_eq!(clip.reference_bytes, core::array::from_fn(|i| i as u8));
        assert_eq!((clip.type_raw, clip.flag_raw), (2, 0xffff));
        assert_eq!(clip.reference_span.length, 32);
        assert_eq!(clip.span.length, 134);
        let BoundaryGeometry::Circular { center, radius, .. } = &clip.curves[0].geometry else {
            panic!()
        };
        assert_eq!(*center, [4., 5., 0.]);
        assert_eq!(*radius, 6.);
        assert_eq!(
            geometry.span.offset + geometry.span.length,
            body.len() as u64
        );
        assert_eq!(serde_json::to_value(&geometry).unwrap()["kind"], "View2D");
        let no_clip = decode(&fixtures::view2d_body(42, false), ParseOptions::default()).unwrap();
        assert!(no_clip.geometry.clip.is_none());
        assert_eq!(no_clip.budget_used, 0);
    }
    #[test]
    fn every_truncation_and_unqualified_variants_are_rejected() {
        let body = fixtures::view2d_body(42, true);
        for len in 0..body.len() {
            assert!(
                decode(&body[..len], ParseOptions::default()).is_err(),
                "length {len}"
            );
        }
        let clip = body.len() - 134;
        for (offset, replacement) in [
            (body.len(), vec![0]),
            (clip - 1, vec![2]),
            (clip + 32, 3u32.to_le_bytes().to_vec()),
            (clip + 38, 0u32.to_le_bytes().to_vec()),
            (clip + 42, 1013u32.to_le_bytes().to_vec()),
            (225, 1u32.to_le_bytes().to_vec()),
            (33, f64::NAN.to_le_bytes().to_vec()),
            (clip + 70, 0f64.to_le_bytes().to_vec()),
            (clip + 62, 1f64.to_le_bytes().to_vec()),
        ] {
            let mut variant = body.clone();
            variant.splice(
                offset..(offset + replacement.len()).min(body.len()),
                replacement,
            );
            assert!(
                matches!(
                    decode(&variant, ParseOptions::default()),
                    Err(ComplexError::Unsupported(_))
                ),
                "offset {offset}"
            );
        }
    }
    #[test]
    fn counts_and_text_obey_resource_limits_before_layout_gates() {
        let body = fixtures::view2d_body(42, true);
        assert!(decode(
            &body,
            ParseOptions {
                max_records: 2,
                ..ParseOptions::default()
            }
        )
        .is_ok());
        assert!(matches!(
            decode(
                &body,
                ParseOptions {
                    max_records: 1,
                    ..ParseOptions::default()
                }
            ),
            Err(ComplexError::Limit(crate::Error::ResourceLimit {
                name: "max_records",
                ..
            }))
        ));
        assert!(matches!(
            decode(
                &body,
                ParseOptions {
                    max_string_bytes: 4,
                    ..ParseOptions::default()
                }
            ),
            Err(ComplexError::Limit(crate::Error::ResourceLimit {
                name: "max_string_bytes",
                ..
            }))
        ));
        for offset in [225, body.len() - 134 + 38] {
            let mut variant = body.clone();
            variant[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(matches!(
                decode(&variant, ParseOptions::default()),
                Err(ComplexError::Limit(crate::Error::ResourceLimit {
                    name: "max_records",
                    ..
                }))
            ));
        }
    }
    #[test]
    fn profile_schema_text_and_source_ranges_are_checked() {
        let body = fixtures::view2d_body(42, true);
        for major in [0, 11, 15, 18] {
            assert!(matches!(
                decode_view2d(
                    &body,
                    &header(body.len()),
                    RecordProfile::Schema3Attributes33,
                    major,
                    ParseOptions::default()
                )
                .unwrap(),
                Err(ComplexError::Unsupported(_))
            ));
        }
        assert!(matches!(
            decode_view2d(
                &body,
                &header(body.len()),
                RecordProfile::Schema3Attributes37,
                10,
                ParseOptions::default()
            )
            .unwrap(),
            Err(ComplexError::Unsupported(_))
        ));
        let mut h = header(body.len());
        h.object_schema = 3;
        assert!(decode_view2d(
            &body,
            &h,
            RecordProfile::Schema3Attributes33,
            10,
            ParseOptions::default()
        )
        .unwrap()
        .is_err());
        h = header(body.len());
        h.payload_span.offset = u64::MAX;
        assert!(decode_view2d(
            &body,
            &h,
            RecordProfile::Schema3Attributes33,
            10,
            ParseOptions::default()
        )
        .unwrap()
        .is_err());
        let mut surrounded = vec![0xff; 17];
        surrounded.extend(&body);
        surrounded.extend([0xff; 23]);
        h = header(body.len());
        h.payload_span.offset = 17;
        let shifted = decode_view2d(
            &surrounded,
            &h,
            RecordProfile::Schema3Attributes33,
            10,
            ParseOptions::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(shifted.geometry.span.offset, 17 + 33);
        assert_eq!(
            shifted.geometry.span.offset + shifted.geometry.span.length,
            (17 + body.len()) as u64
        );
        for replacement in [
            1u32.to_le_bytes().to_vec(),
            0xffffd800u32.to_le_bytes().to_vec(),
        ] {
            let mut variant = body.clone();
            let offset = if replacement[0] == 1 { 229 } else { 233 };
            variant[offset..offset + 4].copy_from_slice(&replacement);
            assert!(decode(&variant, ParseOptions::default()).is_err());
        }
    }
}

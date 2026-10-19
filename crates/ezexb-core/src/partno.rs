//! Stored part-number annotations and their nested item presentations.
//! Number regeneration, table lookup and annotation-style evaluation are separate.
use crate::complex::{decode_presentation, ComplexError, PresentationGeometry, PresentationRecord};
use crate::geometry::{decode_attributes, EntityAttributes};
use crate::records::{embedded_record, FrameError};
use crate::{ParseOptions, RecordHeader, RecordProfile, SourceSpan};
use serde::Serialize;

pub(crate) const PART_NUMBER: &str = "db778346-3d2b-4eb3-852d-80637e5ca149";
const PART_ITEM: &str = "6fee9390-e7b8-4d84-bac4-1974b044e1be";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartNumberObjectReference {
    pub status_raw: u32,
    pub id_raw: i32,
    pub span: SourceSpan,
}

/// Stored reference-point data. Optional point and parameter roles are not
/// inferred from their presence or from the presentation geometry.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartNumberReferencePoint {
    pub flags_raw: u32,
    pub type_raw: u32,
    pub parameter_bytes: [u8; 16],
    pub id_arrays: [Vec<i32>; 2],
    pub points_raw: [Option<[f64; 3]>; 3],
    pub parameters_raw: [Option<f64>; 2],
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartItemGeometry {
    pub index_raw: i32,
    pub presentation_records: Vec<PresentationRecord>,
    pub presentation_span: SourceSpan,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartNumberItem {
    pub header: RecordHeader,
    pub attributes: EntityAttributes,
    pub geometry: PartItemGeometry,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "PartNumber")]
pub struct PartNumberGeometry {
    pub style_reference: PartNumberObjectReference,
    pub table_reference: PartNumberObjectReference,
    pub horizontal_or_vertical_raw: u32,
    pub array_mode_raw: u16,
    pub orientation_raw: u32,
    pub auto_or_reverse_raw: u32,
    pub start_reference: PartNumberReferencePoint,
    pub leader_points: Vec<[f64; 3]>,
    pub scale: f64,
    /// Stored items are separate from their copies in presentation_records.
    pub items: Vec<PartNumberItem>,
    /// Only the empty override-map layout is qualified.
    pub override_count_raw: u32,
    pub presentation_records: Vec<PresentationRecord>,
    pub presentation_span: SourceSpan,
    pub span: SourceSpan,
}

pub(crate) struct PartNumberEntity {
    pub attributes: EntityAttributes,
    pub geometry: PartNumberGeometry,
    pub budget_used: u64,
}

pub(crate) fn decode_part_number(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Option<Result<PartNumberEntity, ComplexError>> {
    if header.type_id != PART_NUMBER {
        return None;
    }
    Some((|| {
        let (attributes, mut c) = cursor(data, header, profile, major, options)?;
        let start = c.pos;
        let (mut presentation_records, presentation_span) = c.presentation(profile, major, 1)?;
        for record in &mut presentation_records {
            if record.header.type_id == PART_ITEM {
                let item =
                    decode_part_item(c.data, &record.header, profile, major, c.remaining(), 1)?;
                c.budget(item.budget_used)?;
                record.attributes = Some(item.attributes);
                record.geometry = Some(PresentationGeometry::PartItem(Box::new(item.geometry)));
                record.unsupported_reason = None;
            }
        }
        let table_reference = c.reference()?;
        let style_reference = c.reference()?;
        let horizontal_or_vertical_raw = c.u32()?;
        let auto_or_reverse_raw = c.u32()?;
        let orientation_raw = c.u32()?;
        let array_mode_raw = c.u16()?;
        let start_reference = c.reference_point()?;
        let count = c.count(24)?;
        let mut leader_points = Vec::new();
        for _ in 0..count {
            leader_points.push(c.point()?);
        }
        let scale = c.f64()?;
        let count = c.u32()?;
        c.check_count(count, 30)?;
        let mut items = Vec::new();
        for _ in 0..count {
            let h = embedded_record(
                c.data,
                c.pos,
                c.data.len(),
                c.options,
                &mut c.used,
                profile,
                1,
            )
            .map_err(frame_error)?;
            if h.type_id != PART_ITEM {
                return Err("Unsupported part-number item class.".into());
            }
            c.pos = span_end(&h.span)?;
            let item = decode_part_item(c.data, &h, profile, major, c.remaining(), 1)?;
            c.budget(item.budget_used)?;
            items.push(PartNumberItem {
                header: h,
                attributes: item.attributes,
                geometry: item.geometry,
            });
        }
        let override_count_raw = c.u32()?;
        c.check_count(override_count_raw, 4)?;
        if override_count_raw != 0 {
            return Err("Part-number override values are not qualified.".into());
        }
        if c.pos != c.data.len() {
            return Err("Unsupported trailing part-number body bytes.".into());
        }
        Ok(PartNumberEntity {
            attributes,
            geometry: PartNumberGeometry {
                style_reference,
                table_reference,
                horizontal_or_vertical_raw,
                array_mode_raw,
                orientation_raw,
                auto_or_reverse_raw,
                start_reference,
                leader_points,
                scale,
                items,
                override_count_raw,
                presentation_records,
                presentation_span,
                span: c.span(start),
            },
            budget_used: c.used,
        })
    })())
}

struct ItemEntity {
    attributes: EntityAttributes,
    geometry: PartItemGeometry,
    budget_used: u64,
}
fn decode_part_item(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
    depth: u64,
) -> Result<ItemEntity, ComplexError> {
    ParseOptions::check_limit("max_record_depth", depth, options.max_record_depth.min(64))
        .map_err(ComplexError::Limit)?;
    let (attributes, mut c) = cursor(data, header, profile, major, options)?;
    let start = c.pos;
    let (presentation_records, presentation_span) = c.presentation(profile, major, depth + 1)?;
    // A nested PartItem is retained as an unsupported presentation, rather than
    // recursively interpreting an unqualified graph of semantic item records.
    let index_raw = c.i32()?;
    if c.pos != c.data.len() {
        return Err("Unsupported trailing part-item body bytes.".into());
    }
    Ok(ItemEntity {
        attributes,
        geometry: PartItemGeometry {
            index_raw,
            presentation_records,
            presentation_span,
            span: c.span(start),
        },
        budget_used: c.used,
    })
}

fn cursor<'a>(
    data: &'a [u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Result<(EntityAttributes, Cursor<'a>), ComplexError> {
    if major != 10 || profile != RecordProfile::Schema3Attributes33 {
        return Err("Unqualified part-number save-version profile.".into());
    }
    if !matches!(header.object_schema, 1 | 3 | 100)
        || (header.object_schema == 3) != header.extension.is_some()
    {
        return Err("Unqualified part-number record schema.".into());
    }
    let start =
        usize::try_from(header.payload_span.offset).map_err(|_| "Part-number offset overflow.")?;
    let end = span_end(&header.payload_span)?;
    if end > data.len() || end < start || end - start < 33 {
        return Err("Truncated part-number body.".into());
    }
    let attributes = decode_attributes(data, &header.payload_span, profile)?;
    Ok((
        attributes,
        Cursor {
            data: &data[..end],
            pos: start + 33,
            options,
            used: 0,
        },
    ))
}
fn span_end(span: &SourceSpan) -> Result<usize, ComplexError> {
    usize::try_from(
        span.offset
            .checked_add(span.length)
            .ok_or("Part-number range overflow.")?,
    )
    .map_err(|_| "Part-number range overflow.".into())
}
fn frame_error(error: FrameError) -> ComplexError {
    match error {
        FrameError::Invalid(error) => ComplexError::Limit(error),
        FrameError::Unsupported(reason) => ComplexError::Unsupported(reason),
    }
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
            .ok_or("Part-number range overflow.")?;
        let value = self
            .data
            .get(self.pos..end)
            .ok_or("Truncated part-number body.")?;
        self.pos = end;
        Ok(value)
    }
    fn span(&self, start: usize) -> SourceSpan {
        SourceSpan {
            offset: start as u64,
            length: (self.pos - start) as u64,
        }
    }
    fn remaining(&self) -> ParseOptions {
        ParseOptions {
            max_records: self.options.max_records.saturating_sub(self.used),
            ..self.options
        }
    }
    fn budget(&mut self, count: u64) -> Result<(), ComplexError> {
        let total = self.used.saturating_add(count);
        ParseOptions::check_limit("max_records", total, self.options.max_records)
            .map_err(ComplexError::Limit)?;
        self.used = total;
        Ok(())
    }
    fn check_count(&self, count: u32, width: usize) -> Result<(), ComplexError> {
        ParseOptions::check_limit(
            "max_records",
            self.used.saturating_add(u64::from(count)),
            self.options.max_records,
        )
        .map_err(ComplexError::Limit)?;
        if count as usize > (self.data.len() - self.pos) / width {
            return Err("Part-number count exceeds bounded body.".into());
        }
        Ok(())
    }
    fn count(&mut self, width: usize) -> Result<u32, ComplexError> {
        let count = self.u32()?;
        self.check_count(count, width)?;
        self.budget(u64::from(count))?;
        Ok(count)
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
            return Err("Non-finite part-number value.".into());
        }
        Ok(value)
    }
    fn point(&mut self) -> Result<[f64; 3], ComplexError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn presence(&mut self) -> Result<bool, ComplexError> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("Unqualified part-number presence flag.".into()),
        }
    }
    fn reference(&mut self) -> Result<PartNumberObjectReference, ComplexError> {
        let start = self.pos;
        self.budget(1)?;
        Ok(PartNumberObjectReference {
            status_raw: self.u32()?,
            id_raw: self.i32()?,
            span: self.span(start),
        })
    }
    fn reference_point(&mut self) -> Result<PartNumberReferencePoint, ComplexError> {
        let start = self.pos;
        self.budget(1)?;
        let flags_raw = self.u32()?;
        let type_raw = self.u32()?;
        let parameter_bytes = self.take(16)?.try_into().unwrap();
        let mut id_arrays = [Vec::new(), Vec::new()];
        for array in &mut id_arrays {
            for _ in 0..self.count(4)? {
                array.push(self.i32()?);
            }
        }
        let p0 = if self.presence()? {
            Some(self.point()?)
        } else {
            None
        };
        let p1 = if self.presence()? {
            Some(self.point()?)
        } else {
            None
        };
        let v0 = if self.presence()? {
            Some(self.f64()?)
        } else {
            None
        };
        let v1 = if self.presence()? {
            Some(self.f64()?)
        } else {
            None
        };
        let p2 = if self.presence()? {
            Some(self.point()?)
        } else {
            None
        };
        Ok(PartNumberReferencePoint {
            flags_raw,
            type_raw,
            parameter_bytes,
            id_arrays,
            points_raw: [p0, p1, p2],
            parameters_raw: [v0, v1],
            span: self.span(start),
        })
    }
    fn presentation(
        &mut self,
        profile: RecordProfile,
        major: u32,
        depth: u64,
    ) -> Result<(Vec<PresentationRecord>, SourceSpan), ComplexError> {
        let (records, span, used) =
            decode_presentation(self.data, self.pos, profile, major, self.remaining(), depth)?;
        self.pos = span_end(&span)?;
        self.budget(used)?;
        Ok((records, span))
    }
}

#[cfg(test)]
#[path = "../examples/support/partno_fixture.rs"]
mod fixtures;
#[cfg(test)]
mod tests {
    use super::*;
    fn header(len: usize) -> RecordHeader {
        RecordHeader {
            type_id: PART_NUMBER.into(),
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
    fn decode(body: &[u8], options: ParseOptions) -> Result<PartNumberEntity, ComplexError> {
        decode_part_number(
            body,
            &header(body.len()),
            RecordProfile::Schema3Attributes33,
            10,
            options,
        )
        .unwrap()
    }
    #[test]
    fn stored_fields_and_item_presentations_stay_separate() {
        let body = fixtures::part_number_body();
        let result = decode(&body, ParseOptions::default()).unwrap();
        let g = result.geometry;
        assert_eq!(g.style_reference.id_raw, -61);
        assert_eq!(g.table_reference.id_raw, -60);
        assert_eq!(
            (
                g.horizontal_or_vertical_raw,
                g.array_mode_raw,
                g.orientation_raw
            ),
            (1, 1, 3)
        );
        assert_eq!(g.auto_or_reverse_raw, 2);
        assert_eq!(g.start_reference.id_arrays, [vec![17, 18], vec![-3]]);
        assert_eq!(
            g.start_reference.points_raw,
            [Some([11., 12., 0.]), Some([0.; 3]), Some([4., 5., 0.])]
        );
        assert_eq!(g.start_reference.parameters_raw, [Some(0.25), Some(0.75)]);
        assert_eq!(g.leader_points, [[1., 2., 0.], [3., 4., 0.]]);
        assert_eq!(g.scale, 2.5);
        assert_eq!(g.items.len(), 1);
        assert_eq!(g.items[0].geometry.index_raw, 102);
        let Some(PresentationGeometry::PartItem(item)) = &g.presentation_records[1].geometry else {
            panic!()
        };
        assert_eq!(item.index_raw, 101);
        assert_eq!(item.presentation_records.len(), 1);
        assert!(item.presentation_records[0].geometry.is_some());
        assert_eq!(g.span.offset + g.span.length, body.len() as u64);
        assert_eq!(serde_json::to_value(&g).unwrap()["kind"], "PartNumber");
    }
    #[test]
    fn every_truncation_and_unknown_suffix_are_rejected() {
        let mut body = fixtures::part_number_body();
        for len in 0..body.len() {
            assert!(
                decode(&body[..len], ParseOptions::default()).is_err(),
                "length {len}"
            );
        }
        body.push(0);
        assert!(decode(&body, ParseOptions::default()).is_err());
    }
    #[test]
    fn nested_records_and_arrays_share_the_resource_budget() {
        let body = fixtures::part_number_body();
        let used = decode(&body, ParseOptions::default()).unwrap().budget_used;
        assert!(used > 10);
        assert!(decode(
            &body,
            ParseOptions {
                max_records: used,
                ..ParseOptions::default()
            }
        )
        .is_ok());
        assert!(matches!(
            decode(
                &body,
                ParseOptions {
                    max_records: used - 1,
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
                    max_record_depth: 1,
                    ..ParseOptions::default()
                }
            ),
            Err(ComplexError::Limit(crate::Error::ResourceLimit {
                name: "max_record_depth",
                ..
            }))
        ));
    }
    #[test]
    fn unqualified_override_maps_and_versions_remain_unsupported() {
        let mut body = fixtures::part_number_body();
        let last = body.len() - 4;
        body[last..].copy_from_slice(&1u32.to_le_bytes());
        body.extend([0; 4]);
        assert!(matches!(
            decode(&body, ParseOptions::default()),
            Err(ComplexError::Unsupported(_))
        ));
        body[last..last + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode(&body, ParseOptions::default()),
            Err(ComplexError::Limit(crate::Error::ResourceLimit {
                name: "max_records",
                ..
            }))
        ));
        assert!(matches!(
            decode_part_number(
                &body,
                &header(body.len()),
                RecordProfile::Schema3Attributes33,
                15,
                ParseOptions::default()
            )
            .unwrap(),
            Err(ComplexError::Unsupported(_))
        ));
    }
}

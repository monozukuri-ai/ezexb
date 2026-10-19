//! Stored BOM-table placement and its typed header/row presentations.
//! Data-table lookup, cell assignment and regeneration are not evaluated.
use crate::complex::{decode_presentation, ComplexError, PresentationRecord};
use crate::geometry::{decode_attributes, EntityAttributes};
use crate::partno::PartNumberObjectReference;
use crate::records::{embedded_record, FrameError};
use crate::{ParseOptions, RecordHeader, RecordProfile, SourceSpan};
use serde::Serialize;

pub(crate) const BOM_TABLE: &str = "b995fced-a25a-4940-8d79-d92a345f37ce";
const BOM_HEADER: &str = "72f972f6-cd18-4d91-b00e-933d81947807";
const BOM_ROW: &str = "1a6eff84-514f-483d-8ff5-df5b204e2ba8";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BomItemKind {
    Header,
    Row,
}

/// A stored display object, not a reconstructed semantic data-table row.
/// Its TEXT records retain text and placement; cell/column assignments are
/// deliberately not inferred from coordinates or record order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BomDisplayItem {
    pub header: RecordHeader,
    pub attributes: EntityAttributes,
    pub item_kind: BomItemKind,
    pub presentation_records: Vec<PresentationRecord>,
    pub presentation_span: SourceSpan,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BomRewindPoint {
    pub index_raw: i32,
    pub point: [f64; 3],
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "BomTable")]
pub struct BomTableGeometry {
    pub table_reference: PartNumberObjectReference,
    pub break_direction_raw: u16,
    pub relation_type_raw: u32,
    pub scale: f64,
    pub location: [f64; 3],
    pub break_point: [f64; 3],
    pub rewind_points: Vec<BomRewindPoint>,
    pub items: Vec<BomDisplayItem>,
    pub presentation_span: SourceSpan,
    pub span: SourceSpan,
}

pub(crate) struct BomTableEntity {
    pub attributes: EntityAttributes,
    pub geometry: BomTableGeometry,
    pub budget_used: u64,
}

pub(crate) fn decode_bom_table(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Option<Result<BomTableEntity, ComplexError>> {
    if header.type_id != BOM_TABLE {
        return None;
    }
    Some((|| {
        let (attributes, mut c) = cursor(data, header, profile, major, options)?;
        let start = c.pos;
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
            let item_kind = match h.type_id.as_str() {
                BOM_HEADER => BomItemKind::Header,
                BOM_ROW => BomItemKind::Row,
                _ => return Err("Unsupported BOM-table display item class.".into()),
            };
            c.pos = span_end(&h.span)?;
            let (item_attributes, mut item) = cursor(c.data, &h, profile, major, c.remaining())?;
            let item_start = item.pos;
            let (presentation_records, presentation_span, used) =
                decode_presentation(item.data, item.pos, profile, major, item.options, 2)?;
            item.pos = span_end(&presentation_span)?;
            item.budget(used)?;
            item.eof()?;
            c.budget(item.used)?;
            items.push(BomDisplayItem {
                span: item.span(item_start),
                header: h,
                attributes: item_attributes,
                item_kind,
                presentation_records,
                presentation_span,
            });
        }
        let presentation_span = c.span(start);
        let reference_start = c.pos;
        c.budget(1)?;
        let table_reference = PartNumberObjectReference {
            status_raw: c.u32()?,
            id_raw: c.i32()?,
            span: c.span(reference_start),
        };
        let break_direction_raw = c.u16()?;
        let relation_type_raw = c.u32()?;
        let scale = c.f64()?;
        let location = c.point()?;
        let break_point = c.point()?;
        let count = c.u32()?;
        c.check_count(count, 28)?;
        c.budget(u64::from(count))?;
        let mut rewind_points = Vec::new();
        for _ in 0..count {
            let start = c.pos;
            rewind_points.push(BomRewindPoint {
                index_raw: c.i32()?,
                point: c.point()?,
                span: c.span(start),
            });
        }
        c.eof()?;
        Ok(BomTableEntity {
            attributes,
            geometry: BomTableGeometry {
                table_reference,
                break_direction_raw,
                relation_type_raw,
                scale,
                location,
                break_point,
                rewind_points,
                items,
                presentation_span,
                span: c.span(start),
            },
            budget_used: c.used,
        })
    })())
}

fn cursor<'a>(
    data: &'a [u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Result<(EntityAttributes, Cursor<'a>), ComplexError> {
    if major != 10 || profile != RecordProfile::Schema3Attributes33 {
        return Err("Unqualified BOM-table save-version profile.".into());
    }
    if !matches!(header.object_schema, 1 | 3 | 100)
        || (header.object_schema == 3) != header.extension.is_some()
    {
        return Err("Unqualified BOM-table record schema.".into());
    }
    let start =
        usize::try_from(header.payload_span.offset).map_err(|_| "BOM-table offset overflow.")?;
    let end = span_end(&header.payload_span)?;
    if end > data.len() || end < start || end - start < 33 {
        return Err("Truncated BOM-table body.".into());
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
            .ok_or("BOM-table range overflow.")?,
    )
    .map_err(|_| "BOM-table range overflow.".into())
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
            .ok_or("BOM-table range overflow.")?;
        let value = self
            .data
            .get(self.pos..end)
            .ok_or("Truncated BOM-table body.")?;
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
            return Err("BOM-table count exceeds bounded body.".into());
        }
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
            return Err("Non-finite BOM-table value.".into());
        }
        Ok(value)
    }
    fn point(&mut self) -> Result<[f64; 3], ComplexError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn eof(&self) -> Result<(), ComplexError> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err("Unsupported trailing BOM-table body bytes.".into())
        }
    }
}

#[cfg(test)]
use crate::remaining_test_fixture::bom as fixtures;
#[cfg(test)]
mod tests {
    use super::*;
    fn header(len: usize, offset: usize) -> RecordHeader {
        RecordHeader {
            type_id: BOM_TABLE.into(),
            class_name: None,
            id_raw: 77,
            owner_raw: -5,
            schema: 1,
            object_schema: 1,
            properties_present: false,
            property_groups: vec![],
            span: SourceSpan {
                offset: offset as u64,
                length: len as u64,
            },
            payload_span: SourceSpan {
                offset: offset as u64,
                length: len as u64,
            },
            extension: None,
        }
    }
    fn decode(body: &[u8], options: ParseOptions) -> Result<BomTableEntity, ComplexError> {
        decode_bom_table(
            body,
            &header(body.len(), 0),
            RecordProfile::Schema3Attributes33,
            10,
            options,
        )
        .unwrap()
    }
    #[test]
    fn typed_rows_preserve_their_presentation_and_placement() {
        let body = fixtures::bom_table_body();
        let result = decode(&body, ParseOptions::default()).unwrap();
        let g = result.geometry;
        assert_eq!(g.table_reference.id_raw, -61);
        assert_eq!((g.break_direction_raw, g.relation_type_raw), (2, 4));
        assert_eq!(g.scale, 2.5);
        assert_eq!(g.location, [12., 34., 0.]);
        assert_eq!(g.break_point, [20., 34., 0.]);
        assert_eq!(g.items.len(), 2);
        assert_eq!(g.items[0].item_kind, BomItemKind::Header);
        assert_eq!(g.items[1].item_kind, BomItemKind::Row);
        assert_eq!(g.items[1].presentation_records.len(), 2);
        assert!(g.items[1].presentation_records[0].geometry.is_some());
        let text = serde_json::to_value(&g.items[1].presentation_records[1]).unwrap();
        assert_eq!(text["geometry"]["geometry"]["contents"], "Part Ω");
        assert_eq!(g.rewind_points.len(), 2);
        assert_eq!(g.rewind_points[0].index_raw, -3);
        assert_eq!(g.rewind_points[1].point, [30., 40., 0.]);
        assert_eq!(g.span.offset + g.span.length, body.len() as u64);
        assert_eq!(serde_json::to_value(&g).unwrap()["kind"], "BomTable");
    }
    #[test]
    fn all_truncations_and_unknown_suffixes_are_rejected() {
        let mut body = fixtures::bom_table_body();
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
    fn nested_items_and_rewind_points_share_hard_limits() {
        let body = fixtures::bom_table_body();
        let used = decode(&body, ParseOptions::default()).unwrap().budget_used;
        assert!(used >= 7);
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
                    max_string_bytes: 2,
                    ..ParseOptions::default()
                }
            ),
            Err(ComplexError::Limit(crate::Error::ResourceLimit {
                name: "max_string_bytes",
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
        let mut excessive = body.clone();
        excessive[33..37].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode(&excessive, ParseOptions::default()),
            Err(ComplexError::Limit(_))
        ));
    }
    #[test]
    fn offsets_remain_absolute_and_profile_is_explicit() {
        let body = fixtures::bom_table_body();
        let mut data = vec![0xa5; 41];
        data.extend(&body);
        let g = decode_bom_table(
            &data,
            &header(body.len(), 41),
            RecordProfile::Schema3Attributes33,
            10,
            ParseOptions::default(),
        )
        .unwrap()
        .unwrap()
        .geometry;
        assert_eq!(g.presentation_span.offset, 74);
        assert_eq!(g.items[0].header.span.offset, 78);
        assert!(g.items[0].presentation_span.offset > 78);
        assert_eq!(g.span.offset + g.span.length, data.len() as u64);
        assert!(matches!(
            decode_bom_table(
                &body,
                &header(body.len(), 0),
                RecordProfile::Schema3Attributes33,
                15,
                ParseOptions::default()
            )
            .unwrap(),
            Err(ComplexError::Unsupported(_))
        ));
    }
    #[test]
    fn unknown_items_and_nonfinite_values_are_rejected() {
        let mut body = fixtures::bom_table_body();
        body[37..53].fill(0xff);
        assert!(decode(&body, ParseOptions::default()).is_err());
        let mut body = fixtures::bom_table_body();
        let scale = body.len() - (8 + 48 + 4 + 2 * 28);
        body[scale..scale + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(matches!(
            decode(&body, ParseOptions::default()),
            Err(ComplexError::Unsupported(_))
        ));
    }
}

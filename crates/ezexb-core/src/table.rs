//! Stored tables and hole-chart tables. Cells retain their formatted text and
//! numeric property tags; style evaluation and linked-data regeneration are not
//! inferred from the cached presentation.
use crate::complex::{decode_presentation, ComplexError, PresentationRecord};
use crate::geometry::{decode_attributes, EntityAttributes};
use crate::{ParseOptions, RecordHeader, RecordProfile, SourceSpan};
use serde::Serialize;

pub(crate) const TABLE: &str = "66dcce32-3156-46cf-88d5-067549db6cb3";
pub(crate) const HOLE_CHART: &str = "ba3395ef-bc2c-4c67-a7c9-3ed507561217";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TableType {
    Table,
    HoleChart,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableObjectReference {
    pub status_raw: u32,
    pub id_raw: i32,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableColumn {
    pub index: i32,
    pub width: f64,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum TableContentValue {
    Text {
        text: String,
    },
    Block {
        auto_scale_raw: u16,
        reference: TableObjectReference,
        scale: f64,
        rotation: f64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableContent {
    pub index: i32,
    pub type_raw: u32,
    pub value: TableContentValue,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum TablePropertyValue {
    U16(u16),
    U32(u32),
    F64(f64),
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableProperty {
    pub index: i32,
    pub type_raw: u32,
    pub value: TablePropertyValue,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableCell {
    pub index: i32,
    pub state_raw: u32,
    pub merged_raw: u16,
    pub merge_range_raw: [i32; 4],
    pub flags_raw: u32,
    pub style_name: String,
    pub contents: Vec<TableContent>,
    pub overrides: Vec<TableProperty>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableRow {
    pub index: i32,
    pub height: f64,
    pub cells: Vec<TableCell>,
    pub reference: TableObjectReference,
    pub state_raw: u32,
    pub index_prefix: String,
    pub index_suffix: String,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableIndexedValue {
    pub index: i32,
    pub value: f64,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableBreakData {
    pub flag_raw: u16,
    pub parameters_raw: [u32; 2],
    pub spacing_raw: f64,
    pub values: Vec<TableIndexedValue>,
    pub vector_raw: [f64; 3],
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HoleChartColumn {
    pub index: i32,
    pub property_raw: u32,
    pub data_type_raw: u32,
    pub layout_raw: u32,
    pub precision_raw: u32,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HoleChartData {
    /// Serialized matrix order; not applied to the already stored presentation.
    pub coordinate_matrix_raw: [f64; 16],
    pub coordinate_status_raw: u32,
    pub coordinate_name: String,
    pub parameters_raw: [u32; 4],
    pub flags_raw: [u16; 2],
    pub sizes_raw: [f64; 2],
    pub columns: Vec<HoleChartColumn>,
    pub references: [TableObjectReference; 5],
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "Table")]
pub struct TableGeometry {
    pub table_type: TableType,
    pub style_reference: TableObjectReference,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub location: [f64; 3],
    pub rotation: f64,
    pub scale: f64,
    pub overrides: Vec<TableProperty>,
    pub break_data: Option<TableBreakData>,
    pub hole_chart: Option<HoleChartData>,
    pub presentation_records: Vec<PresentationRecord>,
    pub presentation_span: SourceSpan,
    pub data_span: SourceSpan,
    /// Raw flag/property meanings and style/link evaluation remain unresolved,
    /// including fields interspersed with the decoded cells in this span.
    pub unresolved_semantics_span: SourceSpan,
    pub span: SourceSpan,
}
pub(crate) struct TableEntity {
    pub attributes: EntityAttributes,
    pub geometry: TableGeometry,
    pub budget_used: u64,
}

pub(crate) fn decode_table(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Option<Result<TableEntity, ComplexError>> {
    let table_type = match header.type_id.as_str() {
        TABLE => TableType::Table,
        HOLE_CHART => TableType::HoleChart,
        _ => return None,
    };
    Some((|| {
        if major != 18 || profile != RecordProfile::Schema3Attributes37 {
            return Err("Unqualified table save-version profile.".into());
        }
        if !matches!(header.object_schema, 1 | 3 | 100)
            || (header.object_schema == 3) != header.extension.is_some()
        {
            return Err("Unqualified table record schema.".into());
        }
        let start =
            usize::try_from(header.payload_span.offset).map_err(|_| "Table offset overflow.")?;
        let end = span_end(&header.payload_span)?;
        if end > data.len() || end < start || end - start < 37 {
            return Err("Truncated table body.".into());
        }
        let attributes = decode_attributes(data, &header.payload_span, profile)?;
        let mut c = Cursor {
            data: &data[..end],
            pos: start + 37,
            options,
            used: 0,
        };
        let geometry_start = c.pos;
        let (presentation_records, presentation_span, used) =
            decode_presentation(c.data, c.pos, profile, major, options, 1)?;
        c.pos = span_end(&presentation_span)?;
        c.budget(used)?;
        let data_start = c.pos;
        let style_reference = c.reference()?;
        let mut columns = Vec::new();
        while let Some((index, s)) = c.index()? {
            columns.push(TableColumn {
                index,
                width: c.f64()?,
                span: c.span(s),
            });
        }
        let mut rows = Vec::new();
        while let Some((index, s)) = c.index()? {
            let height = c.f64()?;
            let mut cells = Vec::new();
            while let Some((index, s)) = c.index()? {
                let state_raw = c.u32()?;
                let merged_raw = c.u16()?;
                let merge_range_raw = [c.i32()?, c.i32()?, c.i32()?, c.i32()?];
                let flags_raw = c.u32()?;
                let style_name = c.string()?;
                let mut contents = Vec::new();
                while let Some((index, s)) = c.index()? {
                    let type_raw = c.u32()?;
                    let value = if type_raw == 4 {
                        TableContentValue::Block {
                            auto_scale_raw: c.u16()?,
                            reference: c.reference()?,
                            scale: c.f64()?,
                            rotation: c.f64()?,
                        }
                    } else {
                        TableContentValue::Text { text: c.string()? }
                    };
                    contents.push(TableContent {
                        index,
                        type_raw,
                        value,
                        span: c.span(s),
                    });
                }
                let overrides = c.properties(true)?;
                cells.push(TableCell {
                    index,
                    state_raw,
                    merged_raw,
                    merge_range_raw,
                    flags_raw,
                    style_name,
                    contents,
                    overrides,
                    span: c.span(s),
                });
            }
            let reference = c.reference()?;
            let state_raw = c.u32()?;
            let index_prefix = c.string()?;
            let index_suffix = c.string()?;
            rows.push(TableRow {
                index,
                height,
                cells,
                reference,
                state_raw,
                index_prefix,
                index_suffix,
                span: c.span(s),
            });
        }
        let location = c.point()?;
        let rotation = c.f64()?;
        let break_data = if table_type == TableType::Table {
            let s = c.pos;
            let flag_raw = c.u16()?;
            let parameters_raw = [c.u32()?, c.u32()?];
            let spacing_raw = c.f64()?;
            let mut values = Vec::new();
            while let Some((index, s)) = c.index()? {
                values.push(TableIndexedValue {
                    index,
                    value: c.f64()?,
                    span: c.span(s),
                });
            }
            let vector_raw = c.point()?;
            Some(TableBreakData {
                flag_raw,
                parameters_raw,
                spacing_raw,
                values,
                vector_raw,
                span: c.span(s),
            })
        } else {
            None
        };
        let scale = c.f64()?;
        let overrides = c.properties(false)?;
        let hole_chart = if table_type == TableType::HoleChart {
            let s = c.pos;
            let mut coordinate_matrix_raw = [0.; 16];
            for v in &mut coordinate_matrix_raw {
                *v = c.f64()?;
            }
            let coordinate_status_raw = c.u32()?;
            let coordinate_name = c.string()?;
            let p0 = c.u32()?;
            let f0 = c.u16()?;
            let p1 = c.u32()?;
            let sizes_raw = [c.f64()?, c.f64()?];
            let p2 = c.u32()?;
            let p3 = c.u32()?;
            let f1 = c.u16()?;
            let mut cols = Vec::new();
            while let Some((index, s)) = c.index()? {
                cols.push(HoleChartColumn {
                    index,
                    property_raw: c.u32()?,
                    data_type_raw: c.u32()?,
                    layout_raw: c.u32()?,
                    precision_raw: c.u32()?,
                    span: c.span(s),
                });
            }
            let references = [
                c.reference()?,
                c.reference()?,
                c.reference()?,
                c.reference()?,
                c.reference()?,
            ];
            Some(HoleChartData {
                coordinate_matrix_raw,
                coordinate_status_raw,
                coordinate_name,
                parameters_raw: [p0, p1, p2, p3],
                flags_raw: [f0, f1],
                sizes_raw,
                columns: cols,
                references,
                span: c.span(s),
            })
        } else {
            None
        };
        if c.pos != end {
            return Err("Unqualified trailing table body bytes.".into());
        }
        let data_span = c.span(data_start);
        Ok(TableEntity {
            attributes,
            budget_used: c.used,
            geometry: TableGeometry {
                table_type,
                style_reference,
                columns,
                rows,
                location,
                rotation,
                scale,
                overrides,
                break_data,
                hole_chart,
                presentation_records,
                presentation_span,
                data_span: data_span.clone(),
                unresolved_semantics_span: data_span,
                span: c.span(geometry_start),
            },
        })
    })())
}
fn span_end(span: &SourceSpan) -> Result<usize, ComplexError> {
    usize::try_from(
        span.offset
            .checked_add(span.length)
            .ok_or("Table range overflow.")?,
    )
    .map_err(|_| "Table range overflow.".into())
}
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    options: ParseOptions,
    used: u64,
}
impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], ComplexError> {
        let end = self.pos.checked_add(n).ok_or("Table range overflow.")?;
        let b = self
            .data
            .get(self.pos..end)
            .ok_or("Truncated table body.")?;
        self.pos = end;
        Ok(b)
    }
    fn span(&self, s: usize) -> SourceSpan {
        SourceSpan {
            offset: s as u64,
            length: (self.pos - s) as u64,
        }
    }
    fn budget(&mut self, n: u64) -> Result<(), ComplexError> {
        let total = self.used.saturating_add(n);
        ParseOptions::check_limit("max_records", total, self.options.max_records)
            .map_err(ComplexError::Limit)?;
        self.used = total;
        Ok(())
    }
    // Native loaders use a signed index and -1 terminator, never a dense count.
    // Preserve indices without allocating by their value.
    fn index(&mut self) -> Result<Option<(i32, usize)>, ComplexError> {
        let s = self.pos;
        let i = self.i32()?;
        if i == -1 {
            return Ok(None);
        }
        if i < 0 {
            return Err("Unqualified table sequence terminator.".into());
        }
        self.budget(1)?;
        Ok(Some((i, s)))
    }
    fn u16(&mut self) -> Result<u16, ComplexError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, ComplexError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, ComplexError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, ComplexError> {
        let x = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !x.is_finite() {
            return Err("Non-finite table value.".into());
        }
        Ok(x)
    }
    fn point(&mut self) -> Result<[f64; 3], ComplexError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn reference(&mut self) -> Result<TableObjectReference, ComplexError> {
        let s = self.pos;
        self.budget(1)?;
        Ok(TableObjectReference {
            status_raw: self.u32()?,
            id_raw: self.i32()?,
            span: self.span(s),
        })
    }
    fn string(&mut self) -> Result<String, ComplexError> {
        let n = self.u32()?;
        ParseOptions::check_limit(
            "max_string_bytes",
            u64::from(n),
            self.options.max_string_bytes,
        )
        .map_err(ComplexError::Limit)?;
        if n == 0 {
            return Ok(String::new());
        }
        if n < 2 || n % 2 != 0 {
            return Err("Invalid table UTF-16 byte length.".into());
        }
        let b = self.take(n as usize)?;
        if b[b.len() - 2..] != [0, 0] {
            return Err("Missing table UTF-16 terminator.".into());
        }
        let units = b[..b.len() - 2]
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| "Invalid table UTF-16 data.".into())
    }
    fn properties(&mut self, cell: bool) -> Result<Vec<TableProperty>, ComplexError> {
        let mut out = Vec::new();
        while let Some((index, s)) = self.index()? {
            let type_raw = self.u32()?;
            let value = match (cell, type_raw) {
                (true, 121..=126 | 135 | 140..=143 | 148) | (false, 21..=23) => {
                    TablePropertyValue::F64(self.f64()?)
                }
                (true, 130 | 144..=147) | (false, 3) => TablePropertyValue::U32(self.u32()?),
                (true, 131) | (false, 1 | 2) => TablePropertyValue::U16(self.u16()?),
                _ => return Err("Unqualified table property value layout.".into()),
            };
            out.push(TableProperty {
                index,
                type_raw,
                value,
                span: self.span(s),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
use crate::remaining_test_fixture::table as fixtures;
#[cfg(test)]
mod tests {
    use super::*;
    fn header(len: usize, hole: bool) -> RecordHeader {
        RecordHeader {
            type_id: if hole { HOLE_CHART } else { TABLE }.into(),
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
    fn decode(body: &[u8], hole: bool, options: ParseOptions) -> Result<TableEntity, ComplexError> {
        decode_table(
            body,
            &header(body.len(), hole),
            RecordProfile::Schema3Attributes37,
            18,
            options,
        )
        .unwrap()
    }
    #[test]
    fn stored_cells_properties_and_tail_fields_are_typed() {
        for hole in [false, true] {
            let b = fixtures::table_body(hole);
            let entity = decode(&b, hole, ParseOptions::default()).unwrap();
            let g = entity.geometry;
            assert_eq!(
                g.columns.iter().map(|c| c.width).collect::<Vec<_>>(),
                [14., 23.]
            );
            assert_eq!(g.rows[0].index, 3);
            assert_eq!(g.rows[0].height, 7.5);
            assert_eq!(g.rows[0].index_prefix, "P-");
            assert_eq!(g.rows[0].index_suffix, "-S");
            let cell = &g.rows[0].cells[0];
            assert_eq!(cell.style_name, "Custom Ω");
            assert_eq!(
                cell.contents[0].value,
                TableContentValue::Text {
                    text: "Cell 日本語".into()
                }
            );
            assert_eq!(cell.overrides[0].value, TablePropertyValue::F64(0.5));
            assert_eq!(cell.overrides[2].value, TablePropertyValue::U16(65535));
            assert!(matches!(
                g.rows[0].cells[1].contents[1].value,
                TableContentValue::Block {
                    scale: 1.25,
                    rotation: 0.125,
                    ..
                }
            ));
            assert_eq!(g.location, [11., 12., 0.]);
            assert_eq!(g.rotation, 0.25);
            assert_eq!(g.scale, 2.);
            assert_eq!(g.presentation_records.len(), 1);
            assert!(g.presentation_records[0].geometry.is_some());
            assert_eq!(g.span.offset + g.span.length, b.len() as u64);
            assert_eq!(g.data_span, g.unresolved_semantics_span);
            if hole {
                let h = g.hole_chart.unwrap();
                assert_eq!(h.coordinate_name, "Origin Ω");
                assert_eq!(h.coordinate_matrix_raw[12..14], [20., 30.]);
                assert_eq!(h.references[4].id_raw, -105);
                assert_eq!(h.columns[0].precision_raw, 3);
                assert!(g.break_data.is_none());
            } else {
                let d = g.break_data.unwrap();
                assert_eq!(d.values[0].index, 2);
                assert_eq!(d.values[0].value, 6.5);
                assert_eq!(d.vector_raw, [1., 2., 3.]);
                assert!(g.hole_chart.is_none());
            }
        }
    }
    #[test]
    fn every_truncation_and_trailing_byte_are_rejected() {
        for hole in [false, true] {
            let mut b = fixtures::table_body(hole);
            for n in 0..b.len() {
                assert!(
                    decode(&b[..n], hole, ParseOptions::default()).is_err(),
                    "hole={hole},len={n}"
                );
            }
            b.push(0);
            assert!(decode(&b, hole, ParseOptions::default()).is_err());
        }
    }
    #[test]
    fn nested_presentations_and_cell_items_share_budget() {
        let b = fixtures::table_body(true);
        let used = decode(&b, true, ParseOptions::default())
            .unwrap()
            .budget_used;
        assert!(used > 20);
        assert!(decode(
            &b,
            true,
            ParseOptions {
                max_records: used,
                ..ParseOptions::default()
            }
        )
        .is_ok());
        assert!(matches!(
            decode(
                &b,
                true,
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
                &b,
                true,
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
        assert!(matches!(
            decode(
                &b,
                true,
                ParseOptions {
                    max_record_depth: 0,
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
    fn invalid_cell_data_and_unqualified_profiles_fail_closed() {
        let b = fixtures::table_body(false);
        let g = decode(&b, false, ParseOptions::default()).unwrap().geometry;
        let cell = &g.rows[0].cells[0];
        let o = cell.overrides[0].span.offset as usize;
        let mut bad = b.clone();
        bad[o + 4..o + 8].copy_from_slice(&132u32.to_le_bytes());
        assert!(decode(&bad, false, ParseOptions::default()).is_err());
        let mut bad = b.clone();
        let o = g.columns[0].span.offset as usize;
        bad[o + 4..o + 12].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&bad, false, ParseOptions::default()).is_err());
        let mut bad = b.clone();
        let o = cell.span.offset as usize + 4 + 4 + 2 + 16 + 4;
        bad[o..o + 4].copy_from_slice(&3u32.to_le_bytes());
        assert!(decode(&bad, false, ParseOptions::default()).is_err());
        for major in [10, 15, 19] {
            assert!(decode_table(
                &b,
                &header(b.len(), false),
                RecordProfile::Schema3Attributes37,
                major,
                ParseOptions::default()
            )
            .unwrap()
            .is_err());
        }
        assert!(decode_table(
            &b,
            &header(b.len(), false),
            RecordProfile::Schema3Attributes33,
            18,
            ParseOptions::default()
        )
        .unwrap()
        .is_err());
    }
    #[test]
    fn offsets_are_absolute_and_bounded_by_the_payload() {
        let b = fixtures::table_body(true);
        let mut all = vec![0x55; 53];
        all.extend(&b);
        all.extend([0x66; 20]);
        let mut h = header(b.len(), true);
        h.payload_span.offset = 53;
        let e = decode_table(
            &all,
            &h,
            RecordProfile::Schema3Attributes37,
            18,
            ParseOptions::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(e.geometry.span.offset, 90);
        assert_eq!(
            e.geometry.data_span.offset + e.geometry.data_span.length,
            53 + b.len() as u64
        );
    }
}

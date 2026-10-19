//! Stored fill boundaries and dimension data. Native presentation caches remain
//! separate from model entities; unresolved dimension semantics are explicit.
use crate::annotation::{decode_annotation, AnnotationError, AnnotationGeometry};
use crate::geometry::{
    attribute_width, decode_attributes, decode_primitive, EntityAttributes, PrimitiveGeometry,
};
use crate::records::{embedded_record, FrameError};
use crate::{Error, ParseOptions, RecordHeader, RecordProfile, SourceSpan, TextField};
use serde::Serialize;
const HATCH: &str = "79b9b625-ee7f-4fc9-b9a6-39036a219cf7";
const SOLID: &str = "d7676d4e-b9e0-46c3-894d-e9ef7fb66fb2";
const NURBS: &str = "838896be-c069-4566-8991-afd750b4547c";
const ELLIPSE: &str = "85dc7b5d-daac-47c8-a126-ffad50d6cd68";
const DIMS: [&str; 5] = [
    "1da848b5-8c84-4c11-80d6-45cf9b26aa48",
    "f391cb78-240a-4fbd-9e3f-4f19572391fc",
    "aba4bd1e-d922-4b4d-931e-7f4c9d6919e7",
    "cc8682e6-a8cf-4a4f-aa36-c600b07aa69c",
    "5f9490d4-b797-4119-90b1-93668e2db71a",
];
#[derive(Debug)]
pub(crate) enum ComplexError {
    Unsupported(String),
    Limit(Error),
}
impl From<String> for ComplexError {
    fn from(v: String) -> Self {
        Self::Unsupported(v)
    }
}
impl From<&str> for ComplexError {
    fn from(v: &str) -> Self {
        Self::Unsupported(v.into())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComplexEntity {
    pub attributes: EntityAttributes,
    pub geometry: ComplexGeometry,
    pub budget_used: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum ComplexGeometry {
    Hatch(HatchGeometry),
    Solid(SolidGeometry),
    Dimension(DimensionGeometry),
    Nurbs(NurbsGeometry),
    Ellipse(EllipseGeometry),
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NurbsGeometry {
    pub plane: [f64; 4],
    pub degree: u32,
    pub type_raw: u32,
    pub planar_type_raw: u32,
    pub control_points: Vec<[f64; 4]>,
    pub knots: Vec<f64>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EllipseGeometry {
    pub center: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub major_radius: f64,
    pub minor_radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub rotation: f64,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoundaryLoop {
    pub curves: Vec<BoundaryCurve>,
    pub loop_type_raw: Option<u32>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoundaryCurve {
    pub native_type: u32,
    pub geometry: BoundaryGeometry,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum BoundaryGeometry {
    Point {
        point: [f64; 3],
    },
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    Circular {
        center: [f64; 3],
        radius: f64,
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        sweep_angle: f64,
    },
    Ellipse {
        center: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
        start_angle: f64,
        end_angle: f64,
        rotation: f64,
    },
    /// Native XYZW controls and knot sequence; no tessellation or normalization.
    Nurbs {
        plane: [f64; 4],
        degree: u32,
        type_raw: u32,
        planar_type_raw: u32,
        control_points: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
    Polyline {
        vertices: Vec<BoundaryVertex>,
        closed: bool,
        normal: Option<[f64; 3]>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoundaryVertex {
    pub point: [f64; 2],
    pub legacy_z: Option<f64>,
    pub bulge: f64,
    pub start_width: f64,
    pub end_width: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FillBoundary {
    pub pick_point: [f64; 3],
    pub loops: Vec<BoundaryLoop>,
    pub settings: Option<FillSettings>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FillSettings {
    pub associated_raw: u32,
    pub relation_flags_raw: u32,
    pub relation_tail_raw: u32,
    pub evaluation: [f64; 3],
    pub normal: [f64; 3],
    pub flags_raw: [u32; 5],
    pub vector_raw: [f64; 3],
    pub flag_raw: u32,
    pub pixel_size: f64,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HatchGeometry {
    pub boundary: FillBoundary,
    pub pattern: HatchPattern,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HatchPattern {
    /// A native char[32], including bytes after its NUL; encoding is not presumed.
    pub name_bytes: Vec<u8>,
    pub name_ascii: Option<String>,
    pub scale: f64,
    pub angle_stored: f64,
    pub spacing: f64,
    pub lines: Vec<HatchPatternLine>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HatchPatternLine {
    pub parameters_stored: [f64; 7],
    pub dash_count: u32,
    /// Only active entries; native storage reserves 50 doubles.
    pub dashes: Vec<f64>,
    pub span: SourceSpan,
    pub reserved_dash_span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SolidGeometry {
    pub boundary: FillBoundary,
    pub gradient: Option<SolidGradient>,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SolidGradient {
    pub type_raw: u32,
    pub name: TextField,
    pub angle: f64,
    pub shift: f64,
    pub one_color_mode_raw: u32,
    pub shade_tint: f64,
    pub colors_raw: [u32; 2],
    pub values: [f64; 2],
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DimensionGeometry {
    pub dimension_type_id: String,
    pub value_stored: f64,
    /// Native base fields, in stream order: Text, NoteText, Prefix, Postfix,
    /// and a fifth base text field whose semantic role is unresolved.
    /// Formatting escapes remain literal.
    pub text_fields_stored: Vec<TextField>,
    pub flags_raw: [u32; 3],
    pub paper_scale: f64,
    pub location: [f64; 3],
    pub text_location: [f64; 3],
    pub note_location: [f64; 3],
    pub presentation_records: Vec<PresentationRecord>,
    pub presentation_span: SourceSpan,
    /// Associativity, overrides and type-specific construction remain unresolved.
    pub unresolved_semantics_span: SourceSpan,
    pub span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresentationRecord {
    pub header: RecordHeader,
    pub attributes: Option<EntityAttributes>,
    pub geometry: Option<PresentationGeometry>,
    pub unsupported_reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "category", content = "geometry")]
pub enum PresentationGeometry {
    Primitive(PrimitiveGeometry),
    Annotation(AnnotationGeometry),
    Solid(SolidGeometry),
}

pub(crate) fn decode_complex(
    data: &[u8],
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
    options: ParseOptions,
) -> Option<Result<ComplexEntity, ComplexError>> {
    if !matches!(header.type_id.as_str(), HATCH | SOLID | NURBS | ELLIPSE)
        && !DIMS.contains(&header.type_id.as_str())
    {
        return None;
    }
    Some((|| {
        if !matches!(
            (profile, major),
            (RecordProfile::Schema3Attributes33, 10 | 11 | 15 | 16)
                | (RecordProfile::Schema3Attributes37, 18)
        ) {
            return Err("Unsupported complex save-version profile.".into());
        }
        if !matches!(header.object_schema, 1 | 100) || header.extension.is_some() {
            return Err("Unsupported complex object schema.".into());
        }
        let start =
            usize::try_from(header.payload_span.offset).map_err(|_| "Complex offset overflow.")?;
        let len =
            usize::try_from(header.payload_span.length).map_err(|_| "Complex length overflow.")?;
        let end = start.checked_add(len).ok_or("Complex range overflow.")?;
        if end > data.len() || len < attribute_width(profile) {
            return Err("Truncated complex body.".into());
        }
        let attributes = decode_attributes(data, &header.payload_span, profile)?;
        let mut c = Cursor {
            data: &data[..end],
            pos: start + attribute_width(profile),
            options,
            used: 0,
        };
        let geometry = if header.type_id == NURBS {
            if major >= 15 {
                return Err(
                    "NURBS fit-point preamble for save major >=15 is not qualified.".into(),
                );
            }
            let s = c.pos;
            let BoundaryGeometry::Nurbs {
                plane,
                degree,
                type_raw,
                planar_type_raw,
                control_points,
                knots,
            } = curve(&mut c, 1016, major)?
            else {
                unreachable!()
            };
            ComplexGeometry::Nurbs(NurbsGeometry {
                plane,
                degree,
                type_raw,
                planar_type_raw,
                control_points,
                knots,
                span: c.span(s),
            })
        } else if header.type_id == ELLIPSE {
            let s = c.pos;
            let BoundaryGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius,
                minor_radius,
                start_angle,
                end_angle,
                rotation,
            } = curve(&mut c, 1014, major)?
            else {
                unreachable!()
            };
            ComplexGeometry::Ellipse(EllipseGeometry {
                center,
                x_axis,
                y_axis,
                major_radius,
                minor_radius,
                start_angle,
                end_angle,
                rotation,
                span: c.span(s),
            })
        } else if DIMS.contains(&header.type_id.as_str()) {
            ComplexGeometry::Dimension(dimension(&mut c, header, profile, major)?)
        } else {
            let geomstart = c.pos;
            let boundary = boundary(&mut c, major)?;
            if header.type_id == HATCH {
                let pattern = pattern(&mut c)?;
                ComplexGeometry::Hatch(HatchGeometry {
                    boundary,
                    pattern,
                    span: c.span(geomstart),
                })
            } else {
                let gradient = if major > 10 {
                    Some(gradient(&mut c)?)
                } else {
                    None
                };
                ComplexGeometry::Solid(SolidGeometry {
                    boundary,
                    gradient,
                    span: c.span(geomstart),
                })
            }
        };
        if c.pos != end {
            return Err("Unsupported trailing complex body bytes.".into());
        }
        Ok(ComplexEntity {
            attributes,
            geometry,
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
    fn span(&self, start: usize) -> SourceSpan {
        SourceSpan {
            offset: start as u64,
            length: (self.pos - start) as u64,
        }
    }
    fn take(&mut self, n: usize) -> Result<&[u8], ComplexError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or("Complex field range overflow.")?;
        let b = self
            .data
            .get(self.pos..end)
            .ok_or("Truncated complex field.")?;
        self.pos = end;
        Ok(b)
    }
    fn u(&mut self) -> Result<u32, ComplexError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f(&mut self) -> Result<f64, ComplexError> {
        let v = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !v.is_finite() {
            return Err("Non-finite complex geometry value.".into());
        }
        Ok(v)
    }
    fn v<const N: usize>(&mut self) -> Result<[f64; N], ComplexError> {
        let mut a = [0.; N];
        for v in &mut a {
            *v = self.f()?
        }
        Ok(a)
    }
    fn count(&mut self) -> Result<u32, ComplexError> {
        let n = self.u()?;
        self.budget(n as u64)?;
        Ok(n)
    }
    fn budget(&mut self, n: u64) -> Result<(), ComplexError> {
        self.used = self.used.saturating_add(n);
        ParseOptions::check_limit("max_records", self.used, self.options.max_records)
            .map_err(ComplexError::Limit)
    }
    fn text(&mut self) -> Result<TextField, ComplexError> {
        let start = self.pos;
        let n = self.u()? as usize;
        ParseOptions::check_limit("max_string_bytes", n as u64, self.options.max_string_bytes)
            .map_err(ComplexError::Limit)?;
        if n == 0 {
            return Ok(TextField {
                value: String::new(),
                span: self.span(start),
            });
        }
        if n < 2 || n % 2 != 0 {
            return Err("Invalid complex UTF-16 byte count.".into());
        }
        let b = self.take(n)?;
        if b[n - 2..] != [0, 0] {
            return Err("Unterminated complex UTF-16 text.".into());
        }
        let units: Vec<_> = b[..n - 2]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let value = String::from_utf16(&units).map_err(|_| "Invalid complex UTF-16 text.")?;
        Ok(TextField {
            value,
            span: self.span(start),
        })
    }
}
fn planar(v: [f64; 3]) -> Result<[f64; 3], ComplexError> {
    if v[2] != 0. {
        return Err("Nonzero legacy boundary Z is outside the qualified 2D profile.".into());
    }
    Ok(v)
}
fn boundary(c: &mut Cursor<'_>, major: u32) -> Result<FillBoundary, ComplexError> {
    let start = c.pos;
    let pick_point = c.v()?;
    let n = c.count()?;
    let mut loops = Vec::new();
    for _ in 0..n {
        let ls = c.pos;
        let nc = c.count()?;
        let mut types = Vec::new();
        for _ in 0..nc {
            types.push(c.u()?)
        }
        let mut curves = Vec::new();
        for t in types {
            let s = c.pos;
            let geometry = curve(c, t, major)?;
            curves.push(BoundaryCurve {
                native_type: t,
                geometry,
                span: c.span(s),
            })
        }
        loops.push(BoundaryLoop {
            curves,
            loop_type_raw: None,
            span: c.span(ls),
        })
    }
    let settings = if major > 10 {
        let s = c.pos;
        let associated_raw = c.u()?;
        let relation_flags_raw = c.u()?;
        // ICxDbLoopRelation.Load22: flags, two counted association maps, tail.
        if c.count()? != 0 {
            return Err("Nonempty fill loop association map is not qualified.".into());
        }
        if c.count()? != 0 {
            return Err("Nonempty fill loop reference map is not qualified.".into());
        }
        let relation_tail_raw = c.u()?;
        let evaluation = c.v()?;
        let normal = c.v()?;
        if normal != [0., 0., 1.] {
            return Err("Nondefault fill normal is outside the qualified profile.".into());
        }
        let flags_raw = [c.u()?, c.u()?, c.u()?, c.u()?, c.u()?];
        let vector_raw = c.v()?;
        let flag_raw = c.u()?;
        let pixel_size = c.f()?;
        for l in &mut loops {
            l.loop_type_raw = Some(c.u()?)
        }
        Some(FillSettings {
            associated_raw,
            relation_flags_raw,
            relation_tail_raw,
            evaluation,
            normal,
            flags_raw,
            vector_raw,
            flag_raw,
            pixel_size,
            span: c.span(s),
        })
    } else {
        None
    };
    Ok(FillBoundary {
        pick_point,
        loops,
        settings,
        span: c.span(start),
    })
}
fn curve(c: &mut Cursor<'_>, t: u32, major: u32) -> Result<BoundaryGeometry, ComplexError> {
    Ok(match t {
        1010 => BoundaryGeometry::Point {
            point: planar(c.v()?)?,
        },
        1011 => BoundaryGeometry::Line {
            start: planar(c.v()?)?,
            end: planar(c.v()?)?,
        },
        1012 | 1013 => {
            let center = planar(c.v()?)?;
            let radius = c.f()?;
            let x_axis = planar(c.v()?)?;
            let y_axis = planar(c.v()?)?;
            let sweep_angle = c.f()?;
            if radius <= 0. {
                return Err("Nonpositive circular boundary radius.".into());
            }
            BoundaryGeometry::Circular {
                center,
                radius,
                x_axis,
                y_axis,
                sweep_angle,
            }
        }
        1014 => {
            let center = planar(c.v()?)?;
            let x_axis = planar(c.v()?)?;
            let y_axis = planar(c.v()?)?;
            let major_radius = c.f()?;
            let minor_radius = c.f()?;
            let start_angle = c.f()?;
            let end_angle = c.f()?;
            let rotation = c.f()?;
            if major_radius <= 0. || minor_radius <= 0. {
                return Err("Nonpositive elliptical boundary radius.".into());
            }
            BoundaryGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius,
                minor_radius,
                start_angle,
                end_angle,
                rotation,
            }
        }
        1015 | 1016 => {
            let plane = c.v()?;
            let n = c.count()?;
            let degree = c.u()?;
            let type_raw = c.u()?;
            let planar_type_raw = c.u()?;
            let nk = u64::from(n) + u64::from(degree) + 1;
            c.budget(nk)?;
            let mut control_points = Vec::new();
            for _ in 0..n {
                control_points.push(c.v()?)
            }
            let mut knots = Vec::new();
            for _ in 0..nk {
                knots.push(c.f()?)
            }
            BoundaryGeometry::Nurbs {
                plane,
                degree,
                type_raw,
                planar_type_raw,
                control_points,
                knots,
            }
        }
        1019 => {
            let n = c.count()?;
            let mut vertices = Vec::new();
            for _ in 0..n {
                let point = c.v()?;
                let legacy_z = if major <= 10 { Some(c.f()?) } else { None };
                if legacy_z.is_some_and(|z| z != 0.) {
                    return Err("Nonzero polyline boundary legacy Z.".into());
                }
                let bulge = c.f()?;
                let start_width = c.f()?;
                let end_width = c.f()?;
                vertices.push(BoundaryVertex {
                    point,
                    legacy_z,
                    bulge,
                    start_width,
                    end_width,
                })
            }
            let closed = c.u()?;
            if closed > 1 {
                return Err("Unknown polyline boundary closed flag.".into());
            }
            let normal = if major > 10 { Some(c.v()?) } else { None };
            if normal.is_some_and(|n| n != [0., 0., 1.]) {
                return Err("Nondefault polyline boundary normal.".into());
            }
            BoundaryGeometry::Polyline {
                vertices,
                closed: closed != 0,
                normal,
            }
        }
        _ => return Err(format!("Unsupported native boundary curve type {t}.").into()),
    })
}
fn pattern(c: &mut Cursor<'_>) -> Result<HatchPattern, ComplexError> {
    let start = c.pos;
    let name_bytes = c.take(32)?.to_vec();
    let stop = name_bytes.iter().position(|v| *v == 0).unwrap_or(32);
    let name_ascii = name_bytes[..stop]
        .is_ascii()
        .then(|| String::from_utf8(name_bytes[..stop].to_vec()).unwrap());
    let scale = c.f()?;
    let angle_stored = c.f()?;
    let spacing = c.f()?;
    let n = c.count()?;
    let mut lines = Vec::new();
    for _ in 0..n {
        let s = c.pos;
        let parameters_stored = c.v()?;
        let dash_count = c.u()?;
        if dash_count > 50 {
            return Err("Hatch dash count exceeds native fixed capacity 50.".into());
        }
        c.budget(u64::from(dash_count))?;
        let mut dashes = Vec::new();
        for _ in 0..dash_count {
            dashes.push(c.f()?)
        }
        let rs = c.pos;
        c.take((50 - dash_count) as usize * 8)?;
        let reserved_dash_span = c.span(rs);
        lines.push(HatchPatternLine {
            parameters_stored,
            dash_count,
            dashes,
            reserved_dash_span,
            span: c.span(s),
        })
    }
    Ok(HatchPattern {
        name_bytes,
        name_ascii,
        scale,
        angle_stored,
        spacing,
        lines,
        span: c.span(start),
    })
}
fn gradient(c: &mut Cursor<'_>) -> Result<SolidGradient, ComplexError> {
    let start = c.pos;
    let type_raw = c.u()?;
    let name = c.text()?;
    let angle = c.f()?;
    let shift = c.f()?;
    let one_color_mode_raw = c.u()?;
    let shade_tint = c.f()?;
    let colors_raw = [c.u()?, c.u()?];
    let values = c.v()?;
    Ok(SolidGradient {
        type_raw,
        name,
        angle,
        shift,
        one_color_mode_raw,
        shade_tint,
        colors_raw,
        values,
        span: c.span(start),
    })
}
fn dimension(
    c: &mut Cursor<'_>,
    header: &RecordHeader,
    profile: RecordProfile,
    major: u32,
) -> Result<DimensionGeometry, ComplexError> {
    let start = c.pos;
    let count = c.u()?;
    ParseOptions::check_limit(
        "max_records",
        c.used.saturating_add(u64::from(count)),
        c.options.max_records,
    )
    .map_err(ComplexError::Limit)?;
    let mut presentation_records = Vec::new();
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
        .map_err(|e| match e {
            FrameError::Invalid(e) => ComplexError::Limit(e),
            FrameError::Unsupported(s) => ComplexError::Unsupported(s),
        })?;
        c.pos = usize::try_from(h.span.offset + h.span.length)
            .map_err(|_| "Presentation offset overflow.")?;
        let mut attributes = None;
        let mut geometry = None;
        let mut unsupported_reason = None;
        let remaining = ParseOptions {
            max_records: c.options.max_records.saturating_sub(c.used),
            ..c.options
        };
        if let Some(r) = decode_primitive(c.data, &h, profile) {
            match r {
                Ok(e) => {
                    attributes = Some(e.attributes);
                    geometry = Some(PresentationGeometry::Primitive(e.geometry))
                }
                Err(s) => unsupported_reason = Some(s),
            }
        } else if let Some(r) = decode_annotation(c.data, &h, profile, major, remaining) {
            match r {
                Ok(e) => {
                    c.budget(e.budget_used)?;
                    attributes = Some(e.attributes);
                    geometry = Some(PresentationGeometry::Annotation(e.geometry))
                }
                Err(AnnotationError::Limit(e)) => return Err(ComplexError::Limit(e)),
                Err(AnnotationError::Unsupported(s)) => unsupported_reason = Some(s),
            }
        } else if h.type_id == SOLID {
            match decode_complex(c.data, &h, profile, major, remaining).unwrap() {
                Ok(e) => {
                    c.budget(e.budget_used)?;
                    attributes = Some(e.attributes);
                    if let ComplexGeometry::Solid(s) = e.geometry {
                        geometry = Some(PresentationGeometry::Solid(s))
                    }
                }
                Err(ComplexError::Limit(e)) => return Err(ComplexError::Limit(e)),
                Err(ComplexError::Unsupported(s)) => unsupported_reason = Some(s),
            }
        } else {
            unsupported_reason = Some("Unsupported native presentation record class.".into())
        }
        presentation_records.push(PresentationRecord {
            header: h,
            attributes,
            geometry,
            unsupported_reason,
        });
    }
    let presentation_span = c.span(start);
    let value_stored = c.f()?;
    let mut text_fields_stored = vec![c.text()?, c.text()?];
    let flag1 = c.u()?;
    for _ in 0..3 {
        text_fields_stored.push(c.text()?)
    }
    let paper_scale = c.f()?;
    let flag2 = c.u()?;
    let flag3 = c.u()?;
    let location = c.v()?;
    let text_location = c.v()?;
    let note_location = c.v()?;
    let unresolved_semantics_span = SourceSpan {
        offset: c.pos as u64,
        length: (c.data.len() - c.pos) as u64,
    };
    c.pos = c.data.len();
    Ok(DimensionGeometry {
        dimension_type_id: header.type_id.clone(),
        value_stored,
        text_fields_stored,
        flags_raw: [flag1, flag2, flag3],
        paper_scale,
        location,
        text_location,
        note_location,
        presentation_records,
        presentation_span,
        unresolved_semantics_span,
        span: c.span(start),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn attrs() -> Vec<u8> {
        let mut b = vec![0; 33];
        b[29..33].copy_from_slice(&1f32.to_le_bytes());
        b
    }
    fn f(b: &mut Vec<u8>, v: f64) {
        b.extend_from_slice(&v.to_le_bytes())
    }
    fn u(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_le_bytes())
    }
    fn header(kind: &str, len: usize) -> RecordHeader {
        RecordHeader {
            type_id: kind.into(),
            class_name: None,
            id_raw: 1,
            owner_raw: 0,
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
    fn body() -> Vec<u8> {
        let mut b = attrs();
        for _ in 0..3 {
            f(&mut b, 0.)
        }
        u(&mut b, 1);
        u(&mut b, 1);
        u(&mut b, 1011);
        for v in [1., 2., 0., 3., 4., 0.] {
            f(&mut b, v)
        }
        b
    }
    fn decode(b: &[u8], kind: &str, o: ParseOptions) -> Result<ComplexEntity, ComplexError> {
        decode_complex(
            b,
            &header(kind, b.len()),
            RecordProfile::Schema3Attributes33,
            10,
            o,
        )
        .unwrap()
    }
    #[test]
    fn fill_boundary_and_pattern_bounds() {
        let mut b = body();
        let result = decode(&b, SOLID, ParseOptions::default()).unwrap();
        assert_eq!(result.budget_used, 2);
        let ComplexGeometry::Solid(s) = result.geometry else {
            panic!()
        };
        assert_eq!(s.boundary.loops[0].curves[0].span.offset, 69);
        assert_eq!(s.boundary.loops[0].curves[0].span.length, 48);
        b.extend_from_slice(&[0; 32]);
        for v in [2., 45., 0.] {
            f(&mut b, v)
        }
        u(&mut b, 0);
        assert!(decode(&b, HATCH, ParseOptions::default()).is_ok());
        for n in 0..b.len() {
            assert!(
                decode(&b[..n], HATCH, ParseOptions::default()).is_err(),
                "{n}"
            )
        }
        b.push(0);
        assert!(decode(&b, HATCH, ParseOptions::default()).is_err())
    }
    #[test]
    fn untrusted_counts_and_unknown_curve_fail_closed() {
        let mut b = body();
        b[61..65].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode(&b, SOLID, ParseOptions::default()),
            Err(ComplexError::Limit(_))
        ));
        let mut b = body();
        b[65..69].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            decode(&b, SOLID, ParseOptions::default()),
            Err(ComplexError::Unsupported(_))
        ));
        let mut b = body();
        b[69..77].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&b, SOLID, ParseOptions::default()).is_err())
    }
    #[test]
    fn dimension_keeps_semantics_and_presentation_distinct() {
        let mut b = attrs();
        u(&mut b, 0);
        f(&mut b, 12.5);
        for _ in 0..2 {
            u(&mut b, 2);
            b.extend_from_slice(&[0, 0])
        }
        u(&mut b, 0);
        for _ in 0..3 {
            u(&mut b, 2);
            b.extend_from_slice(&[0, 0])
        }
        f(&mut b, 1.);
        u(&mut b, 0);
        u(&mut b, 0);
        for _ in 0..9 {
            f(&mut b, 0.)
        }
        b.extend_from_slice(&[1, 2, 3, 4]);
        let d = decode(&b, DIMS[0], ParseOptions::default()).unwrap();
        let ComplexGeometry::Dimension(d) = d.geometry else {
            panic!()
        };
        assert_eq!(d.value_stored, 12.5);
        assert!(d.presentation_records.is_empty());
        assert_eq!(d.unresolved_semantics_span.length, 4);
        let options = ParseOptions {
            max_string_bytes: 1,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode(&b, DIMS[0], options),
            Err(ComplexError::Limit(_))
        ))
    }
    #[test]
    fn newer_fill_relation_and_gradient_have_exact_bounds() {
        let mut b = body();
        u(&mut b, 0);
        u(&mut b, 0x80800000);
        u(&mut b, 0);
        u(&mut b, 0);
        u(&mut b, 0);
        for v in [0., 0., 0., 0., 0., 1.] {
            f(&mut b, v)
        }
        for _ in 0..5 {
            u(&mut b, 0)
        }
        for _ in 0..3 {
            f(&mut b, 0.)
        }
        u(&mut b, 0);
        f(&mut b, 0.);
        u(&mut b, 1);
        u(&mut b, 0);
        u(&mut b, 2);
        b.extend_from_slice(&[0, 0]);
        f(&mut b, 0.);
        f(&mut b, 0.);
        u(&mut b, 0);
        f(&mut b, 0.);
        u(&mut b, 0);
        u(&mut b, 0);
        f(&mut b, 0.);
        f(&mut b, 1.);
        for major in [11, 15, 16] {
            let r = decode_complex(
                &b,
                &header(SOLID, b.len()),
                RecordProfile::Schema3Attributes33,
                major,
                ParseOptions::default(),
            )
            .unwrap();
            assert!(r.is_ok(), "{r:?}")
        }
    }
    #[test]
    fn standalone_nurbs_and_ellipse_are_bounded() {
        let mut b = attrs();
        for v in [0., 0., 1., 0.] {
            f(&mut b, v);
        }
        for v in [2, 1, 1016, 0] {
            u(&mut b, v);
        }
        for v in [0., 0., 0., 1., 2., 3., 0., 1., 0., 0., 1., 1.] {
            f(&mut b, v);
        }
        let result = decode(&b, NURBS, ParseOptions::default()).unwrap();
        let ComplexGeometry::Nurbs(n) = result.geometry else {
            panic!()
        };
        assert_eq!(n.control_points, vec![[0., 0., 0., 1.], [2., 3., 0., 1.]]);
        assert_eq!(n.knots, vec![0., 0., 1., 1.]);
        for end in 0..b.len() {
            assert!(decode(&b[..end], NURBS, ParseOptions::default()).is_err());
        }
        assert!(decode_complex(
            &b,
            &header(NURBS, b.len()),
            RecordProfile::Schema3Attributes33,
            15,
            ParseOptions::default()
        )
        .unwrap()
        .is_err());
        b[69..73].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode(&b, NURBS, ParseOptions::default()),
            Err(ComplexError::Limit(_))
        ));
        let mut b = attrs();
        for v in [
            1.,
            2.,
            0.,
            1.,
            0.,
            0.,
            0.,
            1.,
            0.,
            4.,
            2.,
            0.,
            std::f64::consts::TAU,
            0.,
        ] {
            f(&mut b, v);
        }
        assert!(decode(&b, ELLIPSE, ParseOptions::default()).is_ok());
        b[49..57].copy_from_slice(&1_f64.to_le_bytes());
        assert!(decode(&b, ELLIPSE, ParseOptions::default()).is_err());
    }
}

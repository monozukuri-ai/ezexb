//! Text-style storage and display inputs. These do not claim native glyph layout.
use crate::{ContextRecord, Error, ParseOptions, SourceSpan, TextField, TextGeometry};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct TextStyle {
    pub id: i32,
    pub owner: i32,
    pub span: SourceSpan,
    pub name: TextField,
    pub alternate_name: TextField,
    pub western_font: TextField,
    pub cjk_font: TextField,
    pub western_width_factor: f64,
    pub cjk_width_factor: f64,
    pub character_spacing_factor: f64,
    pub line_spacing_factor: f64,
    pub height: f64,
    pub oblique_angle: f64,
    pub indent: f64,
    pub shape_file_flag_raw: Option<u32>,
    pub shape_file_path: Option<TextField>,
    pub extra_flags_raw: Option<[u16; 3]>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TextStyleContext {
    pub complete: bool,
    pub styles: Vec<TextStyle>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TextPresentation {
    pub contents: String,
    pub style_id: i32,
    pub western_font: Option<String>,
    pub cjk_font: Option<String>,
    pub western_width_factor: Option<f64>,
    pub cjk_width_factor: Option<f64>,
    pub character_spacing_factor: Option<f64>,
    pub line_spacing_factor: Option<f64>,
    pub line_spacing_source: String,
    pub oblique_angle: Option<f64>,
    pub format: String,
    pub area_width: f64,
    pub area_height: f64,
    /// Stored nominal height. Native format/font conversion remains separate.
    pub nominal_height: f64,
    pub diagnostics: Vec<String>,
}
impl TextGeometry {
    pub fn presentation(&self, styles: &[TextStyle]) -> TextPresentation {
        let style = styles.iter().find(|s| s.id == self.style_id_raw);
        let mut diagnostics =
            vec!["Native glyph metrics and format-specific layout are not reproduced.".into()];
        if style.is_none() {
            diagnostics.push(format!("Text style {} is unavailable.", self.style_id_raw));
        }
        let inherited = self.line_spacing_raw == -1.0;
        let format = match self.format_type_id.as_deref() {
            Some("0c782593-3186-4c7b-b75d-e3f25b6c34a2") => "acad",
            Some("01baf0ca-7a9c-4169-a67d-48e89b4e5fec") => "neb",
            Some("5a1c95d6-decd-4757-ae65-77bc36f5540b") => "oeb",
            Some("6cabfdc4-340a-44d6-a844-d865a15538c0") => "table",
            None if self.legacy_format_raw == Some(0) => "oeb",
            None if self.legacy_format_raw == Some(1) => "neb",
            _ => "unresolved",
        };
        TextPresentation {
            contents: self.contents.clone(),
            style_id: self.style_id_raw,
            western_font: style.map(|s| s.western_font.value.clone()),
            cjk_font: style.map(|s| s.cjk_font.value.clone()),
            western_width_factor: style.map(|s| s.western_width_factor),
            cjk_width_factor: style.map(|s| s.cjk_width_factor),
            character_spacing_factor: style.map(|s| s.character_spacing_factor),
            line_spacing_factor: if inherited {
                style.map(|s| s.line_spacing_factor)
            } else {
                Some(self.line_spacing_raw)
            },
            line_spacing_source: if inherited {
                "by_text_style"
            } else {
                "explicit"
            }
            .into(),
            oblique_angle: style.map(|s| s.oblique_angle),
            format: format.into(),
            area_width: self.area_width_stored * self.drawing_scale,
            area_height: self.area_height_stored * self.drawing_scale,
            nominal_height: self.text_height_stored,
            diagnostics,
        }
    }
}

/// Decodes already-framed records. No additional record charge: each record was
/// charged by the style-context walker. String allocations remain individually bounded.
pub fn decode_text_styles(
    data: &[u8],
    records: &[ContextRecord],
    options: ParseOptions,
    major: u32,
) -> Result<TextStyleContext, Error> {
    let mut result = TextStyleContext {
        complete: true,
        styles: vec![],
        diagnostics: vec![],
    };
    for record in records
        .iter()
        .filter(|r| r.type_id == "b9bd09d2-0eaa-4fe7-9ff6-a23f19fe8954")
    {
        match decode(data, record, options, major) {
            Ok(style) => result.styles.push(style),
            Err(Error::ResourceLimit {
                name,
                actual,
                limit,
            }) => {
                return Err(Error::ResourceLimit {
                    name,
                    actual,
                    limit,
                })
            }
            Err(error) => {
                result.complete = false;
                result
                    .diagnostics
                    .push(format!("Text style {}: {error}", record.id));
            }
        }
    }
    Ok(result)
}
fn invalid() -> Error {
    Error::UnsupportedFeature("text style body layout")
}
fn decode(
    data: &[u8],
    r: &ContextRecord,
    options: ParseOptions,
    major: u32,
) -> Result<TextStyle, Error> {
    if !matches!(major, 10 | 11 | 15 | 16 | 18) || !matches!(r.object_schema, 1 | 2 | 100) {
        return Err(invalid());
    }
    let start = usize::try_from(r.payload_span.offset).map_err(|_| invalid())?;
    let length = usize::try_from(r.payload_span.length).map_err(|_| invalid())?;
    let end = start.checked_add(length).ok_or_else(invalid)?;
    let mut c = Cursor {
        data: data.get(start..end).ok_or_else(invalid)?,
        pos: 0,
        base: r.payload_span.offset,
        options,
    };
    let name = c.text()?;
    let alternate_name = c.text()?;
    let western_font = c.text()?;
    let cjk_font = c.text()?;
    let western_width_factor = c.f64()?;
    let cjk_width_factor = c.f64()?;
    let character_spacing_factor = c.f64()?;
    let line_spacing_factor = c.f64()?;
    let height = c.f64()?;
    let oblique_angle = c.f64()?;
    let indent = c.f64()?;
    let (shape_file_flag_raw, shape_file_path) = if major > 10 {
        (Some(c.u32()?), Some(c.text()?))
    } else {
        (None, None)
    };
    let extra_flags_raw = if major >= 18 {
        Some([c.u16()?, c.u16()?, c.u16()?])
    } else {
        None
    };
    if c.pos != c.data.len() {
        return Err(invalid());
    }
    Ok(TextStyle {
        id: r.id,
        owner: r.owner,
        span: r.payload_span.clone(),
        name,
        alternate_name,
        western_font,
        cjk_font,
        western_width_factor,
        cjk_width_factor,
        character_spacing_factor,
        line_spacing_factor,
        height,
        oblique_angle,
        indent,
        shape_file_flag_raw,
        shape_file_path,
        extra_flags_raw,
    })
}
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    base: u64,
    options: ParseOptions,
}
impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.pos.checked_add(n).ok_or_else(invalid)?;
        let b = self.data.get(self.pos..end).ok_or_else(invalid)?;
        self.pos = end;
        Ok(b)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, Error> {
        let v = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !v.is_finite() {
            return Err(invalid());
        }
        Ok(v)
    }
    fn text(&mut self) -> Result<TextField, Error> {
        let start = self.pos;
        let n = self.u32()?;
        ParseOptions::check_limit("max_string_bytes", n.into(), self.options.max_string_bytes)?;
        if n < 2 || n % 2 != 0 {
            return Err(invalid());
        }
        let raw = self.take(n as usize)?;
        if raw[raw.len() - 2..] != [0, 0] {
            return Err(invalid());
        }
        let chars: Vec<_> = raw[..raw.len() - 2]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let value = String::from_utf16(&chars).map_err(|_| invalid())?;
        if value.contains('\0') {
            return Err(invalid());
        }
        Ok(TextField {
            value,
            span: SourceSpan {
                offset: self.base + start as u64,
                length: (self.pos - start) as u64,
            },
        })
    }
}

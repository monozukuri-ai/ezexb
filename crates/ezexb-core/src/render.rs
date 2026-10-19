//! Portable SVG preview. Native font layout and device styles remain unqualified.
use crate::*;
use serde::Serialize;
use std::fmt::Write;

#[derive(Clone, Debug, Serialize)]
pub struct SvgPreview {
    pub svg: String,
    pub space_id: i32,
    pub source_document_complete: bool,
    pub native_display_equivalent: bool,
    pub rendered_curve_count: u64,
    pub rendered_text_count: u64,
    pub diagnostics: Vec<String>,
}
struct Canvas {
    body: String,
    bounds: [f64; 4],
    curves: u64,
    texts: u64,
    diagnostics: Vec<String>,
    options: ParseOptions,
}
impl Canvas {
    fn bound(&mut self, p: [f64; 3]) {
        self.bounds[0] = self.bounds[0].min(p[0]);
        self.bounds[1] = self.bounds[1].min(p[1]);
        self.bounds[2] = self.bounds[2].max(p[0]);
        self.bounds[3] = self.bounds[3].max(p[1]);
    }
    fn check(&self) -> Result<(), Error> {
        ParseOptions::check_limit(
            "max_decoded_stream_bytes",
            self.body.len() as u64,
            self.options.max_decoded_stream_bytes,
        )?;
        ParseOptions::check_limit(
            "max_records",
            self.curves + self.texts,
            self.options.max_records,
        )
    }
    fn curve(&mut self, curve: &WorldCurve) -> Result<(), Error> {
        match curve {
            WorldCurve::Point { point } => {
                self.bound(*point);
                writeln!(
                    self.body,
                    "<path vector-effect=\"non-scaling-stroke\" stroke-linecap=\"round\" d=\"M {} {} L {} {}\"/>",
                    point[0], point[1], point[0], point[1]
                )
                .unwrap();
            }
            WorldCurve::Line { start, end } => {
                self.bound(*start);
                self.bound(*end);
                writeln!(
                    self.body,
                    "<path vector-effect=\"non-scaling-stroke\" d=\"M {} {} L {} {}\"/>",
                    start[0], start[1], end[0], end[1]
                )
                .unwrap();
            }
            _ => {
                let (center, u, v, sweep, closed) = match curve {
                    WorldCurve::Circle {
                        center,
                        radius,
                        x_axis,
                        y_axis,
                    } => (
                        *center,
                        x_axis.map(|v| v * radius),
                        y_axis.map(|v| v * radius),
                        std::f64::consts::TAU,
                        true,
                    ),
                    WorldCurve::Arc {
                        center,
                        radius,
                        x_axis,
                        y_axis,
                        sweep_angle,
                        ..
                    } => (
                        *center,
                        x_axis.map(|v| v * radius),
                        y_axis.map(|v| v * radius),
                        *sweep_angle,
                        *sweep_angle == std::f64::consts::TAU,
                    ),
                    WorldCurve::EllipticCurve {
                        center,
                        axis_u,
                        axis_v,
                        sweep_angle,
                        closed,
                        ..
                    } => (*center, *axis_u, *axis_v, *sweep_angle, *closed),
                    _ => unreachable!(),
                };
                let x = u[0].abs() + v[0].abs();
                let y = u[1].abs() + v[1].abs();
                self.bound([center[0] - x, center[1] - y, 0.]);
                self.bound([center[0] + x, center[1] + y, 0.]);
                let d = if closed {
                    "M 1 0 A 1 1 0 1 1 -1 0 A 1 1 0 1 1 1 0".into()
                } else {
                    format!(
                        "M 1 0 A 1 1 0 {} 1 {} {}",
                        u8::from(sweep > std::f64::consts::PI),
                        sweep.cos(),
                        sweep.sin()
                    )
                };
                writeln!(
                    self.body,
                    "<path vector-effect=\"non-scaling-stroke\" transform=\"matrix({} {} {} {} {} {})\" d=\"{}\"/>",
                    u[0], u[1], v[0], v[1], center[0], center[1], d
                )
                .unwrap();
            }
        }
        self.curves += 1;
        self.check()
    }
    fn boundary(
        &mut self,
        boundary: &FillBoundary,
        transform: &WorldTransform,
        id: i32,
    ) -> Result<(), Error> {
        for boundary_loop in &boundary.loops {
            for boundary in &boundary_loop.curves {
                let primitive = match &boundary.geometry {
                    BoundaryGeometry::Point { point } => {
                        Some(PrimitiveGeometry::Point { point: *point })
                    }
                    BoundaryGeometry::Line { start, end } => Some(PrimitiveGeometry::Line {
                        start: *start,
                        end: *end,
                    }),
                    BoundaryGeometry::Circular {
                        center,
                        radius,
                        x_axis,
                        y_axis,
                        sweep_angle,
                    } => Some(PrimitiveGeometry::Arc {
                        center: *center,
                        radius: *radius,
                        x_axis: *x_axis,
                        y_axis: *y_axis,
                        sweep_angle: *sweep_angle,
                        start: [0.; 3],
                        end: [0.; 3],
                    }),
                    BoundaryGeometry::Polyline {
                        vertices,
                        closed,
                        normal,
                    } => {
                        let p = PolylineGeometry {
                            vertices: vertices
                                .iter()
                                .map(|v| PolylineVertex {
                                    point: v.point,
                                    legacy_z: v.legacy_z,
                                    bulge: v.bulge,
                                    start_width: v.start_width,
                                    end_width: v.end_width,
                                    span: boundary.span.clone(),
                                })
                                .collect(),
                            closed: *closed,
                            closed_raw: u32::from(*closed),
                            normal: *normal,
                            span: boundary.span.clone(),
                        };
                        if p.normal.is_some_and(|n| n != [0., 0., 1.])
                            || p.vertices
                                .iter()
                                .any(|v| v.legacy_z.is_some_and(|z| z != 0.))
                        {
                            self.diagnostics.push(format!(
                                "Entity {id}: nondefault boundary polyline plane omitted."
                            ));
                        } else {
                            match crate::world::transform_polyline(&p, transform) {
                                Ok(p) => {
                                    for s in &p.segments {
                                        self.curve(&s.curve)?;
                                    }
                                }
                                Err(e) => self.diagnostics.push(format!("Entity {id}: {e}")),
                            }
                        }
                        None
                    }
                    _ => {
                        self.diagnostics.push(format!(
                            "Entity {id}: ellipse/NURBS boundary preview is unavailable."
                        ));
                        None
                    }
                };
                if let Some(p) = primitive {
                    match transform_primitive(&p, transform) {
                        Ok(curve) => self.curve(&curve)?,
                        Err(e) => self.diagnostics.push(format!("Entity {id}: {e}")),
                    }
                }
            }
        }
        Ok(())
    }
    fn presentation(
        &mut self,
        records: &[PresentationRecord],
        transform: &WorldTransform,
        styles: &[TextStyle],
        id: i32,
    ) -> Result<(), Error> {
        for r in records {
            if r.attributes.as_ref().is_some_and(|a| a.hide_flag_raw != 0) {
                if r.attributes
                    .as_ref()
                    .is_some_and(|a| a.hide_flag_raw != u16::MAX)
                {
                    self.diagnostics.push(format!(
                        "Entity {id}: unknown cached visibility; item omitted."
                    ));
                }
                continue;
            }
            match &r.geometry {
                Some(PresentationGeometry::Primitive(p)) => match transform_primitive(p, transform)
                {
                    Ok(curve) => self.curve(&curve)?,
                    Err(e) => self.diagnostics.push(format!("Entity {id}: {e}")),
                },
                Some(PresentationGeometry::Annotation(AnnotationGeometry::Text(text))) => {
                    self.text(text, transform, styles, id)?
                }
                Some(PresentationGeometry::Annotation(AnnotationGeometry::Polyline(p))) => {
                    match crate::world::transform_polyline(p, transform) {
                        Ok(p) => {
                            for s in &p.segments {
                                self.curve(&s.curve)?;
                            }
                        }
                        Err(e) => self.diagnostics.push(format!("Entity {id}: {e}")),
                    }
                }
                Some(PresentationGeometry::Solid(s)) => {
                    self.boundary(&s.boundary, transform, id)?
                }
                Some(PresentationGeometry::PartItem(item)) => {
                    self.presentation(&item.presentation_records, transform, styles, id)?
                }
                None => self.diagnostics.push(format!(
                    "Entity {id}: unsupported cached presentation record {}.",
                    r.header.id_raw
                )),
            }
        }
        Ok(())
    }
    fn text(
        &mut self,
        text: &TextGeometry,
        transform: &WorldTransform,
        styles: &[TextStyle],
        id: i32,
    ) -> Result<(), Error> {
        let p = text.presentation(styles);
        if text.alignment_raw != 0 || text.writing_direction != 0 || text.fit_mode != 0 {
            self.diagnostics.push(format!(
                "Entity {id}: text alignment/writing/fit mode is unsupported; text omitted."
            ));
            return Ok(());
        }
        if text.contents.contains(['\\', '{', '}']) || text.contents.contains('%') {
            self.diagnostics.push(format!(
                "Entity {id}: formatting escapes are unsupported; text omitted."
            ));
            return Ok(());
        }
        let height = text.text_height_stored * text.drawing_scale;
        let width = p.western_width_factor.unwrap_or(1.0);
        if !height.is_finite() || height <= 0. || !width.is_finite() || width <= 0. {
            self.diagnostics.push(format!(
                "Entity {id}: invalid nominal text size; text omitted."
            ));
            return Ok(());
        }
        let spacing = p.line_spacing_factor.unwrap_or(1.0);
        if spacing <= 0.
            || !spacing.is_finite()
            || !p.area_width.is_finite()
            || !p.area_height.is_finite()
        {
            self.diagnostics.push(format!(
                "Entity {id}: text area/spacing is not qualified; text omitted."
            ));
            return Ok(());
        }
        let (sin, cos) = text.rotation.sin_cos();
        let origin = transform.transform_point(text.insertion);
        let x = transform.transform_vector([cos * width, sin * width, 0.]);
        let y = transform.transform_vector([sin, -cos, 0.]);
        let (Ok(origin), Ok(x), Ok(y)) = (origin, x, y) else {
            self.diagnostics
                .push(format!("Entity {id}: text transform overflow."));
            return Ok(());
        };
        let font = p.western_font.as_deref().unwrap_or("sans-serif");
        let cjk = p.cjk_font.as_deref().unwrap_or("sans-serif");
        if [font, cjk].iter().any(|s| {
            s.to_ascii_lowercase().ends_with(".fnt") || s.to_ascii_lowercase().ends_with(".shx")
        }) {
            self.diagnostics.push(format!(
                "Entity {id}: CAD stroke font unavailable; system fallback used."
            ));
        }
        if p.cjk_width_factor.is_some_and(|v| v != width)
            || p.character_spacing_factor.is_some_and(|v| v != 0.)
            || p.oblique_angle.is_some_and(|v| v != 0.)
        {
            self.diagnostics.push(format!(
                "Entity {id}: mixed width/spacing/oblique style approximated."
            ));
        }
        // All XML data is escaped. No font URLs, scripts, external images or CSS are emitted.
        writeln!(self.body,"<text fill=\"#222\" stroke=\"none\" font-family=\"{}, {}, sans-serif\" font-size=\"{}\" dominant-baseline=\"text-before-edge\" transform=\"matrix({} {} {} {} {} {})\">",xml(font),xml(cjk),height,x[0],x[1],y[0],y[1],origin[0],origin[1]).unwrap();
        let lines = text.contents.replace("\r\n", "\n").replace('\r', "\n");
        for (i, line) in lines.split('\n').enumerate() {
            let dy = i as f64 * height * spacing;
            if !dy.is_finite() {
                return Err(Error::UnsupportedFeature("overflowing SVG text layout"));
            }
            writeln!(
                self.body,
                "<tspan x=\"0\" y=\"{}\" xml:space=\"preserve\">{}</tspan>",
                dy,
                xml(line)
            )
            .unwrap();
        }
        self.body.push_str("</text>\n");
        let w = p
            .area_width
            .abs()
            .max(height * text.contents.chars().count() as f64);
        let h = p
            .area_height
            .abs()
            .max(height * spacing * lines.lines().count().max(1) as f64);
        for (a, b) in [(0., 0.), (w, 0.), (0., h), (w, h)] {
            let point = [
                origin[0] + x[0] * a + y[0] * b,
                origin[1] + x[1] * a + y[1] * b,
                0.,
            ];
            if point.iter().any(|v| !v.is_finite()) {
                return Err(Error::UnsupportedFeature("overflowing SVG text bounds"));
            }
            self.bound(point);
        }
        self.texts += 1;
        self.check()
    }
}
fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| {
            *c == '\n'
                || *c == '\r'
                || *c == '\t'
                || ('\u{20}'..='\u{d7ff}').contains(c)
                || ('\u{e000}'..='\u{fffd}').contains(c)
                || ('\u{10000}'..='\u{10ffff}').contains(c)
        })
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            _ => c.to_string(),
        })
        .collect()
}
/// Explicit world-XY preview using system fonts and neutral stroke styling.
/// Check diagnostics: a generated SVG never implies CAXA display equivalence.
pub fn render_svg(
    document: &Document,
    space_id: i32,
    options: ParseOptions,
) -> Result<SvgPreview, Error> {
    let scene = expand_world(document, space_id, options)?;
    let mut c=Canvas{body:String::new(),bounds:[f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY],curves:0,texts:0,diagnostics:vec!["Portable world-XY preview: system fonts, nominal text metrics and neutral strokes; native layout, line styles and hatch filling are not reproduced.".into()],options};
    if document
        .blocks
        .iter()
        .any(|b| b.definition.solid_attachment.is_some())
    {
        c.diagnostics
            .push("Optional block SOLID attachments are not rendered.".into());
    }
    for d in &scene.diagnostics {
        c.diagnostics.push(format!("{}: {}", d.code, d.message));
    }
    for instance in &scene.instances {
        if !instance.visible {
            continue;
        }
        if let Some(geometry) = &instance.geometry {
            match geometry {
                WorldGeometry::Curve(curve) => c.curve(curve)?,
                WorldGeometry::Polyline(p) => {
                    for segment in &p.segments {
                        c.curve(&segment.curve)?;
                    }
                }
            }
        }
        let entity = &document.entities[instance.entity_index];
        match &entity.geometry {
            Geometry::Annotation(AnnotationGeometry::Text(text)) => c.text(
                text,
                &instance.transform,
                &document.text_style_context.styles,
                entity.id,
            )?,
            Geometry::Insert(insert) => {
                for attribute in &insert.attached_attributes {
                    // Native Draw pops this INSERT matrix before drawing attached attributes.
                    // `instance.transform` is the outer container transform.
                    if attribute.visibility == Some(true)
                        && attribute.attributes.hide_flag_raw == 0
                        && attribute.text_attributes.hide_flag_raw == 0
                    {
                        c.text(
                            &attribute.text,
                            &instance.transform,
                            &document.text_style_context.styles,
                            attribute.record.id,
                        )?;
                    } else if attribute.visibility.is_none()
                        || !matches!(attribute.attributes.hide_flag_raw, 0 | u16::MAX)
                        || !matches!(attribute.text_attributes.hide_flag_raw, 0 | u16::MAX)
                    {
                        c.diagnostics.push(format!(
                            "Attribute {}: visibility unknown; text omitted.",
                            attribute.record.id
                        ));
                    }
                }
            }
            Geometry::Complex(ComplexGeometry::Hatch(h)) => {
                c.boundary(&h.boundary, &instance.transform, entity.id)?
            }
            Geometry::Complex(ComplexGeometry::Solid(s)) => {
                c.boundary(&s.boundary, &instance.transform, entity.id)?
            }
            Geometry::Complex(ComplexGeometry::Dimension(d)) => c.presentation(
                &d.presentation_records,
                &instance.transform,
                &document.text_style_context.styles,
                entity.id,
            )?,
            Geometry::Complex(ComplexGeometry::Symbol(s)) => c.presentation(
                &s.presentation_records,
                &instance.transform,
                &document.text_style_context.styles,
                entity.id,
            )?,
            Geometry::PartNumber(p) => c.presentation(
                &p.presentation_records,
                &instance.transform,
                &document.text_style_context.styles,
                entity.id,
            )?,
            Geometry::Table(table) => c.presentation(
                &table.presentation_records,
                &instance.transform,
                &document.text_style_context.styles,
                entity.id,
            )?,
            Geometry::BomTable(table) => {
                for item in &table.items {
                    match item.attributes.hide_flag_raw {
                        0 => c.presentation(
                            &item.presentation_records,
                            &instance.transform,
                            &document.text_style_context.styles,
                            entity.id,
                        )?,
                        u16::MAX => {}
                        _ => c.diagnostics.push(format!(
                            "BOM item {}: visibility unknown; presentation omitted.",
                            item.header.id_raw
                        )),
                    }
                }
            }
            Geometry::AttributeDefinition(_) => {
                c.diagnostics.push(format!("Entity {}: attribute definition display is omitted; attached instance text is handled separately.",entity.id));
            }
            _ => {}
        }
    }
    if !c.bounds[0].is_finite() {
        c.bounds = [0., 0., 1., 1.];
    }
    let width = (c.bounds[2] - c.bounds[0]).max(1.);
    let height = (c.bounds[3] - c.bounds[1]).max(1.);
    let margin = width.max(height) * 0.03;
    if !width.is_finite() || !height.is_finite() || !margin.is_finite() {
        return Err(Error::UnsupportedFeature("overflowing SVG bounds"));
    }
    let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\"><title>EXB portable preview (approximate text and styles)</title><g transform=\"scale(1 -1)\" fill=\"none\" stroke=\"#222\" stroke-width=\"{}\" vector-effect=\"non-scaling-stroke\">\n{}</g></svg>\n",c.bounds[0]-margin,-c.bounds[3]-margin,width+2.*margin,height+2.*margin,width.max(height)/1500.,c.body);
    ParseOptions::check_limit(
        "max_decoded_stream_bytes",
        svg.len() as u64,
        options.max_decoded_stream_bytes,
    )?;
    Ok(SvgPreview {
        svg,
        space_id,
        source_document_complete: document.report.complete,
        native_display_equivalent: false,
        rendered_curve_count: c.curves,
        rendered_text_count: c.texts,
        diagnostics: c.diagnostics,
    })
}

#[cfg(test)]
use crate::test_fixture as fixture;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_svg_escapes_text_and_never_claims_native_fidelity() {
        let mut doc = read_bytes(&fixture::fixture(18), ParseOptions::default(), true).unwrap();
        let Geometry::Annotation(AnnotationGeometry::Text(text)) = &mut doc.entities[4].geometry
        else {
            panic!()
        };
        text.contents = "<&\" 日本語".into();
        text.alignment_raw = 0;
        text.writing_direction = 0;
        text.fit_mode = 0;
        let svg = render_svg(&doc, -7, ParseOptions::default()).unwrap();
        assert!(!svg.native_display_equivalent);
        assert!(svg.rendered_curve_count >= 3);
        assert_eq!(svg.rendered_text_count, 1);
        assert!(svg.svg.contains("&lt;&amp;&quot; 日本語"));
        assert!(!svg.svg.contains("<script"));
        assert!(svg.svg.contains("sans-serif"));
        assert!(!svg.diagnostics.is_empty());
    }
    #[test]
    fn svg_size_and_item_limits_are_hard_errors() {
        let doc = read_bytes(&fixture::fixture(10), ParseOptions::default(), true).unwrap();
        let opts = ParseOptions {
            max_decoded_stream_bytes: 50,
            ..ParseOptions::default()
        };
        assert!(matches!(
            render_svg(&doc, -7, opts),
            Err(Error::ResourceLimit { .. })
        ));
        let opts = ParseOptions {
            max_records: 1,
            ..ParseOptions::default()
        };
        assert!(matches!(
            render_svg(&doc, -7, opts),
            Err(Error::ResourceLimit { .. })
        ));
    }
    #[test]
    fn text_style_spacing_is_resolved_without_replacing_stored_values() {
        let doc = read_bytes(&fixture::fixture(18), ParseOptions::default(), true).unwrap();
        assert!(doc.text_style_context.complete);
        let Geometry::Annotation(AnnotationGeometry::Text(text)) = &doc.entities[4].geometry else {
            panic!()
        };
        let mut text = text.clone();
        text.line_spacing_raw = -1.;
        let presentation = text.presentation(&doc.text_style_context.styles);
        assert_eq!(presentation.line_spacing_factor, Some(1.5));
        assert_eq!(text.line_spacing_raw, -1.);
        assert_eq!(presentation.line_spacing_source, "by_text_style");
        assert_eq!(presentation.western_font.as_deref(), Some("sans-serif"));
    }
}

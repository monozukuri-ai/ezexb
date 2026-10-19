//! Bounded expansion of stored block instances using native affine matrices.
//! This is a geometry scene; viewport projection and font/device rendering are separate.
use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::attributes::resolve_entity_style;
use crate::{
    AnnotationGeometry, Diagnostic, Document, Entity, Error, Geometry, Insert, Layer, ParseOptions,
    PolylineGeometry, PrimitiveGeometry, ResolvedStyle,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorldTransform {
    /// Row-major array applied to row vectors: x' = x*m[0]+y*m[4]+z*m[8]+m[12].
    pub matrix: [f64; 16],
}
impl WorldTransform {
    pub fn identity() -> Self {
        Self {
            matrix: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
        }
    }
    pub fn from_stored(matrix: [f64; 16]) -> Result<Self, String> {
        if matrix.iter().any(|v| !v.is_finite()) {
            return Err("Non-finite world transformation.".into());
        }
        if matrix[3] != 0. || matrix[7] != 0. || matrix[11] != 0. || matrix[15] != 1. {
            return Err("Only affine stored INSERT transformations are qualified.".into());
        }
        Ok(Self { matrix })
    }
    /// Qualify the saved matrix against native XY draw components. Contradictory
    /// representations are rejected instead of choosing one silently.
    pub fn from_insert(insert: &Insert) -> Result<Self, String> {
        if !insert.rotation.is_finite()
            || insert
                .insertion
                .iter()
                .chain(&insert.scales)
                .any(|v| !v.is_finite())
            || insert.insertion[2] != 0.
            || insert.scales[0] == 0.
            || insert.scales[1] == 0.
        {
            return Err("INSERT draw components are nonfinite, degenerate, or nonplanar.".into());
        }
        let angle = if insert.rotation.abs() < 1e-5 {
            0.
        } else {
            insert.rotation
        };
        let mut sx = insert.scales[0];
        let mut sy = insert.scales[1];
        if angle == 0. && (sx - 1.).abs() < 1e-10 && (sy - 1.).abs() < 1e-10 {
            sx = 1.;
            sy = 1.;
        }
        let (sin, cos) = angle.sin_cos();
        let expected = [
            sx * cos,
            sx * sin,
            0.,
            0.,
            -sy * sin,
            sy * cos,
            0.,
            0.,
            0.,
            0.,
            insert.scales[2],
            0.,
            insert.insertion[0],
            insert.insertion[1],
            0.,
            1.,
        ];
        let transform = Self::from_stored(insert.matrix_stored)?;
        if transform
            .matrix
            .iter()
            .zip(expected)
            .any(|(actual, expected)| (*actual - expected).abs() > 1e-10 * expected.abs().max(1.))
        {
            return Err("Stored INSERT matrix disagrees with native XY draw components.".into());
        }
        Ok(transform)
    }
    /// Apply this transform, followed by `parent`. Child-to-root is child * parent.
    pub fn then(&self, parent: &Self) -> Result<Self, String> {
        Self::from_stored(self.matrix)?;
        Self::from_stored(parent.matrix)?;
        let mut result = [0.; 16];
        for row in 0..4 {
            for col in 0..4 {
                result[row * 4 + col] = (0..4)
                    .map(|k| self.matrix[row * 4 + k] * parent.matrix[k * 4 + col])
                    .sum();
            }
        }
        Self::from_stored(result)
    }
    pub fn transform_point(&self, point: [f64; 3]) -> Result<[f64; 3], String> {
        self.apply(point, true)
    }
    pub fn transform_vector(&self, vector: [f64; 3]) -> Result<[f64; 3], String> {
        self.apply(vector, false)
    }
    fn apply(&self, point: [f64; 3], translate: bool) -> Result<[f64; 3], String> {
        Self::from_stored(self.matrix)?;
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Non-finite source coordinate.".into());
        }
        let result = std::array::from_fn(|i| {
            point[0] * self.matrix[i]
                + point[1] * self.matrix[4 + i]
                + point[2] * self.matrix[8 + i]
                + if translate { self.matrix[12 + i] } else { 0. }
        });
        if result.iter().any(|v: &f64| !v.is_finite()) {
            return Err("World coordinate computation overflowed.".into());
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum WorldCurve {
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
        sweep_angle: f64,
        start: [f64; 3],
        end: [f64; 3],
    },
    /// p(t)=center+cos(t)*axis_u+sin(t)*axis_v, 0 <= t <= sweep_angle.
    /// These radius-inclusive vectors need not be principal or perpendicular axes.
    EllipticCurve {
        center: [f64; 3],
        axis_u: [f64; 3],
        axis_v: [f64; 3],
        sweep_angle: f64,
        closed: bool,
        start: [f64; 3],
        end: [f64; 3],
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorldPolylineSegment {
    pub curve: WorldCurve,
    /// Widths remain in the definition's coordinates; no filled outline is implied.
    pub start_width_stored: f64,
    pub end_width_stored: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "Polyline")]
pub struct WorldPolyline {
    pub closed: bool,
    pub segments: Vec<WorldPolylineSegment>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WorldGeometry {
    Curve(WorldCurve),
    Polyline(WorldPolyline),
}
#[derive(Clone, Debug, Serialize)]
pub struct WorldInstance {
    pub entity_index: usize,
    pub entity_id: i32,
    /// Containing INSERT identifiers, ordered from the root to the immediate parent.
    pub insert_path: Vec<i32>,
    pub transform: WorldTransform,
    pub style: ResolvedStyle,
    pub visible: bool,
    /// INSERT containers have no own curve. Other absent geometry has a diagnostic.
    pub geometry: Option<WorldGeometry>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorldScene {
    pub space_id: i32,
    pub root_block_id: i32,
    pub source_document_complete: bool,
    pub transforms_complete: bool,
    pub geometry_complete: bool,
    pub styles_complete: bool,
    /// Geometry expansion does not establish font/device/viewport rendering.
    pub render_complete: bool,
    pub complete: bool,
    pub budget_used: u64,
    pub instances: Vec<WorldInstance>,
    pub diagnostics: Vec<Diagnostic>,
}

fn norm(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: [f64; 3], value: f64) -> [f64; 3] {
    a.map(|v| v * value)
}

/// Transform a known primitive without treating nonuniform scaling as a circle.
pub fn transform_primitive(
    geometry: &PrimitiveGeometry,
    transform: &WorldTransform,
) -> Result<WorldCurve, String> {
    let (center, radius, x_axis, y_axis, sweep, closed) = match geometry {
        PrimitiveGeometry::DegenerateArc { .. } => {
            return Err(
                "Degenerate ARC retains stored values but has no qualified world curve.".into(),
            );
        }
        PrimitiveGeometry::Point { point } => {
            return Ok(WorldCurve::Point {
                point: transform.transform_point(*point)?,
            })
        }
        PrimitiveGeometry::Line { start, end } => {
            return Ok(WorldCurve::Line {
                start: transform.transform_point(*start)?,
                end: transform.transform_point(*end)?,
            })
        }
        PrimitiveGeometry::Circle {
            center,
            radius,
            x_axis,
            y_axis,
        } => (
            *center,
            *radius,
            *x_axis,
            *y_axis,
            std::f64::consts::TAU,
            true,
        ),
        PrimitiveGeometry::Arc {
            center,
            radius,
            x_axis,
            y_axis,
            sweep_angle,
            ..
        } => (*center, *radius, *x_axis, *y_axis, *sweep_angle, false),
    };
    if !radius.is_finite()
        || radius <= 0.
        || !sweep.is_finite()
        || sweep <= 0.
        || sweep > std::f64::consts::TAU
    {
        return Err("Invalid source circular primitive.".into());
    }
    let center = transform.transform_point(center)?;
    let axis_u = transform.transform_vector(scale(x_axis, radius))?;
    let axis_v = transform.transform_vector(scale(y_axis, radius))?;
    let u = norm(axis_u);
    let v = norm(axis_v);
    if !u.is_finite() || !v.is_finite() || u == 0. || v == 0. {
        return Err("Degenerate or overflowing transformed circular primitive.".into());
    }
    let x_axis = scale(axis_u, 1. / u);
    let y_axis = scale(axis_v, 1. / v);
    let cosine = dot(x_axis, y_axis);
    if !cosine.is_finite() || cosine.abs() >= 1. - 1e-12 {
        return Err("Transformed circular primitive has collinear axes.".into());
    }
    let start = add(center, axis_u);
    let end = add(
        center,
        add(scale(axis_u, sweep.cos()), scale(axis_v, sweep.sin())),
    );
    if start.iter().chain(&end).any(|v| !v.is_finite()) {
        return Err("Transformed curve endpoint computation overflowed.".into());
    }
    if (u - v).abs() <= u.max(v) * 1e-12 && cosine.abs() <= 1e-12 {
        if closed {
            Ok(WorldCurve::Circle {
                center,
                radius: u,
                x_axis,
                y_axis,
            })
        } else {
            Ok(WorldCurve::Arc {
                center,
                radius: u,
                x_axis,
                y_axis,
                sweep_angle: sweep,
                start,
                end,
            })
        }
    } else {
        Ok(WorldCurve::EllipticCurve {
            center,
            axis_u,
            axis_v,
            sweep_angle: sweep,
            closed,
            start,
            end,
        })
    }
}

pub(crate) fn transform_polyline(
    polyline: &PolylineGeometry,
    transform: &WorldTransform,
) -> Result<WorldPolyline, String> {
    let count = polyline.vertices.len();
    let segment_count = if polyline.closed {
        count
    } else {
        count.saturating_sub(1)
    };
    let mut segments = Vec::with_capacity(segment_count);
    for i in 0..segment_count {
        let a = &polyline.vertices[i];
        let b = &polyline.vertices[(i + 1) % count];
        let start = [a.point[0], a.point[1], 0.];
        let end = [b.point[0], b.point[1], 0.];
        let primitive = if a.bulge == 0. {
            PrimitiveGeometry::Line { start, end }
        } else {
            let dx = end[0] - start[0];
            let dy = end[1] - start[1];
            let chord = dx.hypot(dy);
            let bulge = a.bulge;
            if !bulge.is_finite() || !chord.is_finite() || chord == 0. {
                return Err("Degenerate polyline bulge segment.".into());
            }
            let offset = (1. / bulge - bulge) / 4.;
            let center = [
                (start[0] + end[0]) / 2. - dy * offset,
                (start[1] + end[1]) / 2. + dx * offset,
                0.,
            ];
            let radius = chord * (bulge.abs() + 1. / bulge.abs()) / 4.;
            let x_axis = [
                (start[0] - center[0]) / radius,
                (start[1] - center[1]) / radius,
                0.,
            ];
            let sign = bulge.signum();
            let y_axis = [-x_axis[1] * sign, x_axis[0] * sign, 0.];
            PrimitiveGeometry::Arc {
                center,
                radius,
                x_axis,
                y_axis,
                sweep_angle: 4. * bulge.atan().abs(),
                start,
                end,
            }
        };
        segments.push(WorldPolylineSegment {
            curve: transform_primitive(&primitive, transform)?,
            start_width_stored: a.start_width,
            end_width_stored: a.end_width,
        });
    }
    Ok(WorldPolyline {
        closed: polyline.closed,
        segments,
    })
}

struct Expansion<'a> {
    document: &'a Document,
    blocks: HashMap<i32, Vec<usize>>,
    layers: HashMap<i32, &'a Layer>,
    options: ParseOptions,
    scene: WorldScene,
}
impl Expansion<'_> {
    fn charge(&mut self, count: u64) -> Result<(), Error> {
        self.scene.budget_used = self.scene.budget_used.saturating_add(count);
        ParseOptions::check_limit(
            "max_records",
            self.scene.budget_used,
            self.options.max_records,
        )
    }
    fn issue(&mut self, code: &str, message: impl Into<String>, entity: Option<&Entity>) {
        match code {
            "cyclic_block_reference"
            | "unresolved_block"
            | "unsupported_world_clip"
            | "unsupported_world_transform" => self.scene.transforms_complete = false,
            "unsupported_world_geometry" => self.scene.geometry_complete = false,
            _ => self.scene.styles_complete = false,
        }
        self.scene.diagnostics.push(Diagnostic {
            code: code.into(),
            message: message.into(),
            stream_path: entity.map(|v| v.source.stream_path.clone()),
            span: entity.map(|v| v.source.span.clone()),
            record_id: entity.map(|v| v.id),
            type_id: entity.map(|v| v.type_id.clone()),
        });
    }
    fn block(
        &mut self,
        block_id: i32,
        transform: &WorldTransform,
        inherited: Option<&ResolvedStyle>,
        insert_path: &mut Vec<i32>,
        block_path: &mut HashSet<i32>,
        parent_visible: bool,
    ) -> Result<(), Error> {
        ParseOptions::check_limit(
            "max_record_depth",
            insert_path.len() as u64,
            self.options.max_record_depth.min(64),
        )?;
        if !block_path.insert(block_id) {
            self.issue(
                "cyclic_block_reference",
                format!("Cyclic block reference {block_id}."),
                None,
            );
            return Ok(());
        }
        let Some(count) = self.blocks.get(&block_id).map(Vec::len) else {
            self.issue(
                "unresolved_block",
                format!("Block {block_id} is unavailable."),
                None,
            );
            block_path.remove(&block_id);
            return Ok(());
        };
        for position in 0..count {
            let index = self.blocks[&block_id][position];
            self.charge(1)?;
            let entity = &self.document.entities[index];
            let stored_layer = self.layers.get(&entity.attributes.layer_id_raw).copied();
            let zero_layer = stored_layer.is_some_and(|layer| layer.status_raw & 0x8000_0000 != 0);
            let layer = if zero_layer {
                inherited
                    .and_then(|style| self.layers.get(&style.layer_id).copied())
                    .or(stored_layer)
            } else {
                stored_layer
            };
            let mut attributes = entity.attributes.clone();
            if zero_layer {
                if attributes.linetype_flag_raw <= 1 {
                    attributes.linetype_flag_raw = 0;
                }
                if attributes.line_width_flag_raw <= 1 {
                    attributes.line_width_flag_raw = 0;
                }
            }
            let resolution = resolve_entity_style(&attributes, layer, inherited);
            for diagnostic in resolution.diagnostics {
                self.issue(&diagnostic.code, diagnostic.message, Some(entity));
            }
            let mut style = resolution.style;
            if zero_layer {
                if entity.attributes.linetype_flag_raw <= 1 {
                    style.linetype.source = "zero_layer".into();
                }
                if entity.attributes.line_width_flag_raw <= 1 {
                    style.line_width.source =
                        style
                            .line_width
                            .source
                            .replacen("by_layer", "zero_layer", 1);
                }
            }
            for (name, source, resolved) in [
                (
                    "linetype",
                    style.linetype.source.as_str(),
                    style.linetype.value.is_some(),
                ),
                (
                    "color",
                    style.color_raw.source.as_str(),
                    style.color_raw.value.is_some(),
                ),
                (
                    "line width",
                    style.line_width.source.as_str(),
                    style.line_width.value.is_some(),
                ),
            ] {
                if !resolved {
                    self.issue(
                        "unresolved_world_style",
                        format!(
                            "World {name} selection {source} needs instance or device context."
                        ),
                        Some(entity),
                    );
                }
            }
            if !matches!(entity.attributes.hide_flag_raw, 0 | u16::MAX) {
                self.issue(
                    "unsupported_world_visibility",
                    "Entity hide flag is outside the qualified 0/65535 boolean values.",
                    Some(entity),
                );
            }
            let visible = parent_visible
                && entity.attributes.hide_flag_raw == 0
                && layer.is_none_or(|v| v.on && !v.frozen);
            let geometry = match &entity.geometry {
                Geometry::Primitive(primitive) => transform_primitive(primitive,transform).map(WorldGeometry::Curve),
                Geometry::Annotation(AnnotationGeometry::Polyline(polyline)) => {
                    self.charge(polyline.vertices.len()as u64)?;
                    transform_polyline(polyline,transform).map(WorldGeometry::Polyline)
                }
                Geometry::Insert(_) => { self.scene.instances.push(WorldInstance {
                    entity_index:index,entity_id:entity.id,insert_path:insert_path.clone(),transform:transform.clone(),style:style.clone(),visible,geometry:None });
                    if let Geometry::Insert(insert) = &entity.geometry {
                        if !matches!(insert.hide_flag_raw, 0 | u16::MAX) {
                            self.issue("unsupported_world_visibility","INSERT hide flag is outside the qualified 0/65535 boolean values.",Some(entity));
                        }
                        if insert.clip_flag_raw.is_some_and(|v|v!=0) {
                            self.issue("unsupported_world_clip","Clipped INSERT expansion is not qualified.",Some(entity));continue;
                        }
                        match WorldTransform::from_insert(insert).and_then(|child|child.then(transform)) {
                            Ok(child) => {
                                insert_path.push(entity.id);
                                self.block(insert.block_id,&child,Some(&style),insert_path,block_path,visible && insert.hide_flag_raw == 0)?;
                                insert_path.pop();
                            }
                            Err(reason) => self.issue("unsupported_world_transform",reason,Some(entity)),
                        }
                    }
                    continue;
                }
                Geometry::View2D(_) => {
                    self.issue("unsupported_world_clip", "View2D placement and clipping are not evaluated; its referenced block is not expanded.", Some(entity));
                    Err("View2D is retained as a stored view without world geometry.".into())
                }
                _ => Err("World curve conversion is unavailable for this stored entity; its instance transform is retained.".into()),
            };
            let geometry = match geometry {
                Ok(value) => Some(value),
                Err(reason) => {
                    self.issue("unsupported_world_geometry", reason, Some(entity));
                    None
                }
            };
            self.scene.instances.push(WorldInstance {
                entity_index: index,
                entity_id: entity.id,
                insert_path: insert_path.clone(),
                transform: transform.clone(),
                style,
                visible,
                geometry,
            });
        }
        block_path.remove(&block_id);
        Ok(())
    }
}

/// Expand one stored space with bounded visits and recursive INSERT depth.
/// No implicit INSERT rotation/scale or block-base correction is added to the saved matrix.
pub fn expand_world(
    document: &Document,
    space_id: i32,
    options: ParseOptions,
) -> Result<WorldScene, Error> {
    let space = document
        .space_context
        .spaces
        .iter()
        .find(|s| s.id == space_id)
        .ok_or(Error::InvalidInput("Requested world space is unavailable."))?;
    let mut blocks = HashMap::new();
    for block in &document.blocks {
        if blocks.insert(block.definition.id, Vec::new()).is_some() {
            return Err(Error::InvalidInput(
                "Duplicate block identifiers prevent world expansion.",
            ));
        }
    }
    for (index, entity) in document.entities.iter().enumerate() {
        if let Some(indices) = entity.block_id.and_then(|id| blocks.get_mut(&id)) {
            indices.push(index);
        }
    }
    let mut expansion = Expansion {
        document,
        blocks,
        layers: document.styles.layers.iter().map(|v| (v.id, v)).collect(),
        options,
        scene: WorldScene {
            space_id,
            root_block_id: space.block_id,
            source_document_complete: document.report.complete,
            transforms_complete: true,
            geometry_complete: true,
            styles_complete: true,
            render_complete: false,
            complete: false,
            budget_used: 0,
            instances: Vec::new(),
            diagnostics: Vec::new(),
        },
    };
    expansion.block(
        space.block_id,
        &WorldTransform::identity(),
        None,
        &mut Vec::new(),
        &mut HashSet::new(),
        true,
    )?;
    expansion.scene.complete = expansion.scene.transforms_complete
        && expansion.scene.geometry_complete
        && expansion.scene.styles_complete;
    Ok(expansion.scene)
}

impl crate::Container {
    pub fn world_scene(&mut self, space_id: i32, strict: bool) -> Result<WorldScene, Error> {
        let options = self.options();
        expand_world(&self.document(strict)?, space_id, options)
    }
    pub fn svg_preview(&mut self, space_id: i32, strict: bool) -> Result<crate::SvgPreview, Error> {
        let options = self.options();
        crate::render_svg(&self.document(strict)?, space_id, options)
    }
}

#[cfg(test)]
use crate::test_fixture as fixture;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn point_transform_preserves_translation_and_rejects_overflow() {
        let point = PrimitiveGeometry::Point {
            point: [2., -3., 0.],
        };
        let mut transform = WorldTransform::identity();
        transform.matrix[0] = -2.;
        transform.matrix[5] = 3.;
        transform.matrix[12] = 10.;
        transform.matrix[13] = 20.;
        assert_eq!(
            transform_primitive(&point, &transform).unwrap(),
            WorldCurve::Point {
                point: [6., 11., 0.]
            }
        );
        transform.matrix[0] = f64::MAX;
        assert!(transform_primitive(&point, &transform).is_err());
    }
    #[test]
    fn native_row_vector_order_and_nested_composition() {
        let mut translation = WorldTransform::identity();
        translation.matrix[12] = 10.;
        translation.matrix[13] = -2.;
        let rotation = WorldTransform::from_stored([
            0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ])
        .unwrap();
        let point = [2., 3., 0.];
        assert_eq!(rotation.transform_point(point).unwrap(), [-3., 2., 0.]);
        assert_eq!(
            rotation
                .then(&translation)
                .unwrap()
                .transform_point(point)
                .unwrap(),
            [7., 0., 0.]
        );
        assert_eq!(
            translation
                .then(&rotation)
                .unwrap()
                .transform_point(point)
                .unwrap(),
            [-1., 12., 0.]
        );
        assert_eq!(translation.transform_vector(point).unwrap(), point);
    }
    #[test]
    fn nonuniform_circle_is_ellipse_and_shear_preserves_parameterization() {
        let circle = PrimitiveGeometry::Circle {
            center: [1., 2., 0.],
            radius: 3.,
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        };
        let mut transform = WorldTransform::identity();
        transform.matrix[0] = 2.;
        transform.matrix[4] = 0.5;
        let WorldCurve::EllipticCurve {
            center,
            axis_u,
            axis_v,
            closed,
            ..
        } = transform_primitive(&circle, &transform).unwrap()
        else {
            panic!("ellipse expected")
        };
        assert_eq!(center, [3., 2., 0.]);
        assert_eq!(axis_u, [6., 0., 0.]);
        assert_eq!(axis_v, [1.5, 3., 0.]);
        assert!(closed);
        let mut mirror = WorldTransform::identity();
        mirror.matrix[0] = -2.;
        mirror.matrix[5] = 2.;
        let WorldCurve::Circle {
            radius,
            x_axis,
            y_axis,
            ..
        } = transform_primitive(&circle, &mirror).unwrap()
        else {
            panic!("circle expected")
        };
        assert_eq!(radius, 6.);
        assert_eq!(x_axis, [-1., 0., 0.]);
        assert_eq!(y_axis, [0., 1., 0.]);
    }
    #[test]
    fn nonfinite_projective_and_derived_overflow_are_rejected() {
        let mut matrix = WorldTransform::identity().matrix;
        matrix[3] = 0.01;
        assert!(WorldTransform::from_stored(matrix).is_err());
        matrix[3] = 0.;
        matrix[0] = f64::INFINITY;
        assert!(WorldTransform::from_stored(matrix).is_err());
        matrix[0] = f64::MAX;
        let transform = WorldTransform::from_stored(matrix).unwrap();
        assert!(transform.transform_point([2., 0., 0.]).is_err());
        assert!(transform.then(&transform).is_err());
    }

    fn document() -> Document {
        let mut document =
            crate::read_bytes(&fixture::fixture(15), ParseOptions::default(), false).unwrap();
        // The legacy storage fixture intentionally had an identity cached matrix
        // alongside a translated insertion. Qualify this world test's own copy.
        for entity in &mut document.entities {
            if let Geometry::Insert(insert) = &mut entity.geometry {
                insert.matrix_stored[12] = insert.insertion[0];
                insert.matrix_stored[13] = insert.insertion[1];
            }
        }
        document
    }

    #[test]
    fn repeated_block_instances_resolve_byblock_per_instance_and_keep_text_transform() {
        let mut doc = document();
        doc.entities
            .iter_mut()
            .find(|e| e.id == 2)
            .unwrap()
            .attributes
            .color_flag_raw = 1;
        let first = doc.entities.iter_mut().find(|e| e.id == 6).unwrap();
        first.attributes.color_flag_raw = 2;
        first.attributes.color_raw = 0x112233;
        let mut second = first.clone();
        second.id = 7;
        second.attributes.color_raw = 0x445566;
        let Geometry::Insert(insert) = &mut second.geometry else {
            panic!("INSERT expected")
        };
        insert.insertion[0] = 40.;
        insert.matrix_stored[12] = 40.;
        doc.entities.push(second);
        let scene = expand_world(&doc, -7, ParseOptions::default()).unwrap();
        let circles: Vec<_> = scene
            .instances
            .iter()
            .filter(|i| i.entity_id == 2)
            .collect();
        assert_eq!(circles.len(), 2);
        assert_eq!(circles[0].insert_path, vec![6]);
        assert_eq!(circles[1].insert_path, vec![7]);
        assert_eq!(circles[0].style.color_raw.value, Some(0x112233));
        assert_eq!(circles[1].style.color_raw.value, Some(0x445566));
        let Some(WorldGeometry::Curve(WorldCurve::Circle { center, .. })) = &circles[0].geometry
        else {
            panic!("circle expected")
        };
        assert_eq!(*center, [25., 36., 0.]);
        let text = scene.instances.iter().find(|i| i.entity_id == 5).unwrap();
        assert_eq!(text.transform, WorldTransform::identity());
        assert!(text.geometry.is_none());
        assert!(scene.transforms_complete);
        assert!(!scene.geometry_complete);
        assert!(!scene.render_complete);
    }

    #[test]
    fn native_zero_layer_replaces_layer_for_type_and_width_but_preserves_byblock_color() {
        let mut doc = document();
        let mut zero = doc.styles.layers[0].clone();
        zero.id = 900;
        zero.status_raw |= 0x8000_0000;
        zero.linetype_id = 99;
        zero.width_flag = 2;
        zero.width = 99.;
        doc.styles.layers.push(zero);
        let actual_layer = doc.styles.layers[0].clone();
        let child = doc.entities.iter_mut().find(|e| e.id == 2).unwrap();
        child.attributes.layer_id_raw = 900;
        child.attributes.linetype_flag_raw = 1;
        child.attributes.line_width_flag_raw = 1;
        child.attributes.color_flag_raw = 1;
        let parent = doc.entities.iter_mut().find(|e| e.id == 6).unwrap();
        parent.attributes.linetype_flag_raw = 2;
        parent.attributes.linetype_id_raw = 123;
        parent.attributes.line_width_flag_raw = 2;
        parent.attributes.line_width_raw = 5.;
        parent.attributes.color_flag_raw = 2;
        parent.attributes.color_raw = 0x123456;
        let scene = expand_world(&doc, -7, ParseOptions::default()).unwrap();
        let child = scene.instances.iter().find(|i| i.entity_id == 2).unwrap();
        assert_eq!(child.style.layer_id, actual_layer.id);
        assert_eq!(child.style.linetype.value, Some(actual_layer.linetype_id));
        assert_eq!(child.style.linetype.source, "zero_layer");
        assert_eq!(child.style.line_width.value, Some(actual_layer.width));
        assert_eq!(child.style.line_width.source, "zero_layer");
        assert_eq!(child.style.color_raw.value, Some(0x123456));
        assert_eq!(child.style.color_raw.source, "by_block");
    }

    #[test]
    fn unknown_hide_flags_do_not_produce_visible_instances() {
        let mut doc = document();
        doc.entities
            .iter_mut()
            .find(|e| e.id == 1)
            .unwrap()
            .attributes
            .hide_flag_raw = 1;
        let Geometry::Insert(insert) = &mut doc
            .entities
            .iter_mut()
            .find(|e| e.id == 6)
            .unwrap()
            .geometry
        else {
            panic!("INSERT expected")
        };
        insert.hide_flag_raw = 1;
        let scene = expand_world(&doc, -7, ParseOptions::default()).unwrap();
        assert!(
            !scene
                .instances
                .iter()
                .find(|i| i.entity_id == 1)
                .unwrap()
                .visible
        );
        assert!(
            !scene
                .instances
                .iter()
                .find(|i| i.entity_id == 2)
                .unwrap()
                .visible
        );
        assert!(!scene.styles_complete);
        assert_eq!(
            scene
                .diagnostics
                .iter()
                .filter(|d| d.code == "unsupported_world_visibility")
                .count(),
            2
        );
    }

    #[test]
    fn graph_cycles_missing_blocks_and_expansion_limits_are_not_silently_ignored() {
        let doc = document();
        assert!(matches!(
            expand_world(
                &doc,
                -7,
                ParseOptions {
                    max_records: 0,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
        assert!(matches!(
            expand_world(
                &doc,
                -7,
                ParseOptions {
                    max_record_depth: 0,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit {
                name: "max_record_depth",
                ..
            })
        ));
        let mut cycle = doc.clone();
        let Geometry::Insert(insert) = &mut cycle
            .entities
            .iter_mut()
            .find(|e| e.id == 6)
            .unwrap()
            .geometry
        else {
            panic!("INSERT expected")
        };
        insert.block_id = -5;
        let scene = expand_world(&cycle, -7, ParseOptions::default()).unwrap();
        assert!(!scene.transforms_complete);
        assert_eq!(
            scene
                .diagnostics
                .iter()
                .filter(|d| d.code == "cyclic_block_reference")
                .count(),
            1
        );
        let Geometry::Insert(insert) = &mut cycle
            .entities
            .iter_mut()
            .find(|e| e.id == 6)
            .unwrap()
            .geometry
        else {
            panic!("INSERT expected")
        };
        insert.block_id = 999;
        let scene = expand_world(&cycle, -7, ParseOptions::default()).unwrap();
        assert!(scene
            .diagnostics
            .iter()
            .any(|d| d.code == "unresolved_block"));
    }

    #[test]
    fn contradictory_insert_components_are_refused() {
        let mut doc = document();
        let Geometry::Insert(insert) = &mut doc
            .entities
            .iter_mut()
            .find(|e| e.id == 6)
            .unwrap()
            .geometry
        else {
            panic!("INSERT expected")
        };
        assert!(WorldTransform::from_insert(insert).is_ok());
        insert.matrix_stored[12] += 1.;
        assert!(WorldTransform::from_insert(insert)
            .unwrap_err()
            .contains("disagrees"));
        insert.matrix_stored[12] -= 1.;
        insert.insertion[2] = 1.;
        assert!(WorldTransform::from_insert(insert).is_err());
    }

    #[test]
    fn negative_bulge_with_nonuniform_scale_retains_endpoints_and_stored_widths() {
        let span = crate::SourceSpan {
            offset: 0,
            length: 0,
        };
        let vertex = |point, bulge| crate::PolylineVertex {
            point,
            bulge,
            legacy_z: None,
            start_width: 0.2,
            end_width: 0.4,
            span: span.clone(),
        };
        let polyline = PolylineGeometry {
            vertices: vec![vertex([0., 0.], -1.), vertex([2., 0.], 0.)],
            closed: false,
            closed_raw: 0,
            normal: Some([0., 0., 1.]),
            span,
        };
        let mut matrix = WorldTransform::identity();
        matrix.matrix[0] = 2.;
        let result = transform_polyline(&polyline, &matrix).unwrap();
        assert_eq!(result.segments.len(), 1);
        let WorldCurve::EllipticCurve {
            center,
            start,
            end,
            axis_v,
            ..
        } = result.segments[0].curve
        else {
            panic!("ellipse expected")
        };
        assert_eq!(center, [2., 0., 0.]);
        assert_eq!(start, [0., 0., 0.]);
        assert!((end[0] - 4.).abs() < 1e-14 && end[1].abs() < 1e-14);
        assert!(axis_v[1] > 0.);
        assert_eq!(result.segments[0].start_width_stored, 0.2);
    }
}

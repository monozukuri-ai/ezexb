//! Native attribute inheritance, retaining device-dependent width selections.
//! A symbolic ByBlock value is valid stored data; instances supply its context.
use serde::Serialize;

use crate::{EntityAttributes, Layer, ResolvedStyle, StyleValue};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AttributeDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StyleResolution {
    pub style: ResolvedStyle,
    pub diagnostics: Vec<AttributeDiagnostic>,
}

fn value<T>(source: &str, value: Option<T>) -> StyleValue<T> {
    StyleValue {
        source: source.into(),
        value,
    }
}

fn inherited<T: Copy>(context: Option<&StyleValue<T>>) -> StyleValue<T> {
    match context {
        Some(context) => {
            let source = context
                .source
                .strip_prefix("by_layer_")
                .or_else(|| context.source.strip_prefix("by_block_"))
                .or_else(|| context.source.strip_prefix("zero_layer_"))
                .unwrap_or(&context.source);
            let source = if matches!(source, "thin" | "wide" | "mid" | "double_wide" | "device") {
                format!("by_block_{source}")
            } else {
                "by_block".into()
            };
            StyleValue {
                source,
                value: context.value,
            }
        }
        None => value("by_block", None),
    }
}

fn ordinary<T: Copy>(
    flag: u8,
    layer: Option<T>,
    stored: T,
    block: Option<&StyleValue<T>>,
) -> StyleValue<T> {
    match flag {
        0 => value("by_layer", layer),
        1 => inherited(block),
        2 => value("explicit", Some(stored)),
        _ => value("unsupported", None),
    }
}

fn width(flag: u32, stored: f64) -> StyleValue<f64> {
    match flag {
        2 => value("explicit", Some(stored)),
        3 => value("thin", None),
        4 => value("wide", None),
        5 => value("mid", None),
        6 => value("double_wide", None),
        // The native draw helper leaves the existing device width unchanged.
        0 | 1 | 7 => value("device", None),
        _ => value("unsupported", None),
    }
}

/// Resolve native stored attributes using an already-indexed layer and optional
/// containing INSERT style. No screen/device defaults or physical units are guessed.
/// Known symbolic selections are retained without a stored-data diagnostic.
pub fn resolve_entity_style(
    attributes: &EntityAttributes,
    layer: Option<&Layer>,
    block_style: Option<&ResolvedStyle>,
) -> StyleResolution {
    let mut diagnostics = Vec::new();
    let mut issue = |code: &str, message: String| {
        diagnostics.push(AttributeDiagnostic {
            code: code.into(),
            message,
        })
    };
    if layer.is_none() {
        issue(
            "unresolved_layer",
            format!("Layer {} is unavailable.", attributes.layer_id_raw),
        );
    }
    let linetype = ordinary(
        attributes.linetype_flag_raw,
        layer.map(|v| v.linetype_id),
        attributes.linetype_id_raw,
        block_style.map(|v| &v.linetype),
    );
    let color_raw = ordinary(
        attributes.color_flag_raw,
        layer.map(|v| v.color_raw),
        attributes.color_raw,
        block_style.map(|v| &v.color_raw),
    );
    let line_width = match attributes.line_width_flag_raw {
        0 => match layer {
            Some(layer) => {
                let mut result = width(layer.width_flag, layer.width);
                result.source = if result.source == "explicit" {
                    "by_layer".into()
                } else {
                    format!("by_layer_{}", result.source)
                };
                result
            }
            None => value("by_layer", None),
        },
        1 => inherited(block_style.map(|v| &v.line_width)),
        flag => width(u32::from(flag), attributes.line_width_raw),
    };
    for (name, result) in [
        ("linetype", linetype.source.as_str()),
        ("color", color_raw.source.as_str()),
        ("line width", line_width.source.as_str()),
    ] {
        if result.ends_with("unsupported") {
            issue(
                "unsupported_attribute_inheritance",
                format!("Unsupported {name} selection flag."),
            );
        }
    }
    StyleResolution {
        style: ResolvedStyle {
            layer_id: layer.map(|v| v.id).unwrap_or(attributes.layer_id_raw),
            layer_name: layer.map(|v| v.name.value.clone()),
            linetype,
            color_raw,
            line_width,
            linetype_scale: attributes.linetype_scale,
        },
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceSpan, TextField};
    fn attributes() -> EntityAttributes {
        EntityAttributes {
            span: SourceSpan {
                offset: 0,
                length: 33,
            },
            layer_status_raw: 0,
            layer_id_raw: 1,
            linetype_status_raw: 0,
            linetype_id_raw: 9,
            color_raw: 0x123456,
            line_width_raw: 0.7,
            linetype_flag_raw: 2,
            line_width_flag_raw: 2,
            color_flag_raw: 2,
            hide_flag_raw: 0,
            linetype_scale: 1.25,
            extension_raw: None,
        }
    }
    fn layer() -> Layer {
        let text = TextField {
            value: "A".into(),
            span: SourceSpan {
                offset: 0,
                length: 0,
            },
        };
        Layer {
            id: 1,
            owner: 0,
            span: text.span.clone(),
            payload_span: text.span.clone(),
            name: text.clone(),
            alternate_name: text.clone(),
            key_name: text.clone(),
            description: text,
            color_raw: 0xabcdef,
            status_raw: 0,
            width: 0.25,
            width_flag: 2,
            linetype_id: 3,
            linetype_status: 0,
            on: true,
            locked: false,
            printable: true,
            frozen: false,
            unknown_fields: vec![],
        }
    }
    #[test]
    fn explicit_layer_and_block_are_distinct_and_instance_scoped() {
        let mut attributes = attributes();
        let layer = layer();
        let explicit = resolve_entity_style(&attributes, Some(&layer), None);
        assert!(explicit.diagnostics.is_empty());
        assert_eq!(explicit.style.color_raw.value, Some(0x123456));
        attributes.color_flag_raw = 0;
        attributes.line_width_flag_raw = 0;
        let by_layer = resolve_entity_style(&attributes, Some(&layer), None);
        assert_eq!(by_layer.style.color_raw.value, Some(0xabcdef));
        assert_eq!(by_layer.style.line_width.value, Some(0.25));
        attributes.color_flag_raw = 1;
        attributes.line_width_flag_raw = 1;
        attributes.linetype_flag_raw = 1;
        let symbolic = resolve_entity_style(&attributes, Some(&layer), None);
        assert!(symbolic.diagnostics.is_empty());
        assert_eq!(symbolic.style.color_raw.source, "by_block");
        assert!(symbolic.style.color_raw.value.is_none());
        let first = resolve_entity_style(&attributes, Some(&layer), Some(&explicit.style));
        let second = resolve_entity_style(&attributes, Some(&layer), Some(&by_layer.style));
        assert_eq!(first.style.color_raw.value, Some(0x123456));
        assert_eq!(second.style.color_raw.value, Some(0xabcdef));
    }
    #[test]
    fn device_width_modes_never_use_the_stored_float_as_a_resolved_width() {
        let mut attributes = attributes();
        let mut layer = layer();
        for (flag, name) in [
            (3, "thin"),
            (4, "wide"),
            (5, "mid"),
            (6, "double_wide"),
            (7, "device"),
        ] {
            attributes.line_width_flag_raw = flag;
            let direct = resolve_entity_style(&attributes, Some(&layer), None);
            assert!(direct.diagnostics.is_empty());
            assert_eq!(direct.style.line_width.source, name);
            assert!(direct.style.line_width.value.is_none());
            attributes.line_width_flag_raw = 1;
            let nested = resolve_entity_style(&attributes, Some(&layer), Some(&direct.style));
            assert_eq!(nested.style.line_width.source, format!("by_block_{name}"));
            layer.width_flag = u32::from(flag);
            attributes.line_width_flag_raw = 0;
            let inherited = resolve_entity_style(&attributes, Some(&layer), None);
            assert_eq!(
                inherited.style.line_width.source,
                format!("by_layer_{name}")
            );
            assert!(inherited.style.line_width.value.is_none());
        }
    }
    #[test]
    fn layer_width_zero_and_one_retain_current_device_selection() {
        let mut attributes = attributes();
        attributes.line_width_flag_raw = 0;
        let mut layer = layer();
        for flag in [0, 1] {
            layer.width_flag = flag;
            let result = resolve_entity_style(&attributes, Some(&layer), None);
            assert!(result.diagnostics.is_empty());
            assert_eq!(result.style.line_width.source, "by_layer_device");
            assert_eq!(result.style.line_width.value, None);
        }
        let mut block = resolve_entity_style(&attributes, Some(&layer), None).style;
        block.line_width.source = "zero_layer_thin".into();
        attributes.line_width_flag_raw = 1;
        let result = resolve_entity_style(&attributes, Some(&layer), Some(&block));
        assert_eq!(result.style.line_width.source, "by_block_thin");
        assert_eq!(result.style.line_width.value, None);
    }

    #[test]
    fn unknown_flags_and_missing_layers_remain_diagnostic() {
        let mut attributes = attributes();
        attributes.linetype_flag_raw = 99;
        attributes.line_width_flag_raw = 99;
        attributes.color_flag_raw = 99;
        let result = resolve_entity_style(&attributes, None, None);
        assert_eq!(result.diagnostics.len(), 4);
        assert!(result.style.linetype.value.is_none());
        assert!(result.style.color_raw.value.is_none());
        assert!(result.style.line_width.value.is_none());
    }
}

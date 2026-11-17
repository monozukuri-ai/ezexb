use crate::test_fixture as fixture;
use crate::*;
const BLOCK: &str = "/*BlockStg/*GBlock";

fn change_record(
    bytes: &[u8],
    id: i32,
    change: impl FnOnce(&mut Vec<u8>, &RecordHeader),
) -> Vec<u8> {
    fixture::mutate_stream(bytes, BLOCK, |data| {
        let record = inspect_record_headers(data, ParseOptions::default())
            .unwrap()
            .records
            .into_iter()
            .find(|r| r.id_raw == id)
            .unwrap();
        change(data, &record);
    })
}
#[test]
fn complete_profile_preserves_stored_geometry_and_membership_for_all_builds() {
    for major in [10, 11, 15, 16, 18] {
        let doc = read_bytes(&fixture::fixture(major), ParseOptions::default(), true).unwrap();
        assert!(doc.report.complete);
        assert_eq!(doc.entities.len(), 6);
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.entities_in_space(-7).unwrap().count(), 5);
        assert_eq!(doc.entities_in_block(-60).count(), 1);
        assert!(doc.units.is_none());
        assert_eq!(
            doc.entities[0].style.line_width.value,
            Some(f64::from(0.35f32))
        );
        assert_eq!(doc.entities[0].style.color_raw.value, Some(0x123456));
        assert_eq!(
            doc.entities[0].attributes.extension_raw,
            if major == 18 { Some(0) } else { None }
        );
        assert!(matches!(
            doc.entities[0].geometry,
            Geometry::Primitive(PrimitiveGeometry::Line {
                start: [1., 2., 0.],
                end: [4., 6., 0.]
            })
        ));
        let Geometry::Insert(insert) = &doc.entities[5].geometry else {
            panic!("insert expected")
        };
        assert_eq!(insert.block_id, -60);
        assert_eq!(insert.insertion, [20., 30., 0.]);
    }
}
fn with_build(base: &[u8], from: &str, to: &str) -> Vec<u8> {
    let utf16 = |s: &str| {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()
    };
    let (needle, replacement) = (utf16(from), utf16(to));
    assert_eq!(needle.len(), replacement.len());
    fixture::mutate_stream(base, "/*FileInfoStream", |data| {
        let at = data
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap();
        data[at..at + needle.len()].copy_from_slice(&replacement);
    })
}
#[test]
fn unverified_builds_of_a_qualified_major_read_with_a_diagnostic() {
    let base = fixture::fixture(16);
    let verified = read_bytes(&base, ParseOptions::default(), true).unwrap();
    let field = with_build(&base, "16.4.0.13104", "16.4.0.12833");
    let doc = read_bytes(&field, ParseOptions::default(), false).unwrap();
    assert_eq!(doc.entities.len(), verified.entities.len());
    assert_eq!(doc.blocks.len(), verified.blocks.len());
    assert_eq!(doc.report.diagnostics.len(), 1);
    let diagnostic = &doc.report.diagnostics[0];
    assert_eq!(diagnostic.code, "unverified_product_build");
    assert_eq!(diagnostic.stream_path.as_deref(), Some("/*FileInfoStream"));
    assert!(
        diagnostic.message.contains("16.4.0.12833") && diagnostic.message.contains("16.4.0.13104")
    );
    assert!(!doc.report.complete);
    assert!(matches!(
        read_bytes(&field, ParseOptions::default(), true),
        Err(Error::IncompleteRead(_))
    ));
    // Majors without any sample, and builds that are not dotted numbers, stay unsupported
    for build in ["20.0.0.13104", "27.0.0.21423", "9.4.0.13104 "] {
        assert!(
            matches!(
                read_bytes(
                    &with_build(&base, "16.4.0.13104", build),
                    ParseOptions::default(),
                    false
                ),
                Err(Error::UnsupportedFeature(_))
            ),
            "{build}"
        );
    }
    // A build of a major without its own sample uses the neighbouring rules (12 reads
    // like 11; the fixture of major 11 is byte-compatible with those rules)
    let doc = read_bytes(
        &with_build(&fixture::fixture(11), "11.7.0.5203", "12.5.0.5302"),
        ParseOptions::default(),
        false,
    )
    .unwrap();
    assert_eq!(doc.entities.len(), verified.entities.len());
    let message = &doc.report.diagnostics[0].message;
    assert!(message.contains("major 12") && message.contains("11.7.0.5203"));
    assert!(message.contains("15.0.0.5681"));
    // Major 19 reads with the rules of 18 (its nearest sample); a platform note
    // after the build does not change the major (same length as the needle)
    let newer = fixture::fixture(18);
    let verified = read_bytes(&newer, ParseOptions::default(), true).unwrap();
    for build in ["19.0.0.20385", "18.0.0 (x64)"] {
        let doc = read_bytes(
            &with_build(&newer, "18.0.0.18159", build),
            ParseOptions::default(),
            false,
        )
        .unwrap();
        assert_eq!(doc.entities.len(), verified.entities.len(), "{build}");
        let message = &doc.report.diagnostics[0].message;
        assert!(
            message.contains(build) && message.contains("18.0.0.18159"),
            "{message}"
        );
    }
}
#[test]
fn unsupported_records_are_explicit_and_strict_error_retains_report() {
    let data = fixture::mutate_stream(&fixture::fixture(10), BLOCK, |data| {
        data.extend(fixture::record(
            99,
            -5,
            "11111111-1111-1111-1111-111111111111",
            1,
            &[9; 7],
        ))
    });
    let partial = read_bytes(&data, ParseOptions::default(), false).unwrap();
    assert_eq!(partial.entities.len(), 6);
    assert_eq!(partial.report.unsupported_entity_count, 1);
    let diagnostic = partial
        .report
        .diagnostics
        .iter()
        .find(|d| d.code == "unsupported_entity")
        .unwrap();
    assert_eq!(diagnostic.record_id, Some(99));
    assert_eq!(diagnostic.span.as_ref().unwrap().length, 37);
    let Error::IncompleteRead(report) =
        read_bytes(&data, ParseOptions::default(), true).unwrap_err()
    else {
        panic!("incomplete expected")
    };
    assert_eq!(report.decoded_entity_count, 6);
    assert!(!report.complete);
}
#[test]
fn dangling_references_unknown_flags_duplicate_ids_and_cycles_fail_closed() {
    for (id, offset, value, code) in [
        (1, 4, 999i32, "unresolved_layer"),
        (6, 225, 999, "unresolved_insert_block"),
        (6, 225, -5, "cyclic_block_reference"),
    ] {
        let data = change_record(&fixture::fixture(10), id, |data, r| {
            let p = r.payload_span.offset as usize + offset;
            data[p..p + 4].copy_from_slice(&value.to_le_bytes());
        });
        let doc = read_bytes(&data, ParseOptions::default(), false).unwrap();
        assert!(
            doc.report.diagnostics.iter().any(|d| d.code == code),
            "{code}"
        );
        assert!(matches!(
            read_bytes(&data, ParseOptions::default(), true),
            Err(Error::IncompleteRead(_))
        ));
    }
    let data = change_record(&fixture::fixture(10), 1, |data, r| {
        data[r.payload_span.offset as usize + 24] = 255
    });
    assert!(read_bytes(&data, ParseOptions::default(), false)
        .unwrap()
        .report
        .diagnostics
        .iter()
        .any(|d| d.code == "unsupported_attribute_inheritance"));
    let data = change_record(&fixture::fixture(10), 1, |data, r| {
        let copied =
            data[r.span.offset as usize..(r.span.offset + r.span.length) as usize].to_vec();
        data.extend(copied);
    });
    assert!(read_bytes(&data, ParseOptions::default(), false)
        .unwrap()
        .report
        .diagnostics
        .iter()
        .any(|d| d.code == "duplicate_entity_id"));
}
#[test]
fn document_limits_are_cumulative_and_partial_mode_never_bypasses_them() {
    let data = fixture::fixture(18);
    let report = read_bytes(&data, ParseOptions::default(), true)
        .unwrap()
        .report;
    for strict in [false, true] {
        let exact = ParseOptions {
            max_records: report.budget_used,
            max_total_decoded_bytes: report.decoded_bytes,
            ..ParseOptions::default()
        };
        assert!(read_bytes(&data, exact, strict).is_ok());
        for options in [
            ParseOptions {
                max_records: report.budget_used - 1,
                ..exact
            },
            ParseOptions {
                max_total_decoded_bytes: report.decoded_bytes - 1,
                ..exact
            },
        ] {
            assert!(matches!(
                read_bytes(&data, options, strict),
                Err(Error::ResourceLimit { .. })
            ));
        }
    }
}
#[test]
fn unknown_build_and_truncated_known_fields_are_not_partial_success() {
    let data = fixture::mutate_stream(&fixture::fixture(10), "/*FileInfoStream", |data| {
        let needle = "10.0.0.0000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let p = data
            .windows(needle.len())
            .position(|x| x == needle)
            .unwrap();
        data[p] = b'9';
    });
    assert!(matches!(
        read_bytes(&data, ParseOptions::default(), false),
        Err(Error::UnsupportedFeature(_))
    ));
    let data = fixture::mutate_stream(&fixture::fixture(10), BLOCK, |data| {
        data.pop();
    });
    assert!(matches!(
        read_bytes(&data, ParseOptions::default(), false),
        Err(Error::InvalidStream(_))
    ));
}

use crate::remaining_test_fixture as remaining_fixture;

#[test]
fn view_references_are_checked_without_expanding_clipped_blocks() {
    let bytes = remaining_fixture::fixture(10);
    let doc = read_bytes(&bytes, ParseOptions::default(), false).unwrap();
    assert_eq!(doc.report.unsupported_entity_count, 0);
    assert!(!doc.report.complete);
    let scene = expand_world(&doc, -7, ParseOptions::default()).unwrap();
    assert!(!scene.transforms_complete);
    assert!(scene
        .instances
        .iter()
        .all(|i| !i.insert_path.contains(&150)));
    for id in [140, 150] {
        assert!(scene
            .instances
            .iter()
            .find(|i| i.entity_id == id)
            .unwrap()
            .geometry
            .is_none());
    }
    for (target, code) in [
        (999i32, "unresolved_view_block"),
        (-5, "cyclic_block_reference"),
    ] {
        let view_source = &doc.entities.iter().find(|e| e.id == 150).unwrap().source;
        let bad = fixture::mutate_stream(&bytes, BLOCK, |data| {
            let p = view_source.payload_span.offset as usize + 221;
            data[p..p + 4].copy_from_slice(&target.to_le_bytes());
        });
        let partial = read_bytes(&bad, ParseOptions::default(), false).unwrap();
        assert!(partial.report.diagnostics.iter().any(|d| d.code == code));
        assert!(matches!(
            read_bytes(&bad, ParseOptions::default(), true),
            Err(Error::IncompleteRead(_))
        ));
    }
}

#[test]
fn bom_presentation_visibility_and_table_placement_are_not_applied_twice() {
    let options = ParseOptions::default();
    let mut doc = read_bytes(&remaining_fixture::fixture(10), options, false).unwrap();
    let baseline = render_svg(
        &read_bytes(&fixture::fixture(10), options, true).unwrap(),
        -7,
        options,
    )
    .unwrap();
    for (flag, extra) in [(0, 2), (u16::MAX, 1), (17, 1)] {
        let Geometry::BomTable(bom) = &mut doc
            .entities
            .iter_mut()
            .find(|e| e.id == 160)
            .unwrap()
            .geometry
        else {
            panic!()
        };
        bom.items[0].attributes.hide_flag_raw = flag;
        let svg = render_svg(&doc, -7, options).unwrap();
        assert_eq!(
            svg.rendered_curve_count,
            baseline.rendered_curve_count + extra
        );
        assert_eq!(
            svg.rendered_text_count,
            baseline.rendered_text_count + extra
        );
        assert_eq!(svg.svg.matches("Part Ω").count(), extra as usize);
        if flag == 17 {
            assert!(svg
                .diagnostics
                .iter()
                .any(|d| d.contains("BOM item") && d.contains("visibility unknown")));
        }
    }
    let mut table_doc = read_bytes(&remaining_fixture::fixture(18), options, false).unwrap();
    let before = render_svg(&table_doc, -7, options).unwrap();
    for entity in &mut table_doc.entities {
        if let Geometry::Table(table) = &mut entity.geometry {
            table.location = [1000., 2000., 0.];
            table.rotation = 0.75;
            table.scale = 7.;
        }
    }
    let after = render_svg(&table_doc, -7, options).unwrap();
    assert_eq!(before.svg, after.svg); // Saved presentations are already placed.
}

use crate::remaining_test_fixture::display;

fn entity(doc: &Document, id: i32) -> &Entity {
    doc.entities
        .iter()
        .find(|entity| entity.id == id)
        .unwrap_or_else(|| panic!("entity {id}"))
}
fn codes_for(doc: &Document, id: i32) -> Vec<&str> {
    doc.report
        .diagnostics
        .iter()
        .filter(|d| d.record_id == Some(id))
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn records_with_cached_display_are_entities_and_the_rest_stays_unsupported() {
    for major in [10, 16, 18] {
        let bytes = display::fixture(major);
        let doc = read_bytes(&bytes, ParseOptions::default(), false).unwrap();
        let base = read_bytes(&fixture::fixture(major), ParseOptions::default(), true).unwrap();
        let added = if major > 10 { 11 } else { 8 };
        assert_eq!(doc.entities.len(), base.entities.len() + added, "{major}");
        assert_eq!(doc.report.decoded_entity_count, doc.entities.len() as u64);
        // Only the record without any display records is counted as unsupported.
        assert_eq!(doc.report.unsupported_entity_count, 1);
        let unsupported: Vec<_> = doc
            .report
            .diagnostics
            .iter()
            .filter(|d| d.code == "unsupported_entity")
            .collect();
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].record_id, Some(display::BARE_UNKNOWN));
        assert_eq!(
            unsupported[0].type_id.as_deref(),
            Some(display::UNKNOWN_BARE)
        );
        assert!(doc.entities.iter().all(|e| e.id != display::BARE_UNKNOWN));
        // The entities read before stay exactly as they were.
        for (before, after) in base.entities.iter().zip(&doc.entities) {
            assert_eq!(
                serde_json::to_value(before).unwrap(),
                serde_json::to_value(after).unwrap()
            );
        }

        // An unknown class is kept through the display records that open its body.
        let unknown = entity(&doc, display::UNKNOWN_ENTITY);
        assert_eq!(unknown.type_id, display::UNKNOWN_CLASS);
        assert_eq!(unknown.block_id, Some(-5));
        assert_eq!(unknown.attributes.layer_id_raw, -24);
        assert_eq!(unknown.style.layer_name.as_deref(), Some("Test layer"));
        let Geometry::CachedDisplay(shown) = &unknown.geometry else {
            panic!("cached display expected")
        };
        assert_eq!(shown.source, CachedDisplaySource::Body);
        assert_eq!(shown.presentation_records.len(), 3);
        assert_eq!(shown.unresolved_semantics_span.length, 11);
        assert!(matches!(
            shown.presentation_records[0].geometry,
            Some(PresentationGeometry::Primitive(PrimitiveGeometry::Line {
                start: [80., 0., 0.],
                end: [90., 0., 0.]
            }))
        ));
        let Some(PresentationGeometry::Annotation(AnnotationGeometry::Text(text))) =
            &shown.presentation_records[1].geometry
        else {
            panic!("cached text expected")
        };
        assert_eq!(text.contents, "Original Ω");
        let item = &shown.presentation_records[2];
        assert_eq!(item.header.type_id, display::UNKNOWN_ITEM);
        assert_eq!(item.attributes.as_ref().unwrap().hide_flag_raw, u16::MAX);
        let Some(PresentationGeometry::CachedDisplay(nested)) = &item.geometry else {
            panic!("nested cached display expected")
        };
        assert_eq!(nested.presentation_records.len(), 2);
        assert_eq!(nested.unresolved_semantics_span.length, 4);
        assert_eq!(
            codes_for(&doc, display::UNKNOWN_ENTITY),
            ["unsupported_entity_semantics"]
        );
        let note = doc
            .report
            .diagnostics
            .iter()
            .find(|d| d.record_id == Some(display::UNKNOWN_ENTITY))
            .unwrap();
        assert!(note.message.contains("Unsupported entity type"));
        assert_eq!(note.span.as_ref(), Some(&shown.unresolved_semantics_span));

        // A class that keeps its attributes in the schema-3 extension is framed,
        // and shown through the block reference in its display cache.
        let array = entity(&doc, display::ARRAY);
        assert_eq!(array.object_schema, 3);
        assert_eq!(array.attributes.layer_id_raw, -24);
        let Geometry::CachedDisplay(shown) = &array.geometry else {
            panic!("cached display expected")
        };
        assert_eq!(shown.source, CachedDisplaySource::DisplayCache);
        let Some(PresentationGeometry::Insert(insert)) = &shown.presentation_records[0].geometry
        else {
            panic!("cached block reference expected")
        };
        assert_eq!((insert.block_id, insert.insertion), (-60, [40., 50., 0.]));
        assert_eq!(
            codes_for(&doc, display::ARRAY),
            ["unsupported_entity_semantics"]
        );
        // Records behind it are framed as well: nothing is left unparsed.
        assert!(doc
            .report
            .diagnostics
            .iter()
            .all(|d| d.code != "unframed_records"));

        // Block arrowheads of a dimension are cached block references.
        let Geometry::Complex(ComplexGeometry::Dimension(dimension)) =
            &entity(&doc, display::BLOCK_ARROW_DIMENSION).geometry
        else {
            panic!("dimension expected")
        };
        assert_eq!(dimension.value_stored, 10.);
        assert!(matches!(
            dimension.presentation_records[1].geometry,
            Some(PresentationGeometry::Insert(_))
        ));
        assert_eq!(
            codes_for(&doc, display::BLOCK_ARROW_DIMENSION),
            ["unsupported_dimension_semantics"]
        );
        let Geometry::Complex(ComplexGeometry::Dimension(angle)) =
            &entity(&doc, display::THREE_POINT_ANGLE_ID).geometry
        else {
            panic!("three-point angle dimension expected")
        };
        assert_eq!(angle.value_stored, std::f64::consts::FRAC_PI_4);
        assert_eq!(angle.presentation_records.len(), 2);
        assert_eq!(angle.unresolved_semantics_span.length, 24);

        // Circular primitives with a non-orthonormal stored basis.
        let Geometry::Primitive(PrimitiveGeometry::Arc {
            radius,
            x_axis,
            y_axis,
            start,
            end,
            stored_axes,
            ..
        }) = &entity(&doc, display::SCALED_ARC).geometry
        else {
            panic!("arc expected")
        };
        assert_eq!(
            (*radius, *x_axis, *y_axis),
            (5., [1., 0., 0.], [0., 1., 0.])
        );
        assert_eq!(*start, [35., 40., 0.]);
        assert!((end[0] - 30.).abs() < 1e-12 && (end[1] - 45.).abs() < 1e-12);
        assert_eq!(*stored_axes, Some([[2., 0., 0.], [0., 2., 0.]]));
        assert_eq!(
            codes_for(&doc, display::SCALED_ARC),
            ["nonorthonormal_circular_basis"]
        );
        assert!(matches!(
            entity(&doc, display::SKEWED_CIRCLE).geometry,
            Geometry::Primitive(PrimitiveGeometry::Circle {
                radius: 4.0,
                x_axis: [1., 0., 0.],
                y_axis: [0., 1., 0.],
                stored_axes: Some(_),
                ..
            })
        ));
        assert_eq!(
            codes_for(&doc, display::SKEWED_CIRCLE),
            ["nonorthonormal_circular_basis"]
        );

        // A construction line and a line with residue in Z are plain entities.
        assert!(matches!(
            entity(&doc, display::XLINE_ID).geometry,
            Geometry::Primitive(PrimitiveGeometry::Xline {
                point: [0., 25., 0.],
                direction: [1., 0., 0.]
            })
        ));
        assert!(codes_for(&doc, display::XLINE_ID).is_empty());
        assert!(matches!(
            entity(&doc, display::RESIDUE_LINE).geometry,
            Geometry::Primitive(PrimitiveGeometry::Line { .. })
        ));
        assert!(codes_for(&doc, display::RESIDUE_LINE).is_empty());

        if major > 10 {
            let Geometry::Annotation(AnnotationGeometry::Polyline(polyline)) =
                &entity(&doc, display::ZERO_NORMAL_POLYLINE).geometry
            else {
                panic!("polyline expected")
            };
            assert_eq!(polyline.normal, Some([0.; 3]));
            assert_eq!(polyline.vertices[2].point, [10., 60.]);
            assert_eq!(
                codes_for(&doc, display::ZERO_NORMAL_POLYLINE),
                ["degenerate_polyline_normal"]
            );
            // The symbol's fields are not qualified for this save version.
            let roughness = entity(&doc, display::UNQUALIFIED_ROUGHNESS);
            let Geometry::CachedDisplay(shown) = &roughness.geometry else {
                panic!("cached display expected")
            };
            assert_eq!(shown.presentation_records.len(), 4);
            assert_eq!(shown.unresolved_semantics_span.length, 40);
            let note = doc
                .report
                .diagnostics
                .iter()
                .find(|d| d.record_id == Some(display::UNQUALIFIED_ROUGHNESS))
                .unwrap();
            assert_eq!(note.code, "unsupported_entity_semantics");
            assert!(note.message.contains("engineering-symbol save-version"));
            let Geometry::Complex(ComplexGeometry::Nurbs(nurbs)) =
                &entity(&doc, display::FITTED_NURBS).geometry
            else {
                panic!("NURBS expected")
            };
            assert_eq!(nurbs.fit_points.len(), 3);
            assert_eq!(nurbs.fit_points[1], [5., 75., 0.]);
            assert_eq!(nurbs.control_points.len(), 4);
            assert!(codes_for(&doc, display::FITTED_NURBS).is_empty());
        }

        // Strict reading still refuses the document and keeps the report.
        let Err(Error::IncompleteRead(report)) = read_bytes(&bytes, ParseOptions::default(), true)
        else {
            panic!("strict read must stay incomplete")
        };
        assert_eq!(report.unsupported_entity_count, 1);
        assert_eq!(report.diagnostics.len(), doc.report.diagnostics.len());

        // The world scene and the preview accept the new geometry kinds.
        let scene = expand_world(&doc, -7, ParseOptions::default()).unwrap();
        assert!(scene
            .instances
            .iter()
            .any(|i| i.entity_id == display::XLINE_ID && i.geometry.is_none()));
        let svg = render_svg(&doc, -7, ParseOptions::default()).unwrap();
        let plain = render_svg(&base, -7, ParseOptions::default()).unwrap();
        // The unknown entity's line, the array's block (one circle), the two block
        // arrowheads (one circle each), the dimensions' line, arc and line, the
        // scaled arc, the circle and the residue line; the hidden item is omitted.
        // From major 16: two polyline segments and the three roughness lines.
        let more = if major > 10 { 10 + 2 + 3 } else { 10 };
        assert_eq!(
            svg.rendered_curve_count,
            plain.rendered_curve_count + more,
            "{major}"
        );
    }
}

#[test]
fn cached_block_references_are_validated_like_stored_ones() {
    let bytes = fixture::mutate_stream(&fixture::fixture(16), BLOCK, |data| {
        data.extend(display::array_record(400, -5, 16, -999));
        data.extend(fixture::record(
            401,
            -60,
            "1da848b5-8c84-4c11-80d6-45cf9b26aa48",
            1,
            &display::block_arrow_dimension_body(16, -60),
        ));
    });
    let doc = read_bytes(&bytes, ParseOptions::default(), false).unwrap();
    let missing: Vec<_> = doc
        .report
        .diagnostics
        .iter()
        .filter(|d| d.code == "unresolved_insert_block")
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].record_id, Some(400));
    assert!(missing[0].message.contains("-999"));
    // A dimension inside block -60 whose arrowhead is block -60 itself.
    assert!(doc
        .report
        .diagnostics
        .iter()
        .any(|d| d.code == "cyclic_block_reference"));
    // The preview does not follow the cycle and does not fail on the missing block.
    let svg = render_svg(&doc, -7, ParseOptions::default()).unwrap();
    assert!(svg
        .diagnostics
        .iter()
        .any(|d| d.contains("unresolved_block") || d.contains("cyclic")));
}

#[test]
fn cached_display_lookup_is_charged_to_the_cumulative_budget() {
    let bytes = display::fixture(16);
    let complete = read_bytes(&bytes, ParseOptions::default(), false).unwrap();
    let used = complete.report.budget_used;
    let exact = ParseOptions {
        max_records: used,
        ..ParseOptions::default()
    };
    assert_eq!(
        read_bytes(&bytes, exact, false).unwrap().entities.len(),
        complete.entities.len()
    );
    for max_records in [used - 1, used / 2, 40, 0] {
        let options = ParseOptions {
            max_records,
            ..ParseOptions::default()
        };
        assert!(
            matches!(
                read_bytes(&bytes, options, false),
                Err(Error::ResourceLimit {
                    name: "max_records",
                    ..
                })
            ),
            "{max_records}"
        );
    }
}

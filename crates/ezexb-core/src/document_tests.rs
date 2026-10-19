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

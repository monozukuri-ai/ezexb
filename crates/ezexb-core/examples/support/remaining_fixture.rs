//! Original synthetic remaining-entity fixtures; no CAD-authored drawing data.
#![allow(dead_code)]
#[path = "document_fixture.rs"]
pub mod base;
#[path = "bom_fixture.rs"]
pub mod bom;
#[path = "table_fixture.rs"]
pub mod table;
#[path = "view2d_fixture.rs"]
pub mod view;

pub fn degenerate_arc_record(major: u32) -> Vec<u8> {
    let mut body = vec![0; if major >= 18 { 37 } else { 33 }];
    body[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    body[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    body[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    body[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    body[29..33].copy_from_slice(&1f32.to_le_bytes());
    for value in [
        2.,
        -3.5,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        std::f64::consts::TAU,
    ] {
        body.extend(value.to_le_bytes());
    }
    base::record(
        140,
        -5,
        "91d6556b-0fb8-4e13-998f-bdacbfd31025",
        if major >= 18 { 100 } else { 1 },
        &body,
    )
}

pub fn fixture(major: u32) -> Vec<u8> {
    base::mutate_stream(&base::fixture(major), "/*BlockStg/*GBlock", |data| {
        data.extend(degenerate_arc_record(major));
        if major == 10 {
            data.extend(view::view2d_record(150, -5, -60));
            data.extend(bom::bom_table_record(160, -5));
        } else {
            data.extend(table::table_record(170, -5, false));
            data.extend(table::table_record(171, -5, true));
        }
    })
}

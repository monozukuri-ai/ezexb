//! Original synthetic table records; no vendor drawing bytes or strings.
#![allow(dead_code)]
const TABLE: &str = "66dcce32-3156-46cf-88d5-067549db6cb3";
const HOLE_CHART: &str = "ba3395ef-bc2c-4c67-a7c9-3ed507561217";
fn attributes() -> Vec<u8> {
    let mut b = vec![0; 37];
    b[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    b[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    b[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    b[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    b[29..33].copy_from_slice(&1f32.to_le_bytes());
    b
}
fn u(b: &mut Vec<u8>, x: u32) {
    b.extend(x.to_le_bytes());
}
fn i(b: &mut Vec<u8>, x: i32) {
    b.extend(x.to_le_bytes());
}
fn h(b: &mut Vec<u8>, x: u16) {
    b.extend(x.to_le_bytes());
}
fn f(b: &mut Vec<u8>, x: f64) {
    b.extend(x.to_le_bytes());
}
fn string(b: &mut Vec<u8>, s: &str) {
    let units: Vec<_> = s.encode_utf16().chain([0]).collect();
    u(b, (units.len() * 2) as u32);
    for x in units {
        h(b, x);
    }
}
fn reference(b: &mut Vec<u8>, id: i32) {
    u(b, 0x800000);
    i(b, id);
}
fn record(kind: &str, b: &[u8], id: i32, owner: i32, extended: bool) -> Vec<u8> {
    let mut out = uuid::Uuid::parse_str(kind).unwrap().to_bytes_le().to_vec();
    i(&mut out, id);
    i(&mut out, owner);
    h(&mut out, if extended { 3 } else { 1 });
    u(&mut out, b.len() as u32);
    out.extend(b);
    if extended {
        out.extend(
            uuid::Uuid::parse_str("0fbb54e7-fc10-4b09-9499-924097c3bf94")
                .unwrap()
                .to_bytes_le(),
        );
        u(&mut out, 0);
        h(&mut out, 0);
        u(&mut out, 1);
        u(&mut out, 0xf000fe00);
        out.extend(&b[..37]);
        out.extend([0, 0]);
    }
    out
}
pub fn table_body(hole_chart: bool) -> Vec<u8> {
    let mut b = attributes();
    u(&mut b, 1);
    let mut line = attributes();
    for x in [2., 3., 0., 9., 3., 0.] {
        f(&mut line, x);
    }
    b.extend(record(
        "17ecf72d-c046-4e9a-8416-fd8220fd33b5",
        &line,
        0,
        0,
        false,
    ));
    reference(&mut b, -72);
    for (index, width) in [(0, 14.), (1, 23.)] {
        i(&mut b, index);
        f(&mut b, width);
    }
    i(&mut b, -1);
    i(&mut b, 3);
    f(&mut b, 7.5); // Sparse row index is stored, not a dense allocation.
    for col in 0..2 {
        i(&mut b, col);
        u(&mut b, 1);
        h(&mut b, 0);
        for _ in 0..4 {
            i(&mut b, -1);
        }
        u(&mut b, 17 + col as u32);
        string(&mut b, "Custom Ω");
        i(&mut b, 0);
        u(&mut b, 1);
        string(&mut b, if col == 0 { "Cell 日本語" } else { "42" });
        if col == 1 {
            i(&mut b, 2);
            u(&mut b, 4);
            h(&mut b, 65535);
            reference(&mut b, -83);
            f(&mut b, 1.25);
            f(&mut b, 0.125);
        }
        i(&mut b, -1);
        i(&mut b, 0);
        u(&mut b, 121);
        f(&mut b, 0.5);
        i(&mut b, 2);
        u(&mut b, 130);
        u(&mut b, 5);
        i(&mut b, 3);
        u(&mut b, 131);
        h(&mut b, 65535);
        i(&mut b, -1);
    }
    i(&mut b, -1);
    reference(&mut b, -91);
    u(&mut b, 2);
    string(&mut b, "P-");
    string(&mut b, "-S");
    i(&mut b, -1);
    for x in [11., 12., 0., 0.25] {
        f(&mut b, x);
    }
    if !hole_chart {
        h(&mut b, 1);
        u(&mut b, 2);
        u(&mut b, 3);
        f(&mut b, 4.5);
        i(&mut b, 2);
        f(&mut b, 6.5);
        i(&mut b, -1);
        for x in [1., 2., 3.] {
            f(&mut b, x);
        }
    }
    f(&mut b, 2.);
    i(&mut b, 0);
    u(&mut b, 3);
    u(&mut b, 7);
    i(&mut b, -1);
    if hole_chart {
        for x in [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 20., 30., 0., 1.,
        ] {
            f(&mut b, x);
        }
        u(&mut b, 1001);
        string(&mut b, "Origin Ω");
        u(&mut b, 3);
        h(&mut b, 1);
        u(&mut b, 2);
        f(&mut b, 2.5);
        f(&mut b, 5.);
        u(&mut b, 4);
        u(&mut b, 5);
        h(&mut b, 0);
        i(&mut b, 1);
        for x in [4, 1, 2, 3] {
            u(&mut b, x);
        }
        i(&mut b, -1);
        for x in [-101, -102, -103, -104, -105] {
            reference(&mut b, x);
        }
    }
    b
}
pub fn table_record(id: i32, owner: i32, hole_chart: bool) -> Vec<u8> {
    record(
        if hole_chart { HOLE_CHART } else { TABLE },
        &table_body(hole_chart),
        id,
        owner,
        true,
    )
}

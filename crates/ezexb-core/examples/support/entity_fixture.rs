//! Original synthetic stored entities; no third-party drawing bytes.
#![allow(dead_code)]
#[path = "document_fixture.rs"]
mod base;
#[path = "partno_fixture.rs"]
mod partno;

pub fn attributes(major: u32) -> Vec<u8> {
    let mut b = vec![0; if major >= 18 { 37 } else { 33 }];
    b[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    b[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    b[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    b[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    b[29..33].copy_from_slice(&1f32.to_le_bytes());
    b
}
fn f(b: &mut Vec<u8>, values: &[f64]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
fn u(b: &mut Vec<u8>, value: u32) {
    b.extend(value.to_le_bytes());
}
pub fn point_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    f(&mut b, &[12.5, -8.25, 0.]);
    b
}
fn presentation(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 1);
    b.extend(base::record(
        900,
        -5,
        "d6da372e-0f4f-4bfb-9e92-3b3363ac6c22",
        if major >= 18 { 100 } else { 1 },
        &point_body(major),
    ));
    b
}
pub fn dimension_body(major: u32) -> Vec<u8> {
    let mut b = presentation(major);
    f(&mut b, &[12.5]);
    b.extend(base::text("12.5"));
    b.extend(base::text("Original note"));
    u(&mut b, 0);
    for s in ["prefix", "suffix", ""] {
        b.extend(base::text(s));
    }
    f(&mut b, &[1.]);
    u(&mut b, 0);
    u(&mut b, 0);
    f(&mut b, &[1., 2., 0., 3., 4., 0., 5., 6., 0.]);
    // Deliberately uninterpreted type-specific construction data.
    b.extend([0; 128]);
    b
}
fn points(b: &mut Vec<u8>) {
    u(b, 1);
    f(b, &[2., 3., 0.]);
}
pub fn symbol_body(major: u32, index: usize) -> Vec<u8> {
    let mut b = presentation(major);
    match index {
        0 => {
            b.extend(1u16.to_le_bytes());
            b.push(0);
            b.extend(2u16.to_le_bytes());
            for text in ["", "1.6", "Original", "caption", "", ""] {
                b.extend(base::text(text));
            }
            points(&mut b);
        }
        1 => {
            points(&mut b);
            b.extend(1u16.to_le_bytes());
            b.extend(base::text("Top note"));
            b.extend(base::text("Bottom note"));
            b.push(1);
        }
        2 => {
            points(&mut b);
            u(&mut b, 1);
            f(&mut b, &[4., 5., 0.]);
            b.extend(base::text("A"));
            f(&mut b, &[1., 2., 0., 3., 4., 0.]);
            b.extend(1u16.to_le_bytes());
        }
        3 => {
            u(&mut b, 0x800000);
            u(&mut b, (-24i32) as u32);
            f(&mut b, &[7., 8., 0.]);
            u(&mut b, 1);
            points(&mut b);
            b.extend(base::text("Upper"));
            b.extend(base::text("Lower"));
            u(&mut b, 0);
            u(&mut b, 1);
            b.extend(base::text("B"));
            f(&mut b, &[0.5]);
        }
        _ => unreachable!(),
    }
    // Deliberately unresolved reference/style data, never treated as complete.
    b.extend([0; 40]);
    b
}
pub fn fixture(major: u32) -> Vec<u8> {
    base::mutate_stream(&base::fixture(major), "/*BlockStg/*GBlock", |data| {
        let schema = if major >= 18 { 100 } else { 1 };
        data.extend(base::record(
            100,
            -5,
            "d6da372e-0f4f-4bfb-9e92-3b3363ac6c22",
            schema,
            &point_body(major),
        ));
        for (i, kind) in [
            "86e4facf-ec82-45c2-8c00-c6a37e4e1c77",
            "131e29f6-4ef8-4503-829c-451acf7848c9",
            "76d19095-db03-4734-8a2e-178e1e7ec320",
            "5744b3d8-69bd-417f-9ea1-9e818dd7c25f",
            "0bdf94e2-0699-4ce6-94f8-dee137ae16b4",
            "432ac627-53ca-4496-a18c-53767f5aa8df",
        ]
        .iter()
        .enumerate()
        {
            data.extend(base::record(
                110 + i as i32,
                -5,
                kind,
                schema,
                &dimension_body(major),
            ));
        }
        if matches!(major, 10 | 15) {
            for (i, kind) in [
                "64e92135-bb0b-4c37-bee3-e7dcdabf179e",
                "9b79570d-4d57-4286-8105-ab9ce81c7f6c",
                "4b35dd46-6ddb-4b03-b233-51c18f214cf1",
                "ebe7faa2-a9db-4c6d-a156-5ba4ed6df79a",
            ]
            .iter()
            .enumerate()
            {
                data.extend(base::record(
                    120 + i as i32,
                    -5,
                    kind,
                    schema,
                    &symbol_body(major, i),
                ));
            }
        }
        if major == 10 {
            data.extend(partno::part_number_record(130, -5));
        }
    })
}

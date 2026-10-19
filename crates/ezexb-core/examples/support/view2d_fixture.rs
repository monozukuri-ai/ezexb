//! Original synthetic view records, independent of vendor drawings.
#![allow(dead_code)]
fn attributes() -> Vec<u8> {
    let mut body = vec![0; 33];
    body[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    body[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    body[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    body[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    body[29..33].copy_from_slice(&1f32.to_le_bytes());
    body
}
fn doubles(body: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        body.extend(value.to_le_bytes());
    }
}
fn text(body: &mut Vec<u8>, value: &str) {
    let units: Vec<u16> = value.encode_utf16().chain([0]).collect();
    body.extend(((units.len() * 2) as u32).to_le_bytes());
    for unit in units {
        body.extend(unit.to_le_bytes());
    }
}
pub fn view2d_body(block_id: i32, clip: bool) -> Vec<u8> {
    let mut body = attributes();
    doubles(&mut body, &[0.5, 2., 3., 1., 20., 30., 0.]);
    let (sine, cosine) = 0.5f64.sin_cos();
    doubles(
        &mut body,
        &[
            2. * cosine,
            2. * sine,
            0.,
            0.,
            -3. * sine,
            3. * cosine,
            0.,
            0.,
            0.,
            0.,
            1.,
            0.,
            20.,
            30.,
            0.,
            1.,
        ],
    );
    body.extend(0x800000u32.to_le_bytes());
    body.extend(block_id.to_le_bytes());
    body.extend(0u32.to_le_bytes()); // No attached attributes.
    text(&mut body, "detail-ref");
    body.extend(1u16.to_le_bytes());
    text(&mut body, "Detail view A");
    body.extend(19u32.to_le_bytes());
    body.push(u8::from(clip));
    if clip {
        body.extend(0u8..32); // Preserve arbitrary raw reference bytes.
        body.extend(2u32.to_le_bytes());
        body.extend(0xffffu16.to_le_bytes());
        body.extend(1u32.to_le_bytes());
        body.extend(1012u32.to_le_bytes());
        doubles(
            &mut body,
            &[
                4.,
                5.,
                0.,
                6.,
                1.,
                0.,
                0.,
                0.,
                1.,
                0.,
                std::f64::consts::TAU,
            ],
        );
    }
    body
}
pub fn view2d_record(id: i32, owner: i32, block_id: i32) -> Vec<u8> {
    let body = view2d_body(block_id, true);
    let mut out = uuid::Uuid::parse_str("bdf700da-e09c-4ade-a1e4-0eadf201ff16")
        .unwrap()
        .to_bytes_le()
        .to_vec();
    out.extend(id.to_le_bytes());
    out.extend(owner.to_le_bytes());
    out.extend([3, 0]);
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(&body);
    out.extend(
        uuid::Uuid::parse_str("0fbb54e7-fc10-4b09-9499-924097c3bf94")
            .unwrap()
            .to_bytes_le(),
    );
    out.extend(0u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(0xf000fe00u32.to_le_bytes());
    out.extend(&body[..33]);
    out.extend([0, 0]);
    out
}

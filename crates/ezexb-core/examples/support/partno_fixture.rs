//! Original synthetic part-number records, independent of vendor drawings.
#![allow(dead_code)]
const PART_NUMBER: &str = "db778346-3d2b-4eb3-852d-80637e5ca149";
const PART_ITEM: &str = "6fee9390-e7b8-4d84-bac4-1974b044e1be";
const LINE: &str = "17ecf72d-c046-4e9a-8416-fd8220fd33b5";

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
fn record(kind: &str, body: &[u8], id: i32, owner: i32, schema3: bool) -> Vec<u8> {
    let mut out = uuid::Uuid::parse_str(kind).unwrap().to_bytes_le().to_vec();
    out.extend(id.to_le_bytes());
    out.extend(owner.to_le_bytes());
    out.extend([if schema3 { 3 } else { 1 }, 0]);
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    if schema3 {
        out.extend(
            uuid::Uuid::parse_str("0fbb54e7-fc10-4b09-9499-924097c3bf94")
                .unwrap()
                .to_bytes_le(),
        );
        out.extend(0u32.to_le_bytes()); // Empty relation list.
        out.extend(0u16.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(0xf000fe00u32.to_le_bytes());
        out.extend(&body[..33]);
        out.extend([0, 0]); // No display-cache object or terminal object.
    }
    out
}
fn line() -> Vec<u8> {
    let mut body = attributes();
    doubles(&mut body, &[2., 3., 0., 5., 7., 0.]);
    record(LINE, &body, 0, 0, false)
}
pub fn part_item_body(index: i32) -> Vec<u8> {
    let mut body = attributes();
    body.extend(1u32.to_le_bytes());
    body.extend(line());
    body.extend(index.to_le_bytes());
    body
}
pub fn part_number_body() -> Vec<u8> {
    let mut body = attributes();
    body.extend(2u32.to_le_bytes());
    body.extend(line());
    body.extend(record(PART_ITEM, &part_item_body(101), 0, 0, true));
    for id in [-60i32, -61] {
        body.extend(0x800000u32.to_le_bytes());
        body.extend(id.to_le_bytes());
    }
    for value in [1u32, 2, 3] {
        body.extend(value.to_le_bytes());
    }
    body.extend(1u16.to_le_bytes());
    body.extend(0x80800000u32.to_le_bytes());
    body.extend(0u32.to_le_bytes());
    body.extend([0; 16]);
    body.extend(2u32.to_le_bytes());
    body.extend(17i32.to_le_bytes());
    body.extend(18i32.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    body.extend((-3i32).to_le_bytes());
    for point in [[11., 12., 0.], [0., 0., 0.]] {
        body.push(1);
        doubles(&mut body, &point);
    }
    body.push(1);
    doubles(&mut body, &[0.25]);
    body.push(1);
    doubles(&mut body, &[0.75]);
    body.push(1);
    doubles(&mut body, &[4., 5., 0.]);
    body.extend(2u32.to_le_bytes());
    doubles(&mut body, &[1., 2., 0., 3., 4., 0.]);
    doubles(&mut body, &[2.5]);
    body.extend(1u32.to_le_bytes());
    body.extend(record(PART_ITEM, &part_item_body(102), 0, 0, true));
    body.extend(0u32.to_le_bytes()); // Empty override map.
    body
}
pub fn part_number_record(id: i32, owner: i32) -> Vec<u8> {
    record(PART_NUMBER, &part_number_body(), id, owner, true)
}

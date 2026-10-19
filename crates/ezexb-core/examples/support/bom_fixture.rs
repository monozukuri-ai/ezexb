//! Handwritten synthetic BOM-table bytes; contains no vendor drawing data.
#![allow(dead_code)]
const BOM_TABLE: &str = "b995fced-a25a-4940-8d79-d92a345f37ce";
const BOM_HEADER: &str = "72f972f6-cd18-4d91-b00e-933d81947807";
const BOM_ROW: &str = "1a6eff84-514f-483d-8ff5-df5b204e2ba8";
fn attributes() -> Vec<u8> {
    let mut b = vec![0; 33];
    b[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    b[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    b[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    b[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    b[29..33].copy_from_slice(&1f32.to_le_bytes());
    b
}
fn doubles(b: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        b.extend(value.to_le_bytes());
    }
}
fn record(kind: &str, body: &[u8], id: i32, owner: i32, extended: bool) -> Vec<u8> {
    let mut b = uuid::Uuid::parse_str(kind).unwrap().to_bytes_le().to_vec();
    b.extend(id.to_le_bytes());
    b.extend(owner.to_le_bytes());
    b.extend([if extended { 3 } else { 1 }, 0]);
    b.extend((body.len() as u32).to_le_bytes());
    b.extend(body);
    if extended {
        b.extend(
            uuid::Uuid::parse_str("0fbb54e7-fc10-4b09-9499-924097c3bf94")
                .unwrap()
                .to_bytes_le(),
        );
        b.extend(0u32.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(0xf000fe00u32.to_le_bytes());
        b.extend(&body[..33]);
        b.extend([0, 0]);
    }
    b
}
fn line() -> Vec<u8> {
    let mut b = attributes();
    doubles(&mut b, &[2., 3., 0., 5., 7., 0.]);
    record("17ecf72d-c046-4e9a-8416-fd8220fd33b5", &b, 0, 0, false)
}
fn text() -> Vec<u8> {
    let mut b = attributes();
    let units: Vec<_> = "Part Ω".encode_utf16().chain([0]).collect();
    b.extend(((units.len() * 2) as u32).to_le_bytes());
    for unit in units {
        b.extend(unit.to_le_bytes());
    }
    doubles(&mut b, &[3.5, 4., 15., 0.]);
    b.push(0);
    b.extend(0u32.to_le_bytes());
    doubles(&mut b, &[1.]);
    b.extend(0x800000u32.to_le_bytes());
    b.extend((-33i32).to_le_bytes());
    doubles(&mut b, &[3., 5., 0.]);
    b.extend(0u32.to_le_bytes());
    doubles(&mut b, &[1.]);
    b.extend(0u32.to_le_bytes());
    record("76d76221-4358-465d-bc53-4449b3004657", &b, 0, 0, false)
}
pub fn bom_item_body() -> Vec<u8> {
    let mut b = attributes();
    b.extend(2u32.to_le_bytes());
    b.extend(line());
    b.extend(text());
    b
}
pub fn bom_table_body() -> Vec<u8> {
    let mut b = attributes();
    b.extend(2u32.to_le_bytes());
    b.extend(record(BOM_HEADER, &bom_item_body(), 0, 0, true));
    b.extend(record(BOM_ROW, &bom_item_body(), 0, 0, true));
    b.extend(0x800000u32.to_le_bytes());
    b.extend((-61i32).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    doubles(&mut b, &[2.5, 12., 34., 0., 20., 34., 0.]);
    b.extend(2u32.to_le_bytes());
    b.extend((-3i32).to_le_bytes());
    doubles(&mut b, &[20., 40., 0.]);
    b.extend(5i32.to_le_bytes());
    doubles(&mut b, &[30., 40., 0.]);
    b
}
pub fn bom_table_record(id: i32, owner: i32) -> Vec<u8> {
    record(BOM_TABLE, &bom_table_body(), id, owner, true)
}

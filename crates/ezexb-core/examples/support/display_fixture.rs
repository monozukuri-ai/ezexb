//! Original synthetic records for cached display records, circular primitives
//! with a non-orthonormal stored basis, construction lines, a zero polyline
//! normal and stored NURBS fit points. No CAD-authored drawing data.
#![allow(dead_code)]
use super::base;

const LINE: &str = "17ecf72d-c046-4e9a-8416-fd8220fd33b5";
const CIRCLE: &str = "7587733c-3df6-4c9d-8b08-20aeffe18291";
const ARC: &str = "91d6556b-0fb8-4e13-998f-bdacbfd31025";
const XLINE: &str = "3d98eac3-4cf1-444a-8204-fb392ccd23c8";
const POLYLINE: &str = "56d8e732-ad1f-426d-a4b0-6c5998434583";
const TEXT: &str = "76d76221-4358-465d-bc53-4449b3004657";
const NURBS: &str = "838896be-c069-4566-8991-afd750b4547c";
const INSERT: &str = "d38b4eba-a81f-4b28-aa19-71a60d884004";
const LINEAR_DIMENSION: &str = "1da848b5-8c84-4c11-80d6-45cf9b26aa48";
const THREE_POINT_ANGLE: &str = "d0e58c76-f166-4241-9943-62e6113a9da4";
const ROUGHNESS: &str = "64e92135-bb0b-4c37-bee3-e7dcdabf179e";
/// Synthetic class identifiers that no CAD program uses.
pub const UNKNOWN_CLASS: &str = "5a17c0de-0000-4000-8000-00000000c1a5";
pub const UNKNOWN_ITEM: &str = "5a17c0de-0000-4000-8000-0000000017e3";
pub const UNKNOWN_ARRAY: &str = "5a17c0de-0000-4000-8000-0000000a44a1";
pub const UNKNOWN_BARE: &str = "5a17c0de-0000-4000-8000-0000000ba4e0";
const RELATIONS: &str = "0fbb54e7-fc10-4b09-9499-924097c3bf94";
const DISPLAY_CACHE: &str = "0045f336-69f0-4e1a-b117-6f062840b2ab";

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
fn schema(major: u32) -> u8 {
    if major >= 18 {
        100
    } else {
        1
    }
}
fn guid(kind: &str) -> [u8; 16] {
    uuid::Uuid::parse_str(kind).unwrap().to_bytes_le()
}
/// A display record without property groups, as CAXA nests them.
fn child(kind: &str, id: i32, body: &[u8]) -> Vec<u8> {
    let mut b = guid(kind).to_vec();
    b.extend(id.to_le_bytes());
    b.extend(0i32.to_le_bytes());
    b.extend([1, 0]);
    u(&mut b, body.len() as u32);
    b.extend(body);
    b
}
fn line_body(major: u32, values: [f64; 4]) -> Vec<u8> {
    let mut b = attributes(major);
    f(
        &mut b,
        &[values[0], values[1], 0., values[2], values[3], 0.],
    );
    b
}
fn circular_body(major: u32, values: [f64; 8], sweep: f64) -> Vec<u8> {
    let mut b = attributes(major);
    let [cx, cy, radius, x0, x1, y0, y1, _] = values;
    f(&mut b, &[cx, cy, 0., radius, x0, x1, 0., y0, y1, 0., sweep]);
    b
}
fn text_body(major: u32, contents: &str, at: [f64; 2]) -> Vec<u8> {
    let mut b = attributes(major);
    b.extend(base::text(contents));
    f(&mut b, &[3.5, 4., 20., 0.]);
    b.push(0);
    u(&mut b, 0);
    f(&mut b, &[1.]);
    u(&mut b, 0x800000);
    b.extend((-33i32).to_le_bytes());
    f(&mut b, &[at[0], at[1], 0.]);
    if major <= 10 {
        b.extend([0; 4]);
    }
    f(&mut b, &[-1.]);
    b.extend([0; 4]);
    if major >= 15 {
        b.extend([0; 14]);
    }
    if major > 10 {
        b.extend(guid("6cabfdc4-340a-44d6-a844-d865a15538c0"));
        b.extend([0; 4]);
    }
    if major >= 15 {
        b.extend([0; 24]);
    }
    b
}
/// A plain block reference: no display records, attributes or clip.
fn insert_body(major: u32, block: i32, at: [f64; 2]) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 0);
    f(&mut b, &[0., 1., 1., 1., at[0], at[1], 0.]);
    f(
        &mut b,
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., at[0], at[1], 0., 1.,
        ],
    );
    u(&mut b, 0x800000);
    b.extend(block.to_le_bytes());
    u(&mut b, 0);
    b.extend(base::text(""));
    b.extend([0; 2]);
    if major > 10 {
        b.extend([0; 6]);
    }
    b
}
fn dimension_fields(b: &mut Vec<u8>, value: f64, shown: &str) {
    f(b, &[value]);
    b.extend(base::text(shown));
    b.extend(base::text(""));
    u(b, 0);
    for s in ["", "", shown] {
        b.extend(base::text(s));
    }
    f(b, &[1.]);
    u(b, 0);
    u(b, 2);
    f(b, &[60., 12., 0., 65., 14., 0., 0., 0., 0.]);
}

/// ARC whose stored vectors are both twice a unit vector.
pub fn scaled_arc_body(major: u32) -> Vec<u8> {
    circular_body(
        major,
        [30., 40., 5., 2., 0., 0., 2., 0.],
        std::f64::consts::FRAC_PI_2,
    )
}
/// CIRCLE whose stored Y vector is neither unit nor perpendicular to X.
pub fn skewed_circle_body(major: u32) -> Vec<u8> {
    circular_body(
        major,
        [50., 40., 4., 1., 0., 0.5, 0.5, 0.],
        std::f64::consts::TAU,
    )
}
pub fn xline_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    f(&mut b, &[0., 25., 0., 1., 0., 0.]);
    b
}
/// LINE with floating-point residue in one stored Z.
pub fn residue_line_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    f(&mut b, &[0., 30., 0., 35., 30., 2.5e-28]);
    b
}
/// POLYLINE whose stored normal is the zero vector (save majors after 10).
pub fn zero_normal_polyline_body(major: u32) -> Vec<u8> {
    assert!(major > 10);
    let mut b = attributes(major);
    u(&mut b, 3);
    for [x, y] in [[0., 50.], [10., 50.], [10., 60.]] {
        f(&mut b, &[x, y, 0., 0., 0.]);
    }
    u(&mut b, 0);
    f(&mut b, &[0., 0., 0.]);
    b
}
/// NURBS with three stored fit points (save majors from 15).
pub fn fitted_nurbs_body(major: u32) -> Vec<u8> {
    assert!(major >= 15);
    let mut b = attributes(major);
    for (index, point) in [[0., 70.], [5., 75.], [10., 70.]].iter().enumerate() {
        u(&mut b, index as u32);
        f(&mut b, &[point[0], point[1], 0.]);
    }
    u(&mut b, u32::MAX);
    f(&mut b, &[1e-10]);
    f(&mut b, &[0., 0., 1., -0.]);
    for v in [4, 3, 1015, 1030] {
        u(&mut b, v);
    }
    for [x, y] in [[0., 70.], [2., 76.], [8., 76.], [10., 70.]] {
        f(&mut b, &[x, y, 0., 1.]);
    }
    f(&mut b, &[0., 0., 0., 0., 1., 1., 1., 1.]);
    b
}
/// A hidden item of another unknown class: one line and one text of its own.
fn unknown_item_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    b[27..29].copy_from_slice(&u16::MAX.to_le_bytes());
    u(&mut b, 2);
    b.extend(child(LINE, 0, &line_body(major, [80., 5., 90., 5.])));
    b.extend(child(TEXT, 0, &text_body(major, "Hidden item", [80., 6.])));
    b.extend(7i32.to_le_bytes());
    b
}
/// An entity of a class no reader knows: its body opens with the display
/// records CAXA would have cached (a line, a text, a hidden item), then holds
/// eleven bytes of its own.
pub fn unknown_entity_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 3);
    b.extend(child(LINE, 0, &line_body(major, [80., 0., 90., 0.])));
    b.extend(child(TEXT, 0, &text_body(major, "Original Ω", [80., 1.])));
    b.extend(child(UNKNOWN_ITEM, 0, &unknown_item_body(major)));
    b.extend([0x5a; 11]);
    b
}
/// A surface-roughness symbol of a save version whose fields are not qualified:
/// three lines and a text as display records, then an opaque synthetic tail.
pub fn unqualified_roughness_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 4);
    for values in [
        [100., 0., 101., 2.],
        [101., 2., 102., 0.],
        [102., 0., 104., 4.],
    ] {
        b.extend(child(LINE, 0, &line_body(major, values)));
    }
    b.extend(child(TEXT, 0, &text_body(major, "Ra 3.2", [102., 4.])));
    b.extend([0x33; 40]);
    b
}
/// A linear dimension drawn with two block arrowheads: a line and two cached
/// block references, then the shared dimension fields.
pub fn block_arrow_dimension_body(major: u32, block: i32) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 3);
    b.extend(child(LINE, 0, &line_body(major, [60., 10., 70., 10.])));
    b.extend(child(INSERT, 0, &insert_body(major, block, [60., 10.])));
    b.extend(child(INSERT, 0, &insert_body(major, block, [70., 10.])));
    dimension_fields(&mut b, 10., "10");
    b
}
/// A three-point angle dimension: an arc and a line as display records, the
/// shared dimension fields and a synthetic type-specific tail.
pub fn three_point_angle_body(major: u32) -> Vec<u8> {
    let mut b = attributes(major);
    u(&mut b, 2);
    b.extend(child(
        ARC,
        0,
        &circular_body(
            major,
            [60., 20., 6., 1., 0., 0., 1., 0.],
            std::f64::consts::FRAC_PI_4,
        ),
    ));
    b.extend(child(LINE, 0, &line_body(major, [60., 20., 68., 20.])));
    dimension_fields(&mut b, std::f64::consts::FRAC_PI_4, "45%d");
    b.extend([0x44; 24]);
    b
}

fn extension(b: &mut Vec<u8>, attributes: &[u8], cached: &[Vec<u8>]) {
    b.extend(guid(RELATIONS));
    u(b, 0);
    b.extend(0u16.to_le_bytes());
    u(b, 1);
    u(b, 0xf000fe00);
    b.extend(attributes);
    if cached.is_empty() {
        b.push(0);
    } else {
        b.push(1);
        b.extend(guid(DISPLAY_CACHE));
        u(b, cached.len() as u32);
        for record in cached {
            b.extend(record);
        }
    }
    b.push(0);
}
/// A schema-3 record whose body holds only the class's own fields: the entity
/// attribute block and one cached block reference live in the extension, the
/// layout of block-reference arrays.
pub fn array_record(id: i32, owner: i32, major: u32, block: i32) -> Vec<u8> {
    let mut body = Vec::new();
    f(&mut body, &[0., 1., 1., 1., 40., 50., 0.]);
    body.extend([0x5a; 24]);
    let mut b = guid(UNKNOWN_ARRAY).to_vec();
    b.extend(id.to_le_bytes());
    b.extend(owner.to_le_bytes());
    b.extend([3, 0]);
    u(&mut b, body.len() as u32);
    b.extend(&body);
    extension(
        &mut b,
        &attributes(major),
        &[child(INSERT, 0, &insert_body(major, block, [40., 50.]))],
    );
    b
}

/// Identifiers of the appended records, in stream order.
pub const SCALED_ARC: i32 = 300;
pub const SKEWED_CIRCLE: i32 = 301;
pub const XLINE_ID: i32 = 302;
pub const RESIDUE_LINE: i32 = 303;
pub const UNKNOWN_ENTITY: i32 = 304;
pub const ARRAY: i32 = 305;
pub const BLOCK_ARROW_DIMENSION: i32 = 306;
pub const THREE_POINT_ANGLE_ID: i32 = 307;
pub const BARE_UNKNOWN: i32 = 308;
pub const ZERO_NORMAL_POLYLINE: i32 = 309;
pub const UNQUALIFIED_ROUGHNESS: i32 = 310;
pub const FITTED_NURBS: i32 = 311;

/// The stored-document fixture with the records above appended to its space.
/// Save major 10 has no polyline normal, no fit-point section and a qualified
/// roughness layout, so those three records are added from major 16 only.
pub fn fixture(major: u32) -> Vec<u8> {
    let schema = schema(major);
    base::mutate_stream(&base::fixture(major), "/*BlockStg/*GBlock", |data| {
        let mut add = |id: i32, kind: &str, body: &[u8]| {
            data.extend(base::record(id, -5, kind, schema, body));
        };
        add(SCALED_ARC, ARC, &scaled_arc_body(major));
        add(SKEWED_CIRCLE, CIRCLE, &skewed_circle_body(major));
        add(XLINE_ID, XLINE, &xline_body(major));
        add(RESIDUE_LINE, LINE, &residue_line_body(major));
        add(UNKNOWN_ENTITY, UNKNOWN_CLASS, &unknown_entity_body(major));
        data.extend(array_record(ARRAY, -5, major, -60));
        let mut add = |id: i32, kind: &str, body: &[u8]| {
            data.extend(base::record(id, -5, kind, schema, body));
        };
        add(
            BLOCK_ARROW_DIMENSION,
            LINEAR_DIMENSION,
            &block_arrow_dimension_body(major, -60),
        );
        add(
            THREE_POINT_ANGLE_ID,
            THREE_POINT_ANGLE,
            &three_point_angle_body(major),
        );
        // No display records at all: this one stays an unsupported entity.
        let mut bare = attributes(major);
        f(&mut bare, &[1234.5, -20., 0.]);
        add(BARE_UNKNOWN, UNKNOWN_BARE, &bare);
        if major > 10 {
            add(
                ZERO_NORMAL_POLYLINE,
                POLYLINE,
                &zero_normal_polyline_body(major),
            );
            add(
                UNQUALIFIED_ROUGHNESS,
                ROUGHNESS,
                &unqualified_roughness_body(major),
            );
            add(FITTED_NURBS, NURBS, &fitted_nurbs_body(major));
        }
    })
}

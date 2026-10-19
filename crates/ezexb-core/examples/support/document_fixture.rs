//! Original handcrafted fixture, never exported by or copied from a CAD program.
#![allow(dead_code)]
use std::io::{Cursor, Write};

pub fn text(value: &str) -> Vec<u8> {
    let words: Vec<_> = value.encode_utf16().chain([0]).collect();
    let mut out = ((words.len() * 2) as u32).to_le_bytes().to_vec();
    for word in words {
        out.extend(word.to_le_bytes());
    }
    out
}
pub fn header() -> Vec<u8> {
    let mut out = text("0.0");
    out.resize(172, 0);
    out
}
pub fn record(id: i32, owner: i32, kind: &str, schema: u8, body: &[u8]) -> Vec<u8> {
    let mut out = uuid::Uuid::parse_str(kind).unwrap().to_bytes_le().to_vec();
    out.extend(id.to_le_bytes());
    out.extend(owner.to_le_bytes());
    out.extend([schema, u8::from(schema == 100)]);
    if schema == 100 {
        out.extend([0; 4]);
    }
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}
fn doubles(out: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn attributes(major: u32) -> Vec<u8> {
    let mut out = vec![0; if major >= 18 { 37 } else { 33 }];
    out[0..4].copy_from_slice(&0x800000u32.to_le_bytes());
    out[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    out[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
    out[12..16].copy_from_slice(&(-19i32).to_le_bytes());
    out[20..24].copy_from_slice(&0.6f32.to_le_bytes());
    out[29..33].copy_from_slice(&1.25f32.to_le_bytes());
    out
}
fn block(id: i32, major: u32, name: &str) -> Vec<u8> {
    let mut body = text(name);
    body.extend([0; 9]);
    if major > 10 {
        body.extend([0; 10]);
        body.extend(text(""));
        body.extend(text(""));
        body.push(0);
    }
    record(
        id,
        -4,
        "b65f3081-5c0e-4d00-97b4-0c5231051af1",
        if major >= 18 { 100 } else { 1 },
        &body,
    )
}
pub fn fixture(major: u32) -> Vec<u8> {
    let build = match major {
        10 => "10.0.0.0000",
        11 => "11.7.0.5203",
        15 => "15.0.0.5681",
        16 => "16.4.0.13104",
        18 => "18.0.0.18159",
        _ => panic!("bad fixture major"),
    };
    let mut cfb =
        cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new())).unwrap();
    cfb.create_storage("/*BlockStg").unwrap();
    let file = [
        text("0.0"),
        text("CaxaDraft Document"),
        text(build),
        text("Original ezexb test fixture"),
        vec![0; 213],
        text(""),
        vec![0; 8],
    ]
    .concat();
    cfb.create_stream("/*FileInfoStream")
        .unwrap()
        .write_all(&file)
        .unwrap();
    let mut styles = header();
    let mut layer = [text("Test layer"), text("Test layer"), text("test-layer")].concat();
    layer.extend(0x00123456u32.to_le_bytes());
    layer.extend(0x50000000u32.to_le_bytes());
    layer.extend(text("Original"));
    layer.extend(0.35f32.to_le_bytes());
    layer.extend(2u32.to_le_bytes()); // Explicit native line width selection.
    layer.extend(0x800000u32.to_le_bytes());
    layer.extend((-19i32).to_le_bytes());
    if major >= 18 {
        layer.extend([0; 4]);
    }
    styles.extend(record(
        -24,
        -23,
        "6d0ae65f-a804-4cf3-9d92-07831c336a7a",
        1,
        &layer,
    ));
    let mut lt = [
        text("Continuous"),
        text("Original"),
        1f32.to_le_bytes().to_vec(),
        vec![0; 4],
        text("Continuous"),
    ]
    .concat();
    if major > 10 {
        lt.push(0);
    }
    styles.extend(record(
        -19,
        -16,
        "c8f1ff60-a882-4639-93cb-87921b4f5d0b",
        1,
        &lt,
    ));
    let mut txt_style = [
        text("Original style"),
        text(""),
        text("sans-serif"),
        text("sans-serif"),
    ]
    .concat();
    doubles(&mut txt_style, &[1., 1., 0., 1.5, 3.5, 0., 0.]);
    if major > 10 {
        txt_style.extend(0u32.to_le_bytes());
        txt_style.extend(text(""));
    }
    if major >= 18 {
        txt_style.extend([0; 6]);
    }
    styles.extend(record(
        -33,
        -32,
        "b9bd09d2-0eaa-4fe7-9ff6-a23f19fe8954",
        1,
        &txt_style,
    ));
    cfb.create_stream("/*StyleStream")
        .unwrap()
        .write_all(&styles)
        .unwrap();
    let mut spaces = header();
    let mut paper = vec![0; 120];
    for _ in 0..10 {
        paper.extend(text(""));
    }
    paper.extend(text("Sheet A"));
    paper.extend([0; 4]);
    paper.extend(1f64.to_le_bytes());
    paper.extend([0; 32]);
    paper.extend(0x800000u32.to_le_bytes());
    paper.extend((-5i32).to_le_bytes());
    paper.extend([0; 60]);
    if major > 10 {
        paper.extend([0; 4]);
    }
    if major >= 17 {
        paper.extend([0; 16]);
    }
    spaces.extend(record(
        -7,
        -6,
        "979f2fd6-ca62-42b8-9cb8-d47716dd1dd7",
        1,
        &paper,
    ));
    cfb.create_stream("/*SpaceStream")
        .unwrap()
        .write_all(&spaces)
        .unwrap();
    for path in [
        "/*AppInfoStream",
        "/*DBSelfData",
        "/*DictionaryStream",
        "/*BlockStg/*BlockMgrStream",
    ] {
        cfb.create_stream(path)
            .unwrap()
            .write_all(&header())
            .unwrap();
    }
    let schema = if major >= 18 { 100 } else { 1 };
    let mut data = header();
    data.extend(block(-5, major, "Sheet contents"));
    data.extend(block(-60, major, "Reusable part"));
    let mut line = attributes(major);
    doubles(&mut line, &[1., 2., 0., 4., 6., 0.]);
    data.extend(record(
        1,
        -5,
        "17ecf72d-c046-4e9a-8416-fd8220fd33b5",
        schema,
        &line,
    ));
    let mut circle = attributes(major);
    doubles(
        &mut circle,
        &[
            5.,
            6.,
            0.,
            2.,
            1.,
            0.,
            0.,
            0.,
            1.,
            0.,
            std::f64::consts::TAU,
        ],
    );
    data.extend(record(
        2,
        -60,
        "7587733c-3df6-4c9d-8b08-20aeffe18291",
        schema,
        &circle,
    ));
    let mut arc = attributes(major);
    doubles(
        &mut arc,
        &[
            0.,
            0.,
            0.,
            3.,
            1.,
            0.,
            0.,
            0.,
            -1.,
            0.,
            std::f64::consts::FRAC_PI_2,
        ],
    );
    data.extend(record(
        3,
        -5,
        "91d6556b-0fb8-4e13-998f-bdacbfd31025",
        schema,
        &arc,
    ));
    let mut poly = attributes(major);
    poly.extend(3u32.to_le_bytes());
    for [x, y] in [[0., 0.], [10., 0.], [10., 10.]] {
        doubles(&mut poly, &[x, y]);
        if major <= 10 {
            doubles(&mut poly, &[0.]);
        }
        doubles(&mut poly, &[0., 0., 0.]);
    }
    poly.extend(1u32.to_le_bytes());
    if major > 10 {
        doubles(&mut poly, &[0., 0., 1.]);
    }
    data.extend(record(
        4,
        -5,
        "56d8e732-ad1f-426d-a4b0-6c5998434583",
        schema,
        &poly,
    ));
    let mut txt = attributes(major);
    txt.extend(text("Original 日本語"));
    doubles(&mut txt, &[3.5, 4., 20., 0.]);
    txt.push(0);
    txt.extend(0u32.to_le_bytes());
    doubles(&mut txt, &[1.]);
    txt.extend(0x800000u32.to_le_bytes());
    txt.extend((-33i32).to_le_bytes());
    doubles(&mut txt, &[7., 8., 0.]);
    if major <= 10 {
        txt.extend([0; 4]);
    }
    doubles(&mut txt, &[-1.]);
    txt.extend([0; 4]);
    if major >= 15 {
        txt.extend([0; 14]);
    }
    if major > 10 {
        txt.extend(
            uuid::Uuid::parse_str("6cabfdc4-340a-44d6-a844-d865a15538c0")
                .unwrap()
                .to_bytes_le(),
        );
        txt.extend([0; 4]);
    }
    if major >= 15 {
        txt.extend([0; 24]);
    }
    data.extend(record(
        5,
        -5,
        "76d76221-4358-465d-bc53-4449b3004657",
        schema,
        &txt,
    ));
    let mut insert = attributes(major);
    insert.extend([0; 4]);
    doubles(&mut insert, &[0., 1., 1., 1., 20., 30., 0.]);
    doubles(
        &mut insert,
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
    );
    insert.extend(0x800000u32.to_le_bytes());
    insert.extend((-60i32).to_le_bytes());
    insert.extend([0; 4]);
    insert.extend(text(""));
    insert.extend([0; 2]);
    if major > 10 {
        insert.extend([0; 6]);
    }
    data.extend(record(
        6,
        -5,
        "d38b4eba-a81f-4b28-aa19-71a60d884004",
        schema,
        &insert,
    ));
    cfb.create_stream("/*BlockStg/*GBlock")
        .unwrap()
        .write_all(&data)
        .unwrap();
    cfb.into_inner().into_inner()
}

pub fn mutate_stream(bytes: &[u8], path: &str, mutate: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    use std::io::Read;
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes.to_vec())).unwrap();
    let mut data = Vec::new();
    file.open_stream(path)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    mutate(&mut data);
    let mut stream = file.create_stream(path).unwrap();
    stream.write_all(&data).unwrap();
    drop(stream);
    file.into_inner().into_inner()
}

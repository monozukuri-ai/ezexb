//! Original synthetic structures for testing observed stream layouts, not a CAD-authored EXB.
use flate2::{write::ZlibEncoder, Compression};
use std::{
    env,
    fs::OpenOptions,
    io::{Cursor, Write},
};

fn text(value: &str) -> Vec<u8> {
    let words: Vec<_> = value.encode_utf16().chain([0]).collect();
    let mut out = ((words.len() * 2) as u32).to_le_bytes().to_vec();
    for word in words {
        out.extend(word.to_le_bytes());
    }
    out
}

fn header() -> Vec<u8> {
    let mut bytes = text("0.0");
    bytes.extend([0; 160]);
    bytes
}

fn record(guid: [u8; 16], schema: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = guid.to_vec();
    bytes.extend(7i32.to_le_bytes());
    bytes.extend((-5i32).to_le_bytes());
    bytes.extend(schema.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn compressed(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0xde, 0xe3, 0x3d];
    out.extend(262144u32.to_le_bytes());
    out.extend((data.len() as u64).to_le_bytes());
    for chunk in data.chunks(262144) {
        let mut padded = chunk.to_vec();
        padded.resize(262144, 0);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&padded).unwrap();
        let block = encoder.finish().unwrap();
        out.extend((block.len() as u32).to_le_bytes());
        out.extend(block);
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .ok_or("usage: make_analysis_fixture OUTPUT")?;
    let mut cfb =
        cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    cfb.create_storage("/*BlockStg")?;
    for path in [
        "/*SpaceStream",
        "/*StyleStream",
        "/*DBSelfData",
        "/*DictionaryStream",
        "/*BlockStg/*BlockMgrStream",
    ] {
        cfb.create_stream(path)?;
    }
    let mut file = [
        text("0.0"),
        text("CaxaDraft Document"),
        text("Synthetic Build"),
        text("Original ezexb fixture"),
    ]
    .concat();
    file.extend([0; 213]);
    file.extend(text("Opaque synthetic value"));
    file.extend(1u32.to_le_bytes());
    file.extend(text("Title"));
    file.extend(text("Test 日本語"));
    file.extend([0; 4]);
    cfb.create_stream("/*FileInfoStream")?.write_all(&file)?;
    let mut app = header();
    let fields = [text("SyntheticApp"), text("1.0"), text(""), text("fixture")].concat();
    app.extend(record(
        [
            0xf9, 0xcf, 0x80, 0x8c, 0xb6, 0x27, 0x8e, 0x42, 0x9a, 0x3d, 0x5e, 0xaf, 0x4e, 0x1c,
            0xba, 0x51,
        ],
        1,
        &fields,
    ));
    cfb.create_stream("/*AppInfoStream")?
        .write_all(&compressed(&app))?;

    let mut bmp = b"BM".to_vec();
    bmp.extend(62u32.to_le_bytes());
    bmp.extend([0; 4]);
    bmp.extend(54u32.to_le_bytes());
    bmp.extend(40u32.to_le_bytes());
    bmp.extend(2i32.to_le_bytes());
    bmp.extend(1i32.to_le_bytes());
    bmp.extend(1u16.to_le_bytes());
    bmp.extend(24u16.to_le_bytes());
    bmp.extend([0; 4]);
    bmp.extend(8u32.to_le_bytes());
    bmp.extend([0; 16]);
    bmp.extend([0, 0, 255, 0, 255, 0, 0, 0]);
    let mut preview_body = vec![
        2, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
    ];
    preview_body.extend((bmp.len() as u32).to_le_bytes());
    preview_body.extend(bmp);
    let mut preview = header();
    preview.extend(record(
        [
            0xf1, 0xcb, 0xe9, 0x25, 0xb1, 0xbe, 0x72, 0x4c, 0xbc, 0xfe, 0x82, 0x50, 0x88, 0xc4,
            0x8b, 0x21,
        ],
        1,
        &preview_body,
    ));
    cfb.create_stream("/*PreviewStream")?.write_all(&preview)?;
    let line_guid = [
        0x2d, 0xf7, 0xec, 0x17, 0x46, 0xc0, 0x9a, 0x4e, 0x84, 0x16, 0xfd, 0x82, 0x20, 0xfd, 0x33,
        0xb5,
    ];
    let mut block = header();
    let mut line = vec![0; 81];
    line[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    line[20..24].copy_from_slice(&0.6f32.to_le_bytes());
    line[29..33].copy_from_slice(&1.25f32.to_le_bytes());
    for (i, value) in [1.25f64, -2.5, 0.0, 4.25, 1.5, 0.0].iter().enumerate() {
        line[33 + i * 8..41 + i * 8].copy_from_slice(&value.to_le_bytes());
    }
    block.extend(record(line_guid, 1, &line));
    block.extend(record([0x42; 16], 0x7777, &[0; 32]));
    block.extend(record(line_guid, 1, &[0; 81]));
    cfb.create_stream("/*BlockStg/*Blk_1")?
        .write_all(&compressed(&block))?;
    let plain = record(line_guid, 1, &line);
    let mut extended = header();
    extended.extend(&plain[..24]);
    extended.extend([1, 1]); // Object schema and property-presence are separate fields.
    extended.extend(1u32.to_le_bytes());
    extended.extend([0x11; 16]);
    extended.extend(2u32.to_le_bytes());
    extended.push(7);
    extended.extend(text("Label"));
    extended.extend(text("Original synthetic value"));
    extended.push(12);
    extended.extend([0x22; 16]);
    extended.extend(3u32.to_le_bytes());
    extended.extend([10, 20, 30]);
    extended.extend(&plain[26..]);
    cfb.create_stream("/*BlockStg/*Blk_2")?
        .write_all(&compressed(&extended))?;
    let mut large = vec![b'X'; 262144];
    large.extend(b"hello world");
    cfb.create_stream("/multiblock")?
        .write_all(&compressed(&large))?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(&cfb.into_inner().into_inner())?;
    Ok(())
}

//! Original synthetic recursive records; no vendor sample bytes.
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

fn guid(value: &str) -> [u8; 16] {
    uuid::Uuid::parse_str(value).unwrap().to_bytes_le()
}

fn record(id: i32, schema: u8, kind: [u8; 16], body: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend(id.to_le_bytes());
    out.extend((-5i32).to_le_bytes());
    out.extend([schema, u8::from(schema == 100)]);
    if schema == 100 {
        out.extend(0u32.to_le_bytes());
    }
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn line(id: i32, schema: u8, width: usize) -> Vec<u8> {
    let mut body = vec![0; width + 48];
    body[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    body[29..33].copy_from_slice(&1f32.to_le_bytes());
    for (i, value) in [1f64, 2., 3., 4., 5., 6.].iter().enumerate() {
        body[width + i * 8..width + i * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    record(
        id,
        schema,
        guid("17ecf72d-c046-4e9a-8416-fd8220fd33b5"),
        &body,
    )
}

fn extension(id: i32, width: usize, children: Option<&[Vec<u8>]>, tail: u8) -> Vec<u8> {
    let mut attributes = vec![0; width];
    attributes[4..8].copy_from_slice(&(-24i32).to_le_bytes());
    attributes[29..33].copy_from_slice(&1.25f32.to_le_bytes());
    let mut body = attributes.clone();
    body.extend(b"Original opaque synthetic body");
    let mut out = record(id, 3, [0x55; 16], &body);
    out.extend(guid("0fbb54e7-fc10-4b09-9499-924097c3bf94"));
    out.extend(1u32.to_le_bytes());
    out.extend(guid("62691bae-8d34-4142-a32f-b5e5ba30d444"));
    out.extend(0u32.to_le_bytes());
    out.extend((-24i32).to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(0xf000fe40u32.to_le_bytes());
    out.extend(attributes);
    out.push(u8::from(children.is_some()));
    if let Some(children) = children {
        out.extend(guid("0045f336-69f0-4e1a-b117-6f062840b2ab"));
        out.extend((children.len() as u32).to_le_bytes());
        for child in children {
            out.extend(child);
        }
    }
    out.push(tail);
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: make_schema3_fixture OUTPUT 33|37".into());
    }
    let width = args[1].to_str().ok_or("invalid width")?.parse::<usize>()?;
    if !matches!(width, 33 | 37) {
        return Err("expected width 33 or 37".into());
    }
    let build = if width == 33 {
        "10.0.0.0000"
    } else {
        "18.0.0.18159"
    };
    let mut cfb =
        cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    cfb.create_storage("/*BlockStg")?;
    let mut info = [
        text("0.0"),
        text("CaxaDraft Document"),
        text(build),
        text("Original ezexb fixture"),
    ]
    .concat();
    info.extend([0; 213]);
    info.extend(text("Synthetic schema3 test"));
    info.extend(0u32.to_le_bytes());
    info.extend([0; 4]);
    cfb.create_stream("/*FileInfoStream")?.write_all(&info)?;

    let child = extension(2, width, Some(&[line(3, 1, width)]), 0);
    let parent = extension(1, width, Some(&[child, line(4, 1, width)]), 0);
    let mut truncated = parent.clone();
    truncated.pop();
    let cases = [
        (
            "/*BlockStg/*Blk_1",
            vec![parent, line(5, 1, 33), line(6, 100, 37)],
        ),
        (
            "/*BlockStg/*Blk_2",
            vec![line(5, 1, 33), extension(1, width, None, 1), line(7, 1, 33)],
        ),
        ("/*BlockStg/*Blk_3", vec![truncated]),
        (
            "/*BlockStg/*Blk_4",
            vec![
                extension(1, width, Some(&[record(8, 77, [0x44; 16], &[0; 81])]), 0),
                line(7, 1, 33),
            ],
        ),
        ("/*BlockStg/*Blk_5", vec![extension(1, width, None, 0)]),
    ];
    for (path, records) in cases {
        let mut bytes = text("0.0");
        bytes.extend([0; 160]);
        for item in records {
            bytes.extend(item);
        }
        cfb.create_stream(path)?.write_all(&bytes)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[0])?
        .write_all(&cfb.into_inner().into_inner())?;
    Ok(())
}

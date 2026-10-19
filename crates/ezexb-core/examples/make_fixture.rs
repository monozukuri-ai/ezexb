//! Generate an original synthetic CFB for public tests. This is not a real EXB.
use std::{
    env,
    fs::OpenOptions,
    io::{Cursor, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("usage: make_fixture OUTPUT")?;
    let mut cfb =
        cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    cfb.create_storage("/nested")?;
    cfb.create_storage("/empty")?;
    cfb.create_stream("/nested/日本語")?
        .write_all(b"small stream\x00\xff")?;
    cfb.create_stream("/large")?.write_all(&vec![0x5a; 9000])?;
    cfb.create_stream("/zero")?;
    let data = cfb.into_inner().into_inner();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(&data)?;
    Ok(())
}

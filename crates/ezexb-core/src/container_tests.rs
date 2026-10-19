use crate::{inspect_bytes, Container, Error, ParseOptions};
use std::io::{Cursor, Write};

fn fixture(version: cfb::Version) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
    cfb.create_storage("/nested").unwrap();
    cfb.create_stream("/nested/small")
        .unwrap()
        .write_all(b"hello\0\xff")
        .unwrap();
    cfb.create_stream("/large")
        .unwrap()
        .write_all(&vec![0x5a; 9000])
        .unwrap();
    cfb.create_stream("/zero").unwrap();
    cfb.into_inner().into_inner()
}

#[test]
fn versions_and_mini_regular_and_empty_streams() {
    for version in [cfb::Version::V3, cfb::Version::V4] {
        let bytes = fixture(version);
        let mut container = Container::from_bytes(&bytes, ParseOptions::default()).unwrap();
        assert_eq!(container.info().classification, "cfb");
        assert!(!container.info().exb_confirmed);
        assert_eq!(container.info().cfb_version, Some(version.number()));
        assert_eq!(container.info().entries.len(), 4);
        assert_eq!(
            container.read_stream("/nested/small", 0, None).unwrap(),
            b"hello\0\xff"
        );
        assert_eq!(
            container.read_stream("/large", 4090, Some(20)).unwrap(),
            vec![0x5a; 20]
        );
        assert_eq!(container.read_stream("/zero", 0, None).unwrap(), b"");
        assert_eq!(
            container.stream_sha256("/zero").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}

#[test]
fn marker_layout_is_only_a_candidate_and_requires_correct_kinds() {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb.create_storage("/*BlockStg").unwrap();
    for path in [
        "/*fileinfostream",
        "/*AppInfoStream",
        "/*SpaceStream",
        "/*StyleStream",
        "/*DBSelfData",
        "/*DictionaryStream",
        "/*XrefInfoStream",
        "/*BlockStg/*BlockMgrStream",
    ] {
        cfb.create_stream(path).unwrap();
    }
    let bytes = cfb.into_inner().into_inner();
    let info = inspect_bytes(&bytes, ParseOptions::default()).unwrap();
    assert_eq!(info.classification, "cfb_exb_candidate");
    assert!(info.missing_markers.is_empty());
    assert!(!info.exb_confirmed);
    let mut cfb = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    cfb.remove_stream("/*XrefInfoStream").unwrap();
    let bytes = cfb.into_inner().into_inner();
    let info = inspect_bytes(&bytes, ParseOptions::default()).unwrap();
    assert_eq!(info.classification, "cfb_exb_candidate");
    assert_eq!(info.missing_markers, ["/*XrefInfoStream"]);
    assert!(!info.exb_confirmed);
    let mut cfb = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    cfb.remove_stream("/*StyleStream").unwrap();
    cfb.create_storage("/*StyleStream").unwrap();
    let info = inspect_bytes(&cfb.into_inner().into_inner(), ParseOptions::default()).unwrap();
    assert_eq!(info.classification, "cfb");
    assert_eq!(info.missing_markers, ["/*StyleStream", "/*XrefInfoStream"]);
}

#[test]
fn non_cfb_and_malformed_cfb_are_distinct() {
    for (bytes, classification) in [
        (b"text".as_slice(), "unknown"),
        (b"\x10CaxaEbForWindows", "legacy_caxa_candidate"),
    ] {
        assert_eq!(
            inspect_bytes(bytes, ParseOptions::default())
                .unwrap()
                .classification,
            classification
        );
        assert!(matches!(
            Container::from_bytes(bytes, ParseOptions::default()),
            Err(Error::UnsupportedFeature(_))
        ));
    }
    let bytes = fixture(cfb::Version::V3);
    for size in [8, 32, 512, 1024] {
        assert!(matches!(
            Container::from_bytes(&bytes[..size], ParseOptions::default()),
            Err(Error::InvalidContainer(_))
        ));
    }
}

#[test]
fn all_resource_limits_are_enforced() {
    let bytes = fixture(cfb::Version::V3);
    let defaults = ParseOptions::default();
    for options in [
        ParseOptions {
            max_entries: 3,
            ..defaults
        },
        ParseOptions {
            max_stream_bytes: 8999,
            ..defaults
        },
        ParseOptions {
            max_total_stream_bytes: 9006,
            ..defaults
        },
        ParseOptions {
            max_storage_depth: 1,
            ..defaults
        },
        ParseOptions {
            max_container_read_bytes: 511,
            ..defaults
        },
    ] {
        assert!(matches!(
            Container::from_bytes(&bytes, options),
            Err(Error::ResourceLimit { .. })
        ));
    }
    assert!(Container::from_bytes(
        &bytes,
        ParseOptions {
            max_entries: 4,
            max_stream_bytes: 9000,
            max_total_stream_bytes: 9007,
            max_storage_depth: 2,
            ..defaults
        }
    )
    .is_ok());
}

#[test]
fn invalid_selectors_ranges_and_storage_are_rejected() {
    let mut container =
        Container::from_bytes(&fixture(cfb::Version::V3), ParseOptions::default()).unwrap();
    for path in [
        "large",
        "/",
        "/nested/../large",
        "/./large",
        "/nested\\small",
        "/large/",
        "//large",
        "/C:/large",
    ] {
        assert!(matches!(
            container.read_stream(path, 0, None),
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(matches!(
        container.read_stream("/nested", 0, None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        container.read_stream("/missing", 0, None),
        Err(Error::StreamNotFound(_))
    ));
    assert!(matches!(
        container.read_stream("/large", 9001, None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        container.read_stream("/large", 9000, Some(1)),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(container.read_stream("/large", 9000, Some(0)).unwrap(), b"");
    assert!(matches!(
        container.read_stream("/large", u64::MAX, Some(u64::MAX)),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn directory_and_fat_cycles_do_not_loop() {
    let original = fixture(cfb::Version::V3);
    let directory_sector = u32::from_le_bytes(original[48..52].try_into().unwrap()) as usize;
    let root = (directory_sector + 1) * 512;
    let mut bytes = original.clone();
    bytes[root + 76..root + 80].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        Container::from_bytes(&bytes, ParseOptions::default()),
        Err(Error::InvalidContainer(_))
    ));
    let fat_sector = u32::from_le_bytes(original[76..80].try_into().unwrap()) as usize;
    let position = (fat_sector + 1) * 512 + directory_sector * 4;
    let mut bytes = original;
    bytes[position..position + 4].copy_from_slice(&(directory_sector as u32).to_le_bytes());
    assert!(matches!(
        Container::from_bytes(&bytes, ParseOptions::default()),
        Err(Error::InvalidContainer(_))
    ));
}

#[test]
fn oversized_declared_stream_is_rejected_before_allocation() {
    let mut bytes = fixture(cfb::Version::V3);
    let directory_sector = u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize;
    let first_sector = (directory_sector + 1) * 512;
    bytes[first_sector + 2 * 128 + 120..first_sector + 2 * 128 + 124]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Container::from_bytes(&bytes, ParseOptions::default()),
        Err(Error::ResourceLimit {
            name: "max_stream_bytes",
            ..
        })
    ));
}

#[test]
fn short_stream_chain_fails_on_read_and_hash() {
    let mut bytes = fixture(cfb::Version::V3);
    let directory_sector = u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize;
    let large_entry = (directory_sector + 1) * 512 + 3 * 128;
    let first_stream_sector = u32::from_le_bytes(
        bytes[large_entry + 116..large_entry + 120]
            .try_into()
            .unwrap(),
    ) as usize;
    let fat_sector = u32::from_le_bytes(bytes[76..80].try_into().unwrap()) as usize;
    let next = (fat_sector + 1) * 512 + first_stream_sector * 4;
    bytes[next..next + 4].copy_from_slice(&0xffff_fffeu32.to_le_bytes());
    let mut container = Container::from_bytes(&bytes, ParseOptions::default()).unwrap();
    assert!(matches!(
        container.read_stream("/large", 0, None),
        Err(Error::InvalidContainer(_))
    ));
    assert!(matches!(
        container.stream_sha256("/large"),
        Err(Error::InvalidContainer(_))
    ));
}

#[test]
fn low_level_budget_bounds_reads_and_resets_between_operations() {
    let bytes = fixture(cfb::Version::V3);
    let options = ParseOptions {
        max_container_read_bytes: 8192,
        ..ParseOptions::default()
    };
    let mut container = Container::from_bytes(&bytes, options).unwrap();
    assert!(matches!(
        container.read_stream("/large", 0, None),
        Err(Error::ResourceLimit {
            name: "max_container_read_bytes",
            ..
        })
    ));
    assert!(matches!(
        container.stream_sha256("/large"),
        Err(Error::ResourceLimit {
            name: "max_container_read_bytes",
            ..
        })
    ));
    for _ in 0..4 {
        assert_eq!(
            container.read_stream("/nested/small", 0, None).unwrap(),
            b"hello\0\xff"
        );
    }
}

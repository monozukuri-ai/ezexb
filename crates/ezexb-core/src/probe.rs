use std::{fs::File, io::Read, path::Path};

use crate::{Error, ParseOptions};

const CFB_MAGIC: &[u8] = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1";
const LEGACY_MAGIC: &[u8] = b"\x10CaxaEbForWindows";

/// A signature observation only; neither variant confirms valid EXB contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    CompoundFile,
    LegacyCaxaCandidate,
    Unknown,
}

impl Signature {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompoundFile => "cfb",
            Self::LegacyCaxaCandidate => "legacy_caxa_candidate",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderInfo {
    pub signature: Signature,
    pub byte_length: u64,
}

fn probe_prefix(prefix: &[u8], byte_length: u64) -> HeaderInfo {
    let signature = if prefix.starts_with(CFB_MAGIC) {
        Signature::CompoundFile
    } else if prefix.starts_with(LEGACY_MAGIC) {
        Signature::LegacyCaxaCandidate
    } else {
        Signature::Unknown
    };
    HeaderInfo {
        signature,
        byte_length,
    }
}

pub fn probe_bytes(data: &[u8], options: ParseOptions) -> Result<HeaderInfo, Error> {
    options.check_size(data.len() as u64)?;
    Ok(probe_prefix(data, data.len() as u64))
}

/// Read at most 17 bytes from a regular file; no CFB directory traversal occurs.
pub fn probe_file(path: impl AsRef<Path>, options: ParseOptions) -> Result<HeaderInfo, Error> {
    // Check before opening, so a FIFO passed by mistake does not block the caller.
    if !path.as_ref().metadata()?.is_file() {
        return Err(Error::InvalidInput("probe requires a regular file"));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput("probe requires a regular file"));
    }
    options.check_size(metadata.len())?;
    let mut prefix = Vec::with_capacity(17);
    file.take(17).read_to_end(&mut prefix)?;
    Ok(probe_prefix(&prefix, metadata.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_signatures_are_unknown() {
        for magic in [CFB_MAGIC, LEGACY_MAGIC] {
            for end in 0..magic.len() {
                assert_eq!(
                    probe_bytes(&magic[..end], ParseOptions::default())
                        .unwrap()
                        .signature,
                    Signature::Unknown
                );
            }
        }
    }

    #[test]
    fn header_observations_do_not_claim_exb_validation() {
        assert_eq!(
            probe_bytes(CFB_MAGIC, ParseOptions::default())
                .unwrap()
                .signature,
            Signature::CompoundFile
        );
        assert_eq!(
            probe_bytes(LEGACY_MAGIC, ParseOptions::default())
                .unwrap()
                .signature,
            Signature::LegacyCaxaCandidate
        );
        assert_eq!(
            probe_bytes(b"a file named drawing.exb", ParseOptions::default())
                .unwrap()
                .signature,
            Signature::Unknown
        );
    }

    #[test]
    fn size_limit_is_inclusive_and_precedes_identification() {
        let options = ParseOptions {
            max_file_bytes: 8,
            ..ParseOptions::default()
        };
        assert!(probe_bytes(CFB_MAGIC, options).is_ok());
        assert!(matches!(
            probe_bytes(b"123456789", options),
            Err(Error::InputLimit {
                actual: 9,
                limit: 8
            })
        ));
    }

    #[test]
    fn limited_entity_reader_is_available() {
        assert!(crate::require_entity_reader().is_ok());
    }
}

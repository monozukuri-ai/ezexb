//! Bounded decoding of the supported EXB metadata layouts.
//! Field names describe observations, not an official EXB specification.

use crate::{Error, ParseOptions};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TextField {
    pub value: String,
    /// Includes the four-byte length and UTF-16 terminator.
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct UnknownField {
    pub span: SourceSpan,
    pub hex: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MetadataProperty {
    pub key: TextField,
    pub value: TextField,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileInfo {
    pub evidence: &'static str,
    pub stream_version: TextField,
    pub document_type: TextField,
    /// A stored product build string; it is not a file-format version guarantee.
    pub product_build: TextField,
    pub copyright: TextField,
    pub properties: Vec<MetadataProperty>,
    pub unknown_fields: Vec<UnknownField>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppRecord {
    pub span: SourceSpan,
    /// Sixteen bytes in stream order, without asserting GUID byte ordering.
    pub type_id_hex: String,
    pub id_raw: i32,
    pub owner_raw: i32,
    pub schema: u16,
    /// Ordered strings; application names and versions are not inferred by position.
    pub strings: Vec<TextField>,
    pub unknown_fields: Vec<UnknownField>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppInfo {
    pub evidence: &'static str,
    /// Offsets refer to the decompressed meaningful payload, not compressed bytes.
    pub stream_version: TextField,
    pub reserved: UnknownField,
    pub records: Vec<AppRecord>,
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidStream(message.into())
}

pub(crate) fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(data.len().saturating_mul(2));
    for byte in data {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

pub(crate) fn unknown(data: &[u8], offset: usize, length: usize) -> UnknownField {
    UnknownField {
        span: SourceSpan {
            offset: offset as u64,
            length: length as u64,
        },
        hex: hex(&data[offset..offset + length]),
    }
}

pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub offset: usize,
    pub options: ParseOptions,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], options: ParseOptions) -> Result<Self, Error> {
        ParseOptions::check_limit(
            "max_decoded_stream_bytes",
            data.len() as u64,
            options.max_decoded_stream_bytes,
        )?;
        Ok(Self {
            data,
            offset: 0,
            options,
        })
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
        let end = self
            .offset
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| invalid(format!("truncated field at offset {}", self.offset)))?;
        let result = &self.data[self.offset..end];
        self.offset = end;
        Ok(result)
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn text(&mut self) -> Result<TextField, Error> {
        let start = self.offset;
        let length = self.u32()? as usize;
        ParseOptions::check_limit(
            "max_string_bytes",
            length as u64,
            self.options.max_string_bytes,
        )?;
        if length < 2 || length % 2 != 0 {
            return Err(invalid(format!(
                "invalid UTF-16 byte length at offset {start}"
            )));
        }
        let bytes = self.take(length)?;
        if bytes[length - 2..] != [0, 0] {
            return Err(invalid(format!(
                "missing UTF-16 terminator at offset {start}"
            )));
        }
        let words: Vec<u16> = bytes[..length - 2]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if words.contains(&0) {
            return Err(invalid(format!(
                "embedded UTF-16 terminator at offset {start}"
            )));
        }
        let value = String::from_utf16(&words)
            .map_err(|_| invalid(format!("invalid UTF-16 at offset {start}")))?;
        Ok(TextField {
            value,
            span: SourceSpan {
                offset: start as u64,
                length: (4 + length) as u64,
            },
        })
    }

    pub fn version(&mut self) -> Result<TextField, Error> {
        let version = self.text()?;
        if version.value != "0.0" {
            return Err(invalid("unsupported metadata stream version"));
        }
        Ok(version)
    }
}

/// Decode the observed FileInfo layout, retaining all uninterpreted bytes.
pub fn decode_file_info(data: &[u8], options: ParseOptions) -> Result<FileInfo, Error> {
    let mut reader = Reader::new(data, options)?;
    let stream_version = reader.version()?;
    let document_type = reader.text()?;
    if document_type.value != "CaxaDraft Document" {
        return Err(invalid("unsupported FileInfo document type"));
    }
    let product_build = reader.text()?;
    let copyright = reader.text()?;
    let fixed_start = reader.offset;
    // Observed fixed section: opaque 16 bytes, numeric/date-shaped fields,
    // and 153 reserved bytes. Their semantics have not been established.
    reader.take(213)?;
    let opaque_start = reader.offset;
    reader.text()?;
    let opaque_end = reader.offset;
    let count = reader.u32()? as u64;
    ParseOptions::check_limit("max_records", count, options.max_records)?;
    // Each pair needs at least two u32 lengths and two UTF-16 NULs.
    if count > (data.len() - reader.offset) as u64 / 12 {
        return Err(invalid("property count exceeds remaining FileInfo bytes"));
    }
    let mut properties = Vec::new();
    for _ in 0..count {
        properties.push(MetadataProperty {
            key: reader.text()?,
            value: reader.text()?,
        });
    }
    let mut unknown_fields = vec![
        unknown(data, fixed_start, 213),
        unknown(data, opaque_start, opaque_end - opaque_start),
    ];
    if reader.offset < data.len() {
        unknown_fields.push(unknown(data, reader.offset, data.len() - reader.offset));
    }
    Ok(FileInfo {
        evidence: "corpus_consistent",
        stream_version,
        document_type,
        product_build,
        copyright,
        properties,
        unknown_fields,
    })
}

const TABLE_TYPE: &str = "183a0838a9c6b14ca10671bd42038a90";
const APP_TYPE: &str = "f9cf808cb6278e429a3d5eaf4e1cba51";

/// Decode AppInfo after the compression wrapper and allocation padding are removed.
pub fn decode_app_info(data: &[u8], options: ParseOptions) -> Result<AppInfo, Error> {
    let mut reader = Reader::new(data, options)?;
    let stream_version = reader.version()?;
    let reserved_start = reader.offset;
    reader.take(160)?;
    let reserved = unknown(data, reserved_start, 160);
    let mut records = Vec::new();
    while reader.offset < data.len() {
        ParseOptions::check_limit("max_records", records.len() as u64 + 1, options.max_records)?;
        let start = reader.offset;
        let type_id_hex = hex(reader.take(16)?);
        let id_raw = reader.u32()? as i32;
        let owner_raw = reader.u32()? as i32;
        let schema = reader.u16()?;
        // Other schemas can insert extensions before the length field. Their
        // boundary cannot be inferred from the schema-1 framing rule.
        if schema != 1 {
            return Err(invalid(format!(
                "unsupported AppInfo record schema {schema:#06x} at offset {start}"
            )));
        }
        let payload_length = reader.u32()? as usize;
        let payload_start = reader.offset;
        reader.take(payload_length)?;
        let end = reader.offset;
        let mut payload_reader = Reader::new(&data[..end], options)?;
        payload_reader.offset = payload_start;
        let mut strings = Vec::new();
        let mut unknown_fields = Vec::new();
        if schema == 1 && type_id_hex == APP_TYPE {
            for _ in 0..4 {
                strings.push(payload_reader.text()?);
            }
            if payload_reader.offset != end {
                return Err(invalid("unexpected AppInfo application payload length"));
            }
        } else if schema == 1 && type_id_hex == TABLE_TYPE {
            strings.push(payload_reader.text()?);
            if payload_reader.offset < end {
                unknown_fields.push(unknown(
                    data,
                    payload_reader.offset,
                    end - payload_reader.offset,
                ));
            }
        } else {
            unknown_fields.push(unknown(data, payload_start, payload_length));
        }
        records.push(AppRecord {
            span: SourceSpan {
                offset: start as u64,
                length: (end - start) as u64,
            },
            type_id_hex,
            id_raw,
            owner_raw,
            schema,
            strings,
            unknown_fields,
        });
    }
    Ok(AppInfo {
        evidence: "corpus_consistent",
        stream_version,
        reserved,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Vec<u8> {
        let mut words: Vec<u16> = value.encode_utf16().collect();
        words.push(0);
        let mut bytes = ((words.len() * 2) as u32).to_le_bytes().to_vec();
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
        bytes
    }

    fn file_info() -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [
            "0.0",
            "CaxaDraft Document",
            "synthetic-build",
            "synthetic copyright",
        ] {
            bytes.extend(text(value));
        }
        bytes.extend([0x42; 213]);
        bytes.extend(text("opaque"));
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(text("Author"));
        bytes.extend(text("日本語😀"));
        bytes.extend([7, 8, 9]);
        bytes
    }

    #[test]
    fn file_info_preserves_fields_and_unknown_ranges() {
        let bytes = file_info();
        let info = decode_file_info(&bytes, ParseOptions::default()).unwrap();
        assert_eq!(info.properties[0].value.value, "日本語😀");
        assert_eq!(info.unknown_fields[0].span.length, 213);
        assert_eq!(info.unknown_fields.last().unwrap().hex, "070809");
        let span = &info.properties[0].value.span;
        assert_eq!(
            &bytes[span.offset as usize..(span.offset + span.length) as usize],
            text("日本語😀")
        );
    }

    #[test]
    fn strings_reject_bad_length_terminators_utf16_and_limit() {
        for bytes in [
            vec![1, 0, 0, 0, 0],
            vec![2, 0, 0, 0, 1, 0],
            vec![4, 0, 0, 0, 0, 0xd8, 0, 0],
        ] {
            assert!(Reader::new(&bytes, ParseOptions::default())
                .unwrap()
                .text()
                .is_err());
        }
        let options = ParseOptions {
            max_string_bytes: 2,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_file_info(&file_info(), options),
            Err(Error::ResourceLimit {
                name: "max_string_bytes",
                ..
            })
        ));
    }

    #[test]
    fn unknown_app_record_is_preserved_and_bounds_checked() {
        let mut bytes = text("0.0");
        bytes.extend([0; 160]);
        bytes.extend([0x99; 16]);
        bytes.extend((-2i32).to_le_bytes());
        bytes.extend((-1i32).to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(3u32.to_le_bytes());
        bytes.extend([1, 2, 3]);
        let info = decode_app_info(&bytes, ParseOptions::default()).unwrap();
        assert_eq!(info.records[0].id_raw, -2);
        assert_eq!(info.records[0].unknown_fields[0].hex, "010203");
        let mut unsupported = bytes.clone();
        unsupported[196..198].copy_from_slice(&0x101u16.to_le_bytes());
        assert!(matches!(
            decode_app_info(&unsupported, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        assert!(decode_app_info(&bytes[..bytes.len() - 1], ParseOptions::default()).is_err());
        assert!(matches!(
            decode_app_info(
                &bytes,
                ParseOptions {
                    max_records: 0,
                    ..ParseOptions::default()
                }
            ),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
    }

    #[test]
    fn property_count_cannot_allocate_from_untrusted_count() {
        let mut bytes = file_info();
        let info = decode_file_info(&bytes, ParseOptions::default()).unwrap();
        let count_offset =
            (info.unknown_fields[1].span.offset + info.unknown_fields[1].span.length) as usize;
        bytes[count_offset..count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_file_info(&bytes, ParseOptions::default()),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
    }
}

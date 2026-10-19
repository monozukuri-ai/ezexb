//! Extraction of the embedded BMP in the observed version-0.0 preview layout.

use crate::metadata::{hex, invalid, unknown, Reader, SourceSpan, TextField, UnknownField};
use crate::{Error, ParseOptions};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct PreviewInfo {
    pub evidence: &'static str,
    pub stream_version: TextField,
    pub record_type_id_hex: String,
    pub bmp_span: SourceSpan,
    pub width: i32,
    /// A negative value denotes a top-down BMP; a positive value is bottom-up.
    pub height: i32,
    pub bits_per_pixel: u16,
    pub compression: u32,
    pub row_stride: u64,
    pub pixel_bytes: u64,
    pub unknown_fields: Vec<UnknownField>,
}

const PREVIEW_TYPE: &str = "f1cbe925b1be724cbcfe825088c48b21";
const PRELUDE: [u8; 20] = [
    2, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
];

/// Validate the known EXB wrapper and a 24-bit BI_RGB BMP, without decoding pixels.
pub fn decode_preview(data: &[u8], options: ParseOptions) -> Result<PreviewInfo, Error> {
    let mut reader = Reader::new(data, options)?;
    let stream_version = reader.version()?;
    let reserved_start = reader.offset;
    reader.take(160)?;
    let record_start = reader.offset;
    let record_type_id_hex = hex(reader.take(16)?);
    if record_type_id_hex != PREVIEW_TYPE {
        return Err(invalid("unsupported PreviewStream record type"));
    }
    reader.take(8)?;
    if reader.u16()? != 1 {
        return Err(invalid("unsupported PreviewStream record schema"));
    }
    let payload_length = reader.u32()? as usize;
    if payload_length != data.len() - reader.offset {
        return Err(invalid(
            "PreviewStream record length does not match remaining bytes",
        ));
    }
    let prelude_start = reader.offset;
    if reader.take(20)? != PRELUDE {
        return Err(invalid("unsupported PreviewStream bitmap prelude"));
    }
    let bitmap_length = reader.u32()? as usize;
    let bmp_start = reader.offset;
    if bitmap_length != data.len() - bmp_start {
        return Err(invalid(
            "PreviewStream bitmap length does not match remaining bytes",
        ));
    }
    if reader.take(2)? != b"BM" {
        return Err(invalid("missing BMP signature"));
    }
    if reader.u32()? as usize != bitmap_length {
        return Err(invalid("BMP file size differs from PreviewStream length"));
    }
    if reader.u32()? != 0 {
        return Err(invalid("BMP reserved fields must be zero"));
    }
    let pixel_offset = reader.u32()?;
    let dib_length = reader.u32()?;
    let width = reader.u32()? as i32;
    let height = reader.u32()? as i32;
    let planes = reader.u16()?;
    let bits_per_pixel = reader.u16()?;
    let compression = reader.u32()?;
    let declared_image_bytes = reader.u32()?;
    reader.take(8)?; // Resolution is not needed for extraction.
    let colors_used = reader.u32()?;
    reader.u32()?; // Important-color hint.
    if dib_length != 40
        || pixel_offset != 54
        || bits_per_pixel != 24
        || compression != 0
        || colors_used != 0
    {
        return Err(invalid(
            "unsupported preview BMP profile (requires 24-bit BI_RGB and 40-byte DIB header)",
        ));
    }
    if width <= 0 || height == 0 || planes != 1 {
        return Err(invalid("invalid preview BMP dimensions or plane count"));
    }
    let rows = (height as i64).unsigned_abs();
    let pixels = (width as u64) * rows;
    ParseOptions::check_limit("max_preview_pixels", pixels, options.max_preview_pixels)?;
    let row_stride = (width as u64 * 24).div_ceil(32) * 4;
    let pixel_bytes = row_stride * rows;
    if pixel_offset as u64 + pixel_bytes != bitmap_length as u64 {
        return Err(invalid("BMP pixel extent differs from declared file size"));
    }
    if declared_image_bytes != 0 && declared_image_bytes as u64 != pixel_bytes {
        return Err(invalid("BMP image size differs from computed pixel extent"));
    }
    Ok(PreviewInfo {
        evidence: "corpus_consistent",
        stream_version,
        record_type_id_hex,
        bmp_span: SourceSpan {
            offset: bmp_start as u64,
            length: bitmap_length as u64,
        },
        width,
        height,
        bits_per_pixel,
        compression,
        row_stride,
        pixel_bytes,
        unknown_fields: vec![
            unknown(data, reserved_start, 160),
            unknown(data, record_start + 16, 8),
            unknown(data, prelude_start, 20),
        ],
    })
}

/// Return the original embedded BMP bytes, with no EXB wrapper or image re-encoding.
pub fn extract_preview_bmp(data: &[u8], options: ParseOptions) -> Result<Vec<u8>, Error> {
    let info = decode_preview(data, options)?;
    let start = info.bmp_span.offset as usize;
    let end = start + info.bmp_span.length as usize;
    Ok(data[start..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> Vec<u8> {
        // Independent 2x1 synthetic bitmap, with two row-padding bytes.
        let mut bmp = b"BM".to_vec();
        bmp.extend(62u32.to_le_bytes());
        bmp.extend(0u32.to_le_bytes());
        bmp.extend(54u32.to_le_bytes());
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(2i32.to_le_bytes());
        bmp.extend(1i32.to_le_bytes());
        bmp.extend(1u16.to_le_bytes());
        bmp.extend(24u16.to_le_bytes());
        bmp.extend(0u32.to_le_bytes());
        bmp.extend(8u32.to_le_bytes());
        bmp.extend([0; 16]);
        bmp.extend([0, 0, 255, 0, 255, 0, 0, 0]);
        let mut bytes = vec![8, 0, 0, 0, b'0', 0, b'.', 0, b'0', 0, 0, 0];
        bytes.extend([0; 160]);
        bytes.extend([
            0xf1, 0xcb, 0xe9, 0x25, 0xb1, 0xbe, 0x72, 0x4c, 0xbc, 0xfe, 0x82, 0x50, 0x88, 0xc4,
            0x8b, 0x21,
        ]);
        bytes.extend([0; 8]);
        bytes.extend(1u16.to_le_bytes());
        bytes.extend((24u32 + bmp.len() as u32).to_le_bytes());
        bytes.extend(PRELUDE);
        bytes.extend((bmp.len() as u32).to_le_bytes());
        bytes.extend(bmp);
        bytes
    }

    #[test]
    fn extracts_exact_bmp_and_handles_stride_and_top_down() {
        let mut bytes = preview();
        let info = decode_preview(&bytes, ParseOptions::default()).unwrap();
        assert_eq!(
            (info.width, info.height, info.row_stride, info.pixel_bytes),
            (2, 1, 8, 8)
        );
        assert_eq!(
            extract_preview_bmp(&bytes, ParseOptions::default()).unwrap(),
            bytes[226..]
        );
        bytes[248..252].copy_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(
            decode_preview(&bytes, ParseOptions::default())
                .unwrap()
                .height,
            -1
        );
    }

    #[test]
    fn rejects_truncation_size_mismatch_and_unsupported_profile() {
        let bytes = preview();
        for length in [0, 12, 171, 201, 225, 250, bytes.len() - 1] {
            assert!(decode_preview(&bytes[..length], ParseOptions::default()).is_err());
        }
        for offset in [198, 222, 228, 236, 240, 244, 252, 254, 256, 260] {
            let mut broken = bytes.clone();
            broken[offset] ^= 1;
            assert!(
                decode_preview(&broken, ParseOptions::default()).is_err(),
                "offset={offset}"
            );
        }
    }

    #[test]
    fn rejects_large_dimensions_before_pixel_allocation() {
        let mut bytes = preview();
        bytes[244..248].copy_from_slice(&i32::MAX.to_le_bytes());
        bytes[248..252].copy_from_slice(&i32::MIN.to_le_bytes());
        assert!(matches!(
            decode_preview(&bytes, ParseOptions::default()),
            Err(Error::ResourceLimit {
                name: "max_preview_pixels",
                ..
            })
        ));
    }
}

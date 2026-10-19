//! Bounded decoding of the chunked zlib envelope observed in EXB CFB streams.
//!
//! This decodes bytes only; it does not validate the records inside the payload.

use flate2::{Decompress, FlushDecompress, Status};
use serde::Serialize;

use crate::{Error, ParseOptions};

const MAGIC: &[u8; 4] = b"\x00\xde\xe3\x3d";
const BLOCK_CAPACITY: u64 = 262_144;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StreamPayloadBlock {
    /// Offset of the compressed bytes in the original stream, after its u32 length.
    pub stream_offset: u64,
    pub compressed_size: u64,
    pub decoded_size: u64,
    /// Offset in the concatenated, decompressed logical payload.
    pub logical_offset: u64,
    /// Meaningful bytes in this block, excluding final zero padding.
    pub logical_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StreamPayloadInfo {
    /// `raw` or the observed `zlib_blocks` envelope.
    pub encoding: String,
    pub logical_size: u64,
    /// Full decompressed allocation, including padding; bounded before inflation.
    pub allocated_size: u64,
    pub blocks: Vec<StreamPayloadBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecodedStream {
    pub info: StreamPayloadInfo,
    /// Meaningful bytes only; envelope padding is checked and removed.
    pub data: Vec<u8>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidStream(message.into())
}

fn allocate_zeroed(length: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| invalid("cannot allocate decoded stream buffer"))?;
    bytes.resize(length, 0);
    Ok(bytes)
}

/// Decode the observed EXB chunked-zlib envelope, or copy an unwrapped stream.
///
/// Both stored input and allocated decompressed output obey `ParseOptions`.
/// Wrapped streams must have the observed 256 KiB block capacity, valid zlib
/// checksums, exactly the required number of full blocks, no trailing bytes,
/// and zero padding beyond the declared logical length. Unsupported framing is
/// rejected rather than returning a partial or guessed payload.
pub fn decode_stream_payload(data: &[u8], options: ParseOptions) -> Result<DecodedStream, Error> {
    ParseOptions::check_limit(
        "max_stream_bytes",
        data.len() as u64,
        options.max_stream_bytes,
    )?;
    if !data.starts_with(MAGIC) {
        ParseOptions::check_limit(
            "max_decoded_stream_bytes",
            data.len() as u64,
            options.max_decoded_stream_bytes,
        )?;
        let mut decoded = allocate_zeroed(data.len())?;
        decoded.copy_from_slice(data);
        return Ok(DecodedStream {
            info: StreamPayloadInfo {
                encoding: "raw".to_owned(),
                logical_size: data.len() as u64,
                allocated_size: data.len() as u64,
                blocks: Vec::new(),
            },
            data: decoded,
        });
    }
    if data.len() < 16 {
        return Err(invalid("truncated zlib-block header"));
    }
    let capacity = u32::from_le_bytes(data[4..8].try_into().expect("checked header")) as u64;
    if capacity != BLOCK_CAPACITY {
        return Err(invalid(format!(
            "unsupported zlib-block capacity {capacity}; expected {BLOCK_CAPACITY}"
        )));
    }
    let logical_size = u64::from_le_bytes(data[8..16].try_into().expect("checked header"));
    let block_count = logical_size / capacity + u64::from(logical_size % capacity != 0);
    ParseOptions::check_limit("max_records", block_count, options.max_records)?;
    let allocated_size = block_count
        .checked_mul(capacity)
        .ok_or_else(|| invalid("decoded allocation size overflows u64"))?;
    ParseOptions::check_limit(
        "max_decoded_stream_bytes",
        allocated_size,
        options.max_decoded_stream_bytes,
    )?;
    let allocated = usize::try_from(allocated_size)
        .map_err(|_| invalid("decoded allocation does not fit this platform"))?;
    let logical = usize::try_from(logical_size)
        .map_err(|_| invalid("logical size does not fit this platform"))?;
    let mut decoded = allocate_zeroed(allocated)?;
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(block_count as usize)
        .map_err(|_| invalid("cannot allocate stream block index"))?;
    let mut offset = 16usize;
    for index in 0..block_count {
        let length_bytes = data
            .get(offset..offset.saturating_add(4))
            .ok_or_else(|| invalid("truncated compressed-block length"))?;
        let compressed_size = u32::from_le_bytes(length_bytes.try_into().expect("four bytes"));
        offset += 4;
        let end = offset
            .checked_add(compressed_size as usize)
            .ok_or_else(|| invalid("compressed-block end overflows this platform"))?;
        let compressed = data
            .get(offset..end)
            .ok_or_else(|| invalid("compressed block extends beyond stream"))?;
        let logical_offset = index * capacity;
        let output_start = logical_offset as usize;
        let output_end = output_start + capacity as usize;
        let mut inflater = Decompress::new(true);
        let status = inflater
            .decompress(
                compressed,
                &mut decoded[output_start..output_end],
                FlushDecompress::Finish,
            )
            .map_err(|error| invalid(format!("invalid zlib block {index}: {error}")))?;
        if status != Status::StreamEnd
            || inflater.total_in() != compressed_size as u64
            || inflater.total_out() != capacity
        {
            return Err(invalid(format!(
                "zlib block {index} must consume its exact compressed length and decode {capacity} bytes"
            )));
        }
        blocks.push(StreamPayloadBlock {
            stream_offset: offset as u64,
            compressed_size: compressed_size as u64,
            decoded_size: capacity,
            logical_offset,
            logical_size: (logical_size - logical_offset).min(capacity),
        });
        offset = end;
    }
    if offset != data.len() {
        return Err(invalid("trailing bytes after the declared zlib blocks"));
    }
    if decoded[logical..].iter().any(|&byte| byte != 0) {
        return Err(invalid("nonzero bytes in final block padding"));
    }
    decoded.truncate(logical);
    Ok(DecodedStream {
        info: StreamPayloadInfo {
            encoding: "zlib_blocks".to_owned(),
            logical_size,
            allocated_size,
            blocks,
        },
        data: decoded,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{write::ZlibEncoder, Compression};

    use super::*;

    fn compressed(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn envelope(logical: &[u8]) -> Vec<u8> {
        let mut result = MAGIC.to_vec();
        result.extend_from_slice(&(BLOCK_CAPACITY as u32).to_le_bytes());
        result.extend_from_slice(&(logical.len() as u64).to_le_bytes());
        for chunk in logical.chunks(BLOCK_CAPACITY as usize) {
            let mut padded = vec![0; BLOCK_CAPACITY as usize];
            padded[..chunk.len()].copy_from_slice(chunk);
            let packed = compressed(&padded);
            result.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            result.extend_from_slice(&packed);
        }
        result
    }

    #[test]
    fn raw_payload_retains_bytes_and_enforces_both_limits() {
        let result = decode_stream_payload(b"unwrapped", ParseOptions::default()).unwrap();
        assert_eq!(result.data, b"unwrapped");
        assert_eq!(result.info.encoding, "raw");
        assert_eq!(result.info.allocated_size, 9);
        assert!(result.info.blocks.is_empty());
        for options in [
            ParseOptions {
                max_stream_bytes: 8,
                ..ParseOptions::default()
            },
            ParseOptions {
                max_decoded_stream_bytes: 8,
                ..ParseOptions::default()
            },
        ] {
            assert!(matches!(
                decode_stream_payload(b"unwrapped", options),
                Err(Error::ResourceLimit { .. })
            ));
        }
    }

    #[test]
    fn multiblock_payload_preserves_boundary_and_source_offsets() {
        let mut payload = vec![0x41; BLOCK_CAPACITY as usize + 11];
        payload[BLOCK_CAPACITY as usize..].copy_from_slice(b"second part");
        let bytes = envelope(&payload);
        let result = decode_stream_payload(&bytes, ParseOptions::default()).unwrap();
        assert_eq!(result.data, payload);
        assert_eq!(result.info.encoding, "zlib_blocks");
        assert_eq!(result.info.allocated_size, 2 * BLOCK_CAPACITY);
        assert_eq!(result.info.blocks.len(), 2);
        assert_eq!(result.info.blocks[0].stream_offset, 20);
        assert_eq!(
            result.info.blocks[1].stream_offset,
            24 + result.info.blocks[0].compressed_size
        );
        assert_eq!(result.info.blocks[1].logical_offset, BLOCK_CAPACITY);
        assert_eq!(result.info.blocks[1].logical_size, 11);
    }

    #[test]
    fn allocation_budget_checks_full_padded_size_before_inflation() {
        let bytes = envelope(b"small");
        let options = ParseOptions {
            max_decoded_stream_bytes: BLOCK_CAPACITY - 1,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_stream_payload(&bytes, options),
            Err(Error::ResourceLimit {
                name: "max_decoded_stream_bytes",
                actual: BLOCK_CAPACITY,
                ..
            })
        ));
        let options = ParseOptions {
            max_records: 0,
            ..ParseOptions::default()
        };
        assert!(matches!(
            decode_stream_payload(&bytes, options),
            Err(Error::ResourceLimit {
                name: "max_records",
                ..
            })
        ));
    }

    #[test]
    fn invalid_framing_and_trailing_data_are_rejected() {
        let valid = envelope(b"test");
        let mut cases = vec![MAGIC.to_vec(), valid[..valid.len() - 1].to_vec()];
        let mut capacity = valid.clone();
        capacity[4..8].copy_from_slice(&1024u32.to_le_bytes());
        cases.push(capacity);
        let mut trailing = valid.clone();
        trailing.push(0);
        cases.push(trailing);
        let mut oversized_length = valid.clone();
        oversized_length[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(oversized_length);
        let mut incorrect_logical_size = valid.clone();
        incorrect_logical_size[8..16].copy_from_slice(&(BLOCK_CAPACITY + 1).to_le_bytes());
        cases.push(incorrect_logical_size);
        for bytes in cases {
            assert!(matches!(
                decode_stream_payload(&bytes, ParseOptions::default()),
                Err(Error::InvalidStream(_))
            ));
        }
    }

    #[test]
    fn checksum_and_exact_zlib_block_consumption_are_required() {
        let valid = envelope(b"test");
        let mut checksum = valid.clone();
        *checksum.last_mut().unwrap() ^= 1;
        assert!(matches!(
            decode_stream_payload(&checksum, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
        let mut extra_inside_block = valid.clone();
        extra_inside_block.push(0);
        let length = (extra_inside_block.len() - 20) as u32;
        extra_inside_block[16..20].copy_from_slice(&length.to_le_bytes());
        assert!(matches!(
            decode_stream_payload(&extra_inside_block, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
    }

    #[test]
    fn wrong_decompressed_size_and_nonzero_padding_are_rejected() {
        for length in [BLOCK_CAPACITY as usize - 1, BLOCK_CAPACITY as usize + 1] {
            let packed = compressed(&vec![0; length]);
            let mut bytes = envelope(b"a")[..16].to_vec();
            bytes.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&packed);
            assert!(matches!(
                decode_stream_payload(&bytes, ParseOptions::default()),
                Err(Error::InvalidStream(_))
            ));
        }
        let mut nonzero_padding = envelope(b"abcd");
        nonzero_padding[8..16].copy_from_slice(&3u64.to_le_bytes());
        assert!(matches!(
            decode_stream_payload(&nonzero_padding, ParseOptions::default()),
            Err(Error::InvalidStream(_))
        ));
    }

    #[test]
    fn empty_and_exact_capacity_payloads_have_no_padding_ambiguity() {
        for payload in [vec![], vec![7; BLOCK_CAPACITY as usize]] {
            let result =
                decode_stream_payload(&envelope(&payload), ParseOptions::default()).unwrap();
            assert_eq!(result.data, payload);
            assert_eq!(result.info.logical_size, result.info.allocated_size);
        }
    }
}

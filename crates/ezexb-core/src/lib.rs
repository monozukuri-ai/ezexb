//! EXB parser foundations. Header observations do not validate a drawing.
#![forbid(unsafe_code)]

mod annotation;
mod attributes;
mod bom;
mod build;
mod complex;
mod compression;
mod container;
mod context;
mod document;
mod geometry;
mod lines;
mod metadata;
mod partno;
mod preview;
mod records;
mod render;
mod table;
mod text_style;
mod view2d;
mod world;

#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod document_tests;
mod error;
mod probe;
#[cfg(test)]
#[path = "../examples/support/remaining_fixture.rs"]
mod remaining_test_fixture;
#[cfg(test)]
use remaining_test_fixture::base as test_fixture;

pub use annotation::{
    AnnotationEntity, AnnotationGeometry, PolylineGeometry, PolylineVertex, TextGeometry,
};
pub use attributes::*;
pub use bom::*;
pub use build::*;
pub use complex::*;
pub use compression::{
    decode_stream_payload, DecodedStream, StreamPayloadBlock, StreamPayloadInfo,
};
pub use container::{inspect_bytes, inspect_file, Container, EntryInfo, EntryKind, Inspection};
pub use context::*;
pub use document::*;
pub use geometry::{EntityAttributes, PrimitiveEntity, PrimitiveGeometry};
pub use lines::{
    inspect_lines, inspect_lines_with_profile, LineAttributes, LineInspection, LineRecord,
    UnsupportedLine,
};
pub use metadata::*;
pub use partno::*;
pub use preview::*;
pub use records::{
    inspect_record_headers, inspect_record_headers_with_profile, RecordDisplayCache,
    RecordExtension, RecordHeader, RecordInspection, RecordProfile, RecordPropertyGroup,
    RecordPropertyValue, RecordRelation,
};
pub use render::*;
pub use table::*;
pub use text_style::*;
pub use view2d::*;
pub use world::*;

pub use error::Error;
pub use probe::{probe_bytes, probe_file, HeaderInfo, Signature};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Limits implemented by the current API. Further decoder limits come with decoding.
#[derive(Clone, Copy, Debug)]
pub struct ParseOptions {
    pub max_file_bytes: u64,
    pub max_entries: u64,
    pub max_stream_bytes: u64,
    pub max_total_stream_bytes: u64,
    pub max_storage_depth: u64,
    /// Low-level CFB bytes read per open/read/hash operation, including repeated reads.
    pub max_container_read_bytes: u64,
    /// Per-stream decompressed allocation, including padded zlib blocks.
    pub max_decoded_stream_bytes: u64,
    pub max_string_bytes: u64,
    pub max_preview_pixels: u64,
    /// Per-operation cap; record walks cumulatively count records, property groups,
    /// values, relations, and relation-array entries. Compression has its own count.
    pub max_records: u64,
    /// Nested display-record depth. Top-level is zero; a hard ceiling of 64
    /// remains effective even when this configurable limit is raised.
    pub max_record_depth: u64,
    /// Cumulative decoded bytes retained/processed by one document read.
    pub max_total_decoded_bytes: u64,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_entries: 10_000,
            max_stream_bytes: 64 * 1024 * 1024,
            max_total_stream_bytes: 256 * 1024 * 1024,
            max_storage_depth: 64,
            max_container_read_bytes: 64 * 1024 * 1024,
            max_decoded_stream_bytes: 64 * 1024 * 1024,
            max_string_bytes: 64 * 1024,
            max_preview_pixels: 16 * 1024 * 1024,
            max_records: 100_000,
            max_record_depth: 32,
            max_total_decoded_bytes: 256 * 1024 * 1024,
        }
    }
}

impl ParseOptions {
    pub(crate) fn check_limit(name: &'static str, actual: u64, limit: u64) -> Result<(), Error> {
        if actual > limit {
            return Err(Error::ResourceLimit {
                name,
                actual,
                limit,
            });
        }
        Ok(())
    }
    pub(crate) fn check_size(self, actual: u64) -> Result<(), Error> {
        if actual > self.max_file_bytes {
            return Err(Error::InputLimit {
                actual,
                limit: self.max_file_bytes,
            });
        }
        Ok(())
    }
}

/// Compatibility capability check. Reading still requires an observed profile.
pub fn require_entity_reader() -> Result<(), Error> {
    Ok(())
}

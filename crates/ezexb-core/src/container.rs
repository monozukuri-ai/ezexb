use std::{
    fs::File,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{probe_bytes, Error, ParseOptions, Signature};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Storage,
    Stream,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryInfo {
    /// Absolute CFB path with forward slashes on every OS. Not a host file path.
    pub path: String,
    pub kind: EntryKind,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inspection {
    pub signature: String,
    /// unknown, legacy_caxa_candidate, cfb, or cfb_exb_candidate.
    pub classification: String,
    pub byte_length: u64,
    pub cfb_version: Option<u16>,
    pub sector_size: Option<u32>,
    /// Always false: EXB record/geometry semantics are not validated.
    pub exb_confirmed: bool,
    pub entries: Vec<EntryInfo>,
    pub total_stream_bytes: u64,
    pub matched_markers: Vec<String>,
    pub missing_markers: Vec<String>,
    pub diagnostics: Vec<String>,
}

const MARKERS: &[(&str, EntryKind)] = &[
    ("/*FileInfoStream", EntryKind::Stream),
    ("/*AppInfoStream", EntryKind::Stream),
    ("/*SpaceStream", EntryKind::Stream),
    ("/*StyleStream", EntryKind::Stream),
    ("/*DBSelfData", EntryKind::Stream),
    ("/*DictionaryStream", EntryKind::Stream),
    ("/*XrefInfoStream", EntryKind::Stream),
    ("/*BlockStg", EntryKind::Storage),
    ("/*BlockStg/*BlockMgrStream", EntryKind::Stream),
];

/// A bounded read-only snapshot. No stream is resolved to an external file.
pub struct Container {
    compound: cfb::CompoundFile<BudgetCursor>,
    info: Inspection,
    options: ParseOptions,
    budget: Arc<Budget>,
}

struct Budget {
    remaining: AtomicU64,
    exhausted: AtomicBool,
}

struct BudgetCursor {
    cursor: Cursor<Vec<u8>>,
    budget: Arc<Budget>,
}

impl Read for BudgetCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = (self.cursor.get_ref().len() as u64).saturating_sub(self.cursor.position());
        let requested = available.min(buf.len() as u64);
        let remaining = self.budget.remaining.load(Ordering::Relaxed);
        if requested > remaining {
            self.budget.exhausted.store(true, Ordering::Relaxed);
            return Err(io::Error::other("CFB read budget exhausted"));
        }
        let count = self.cursor.read(buf)?;
        self.budget
            .remaining
            .fetch_sub(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}

impl Seek for BudgetCursor {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.cursor.seek(pos)
    }
}

fn map_cfb_error(error: io::Error, budget: &Budget, options: ParseOptions) -> Error {
    if budget.exhausted.load(Ordering::Relaxed) {
        Error::ResourceLimit {
            name: "max_container_read_bytes",
            actual: options.max_container_read_bytes.saturating_add(1),
            limit: options.max_container_read_bytes,
        }
    } else {
        Error::InvalidContainer(error.to_string())
    }
}

fn portable_path(path: &Path) -> String {
    let mut result = String::new();
    for part in path.components() {
        if let Component::Normal(name) = part {
            result.push('/');
            result.push_str(&name.to_string_lossy());
        }
    }
    result
}

fn validate_selector(path: &str) -> Result<(), Error> {
    if !path.starts_with('/')
        || path.len() == 1
        || path.contains(['\\', '\0', ':'])
        || path[1..]
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::InvalidInput(
            "stream path must be an absolute CFB path using / without empty, . or .. components",
        ));
    }
    Ok(())
}

fn load_file(path: &Path, options: ParseOptions) -> Result<Vec<u8>, Error> {
    if !path.metadata()?.is_file() {
        return Err(Error::InvalidInput("requires a regular file"));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput("requires a regular file"));
    }
    options.check_size(metadata.len())?;
    // take also bounds a file that grows after the metadata check.
    let mut data = Vec::new();
    file.take(options.max_file_bytes.saturating_add(1))
        .read_to_end(&mut data)?;
    options.check_size(data.len() as u64)?;
    Ok(data)
}

fn header_inspection(data: &[u8], options: ParseOptions) -> Result<Inspection, Error> {
    let header = probe_bytes(data, options)?;
    Ok(Inspection {
        signature: header.signature.as_str().into(),
        classification: header.signature.as_str().into(),
        byte_length: header.byte_length,
        cfb_version: None,
        sector_size: None,
        exb_confirmed: false,
        entries: Vec::new(),
        total_stream_bytes: 0,
        matched_markers: Vec::new(),
        missing_markers: Vec::new(),
        diagnostics: vec!["Directory inspection does not validate drawing entities.".into()],
    })
}

impl Container {
    pub fn open(path: impl AsRef<Path>, options: ParseOptions) -> Result<Self, Error> {
        Self::from_owned(load_file(path.as_ref(), options)?, options)
    }

    pub fn from_bytes(data: &[u8], options: ParseOptions) -> Result<Self, Error> {
        options.check_size(data.len() as u64)?;
        Self::from_owned(data.to_vec(), options)
    }

    fn from_owned(data: Vec<u8>, options: ParseOptions) -> Result<Self, Error> {
        let mut info = header_inspection(&data, options)?;
        if info.signature != "cfb" {
            return Err(Error::UnsupportedFeature(
                "stream inspection for non-CFB input",
            ));
        }
        let budget = Arc::new(Budget {
            remaining: AtomicU64::new(options.max_container_read_bytes),
            exhausted: AtomicBool::new(false),
        });
        let cursor = BudgetCursor {
            cursor: Cursor::new(data),
            budget: Arc::clone(&budget),
        };
        let compound = cfb::OpenOptions::new()
            .strict()
            .max_buffer_size(64 * 1024)
            .open_with(cursor)
            .map_err(|e| map_cfb_error(e, &budget, options))?;
        info.cfb_version = Some(compound.version().number());
        info.sector_size = Some(if info.cfb_version == Some(3) {
            512
        } else {
            4096
        });
        for entry in compound.walk().skip(1) {
            ParseOptions::check_limit(
                "max_entries",
                info.entries.len() as u64 + 1,
                options.max_entries,
            )?;
            let path = portable_path(entry.path());
            ParseOptions::check_limit(
                "max_storage_depth",
                path.matches('/').count() as u64,
                options.max_storage_depth,
            )?;
            let size_bytes = if entry.is_stream() {
                let size = entry.len();
                ParseOptions::check_limit("max_stream_bytes", size, options.max_stream_bytes)?;
                if size > info.byte_length {
                    return Err(Error::InvalidContainer(format!(
                        "stream {path:?} length exceeds file size"
                    )));
                }
                let total = info
                    .total_stream_bytes
                    .checked_add(size)
                    .ok_or(Error::InvalidInput("total stream length overflow"))?;
                ParseOptions::check_limit(
                    "max_total_stream_bytes",
                    total,
                    options.max_total_stream_bytes,
                )?;
                info.total_stream_bytes = total;
                Some(size)
            } else {
                None
            };
            info.entries.push(EntryInfo {
                path,
                size_bytes,
                kind: if entry.is_stream() {
                    EntryKind::Stream
                } else {
                    EntryKind::Storage
                },
            });
        }
        info.entries.sort_by(|a, b| a.path.cmp(&b.path));
        for &(path, kind) in MARKERS {
            if info
                .entries
                .iter()
                .any(|entry| entry.kind == kind && entry.path.eq_ignore_ascii_case(path))
            {
                info.matched_markers.push(path.into());
            } else {
                info.missing_markers.push(path.into());
            }
        }
        if info
            .missing_markers
            .iter()
            .all(|path| path == "/*XrefInfoStream")
        {
            info.classification = "cfb_exb_candidate".into();
            info.diagnostics.push("EXB marker layout matched; proprietary stream contents and save version are unvalidated.".into());
            if !info.missing_markers.is_empty() {
                info.diagnostics
                    .push("Optional XrefInfoStream is absent in this marker layout.".into());
            }
        } else {
            info.diagnostics.push(
                "EXB marker layout did not match; generic CFB or an unrecognized EXB variant."
                    .into(),
            );
        }
        info.diagnostics.push(
            "Directory inspected; stream chains are checked when read, not by listing alone."
                .into(),
        );
        Ok(Self {
            compound,
            info,
            options,
            budget,
        })
    }

    pub fn info(&self) -> &Inspection {
        &self.info
    }

    pub(crate) fn options(&self) -> ParseOptions {
        self.options
    }

    /// Read the supported stored-geometry profile. Strict mode rejects any
    /// unsupported required object or unresolved reference.
    pub fn document(&mut self, strict: bool) -> Result<crate::Document, Error> {
        crate::document::read_container(self, strict)
    }

    /// Decode the observed block-compressed envelope, or return raw bytes unchanged.
    pub fn stream_payload(&mut self, path: &str) -> Result<crate::DecodedStream, Error> {
        let bytes = self.read_stream(path, 0, None)?;
        crate::decode_stream_payload(&bytes, self.options)
    }

    pub fn file_info(&mut self) -> Result<crate::FileInfo, Error> {
        let bytes = self.read_stream("/*FileInfoStream", 0, None)?;
        crate::decode_file_info(&bytes, self.options)
    }

    pub fn app_info(&mut self) -> Result<crate::AppInfo, Error> {
        let payload = self.stream_payload("/*AppInfoStream")?;
        crate::decode_app_info(&payload.data, self.options)
    }

    pub fn preview_info(&mut self) -> Result<crate::PreviewInfo, Error> {
        let bytes = self.read_stream("/*PreviewStream", 0, None)?;
        crate::decode_preview(&bytes, self.options)
    }

    pub fn preview_bmp(&mut self) -> Result<Vec<u8>, Error> {
        let bytes = self.read_stream("/*PreviewStream", 0, None)?;
        crate::extract_preview_bmp(&bytes, self.options)
    }

    /// Inspect a contiguous prefix of simple block records; unknown schemas stop parsing.
    pub fn record_headers(&mut self, path: &str) -> Result<crate::RecordInspection, Error> {
        if !path.starts_with("/*BlockStg/*Blk_") && path != "/*BlockStg/*GBlock" {
            return Err(Error::InvalidInput(
                "record inspection currently requires a block stream",
            ));
        }
        let payload = self.stream_payload(path)?;
        let profile = self.record_profile()?;
        crate::inspect_record_headers_with_profile(&payload.data, self.options, profile)
    }

    /// Inspect the experimental 81-byte LINE layout in a block stream prefix.
    pub fn lines(&mut self, path: &str) -> Result<crate::LineInspection, Error> {
        if !path.starts_with("/*BlockStg/*Blk_") && path != "/*BlockStg/*GBlock" {
            return Err(Error::InvalidInput(
                "LINE inspection requires a block stream",
            ));
        }
        let payload = self.stream_payload(path)?;
        let profile = self.record_profile()?;
        crate::inspect_lines_with_profile(&payload.data, self.options, profile)
    }

    fn record_profile(&mut self) -> Result<Option<crate::RecordProfile>, Error> {
        match self.file_info() {
            Ok(info) => Ok(crate::RecordProfile::from_product_build(
                &info.product_build.value,
            )),
            // Metadata is optional for the previously supported record prefix.
            // An unavailable profile keeps schema 3 stopped, rather than guessing.
            Err(
                Error::StreamNotFound(_) | Error::InvalidStream(_) | Error::UnsupportedFeature(_),
            ) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn reset_budget(&self) {
        self.budget
            .remaining
            .store(self.options.max_container_read_bytes, Ordering::Relaxed);
        self.budget.exhausted.store(false, Ordering::Relaxed);
    }

    fn stream_entry(&self, path: &str) -> Result<&EntryInfo, Error> {
        validate_selector(path)?;
        // Select the exact spelling returned by the listing; this is OS independent.
        match self.info.entries.iter().find(|entry| entry.path == path) {
            Some(entry) if entry.kind == EntryKind::Stream => Ok(entry),
            Some(_) => Err(Error::InvalidInput(
                "selected CFB path is a storage, not a stream",
            )),
            None => Err(Error::StreamNotFound(path.into())),
        }
    }

    /// Read a bounded range. With length=None, return the remaining bytes.
    /// A range read does not certify parts of the stream outside that range.
    pub fn read_stream(
        &mut self,
        path: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<Vec<u8>, Error> {
        let size = self.stream_entry(path)?.size_bytes.unwrap_or(0);
        let remaining = size
            .checked_sub(offset)
            .ok_or(Error::InvalidInput("offset exceeds stream size"))?;
        let count = length.unwrap_or(remaining);
        if count > remaining {
            return Err(Error::InvalidInput("requested range exceeds stream size"));
        }
        ParseOptions::check_limit(
            "max_container_read_bytes",
            count,
            self.options.max_container_read_bytes,
        )?;
        let count_usize = usize::try_from(count)
            .map_err(|_| Error::InvalidInput("stream range exceeds address space"))?;
        self.reset_budget();
        let mut stream = self
            .compound
            .open_stream(path)
            .map_err(|e| map_cfb_error(e, &self.budget, self.options))?;
        stream
            .seek(SeekFrom::Start(offset))
            .map_err(|e| map_cfb_error(e, &self.budget, self.options))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count_usize)
            .map_err(|_| Error::InvalidInput("unable to allocate stream range"))?;
        stream
            .take(count)
            .read_to_end(&mut bytes)
            .map_err(|e| map_cfb_error(e, &self.budget, self.options))?;
        if bytes.len() != count_usize {
            return Err(Error::InvalidContainer("truncated stream".into()));
        }
        Ok(bytes)
    }

    /// Hash all declared stream bytes without allocating a whole-stream buffer.
    pub fn stream_sha256(&mut self, path: &str) -> Result<String, Error> {
        let size = self.stream_entry(path)?.size_bytes.unwrap_or(0);
        self.reset_budget();
        let mut stream = self
            .compound
            .open_stream(path)
            .map_err(|e| map_cfb_error(e, &self.budget, self.options))?;
        let mut hash = Sha256::new();
        let mut remaining = size;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            stream
                .read_exact(&mut buffer[..count])
                .map_err(|e| map_cfb_error(e, &self.budget, self.options))?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        Ok(format!("{:x}", hash.finalize()))
    }
}

pub fn inspect_bytes(data: &[u8], options: ParseOptions) -> Result<Inspection, Error> {
    let header = header_inspection(data, options)?;
    if header.signature != Signature::CompoundFile.as_str() {
        return Ok(header);
    }
    Ok(Container::from_bytes(data, options)?.info)
}

pub fn inspect_file(path: impl AsRef<Path>, options: ParseOptions) -> Result<Inspection, Error> {
    let data = load_file(path.as_ref(), options)?;
    let header = header_inspection(&data, options)?;
    if header.signature != Signature::CompoundFile.as_str() {
        return Ok(header);
    }
    Ok(Container::from_owned(data, options)?.info)
}

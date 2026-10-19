use std::{fmt, io};

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Io(io::Error),
    IncompleteRead(Box<crate::ReadReport>),
    InputLimit {
        actual: u64,
        limit: u64,
    },
    InvalidInput(&'static str),
    UnsupportedFeature(&'static str),
    ResourceLimit {
        name: &'static str,
        actual: u64,
        limit: u64,
    },
    InvalidContainer(String),
    InvalidStream(String),
    StreamNotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => err.fmt(f),
            Self::IncompleteRead(report) => write!(f, "incomplete EXB read: {} diagnostics, {} decoded entities; use explicit partial mode to inspect", report.diagnostics.len(), report.decoded_entity_count),
            Self::InputLimit { actual, limit } => {
                write!(f, "input size {actual} exceeds max_file_bytes={limit}")
            }
            Self::InvalidInput(message) => f.write_str(message),
            Self::ResourceLimit {
                name,
                actual,
                limit,
            } => write!(f, "{name} exceeded: {actual} > {limit}"),
            Self::InvalidContainer(message) => write!(f, "invalid CFB container: {message}"),
            Self::InvalidStream(message) => write!(f, "invalid EXB stream: {message}"),
            Self::StreamNotFound(path) => write!(f, "stream not found: {path:?}"),
            Self::UnsupportedFeature(feature) => {
                write!(
                    f,
                    "{feature} is not implemented in ezexb {version}",
                    version = crate::VERSION
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

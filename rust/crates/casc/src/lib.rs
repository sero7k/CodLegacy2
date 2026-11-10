//! Read-only access to a Battle.net CASC install (Black Ops Cold War, product `zeus`).
//!
//! - [`idx`]: the local `.idx` v7 indexes (EKey prefix -> archive, offset, size).
//! - [`blte`]: BLTE decoding (`N` raw and `Z` zlib chunks; `E` encrypted chunks are reported,
//!   never decoded).
//! - [`config`]: `.build.info` and the build config (`key = value` text).
//! - [`tvfs`]: the TVFS file tree named by the build config's `vfs-root`.
//! - [`storage`]: everything put together. It opens the install read-only and reads one file by
//!   its path as a ranged read of `Data/data/data.NNN`.
//!
//! Layouts are in `docs/CW_SURVEY.md`. Nothing here writes to the install.
#![forbid(unsafe_code)]

pub mod blte;
pub mod config;
pub mod idx;
pub mod storage;
pub mod tvfs;

/// Errors from any layer.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The bytes do not have the expected layout.
    Format(String),
    /// A BLTE chunk is encrypted (`E` mode). Carries the key name (reversed, as hex). It is never
    /// decoded.
    Encrypted(String),
    /// No such path / key.
    NotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Format(s) => write!(f, "format: {s}"),
            Self::Encrypted(k) => write!(f, "encrypted BLTE chunk (key {k}); not decoded"),
            Self::NotFound(s) => write!(f, "not found: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn fmt_err<T>(s: impl Into<String>) -> Result<T> {
    Err(Error::Format(s.into()))
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

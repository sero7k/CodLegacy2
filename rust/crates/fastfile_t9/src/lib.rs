//! Call of Duty: Black Ops Cold War (Treyarch "T9"), Battle.net PC, TU35.
//!
//! - [`taff`]: the `.ff` envelope (`TAFF` v9). It has a signed header, then a chain of Oodle
//!   Kraken blocks that decode to the zone image. Not encrypted.
//! - [`fd`]: the `.fd` patch that sits next to every `.ff`. Only its header is read; it carries the
//!   patched and the base TAFF headers.
//! - [`kapi`]: the `.xpak` package index (`KAPI` v0x10): part count and the sorted key table of
//!   the `.xsub` stream parts.
//!
//! Files come from the install with the `casc` crate. Decoding the zone needs an Oodle backend
//! from `fastfile_jup::oodle`: the game's `oo2core_8_win64.dll` on Windows, or a decoder library
//! named by `$JUP_OODLE_LIB`. Layouts are in `docs/CW_SURVEY.md`.
#![forbid(unsafe_code)]

pub mod fd;
pub mod kapi;
pub mod taff;

#[derive(Debug)]
pub enum Error {
    Format(String),
    Oodle(fastfile_jup::oodle::OodleError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(s) => write!(f, "format: {s}"),
            Self::Oodle(e) => write!(f, "oodle: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn u32le(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .ok_or_else(|| Error::Format(format!("read past end at {o:#x}")))
}

pub(crate) fn u64le(b: &[u8], o: usize) -> Result<u64> {
    b.get(o..o + 8)
        .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
        .ok_or_else(|| Error::Format(format!("read past end at {o:#x}")))
}

pub(crate) fn cstr(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}

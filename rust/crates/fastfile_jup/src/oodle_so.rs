//! A Kraken decoder in a shared library the owner points at, for hosts that
//! are not Windows (the game's Oodle DLL loads only there): `$JUP_OODLE_LIB`
//! names the library. It must export `ooz_kraken(src, src_len, dst,
//! dst_len) -> i32` (the decoded length, 0 or less on failure). Nothing is
//! shipped or linked: the library is opened at run time, and only when the
//! variable is set. Used to export the guns on a build machine
//! (`docs/JUP.md`, "The exported guns").
#![allow(unsafe_code)]

use std::path::Path;

use crate::oodle::{Decompressor, OodleError};

/// Names the shared library (a full path).
pub const OODLE_LIB_ENV: &str = "JUP_OODLE_LIB";

type KrakenFn = unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> i32;

pub struct OodleSo {
    kraken: KrakenFn,
    // Keeps the library mapped while `kraken` may be called.
    _lib: libloading::Library,
}

impl OodleSo {
    pub fn load(path: &Path) -> Result<Self, OodleError> {
        if !path.is_file() {
            return Err(OodleError::NoBackend(format!("{} not found", path.display())));
        }
        // SAFETY: opening the library the owner named runs its initialisers;
        // the symbol has the signature above.
        unsafe {
            let lib = libloading::Library::new(path)
                .map_err(|e| OodleError::NoBackend(format!("loading {}: {e}", path.display())))?;
            let sym: libloading::Symbol<KrakenFn> = lib
                .get(b"ooz_kraken\0")
                .map_err(|e| OodleError::NoBackend(format!("ooz_kraken in {}: {e}", path.display())))?;
            let kraken = *sym;
            Ok(Self { kraken, _lib: lib })
        }
    }
}

impl Decompressor for OodleSo {
    fn name(&self) -> &str {
        "shared library ($JUP_OODLE_LIB)"
    }

    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<(), OodleError> {
        if src.is_empty() || dst.is_empty() {
            return Err(OodleError::WrongLength { got: 0, want: dst.len() });
        }
        // (Decoders of this family may write a little past the end: decode
        // into a padded buffer.)
        let mut tmp = vec![0u8; dst.len() + 64];
        // SAFETY: `src` is a live slice of the length passed and `tmp` holds
        // the decoded length plus the slack.
        let got = unsafe { (self.kraken)(src.as_ptr(), src.len(), tmp.as_mut_ptr(), dst.len()) };
        if got < 0 || got as usize != dst.len() {
            return Err(OodleError::WrongLength { got: i64::from(got), want: dst.len() });
        }
        dst.copy_from_slice(&tmp[..dst.len()]);
        Ok(())
    }
}

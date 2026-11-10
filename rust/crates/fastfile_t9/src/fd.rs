//! `.fd`: the patch shipped next to each `.ff`. The header was read; the delta body was not.
//!
//! ```text
//! 0x00 u32 0xb08, u32 0, f32 1.0, u64 0x200000, u64 patched decoded size, u64 (unknown)
//! 0x28 TAFF header of the patched zone (builder "TA-BM-SV-T9-23" in TU35)
//! 0x5e2 TAFF header of the base .ff it applies to (identical to that .ff's first 0x5ba bytes)
//! then delta data with Kraken streams (first `8C 06` at 0x915a in mp_sm_central.fd)
//! ```

use crate::taff::{self, BLOCKS_AT};
use crate::{Error, Result, u64le};

pub const PATCHED_AT: usize = 0x28;
pub const BASE_AT: usize = PATCHED_AT + BLOCKS_AT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub patched_size: u64,
    pub patched: taff::Header,
    pub base: taff::Header,
}

pub fn header(b: &[u8]) -> Result<Header> {
    let tail = |o: usize| b.get(o..).ok_or_else(|| Error::Format(".fd too short".into()));
    Ok(Header { patched_size: u64le(b, 0x18)?, patched: taff::header(tail(PATCHED_AT)?)?, base: taff::header(tail(BASE_AT)?)? })
}

/// Whether `fd` patches exactly this `.ff` (the embedded base header equals the `.ff`'s).
pub fn applies_to(fd: &[u8], ff: &[u8]) -> bool {
    match (fd.get(BASE_AT..BASE_AT + BLOCKS_AT), ff.get(..BLOCKS_AT)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

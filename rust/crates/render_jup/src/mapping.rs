//! Which programs a material draws with.
//!
//! The material header's technique pointers (+0x20, +0x28, +0x38, +0x40) lead
//! to shared records that hold float data, not a program id, and the
//! program a material uses is not decoded in the install (`docs/JUP.md`,
//! "Shaders, techniques and materials"). Another reader is decoding the
//! material to program mapping and the slot meanings; its API goes in
//! [`choose`] when it lands in `fastfile_jup`. Until then the answer is
//! `None`, every surface takes the fallback shader (`fallback.wgsl`, which
//! implements the decoded terms), and the host logs how many did.
//!
//! `BO2MP_JUP_FORCE_PS=<program index>` names one pixel program to build the
//! pipeline check for (`crate::coverage`), for looking at a family on the PC;
//! it does not make surfaces draw with it (the vertex stage pulls its vertices
//! from scene buffers whose layout is not decoded).

use crate::library::{Library, material_pairs};

/// A vertex and a pixel program (indices into the library).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub vertex: usize,
    pub pixel: usize,
}

/// What a map material needs to be matched to a program.
#[derive(Clone, Copy, Debug)]
pub struct MaterialKey {
    pub hash: u64,
    pub has_colour: bool,
    pub has_normal: bool,
}

/// The programs `material` draws with, or `None` where the mapping is not
/// known (the fallback shader then draws it).
pub fn choose(_lib: &Library, _material: &MaterialKey) -> Option<Pair> {
    None
}

/// The pairs the pipeline check builds: every distinct binding family of the
/// library's material pixel programs (one of each), the forced one first.
pub fn check_pairs(lib: &Library, limit: usize) -> Vec<Pair> {
    let mut out = Vec::new();
    if let Some(ps) = std::env::var("BO2MP_JUP_FORCE_PS").ok().and_then(|v| v.trim().parse::<usize>().ok())
        && let Some(vertex) = crate::library::vertex_for(lib, ps)
    {
        out.push(Pair { vertex, pixel: ps });
    }
    let mut seen: Vec<Vec<(u32, u32, u32, u32)>> = Vec::new();
    for (vertex, pixel) in material_pairs(lib) {
        let family: Vec<(u32, u32, u32, u32)> = lib.programs[pixel].bindings.iter().map(|b| (b.space, b.lower, b.upper, b.kind)).collect();
        if seen.contains(&family) {
            continue;
        }
        seen.push(family);
        let pair = Pair { vertex, pixel };
        if !out.contains(&pair) {
            out.push(pair);
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

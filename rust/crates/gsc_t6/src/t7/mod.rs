//! bo2mp: Black Ops III (T7) compiled scripts, run by this crate's VM.
//!
//! [`object`] reads and decodes a script object (`\x80GSC\r\n\0\x1c`) as
//! the retail patch zones hold them; [`ops`] is the VM's opcode table, read
//! from `BlackOps3.exe`; [`canon`] the name hash the scripts link by;
//! [`names`] the names known for those hashes; [`translate`] makes Black
//! Ops II objects of them. (Here rather than in a crate of its own so the
//! sim, which hosts this VM, needs no new dependency; `crates/gsc_t7`
//! holds the generator, tests and tools.) See `docs/T7_ZOMBIES_PLAN.md`.
//! Not part of upstream IW4L.

mod gen_builtins;
mod gen_names;
mod gen_slots;
pub mod names;
pub mod object;
pub mod ops;
pub mod translate;

pub use object::{CaseValue, Export, Function, Import, Insn, T7Object, is_t7_object};
pub use ops::{T7Op, op_of};
pub use translate::{Summary, autoexecs, include_lists, translate, translate_ext};

/// The canonical name hash Black Ops III links by (`BlackOps3.exe`
/// 0x14136ccc0): FNV-1a over the lower-cased name with basis 0x4B9ACE2F,
/// then one more multiply by the prime.
pub fn canon(name: &str) -> u32 {
    let mut x: u32 = 0x4b9a_ce2f;
    for c in name.bytes() {
        x = (x ^ u32::from(c.to_ascii_lowercase())).wrapping_mul(0x0100_0193);
    }
    x.wrapping_mul(0x0100_0193)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_hash_matches_the_exe() {
        // Both from scripts/zm/zm_zod.gsc's export table.
        assert_eq!(canon("zm_zod"), 0xbcfa_45b0);
        assert_eq!(canon("main"), 0xd290_ebfa);
        assert_eq!(canon("MAIN"), canon("main"));
    }

    #[test]
    fn opcode_table_covers_every_slot() {
        assert_eq!(ops::SLOTS.len(), 0x2000);
        let distinct: std::collections::HashSet<_> = ops::SLOTS.iter().filter(|o| **o != T7Op::INVALID).collect();
        assert_eq!(distinct.len(), 114);
        for code in 0..0x0c {
            assert_eq!(op_of(code), T7Op::INVALID, "debugger slot {code:#x}");
        }
        assert_eq!(op_of(0x0e), T7Op::PRE_SCRIPT_CALL);
        assert_eq!(op_of(0x0f), T7Op::CALL_BUILTIN);
        assert_eq!(op_of(0x10), T7Op::END);
        // (Bit 13 set: the second table. Town Remastered's first
        // functions open with 0x200d, its main with 0x3af6.)
        assert_eq!(op_of(0x200d), T7Op::CHECK_CLEAR_PARAMS);
        assert_eq!(op_of(0x3af6), T7Op::SAFE_CREATE_LOCAL_VARIABLES);
        assert_eq!(op_of(0x2e88), T7Op::SAFE_CREATE_LOCAL_VARIABLES);
        assert_eq!(op_of(0x4000), T7Op::INVALID);
        let hi: std::collections::HashSet<_> = ops::SLOTS_HI.iter().filter(|o| **o != T7Op::INVALID).collect();
        assert_eq!(hi, distinct);
    }
}

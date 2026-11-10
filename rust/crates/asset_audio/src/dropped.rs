//! bo2mp: stand-ins for the games left out of the build (owner, 5 Nov
//! 2026): Infinite Warfare (`iw7` feature) and Modern Warfare III (2023)
//! (`jup` feature). Same names as the real modules, so the callers need no
//! switches: no install found, no sounds, nothing to play.

#[cfg(not(feature = "jup"))]
pub mod jup {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::sound_catalog::SoundCatalog;

    const LEFT_OUT: &str = "Modern Warfare III (2023) is not in this build";

    pub const JUP_INSTALL_ENV: &str = "BO2_JUP_INSTALL";
    pub const PACK_MARK: &str = "bo2mp_pack.txt";

    /// A decoded clip (never made here).
    pub struct Pcm {
        pub rate: u32,
        pub channels: u16,
        pub samples: Vec<i16>,
    }

    pub fn want_jup_map(_: Option<&str>) {}
    pub(crate) fn jup_map_wanted() -> Option<String> {
        None
    }
    pub fn jup_map_sounds(_: &Path, _: &str) -> Result<Arc<SoundCatalog>, String> {
        Err(LEFT_OUT.to_owned())
    }
    pub fn want_jup_sounds(_: bool, _: bool) {}
    pub(crate) fn jup_sounds_wanted() -> Option<bool> {
        None
    }
    pub fn has_jup_sounds(_: &SoundCatalog) -> bool {
        false
    }
    /// The install, as the real module finds it: the menu's emblems and
    /// calling cards are read from it (kept in the game; owner, 5 Nov 2026).
    pub fn jup_install() -> Option<PathBuf> {
        let set = std::env::var_os(JUP_INSTALL_ENV)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| asset_transport::game_paths::get("jup"))?;
        ((set.join("zone").join("ingame_br.ff").is_file() || set.join(PACK_MARK).is_file())
            && fastfile_jup::oodle::dll_path(&set).is_file())
        .then_some(set)
    }
    pub fn jup_gun_source() -> Option<PathBuf> {
        None
    }
    /// The catalog key of an alias, as the real module makes it (`#<hash>`,
    /// FNV-1a 64 of the name), so keys stay the same whichever is built.
    pub fn jup_alias_key(alias: &str) -> String {
        let hex = alias.strip_prefix('#').or_else(|| alias.strip_prefix("0x"));
        match hex.and_then(|h| u64::from_str_radix(h, 16).ok()) {
            Some(h) => format!("#{h:016x}"),
            None => {
                let mut h: u64 = 0xCBF2_9CE4_8422_2325;
                for b in alias.bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                }
                format!("#{h:016x}")
            }
        }
    }
    pub fn jup_has(_: &str) -> bool {
        false
    }
    pub fn jup_resurgence_sounds(_: &Path, _: bool) -> Result<Arc<SoundCatalog>, String> {
        Err(LEFT_OUT.to_owned())
    }
    pub fn prefetch_jup_resurgence_sounds() {}
    pub fn jup_gun_sounds(_: &Path) -> Result<Arc<SoundCatalog>, String> {
        Err(LEFT_OUT.to_owned())
    }
    pub fn read_jup_clip(_: &str, _: &str) -> Result<Pcm, String> {
        Err(LEFT_OUT.to_owned())
    }
}

#[cfg(not(feature = "iw7"))]
pub mod iw7 {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::sound_catalog::SoundCatalog;

    const LEFT_OUT: &str = "Infinite Warfare is not in this build";

    pub fn iw7_zone_dir() -> Option<PathBuf> {
        None
    }
    pub fn iw7_weapon_sounds(_: &Path) -> Result<Arc<SoundCatalog>, String> {
        Err(LEFT_OUT.to_owned())
    }
    pub fn read_iw7_stream(_: &str, _: &str) -> Result<Vec<u8>, String> {
        Err(LEFT_OUT.to_owned())
    }
    pub fn iw7_map_ambience_enabled() -> bool {
        false
    }
    pub fn iw7_map_sounds(_: &Path) -> Result<SoundCatalog, String> {
        Err(LEFT_OUT.to_owned())
    }
}

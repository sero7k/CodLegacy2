//! bo2mp: Call of Duty: Modern Warfare III's (2023, "jup") sounds, read
//! from the owner's install at runtime: Resurgence's announcer, music,
//! interface and gameplay sounds on Rebirth Island.
//!
//! `fastfile_jup`'s sound reader takes the sndassets and alias links of the
//! Resurgence zones ([`RESURGENCE_ZONES`]) and finds each sound's stream in
//! the KAPI packages of the install's `zone` folder. Alias names are FNV-1a
//! hashes there, so every alias is kept under its hash, `#<16 hex digits>`
//! ([`jup_alias_key`] turns a name into it). A clip names its package and
//! row (`<package path>`, `<key hex>:<offset>:<size>:<decoded>:<compressed>:
//! <seek table>:<rate>:<channels>:<frames>`) and is decoded when played:
//! the package entry through the game's own Oodle DLL, the Opus packets by
//! `rusty-opus` (pure Rust).
//!
//! The install is `$BO2_JUP_INSTALL`, else the `jup` line of the game
//! folders file. Nothing of the game is copied.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_jup::oodle::{self, Decompressor};
use fastfile_jup::sound::{FORMAT_OPUS, alias_hash, loaded_stream, packets};
use fastfile_jup::sound_bank::{Pcm, SoundBank, decode_opus};
use fastfile_jup::xsub::{XSubEntry, decode_entry};

use crate::sound_catalog::{CapturedAlias, CapturedSound, SoundCatalog};
use crate::{AssetNamespace, ZoneGame, ZoneOwner};

pub const JUP_INSTALL_ENV: &str = "BO2_JUP_INSTALL";

/// bo2mp: the file a small copy of the install (its guns, their sounds and
/// pictures, the calling cards and emblems) is marked with.
pub const PACK_MARK: &str = "bo2mp_pack.txt";

/// The zones Resurgence's sounds come from: the battle royale's own
/// (gameplay, interface, music), its English voice lines, Rebirth Island's
/// English ones, and the shared multiplayer one (the battle royale's
/// pickups, cash, armor hit markers, purchase and victory/defeat stings). (The map zone, `mp_jup_escape5`, holds the
/// island's emitters and ambience; 270 MB to inflate, read once with them.)
pub const RESURGENCE_ZONES: &[&str] = &[
    "ingame_br",
    "eng_ingame_br",
    "eng_mp_jup_escape5",
    "ingame_shared_mp",
];

/// Rebirth Island's map zone (its ambience emitters), read with Resurgence's sounds.
pub const REBIRTH_MAP_ZONE: &str = "mp_jup_escape5";

/// bo2mp: each Modern Warfare III map's ambience beds: the alias hashes of
/// its long, quiet, steady stereo loops (90 to 150 s, -45 to -30 dBFS, no
/// beat), found by measuring every stream of 40 s or more in the map's zone
/// (`docs/JUP.md`, "Collision, water and ambience"). The map's own
/// emitter table is not read; these play together under the match.
pub const JUP_MAP_BEDS: &[(&str, &[u64])] = &[
    ("mp_jup_derail", &[0xf28a23bd791e4aff, 0x32dbbdd2f22cccf5, 0x789d609bb45cda5b]),
    ("mp_jup_shipment_iw9", &[0x04132c7f81e65993, 0xcb3d8667bcaf029f]),
    ("mp_jup_das_haus", &[0xc0499974fae01b22]),
    ("mp_jup_elevated", &[0xc058de46c80a633c, 0xacb290cc0a1b33dc]),
    ("mp_jup_farm_18", &[0x0d499d94578688c9]),
    ("mp_jup_afghan_v2", &[0x38bc3ddffa4f75c6, 0xf9a97fe6179dc668]),
];

/// A map's ambience beds as `jup:#<hash>` aliases (none for a map not listed).
pub fn jup_map_beds(zone: &str) -> Vec<String> {
    JUP_MAP_BEDS
        .iter()
        .find(|(z, _)| z.eq_ignore_ascii_case(zone))
        .map(|(_, v)| v.iter().map(|h| format!("jup:#{h:016x}")).collect())
        .unwrap_or_default()
}

/// The Modern Warfare III map zone whose sounds the next bank takes in.
static MAP_WANTED: Mutex<Option<String>> = Mutex::new(None);

/// Set before a match's bank is composed: the MW3 map being played (or none).
pub fn want_jup_map(zone: Option<&str>) {
    if let Ok(mut w) = MAP_WANTED.lock() {
        *w = zone.filter(|z| !jup_map_beds(z).is_empty()).map(str::to_owned);
    }
}

pub(crate) fn jup_map_wanted() -> Option<String> {
    MAP_WANTED.lock().ok().and_then(|w| w.clone())
}

/// One map zone's sounds of the install (read once per process and zone).
pub fn jup_map_sounds(install: &Path, zone: &str) -> Result<Arc<SoundCatalog>, String> {
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    static MAPS: OnceLock<Mutex<HashMap<(PathBuf, String), Arc<SoundCatalog>>>> = OnceLock::new();
    let maps = MAPS.get_or_init(Default::default);
    let key = (install.to_path_buf(), zone.to_owned());
    if let Some(hit) = maps.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_sound_catalog_jup(install, &[zone])?);
    if let Ok(mut c) = maps.lock() {
        c.insert(key, catalog.clone());
    }
    Ok(catalog)
}

/// Language packs asked first.
const LANG: &str = "eng";

/// 0: none; 1: Resurgence's sounds; 2: and Rebirth Island's map zone.
static WANTED: AtomicU8 = AtomicU8::new(0);

/// Whether the next sound bank takes in Resurgence's sounds (set before a
/// match's bank is composed), and Rebirth Island's map zone with them.
pub fn want_jup_sounds(resurgence: bool, rebirth_map: bool) {
    WANTED.store(
        u8::from(resurgence) * (1 + u8::from(rebirth_map)),
        Ordering::Relaxed,
    );
}

pub(crate) fn jup_sounds_wanted() -> Option<bool> {
    match WANTED.load(Ordering::Relaxed) {
        0 => None,
        n => Some(n == 2),
    }
}

/// Whether a composed bank already holds Resurgence's sounds (its drop
/// music: the MW3 guns' sounds are in the other banks too).
pub fn has_jup_sounds(catalog: &SoundCatalog) -> bool {
    catalog
        .index_in(
            AssetNamespace::Jup,
            &jup_alias_key("mus_mx_mp_br_ri_infil_1_intro"),
        )
        .is_some()
}

/// The Modern Warfare III install, when it is set and looks like one
/// (`zone/ingame_br.ff` and the Oodle DLL beside it).
pub fn jup_install() -> Option<PathBuf> {
    let set = std::env::var_os(JUP_INSTALL_ENV)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| asset_transport::game_paths::get("jup"))?;
    // (A small copy of the install, `fastfile_jup::pack`, has no
    // `ingame_br`: it says what it is in `bo2mp_pack.txt`.)
    ((set.join("zone").join("ingame_br.ff").is_file() || set.join(PACK_MARK).is_file()) && oodle::dll_path(&set).is_file())
        .then_some(set)
}

/// bo2mp: where the Modern Warfare III guns' sounds are read from: the
/// install, else the folder the guns were exported to
/// (`asset_transport::jup_export`).
pub fn jup_gun_source() -> Option<PathBuf> {
    jup_install().or_else(asset_transport::jup_export::dir)
}

/// The catalog key of an alias: `#<hash>` (a name is hashed; a `#<hash>`
/// or `0x<hash>` stays as it is).
pub fn jup_alias_key(alias: &str) -> String {
    let hex = alias.strip_prefix('#').or_else(|| alias.strip_prefix("0x"));
    match hex.and_then(|h| u64::from_str_radix(h, 16).ok()) {
        Some(h) => format!("#{h:016x}"),
        None => format!("#{:016x}", alias_hash(alias)),
    }
}

fn available() -> &'static Mutex<HashSet<u64>> {
    static SET: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// Whether the install has this alias (a name, or `#<hash>`), with a clip
/// to play. The first question reads Resurgence's sounds (and Rebirth
/// Island's zone) when the install is set and they are not read yet, so the
/// match script knows its sounds before the match's sound bank is composed.
pub fn jup_has(alias: &str) -> bool {
    let key = jup_alias_key(alias);
    let Ok(h) = u64::from_str_radix(&key[1..], 16) else {
        return false;
    };
    static READ: OnceLock<()> = OnceLock::new();
    READ.get_or_init(|| {
        if available().lock().is_ok_and(|s| s.is_empty())
            && let Some(install) = jup_install()
            && let Err(e) = jup_resurgence_sounds(&install, true)
        {
            diag::warn!(Zone, "sound bank: MW3 (2023) {}: {e}", install.display());
        }
    });
    available().lock().is_ok_and(|s| s.contains(&h))
}

/// Catalogs read, by install and whether the map zone is in.
type Cache = Mutex<HashMap<(PathBuf, bool), Arc<SoundCatalog>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn oodle_for(install: &Path) -> Result<&'static dyn Decompressor, String> {
    static OODLE: OnceLock<Result<Box<dyn Decompressor>, String>> = OnceLock::new();
    OODLE
        .get_or_init(|| oodle::load(install).map_err(|e| e.to_string()))
        .as_ref()
        .map(|b| b.as_ref())
        .map_err(Clone::clone)
}

/// Resurgence's sounds of the install (read once per process; with the
/// Rebirth Island map zone when `with_map`).
pub fn jup_resurgence_sounds(install: &Path, with_map: bool) -> Result<Arc<SoundCatalog>, String> {
    // (One read at a time: a match's bank asking while the prefetch reads
    // waits for it and takes its catalog.)
    static LOAD: Mutex<()> = Mutex::new(());
    let _one = LOAD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (install.to_path_buf(), with_map);
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(hit);
    }
    let mut zones: Vec<&str> = RESURGENCE_ZONES.to_vec();
    if with_map {
        zones.push(REBIRTH_MAP_ZONE);
    }
    let catalog = Arc::new(load_sound_catalog_jup(install, &zones)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(key, catalog.clone());
    }
    Ok(catalog)
}

/// bo2mp: read Resurgence's sounds (with Rebirth Island's map zone) on a
/// background thread ahead of the match (the WARZONE menu opened, the
/// mode chosen): the match's sound bank then takes them from the cache
/// instead of reading 270 MB of zones while the island loads. Once per
/// process; nothing when the install is not there.
pub fn prefetch_jup_resurgence_sounds() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(install) = jup_install() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("jup-sounds".to_owned())
        .spawn(move || {
            let t = std::time::Instant::now();
            match jup_resurgence_sounds(&install, true) {
                Ok(c) => diag::info!(
                    Zone,
                    "sound bank: Resurgence's MW3 (2023) sounds read ahead in {:.1} s ({} aliases)",
                    t.elapsed().as_secs_f32(),
                    c.sounds.len()
                ),
                Err(e) => diag::warn!(Zone, "sound bank: Resurgence's MW3 (2023) sounds not read ahead: {e}"),
            }
        });
    if spawned.is_err() {
        STARTED.store(false, Ordering::Relaxed);
    }
}

/// The zone Modern Warfare III's guns' sounds are read from: `ingame`,
/// the guns' fire layers (`wpn_pi_mike93_fire_plr_transient`, `_shot`,
/// `_mech`, `_lfe`, `_atmo` ...) and their foley (`wfoly_plr_pi_mike93_
/// reload_magout` ...).
pub const JUP_GUN_ZONE: &str = "ingame";
/// The zones gun sounds are read from: `ingame`, and `ingame_shared_mp`
/// where most guns' fire layers are (the BAL-27's, the KATT-AMR's, ...).
pub const JUP_GUN_ZONES: &[&str] = &[JUP_GUN_ZONE, "ingame_shared_mp"];

/// A Modern Warfare III gun's own sounds, in the slots of the Black Ops II
/// gun it is a copy of (see `asset_game::GuestCopy`): alias names (or
/// `#<hash>`), "" for none (the slot keeps its donor's sound).
#[derive(Clone, Copy, Debug)]
pub struct JupGunSounds {
    /// The gun (`jp12_pi_mike93`).
    pub gun: &'static str,
    /// Fired, heard in the world (another player's shot).
    pub fire: &'static str,
    /// Fired, in first person: the head of its layers.
    pub fire_player: &'static str,
    /// The last round, in first person (the slide locking back).
    pub fire_last_player: &'static str,
    pub raise_player: &'static str,
    pub first_raise_player: &'static str,
    pub putaway_player: &'static str,
    pub pickup: &'static str,
    pub pickup_player: &'static str,
    pub ads_up: &'static str,
    pub ads_down: &'static str,
    /// The donor's notetrack sounds by a word of their alias, played as
    /// this gun's (the first pair whose word the alias holds; the others
    /// keep the donor's).
    pub notes: &'static [(&'static str, &'static str)],
    /// Layers played together (alias, the one played with it) where the
    /// zone's chain stops short (its end is in no zone read): the shot's
    /// low end into the long tail.
    pub layers: &'static [(&'static str, &'static str)],
}

impl JupGunSounds {
    /// Every alias it names (the layers' chains are followed when read).
    pub fn aliases(&self) -> impl Iterator<Item = &'static str> {
        [
            self.fire,
            self.fire_player,
            self.fire_last_player,
            self.raise_player,
            self.first_raise_player,
            self.putaway_player,
            self.pickup,
            self.pickup_player,
            self.ads_up,
            self.ads_down,
        ]
        .into_iter()
        .chain(self.notes.iter().map(|(_, a)| *a))
        .chain(self.layers.iter().flat_map(|(a, b)| [*a, *b]))
        .filter(|a| !a.is_empty())
    }
}

/// The guns played and their sounds (in the install's `ingame` and
/// `ingame_shared_mp`; names recovered by hashing, each checked against the
/// zones, see docs/JUP.md): MWIII's `wpn_<class>_<gun>_fire_*`, MWII's
/// `weap_<gun>_fire_*` (no transient: the shot heads its layers), foley
/// `wfoly_plr_<class>_<gun>_*` (reloads numbered `reload_01`..). A name the
/// install lacks is not read, and the gun keeps its donor's sound there
/// ([`jup_gun_has`]).
pub const JUP_GUN_SOUNDS: &[JupGunSounds] = &[
    JupGunSounds {
        gun: "jp12_pi_mike93",
        fire: "wpn_pi_mike93_fire_npc",
        fire_player: "wpn_pi_mike93_fire_plr_transient",
        fire_last_player: "wpn_pi_mike93_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_mike93_raise_quick_01",
        first_raise_player: "wfoly_plr_pi_mike93_raise_first_01",
        putaway_player: "wfoly_plr_pi_mike93_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_mike93_reload_magout"), ("mag_out", "wfoly_plr_pi_mike93_reload_magout"), ("clipout", "wfoly_plr_pi_mike93_reload_magout"), ("clip_out", "wfoly_plr_pi_mike93_reload_magout"), ("magin", "wfoly_plr_pi_mike93_reload_magin"), ("mag_in", "wfoly_plr_pi_mike93_reload_magin"), ("clipin", "wfoly_plr_pi_mike93_reload_magin"), ("clip_in", "wfoly_plr_pi_mike93_reload_magin"), ("slide", "wfoly_plr_pi_mike93_reload_slide_release"), ("chamber", "wfoly_plr_pi_mike93_reload_slide_release"), ("release", "wfoly_plr_pi_mike93_reload_slide_release"), ("bolt", "wfoly_plr_pi_mike93_reload_slide_release"), ("charge", "wfoly_plr_pi_mike93_reload_slide_release"), ("lift", "wfoly_plr_pi_mike93_reload_hand"), ("grab", "wfoly_plr_pi_mike93_reload_hand"), ("hand", "wfoly_plr_pi_mike93_reload_hand"), ("raise", "wfoly_plr_pi_mike93_raise_quick_01"), ("pullout", "wfoly_plr_pi_mike93_raise_quick_01"), ("drop", "wfoly_plr_pi_mike93_drop_quick_01"), ("putaway", "wfoly_plr_pi_mike93_drop_quick_01")],
        layers: &[("wpn_pi_mike93_fire_plr_lfe", "wpn_pi_mike93_fire_plr_atmo"), ("wpn_pi_mike93_fire_npc_mech", "wpn_pi_mike93_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp36_ar_anov94",
        fire: "",
        fire_player: "",
        fire_last_player: "",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[],
    },
    JupGunSounds {
        gun: "jp19_ar_acharlie",
        fire: "wpn_ar_acharlie_fire_npc",
        fire_player: "wpn_ar_acharlie_fire_plr_transient",
        fire_last_player: "wpn_ar_acharlie_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_acharlie_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_acharlie_raise_first_01",
        putaway_player: "wfoly_plr_ar_acharlie_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_acharlie_reload_02"), ("mag_out", "wfoly_plr_ar_acharlie_reload_02"), ("clipout", "wfoly_plr_ar_acharlie_reload_02"), ("clip_out", "wfoly_plr_ar_acharlie_reload_02"), ("magin", "wfoly_plr_ar_acharlie_reload_03"), ("mag_in", "wfoly_plr_ar_acharlie_reload_03"), ("clipin", "wfoly_plr_ar_acharlie_reload_03"), ("clip_in", "wfoly_plr_ar_acharlie_reload_03"), ("slide", "wfoly_plr_ar_acharlie_reload_empty_04"), ("chamber", "wfoly_plr_ar_acharlie_reload_empty_04"), ("release", "wfoly_plr_ar_acharlie_reload_empty_04"), ("bolt", "wfoly_plr_ar_acharlie_reload_empty_04"), ("charge", "wfoly_plr_ar_acharlie_reload_empty_04"), ("lift", "wfoly_plr_ar_acharlie_reload_01"), ("grab", "wfoly_plr_ar_acharlie_reload_01"), ("hand", "wfoly_plr_ar_acharlie_reload_01"), ("raise", "wfoly_plr_ar_acharlie_raise_quick_01"), ("pullout", "wfoly_plr_ar_acharlie_raise_quick_01"), ("drop", "wfoly_plr_ar_acharlie_drop_quick_01"), ("putaway", "wfoly_plr_ar_acharlie_drop_quick_01")],
        layers: &[("wpn_ar_acharlie_fire_plr_lfe", "wpn_ar_acharlie_fire_plr_atmo"), ("wpn_ar_acharlie_fire_npc_mech", "wpn_ar_acharlie_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp34_ar_balpha27",
        fire: "wpn_ar_balpha27_fire_npc",
        fire_player: "wpn_ar_balpha27_fire_plr_transient",
        fire_last_player: "wpn_ar_balpha27_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_balpha27_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_balpha27_raise_first_01",
        putaway_player: "wfoly_plr_ar_balpha27_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_balpha27_reload_02"), ("mag_out", "wfoly_plr_ar_balpha27_reload_02"), ("clipout", "wfoly_plr_ar_balpha27_reload_02"), ("clip_out", "wfoly_plr_ar_balpha27_reload_02"), ("magin", "wfoly_plr_ar_balpha27_reload_03"), ("mag_in", "wfoly_plr_ar_balpha27_reload_03"), ("clipin", "wfoly_plr_ar_balpha27_reload_03"), ("clip_in", "wfoly_plr_ar_balpha27_reload_03"), ("slide", "wfoly_plr_ar_balpha27_reload_empty_04"), ("chamber", "wfoly_plr_ar_balpha27_reload_empty_04"), ("release", "wfoly_plr_ar_balpha27_reload_empty_04"), ("bolt", "wfoly_plr_ar_balpha27_reload_empty_04"), ("charge", "wfoly_plr_ar_balpha27_reload_empty_04"), ("lift", "wfoly_plr_ar_balpha27_reload_01"), ("grab", "wfoly_plr_ar_balpha27_reload_01"), ("hand", "wfoly_plr_ar_balpha27_reload_01"), ("raise", "wfoly_plr_ar_balpha27_raise_quick_01"), ("pullout", "wfoly_plr_ar_balpha27_raise_quick_01"), ("drop", "wfoly_plr_ar_balpha27_drop_quick_01"), ("putaway", "wfoly_plr_ar_balpha27_drop_quick_01")],
        layers: &[("wpn_ar_balpha27_fire_plr_lfe", "wpn_ar_balpha27_fire_plr_atmo"), ("wpn_ar_balpha27_fire_npc_mech", "wpn_ar_balpha27_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp05_ar_foxtrot2000",
        fire: "wpn_ar_foxtrot2000_fire_npc",
        fire_player: "wpn_ar_foxtrot2000_fire_plr_transient",
        fire_last_player: "wpn_ar_foxtrot2000_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_foxtrot2000_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_foxtrot2000_raise_first_01",
        putaway_player: "wfoly_plr_ar_foxtrot2000_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_foxtrot2000_reload_02"), ("mag_out", "wfoly_plr_ar_foxtrot2000_reload_02"), ("clipout", "wfoly_plr_ar_foxtrot2000_reload_02"), ("clip_out", "wfoly_plr_ar_foxtrot2000_reload_02"), ("magin", "wfoly_plr_ar_foxtrot2000_reload_03"), ("mag_in", "wfoly_plr_ar_foxtrot2000_reload_03"), ("clipin", "wfoly_plr_ar_foxtrot2000_reload_03"), ("clip_in", "wfoly_plr_ar_foxtrot2000_reload_03"), ("slide", "wfoly_plr_ar_foxtrot2000_reload_empty_04"), ("chamber", "wfoly_plr_ar_foxtrot2000_reload_empty_04"), ("release", "wfoly_plr_ar_foxtrot2000_reload_empty_04"), ("bolt", "wfoly_plr_ar_foxtrot2000_reload_empty_04"), ("charge", "wfoly_plr_ar_foxtrot2000_reload_empty_04"), ("lift", "wfoly_plr_ar_foxtrot2000_reload_01"), ("grab", "wfoly_plr_ar_foxtrot2000_reload_01"), ("hand", "wfoly_plr_ar_foxtrot2000_reload_01"), ("raise", "wfoly_plr_ar_foxtrot2000_raise_quick_01"), ("pullout", "wfoly_plr_ar_foxtrot2000_raise_quick_01"), ("drop", "wfoly_plr_ar_foxtrot2000_drop_quick_01"), ("putaway", "wfoly_plr_ar_foxtrot2000_drop_quick_01")],
        layers: &[("wpn_ar_foxtrot2000_fire_plr_lfe", "wpn_ar_foxtrot2000_fire_plr_atmo"), ("wpn_ar_foxtrot2000_fire_npc_mech", "wpn_ar_foxtrot2000_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp08_ar_qbravo97",
        fire: "wpn_ar_qbravo97_fire_npc",
        fire_player: "wpn_ar_qbravo97_fire_plr_transient",
        fire_last_player: "wpn_ar_qbravo97_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_qbravo97_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_qbravo97_raise_first_01",
        putaway_player: "wfoly_plr_ar_qbravo97_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_qbravo97_reload_02"), ("mag_out", "wfoly_plr_ar_qbravo97_reload_02"), ("clipout", "wfoly_plr_ar_qbravo97_reload_02"), ("clip_out", "wfoly_plr_ar_qbravo97_reload_02"), ("magin", "wfoly_plr_ar_qbravo97_reload_03"), ("mag_in", "wfoly_plr_ar_qbravo97_reload_03"), ("clipin", "wfoly_plr_ar_qbravo97_reload_03"), ("clip_in", "wfoly_plr_ar_qbravo97_reload_03"), ("slide", "wfoly_plr_ar_qbravo97_reload_empty_04"), ("chamber", "wfoly_plr_ar_qbravo97_reload_empty_04"), ("release", "wfoly_plr_ar_qbravo97_reload_empty_04"), ("bolt", "wfoly_plr_ar_qbravo97_reload_empty_04"), ("charge", "wfoly_plr_ar_qbravo97_reload_empty_04"), ("lift", "wfoly_plr_ar_qbravo97_reload_01"), ("grab", "wfoly_plr_ar_qbravo97_reload_01"), ("hand", "wfoly_plr_ar_qbravo97_reload_01"), ("raise", "wfoly_plr_ar_qbravo97_raise_quick_01"), ("pullout", "wfoly_plr_ar_qbravo97_raise_quick_01"), ("drop", "wfoly_plr_ar_qbravo97_drop_quick_01"), ("putaway", "wfoly_plr_ar_qbravo97_drop_quick_01")],
        layers: &[("wpn_ar_qbravo97_fire_plr_lfe", "wpn_ar_qbravo97_fire_plr_atmo"), ("wpn_ar_qbravo97_fire_npc_mech", "wpn_ar_qbravo97_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp44_ar_stango44",
        fire: "wpn_ar_stango44_fire_npc",
        fire_player: "wpn_ar_stango44_fire_plr_transient",
        fire_last_player: "wpn_ar_stango44_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_stango44_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_stango44_raise_first_01",
        putaway_player: "wfoly_plr_ar_stango44_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_stango44_reload_02"), ("mag_out", "wfoly_plr_ar_stango44_reload_02"), ("clipout", "wfoly_plr_ar_stango44_reload_02"), ("clip_out", "wfoly_plr_ar_stango44_reload_02"), ("magin", "wfoly_plr_ar_stango44_reload_03"), ("mag_in", "wfoly_plr_ar_stango44_reload_03"), ("clipin", "wfoly_plr_ar_stango44_reload_03"), ("clip_in", "wfoly_plr_ar_stango44_reload_03"), ("slide", "wfoly_plr_ar_stango44_reload_empty_04"), ("chamber", "wfoly_plr_ar_stango44_reload_empty_04"), ("release", "wfoly_plr_ar_stango44_reload_empty_04"), ("bolt", "wfoly_plr_ar_stango44_reload_empty_04"), ("charge", "wfoly_plr_ar_stango44_reload_empty_04"), ("lift", "wfoly_plr_ar_stango44_reload_01"), ("grab", "wfoly_plr_ar_stango44_reload_01"), ("hand", "wfoly_plr_ar_stango44_reload_01"), ("raise", "wfoly_plr_ar_stango44_raise_quick_01"), ("pullout", "wfoly_plr_ar_stango44_raise_quick_01"), ("drop", "wfoly_plr_ar_stango44_drop_quick_01"), ("putaway", "wfoly_plr_ar_stango44_drop_quick_01")],
        layers: &[("wpn_ar_stango44_fire_plr_lfe", "wpn_ar_stango44_fire_plr_atmo"), ("wpn_ar_stango44_fire_npc_mech", "wpn_ar_stango44_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp19_br_acharlie450",
        fire: "wpn_br_acharlie450_fire_npc",
        fire_player: "wpn_br_acharlie450_fire_plr_transient",
        fire_last_player: "wpn_br_acharlie450_fire_plr_last_mech",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[("wpn_br_acharlie450_fire_plr_lfe", "wpn_br_acharlie450_fire_plr_atmo"), ("wpn_br_acharlie450_fire_npc_mech", "wpn_br_acharlie450_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp03_br_aromeo200",
        fire: "wpn_br_aromeo200_fire_npc",
        fire_player: "wpn_br_aromeo200_fire_plr_transient",
        fire_last_player: "wpn_br_aromeo200_fire_plr_last_mech",
        raise_player: "wfoly_plr_br_aromeo200_raise_quick_01",
        first_raise_player: "wfoly_plr_br_aromeo200_raise_first_01",
        putaway_player: "wfoly_plr_br_aromeo200_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_br_aromeo200_reload_02"), ("mag_out", "wfoly_plr_br_aromeo200_reload_02"), ("clipout", "wfoly_plr_br_aromeo200_reload_02"), ("clip_out", "wfoly_plr_br_aromeo200_reload_02"), ("magin", "wfoly_plr_br_aromeo200_reload_03"), ("mag_in", "wfoly_plr_br_aromeo200_reload_03"), ("clipin", "wfoly_plr_br_aromeo200_reload_03"), ("clip_in", "wfoly_plr_br_aromeo200_reload_03"), ("slide", "wfoly_plr_br_aromeo200_reload_empty_04"), ("chamber", "wfoly_plr_br_aromeo200_reload_empty_04"), ("release", "wfoly_plr_br_aromeo200_reload_empty_04"), ("bolt", "wfoly_plr_br_aromeo200_reload_empty_04"), ("charge", "wfoly_plr_br_aromeo200_reload_empty_04"), ("lift", "wfoly_plr_br_aromeo200_reload_01"), ("grab", "wfoly_plr_br_aromeo200_reload_01"), ("hand", "wfoly_plr_br_aromeo200_reload_01"), ("raise", "wfoly_plr_br_aromeo200_raise_quick_01"), ("pullout", "wfoly_plr_br_aromeo200_raise_quick_01"), ("drop", "wfoly_plr_br_aromeo200_drop_quick_01"), ("putaway", "wfoly_plr_br_aromeo200_drop_quick_01")],
        layers: &[("wpn_br_aromeo200_fire_plr_lfe", "wpn_br_aromeo200_fire_plr_atmo"), ("wpn_br_aromeo200_fire_npc_mech", "wpn_br_aromeo200_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp37_dm_kappa98",
        fire: "wpn_dm_kappa98_fire_npc",
        fire_player: "wpn_dm_kappa98_fire_plr_transient",
        fire_last_player: "wpn_dm_kappa98_fire_plr_last_mech",
        raise_player: "wfoly_plr_dm_kappa98_raise_quick_01",
        first_raise_player: "wfoly_plr_dm_kappa98_raise_first_01",
        putaway_player: "wfoly_plr_dm_kappa98_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("slide", "wfoly_plr_dm_kappa98_reload_empty_04"), ("chamber", "wfoly_plr_dm_kappa98_reload_empty_04"), ("release", "wfoly_plr_dm_kappa98_reload_empty_04"), ("bolt", "wfoly_plr_dm_kappa98_reload_empty_04"), ("charge", "wfoly_plr_dm_kappa98_reload_empty_04"), ("raise", "wfoly_plr_dm_kappa98_raise_quick_01"), ("pullout", "wfoly_plr_dm_kappa98_raise_quick_01"), ("drop", "wfoly_plr_dm_kappa98_drop_quick_01"), ("putaway", "wfoly_plr_dm_kappa98_drop_quick_01")],
        layers: &[("wpn_dm_kappa98_fire_plr_lfe", "wpn_dm_kappa98_fire_plr_atmo"), ("wpn_dm_kappa98_fire_npc_mech", "wpn_dm_kappa98_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp06_lm_pkilop",
        fire: "wpn_lm_pkilop_fire_npc",
        fire_player: "wpn_lm_pkilop_fire_plr_transient",
        fire_last_player: "wpn_lm_pkilop_fire_plr_last_mech",
        raise_player: "wfoly_plr_lm_pkilop_raise_quick_01",
        first_raise_player: "wfoly_plr_lm_pkilop_raise_first_01",
        putaway_player: "wfoly_plr_lm_pkilop_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_lm_pkilop_reload_02"), ("mag_out", "wfoly_plr_lm_pkilop_reload_02"), ("clipout", "wfoly_plr_lm_pkilop_reload_02"), ("clip_out", "wfoly_plr_lm_pkilop_reload_02"), ("magin", "wfoly_plr_lm_pkilop_reload_03"), ("mag_in", "wfoly_plr_lm_pkilop_reload_03"), ("clipin", "wfoly_plr_lm_pkilop_reload_03"), ("clip_in", "wfoly_plr_lm_pkilop_reload_03"), ("slide", "wfoly_plr_lm_pkilop_reload_empty_04"), ("chamber", "wfoly_plr_lm_pkilop_reload_empty_04"), ("release", "wfoly_plr_lm_pkilop_reload_empty_04"), ("bolt", "wfoly_plr_lm_pkilop_reload_empty_04"), ("charge", "wfoly_plr_lm_pkilop_reload_empty_04"), ("lift", "wfoly_plr_lm_pkilop_reload_01"), ("grab", "wfoly_plr_lm_pkilop_reload_01"), ("hand", "wfoly_plr_lm_pkilop_reload_01"), ("raise", "wfoly_plr_lm_pkilop_raise_quick_01"), ("pullout", "wfoly_plr_lm_pkilop_raise_quick_01"), ("drop", "wfoly_plr_lm_pkilop_drop_quick_01"), ("putaway", "wfoly_plr_lm_pkilop_drop_quick_01")],
        layers: &[("wpn_lm_pkilop_fire_plr_lfe", "wpn_lm_pkilop_fire_plr_atmo"), ("wpn_lm_pkilop_fire_npc_mech", "wpn_lm_pkilop_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp08_lm_qbravo95lsw",
        fire: "wpn_lm_qbravo95lsw_fire_npc",
        fire_player: "",
        fire_last_player: "",
        raise_player: "wfoly_plr_lm_qbravo95lsw_raise_quick_01",
        first_raise_player: "wfoly_plr_lm_qbravo95lsw_raise_first_01",
        putaway_player: "wfoly_plr_lm_qbravo95lsw_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_lm_qbravo95lsw_reload_02"), ("mag_out", "wfoly_plr_lm_qbravo95lsw_reload_02"), ("clipout", "wfoly_plr_lm_qbravo95lsw_reload_02"), ("clip_out", "wfoly_plr_lm_qbravo95lsw_reload_02"), ("magin", "wfoly_plr_lm_qbravo95lsw_reload_03"), ("mag_in", "wfoly_plr_lm_qbravo95lsw_reload_03"), ("clipin", "wfoly_plr_lm_qbravo95lsw_reload_03"), ("clip_in", "wfoly_plr_lm_qbravo95lsw_reload_03"), ("slide", "wfoly_plr_lm_qbravo95lsw_reload_empty_04"), ("chamber", "wfoly_plr_lm_qbravo95lsw_reload_empty_04"), ("release", "wfoly_plr_lm_qbravo95lsw_reload_empty_04"), ("bolt", "wfoly_plr_lm_qbravo95lsw_reload_empty_04"), ("charge", "wfoly_plr_lm_qbravo95lsw_reload_empty_04"), ("lift", "wfoly_plr_lm_qbravo95lsw_reload_01"), ("grab", "wfoly_plr_lm_qbravo95lsw_reload_01"), ("hand", "wfoly_plr_lm_qbravo95lsw_reload_01"), ("raise", "wfoly_plr_lm_qbravo95lsw_raise_quick_01"), ("pullout", "wfoly_plr_lm_qbravo95lsw_raise_quick_01"), ("drop", "wfoly_plr_lm_qbravo95lsw_drop_quick_01"), ("putaway", "wfoly_plr_lm_qbravo95lsw_drop_quick_01")],
        layers: &[("wpn_lm_qbravo95lsw_fire_npc_mech", "wpn_lm_qbravo95lsw_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp38_sh_spapa12",
        fire: "wpn_sh_spapa12_fire_npc",
        fire_player: "wpn_sh_spapa12_fire_plr_transient",
        fire_last_player: "wpn_sh_spapa12_fire_plr_last_mech",
        raise_player: "wfoly_plr_sh_spapa12_raise_quick_01",
        first_raise_player: "wfoly_plr_sh_spapa12_raise_first_01",
        putaway_player: "wfoly_plr_sh_spapa12_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("raise", "wfoly_plr_sh_spapa12_raise_quick_01"), ("pullout", "wfoly_plr_sh_spapa12_raise_quick_01"), ("drop", "wfoly_plr_sh_spapa12_drop_quick_01"), ("putaway", "wfoly_plr_sh_spapa12_drop_quick_01")],
        layers: &[("wpn_sh_spapa12_fire_plr_lfe", "wpn_sh_spapa12_fire_plr_atmo"), ("wpn_sh_spapa12_fire_npc_mech", "wpn_sh_spapa12_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp41_sm_kpappa50",
        fire: "wpn_sm_kpappa50_fire_npc",
        fire_player: "wpn_sm_kpappa50_fire_plr_transient",
        fire_last_player: "wpn_sm_kpappa50_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_kpappa50_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_kpappa50_raise_first_01",
        putaway_player: "wfoly_plr_sm_kpappa50_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_kpappa50_reload_02"), ("mag_out", "wfoly_plr_sm_kpappa50_reload_02"), ("clipout", "wfoly_plr_sm_kpappa50_reload_02"), ("clip_out", "wfoly_plr_sm_kpappa50_reload_02"), ("magin", "wfoly_plr_sm_kpappa50_reload_03"), ("mag_in", "wfoly_plr_sm_kpappa50_reload_03"), ("clipin", "wfoly_plr_sm_kpappa50_reload_03"), ("clip_in", "wfoly_plr_sm_kpappa50_reload_03"), ("slide", "wfoly_plr_sm_kpappa50_reload_empty_04"), ("chamber", "wfoly_plr_sm_kpappa50_reload_empty_04"), ("release", "wfoly_plr_sm_kpappa50_reload_empty_04"), ("bolt", "wfoly_plr_sm_kpappa50_reload_empty_04"), ("charge", "wfoly_plr_sm_kpappa50_reload_empty_04"), ("lift", "wfoly_plr_sm_kpappa50_reload_01"), ("grab", "wfoly_plr_sm_kpappa50_reload_01"), ("hand", "wfoly_plr_sm_kpappa50_reload_01"), ("raise", "wfoly_plr_sm_kpappa50_raise_quick_01"), ("pullout", "wfoly_plr_sm_kpappa50_raise_quick_01"), ("drop", "wfoly_plr_sm_kpappa50_drop_quick_01"), ("putaway", "wfoly_plr_sm_kpappa50_drop_quick_01")],
        layers: &[("wpn_sm_kpappa50_fire_plr_lfe", "wpn_sm_kpappa50_fire_plr_atmo"), ("wpn_sm_kpappa50_fire_npc_mech", "wpn_sm_kpappa50_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp25_sm_talpham",
        fire: "wpn_sm_talpham_fire_npc",
        fire_player: "wpn_sm_talpham_fire_plr_transient",
        fire_last_player: "wpn_sm_talpham_fire_plr_last_mech",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[("wpn_sm_talpham_fire_plr_lfe", "wpn_sm_talpham_fire_plr_atmo"), ("wpn_sm_talpham_fire_npc_mech", "wpn_sm_talpham_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp10_sn_cdelta50",
        fire: "wpn_sn_cdelta50_fire_npc",
        fire_player: "wpn_sn_cdelta50_fire_plr_transient",
        fire_last_player: "wpn_sn_cdelta50_fire_plr_last_mech",
        raise_player: "wfoly_plr_sn_cdelta50_raise_quick_01",
        first_raise_player: "wfoly_plr_sn_cdelta50_raise_first_01",
        putaway_player: "wfoly_plr_sn_cdelta50_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sn_cdelta50_reload_02"), ("mag_out", "wfoly_plr_sn_cdelta50_reload_02"), ("clipout", "wfoly_plr_sn_cdelta50_reload_02"), ("clip_out", "wfoly_plr_sn_cdelta50_reload_02"), ("magin", "wfoly_plr_sn_cdelta50_reload_03"), ("mag_in", "wfoly_plr_sn_cdelta50_reload_03"), ("clipin", "wfoly_plr_sn_cdelta50_reload_03"), ("clip_in", "wfoly_plr_sn_cdelta50_reload_03"), ("slide", "wfoly_plr_sn_cdelta50_reload_empty_04"), ("chamber", "wfoly_plr_sn_cdelta50_reload_empty_04"), ("release", "wfoly_plr_sn_cdelta50_reload_empty_04"), ("bolt", "wfoly_plr_sn_cdelta50_reload_empty_04"), ("charge", "wfoly_plr_sn_cdelta50_reload_empty_04"), ("lift", "wfoly_plr_sn_cdelta50_reload_01"), ("grab", "wfoly_plr_sn_cdelta50_reload_01"), ("hand", "wfoly_plr_sn_cdelta50_reload_01"), ("raise", "wfoly_plr_sn_cdelta50_raise_quick_01"), ("pullout", "wfoly_plr_sn_cdelta50_raise_quick_01"), ("drop", "wfoly_plr_sn_cdelta50_drop_quick_01"), ("putaway", "wfoly_plr_sn_cdelta50_drop_quick_01")],
        layers: &[("wpn_sn_cdelta50_fire_plr_lfe", "wpn_sn_cdelta50_fire_plr_atmo"), ("wpn_sn_cdelta50_fire_npc_mech", "wpn_sn_cdelta50_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "jp17_sn_hsierra",
        fire: "wpn_sn_hsierra_fire_npc",
        fire_player: "wpn_sn_hsierra_fire_plr_shot",
        fire_last_player: "wpn_sn_hsierra_fire_plr_last_mech",
        raise_player: "wfoly_plr_sn_hsierra_raise_quick_01",
        first_raise_player: "wfoly_plr_sn_hsierra_raise_first_01",
        putaway_player: "wfoly_plr_sn_hsierra_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sn_hsierra_reload_02"), ("mag_out", "wfoly_plr_sn_hsierra_reload_02"), ("clipout", "wfoly_plr_sn_hsierra_reload_02"), ("clip_out", "wfoly_plr_sn_hsierra_reload_02"), ("magin", "wfoly_plr_sn_hsierra_reload_03"), ("mag_in", "wfoly_plr_sn_hsierra_reload_03"), ("clipin", "wfoly_plr_sn_hsierra_reload_03"), ("clip_in", "wfoly_plr_sn_hsierra_reload_03"), ("slide", "wfoly_plr_sn_hsierra_reload_empty_04"), ("chamber", "wfoly_plr_sn_hsierra_reload_empty_04"), ("release", "wfoly_plr_sn_hsierra_reload_empty_04"), ("bolt", "wfoly_plr_sn_hsierra_reload_empty_04"), ("charge", "wfoly_plr_sn_hsierra_reload_empty_04"), ("lift", "wfoly_plr_sn_hsierra_reload_01"), ("grab", "wfoly_plr_sn_hsierra_reload_01"), ("hand", "wfoly_plr_sn_hsierra_reload_01"), ("raise", "wfoly_plr_sn_hsierra_raise_quick_01"), ("pullout", "wfoly_plr_sn_hsierra_raise_quick_01"), ("drop", "wfoly_plr_sn_hsierra_drop_quick_01"), ("putaway", "wfoly_plr_sn_hsierra_drop_quick_01")],
        layers: &[("wpn_sn_hsierra_fire_plr_lfe", "wpn_sn_hsierra_fire_plr_atmo"), ("wpn_sn_hsierra_fire_npc_mech", "wpn_sn_hsierra_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_mike4",
        fire: "weap_mike4_fire_npc",
        fire_player: "weap_mike4_fire_plr_shot",
        fire_last_player: "weap_mike4_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_mike4_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_mike4_raise_first_01",
        putaway_player: "wfoly_plr_ar_mike4_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_mike4_reload_02"), ("mag_out", "wfoly_plr_ar_mike4_reload_02"), ("clipout", "wfoly_plr_ar_mike4_reload_02"), ("clip_out", "wfoly_plr_ar_mike4_reload_02"), ("magin", "wfoly_plr_ar_mike4_reload_03"), ("mag_in", "wfoly_plr_ar_mike4_reload_03"), ("clipin", "wfoly_plr_ar_mike4_reload_03"), ("clip_in", "wfoly_plr_ar_mike4_reload_03"), ("slide", "wfoly_plr_ar_mike4_reload_empty_04"), ("chamber", "wfoly_plr_ar_mike4_reload_empty_04"), ("release", "wfoly_plr_ar_mike4_reload_empty_04"), ("bolt", "wfoly_plr_ar_mike4_reload_empty_04"), ("charge", "wfoly_plr_ar_mike4_reload_empty_04"), ("lift", "wfoly_plr_ar_mike4_reload_01"), ("grab", "wfoly_plr_ar_mike4_reload_01"), ("hand", "wfoly_plr_ar_mike4_reload_01"), ("raise", "wfoly_plr_ar_mike4_raise_quick_01"), ("pullout", "wfoly_plr_ar_mike4_raise_quick_01"), ("drop", "wfoly_plr_ar_mike4_drop_quick_01"), ("putaway", "wfoly_plr_ar_mike4_drop_quick_01")],
        layers: &[("weap_mike4_fire_plr_lfe", "weap_mike4_fire_plr_atmo"), ("weap_mike4_fire_npc_mech", "weap_mike4_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_mike16",
        fire: "weap_mike16_fire_npc",
        fire_player: "weap_mike16_fire_plr_shot",
        fire_last_player: "weap_mike16_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_mike16_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_mike16_raise_first_01",
        putaway_player: "wfoly_plr_ar_mike16_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_mike16_reload_02"), ("mag_out", "wfoly_plr_ar_mike16_reload_02"), ("clipout", "wfoly_plr_ar_mike16_reload_02"), ("clip_out", "wfoly_plr_ar_mike16_reload_02"), ("magin", "wfoly_plr_ar_mike16_reload_03"), ("mag_in", "wfoly_plr_ar_mike16_reload_03"), ("clipin", "wfoly_plr_ar_mike16_reload_03"), ("clip_in", "wfoly_plr_ar_mike16_reload_03"), ("slide", "wfoly_plr_ar_mike16_reload_empty_04"), ("chamber", "wfoly_plr_ar_mike16_reload_empty_04"), ("release", "wfoly_plr_ar_mike16_reload_empty_04"), ("bolt", "wfoly_plr_ar_mike16_reload_empty_04"), ("charge", "wfoly_plr_ar_mike16_reload_empty_04"), ("lift", "wfoly_plr_ar_mike16_reload_01"), ("grab", "wfoly_plr_ar_mike16_reload_01"), ("hand", "wfoly_plr_ar_mike16_reload_01"), ("raise", "wfoly_plr_ar_mike16_raise_quick_01"), ("pullout", "wfoly_plr_ar_mike16_raise_quick_01"), ("drop", "wfoly_plr_ar_mike16_drop_quick_01"), ("putaway", "wfoly_plr_ar_mike16_drop_quick_01")],
        layers: &[("weap_mike16_fire_plr_lfe", "weap_mike16_fire_plr_atmo"), ("weap_mike16_fire_npc_mech", "weap_mike16_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_akilo",
        fire: "weap_akilo_fire_npc",
        fire_player: "weap_akilo_fire_plr_shot",
        fire_last_player: "weap_akilo_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_akilo_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_akilo_raise_first_01",
        putaway_player: "wfoly_plr_ar_akilo_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_akilo_reload_02"), ("mag_out", "wfoly_plr_ar_akilo_reload_02"), ("clipout", "wfoly_plr_ar_akilo_reload_02"), ("clip_out", "wfoly_plr_ar_akilo_reload_02"), ("magin", "wfoly_plr_ar_akilo_reload_03"), ("mag_in", "wfoly_plr_ar_akilo_reload_03"), ("clipin", "wfoly_plr_ar_akilo_reload_03"), ("clip_in", "wfoly_plr_ar_akilo_reload_03"), ("slide", "wfoly_plr_ar_akilo_reload_empty_04"), ("chamber", "wfoly_plr_ar_akilo_reload_empty_04"), ("release", "wfoly_plr_ar_akilo_reload_empty_04"), ("bolt", "wfoly_plr_ar_akilo_reload_empty_04"), ("charge", "wfoly_plr_ar_akilo_reload_empty_04"), ("lift", "wfoly_plr_ar_akilo_reload_01"), ("grab", "wfoly_plr_ar_akilo_reload_01"), ("hand", "wfoly_plr_ar_akilo_reload_01"), ("raise", "wfoly_plr_ar_akilo_raise_quick_01"), ("pullout", "wfoly_plr_ar_akilo_raise_quick_01"), ("drop", "wfoly_plr_ar_akilo_drop_quick_01"), ("putaway", "wfoly_plr_ar_akilo_drop_quick_01")],
        layers: &[("weap_akilo_fire_plr_lfe", "weap_akilo_fire_plr_atmo"), ("weap_akilo_fire_npc_mech", "weap_akilo_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_akilo105",
        fire: "weap_akilo105_fire_npc",
        fire_player: "weap_akilo105_fire_plr_shot",
        fire_last_player: "weap_akilo105_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_akilo105_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_akilo105_raise_first_01",
        putaway_player: "wfoly_plr_ar_akilo105_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_akilo105_reload_02"), ("mag_out", "wfoly_plr_ar_akilo105_reload_02"), ("clipout", "wfoly_plr_ar_akilo105_reload_02"), ("clip_out", "wfoly_plr_ar_akilo105_reload_02"), ("magin", "wfoly_plr_ar_akilo105_reload_03"), ("mag_in", "wfoly_plr_ar_akilo105_reload_03"), ("clipin", "wfoly_plr_ar_akilo105_reload_03"), ("clip_in", "wfoly_plr_ar_akilo105_reload_03"), ("slide", "wfoly_plr_ar_akilo105_reload_empty_04"), ("chamber", "wfoly_plr_ar_akilo105_reload_empty_04"), ("release", "wfoly_plr_ar_akilo105_reload_empty_04"), ("bolt", "wfoly_plr_ar_akilo105_reload_empty_04"), ("charge", "wfoly_plr_ar_akilo105_reload_empty_04"), ("lift", "wfoly_plr_ar_akilo105_reload_01"), ("grab", "wfoly_plr_ar_akilo105_reload_01"), ("hand", "wfoly_plr_ar_akilo105_reload_01"), ("raise", "wfoly_plr_ar_akilo105_raise_quick_01"), ("pullout", "wfoly_plr_ar_akilo105_raise_quick_01"), ("drop", "wfoly_plr_ar_akilo105_drop_quick_01"), ("putaway", "wfoly_plr_ar_akilo105_drop_quick_01")],
        layers: &[("weap_akilo105_fire_plr_lfe", "weap_akilo105_fire_plr_atmo"), ("weap_akilo105_fire_npc_mech", "weap_akilo105_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_akilo74",
        fire: "weap_akilo74_fire_npc",
        fire_player: "weap_akilo74_fire_plr_shot",
        fire_last_player: "weap_akilo74_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_akilo74_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_akilo74_raise_first_01",
        putaway_player: "wfoly_plr_ar_akilo74_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_akilo74_reload_02"), ("mag_out", "wfoly_plr_ar_akilo74_reload_02"), ("clipout", "wfoly_plr_ar_akilo74_reload_02"), ("clip_out", "wfoly_plr_ar_akilo74_reload_02"), ("magin", "wfoly_plr_ar_akilo74_reload_03"), ("mag_in", "wfoly_plr_ar_akilo74_reload_03"), ("clipin", "wfoly_plr_ar_akilo74_reload_03"), ("clip_in", "wfoly_plr_ar_akilo74_reload_03"), ("slide", "wfoly_plr_ar_akilo74_reload_empty_04"), ("chamber", "wfoly_plr_ar_akilo74_reload_empty_04"), ("release", "wfoly_plr_ar_akilo74_reload_empty_04"), ("bolt", "wfoly_plr_ar_akilo74_reload_empty_04"), ("charge", "wfoly_plr_ar_akilo74_reload_empty_04"), ("lift", "wfoly_plr_ar_akilo74_reload_01"), ("grab", "wfoly_plr_ar_akilo74_reload_01"), ("hand", "wfoly_plr_ar_akilo74_reload_01"), ("raise", "wfoly_plr_ar_akilo74_raise_quick_01"), ("pullout", "wfoly_plr_ar_akilo74_raise_quick_01"), ("drop", "wfoly_plr_ar_akilo74_drop_quick_01"), ("putaway", "wfoly_plr_ar_akilo74_drop_quick_01")],
        layers: &[("weap_akilo74_fire_plr_lfe", "weap_akilo74_fire_plr_atmo"), ("weap_akilo74_fire_npc_mech", "weap_akilo74_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_kilo53",
        fire: "weap_kilo53_fire_npc",
        fire_player: "weap_kilo53_fire_plr_shot",
        fire_last_player: "weap_kilo53_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_kilo53_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_kilo53_raise_first_01",
        putaway_player: "wfoly_plr_ar_kilo53_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_kilo53_reload_02"), ("mag_out", "wfoly_plr_ar_kilo53_reload_02"), ("clipout", "wfoly_plr_ar_kilo53_reload_02"), ("clip_out", "wfoly_plr_ar_kilo53_reload_02"), ("magin", "wfoly_plr_ar_kilo53_reload_03"), ("mag_in", "wfoly_plr_ar_kilo53_reload_03"), ("clipin", "wfoly_plr_ar_kilo53_reload_03"), ("clip_in", "wfoly_plr_ar_kilo53_reload_03"), ("slide", "wfoly_plr_ar_kilo53_reload_empty_04"), ("chamber", "wfoly_plr_ar_kilo53_reload_empty_04"), ("release", "wfoly_plr_ar_kilo53_reload_empty_04"), ("bolt", "wfoly_plr_ar_kilo53_reload_empty_04"), ("charge", "wfoly_plr_ar_kilo53_reload_empty_04"), ("lift", "wfoly_plr_ar_kilo53_reload_01"), ("grab", "wfoly_plr_ar_kilo53_reload_01"), ("hand", "wfoly_plr_ar_kilo53_reload_01"), ("raise", "wfoly_plr_ar_kilo53_raise_quick_01"), ("pullout", "wfoly_plr_ar_kilo53_raise_quick_01"), ("drop", "wfoly_plr_ar_kilo53_drop_quick_01"), ("putaway", "wfoly_plr_ar_kilo53_drop_quick_01")],
        layers: &[("weap_kilo53_fire_plr_lfe", "weap_kilo53_fire_plr_atmo"), ("weap_kilo53_fire_npc_mech", "weap_kilo53_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_golf3",
        fire: "weap_golf3_fire_npc",
        fire_player: "weap_golf3_fire_plr_shot",
        fire_last_player: "weap_golf3_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_golf3_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_golf3_raise_first_01",
        putaway_player: "wfoly_plr_ar_golf3_drop",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_golf3_reload_02"), ("mag_out", "wfoly_plr_ar_golf3_reload_02"), ("clipout", "wfoly_plr_ar_golf3_reload_02"), ("clip_out", "wfoly_plr_ar_golf3_reload_02"), ("magin", "wfoly_plr_ar_golf3_reload_03"), ("mag_in", "wfoly_plr_ar_golf3_reload_03"), ("clipin", "wfoly_plr_ar_golf3_reload_03"), ("clip_in", "wfoly_plr_ar_golf3_reload_03"), ("slide", "wfoly_plr_ar_golf3_reload_empty_04"), ("chamber", "wfoly_plr_ar_golf3_reload_empty_04"), ("release", "wfoly_plr_ar_golf3_reload_empty_04"), ("bolt", "wfoly_plr_ar_golf3_reload_empty_04"), ("charge", "wfoly_plr_ar_golf3_reload_empty_04"), ("lift", "wfoly_plr_ar_golf3_reload_01"), ("grab", "wfoly_plr_ar_golf3_reload_01"), ("hand", "wfoly_plr_ar_golf3_reload_01"), ("raise", "wfoly_plr_ar_golf3_raise_quick_01"), ("pullout", "wfoly_plr_ar_golf3_raise_quick_01"), ("drop", "wfoly_plr_ar_golf3_drop"), ("putaway", "wfoly_plr_ar_golf3_drop")],
        layers: &[("weap_golf3_fire_plr_lfe", "weap_golf3_fire_plr_atmo"), ("weap_golf3_fire_npc_mech", "weap_golf3_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_scharlie",
        fire: "weap_scharlie_fire_npc",
        fire_player: "weap_scharlie_fire_plr_shot",
        fire_last_player: "weap_scharlie_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_scharlie_raise",
        first_raise_player: "wfoly_plr_ar_scharlie_raise_first_01",
        putaway_player: "wfoly_plr_ar_scharlie_drop",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_scharlie_reload_02"), ("mag_out", "wfoly_plr_ar_scharlie_reload_02"), ("clipout", "wfoly_plr_ar_scharlie_reload_02"), ("clip_out", "wfoly_plr_ar_scharlie_reload_02"), ("magin", "wfoly_plr_ar_scharlie_reload_03"), ("mag_in", "wfoly_plr_ar_scharlie_reload_03"), ("clipin", "wfoly_plr_ar_scharlie_reload_03"), ("clip_in", "wfoly_plr_ar_scharlie_reload_03"), ("slide", "wfoly_plr_ar_scharlie_reload_empty_04"), ("chamber", "wfoly_plr_ar_scharlie_reload_empty_04"), ("release", "wfoly_plr_ar_scharlie_reload_empty_04"), ("bolt", "wfoly_plr_ar_scharlie_reload_empty_04"), ("charge", "wfoly_plr_ar_scharlie_reload_empty_04"), ("lift", "wfoly_plr_ar_scharlie_reload_01"), ("grab", "wfoly_plr_ar_scharlie_reload_01"), ("hand", "wfoly_plr_ar_scharlie_reload_01"), ("raise", "wfoly_plr_ar_scharlie_raise"), ("pullout", "wfoly_plr_ar_scharlie_raise"), ("drop", "wfoly_plr_ar_scharlie_drop"), ("putaway", "wfoly_plr_ar_scharlie_drop")],
        layers: &[("weap_scharlie_fire_plr_lfe", "weap_scharlie_fire_plr_atmo"), ("weap_scharlie_fire_npc_mech", "weap_scharlie_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_mcbravo",
        fire: "weap_mcbravo_fire_npc",
        fire_player: "weap_mcbravo_fire_plr_shot",
        fire_last_player: "weap_mcbravo_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_mcbravo_raise_01",
        first_raise_player: "wfoly_plr_ar_mcbravo_raise_first_01",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_mcbravo_reload_02"), ("mag_out", "wfoly_plr_ar_mcbravo_reload_02"), ("clipout", "wfoly_plr_ar_mcbravo_reload_02"), ("clip_out", "wfoly_plr_ar_mcbravo_reload_02"), ("magin", "wfoly_plr_ar_mcbravo_reload_03"), ("mag_in", "wfoly_plr_ar_mcbravo_reload_03"), ("clipin", "wfoly_plr_ar_mcbravo_reload_03"), ("clip_in", "wfoly_plr_ar_mcbravo_reload_03"), ("slide", "wfoly_plr_ar_mcbravo_reload_empty_04"), ("chamber", "wfoly_plr_ar_mcbravo_reload_empty_04"), ("release", "wfoly_plr_ar_mcbravo_reload_empty_04"), ("bolt", "wfoly_plr_ar_mcbravo_reload_empty_04"), ("charge", "wfoly_plr_ar_mcbravo_reload_empty_04"), ("lift", "wfoly_plr_ar_mcbravo_reload_01"), ("grab", "wfoly_plr_ar_mcbravo_reload_01"), ("hand", "wfoly_plr_ar_mcbravo_reload_01"), ("raise", "wfoly_plr_ar_mcbravo_raise_01"), ("pullout", "wfoly_plr_ar_mcbravo_raise_01")],
        layers: &[("weap_mcbravo_fire_plr_lfe", "weap_mcbravo_fire_plr_atmo"), ("weap_mcbravo_fire_npc_mech", "weap_mcbravo_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_mcharlie",
        fire: "weap_mcharlie_fire_npc",
        fire_player: "weap_mcharlie_fire_plr_shot",
        fire_last_player: "weap_mcharlie_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_mcharlie_raise_01",
        first_raise_player: "wfoly_plr_ar_mcharlie_raise_first_01",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_mcharlie_reload_02"), ("mag_out", "wfoly_plr_ar_mcharlie_reload_02"), ("clipout", "wfoly_plr_ar_mcharlie_reload_02"), ("clip_out", "wfoly_plr_ar_mcharlie_reload_02"), ("magin", "wfoly_plr_ar_mcharlie_reload_03"), ("mag_in", "wfoly_plr_ar_mcharlie_reload_03"), ("clipin", "wfoly_plr_ar_mcharlie_reload_03"), ("clip_in", "wfoly_plr_ar_mcharlie_reload_03"), ("slide", "wfoly_plr_ar_mcharlie_reload_empty_04"), ("chamber", "wfoly_plr_ar_mcharlie_reload_empty_04"), ("release", "wfoly_plr_ar_mcharlie_reload_empty_04"), ("bolt", "wfoly_plr_ar_mcharlie_reload_empty_04"), ("charge", "wfoly_plr_ar_mcharlie_reload_empty_04"), ("lift", "wfoly_plr_ar_mcharlie_reload_01"), ("grab", "wfoly_plr_ar_mcharlie_reload_01"), ("hand", "wfoly_plr_ar_mcharlie_reload_01"), ("raise", "wfoly_plr_ar_mcharlie_raise_01"), ("pullout", "wfoly_plr_ar_mcharlie_raise_01")],
        layers: &[("weap_mcharlie_fire_plr_lfe", "weap_mcharlie_fire_plr_atmo"), ("weap_mcharlie_fire_npc_mech", "weap_mcharlie_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_ar_falpha",
        fire: "weap_falpha_fire_npc",
        fire_player: "weap_falpha_fire_plr_shot",
        fire_last_player: "weap_falpha_fire_plr_last_mech",
        raise_player: "wfoly_plr_ar_falpha_raise_quick_01",
        first_raise_player: "wfoly_plr_ar_falpha_raise_first_01",
        putaway_player: "wfoly_plr_ar_falpha_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_ar_falpha_reload_02"), ("mag_out", "wfoly_plr_ar_falpha_reload_02"), ("clipout", "wfoly_plr_ar_falpha_reload_02"), ("clip_out", "wfoly_plr_ar_falpha_reload_02"), ("magin", "wfoly_plr_ar_falpha_reload_03"), ("mag_in", "wfoly_plr_ar_falpha_reload_03"), ("clipin", "wfoly_plr_ar_falpha_reload_03"), ("clip_in", "wfoly_plr_ar_falpha_reload_03"), ("slide", "wfoly_plr_ar_falpha_reload_empty_04"), ("chamber", "wfoly_plr_ar_falpha_reload_empty_04"), ("release", "wfoly_plr_ar_falpha_reload_empty_04"), ("bolt", "wfoly_plr_ar_falpha_reload_empty_04"), ("charge", "wfoly_plr_ar_falpha_reload_empty_04"), ("lift", "wfoly_plr_ar_falpha_reload_01"), ("grab", "wfoly_plr_ar_falpha_reload_01"), ("hand", "wfoly_plr_ar_falpha_reload_01"), ("raise", "wfoly_plr_ar_falpha_raise_quick_01"), ("pullout", "wfoly_plr_ar_falpha_raise_quick_01"), ("drop", "wfoly_plr_ar_falpha_drop_quick_01"), ("putaway", "wfoly_plr_ar_falpha_drop_quick_01")],
        layers: &[("weap_falpha_fire_plr_lfe", "weap_falpha_fire_plr_atmo"), ("weap_falpha_fire_npc_mech", "weap_falpha_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_soscar14",
        fire: "weap_soscar14_fire_npc",
        fire_player: "weap_soscar14_fire_plr_shot",
        fire_last_player: "weap_soscar14_fire_plr_last_mech",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[("weap_soscar14_fire_plr_lfe", "weap_soscar14_fire_plr_atmo"), ("weap_soscar14_fire_npc_mech", "weap_soscar14_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_scromeo",
        fire: "weap_scromeo_fire_npc",
        fire_player: "weap_scromeo_fire_plr_shot",
        fire_last_player: "weap_scromeo_fire_plr_last_mech",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[("weap_scromeo_fire_plr_lfe", "weap_scromeo_fire_plr_atmo"), ("weap_scromeo_fire_npc_mech", "weap_scromeo_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_mike14",
        fire: "weap_mike14_fire_npc",
        fire_player: "weap_mike14_fire_plr_shot",
        fire_last_player: "weap_mike14_fire_plr_last_mech",
        raise_player: "wfoly_plr_dm_mike14_raise_quick_01",
        first_raise_player: "wfoly_plr_dm_mike14_raise_first_01",
        putaway_player: "wfoly_plr_dm_mike14_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_dm_mike14_reload_02"), ("mag_out", "wfoly_plr_dm_mike14_reload_02"), ("clipout", "wfoly_plr_dm_mike14_reload_02"), ("clip_out", "wfoly_plr_dm_mike14_reload_02"), ("magin", "wfoly_plr_dm_mike14_reload_03"), ("mag_in", "wfoly_plr_dm_mike14_reload_03"), ("clipin", "wfoly_plr_dm_mike14_reload_03"), ("clip_in", "wfoly_plr_dm_mike14_reload_03"), ("slide", "wfoly_plr_dm_mike14_reload_empty_04"), ("chamber", "wfoly_plr_dm_mike14_reload_empty_04"), ("release", "wfoly_plr_dm_mike14_reload_empty_04"), ("bolt", "wfoly_plr_dm_mike14_reload_empty_04"), ("charge", "wfoly_plr_dm_mike14_reload_empty_04"), ("lift", "wfoly_plr_dm_mike14_reload_01"), ("grab", "wfoly_plr_dm_mike14_reload_01"), ("hand", "wfoly_plr_dm_mike14_reload_01"), ("raise", "wfoly_plr_dm_mike14_raise_quick_01"), ("pullout", "wfoly_plr_dm_mike14_raise_quick_01"), ("drop", "wfoly_plr_dm_mike14_drop_quick_01"), ("putaway", "wfoly_plr_dm_mike14_drop_quick_01")],
        layers: &[("weap_mike14_fire_plr_lfe", "weap_mike14_fire_plr_atmo"), ("weap_mike14_fire_npc_mech", "weap_mike14_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_stango25",
        fire: "weap_stango25_fire_npc",
        fire_player: "weap_stango25_fire_plr_shot",
        fire_last_player: "weap_stango25_fire_plr_last_mech",
        raise_player: "wfoly_plr_dm_stango25_raise_quick_01",
        first_raise_player: "wfoly_plr_dm_stango25_raise_first_01",
        putaway_player: "wfoly_plr_dm_stango25_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_dm_stango25_reload_02"), ("mag_out", "wfoly_plr_dm_stango25_reload_02"), ("clipout", "wfoly_plr_dm_stango25_reload_02"), ("clip_out", "wfoly_plr_dm_stango25_reload_02"), ("magin", "wfoly_plr_dm_stango25_reload_03"), ("mag_in", "wfoly_plr_dm_stango25_reload_03"), ("clipin", "wfoly_plr_dm_stango25_reload_03"), ("clip_in", "wfoly_plr_dm_stango25_reload_03"), ("slide", "wfoly_plr_dm_stango25_reload_empty_04"), ("chamber", "wfoly_plr_dm_stango25_reload_empty_04"), ("release", "wfoly_plr_dm_stango25_reload_empty_04"), ("bolt", "wfoly_plr_dm_stango25_reload_empty_04"), ("charge", "wfoly_plr_dm_stango25_reload_empty_04"), ("lift", "wfoly_plr_dm_stango25_reload_01"), ("grab", "wfoly_plr_dm_stango25_reload_01"), ("hand", "wfoly_plr_dm_stango25_reload_01"), ("raise", "wfoly_plr_dm_stango25_raise_quick_01"), ("pullout", "wfoly_plr_dm_stango25_raise_quick_01"), ("drop", "wfoly_plr_dm_stango25_drop_quick_01"), ("putaway", "wfoly_plr_dm_stango25_drop_quick_01")],
        layers: &[("weap_stango25_fire_plr_lfe", "weap_stango25_fire_plr_atmo"), ("weap_stango25_fire_npc_mech", "weap_stango25_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_pgolf1",
        fire: "weap_pgolf1_fire_npc",
        fire_player: "weap_pgolf1_fire_plr_shot",
        fire_last_player: "weap_pgolf1_fire_plr_last_mech",
        raise_player: "",
        first_raise_player: "",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[],
        layers: &[("weap_pgolf1_fire_plr_lfe", "weap_pgolf1_fire_plr_atmo"), ("weap_pgolf1_fire_npc_mech", "weap_pgolf1_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_dm_mike24",
        fire: "weap_mike24_fire_npc",
        fire_player: "weap_mike24_fire_plr_shot",
        fire_last_player: "weap_mike24_fire_plr_last_mech",
        raise_player: "wfoly_plr_dm_mike24_raise_quick_01",
        first_raise_player: "wfoly_plr_dm_mike24_raise_first_01",
        putaway_player: "wfoly_plr_dm_mike24_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_dm_mike24_reload_02"), ("mag_out", "wfoly_plr_dm_mike24_reload_02"), ("clipout", "wfoly_plr_dm_mike24_reload_02"), ("clip_out", "wfoly_plr_dm_mike24_reload_02"), ("magin", "wfoly_plr_dm_mike24_reload_03"), ("mag_in", "wfoly_plr_dm_mike24_reload_03"), ("clipin", "wfoly_plr_dm_mike24_reload_03"), ("clip_in", "wfoly_plr_dm_mike24_reload_03"), ("slide", "wfoly_plr_dm_mike24_reload_empty_04"), ("chamber", "wfoly_plr_dm_mike24_reload_empty_04"), ("release", "wfoly_plr_dm_mike24_reload_empty_04"), ("bolt", "wfoly_plr_dm_mike24_reload_empty_04"), ("charge", "wfoly_plr_dm_mike24_reload_empty_04"), ("lift", "wfoly_plr_dm_mike24_reload_01"), ("grab", "wfoly_plr_dm_mike24_reload_01"), ("hand", "wfoly_plr_dm_mike24_reload_01"), ("raise", "wfoly_plr_dm_mike24_raise_quick_01"), ("pullout", "wfoly_plr_dm_mike24_raise_quick_01"), ("drop", "wfoly_plr_dm_mike24_drop_quick_01"), ("putaway", "wfoly_plr_dm_mike24_drop_quick_01")],
        layers: &[("weap_mike24_fire_plr_lfe", "weap_mike24_fire_plr_atmo"), ("weap_mike24_fire_npc_mech", "weap_mike24_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_mpapa5",
        fire: "weap_mpapa5_fire_npc",
        fire_player: "weap_mpapa5_fire_plr_shot",
        fire_last_player: "weap_mpapa5_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_mpapa5_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_mpapa5_raise_first_01",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_mpapa5_reload_02"), ("mag_out", "wfoly_plr_sm_mpapa5_reload_02"), ("clipout", "wfoly_plr_sm_mpapa5_reload_02"), ("clip_out", "wfoly_plr_sm_mpapa5_reload_02"), ("magin", "wfoly_plr_sm_mpapa5_reload_03"), ("mag_in", "wfoly_plr_sm_mpapa5_reload_03"), ("clipin", "wfoly_plr_sm_mpapa5_reload_03"), ("clip_in", "wfoly_plr_sm_mpapa5_reload_03"), ("slide", "wfoly_plr_sm_mpapa5_reload_empty_04"), ("chamber", "wfoly_plr_sm_mpapa5_reload_empty_04"), ("release", "wfoly_plr_sm_mpapa5_reload_empty_04"), ("bolt", "wfoly_plr_sm_mpapa5_reload_empty_04"), ("charge", "wfoly_plr_sm_mpapa5_reload_empty_04"), ("lift", "wfoly_plr_sm_mpapa5_reload_01"), ("grab", "wfoly_plr_sm_mpapa5_reload_01"), ("hand", "wfoly_plr_sm_mpapa5_reload_01"), ("raise", "wfoly_plr_sm_mpapa5_raise_quick_01"), ("pullout", "wfoly_plr_sm_mpapa5_raise_quick_01")],
        layers: &[("weap_mpapa5_fire_plr_lfe", "weap_mpapa5_fire_plr_atmo"), ("weap_mpapa5_fire_npc_mech", "weap_mpapa5_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_mpapa7",
        fire: "weap_mpapa7_fire_npc",
        fire_player: "weap_mpapa7_fire_plr_shot",
        fire_last_player: "weap_mpapa7_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_mpapa7_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_mpapa7_raise_first_01",
        putaway_player: "wfoly_plr_sm_mpapa7_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_mpapa7_reload_02"), ("mag_out", "wfoly_plr_sm_mpapa7_reload_02"), ("clipout", "wfoly_plr_sm_mpapa7_reload_02"), ("clip_out", "wfoly_plr_sm_mpapa7_reload_02"), ("magin", "wfoly_plr_sm_mpapa7_reload_03"), ("mag_in", "wfoly_plr_sm_mpapa7_reload_03"), ("clipin", "wfoly_plr_sm_mpapa7_reload_03"), ("clip_in", "wfoly_plr_sm_mpapa7_reload_03"), ("slide", "wfoly_plr_sm_mpapa7_reload_empty_04"), ("chamber", "wfoly_plr_sm_mpapa7_reload_empty_04"), ("release", "wfoly_plr_sm_mpapa7_reload_empty_04"), ("bolt", "wfoly_plr_sm_mpapa7_reload_empty_04"), ("charge", "wfoly_plr_sm_mpapa7_reload_empty_04"), ("lift", "wfoly_plr_sm_mpapa7_reload_01"), ("grab", "wfoly_plr_sm_mpapa7_reload_01"), ("hand", "wfoly_plr_sm_mpapa7_reload_01"), ("raise", "wfoly_plr_sm_mpapa7_raise_quick_01"), ("pullout", "wfoly_plr_sm_mpapa7_raise_quick_01"), ("drop", "wfoly_plr_sm_mpapa7_drop_quick_01"), ("putaway", "wfoly_plr_sm_mpapa7_drop_quick_01")],
        layers: &[("weap_mpapa7_fire_plr_lfe", "weap_mpapa7_fire_plr_atmo"), ("weap_mpapa7_fire_npc_mech", "weap_mpapa7_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_papa90",
        fire: "weap_papa90_fire_npc",
        fire_player: "weap_papa90_fire_plr_shot",
        fire_last_player: "weap_papa90_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_papa90_raise",
        first_raise_player: "wfoly_plr_sm_papa90_raise_first_01",
        putaway_player: "wfoly_plr_sm_papa90_drop",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("raise", "wfoly_plr_sm_papa90_raise"), ("pullout", "wfoly_plr_sm_papa90_raise"), ("drop", "wfoly_plr_sm_papa90_drop"), ("putaway", "wfoly_plr_sm_papa90_drop")],
        layers: &[("weap_papa90_fire_plr_lfe", "weap_papa90_fire_plr_atmo"), ("weap_papa90_fire_npc_mech", "weap_papa90_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_victor",
        fire: "weap_victor_fire_npc",
        fire_player: "weap_victor_fire_plr_shot",
        fire_last_player: "weap_victor_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_victor_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_victor_raise_first_01",
        putaway_player: "wfoly_plr_sm_victor_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_victor_reload_02"), ("mag_out", "wfoly_plr_sm_victor_reload_02"), ("clipout", "wfoly_plr_sm_victor_reload_02"), ("clip_out", "wfoly_plr_sm_victor_reload_02"), ("magin", "wfoly_plr_sm_victor_reload_03"), ("mag_in", "wfoly_plr_sm_victor_reload_03"), ("clipin", "wfoly_plr_sm_victor_reload_03"), ("clip_in", "wfoly_plr_sm_victor_reload_03"), ("slide", "wfoly_plr_sm_victor_reload_empty_04"), ("chamber", "wfoly_plr_sm_victor_reload_empty_04"), ("release", "wfoly_plr_sm_victor_reload_empty_04"), ("bolt", "wfoly_plr_sm_victor_reload_empty_04"), ("charge", "wfoly_plr_sm_victor_reload_empty_04"), ("lift", "wfoly_plr_sm_victor_reload_01"), ("grab", "wfoly_plr_sm_victor_reload_01"), ("hand", "wfoly_plr_sm_victor_reload_01"), ("raise", "wfoly_plr_sm_victor_raise_quick_01"), ("pullout", "wfoly_plr_sm_victor_raise_quick_01"), ("drop", "wfoly_plr_sm_victor_drop_quick_01"), ("putaway", "wfoly_plr_sm_victor_drop_quick_01")],
        layers: &[("weap_victor_fire_plr_lfe", "weap_victor_fire_plr_atmo"), ("weap_victor_fire_npc_mech", "weap_victor_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_beta",
        fire: "weap_beta_fire_npc",
        fire_player: "weap_beta_fire_plr_shot",
        fire_last_player: "weap_beta_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_beta_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_beta_raise_first_01",
        putaway_player: "wfoly_plr_sm_beta_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_beta_reload_02"), ("mag_out", "wfoly_plr_sm_beta_reload_02"), ("clipout", "wfoly_plr_sm_beta_reload_02"), ("clip_out", "wfoly_plr_sm_beta_reload_02"), ("magin", "wfoly_plr_sm_beta_reload_03"), ("mag_in", "wfoly_plr_sm_beta_reload_03"), ("clipin", "wfoly_plr_sm_beta_reload_03"), ("clip_in", "wfoly_plr_sm_beta_reload_03"), ("slide", "wfoly_plr_sm_beta_reload_empty_04"), ("chamber", "wfoly_plr_sm_beta_reload_empty_04"), ("release", "wfoly_plr_sm_beta_reload_empty_04"), ("bolt", "wfoly_plr_sm_beta_reload_empty_04"), ("charge", "wfoly_plr_sm_beta_reload_empty_04"), ("lift", "wfoly_plr_sm_beta_reload_01"), ("grab", "wfoly_plr_sm_beta_reload_01"), ("hand", "wfoly_plr_sm_beta_reload_01"), ("raise", "wfoly_plr_sm_beta_raise_quick_01"), ("pullout", "wfoly_plr_sm_beta_raise_quick_01"), ("drop", "wfoly_plr_sm_beta_drop_quick_01"), ("putaway", "wfoly_plr_sm_beta_drop_quick_01")],
        layers: &[("weap_beta_fire_plr_lfe", "weap_beta_fire_plr_atmo"), ("weap_beta_fire_npc_mech", "weap_beta_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_alpha57",
        fire: "weap_alpha57_fire_npc",
        fire_player: "weap_alpha57_fire_plr_shot",
        fire_last_player: "weap_alpha57_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_alpha57_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_alpha57_raise_first_01",
        putaway_player: "wfoly_plr_sm_alpha57_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_alpha57_reload_02"), ("mag_out", "wfoly_plr_sm_alpha57_reload_02"), ("clipout", "wfoly_plr_sm_alpha57_reload_02"), ("clip_out", "wfoly_plr_sm_alpha57_reload_02"), ("magin", "wfoly_plr_sm_alpha57_reload_03"), ("mag_in", "wfoly_plr_sm_alpha57_reload_03"), ("clipin", "wfoly_plr_sm_alpha57_reload_03"), ("clip_in", "wfoly_plr_sm_alpha57_reload_03"), ("slide", "wfoly_plr_sm_alpha57_reload_empty_04"), ("chamber", "wfoly_plr_sm_alpha57_reload_empty_04"), ("release", "wfoly_plr_sm_alpha57_reload_empty_04"), ("bolt", "wfoly_plr_sm_alpha57_reload_empty_04"), ("charge", "wfoly_plr_sm_alpha57_reload_empty_04"), ("lift", "wfoly_plr_sm_alpha57_reload_01"), ("grab", "wfoly_plr_sm_alpha57_reload_01"), ("hand", "wfoly_plr_sm_alpha57_reload_01"), ("raise", "wfoly_plr_sm_alpha57_raise_quick_01"), ("pullout", "wfoly_plr_sm_alpha57_raise_quick_01"), ("drop", "wfoly_plr_sm_alpha57_drop_quick_01"), ("putaway", "wfoly_plr_sm_alpha57_drop_quick_01")],
        layers: &[("weap_alpha57_fire_plr_lfe", "weap_alpha57_fire_plr_atmo"), ("weap_alpha57_fire_npc_mech", "weap_alpha57_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_mpapax",
        fire: "weap_mpapax_fire_npc",
        fire_player: "weap_mpapax_fire_plr_shot",
        fire_last_player: "weap_mpapax_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_mpapax_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_mpapax_raise_first_01",
        putaway_player: "wfoly_plr_sm_mpapax_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_mpapax_reload_02"), ("mag_out", "wfoly_plr_sm_mpapax_reload_02"), ("clipout", "wfoly_plr_sm_mpapax_reload_02"), ("clip_out", "wfoly_plr_sm_mpapax_reload_02"), ("magin", "wfoly_plr_sm_mpapax_reload_03"), ("mag_in", "wfoly_plr_sm_mpapax_reload_03"), ("clipin", "wfoly_plr_sm_mpapax_reload_03"), ("clip_in", "wfoly_plr_sm_mpapax_reload_03"), ("slide", "wfoly_plr_sm_mpapax_reload_empty_04"), ("chamber", "wfoly_plr_sm_mpapax_reload_empty_04"), ("release", "wfoly_plr_sm_mpapax_reload_empty_04"), ("bolt", "wfoly_plr_sm_mpapax_reload_empty_04"), ("charge", "wfoly_plr_sm_mpapax_reload_empty_04"), ("lift", "wfoly_plr_sm_mpapax_reload_01"), ("grab", "wfoly_plr_sm_mpapax_reload_01"), ("hand", "wfoly_plr_sm_mpapax_reload_01"), ("raise", "wfoly_plr_sm_mpapax_raise_quick_01"), ("pullout", "wfoly_plr_sm_mpapax_raise_quick_01"), ("drop", "wfoly_plr_sm_mpapax_drop_quick_01"), ("putaway", "wfoly_plr_sm_mpapax_drop_quick_01")],
        layers: &[("weap_mpapax_fire_plr_lfe", "weap_mpapax_fire_plr_atmo"), ("weap_mpapax_fire_npc_mech", "weap_mpapax_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_acharlie45",
        fire: "weap_acharlie45_fire_npc",
        fire_player: "weap_acharlie45_fire_plr_shot",
        fire_last_player: "weap_acharlie45_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_acharlie45_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_acharlie45_raise_first_01",
        putaway_player: "wfoly_plr_sm_acharlie45_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_acharlie45_reload_02"), ("mag_out", "wfoly_plr_sm_acharlie45_reload_02"), ("clipout", "wfoly_plr_sm_acharlie45_reload_02"), ("clip_out", "wfoly_plr_sm_acharlie45_reload_02"), ("magin", "wfoly_plr_sm_acharlie45_reload_03"), ("mag_in", "wfoly_plr_sm_acharlie45_reload_03"), ("clipin", "wfoly_plr_sm_acharlie45_reload_03"), ("clip_in", "wfoly_plr_sm_acharlie45_reload_03"), ("slide", "wfoly_plr_sm_acharlie45_reload_empty_04"), ("chamber", "wfoly_plr_sm_acharlie45_reload_empty_04"), ("release", "wfoly_plr_sm_acharlie45_reload_empty_04"), ("bolt", "wfoly_plr_sm_acharlie45_reload_empty_04"), ("charge", "wfoly_plr_sm_acharlie45_reload_empty_04"), ("lift", "wfoly_plr_sm_acharlie45_reload_01"), ("grab", "wfoly_plr_sm_acharlie45_reload_01"), ("hand", "wfoly_plr_sm_acharlie45_reload_01"), ("raise", "wfoly_plr_sm_acharlie45_raise_quick_01"), ("pullout", "wfoly_plr_sm_acharlie45_raise_quick_01"), ("drop", "wfoly_plr_sm_acharlie45_drop_quick_01"), ("putaway", "wfoly_plr_sm_acharlie45_drop_quick_01")],
        layers: &[("weap_acharlie45_fire_plr_lfe", "weap_acharlie45_fire_plr_atmo"), ("weap_acharlie45_fire_npc_mech", "weap_acharlie45_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_acharlie9",
        fire: "weap_acharlie9_fire_npc",
        fire_player: "weap_acharlie9_fire_plr_shot",
        fire_last_player: "weap_acharlie9_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_acharlie9_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_acharlie9_raise_first_01",
        putaway_player: "wfoly_plr_sm_acharlie9_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_acharlie9_reload_02"), ("mag_out", "wfoly_plr_sm_acharlie9_reload_02"), ("clipout", "wfoly_plr_sm_acharlie9_reload_02"), ("clip_out", "wfoly_plr_sm_acharlie9_reload_02"), ("magin", "wfoly_plr_sm_acharlie9_reload_03"), ("mag_in", "wfoly_plr_sm_acharlie9_reload_03"), ("clipin", "wfoly_plr_sm_acharlie9_reload_03"), ("clip_in", "wfoly_plr_sm_acharlie9_reload_03"), ("slide", "wfoly_plr_sm_acharlie9_reload_empty_04"), ("chamber", "wfoly_plr_sm_acharlie9_reload_empty_04"), ("release", "wfoly_plr_sm_acharlie9_reload_empty_04"), ("bolt", "wfoly_plr_sm_acharlie9_reload_empty_04"), ("charge", "wfoly_plr_sm_acharlie9_reload_empty_04"), ("lift", "wfoly_plr_sm_acharlie9_reload_01"), ("grab", "wfoly_plr_sm_acharlie9_reload_01"), ("hand", "wfoly_plr_sm_acharlie9_reload_01"), ("raise", "wfoly_plr_sm_acharlie9_raise_quick_01"), ("pullout", "wfoly_plr_sm_acharlie9_raise_quick_01"), ("drop", "wfoly_plr_sm_acharlie9_drop_quick_01"), ("putaway", "wfoly_plr_sm_acharlie9_drop_quick_01")],
        layers: &[("weap_acharlie9_fire_plr_lfe", "weap_acharlie9_fire_plr_atmo"), ("weap_acharlie9_fire_npc_mech", "weap_acharlie9_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_aviktor",
        fire: "weap_aviktor_fire_npc",
        fire_player: "weap_aviktor_fire_plr_shot",
        fire_last_player: "weap_aviktor_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_aviktor_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_aviktor_raise_first_01",
        putaway_player: "wfoly_plr_sm_aviktor_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_aviktor_reload_02"), ("mag_out", "wfoly_plr_sm_aviktor_reload_02"), ("clipout", "wfoly_plr_sm_aviktor_reload_02"), ("clip_out", "wfoly_plr_sm_aviktor_reload_02"), ("magin", "wfoly_plr_sm_aviktor_reload_03"), ("mag_in", "wfoly_plr_sm_aviktor_reload_03"), ("clipin", "wfoly_plr_sm_aviktor_reload_03"), ("clip_in", "wfoly_plr_sm_aviktor_reload_03"), ("slide", "wfoly_plr_sm_aviktor_reload_empty_04"), ("chamber", "wfoly_plr_sm_aviktor_reload_empty_04"), ("release", "wfoly_plr_sm_aviktor_reload_empty_04"), ("bolt", "wfoly_plr_sm_aviktor_reload_empty_04"), ("charge", "wfoly_plr_sm_aviktor_reload_empty_04"), ("lift", "wfoly_plr_sm_aviktor_reload_01"), ("grab", "wfoly_plr_sm_aviktor_reload_01"), ("hand", "wfoly_plr_sm_aviktor_reload_01"), ("raise", "wfoly_plr_sm_aviktor_raise_quick_01"), ("pullout", "wfoly_plr_sm_aviktor_raise_quick_01"), ("drop", "wfoly_plr_sm_aviktor_drop_quick_01"), ("putaway", "wfoly_plr_sm_aviktor_drop_quick_01")],
        layers: &[("weap_aviktor_fire_plr_lfe", "weap_aviktor_fire_plr_atmo"), ("weap_aviktor_fire_npc_mech", "weap_aviktor_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sm_apapa",
        fire: "weap_apapa_fire_npc",
        fire_player: "weap_apapa_fire_plr_shot",
        fire_last_player: "weap_apapa_fire_plr_last_mech",
        raise_player: "wfoly_plr_sm_apapa_raise_quick_01",
        first_raise_player: "wfoly_plr_sm_apapa_raise_first_01",
        putaway_player: "wfoly_plr_sm_apapa_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sm_apapa_reload_02"), ("mag_out", "wfoly_plr_sm_apapa_reload_02"), ("clipout", "wfoly_plr_sm_apapa_reload_02"), ("clip_out", "wfoly_plr_sm_apapa_reload_02"), ("magin", "wfoly_plr_sm_apapa_reload_03"), ("mag_in", "wfoly_plr_sm_apapa_reload_03"), ("clipin", "wfoly_plr_sm_apapa_reload_03"), ("clip_in", "wfoly_plr_sm_apapa_reload_03"), ("slide", "wfoly_plr_sm_apapa_reload_empty_04"), ("chamber", "wfoly_plr_sm_apapa_reload_empty_04"), ("release", "wfoly_plr_sm_apapa_reload_empty_04"), ("bolt", "wfoly_plr_sm_apapa_reload_empty_04"), ("charge", "wfoly_plr_sm_apapa_reload_empty_04"), ("lift", "wfoly_plr_sm_apapa_reload_01"), ("grab", "wfoly_plr_sm_apapa_reload_01"), ("hand", "wfoly_plr_sm_apapa_reload_01"), ("raise", "wfoly_plr_sm_apapa_raise_quick_01"), ("pullout", "wfoly_plr_sm_apapa_raise_quick_01"), ("drop", "wfoly_plr_sm_apapa_drop_quick_01"), ("putaway", "wfoly_plr_sm_apapa_drop_quick_01")],
        layers: &[("weap_apapa_fire_plr_lfe", "weap_apapa_fire_plr_atmo"), ("weap_apapa_fire_npc_mech", "weap_apapa_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_lm_rkilo",
        fire: "weap_rkilo_fire_npc",
        fire_player: "weap_rkilo_fire_plr_shot",
        fire_last_player: "weap_rkilo_fire_plr_last_mech",
        raise_player: "wfoly_plr_lm_rkilo_raise_quick_01",
        first_raise_player: "wfoly_plr_lm_rkilo_raise_first_01",
        putaway_player: "wfoly_plr_lm_rkilo_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_lm_rkilo_reload_02"), ("mag_out", "wfoly_plr_lm_rkilo_reload_02"), ("clipout", "wfoly_plr_lm_rkilo_reload_02"), ("clip_out", "wfoly_plr_lm_rkilo_reload_02"), ("magin", "wfoly_plr_lm_rkilo_reload_03"), ("mag_in", "wfoly_plr_lm_rkilo_reload_03"), ("clipin", "wfoly_plr_lm_rkilo_reload_03"), ("clip_in", "wfoly_plr_lm_rkilo_reload_03"), ("slide", "wfoly_plr_lm_rkilo_reload_empty_04"), ("chamber", "wfoly_plr_lm_rkilo_reload_empty_04"), ("release", "wfoly_plr_lm_rkilo_reload_empty_04"), ("bolt", "wfoly_plr_lm_rkilo_reload_empty_04"), ("charge", "wfoly_plr_lm_rkilo_reload_empty_04"), ("lift", "wfoly_plr_lm_rkilo_reload_01"), ("grab", "wfoly_plr_lm_rkilo_reload_01"), ("hand", "wfoly_plr_lm_rkilo_reload_01"), ("raise", "wfoly_plr_lm_rkilo_raise_quick_01"), ("pullout", "wfoly_plr_lm_rkilo_raise_quick_01"), ("drop", "wfoly_plr_lm_rkilo_drop_quick_01"), ("putaway", "wfoly_plr_lm_rkilo_drop_quick_01")],
        layers: &[("weap_rkilo_fire_plr_lfe", "weap_rkilo_fire_plr_atmo"), ("weap_rkilo_fire_npc_mech", "weap_rkilo_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_lm_kilo21",
        fire: "weap_kilo21_fire_npc",
        fire_player: "weap_kilo21_fire_plr_shot",
        fire_last_player: "weap_kilo21_fire_plr_last_mech",
        raise_player: "wfoly_plr_lm_kilo21_raise_quick_01",
        first_raise_player: "wfoly_plr_lm_kilo21_raise_first_01",
        putaway_player: "wfoly_plr_lm_kilo21_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_lm_kilo21_reload_02"), ("mag_out", "wfoly_plr_lm_kilo21_reload_02"), ("clipout", "wfoly_plr_lm_kilo21_reload_02"), ("clip_out", "wfoly_plr_lm_kilo21_reload_02"), ("magin", "wfoly_plr_lm_kilo21_reload_03"), ("mag_in", "wfoly_plr_lm_kilo21_reload_03"), ("clipin", "wfoly_plr_lm_kilo21_reload_03"), ("clip_in", "wfoly_plr_lm_kilo21_reload_03"), ("slide", "wfoly_plr_lm_kilo21_reload_empty_04"), ("chamber", "wfoly_plr_lm_kilo21_reload_empty_04"), ("release", "wfoly_plr_lm_kilo21_reload_empty_04"), ("bolt", "wfoly_plr_lm_kilo21_reload_empty_04"), ("charge", "wfoly_plr_lm_kilo21_reload_empty_04"), ("lift", "wfoly_plr_lm_kilo21_reload_01"), ("grab", "wfoly_plr_lm_kilo21_reload_01"), ("hand", "wfoly_plr_lm_kilo21_reload_01"), ("raise", "wfoly_plr_lm_kilo21_raise_quick_01"), ("pullout", "wfoly_plr_lm_kilo21_raise_quick_01"), ("drop", "wfoly_plr_lm_kilo21_drop_quick_01"), ("putaway", "wfoly_plr_lm_kilo21_drop_quick_01")],
        layers: &[("weap_kilo21_fire_plr_lfe", "weap_kilo21_fire_plr_atmo"), ("weap_kilo21_fire_npc_mech", "weap_kilo21_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_lm_ngolf7",
        fire: "weap_ngolf7_fire_npc",
        fire_player: "weap_ngolf7_fire_plr_shot",
        fire_last_player: "weap_ngolf7_fire_plr_last_mech",
        raise_player: "wfoly_plr_lm_ngolf7_raise_quick_01",
        first_raise_player: "wfoly_plr_lm_ngolf7_raise_first_01",
        putaway_player: "wfoly_plr_lm_ngolf7_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_lm_ngolf7_reload_02"), ("mag_out", "wfoly_plr_lm_ngolf7_reload_02"), ("clipout", "wfoly_plr_lm_ngolf7_reload_02"), ("clip_out", "wfoly_plr_lm_ngolf7_reload_02"), ("magin", "wfoly_plr_lm_ngolf7_reload_03"), ("mag_in", "wfoly_plr_lm_ngolf7_reload_03"), ("clipin", "wfoly_plr_lm_ngolf7_reload_03"), ("clip_in", "wfoly_plr_lm_ngolf7_reload_03"), ("slide", "wfoly_plr_lm_ngolf7_reload_empty_04"), ("chamber", "wfoly_plr_lm_ngolf7_reload_empty_04"), ("release", "wfoly_plr_lm_ngolf7_reload_empty_04"), ("bolt", "wfoly_plr_lm_ngolf7_reload_empty_04"), ("charge", "wfoly_plr_lm_ngolf7_reload_empty_04"), ("lift", "wfoly_plr_lm_ngolf7_reload_01"), ("grab", "wfoly_plr_lm_ngolf7_reload_01"), ("hand", "wfoly_plr_lm_ngolf7_reload_01"), ("raise", "wfoly_plr_lm_ngolf7_raise_quick_01"), ("pullout", "wfoly_plr_lm_ngolf7_raise_quick_01"), ("drop", "wfoly_plr_lm_ngolf7_drop_quick_01"), ("putaway", "wfoly_plr_lm_ngolf7_drop_quick_01")],
        layers: &[("weap_ngolf7_fire_plr_lfe", "weap_ngolf7_fire_plr_atmo"), ("weap_ngolf7_fire_npc_mech", "weap_ngolf7_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sh_vecho",
        fire: "weap_vecho_fire_npc",
        fire_player: "weap_vecho_fire_plr_shot",
        fire_last_player: "weap_vecho_fire_plr_last_mech",
        raise_player: "wfoly_plr_sh_vecho_raise_01",
        first_raise_player: "wfoly_plr_sh_vecho_raise_first_01",
        putaway_player: "wfoly_plr_sh_vecho_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sh_vecho_reload_02"), ("mag_out", "wfoly_plr_sh_vecho_reload_02"), ("clipout", "wfoly_plr_sh_vecho_reload_02"), ("clip_out", "wfoly_plr_sh_vecho_reload_02"), ("magin", "wfoly_plr_sh_vecho_reload_03"), ("mag_in", "wfoly_plr_sh_vecho_reload_03"), ("clipin", "wfoly_plr_sh_vecho_reload_03"), ("clip_in", "wfoly_plr_sh_vecho_reload_03"), ("slide", "wfoly_plr_sh_vecho_reload_empty_04"), ("chamber", "wfoly_plr_sh_vecho_reload_empty_04"), ("release", "wfoly_plr_sh_vecho_reload_empty_04"), ("bolt", "wfoly_plr_sh_vecho_reload_empty_04"), ("charge", "wfoly_plr_sh_vecho_reload_empty_04"), ("lift", "wfoly_plr_sh_vecho_reload_01"), ("grab", "wfoly_plr_sh_vecho_reload_01"), ("hand", "wfoly_plr_sh_vecho_reload_01"), ("raise", "wfoly_plr_sh_vecho_raise_01"), ("pullout", "wfoly_plr_sh_vecho_raise_01"), ("drop", "wfoly_plr_sh_vecho_drop_quick_01"), ("putaway", "wfoly_plr_sh_vecho_drop_quick_01")],
        layers: &[("weap_vecho_fire_plr_lfe", "weap_vecho_fire_plr_atmo"), ("weap_vecho_fire_npc_mech", "weap_vecho_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sh_mike1014",
        fire: "weap_mike1014_fire_npc",
        fire_player: "weap_mike1014_fire_plr_shot",
        fire_last_player: "weap_mike1014_fire_plr_last_mech",
        raise_player: "wfoly_plr_sh_mike1014_raise_quick_01",
        first_raise_player: "wfoly_plr_sh_mike1014_raise_first_01",
        putaway_player: "wfoly_plr_sh_mike1014_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("raise", "wfoly_plr_sh_mike1014_raise_quick_01"), ("pullout", "wfoly_plr_sh_mike1014_raise_quick_01"), ("drop", "wfoly_plr_sh_mike1014_drop_quick_01"), ("putaway", "wfoly_plr_sh_mike1014_drop_quick_01")],
        layers: &[("weap_mike1014_fire_plr_lfe", "weap_mike1014_fire_plr_atmo"), ("weap_mike1014_fire_npc_mech", "weap_mike1014_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sh_charlie725",
        fire: "weap_charlie725_fire_npc",
        fire_player: "weap_charlie725_fire_plr_shot",
        fire_last_player: "weap_charlie725_fire_plr_last_mech",
        raise_player: "wfoly_plr_sh_charlie725_raise_quick_01",
        first_raise_player: "wfoly_plr_sh_charlie725_raise_first_01",
        putaway_player: "wfoly_plr_sh_charlie725_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sh_charlie725_reload_02"), ("mag_out", "wfoly_plr_sh_charlie725_reload_02"), ("clipout", "wfoly_plr_sh_charlie725_reload_02"), ("clip_out", "wfoly_plr_sh_charlie725_reload_02"), ("magin", "wfoly_plr_sh_charlie725_reload_03"), ("mag_in", "wfoly_plr_sh_charlie725_reload_03"), ("clipin", "wfoly_plr_sh_charlie725_reload_03"), ("clip_in", "wfoly_plr_sh_charlie725_reload_03"), ("slide", "wfoly_plr_sh_charlie725_reload_empty_04"), ("chamber", "wfoly_plr_sh_charlie725_reload_empty_04"), ("release", "wfoly_plr_sh_charlie725_reload_empty_04"), ("bolt", "wfoly_plr_sh_charlie725_reload_empty_04"), ("charge", "wfoly_plr_sh_charlie725_reload_empty_04"), ("lift", "wfoly_plr_sh_charlie725_reload_01"), ("grab", "wfoly_plr_sh_charlie725_reload_01"), ("hand", "wfoly_plr_sh_charlie725_reload_01"), ("raise", "wfoly_plr_sh_charlie725_raise_quick_01"), ("pullout", "wfoly_plr_sh_charlie725_raise_quick_01"), ("drop", "wfoly_plr_sh_charlie725_drop_quick_01"), ("putaway", "wfoly_plr_sh_charlie725_drop_quick_01")],
        layers: &[("weap_charlie725_fire_plr_lfe", "weap_charlie725_fire_plr_atmo"), ("weap_charlie725_fire_npc_mech", "weap_charlie725_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sh_tsierra12",
        fire: "weap_tsierra12_fire_npc",
        fire_player: "weap_tsierra12_fire_plr_shot",
        fire_last_player: "weap_tsierra12_fire_plr_last_mech",
        raise_player: "wfoly_plr_sh_tsierra12_raise_quick_01",
        first_raise_player: "wfoly_plr_sh_tsierra12_raise_first_01",
        putaway_player: "wfoly_plr_sh_tsierra12_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sh_tsierra12_reload_02"), ("mag_out", "wfoly_plr_sh_tsierra12_reload_02"), ("clipout", "wfoly_plr_sh_tsierra12_reload_02"), ("clip_out", "wfoly_plr_sh_tsierra12_reload_02"), ("magin", "wfoly_plr_sh_tsierra12_reload_03"), ("mag_in", "wfoly_plr_sh_tsierra12_reload_03"), ("clipin", "wfoly_plr_sh_tsierra12_reload_03"), ("clip_in", "wfoly_plr_sh_tsierra12_reload_03"), ("slide", "wfoly_plr_sh_tsierra12_reload_empty_04"), ("chamber", "wfoly_plr_sh_tsierra12_reload_empty_04"), ("release", "wfoly_plr_sh_tsierra12_reload_empty_04"), ("bolt", "wfoly_plr_sh_tsierra12_reload_empty_04"), ("charge", "wfoly_plr_sh_tsierra12_reload_empty_04"), ("lift", "wfoly_plr_sh_tsierra12_reload_01"), ("grab", "wfoly_plr_sh_tsierra12_reload_01"), ("hand", "wfoly_plr_sh_tsierra12_reload_01"), ("raise", "wfoly_plr_sh_tsierra12_raise_quick_01"), ("pullout", "wfoly_plr_sh_tsierra12_raise_quick_01"), ("drop", "wfoly_plr_sh_tsierra12_drop_quick_01"), ("putaway", "wfoly_plr_sh_tsierra12_drop_quick_01")],
        layers: &[("weap_tsierra12_fire_plr_lfe", "weap_tsierra12_fire_plr_atmo"), ("weap_tsierra12_fire_npc_mech", "weap_tsierra12_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sn_alpha50",
        fire: "weap_alpha50_fire_npc",
        fire_player: "weap_alpha50_fire_plr_shot",
        fire_last_player: "weap_alpha50_fire_plr_last_mech",
        raise_player: "wfoly_plr_sn_alpha50_raise_quick_01",
        first_raise_player: "wfoly_plr_sn_alpha50_raise_first_01",
        putaway_player: "wfoly_plr_sn_alpha50_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sn_alpha50_reload_02"), ("mag_out", "wfoly_plr_sn_alpha50_reload_02"), ("clipout", "wfoly_plr_sn_alpha50_reload_02"), ("clip_out", "wfoly_plr_sn_alpha50_reload_02"), ("magin", "wfoly_plr_sn_alpha50_reload_03"), ("mag_in", "wfoly_plr_sn_alpha50_reload_03"), ("clipin", "wfoly_plr_sn_alpha50_reload_03"), ("clip_in", "wfoly_plr_sn_alpha50_reload_03"), ("slide", "wfoly_plr_sn_alpha50_reload_empty_04"), ("chamber", "wfoly_plr_sn_alpha50_reload_empty_04"), ("release", "wfoly_plr_sn_alpha50_reload_empty_04"), ("bolt", "wfoly_plr_sn_alpha50_reload_empty_04"), ("charge", "wfoly_plr_sn_alpha50_reload_empty_04"), ("lift", "wfoly_plr_sn_alpha50_reload_01"), ("grab", "wfoly_plr_sn_alpha50_reload_01"), ("hand", "wfoly_plr_sn_alpha50_reload_01"), ("raise", "wfoly_plr_sn_alpha50_raise_quick_01"), ("pullout", "wfoly_plr_sn_alpha50_raise_quick_01"), ("drop", "wfoly_plr_sn_alpha50_drop_quick_01"), ("putaway", "wfoly_plr_sn_alpha50_drop_quick_01")],
        layers: &[("weap_alpha50_fire_plr_lfe", "weap_alpha50_fire_plr_atmo"), ("weap_alpha50_fire_npc_mech", "weap_alpha50_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sn_mromeo",
        fire: "weap_mromeo_fire_npc",
        fire_player: "weap_mromeo_fire_plr_shot",
        fire_last_player: "weap_mromeo_fire_plr_last_mech",
        raise_player: "wfoly_plr_sn_mromeo_raise_quick_01",
        first_raise_player: "wfoly_plr_sn_mromeo_raise_first_01",
        putaway_player: "wfoly_plr_sn_mromeo_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sn_mromeo_reload_02"), ("mag_out", "wfoly_plr_sn_mromeo_reload_02"), ("clipout", "wfoly_plr_sn_mromeo_reload_02"), ("clip_out", "wfoly_plr_sn_mromeo_reload_02"), ("magin", "wfoly_plr_sn_mromeo_reload_03"), ("mag_in", "wfoly_plr_sn_mromeo_reload_03"), ("clipin", "wfoly_plr_sn_mromeo_reload_03"), ("clip_in", "wfoly_plr_sn_mromeo_reload_03"), ("slide", "wfoly_plr_sn_mromeo_reload_empty_04"), ("chamber", "wfoly_plr_sn_mromeo_reload_empty_04"), ("release", "wfoly_plr_sn_mromeo_reload_empty_04"), ("bolt", "wfoly_plr_sn_mromeo_reload_empty_04"), ("charge", "wfoly_plr_sn_mromeo_reload_empty_04"), ("lift", "wfoly_plr_sn_mromeo_reload_01"), ("grab", "wfoly_plr_sn_mromeo_reload_01"), ("hand", "wfoly_plr_sn_mromeo_reload_01"), ("raise", "wfoly_plr_sn_mromeo_raise_quick_01"), ("pullout", "wfoly_plr_sn_mromeo_raise_quick_01"), ("drop", "wfoly_plr_sn_mromeo_drop_quick_01"), ("putaway", "wfoly_plr_sn_mromeo_drop_quick_01")],
        layers: &[("weap_mromeo_fire_plr_lfe", "weap_mromeo_fire_plr_atmo"), ("weap_mromeo_fire_npc_mech", "weap_mromeo_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_sn_walpha",
        fire: "weap_walpha_fire_npc",
        fire_player: "weap_walpha_fire_plr_shot",
        fire_last_player: "weap_walpha_fire_plr_last_mech",
        raise_player: "wfoly_plr_sn_walpha_raise",
        first_raise_player: "wfoly_plr_sn_walpha_raise_first_01",
        putaway_player: "wfoly_plr_sn_walpha_drop",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_sn_walpha_reload_02"), ("mag_out", "wfoly_plr_sn_walpha_reload_02"), ("clipout", "wfoly_plr_sn_walpha_reload_02"), ("clip_out", "wfoly_plr_sn_walpha_reload_02"), ("magin", "wfoly_plr_sn_walpha_reload_03"), ("mag_in", "wfoly_plr_sn_walpha_reload_03"), ("clipin", "wfoly_plr_sn_walpha_reload_03"), ("clip_in", "wfoly_plr_sn_walpha_reload_03"), ("slide", "wfoly_plr_sn_walpha_reload_empty_04"), ("chamber", "wfoly_plr_sn_walpha_reload_empty_04"), ("release", "wfoly_plr_sn_walpha_reload_empty_04"), ("bolt", "wfoly_plr_sn_walpha_reload_empty_04"), ("charge", "wfoly_plr_sn_walpha_reload_empty_04"), ("lift", "wfoly_plr_sn_walpha_reload_01"), ("grab", "wfoly_plr_sn_walpha_reload_01"), ("hand", "wfoly_plr_sn_walpha_reload_01"), ("raise", "wfoly_plr_sn_walpha_raise"), ("pullout", "wfoly_plr_sn_walpha_raise"), ("drop", "wfoly_plr_sn_walpha_drop"), ("putaway", "wfoly_plr_sn_walpha_drop")],
        layers: &[("weap_walpha_fire_plr_lfe", "weap_walpha_fire_plr_atmo"), ("weap_walpha_fire_npc_mech", "weap_walpha_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_golf17",
        fire: "weap_golf17_fire_npc",
        fire_player: "",
        fire_last_player: "",
        raise_player: "wfoly_plr_pi_golf17_raise_01",
        first_raise_player: "wfoly_plr_pi_golf17_raise_first_01",
        putaway_player: "",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_golf17_reload_02"), ("mag_out", "wfoly_plr_pi_golf17_reload_02"), ("clipout", "wfoly_plr_pi_golf17_reload_02"), ("clip_out", "wfoly_plr_pi_golf17_reload_02"), ("magin", "wfoly_plr_pi_golf17_reload_03"), ("mag_in", "wfoly_plr_pi_golf17_reload_03"), ("clipin", "wfoly_plr_pi_golf17_reload_03"), ("clip_in", "wfoly_plr_pi_golf17_reload_03"), ("slide", "wfoly_plr_pi_golf17_reload_empty_04"), ("chamber", "wfoly_plr_pi_golf17_reload_empty_04"), ("release", "wfoly_plr_pi_golf17_reload_empty_04"), ("bolt", "wfoly_plr_pi_golf17_reload_empty_04"), ("charge", "wfoly_plr_pi_golf17_reload_empty_04"), ("lift", "wfoly_plr_pi_golf17_reload_01"), ("grab", "wfoly_plr_pi_golf17_reload_01"), ("hand", "wfoly_plr_pi_golf17_reload_01"), ("raise", "wfoly_plr_pi_golf17_raise_01"), ("pullout", "wfoly_plr_pi_golf17_raise_01")],
        layers: &[("weap_golf17_fire_npc_mech", "weap_golf17_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_decho",
        fire: "weap_decho_fire_npc",
        fire_player: "weap_decho_fire_plr_shot",
        fire_last_player: "weap_decho_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_decho_raise_01",
        first_raise_player: "wfoly_plr_pi_decho_raise_first_01",
        putaway_player: "wfoly_plr_pi_decho_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_decho_reload_02"), ("mag_out", "wfoly_plr_pi_decho_reload_02"), ("clipout", "wfoly_plr_pi_decho_reload_02"), ("clip_out", "wfoly_plr_pi_decho_reload_02"), ("magin", "wfoly_plr_pi_decho_reload_03"), ("mag_in", "wfoly_plr_pi_decho_reload_03"), ("clipin", "wfoly_plr_pi_decho_reload_03"), ("clip_in", "wfoly_plr_pi_decho_reload_03"), ("slide", "wfoly_plr_pi_decho_reload_empty_04"), ("chamber", "wfoly_plr_pi_decho_reload_empty_04"), ("release", "wfoly_plr_pi_decho_reload_empty_04"), ("bolt", "wfoly_plr_pi_decho_reload_empty_04"), ("charge", "wfoly_plr_pi_decho_reload_empty_04"), ("lift", "wfoly_plr_pi_decho_reload_01"), ("grab", "wfoly_plr_pi_decho_reload_01"), ("hand", "wfoly_plr_pi_decho_reload_01"), ("raise", "wfoly_plr_pi_decho_raise_01"), ("pullout", "wfoly_plr_pi_decho_raise_01"), ("drop", "wfoly_plr_pi_decho_drop_quick_01"), ("putaway", "wfoly_plr_pi_decho_drop_quick_01")],
        layers: &[("weap_decho_fire_plr_lfe", "weap_decho_fire_plr_atmo"), ("weap_decho_fire_npc_mech", "weap_decho_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_swhiskey",
        fire: "weap_swhiskey_fire_npc",
        fire_player: "weap_swhiskey_fire_plr_shot",
        fire_last_player: "weap_swhiskey_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_swhiskey_raise_quick_01",
        first_raise_player: "wfoly_plr_pi_swhiskey_raise_first_01",
        putaway_player: "wfoly_plr_pi_swhiskey_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("raise", "wfoly_plr_pi_swhiskey_raise_quick_01"), ("pullout", "wfoly_plr_pi_swhiskey_raise_quick_01"), ("drop", "wfoly_plr_pi_swhiskey_drop_quick_01"), ("putaway", "wfoly_plr_pi_swhiskey_drop_quick_01")],
        layers: &[("weap_swhiskey_fire_plr_lfe", "weap_swhiskey_fire_plr_atmo"), ("weap_swhiskey_fire_npc_mech", "weap_swhiskey_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_papa220",
        fire: "weap_papa220_fire_npc",
        fire_player: "weap_papa220_fire_plr_shot",
        fire_last_player: "weap_papa220_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_papa220_raise_quick_01",
        first_raise_player: "wfoly_plr_pi_papa220_raise_first_01",
        putaway_player: "wfoly_plr_pi_papa220_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_papa220_reload_02"), ("mag_out", "wfoly_plr_pi_papa220_reload_02"), ("clipout", "wfoly_plr_pi_papa220_reload_02"), ("clip_out", "wfoly_plr_pi_papa220_reload_02"), ("magin", "wfoly_plr_pi_papa220_reload_03"), ("mag_in", "wfoly_plr_pi_papa220_reload_03"), ("clipin", "wfoly_plr_pi_papa220_reload_03"), ("clip_in", "wfoly_plr_pi_papa220_reload_03"), ("slide", "wfoly_plr_pi_papa220_reload_empty_04"), ("chamber", "wfoly_plr_pi_papa220_reload_empty_04"), ("release", "wfoly_plr_pi_papa220_reload_empty_04"), ("bolt", "wfoly_plr_pi_papa220_reload_empty_04"), ("charge", "wfoly_plr_pi_papa220_reload_empty_04"), ("lift", "wfoly_plr_pi_papa220_reload_01"), ("grab", "wfoly_plr_pi_papa220_reload_01"), ("hand", "wfoly_plr_pi_papa220_reload_01"), ("raise", "wfoly_plr_pi_papa220_raise_quick_01"), ("pullout", "wfoly_plr_pi_papa220_raise_quick_01"), ("drop", "wfoly_plr_pi_papa220_drop_quick_01"), ("putaway", "wfoly_plr_pi_papa220_drop_quick_01")],
        layers: &[("weap_papa220_fire_plr_lfe", "weap_papa220_fire_plr_atmo"), ("weap_papa220_fire_npc_mech", "weap_papa220_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_mike2011",
        fire: "weap_mike2011_fire_npc",
        fire_player: "weap_mike2011_fire_plr_shot",
        fire_last_player: "weap_mike2011_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_mike2011_raise_quick_01",
        first_raise_player: "wfoly_plr_pi_mike2011_raise_first_01",
        putaway_player: "wfoly_plr_pi_mike2011_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_mike2011_reload_02"), ("mag_out", "wfoly_plr_pi_mike2011_reload_02"), ("clipout", "wfoly_plr_pi_mike2011_reload_02"), ("clip_out", "wfoly_plr_pi_mike2011_reload_02"), ("magin", "wfoly_plr_pi_mike2011_reload_03"), ("mag_in", "wfoly_plr_pi_mike2011_reload_03"), ("clipin", "wfoly_plr_pi_mike2011_reload_03"), ("clip_in", "wfoly_plr_pi_mike2011_reload_03"), ("slide", "wfoly_plr_pi_mike2011_reload_empty_04"), ("chamber", "wfoly_plr_pi_mike2011_reload_empty_04"), ("release", "wfoly_plr_pi_mike2011_reload_empty_04"), ("bolt", "wfoly_plr_pi_mike2011_reload_empty_04"), ("charge", "wfoly_plr_pi_mike2011_reload_empty_04"), ("lift", "wfoly_plr_pi_mike2011_reload_01"), ("grab", "wfoly_plr_pi_mike2011_reload_01"), ("hand", "wfoly_plr_pi_mike2011_reload_01"), ("raise", "wfoly_plr_pi_mike2011_raise_quick_01"), ("pullout", "wfoly_plr_pi_mike2011_raise_quick_01"), ("drop", "wfoly_plr_pi_mike2011_drop_quick_01"), ("putaway", "wfoly_plr_pi_mike2011_drop_quick_01")],
        layers: &[("weap_mike2011_fire_plr_lfe", "weap_mike2011_fire_plr_atmo"), ("weap_mike2011_fire_npc_mech", "weap_mike2011_fire_npc_atmo")],
    },
    JupGunSounds {
        gun: "iw9_pi_tango9",
        fire: "weap_tango9_fire_npc",
        fire_player: "weap_tango9_fire_plr_shot",
        fire_last_player: "weap_tango9_fire_plr_last_mech",
        raise_player: "wfoly_plr_pi_tango9_raise_quick_01",
        first_raise_player: "wfoly_plr_pi_tango9_raise_first_01",
        putaway_player: "wfoly_plr_pi_tango9_drop_quick_01",
        pickup: "",
        pickup_player: "",
        ads_up: "",
        ads_down: "",
        notes: &[("magout", "wfoly_plr_pi_tango9_reload_02"), ("mag_out", "wfoly_plr_pi_tango9_reload_02"), ("clipout", "wfoly_plr_pi_tango9_reload_02"), ("clip_out", "wfoly_plr_pi_tango9_reload_02"), ("magin", "wfoly_plr_pi_tango9_reload_03"), ("mag_in", "wfoly_plr_pi_tango9_reload_03"), ("clipin", "wfoly_plr_pi_tango9_reload_03"), ("clip_in", "wfoly_plr_pi_tango9_reload_03"), ("slide", "wfoly_plr_pi_tango9_reload_empty_04"), ("chamber", "wfoly_plr_pi_tango9_reload_empty_04"), ("release", "wfoly_plr_pi_tango9_reload_empty_04"), ("bolt", "wfoly_plr_pi_tango9_reload_empty_04"), ("charge", "wfoly_plr_pi_tango9_reload_empty_04"), ("lift", "wfoly_plr_pi_tango9_reload_01"), ("grab", "wfoly_plr_pi_tango9_reload_01"), ("hand", "wfoly_plr_pi_tango9_reload_01"), ("raise", "wfoly_plr_pi_tango9_raise_quick_01"), ("pullout", "wfoly_plr_pi_tango9_raise_quick_01"), ("drop", "wfoly_plr_pi_tango9_drop_quick_01"), ("putaway", "wfoly_plr_pi_tango9_drop_quick_01")],
        layers: &[("weap_tango9_fire_plr_lfe", "weap_tango9_fire_plr_atmo"), ("weap_tango9_fire_npc_mech", "weap_tango9_fire_npc_atmo")],
    },
];

/// Whether the install's gun sounds hold this alias with a clip to play.
pub fn jup_gun_has(alias: &str) -> bool {
    let Some(install) = jup_gun_source() else {
        return false;
    };
    jup_gun_sounds(&install)
        .is_ok_and(|c| c.index_in(AssetNamespace::Jup, &jup_alias_key(alias)).is_some())
}

/// The gun sounds of the install (`JUP_GUN_ZONE`, only the aliases
/// [`JUP_GUN_SOUNDS`] name and the layers chained from them, each layer the
/// secondary of the one before), read once per process.
pub fn jup_gun_sounds(install: &Path) -> Result<Arc<SoundCatalog>, String> {
    type GunCache = Mutex<HashMap<PathBuf, Arc<SoundCatalog>>>;
    static CACHE: OnceLock<GunCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(install).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_jup(install, JUP_GUN_ZONES, Some(gun_keep()))?);
    if let Ok(mut c) = cache.lock() {
        c.insert(install.to_path_buf(), catalog.clone());
    }
    Ok(catalog)
}

/// The guns' aliases kept, and the layers chained from them.
fn gun_keep() -> Keep {
    let hash = |a: &str| u64::from_str_radix(&jup_alias_key(a)[1..], 16).ok();
    let keep: HashSet<u64> = JUP_GUN_SOUNDS
        .iter()
        .flat_map(JupGunSounds::aliases)
        .filter_map(hash)
        .collect();
    let extra: HashMap<u64, u64> = JUP_GUN_SOUNDS
        .iter()
        .flat_map(|g| g.layers.iter())
        .filter_map(|(a, b)| Some((hash(a)?, hash(b)?)))
        .collect();
    (keep, extra)
}

/// bo2mp: the package entries (keys) of every gun sound's streams, for a
/// small copy of the install (`fastfile_jup::pack`): a sound is decoded
/// only when played, so the copy is told which entries it needs.
pub fn jup_gun_sound_keys(install: &Path) -> Result<Vec<u64>, String> {
    let keep = gun_keep();
    let rows = read_rows(install, JUP_GUN_ZONES, Some(&keep))?;
    Ok(rows
        .aliases
        .iter()
        .flat_map(|a| a.clips.iter())
        .filter_map(|(_, row)| u64::from_str_radix(row.split(':').next()?, 16).ok())
        .collect())
}

/// The sounds of some zones of the install, every alias under `#<hash>`.
pub fn load_sound_catalog_jup(install: &Path, zones: &[&str]) -> Result<SoundCatalog, String> {
    load_jup(install, zones, None)
}

/// Aliases kept, and layers added to the zone's chains.
type Keep = (HashSet<u64>, HashMap<u64, u64>);

/// One alias's clips as the catalog keeps them: its hash, its next layer
/// (a secondary alias), and per clip its package (index into
/// [`JupRows::packages`]) and row text (see the module docs).
struct AliasRows {
    hash: u64,
    secondary: Option<u64>,
    clips: Vec<(u32, String)>,
}

/// What some zones' sounds come to: every alias with the clips the install
/// has, and how many had none.
struct JupRows {
    packages: Vec<String>,
    aliases: Vec<AliasRows>,
    missing: usize,
    streams: usize,
}

/// The sounds of some zones; with `keep`, only those aliases and the
/// layers chained from them (each the secondary of the one before).
///
/// Reading the zones (Resurgence's are 400 MB inflated) and the packages'
/// indexes takes many seconds, and comes to the same rows each time for
/// the same install: the rows are kept in the game's cache
/// (`codlegacy-artifacts/cache/jup_sound`), found by the zones' and packages'
/// sizes and times and this code's own text, and read from there after.
fn load_jup(install: &Path, zones: &[&str], keep: Option<Keep>) -> Result<SoundCatalog, String> {
    let zone_dir = install.join("zone");
    let started = std::time::Instant::now();
    // (An exported folder holds the guns' rows as they were read.)
    let exported = if keep.is_some() { export::rows(install) } else { None };
    let key = rows_key(&zone_dir, zones, keep.is_some()).filter(|_| exported.is_none());
    let cached = key.as_deref().and_then(|k| {
        let bytes = asset_transport::cache_get(ROWS_KIND, k)?;
        let rows = decode_rows(&bytes);
        if rows.is_none() {
            diag::warn!(Zone, "sound bank: MW3 (2023) cache {k} unreadable; reading the zones");
        }
        rows
    });
    let from_cache = cached.is_some() || exported.is_some();
    let rows = match exported.or(cached) {
        Some(rows) => rows,
        None => {
            let rows = read_rows(install, zones, keep.as_ref())?;
            if let Some(k) = &key
                && let Err(e) = asset_transport::cache_put(ROWS_KIND, k, &encode_rows(&rows))
            {
                diag::warn!(Zone, "sound bank: MW3 (2023) cache {k} not written: {e}");
            }
            rows
        }
    };
    let owner = ZoneOwner::from_zone_path(&zone_dir.join(format!(
        "{}.ff",
        zones.first().copied().unwrap_or("ingame_br")
    )));
    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    let mut playable = 0usize;
    let mut have = Vec::with_capacity(rows.aliases.len());
    for a in &rows.aliases {
        let name = format!("#{:016x}", a.hash);
        let secondary = a.secondary.map(|n| format!("#{n:016x}"));
        let clips: Vec<CapturedAlias> = a
            .clips
            .iter()
            .filter_map(|(package, clip)| {
                Some(CapturedAlias {
                    alias_name: name.clone(),
                    secondary: secondary.clone(),
                    file_type: Some(2),
                    file_exists: Some(1),
                    streamed: Some((rows.packages.get(*package as usize)?.clone(), clip.clone())),
                    vol_min: 1.0,
                    vol_max: 1.0,
                    pitch_min: 1.0,
                    pitch_max: 1.0,
                    dist_min: 0.0,
                    dist_max: 100_000.0,
                    probability: 1.0,
                    ..CapturedAlias::default()
                })
            })
            .collect();
        if clips.is_empty() {
            continue;
        }
        playable += clips.len();
        have.push(a.hash);
        catalog.ingest_sound(CapturedSound {
            name,
            aliases: clips,
            game: ZoneGame::T6,
            zone: owner,
            namespace: Some(AssetNamespace::Jup),
        });
    }
    catalog.finalize();
    // (What Resurgence's script may play: the guns' sounds are not asked
    // for, and `jup_has` reads Resurgence's zones while this is empty.)
    if keep.is_none()
        && let Ok(mut s) = available().lock()
    {
        s.extend(have);
    }
    diag::info!(
        Zone,
        "sound bank: MW3 (2023) {}: {} aliases, {playable} clips, {} without a stream ({} streams found in the packages){} in {:.1}s",
        install.display(),
        catalog.sounds.len(),
        rows.missing,
        rows.streams,
        if from_cache { ", from the cache," } else { "" },
        started.elapsed().as_secs_f32()
    );
    Ok(catalog)
}

/// The zones' aliases and their clips' rows, read from the zones and the
/// packages' indexes.
fn read_rows(install: &Path, zones: &[&str], keep: Option<&Keep>) -> Result<JupRows, String> {
    let oodle = oodle_for(install)?;
    let zone_dir = install.join("zone");
    let mut bank = SoundBank::new();
    for z in zones {
        let path = zone_dir.join(format!("{z}.ff"));
        match fastfile_jup::read_zone(&path, oodle) {
            Ok((_, bytes)) => {
                let (sounds, links) = bank.add_zone(z, &bytes);
                diag::info!(
                    Zone,
                    "sound bank: MW3 (2023) {z}: {sounds} sounds, {links} alias links"
                );
            }
            Err(e) => diag::warn!(Zone, "sound bank: MW3 (2023) {}: {e}", path.display()),
        }
    }
    let streams = bank
        .index_packages(&zone_dir, LANG)
        .map_err(|e| format!("{}: {e}", zone_dir.display()))?;
    let mut missing = 0usize;
    // A layer's next: the zone's chain, else one added.
    let next_of = |h: u64| -> Option<u64> {
        let (_, extra) = keep?;
        bank.next
            .get(&h)
            .copied()
            .filter(|n| bank.aliases.contains_key(n))
            .or_else(|| extra.get(&h).copied())
    };
    let mut hashes: Vec<u64> = match keep {
        None => bank.aliases.keys().copied().collect(),
        Some((wanted, _)) => {
            // The wanted aliases and every layer chained from them.
            let mut set = HashSet::new();
            for &w in wanted {
                let mut at = Some(w);
                while let Some(h) = at.filter(|h| set.insert(*h)) {
                    at = next_of(h);
                }
            }
            set.into_iter()
                .filter(|h| bank.aliases.contains_key(h))
                .collect()
        }
    };
    hashes.sort_unstable();
    let mut packages: Vec<String> = Vec::new();
    let mut package_index: HashMap<String, u32> = HashMap::new();
    let mut aliases = Vec::with_capacity(hashes.len());
    for h in hashes {
        let mut clips = Vec::new();
        for &si in &bank.aliases[&h] {
            let s = &bank.sounds[si];
            let a = &s.asset;
            let Some((package, entry)) = bank.row_of(s) else {
                missing += 1;
                continue;
            };
            if a.format != FORMAT_OPUS {
                missing += 1;
                continue;
            }
            let package = package.to_string_lossy().into_owned();
            let pi = *package_index.entry(package.clone()).or_insert_with(|| {
                packages.push(package);
                (packages.len() - 1) as u32
            });
            clips.push((
                pi,
                format!(
                    "{:016x}:{}:{}:{}:{}:{}:{}:{}:{}",
                    entry.key,
                    entry.offset,
                    entry.size,
                    entry.decompressed,
                    u8::from(entry.compressed),
                    a.table_len,
                    a.rate,
                    a.channels,
                    a.frames
                ),
            ));
        }
        aliases.push(AliasRows { hash: h, secondary: next_of(h), clips });
    }
    Ok(JupRows { packages, aliases, missing, streams })
}

/// The cache kind of the rows.
const ROWS_KIND: &str = "jup_sound";
/// The rows' layout's number.
const ROWS_FORMAT: u32 = 1;

/// The cache key of some zones' rows: over the zones' and the packages'
/// names, sizes and times, whether only the guns' are kept, and the code
/// that reads them. `None` when the folder cannot be listed.
fn rows_key(zone_dir: &Path, zones: &[&str], kept: bool) -> Option<String> {
    const CODE: &[&str] = &[
        include_str!("sound_load_jup.rs"),
        include_str!("../../fastfile_jup/src/sound_bank.rs"),
        include_str!("../../fastfile_jup/src/sound.rs"),
        include_str!("../../fastfile_jup/src/xsub.rs"),
    ];
    let fnv = |mut h: u64, b: &[u8]| {
        for &x in b {
            h ^= u64::from(x);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    let stamp = |meta: &std::fs::Metadata| {
        let written = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        (meta.len(), written)
    };
    let mut h = fnv(0xcbf2_9ce4_8422_2325, &ROWS_FORMAT.to_le_bytes());
    h = fnv(h, &[u8::from(kept)]);
    for code in CODE {
        h = fnv(h, code.as_bytes());
    }
    for z in zones {
        let (len, written) = std::fs::metadata(zone_dir.join(format!("{z}.ff"))).map_or((0, 0), |m| stamp(&m));
        h = fnv(h, z.as_bytes());
        h = fnv(h, &len.to_le_bytes());
        h = fnv(h, &written.to_le_bytes());
    }
    let mut subs: Vec<(String, u64, u64)> = std::fs::read_dir(zone_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".xsub") {
                return None;
            }
            let (len, written) = stamp(&e.metadata().ok()?);
            Some((name, len, written))
        })
        .collect();
    subs.sort();
    for (name, len, written) in &subs {
        h = fnv(h, name.as_bytes());
        h = fnv(h, &len.to_le_bytes());
        h = fnv(h, &written.to_le_bytes());
    }
    Some(format!("{}-{h:016x}", if kept { "guns" } else { "zones" }))
}

fn encode_rows(rows: &JupRows) -> Vec<u8> {
    let mut out = Vec::new();
    let u64s = |out: &mut Vec<u8>, v: u64| out.extend_from_slice(&v.to_le_bytes());
    let text = |out: &mut Vec<u8>, s: &str| {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    };
    out.extend_from_slice(&ROWS_FORMAT.to_le_bytes());
    u64s(&mut out, rows.packages.len() as u64);
    for p in &rows.packages {
        text(&mut out, p);
    }
    u64s(&mut out, rows.aliases.len() as u64);
    for a in &rows.aliases {
        u64s(&mut out, a.hash);
        u64s(&mut out, a.secondary.unwrap_or(u64::MAX));
        u64s(&mut out, a.clips.len() as u64);
        for (p, clip) in &a.clips {
            out.extend_from_slice(&p.to_le_bytes());
            text(&mut out, clip);
        }
    }
    u64s(&mut out, rows.missing as u64);
    u64s(&mut out, rows.streams as u64);
    out
}

/// A cursor over the rows' bytes.
struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    /// A count (never more than the bytes left could hold).
    fn count(&mut self) -> Option<usize> {
        usize::try_from(self.u64()?).ok().filter(|&n| n <= self.b.len() - self.at)
    }
    fn text(&mut self) -> Option<String> {
        let n = self.count()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

fn decode_rows(b: &[u8]) -> Option<JupRows> {
    let mut r = Bytes { b, at: 0 };
    if r.u32()? != ROWS_FORMAT {
        return None;
    }
    let n = r.count()?;
    let packages = (0..n).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
    let n = r.count()?;
    let mut aliases = Vec::with_capacity(n);
    for _ in 0..n {
        let hash = r.u64()?;
        let secondary = Some(r.u64()?).filter(|&s| s != u64::MAX);
        let m = r.count()?;
        let mut clips = Vec::with_capacity(m);
        for _ in 0..m {
            let p = r.u32()?;
            if p as usize >= packages.len() {
                return None;
            }
            clips.push((p, r.text()?));
        }
        aliases.push(AliasRows { hash, secondary, clips });
    }
    let missing = usize::try_from(r.u64()?).ok()?;
    let streams = usize::try_from(r.u64()?).ok()?;
    (r.at == b.len()).then_some(JupRows { packages, aliases, missing, streams })
}

/// A clip (see the module docs): its package entry read, decompressed and
/// decoded. (A clip of an exported folder, `pack:<key>`, is read from the
/// folder's own file.)
pub fn read_jup_clip(package: &str, clip: &str) -> Result<Pcm, String> {
    if let Some(key) = clip.strip_prefix(export::CLIP_PREFIX) {
        return export::read_clip(Path::new(package), key);
    }
    let c = read_jup_stream(package, clip)?;
    decode_opus(packets(&c.stream, c.table as usize), c.rate, c.channels, c.frames).map_err(|e| e.to_string())
}

/// A clip's Opus stream as the package holds it.
struct ClipStream {
    stream: Vec<u8>,
    table: u32,
    rate: u32,
    channels: u8,
    frames: u32,
}

/// A clip's package entry read and decompressed.
fn read_jup_stream(package: &str, clip: &str) -> Result<ClipStream, String> {
    let bad = |why: &str| format!("clip `{clip}`: {why}");
    let mut it = clip.split(':');
    let key = it
        .next()
        .and_then(|k| u64::from_str_radix(k, 16).ok())
        .ok_or_else(|| bad("no key"))?;
    let f: Vec<u64> = it
        .map(str::parse::<u64>)
        .collect::<Result<_, _>>()
        .map_err(|e| bad(&e.to_string()))?;
    let [
        offset,
        size,
        decompressed,
        compressed,
        table,
        rate,
        channels,
        frames,
    ] = f[..]
    else {
        return Err(bad("9 fields wanted"));
    };
    let install = Path::new(package)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("{package}: not in an install's zone folder"))?;
    let oodle = oodle_for(install)?;
    let entry = XSubEntry {
        key,
        offset,
        size: u32::try_from(size).map_err(|_| bad("size"))?,
        decompressed: u32::try_from(decompressed).map_err(|_| bad("decoded size"))?,
        compressed: compressed != 0,
    };
    let mut file = std::fs::File::open(package).map_err(|e| format!("{package}: {e}"))?;
    let mut blob = vec![0u8; entry.size as usize];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut blob))
        .map_err(|e| format!("{package}: {e}"))?;
    fastfile_jup::pack::note_range(Path::new(package), entry.offset, entry.size, entry.key);
    let bytes = decode_entry(&entry, &blob, oodle).map_err(|e| format!("{package}: {e}"))?;
    let (rate, channels, frames) = (
        u32::try_from(rate).map_err(|_| bad("rate"))?,
        u8::try_from(channels).map_err(|_| bad("channels"))?,
        u32::try_from(frames).map_err(|_| bad("frames"))?,
    );
    // (A loaded sound's entry has a header before its seek table.)
    let table = u32::try_from(table).map_err(|_| bad("table"))?;
    let stream = loaded_stream(&bytes, table).to_vec();
    Ok(ClipStream { stream, table, rate, channels, frames })
}

// bo2mp: the guns' sounds exported to a folder and read back from it.
#[path = "sound_export_jup.rs"]
mod export;
pub use export::{JupSoundExport, export_jup_gun_sounds};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_read_back_as_written() {
        let rows = JupRows {
            packages: vec!["a.xsub".into(), "b.xsub".into()],
            aliases: vec![
                AliasRows { hash: 1, secondary: Some(2), clips: vec![(0, "00:1:2:3:1:4:48000:2:960".into()), (1, "x".into())] },
                AliasRows { hash: 2, secondary: None, clips: Vec::new() },
            ],
            missing: 3,
            streams: 4,
        };
        let bytes = encode_rows(&rows);
        let back = decode_rows(&bytes).unwrap();
        assert_eq!(encode_rows(&back), bytes);
        assert!(decode_rows(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn alias_keys() {
        assert_eq!(jup_alias_key("#00000000000000ff"), "#00000000000000ff");
        assert_eq!(jup_alias_key("0xff"), "#00000000000000ff");
        assert_eq!(jup_alias_key("a"), "#af63dc4c8601ec8c");
        assert!(!jup_has("never_loaded_alias"));
    }

    /// `BO2_JUP_INSTALL=<install> cargo test -p asset_audio jup_sounds --
    /// --ignored --nocapture`: Resurgence's sounds read and a few decoded.
    #[test]
    #[ignore]
    fn jup_sounds() {
        let install = jup_install().expect("BO2_JUP_INSTALL");
        let c = jup_resurgence_sounds(&install, false).expect("sounds");
        println!("{} aliases", c.sounds.len());
        for s in c.sounds.iter().take(5) {
            let a = &s.aliases[0];
            let (dir, name) = a.streamed.as_ref().unwrap();
            let pcm = read_jup_clip(dir, name).map(|p| (p.rate, p.channels, p.seconds()));
            println!("{}: {pcm:?}", s.name);
        }
    }
}

//! bo2mp: Call of Duty: Modern Warfare III (2023, `jup`) guns as guest
//! guns in a Black Ops II match (`GUNS`).
//!
//! `fastfile_jup::gun` reads each one's part models (view and world) from their
//! `_tr` zones and puts them together on the receiver's tags; this builds
//! them as Black Ops II models (one bone, `tag_weapon`, with `tag_flash`
//! and `tag_brass` where the gun has them) and their materials from the
//! install's `global_stream_mp`:
//!
//! - a gun material's pictures are its colour map (usage 0x36, BC1 or
//!   BC7, as the maps' materials have it), its normal map (format 0x2f,
//!   BC7: gloss in red, occlusion in blue and the normal in diamond form in
//!   green and alpha (`unpack_normal`); turned into BC5 for Black Ops II's
//!   draw), a gloss picture (usage 0x32) and masks. A
//!   material without a colour map draws as plain metal. (Row 6 of its
//!   constants was once taken for a tint; on most guns it is no colour.)
//! - a surface whose material the model points back to (a part's shared
//!   material, not read) draws in dark gun metal.
//!
//! The weapon itself is a copy of a Black Ops II gun of the same kind
//! (`DONOR`: the B23R, Black Ops II's Beretta 93R) with its own models,
//! name and handling (`jup_guest_rows`): Black Ops II's animations, arms,
//! effects and rules, Modern Warfare III's look. MW3's own view
//! animations are not read.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use asset_core::{AssetEdge, AssetNamespace, AssetRef, ZoneOwner};
use asset_material::{
    AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP,
    TS_NORMAL_MAP, TS_SPECULAR_MAP,
};
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use fastfile_jup::gun::{self, Gun, PartModel};
use fastfile_jup::image::{self, ImageRun};
use fastfile_jup::map::ZoneImage;
use fastfile_jup::map::materials::{MapImage, Packages};
use fastfile_jup::oodle::Decompressor;
use fastfile_t7::texture::ImageLevels;

use super::t7_map::{f16_bits, t6_unit_vec, t7_draw};

// bo2mp: the guns written to a folder of our own files, and read back from it
// when the game is not installed (a child module: it holds what `JupRead`
// holds).
#[path = "jup_export.rs"]
mod export;

const NS: AssetNamespace = AssetNamespace::Jup;

/// The texture slots Black Ops II's lit model draw reads.
const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;

/// The largest picture side kept for the first-person and world models.
const FIRST_PERSON_MAX: u32 = 1024;
const WORLD_MAX: u32 = 512;

/// One part of a gun: its view and world part zones and the receiver's
/// tag it goes on (`None`: the receiver).
pub(super) struct Part {
    pub(super) view: &'static str,
    pub(super) world: &'static str,
    pub(super) tag: Option<&'static str>,
}

/// A Modern Warfare III gun played as a guest gun.
pub(crate) struct JupGun {
    /// Internal name without `jup_` (`bo2_profile::jup_names`).
    pub gun: &'static str,
    /// The Black Ops II weapon it is a copy of.
    pub donor: &'static str,
    /// Its own handling over the donor's (Black Ops II's numbers).
    pub fire_type: i32,
    pub fire_time_ms: i32,
    pub clip_size: i32,
    pub max_ammo: i32,
    pub(super) parts: &'static [Part],
}

/// Modern Warfare III's guns played as guest guns, each its receiver and the
/// parts of its default build. The part zones are the install's (named in
/// `global_stream_mp`'s transient zone list); where a gun's own zones hold
/// no plain part of a kind, a sibling's is used or the part is left off.
/// Handling: `fire_time_ms` 0 keeps the donor's (bolt actions and
/// semi-automatics keep Black Ops II's cycling).
pub(crate) const GUNS: &[JupGun] = &[
    // Renetti: its receiver, slide and barrel, 15-round magazine and grip
    // panels; semi-automatic.
    JupGun {
        gun: "jp12_pi_mike93",
        donor: "beretta93r_mp",
        // Semi-automatic (Black Ops II's single shot), 15 rounds.
        fire_type: 1,
        fire_time_ms: 100,
        clip_size: 15,
        max_ammo: 75,
        parts: &[
            Part {
                view: "mp_av_jup_jp19_pi_mike93_rec_000_lod0_tr",
                world: "mp_aw_jup_jp19_pi_mike93_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp12_pi_mike93_bar_000_lod0_tr",
                world: "mp_aw_wm_jup_jp12_pi_mike93_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp12_pi_mike93_mag_15_9mm_000_lod0_tr",
                world: "mp_aw_wm_jup_jp12_pi_mike93_mag_15_9mm_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp12_pi_mike93_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp12_pi_mike93_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // SVA 545: automatic, 600 rounds a minute after its opening pair.
    JupGun {
        gun: "jp36_ar_anov94",
        donor: "an94_mp",
        fire_type: 0,
        fire_time_ms: 100,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp36_ar_anov94_rec_000_lod0_tr",
                world: "mp_aw_jup_jp36_ar_anov94_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp36_ar_anov94_bar_light_000_lod0_tr",
                world: "mp_aw_jup_jp36_ar_anov94_bar_light_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp36_ar_anov94_mag_30_000_lod0_tr",
                world: "mp_aw_wm_jup_jp36_ar_anov94_mag_30_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp36_ar_anov94_stock_000_lod0_tr",
                world: "mp_aw_jup_jp36_ar_anov94_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // MCW: automatic, 780 a minute.
    JupGun {
        gun: "jp19_ar_acharlie",
        donor: "hk416_mp",
        fire_type: 0,
        fire_time_ms: 77,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp19_ar_acharlie_rec_000_lod0_tr",
                world: "mp_aw_jup_jp19_ar_acharlie_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp19_ar_acharlie_bar_000_lod0_tr",
                world: "mp_aw_jup_jp19_ar_acharlie_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp19_ar_acharlie_mag_30_556n_000_lod0_tr",
                world: "mp_aw_wm_jup_jp19_ar_acharlie_mag_30_556n_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp19_ar_acharlie_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp19_ar_acharlie_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp19_ar_acharlie_stock_assault_000_lod0_tr",
                world: "mp_aw_jup_jp19_ar_acharlie_stock_assault_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // BAL-27: automatic, 750 a minute.
    JupGun {
        gun: "jp34_ar_balpha27",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp34_ar_balpha27_rec_000_lod0_tr",
                world: "mp_aw_jup_jp34_ar_balpha27_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp34_ar_balpha27_bar_000_lod0_tr",
                world: "mp_aw_jup_jp34_ar_balpha27_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp34_ar_balpha27_mag_000_lod0_tr",
                world: "mp_aw_wm_jup_jp34_ar_balpha27_mag_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp34_ar_balpha27_stock_000_lod0_tr",
                world: "mp_aw_jup_jp34_ar_balpha27_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // FR 5.56: three-round burst.
    JupGun {
        gun: "jp05_ar_foxtrot2000",
        donor: "xm8_mp",
        fire_type: 3,
        fire_time_ms: 63,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp05_ar_foxtrot2000_rec_000_lod0_tr",
                world: "mp_aw_wm_jup_jp05_ar_foxtrot2000_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp05_ar_foxtrot2000_bar_000_lod0_tr",
                world: "mp_aw_jup_jp05_ar_foxtrot2000_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp05_ar_foxtrot2000_mag_30_556n_000_lod0_tr",
                world: "mp_aw_wm_jup_jp05_ar_foxtrot2000_mag_30_556n_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp05_ar_foxtrot2000_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp05_ar_foxtrot2000_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp05_ar_foxtrot2000_stock_000_lod0_tr",
                world: "mp_aw_jup_jp05_ar_foxtrot2000_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // DG-56: three-round burst.
    JupGun {
        gun: "jp08_ar_qbravo97",
        donor: "type95_mp",
        fire_type: 3,
        fire_time_ms: 67,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp08_ar_qbravo97_rec_000_lod0_tr",
                world: "mp_aw_wm_jup_jp08_ar_qbravo97_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp08_ar_qbravo97_bar_000_lod0_tr",
                world: "mp_aw_jup_jp08_ar_qbravo97_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp08_ar_qbravo97_mag_30_556mm_000_lod0_tr",
                world: "mp_aw_wm_jup_jp08_ar_qbravo97_mag_30_556mm_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
        ],
    },
    // STG44: automatic, 545 a minute.
    JupGun {
        gun: "jp44_ar_stango44",
        donor: "tar21_mp",
        fire_type: 0,
        fire_time_ms: 110,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_jup_jp44_ar_stango44_rec_000_lod0_tr",
                world: "mp_aw_jup_jp44_ar_stango44_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp44_ar_stango44_bar_000_lod0_tr",
                world: "mp_aw_jup_jp44_ar_stango44_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp44_ar_stango44_mag_30_000_lod0_tr",
                world: "mp_aw_wm_jup_jp44_ar_stango44_mag_30_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp44_ar_stango44_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp44_ar_stango44_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp44_ar_stango44_stock_000_lod0_tr",
                world: "mp_aw_jup_jp44_ar_stango44_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // MCW 6.8: automatic, 615 a minute.
    JupGun {
        gun: "jp19_br_acharlie450",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 98,
        clip_size: 20,
        max_ammo: 100,
        parts: &[
            Part {
                view: "mp_av_jup_jp19_br_acharlie450_rec_000_lod0_tr",
                world: "mp_aw_jup_jp19_br_acharlie450_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp19_br_acharlie450_bar_000_lod0_tr",
                world: "mp_aw_jup_jp19_br_acharlie450_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_jup_jp19_br_acharlie450_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp19_br_acharlie450_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp19_ar_acharlie_mag_20_450_000_lod0_tr",
                world: "mp_aw_wm_jup_jp19_ar_acharlie_mag_20_450_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp19_br_acharlie450_stock_assault_000_lod0_tr",
                world: "mp_aw_jup_jp19_br_acharlie450_stock_assault_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // BAS-B: automatic, 650 a minute.
    JupGun {
        gun: "jp03_br_aromeo200",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 92,
        clip_size: 20,
        max_ammo: 100,
        parts: &[
            Part {
                view: "mp_av_jup_jp03_br_aromeo200_rec_000_lod0_tr",
                world: "mp_aw_jup_jp03_br_aromeo200_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp03_br_aromeo200_bar_000_lod0_tr",
                world: "mp_aw_jup_jp03_br_aromeo200_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp03_br_aromeo200_mag_000_lod0_tr",
                world: "mp_aw_wm_jup_jp03_br_aromeo200_mag_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp03_br_aromeo200_stock_000_lod0_tr",
                world: "mp_aw_jup_jp03_br_aromeo200_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // KAR98K: bolt action (the donor's bolt timing).
    JupGun {
        gun: "jp37_dm_kappa98",
        donor: "ballista_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_jup_jp37_dm_kappa98_rec_000_lod0_tr",
                world: "mp_aw_jup_jp37_dm_kappa98_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp37_dm_kappa98_bar_000_lod0_tr",
                world: "mp_aw_jup_jp37_dm_kappa98_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_jup_jp37_dm_kappa98_stock_000_lod0_tr",
                world: "mp_aw_jup_jp37_dm_kappa98_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_jup_jp37_dm_kappa98_irons_000_lod0_tr",
                world: "mp_aw_jup_jp37_dm_kappa98_irons_000_lod0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // Pulemyot 762: automatic, 650 a minute, 100-round belt.
    JupGun {
        gun: "jp06_lm_pkilop",
        donor: "mk48_mp",
        fire_type: 0,
        fire_time_ms: 92,
        clip_size: 100,
        max_ammo: 300,
        parts: &[
            Part {
                view: "mp_av_jup_jp06_lm_pkilop_rec_000_lod0_tr",
                world: "mp_aw_wm_jup_jp06_lm_pkilop_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp06_lm_pkilop_bar_000_lod0_tr",
                world: "mp_aw_wm_jup_jp06_lm_pkilop_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp06_lm_pkilop_belt_100_762n_000_lod0_tr",
                world: "mp_aw_wm_jup_jp06_lm_pkilop_belt_100_762n_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp06_lm_pkilop_stock_000_lod0_tr",
                world: "mp_aw_jup_jp06_lm_pkilop_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // DG-58 LSW: automatic, 640 a minute.
    JupGun {
        gun: "jp08_lm_qbravo95lsw",
        donor: "qbb95_mp",
        fire_type: 0,
        fire_time_ms: 94,
        clip_size: 60,
        max_ammo: 240,
        parts: &[
            Part {
                view: "mp_av_jup_jp08_lm_qbravo95lsw_rec_000_lod0_tr",
                world: "mp_aw_wm_jup_jp08_lm_qbravo95lsw_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp08_lm_qbravo95lsw_bar_000_lod0_tr",
                world: "mp_aw_jup_jp08_lm_qbravo95lsw_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp08_lm_qbravo95lsw_mag_60_58h_000_lod0_tr",
                world: "mp_aw_wm_jup_jp08_lm_qbravo95lsw_mag_60_58h_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
        ],
    },
    // Reclaimer 18: semi-automatic.
    JupGun {
        gun: "jp38_sh_spapa12",
        donor: "saiga12_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 8,
        max_ammo: 32,
        parts: &[
            Part {
                view: "mp_av_jup_jp38_sh_spapa12_rec_000_lod0_tr",
                world: "mp_aw_wm_jup_jp38_sh_spapa12_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp38_sh_spapa12_bar_000_lod0_tr",
                world: "mp_aw_wm_jup_jp38_sh_spapa12_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_jup_jp38_sh_spapa12_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp38_sh_spapa12_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp38_sh_spapa12_stock_000_lod0_tr",
                world: "mp_aw_jup_jp38_sh_spapa12_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // Static-HV: automatic, 850 a minute, 50 rounds.
    JupGun {
        gun: "jp41_sm_kpappa50",
        donor: "pdw57_mp",
        fire_type: 0,
        fire_time_ms: 71,
        clip_size: 50,
        max_ammo: 200,
        parts: &[
            Part {
                view: "mp_av_jup_jp41_sm_kpappa50_rec_000_lod0_tr",
                world: "mp_aw_jup_jp41_sm_kpappa50_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp41_sm_kpappa50_bar_000_lod0_tr",
                world: "mp_aw_jup_jp41_sm_kpappa50_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp41_sm_kpappa50_mag_50_000_lod0_tr",
                world: "mp_aw_wm_jup_jp41_sm_kpappa50_mag_50_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp41_sm_kpappa50_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp41_sm_kpappa50_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp41_sm_kpappa50_stock_000_lod0_tr",
                world: "mp_aw_jup_jp41_sm_kpappa50_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // RAM-9: automatic, 857 a minute.
    JupGun {
        gun: "jp25_sm_talpham",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 70,
        clip_size: 32,
        max_ammo: 160,
        parts: &[
            Part {
                view: "mp_av_jup_jp25_sm_talpham_rec_000_lod0_tr",
                world: "mp_aw_jup_jp25_sm_talpham_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp25_sm_talpham_bar_000_lod0_tr",
                world: "mp_aw_jup_jp25_sm_talpham_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp25_sm_talpham_mag_32_9mm_000_lod0_tr",
                world: "mp_aw_wm_jup_jp25_sm_talpham_mag_32_9mm_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
        ],
    },
    // KATT-AMR: semi-automatic.
    JupGun {
        gun: "jp10_sn_cdelta50",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 7,
        max_ammo: 28,
        parts: &[
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_rec_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_bar_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp10_sn_cdelta50_mag7_50_000_lod0_tr",
                world: "mp_aw_wm_jup_jp10_sn_cdelta50_mag7_50_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_stock_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_guard_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_guard_000_lod0_tr",
                tag: Some("tag_guard_attach"),
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_bolt_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_bolt_000_lod0_tr",
                tag: Some("tag_bolt_attach"),
            },
            Part {
                view: "mp_av_jup_jp10_sn_cdelta50_scope_000_lod0_tr",
                world: "mp_aw_jup_jp10_sn_cdelta50_scope_000_lod0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // XRK Stalker: bolt action (the donor's bolt timing).
    JupGun {
        gun: "jp17_sn_hsierra",
        donor: "dsr50_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_jup_jp17_sn_hsierra_rec_000_lod0_tr",
                world: "mp_aw_jup_jp17_sn_hsierra_rec_000_lod0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_jup_jp17_sn_hsierra_bar_000_lod0_tr",
                world: "mp_aw_jup_jp17_sn_hsierra_bar_000_lod0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_jup_jp17_sn_hsierra_mag_5_50b_000_lod0_tr",
                world: "mp_aw_wm_jup_jp17_sn_hsierra_mag_5_50b_000_lod0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_jup_jp17_sn_hsierra_pgrip_000_lod0_tr",
                world: "mp_aw_jup_jp17_sn_hsierra_pgrip_000_lod0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_jup_jp17_sn_hsierra_stock_000_lod0_tr",
                world: "mp_aw_jup_jp17_sn_hsierra_stock_000_lod0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp17_sn_hsierra_bolt_000_lod0_tr",
                world: "mp_aw_vm_jup_jp17_sn_hsierra_bolt_000_lod0_tr",
                tag: Some("tag_bolt_attach"),
            },
            Part {
                view: "mp_av_vm_jup_jp17_sn_hsierra_scope_000_lod0_tr",
                world: "mp_aw_vm_jup_jp17_sn_hsierra_scope_000_lod0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // Modern Warfare II's guns, as Modern Warfare III carries them (its
    // `att_vm_pNN_<class>_<gun>` part zones, the default `_v0` parts).
    // M4 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_mike4",
        donor: "hk416_mp",
        fire_type: 0,
        fire_time_ms: 75,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_rec_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_barrel_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_mag_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_stock_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_pistolgrip_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_pistolgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // M16 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_mike16",
        donor: "hk416_mp",
        fire_type: 3,
        fire_time_ms: 60,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p01_ar_mike16_rec_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike16_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike16_barrel_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike16_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike16_stock_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike16_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_ar_mike16_smag_v0_tr",
                world: "mp_aw_att_wm_ar_mike16_smag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_ar_mike4_pistolgrip_v0_tr",
                world: "mp_aw_att_wm_p01_ar_mike4_pistolgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Kastov 762 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_akilo",
        donor: "an94_mp",
        fire_type: 0,
        fire_time_ms: 100,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_rec_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_mag_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_stock_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Kastov 545 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_akilo105",
        donor: "an94_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_ar_akilo105_rec_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo105_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo105_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo105_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo105_mag_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo105_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo105_stock_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo105_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo105_pgrip_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo105_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Kastov-74u (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_akilo74",
        donor: "an94_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_rec_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_stock_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_xmag_v16_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_xmag_v16_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Lachmann-556 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_kilo53",
        donor: "hk416_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_rec_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_barrel_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_mag_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_stock_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Lachmann-762 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_golf3",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 100,
        clip_size: 20,
        max_ammo: 100,
        parts: &[
            Part {
                view: "mp_av_att_vm_p02_ar_golf3_rec_v0_tr",
                world: "mp_aw_att_wm_p02_ar_golf3_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p02_ar_golf3_barrel_v0_tr",
                world: "mp_aw_att_wm_p02_ar_golf3_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_golf3_mag_v0_tr",
                world: "mp_aw_att_wm_p02_ar_golf3_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_golf3_stock_v0_tr",
                world: "mp_aw_att_wm_p02_ar_golf3_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_golf3_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p02_ar_golf3_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // TAQ-56 (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_scharlie",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p05_ar_scharlie_rec_v0_tr",
                world: "mp_aw_att_wm_p05_ar_scharlie_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p05_ar_scharlie_barrel_v0_tr",
                world: "mp_aw_att_wm_p05_ar_scharlie_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p05_ar_scharlie_mag_v0_tr",
                world: "mp_aw_att_wm_p05_ar_scharlie_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p05_ar_scharlie_stockhvy_v0_tr",
                world: "mp_aw_att_wm_p05_ar_scharlie_stockhvy_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // M13B (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_mcbravo",
        donor: "hk416_mp",
        fire_type: 0,
        fire_time_ms: 67,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p08_ar_mcbravo_rec_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcbravo_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcbravo_barrel_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcbravo_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcbravo_mag_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcbravo_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcbravo_stock_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcbravo_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcbravo_grip_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcbravo_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // M13C (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_mcharlie",
        donor: "hk416_mp",
        fire_type: 0,
        fire_time_ms: 75,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p08_ar_mcharlie_rec_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcharlie_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcharlie_barrel_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcharlie_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcharlie_mag_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcharlie_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcharlie_stock_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcharlie_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_ar_mcharlie_grip_v0_tr",
                world: "mp_aw_att_wm_p08_ar_mcharlie_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // FR Avancer (Modern Warfare II).
    JupGun {
        gun: "iw9_ar_falpha",
        donor: "xm8_mp",
        fire_type: 0,
        fire_time_ms: 60,
        clip_size: 25,
        max_ammo: 125,
        parts: &[
            Part {
                view: "mp_av_att_vm_p06_ar_falpha_rec_v0_tr",
                world: "mp_aw_att_wm_p06_ar_falpha_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p06_ar_falpha_barrel_v0_tr",
                world: "mp_aw_att_wm_p06_ar_falpha_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p06_ar_falpha_mag_v0_tr",
                world: "mp_aw_att_wm_p06_ar_falpha_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p06_ar_falpha_stock_v0_tr",
                world: "mp_aw_att_wm_p06_ar_falpha_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p06_ar_falpha_grip_v0_tr",
                world: "mp_aw_att_wm_p06_ar_falpha_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // SO-14 (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_soscar14",
        donor: "scar_mp",
        fire_type: 0,
        fire_time_ms: 100,
        clip_size: 20,
        max_ammo: 100,
        parts: &[
            Part {
                view: "mp_av_att_vm_p18_dm_soscar14_rec_v0_tr",
                world: "mp_aw_att_wm_p18_dm_soscar14_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p18_dm_soscar14_barrel_v0_tr",
                world: "mp_aw_att_wm_p18_dm_soscar14_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p18_dm_soscar14_mag_v0_tr",
                world: "mp_aw_att_wm_p18_dm_soscar14_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p18_dm_soscar14_rstock_v0_tr",
                world: "mp_aw_att_wm_p18_dm_soscar14_rstock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // TAQ-M (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_scromeo",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 10,
        max_ammo: 40,
        parts: &[
            Part {
                view: "mp_av_att_vm_p05_dm_scromeo_rec_v0_tr",
                world: "mp_aw_att_wm_p05_dm_scromeo_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p05_dm_scromeo_barrel_v0_tr",
                world: "mp_aw_att_wm_p05_dm_scromeo_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p05_dm_scromeo_mag_v0_tr",
                world: "mp_aw_att_wm_p05_dm_scromeo_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p05_dm_scromeo_stock_v0_tr",
                world: "mp_aw_att_wm_p05_dm_scromeo_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // EBR-14 (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_mike14",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 10,
        max_ammo: 40,
        parts: &[
            Part {
                view: "mp_av_att_vm_p18_dm_mike14_rec_v0_tr",
                world: "mp_aw_att_wm_p18_dm_mike14_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p18_dm_mike14_barrel_v0_tr",
                world: "mp_aw_att_wm_p18_dm_mike14_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p18_dm_mike14_mag_v0_tr",
                world: "mp_aw_att_wm_p18_dm_mike14_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p18_dm_mike14_stock_v0_tr",
                world: "mp_aw_att_wm_p18_dm_mike14_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p18_dm_mike14_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p18_dm_mike14_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Tempus Torrent (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_stango25",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 10,
        max_ammo: 40,
        parts: &[
            Part {
                view: "mp_av_att_vm_p01_dm_stango25_rec_v0_tr",
                world: "mp_aw_att_wm_p01_dm_stango25_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p01_dm_stango25_barrel_v0_tr",
                world: "mp_aw_att_wm_p01_dm_stango25_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_dm_stango25_mag_v0_tr",
                world: "mp_aw_att_wm_p01_dm_stango25_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_dm_stango25_stock_v0_tr",
                world: "mp_aw_att_wm_p01_dm_stango25_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_dm_stango25_guard_v0_tr",
                world: "mp_aw_att_wm_p01_dm_stango25_guard_v0_tr",
                tag: Some("tag_guard_attach"),
            },
        ],
    },
    // LM-S (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_pgolf1",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 10,
        max_ammo: 40,
        parts: &[
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_rec_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_barrel_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_mag_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_stock_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_dm_pgolf1_scope_v0_tr",
                world: "mp_aw_att_wm_p02_dm_pgolf1_scope_v0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // SP-R 208 (Modern Warfare II).
    JupGun {
        gun: "iw9_dm_mike24",
        donor: "ballista_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_rec_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_barrel_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_mag_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_stock_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_bolt_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_bolt_v0_tr",
                tag: Some("tag_rail_attach"),
            },
            Part {
                view: "mp_av_att_vm_p20_dm_mike24_scope_v0_tr",
                world: "mp_aw_att_wm_p20_dm_mike24_scope_v0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // Lachmann Sub (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_mpapa5",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 75,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p02_sm_mpapa5_rec_v0_tr",
                world: "mp_aw_att_wm_p02_sm_mpapa5_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p02_sm_mpapa5_barrel_v0_tr",
                world: "mp_aw_att_wm_p02_sm_mpapa5_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_sm_mpapa5_mag_v0_tr",
                world: "mp_aw_att_wm_p02_sm_mpapa5_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_sm_mpapa5_stock_v0_tr",
                world: "mp_aw_att_wm_p02_sm_mpapa5_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_sm_mpapa5_pstlgrip_v0_tr",
                world: "mp_aw_att_wm_p02_sm_mpapa5_pstlgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // VEL 46 (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_mpapa7",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 63,
        clip_size: 40,
        max_ammo: 160,
        parts: &[
            Part {
                view: "mp_av_att_vm_p09_sm_mpapa7_rec_v0_tr",
                world: "mp_aw_att_wm_p09_sm_mpapa7_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p09_sm_mpapa7_barrel_v0_tr",
                world: "mp_aw_att_wm_p09_sm_mpapa7_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p09_sm_mpapa7_mag_v0_tr",
                world: "mp_aw_att_wm_p09_sm_mpapa7_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p09_sm_mpapa7_stock_v0_tr",
                world: "mp_aw_att_wm_p09_sm_mpapa7_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p09_sm_mpapa7_pgrip_v0_tr",
                world: "mp_aw_att_wm_p09_sm_mpapa7_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // PDSW 528 (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_papa90",
        donor: "pdw57_mp",
        fire_type: 0,
        fire_time_ms: 67,
        clip_size: 50,
        max_ammo: 200,
        parts: &[
            Part {
                view: "mp_av_att_vm_p07_sm_papa90_rec_v0_tr",
                world: "mp_aw_att_wm_p07_sm_papa90_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p07_sm_papa90_barrel_v0_tr",
                world: "mp_aw_att_wm_p07_sm_papa90_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p07_sm_papa90_mag_v0_tr",
                world: "mp_aw_att_wm_p07_sm_papa90_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p07_sm_papa90_stock_v0_tr",
                world: "mp_aw_att_wm_p07_sm_papa90_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p07_sm_papa90_rail_v0_tr",
                world: "mp_aw_att_wm_p07_sm_papa90_rail_v0_tr",
                tag: Some("tag_rail_attach"),
            },
        ],
    },
    // Fennec 45 (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_victor",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 50,
        clip_size: 25,
        max_ammo: 125,
        parts: &[
            Part {
                view: "mp_av_att_vm_p10_sm_victor_rec_v0_tr",
                world: "mp_aw_att_wm_p10_sm_victor_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p10_sm_victor_barrel_v0_tr",
                world: "mp_aw_att_wm_p10_sm_victor_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p10_sm_victor_mag_v0_tr",
                world: "mp_aw_att_wm_p10_sm_victor_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p10_sm_victor_stock_v0_tr",
                world: "mp_aw_att_wm_p10_sm_victor_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // Minibak (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_beta",
        donor: "pdw57_mp",
        fire_type: 0,
        fire_time_ms: 75,
        clip_size: 64,
        max_ammo: 192,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_sm_beta_rec_v0_tr",
                world: "mp_aw_att_wm_p04_sm_beta_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_sm_beta_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_sm_beta_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sm_beta_mag_v0_tr",
                world: "mp_aw_att_wm_p04_sm_beta_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_stock_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // FSS Hurricane (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_alpha57",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 70,
        clip_size: 50,
        max_ammo: 200,
        parts: &[
            Part {
                view: "mp_av_att_vm_p01_sm_alpha57_rec_v0_tr",
                world: "mp_aw_att_wm_p01_sm_alpha57_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p01_sm_alpha57_barrel_v0_tr",
                world: "mp_aw_att_wm_p01_sm_alpha57_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_sm_alpha57_mag_v0_tr",
                world: "mp_aw_att_wm_p01_sm_alpha57_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p01_sm_alpha57_stock_v0_tr",
                world: "mp_aw_att_wm_p01_sm_alpha57_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // BAS-P (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_mpapax",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 67,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p08_sm_mpapax_rec_v0_tr",
                world: "mp_aw_att_wm_p08_sm_mpapax_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p08_sm_mpapax_barrel_v0_tr",
                world: "mp_aw_att_wm_p08_sm_mpapax_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_sm_mpapax_mag_v0_tr",
                world: "mp_aw_att_wm_p08_sm_mpapax_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_sm_mpapax_stock_v0_tr",
                world: "mp_aw_att_wm_p08_sm_mpapax_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p08_sm_mpapax_grip_v0_tr",
                world: "mp_aw_att_wm_p08_sm_mpapax_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // ISO 45 (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_acharlie45",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 75,
        clip_size: 25,
        max_ammo: 125,
        parts: &[
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie45_rec_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie45_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie45_barrel_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie45_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie45_mag_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie45_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie45_stock_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie45_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie45_grip_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie45_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // ISO 9mm (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_acharlie9",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 63,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie9_rec_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie9_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie9_barrel_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie9_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie9_mag_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie9_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie9_stockno_v0_tr",
                world: "mp_aw_att_vm_p43_sm_acharlie9_stockno_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p43_sm_acharlie9_pgrip_v0_tr",
                world: "mp_aw_att_wm_p43_sm_acharlie9_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Vaznev-9K (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_aviktor",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 67,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_sm_aviktor_rec_v0_tr",
                world: "mp_aw_att_wm_p04_sm_aviktor_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_sm_aviktor_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_sm_aviktor_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sm_aviktor_mag_v0_tr",
                world: "mp_aw_att_wm_p04_sm_aviktor_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo74_stock_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo74_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // MX9 (Modern Warfare II).
    JupGun {
        gun: "iw9_sm_apapa",
        donor: "mp7_mp",
        fire_type: 0,
        fire_time_ms: 70,
        clip_size: 30,
        max_ammo: 150,
        parts: &[
            Part {
                view: "mp_av_att_vm_p03_sm_apapa_rec_v0_tr",
                world: "mp_aw_att_wm_p03_sm_apapa_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p03_sm_apapa_barrel_v0_tr",
                world: "mp_aw_att_wm_p03_sm_apapa_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p03_sm_apapa_mag_v0_tr",
                world: "mp_aw_att_wm_p03_sm_apapa_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p03_sm_apapa_stock_v0_tr",
                world: "mp_aw_att_wm_p03_sm_apapa_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // RPK (Modern Warfare II).
    JupGun {
        gun: "iw9_lm_rkilo",
        donor: "mk48_mp",
        fire_type: 0,
        fire_time_ms: 85,
        clip_size: 75,
        max_ammo: 225,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_lm_rkilo_rec_v0_tr",
                world: "mp_aw_att_wm_p04_lm_rkilo_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_lm_rkilo_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_lm_rkilo_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_lm_rkilo_mag_v0_tr",
                world: "mp_aw_att_wm_p04_lm_rkilo_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_lm_rkilo_stock_v0_tr",
                world: "mp_aw_att_wm_p04_lm_rkilo_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_ar_akilo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p04_ar_akilo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // RAPP H (Modern Warfare II).
    JupGun {
        gun: "iw9_lm_kilo21",
        donor: "mk48_mp",
        fire_type: 0,
        fire_time_ms: 90,
        clip_size: 100,
        max_ammo: 300,
        parts: &[
            Part {
                view: "mp_av_att_vm_p02_lm_kilo21_rec_v0_tr",
                world: "mp_aw_att_wm_p02_lm_kilo21_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p02_lm_kilo21_barrel_v0_tr",
                world: "mp_aw_att_wm_p02_lm_kilo21_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_lm_kilo21_mag_v0_tr",
                world: "mp_aw_att_wm_p02_lm_kilo21_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_lm_kilo21_stock_v0_tr",
                world: "mp_aw_att_wm_p02_lm_kilo21_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p02_ar_kilo53_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p02_ar_kilo53_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
    // Sakin MG38 (Modern Warfare II).
    JupGun {
        gun: "iw9_lm_ngolf7",
        donor: "mk48_mp",
        fire_type: 0,
        fire_time_ms: 80,
        clip_size: 100,
        max_ammo: 300,
        parts: &[
            Part {
                view: "mp_av_att_vm_p17_lm_ngolf7_rec_v0_tr",
                world: "mp_aw_att_wm_p17_lm_ngolf7_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p17_lm_ngolf7_barrel_v0_tr",
                world: "mp_aw_att_wm_p17_lm_ngolf7_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p17_lm_ngolf7_mag_v0_tr",
                world: "mp_aw_att_wm_p17_lm_ngolf7_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p17_lm_ngolf7_stock_v0_tr",
                world: "mp_aw_att_wm_p17_lm_ngolf7_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
        ],
    },
    // KV Broadside (Modern Warfare II).
    JupGun {
        gun: "iw9_sh_vecho",
        donor: "saiga12_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 8,
        max_ammo: 32,
        parts: &[
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_rec_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_barrel_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_mag_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_stock_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_pgrip_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p04_sh_vecho_bolt_v0_tr",
                world: "mp_aw_att_wm_p04_sh_vecho_bolt_v0_tr",
                tag: Some("tag_rail_attach"),
            },
        ],
    },
    // Expedite 12 (Modern Warfare II).
    JupGun {
        gun: "iw9_sh_mike1014",
        donor: "saiga12_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 8,
        max_ammo: 32,
        parts: &[
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_rec_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_barrel_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_stock_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_pgrip_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_bolt_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_bolt_v0_tr",
                tag: Some("tag_rail_attach"),
            },
            Part {
                view: "mp_av_att_vm_p12_sh_mike1014_guard_v0_tr",
                world: "mp_aw_att_wm_p12_sh_mike1014_guard_v0_tr",
                tag: Some("tag_guard_attach"),
            },
        ],
    },
    // Lockwood 300 (Modern Warfare II).
    JupGun {
        gun: "iw9_sh_charlie725",
        donor: "saiga12_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 2,
        max_ammo: 24,
        parts: &[
            Part {
                view: "mp_av_att_vm_p14_sh_charlie725_rec_v0_tr",
                world: "mp_aw_att_wm_p14_sh_charlie725_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p14_sh_charlie725_barrel_v0_tr",
                world: "mp_aw_att_wm_p14_sh_charlie725_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p14_sh_charlie725_stock_v0_tr",
                world: "mp_aw_att_wm_p14_sh_charlie725_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p14_sh_charlie725_guard_v0_tr",
                world: "mp_aw_att_wm_p14_sh_charlie725_guard_v0_tr",
                tag: Some("tag_guard_attach"),
            },
        ],
    },
    // MX Guardian (Modern Warfare II).
    JupGun {
        gun: "iw9_sh_tsierra12",
        donor: "saiga12_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 8,
        max_ammo: 32,
        parts: &[
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_rec_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_barrel_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_mag_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_stock_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_pgrip_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p44_sh_tsierra12_trigger_v0_tr",
                world: "mp_aw_att_wm_p44_sh_tsierra12_trigger_v0_tr",
                tag: Some("tag_trigger_attach"),
            },
        ],
    },
    // Victus XMR (Modern Warfare II).
    JupGun {
        gun: "iw9_sn_alpha50",
        donor: "dsr50_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_rec_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_barrel_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_mag_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_stock_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_guard_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_guard_v0_tr",
                tag: Some("tag_guard_attach"),
            },
            Part {
                view: "mp_av_att_vm_p23_sn_alpha50_scope_v0_tr",
                world: "mp_aw_att_wm_p23_sn_alpha50_scope_v0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // MCPR-300 (Modern Warfare II).
    JupGun {
        gun: "iw9_sn_mromeo",
        donor: "dsr50_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_rec_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_barrel_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_mag_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_stock_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_bolt_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_bolt_v0_tr",
                tag: Some("tag_rail_attach"),
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_pstlgrp_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_pstlgrp_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p21_sn_mromeo_scope_v0_tr",
                world: "mp_aw_att_wm_p21_sn_mromeo_scope_v0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // Carrack .300 (Modern Warfare II).
    JupGun {
        gun: "iw9_sn_walpha",
        donor: "svu_mp",
        fire_type: 1,
        fire_time_ms: 0,
        clip_size: 6,
        max_ammo: 30,
        parts: &[
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_rec_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_barrel_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_mag_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_stock_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_pgrip_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_guard_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_guard_v0_tr",
                tag: Some("tag_guard_attach"),
            },
            Part {
                view: "mp_av_att_vm_p54_sn_walpha_scope_v0_tr",
                world: "mp_aw_att_wm_p54_sn_walpha_scope_v0_tr",
                tag: Some("tag_scope_attach"),
            },
        ],
    },
    // X12 (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_golf17",
        donor: "beretta93r_mp",
        fire_type: 1,
        fire_time_ms: 100,
        clip_size: 17,
        max_ammo: 85,
        parts: &[
            Part {
                view: "mp_av_att_vm_p24_pi_golf17_rec_v0_tr",
                world: "mp_aw_att_wm_p24_pi_golf17_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p24_pi_golf17_barrel_v0_tr",
                world: "mp_aw_att_wm_p24_pi_golf17_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p24_pi_golf17_mag_v0_tr",
                world: "mp_aw_att_wm_p24_pi_golf17_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p24_pi_golf17_trigger_v0_tr",
                world: "mp_aw_att_wm_p24_pi_golf17_trigger_v0_tr",
                tag: Some("tag_trigger_attach"),
            },
        ],
    },
    // .50 GS (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_decho",
        donor: "beretta93r_mp",
        fire_type: 1,
        fire_time_ms: 150,
        clip_size: 7,
        max_ammo: 35,
        parts: &[
            Part {
                view: "mp_av_att_vm_p25_pi_decho_rec_v0_tr",
                world: "mp_aw_att_wm_p25_pi_decho_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p25_pi_decho_barrel_v0_tr",
                world: "mp_aw_att_wm_p25_pi_decho_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p25_pi_decho_mag_v0_tr",
                world: "mp_aw_att_wm_p25_pi_decho_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p25_pi_decho_grip_v0_tr",
                world: "mp_aw_att_wm_p25_pi_decho_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p25_pi_decho_trig_v0_tr",
                world: "mp_aw_att_wm_p25_pi_decho_trig_v0_tr",
                tag: Some("tag_trigger_attach"),
            },
        ],
    },
    // Basilisk (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_swhiskey",
        donor: "beretta93r_mp",
        fire_type: 1,
        fire_time_ms: 250,
        clip_size: 5,
        max_ammo: 25,
        parts: &[
            Part {
                view: "mp_av_att_vm_p26_pi_swhiskey_rec_v0_tr",
                world: "mp_aw_att_wm_p26_pi_swhiskey_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p26_pi_swhiskey_bar_v0_tr",
                world: "mp_aw_att_wm_p26_pi_swhiskey_bar_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p26_pi_swhiskey_pgrip_v0_tr",
                world: "mp_aw_att_wm_p26_pi_swhiskey_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p26_pi_swhiskey_cylinder_v0_tr",
                world: "mp_aw_att_wm_p26_pi_swhiskey_cylinder_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p26_pi_swhiskey_trig_v0_tr",
                world: "mp_aw_att_wm_p26_pi_swhiskey_trig_v0_tr",
                tag: Some("tag_trigger_attach"),
            },
        ],
    },
    // P890 (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_papa220",
        donor: "beretta93r_mp",
        fire_type: 1,
        fire_time_ms: 120,
        clip_size: 8,
        max_ammo: 40,
        parts: &[
            Part {
                view: "mp_av_att_vm_p27_pi_papa220_rec_v0_tr",
                world: "mp_aw_att_wm_p27_pi_papa220_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p27_pi_papa220_barrel_v0_tr",
                world: "mp_aw_att_wm_p27_pi_papa220_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p27_pi_papa220_mag_v0_tr",
                world: "mp_aw_att_wm_p27_pi_papa220_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p27_pi_papa220_grip_v0_tr",
                world: "mp_aw_att_wm_p27_pi_papa220_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p27_pi_papa220_hammer_v0_tr",
                world: "mp_aw_att_wm_p27_pi_papa220_hammer_v0_tr",
                tag: Some("tag_rail_attach"),
            },
        ],
    },
    // 9mm Daemon (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_mike2011",
        donor: "beretta93r_mp",
        fire_type: 1,
        fire_time_ms: 90,
        clip_size: 17,
        max_ammo: 85,
        parts: &[
            Part {
                view: "mp_av_att_vm_p55_pi_mike2011_rec_v0_tr",
                world: "mp_aw_att_wm_p55_pi_mike2011_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p55_pi_mike2011_barrel_v0_tr",
                world: "mp_aw_att_wm_p55_pi_mike2011_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p55_pi_mike2011_mag_v0_tr",
                world: "mp_aw_att_wm_p55_pi_mike2011_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p55_pi_mike2011_grip_v0_tr",
                world: "mp_aw_att_wm_p55_pi_mike2011_grip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
            Part {
                view: "mp_av_att_vm_p55_pi_mike2011_trigger_v0_tr",
                world: "mp_aw_att_wm_p55_pi_mike2011_trigger_v0_tr",
                tag: Some("tag_trigger_attach"),
            },
        ],
    },
    // FTAC Siege (Modern Warfare II).
    JupGun {
        gun: "iw9_pi_tango9",
        donor: "beretta93r_mp",
        fire_type: 0,
        fire_time_ms: 60,
        clip_size: 30,
        max_ammo: 120,
        parts: &[
            Part {
                view: "mp_av_att_vm_p49_pi_tango9_rec_v0_tr",
                world: "mp_aw_att_wm_p49_pi_tango9_rec_v0_tr",
                tag: None,
            },
            Part {
                view: "mp_av_att_vm_p49_pi_tango9_barrel_v0_tr",
                world: "mp_aw_att_wm_p49_pi_tango9_barrel_v0_tr",
                tag: Some("tag_barrel_attach"),
            },
            Part {
                view: "mp_av_att_vm_p49_pi_tango9_mag_v0_tr",
                world: "mp_aw_att_wm_p49_pi_tango9_mag_v0_tr",
                tag: Some("tag_mag_attach"),
            },
            Part {
                view: "mp_av_att_vm_p49_pi_tango9_stock_v0_tr",
                world: "mp_aw_att_wm_p49_pi_tango9_stock_v0_tr",
                tag: Some("tag_stock_attach"),
            },
            Part {
                view: "mp_av_att_vm_p49_pi_tango9_pgrip_v0_tr",
                world: "mp_aw_att_wm_p49_pi_tango9_pgrip_v0_tr",
                tag: Some("tag_pistolgrip_attach"),
            },
        ],
    },
];

/// Every guest gun this lane builds: Modern Warfare III's (`GUNS`).
pub(crate) fn all_guns() -> impl Iterator<Item = &'static JupGun> {
    GUNS.iter()
}

/// A gun's first-person model's name.
pub(crate) fn view_model(gun: &str) -> String {
    format!("viewmodel_jup_{gun}")
}

/// A gun's world model's name.
pub(crate) fn world_model(gun: &str) -> String {
    format!("weapon_jup_{gun}")
}

/// The pictures of one material, decoded.
#[derive(Default)]
pub(super) struct MaterialPixels {
    /// sRGB bytes.
    tint: [u8; 3],
    /// (width, height, RGBA8): the colour map, the occlusion picture, the
    /// normal map.
    colour: Option<(u32, u32, Vec<u8>)>,
    occlusion: Option<(u32, u32, Vec<u8>)>,
    normal: Option<(u32, u32, Vec<u8>)>,
    /// A gun's: its part mask (usage 0x32 BC7: green the metal, magenta
    /// the polymer, blue trim), whose colours MWIII lays on from its
    /// weapon layers; its 0x36 picture is then wear and paint detail only.
    mask: Option<(u32, u32, Vec<u8>)>,
    /// A character's gloss picture (usage 0x32, BC1): its specular map's
    /// gloss, and, when it has no colour picture, what its constants'
    /// colour is shaded by.
    gloss: Option<(u32, u32, Vec<u8>)>,
    gun: bool,
}

/// A character material's picks (image hashes) and constant colour.
struct CharacterPictures {
    colour: Option<u64>,
    gloss: Option<u64>,
    tint: Option<[u8; 3]>,
}

/// A character material's colour picture (its largest usage-0x36 picture
/// no more than two wanted materials share, BC7 before BC1 at a size),
/// and its largest 0x32 picture (gloss); and the colour its constants
/// hold (`ff ff ff 00 r g b _` then half-float ones), which the materials
/// without a colour picture are drawn in (Ghost's trousers `2b2b2d`, his
/// shirt `272626`).
fn character_pictures(
    z: &[u8],
    m: usize,
    pictures: &[u64],
    fits: &dyn Fn(u64) -> bool,
    rec: &dyn Fn(u64) -> Option<image::ImageRec>,
    shared: &HashMap<u64, usize>,
) -> CharacterPictures {
    let recs: Vec<(u64, image::ImageRec)> = pictures
        .iter()
        .filter(|&&p| fits(p))
        .filter_map(|&p| rec(p).map(|r| (p, r)))
        .collect();
    let area = |r: &image::ImageRec| u32::from(r.width) * u32::from(r.height);
    let colour = recs
        .iter()
        .filter(|(p, r)| r.usage == 0x36 && matches!(r.format, 0x25 | 0x30) && shared.get(p).copied().unwrap_or(0) <= 2)
        .max_by_key(|(_, r)| (area(r), r.format == 0x30))
        .map(|(_, r)| r.hash);
    let gloss = recs
        .iter()
        .filter(|(_, r)| r.usage == 0x32 && r.format == 0x25)
        .max_by_key(|(_, r)| area(r))
        .map(|(_, r)| r.hash);
    let start = fastfile_jup::map::materials::picture_table(z, m) + 8 * pictures.len();
    let end = (start + 16 * usize::from(z[m + 0x1c]) + 0x100).min(z.len());
    let tint = z.get(start..end).and_then(|c| {
        c.windows(12).find_map(|w| {
            (w[..4] == [0xff, 0xff, 0xff, 0x00] && w[8..12] == [0x00, 0x3c, 0x00, 0x3c]).then(|| [w[4], w[5], w[6]])
        })
    });
    CharacterPictures {
        colour,
        gloss,
        tint,
    }
}

/// What is read off the match's critical path.
pub(crate) enum JupPrefetch {
    Off(Vec<String>),
    Ready(Box<JupRead>),
}

pub(crate) struct JupRead {
    install: PathBuf,
    /// Per gun: (view gun, world gun).
    /// Each gun's view and world models and its view receiver's top.
    guns: Vec<(&'static JupGun, Gun, Gun, f32)>,
    /// By material hash and size cap.
    materials: HashMap<(u64, u32), MaterialPixels>,
    /// The camos' pictures by name.
    camos: HashMap<String, NamedPicture>,
    report: Vec<String>,
}

pub(crate) struct JupGuests {
    pub fpv: FpvMeshBuild,
    pub world_guns: WorldWeaponBuild,
    pub material_seed: MaterialCatalog,
    /// bo2mp: the effects the guns' stand-ins play (Modern Warfare
    /// Remastered's, H1 namespace; Black Ops III's, T7), and their models.
    pub fx: asset_game::FxCatalog,
    pub fx_models: asset_game::FxModelCatalog,
    pub report: Vec<String>,
}

pub(super) fn oodle(install: &std::path::Path) -> Result<Box<dyn Decompressor>, String> {
    fastfile_jup::oodle::load(install).map_err(|e| e.to_string())
}

fn read_part(
    install: &std::path::Path,
    oodle: &dyn Decompressor,
    zone: &str,
) -> Result<PartModel, String> {
    let path = install.join("zone").join(format!("{zone}.ff"));
    let (_, bytes) = fastfile_jup::read_zone(&path, oodle).map_err(|e| format!("{zone}: {e}"))?;
    gun::read_part(&bytes).map_err(|e| format!("{zone}: {e}"))
}

/// The gun's parts read and put together (view, else world zones), and the
/// height of its receiver's top (its sights, unless a part carries them).
fn read_gun(
    install: &std::path::Path,
    oodle: &dyn Decompressor,
    g: &JupGun,
    view: bool,
) -> Result<(Gun, f32), String> {
    assemble_gun(g, view, &|zone| read_part(install, oodle, zone))
}

/// [`read_gun`] with the part zones read by `read` (the audit reads
/// decompressed zones from a folder).
pub(super) fn assemble_gun(
    g: &JupGun,
    view: bool,
    read: &dyn Fn(&str) -> Result<PartModel, String>,
) -> Result<(Gun, f32), String> {
    let zone = |p: &Part| if view { p.view } else { p.world };
    let receiver = g
        .parts
        .iter()
        .find(|p| p.tag.is_none())
        .ok_or("no receiver")?;
    let rec = read(zone(receiver))?;
    let top = rec
        .surfaces
        .iter()
        .flat_map(|s| s.positions.iter().map(|p| p[2]))
        .fold(f32::MIN, f32::max);
    let mut parts = Vec::new();
    // (A part whose model is not read is left off; the gun still draws.)
    let mut unread = Vec::new();
    for p in g.parts.iter().filter(|p| p.tag.is_some()) {
        match read(zone(p)) {
            Ok(m) => parts.push((m, p.tag.unwrap_or_default())),
            Err(e) => unread.push(e),
        }
    }
    let refs: Vec<(&PartModel, &str)> = parts.iter().map(|(m, t)| (m, *t)).collect();
    let mut gun = gun::assemble(&rec, &refs);
    gun.missing.extend(unread);
    // (A pistol's sights are on its slide, a part of its own.)
    let top = if donor_view_scale(g.donor) == 1.0 {
        top
    } else {
        gun.surfaces
            .iter()
            .flat_map(|s| s.positions.iter().map(|p| p[2]))
            .fold(top, f32::max)
    };
    Ok((gun, top))
}

fn u64_at(z: &[u8], o: usize) -> u64 {
    z.get(o..o + 8)
        .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
}

/// A material's header: its hash then the word most materials hold.
const MATERIAL_WORD: [u8; 4] = [0xc0, 0xb6, 0x73, 0x2c];
const MATERIAL_HEADER: usize = 0x78;

/// The materials' pictures from `global_stream_mp`, where gun materials
/// are defined. A picture pointer leads to the image's entry of the
/// asset list: the list's place in the pointers' block is found from the
/// pointers themselves (the one base that makes the most of them images).
pub(super) fn read_materials(
    install: &std::path::Path,
    oodle: &dyn Decompressor,
    wanted: &[(u64, u32)],
    gun: bool,
    report: &mut Vec<String>,
) -> Result<HashMap<(u64, u32), MaterialPixels>, String> {
    read_materials_and_pictures(install, oodle, wanted, gun, &[], report).map(|r| r.0)
}

/// A picture by its name: (width, height, RGBA8).
pub(super) type NamedPicture = (u32, u32, Vec<u8>);

/// [`read_materials`], and some pictures of `global_stream_mp` by name
/// (the camos' Create-a-Class pictures), from the one read of the zone.
pub(super) fn read_materials_and_pictures(
    install: &std::path::Path,
    oodle: &dyn Decompressor,
    wanted: &[(u64, u32)],
    gun: bool,
    names: &[&str],
    report: &mut Vec<String>,
) -> Result<(HashMap<(u64, u32), MaterialPixels>, HashMap<String, NamedPicture>), String> {
    let path = install.join("zone").join("global_stream_mp.ff");
    let (_, bytes) =
        fastfile_jup::read_zone(&path, oodle).map_err(|e| format!("global_stream_mp: {e}"))?;
    let zone = ZoneImage::parse(bytes).map_err(|e| format!("global_stream_mp: {e}"))?;
    let z = &zone.bytes;
    let named = named_pictures(install, z, names, oodle, report);
    let hashes: std::collections::HashSet<u64> = wanted.iter().map(|w| w.0).collect();
    let mut at: HashMap<u64, usize> = HashMap::new();
    // (Every material header: the shared picture tables are found by the
    // shifts their own tables give.)
    let mut headers = Vec::new();
    let mut i = 8;
    while let Some(p) = z
        .get(i..)
        .and_then(|r| r.windows(4).position(|w| w == MATERIAL_WORD))
    {
        let o = i + p - 8;
        let h = u64_at(z, o);
        headers.push(o);
        if hashes.contains(&h) {
            at.entry(h).or_insert(o);
        }
        i += p + 1;
    }
    let shared_tables = fastfile_jup::map::materials::SharedTables::new(z, &headers);
    let fits = |base: usize, p: u64| {
        let off = ((p & 0xffff_ffff) as usize).wrapping_sub(1);
        off >= base && (off - base) % 16 == 0 && zone.types.get((off - base) / 16) == Some(&21)
    };
    // (Its own table: the asset list's place is found from these.)
    let own_pictures = |m: usize| -> Vec<u64> {
        if fastfile_jup::map::materials::is_back(u64_at(z, m + 0x30)) {
            return Vec::new();
        }
        let t = fastfile_jup::map::materials::picture_table(z, m);
        (0..usize::from(z[m + 0x1b])).map(|k| u64_at(z, t + 8 * k)).collect()
    };
    let all: Vec<u64> = at.values().flat_map(|&m| own_pictures(m)).collect();
    let base = (zone.list_at.saturating_sub(0x200)..zone.list_at + 0x40)
        .max_by_key(|&b| {
            (
                all.iter().filter(|&&p| fits(b, p)).count(),
                std::cmp::Reverse(b),
            )
        })
        .unwrap_or(zone.list_at);
    // (Its table, its own or one it shares with an earlier material.)
    let pictures = |m: usize| -> Vec<u64> {
        let n = usize::from(z[m + 0x1b]);
        match u64_at(z, m + 0x30) {
            v if fastfile_jup::map::materials::is_back(v) => shared_tables
                .at(z, v, n, &|p| fits(base, p))
                .map_or_else(Vec::new, |t| (0..n).map(|k| u64_at(z, t + 8 * k)).collect()),
            _ => own_pictures(m),
        }
    };
    // (How many of the wanted materials each picture pointer is in.)
    let mut shared: HashMap<u64, usize> = HashMap::new();
    for &m in at.values() {
        let mut own = pictures(m);
        own.sort_unstable();
        own.dedup();
        for p in own {
            *shared.entry(p).or_default() += 1;
        }
    }
    let mut nth = vec![usize::MAX; zone.types.len()];
    let mut k = 0;
    for (i, &t) in zone.types.iter().enumerate() {
        if t == 21 {
            nth[i] = k;
            k += 1;
        }
    }
    let run = ImageRun::find(z, k).ok_or("global_stream_mp: image run not found")?;
    let paks = Packages::open(install);
    let mut out = HashMap::new();
    let (mut pics, mut missing) = (0usize, 0usize);
    for &(hash, max) in wanted {
        let Some(&m) = at.get(&hash) else {
            report.push(format!(
                "jup guest guns: material {hash:016x} not in global_stream_mp"
            ));
            continue;
        };
        // (A material with no colour picture: plain metal under its
        // occlusion. Row 6 of the constants was taken for a tint, but on
        // most guns those bytes are no colour: the RAM-9 drew teal.)
        let mut px = MaterialPixels {
            tint: METAL,
            gun,
            ..MaterialPixels::default()
        };
        if let Some(dir) = dump_dir() {
            dump_material(&dir, hash, &pictures(m), z, base, &zone.types, &nth, &run, &paks, oodle);
        }
        // (A character's: its colour is its largest picture of usage 0x36
        // that is its own (the ones many materials share are lookups);
        // without one, the colour its constants hold, shaded by its largest
        // 0x32 picture.)
        let chosen = character_pictures(z, m, &pictures(m), &|p| fits(base, p), &|p| {
            let asset = (((p & 0xffff_ffff) as usize) - 1 - base) / 16;
            run.at.get(nth[asset]).and_then(|&o| image::parse(z, o).ok().flatten())
        }, &shared);
        if !gun && let Some(t) = chosen.tint {
            px.tint = t;
        }
        for p in pictures(m) {
            if !fits(base, p) {
                continue;
            }
            let asset = (((p & 0xffff_ffff) as usize) - 1 - base) / 16;
            let Some(rec) = run
                .at
                .get(nth[asset])
                .and_then(|&o| image::parse(z, o).ok().flatten())
            else {
                continue;
            };
            // (A character's usage 0x36 picture is its colour, its 0x32 one
            // its gloss; a gun's 0x36 is a wear layer (not read) and its
            // 0x32 BC7 its part mask. Format 0x2f is the packed normal map:
            // gloss, normal y, occlusion, normal x. The first of a kind is
            // the main one; later ones are detail maps.)
            let slot = match (rec.format, rec.usage) {
                (0x2f, _) => &mut px.normal,
                _ if !gun && chosen.colour == Some(rec.hash) => &mut px.colour,
                _ if !gun && chosen.gloss == Some(rec.hash) => &mut px.gloss,
                (0x30, 0x32) if gun => &mut px.mask,
                _ => continue,
            };
            if slot.is_some() {
                continue;
            }
            let img = MapImage {
                hash: rec.hash,
                width: rec.width,
                height: rec.height,
                format: rec.format,
                groups: rec.groups.clone(),
                inline: rec.inline.map(|(o, n)| z[o..o + n].to_vec()),
            };
            match paks.level(&img, max, oodle) {
                Ok(l) => {
                    match fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data) {
                        Some(rgba) => {
                            pics += 1;
                            *slot = Some((l.width, l.height, rgba));
                        }
                        None => missing += 1,
                    }
                }
                Err(_) => missing += 1,
            }
        }
        out.insert((hash, max), px);
    }
    report.push(format!(
        "jup guest guns: materials: {} of {} found, {pics} pictures, {missing} not in the install",
        out.len(),
        wanted.len()
    ));
    Ok((out, named))
}

/// Pictures of a zone by name, at most 256 pixels a side: its image
/// assets whose hash is the name's, their largest level the packages have.
fn named_pictures(
    install: &std::path::Path,
    z: &[u8],
    names: &[&str],
    oodle: &dyn Decompressor,
    report: &mut Vec<String>,
) -> HashMap<String, NamedPicture> {
    let mut out = HashMap::new();
    if names.is_empty() {
        return out;
    }
    let hashes: Vec<u64> = names.iter().map(|n| fastfile_jup::hash::name(n)).collect();
    let index = fastfile_jup::zone::HashIndex::build(z, hashes.iter().copied(), 8);
    let paks = Packages::open(install);
    for (name, &hash) in names.iter().zip(&hashes) {
        let Some(rec) = index.plain(hash).iter().find_map(|&at| match image::parse(z, at) {
            Ok(Some(r)) if r.hash == hash && r.depth == 1 && r.array == 1 => Some(r),
            _ => None,
        }) else {
            continue;
        };
        let img = MapImage {
            hash: rec.hash,
            width: rec.width,
            height: rec.height,
            format: rec.format,
            groups: rec.groups.clone(),
            inline: rec.inline.map(|(o, n)| z[o..o + n].to_vec()),
        };
        if let Some(px) = paks.level(&img, 256, oodle).ok().and_then(|l| {
            let rgba = fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data)?;
            Some((l.width, l.height, rgba))
        }) {
            out.insert((*name).to_owned(), px);
        }
    }
    report.push(format!("jup camos: {} of {} pictures read", out.len(), names.len()));
    out
}

/// A `jup_dump` folder beside the game holding at most a note: every gun
/// material's pictures are written there, small, to check which is which
/// (once: a folder already holding pictures is left as it is).
fn dump_dir() -> Option<std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::current_exe().ok()?.parent()?.join("jup_dump");
        (std::fs::read_dir(&dir).ok()?.count() <= 1).then_some(dir)
    })
    .clone()
}

#[allow(clippy::too_many_arguments)]
fn dump_material(
    dir: &std::path::Path,
    hash: u64,
    pictures: &[u64],
    z: &[u8],
    base: usize,
    types: &[u32],
    nth: &[usize],
    run: &ImageRun,
    paks: &Packages,
    oodle: &dyn Decompressor,
) {
    for (k, &p) in pictures.iter().enumerate() {
        let off = ((p & 0xffff_ffff) as usize).wrapping_sub(1);
        if off < base || (off - base) % 16 != 0 || types.get((off - base) / 16) != Some(&21) {
            continue;
        }
        let Some(rec) = nth
            .get((off - base) / 16)
            .and_then(|&i| run.at.get(i))
            .and_then(|&o| image::parse(z, o).ok().flatten())
        else {
            continue;
        };
        let img = MapImage {
            hash: rec.hash,
            width: rec.width,
            height: rec.height,
            format: rec.format,
            groups: rec.groups.clone(),
            inline: rec.inline.map(|(o, n)| z[o..o + n].to_vec()),
        };
        let Ok(l) = paks.level(&img, 256, oodle) else {
            continue;
        };
        let Some(rgba) = fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data) else {
            continue;
        };
        let path = dir.join(format!(
            "{hash:016x}_{k:02}_u{:02x}_f{:02x}.png",
            rec.usage, rec.format
        ));
        if let Ok(file) = std::fs::File::create(path) {
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), l.width, l.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut w) = enc.write_header() {
                let _ = w.write_image_data(&rgba);
            }
        }
    }
}

/// bo2mp: reads the Modern Warfare III guest guns' parts and materials
/// from the install (`wanted`: Black Ops II rules with guest guns on).
pub(crate) fn prefetch_jup_guest_guns(wanted: bool) -> JupPrefetch {
    prefetch(wanted, true)
}

/// [`prefetch_jup_guest_guns`]; `exported`: where Modern Warfare III's
/// install is not set, the guns come from the exported folder
/// (`BO2_JUP_EXPORT`, `asset_transport::jup_export`).
fn prefetch(wanted: bool, exported: bool) -> JupPrefetch {
    if !wanted {
        return JupPrefetch::Off(vec!["jup guest guns: off".into()]);
    }
    let jup = asset_audio::jup_install();
    // (Without the game, Modern Warfare III's guns, pictures and camos are
    // read from the folder they were exported to.)
    let export_dir = if jup.is_none() && exported { asset_transport::jup_export::dir() } else { None };
    let Some(install) = jup.clone().or_else(|| export_dir.clone()) else {
        return JupPrefetch::Off(vec![
            "jup guest guns: Modern Warfare III (2023) is not installed, and no export of its guns is found (BO2_JUP_EXPORT)".into(),
        ]);
    };
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let oodle: Option<Box<dyn Decompressor>> = match jup.as_deref().map(oodle) {
        Some(Ok(o)) => Some(o),
        Some(Err(e)) => {
            report.push(format!("jup guest guns: {e}"));
            None
        }
        None => None,
    };
    if oodle.is_none() && export_dir.is_none() {
        return JupPrefetch::Off(report);
    }
    let mut guns = Vec::new();
    let mut wanted_mats: Vec<(u64, u32)> = Vec::new();
    for g in all_guns() {
        let Some((install, oodle)) = jup.as_deref().zip(oodle.as_deref()) else {
            continue;
        };
        // (A world zone the reader does not follow yet, or a view one, gives
        // way to the other model: drawn from the side the other's mesh, the
        // gun is never left without one.)
        let pair = match (
            read_gun(install, oodle, g, true),
            read_gun(install, oodle, g, false),
        ) {
            (Ok(view), Err(e)) => {
                report.push(format!("jup guest guns: {}: world model is the view model's ({e})", g.gun));
                Ok((view.clone(), view))
            }
            (Err(e), Ok(world)) => {
                report.push(format!("jup guest guns: {}: view model is the world model's ({e})", g.gun));
                Ok((world.clone(), world))
            }
            (Ok(view), Ok(world)) => Ok((view, world)),
            (Err(e), Err(_)) => Err(e),
        };
        match pair {
            Ok(((view, view_top), (world, _))) => {
                for (gun, max) in [(&view, FIRST_PERSON_MAX), (&world, WORLD_MAX)] {
                    for s in &gun.surfaces {
                        if let Some(m) = s.material
                            && !wanted_mats.contains(&(m, max))
                        {
                            wanted_mats.push((m, max));
                        }
                    }
                }
                report.push(format!(
                    "jup guest guns: {}: view {} surfaces {} bones, world {} surfaces{}",
                    g.gun,
                    view.surfaces.len(),
                    view.bones.len(),
                    world.surfaces.len(),
                    if view.missing.is_empty() && world.missing.is_empty() {
                        String::new()
                    } else {
                        format!(" (not placed: {:?} {:?})", view.missing, world.missing)
                    }
                ));
                guns.push((g, view, world, view_top));
            }
            Err(e) => report.push(format!("jup guest guns: {}: {e}", g.gun)),
        }
    }
    let camo_names: Vec<&str> = asset_game::weapon_camo_models::JUP_CAMOS
        .iter()
        .chain(asset_game::weapon_camo_models::IW9_CAMOS.iter())
        .copied()
        .collect();
    let (mut materials, mut camos) = match (jup.as_deref(), oodle.as_deref()) {
        (Some(jup), Some(oodle)) => match read_materials_and_pictures(jup, oodle, &wanted_mats, true, &camo_names, &mut report) {
            Ok(m) => m,
            Err(e) => {
                report.push(format!("jup guest guns: {e} (drawn without pictures)"));
                (HashMap::new(), HashMap::new())
            }
        },
        _ => (HashMap::new(), HashMap::new()),
    };
    if let Some(dir) = &export_dir {
        export::read_into(dir, &mut guns, &mut materials, &mut camos, &mut report);
    }
    report.push(format!(
        "jup guest guns: read in {:.1} s",
        started.elapsed().as_secs_f64()
    ));
    JupPrefetch::Ready(Box::new(JupRead {
        install,
        guns,
        materials,
        camos,
        report,
    }))
}

/// sRGB byte to linear.
fn linear(c: u8) -> f32 {
    let s = f32::from(c) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// The colour map: the tint, darkened by the occlusion (alpha of the
/// occlusion picture), as sRGB RGBA8.
fn colour_levels(px: &MaterialPixels) -> ImageLevels {
    if px.gun
        && let Some(l) = gun_colour(px)
    {
        return l;
    }
    if let Some((w, h, rgba)) = &px.colour {
        return ImageLevels {
            format: 29,
            width: *w,
            height: *h,
            levels: 1,
            data: rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            part: None,
        };
    }
    let tint = px.tint.map(linear);
    // (A character's constant colour, shaded by its grey picture about its
    // mean brightness.)
    if px.colour.is_none()
        && let Some((w, h, rgba)) = &px.gloss
    {
        let lum = |p: &[u8]| linear(p[0]) * 0.2126 + linear(p[1]) * 0.7152 + linear(p[2]) * 0.0722;
        let n = (rgba.len() / 4).max(1);
        let mean = (rgba.chunks_exact(4).map(lum).sum::<f32>() / n as f32).max(1e-3);
        let data = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                let k = (lum(p) / mean).clamp(0.4, 1.6).sqrt();
                [srgb(tint[0] * k), srgb(tint[1] * k), srgb(tint[2] * k), 255]
            })
            .collect();
        return ImageLevels {
            format: 29,
            width: *w,
            height: *h,
            levels: 1,
            data,
            part: None,
        };
    }
    match &px.occlusion {
        Some((w, h, rgba)) => {
            let data = rgba
                .chunks_exact(4)
                .flat_map(|p| {
                    let ao = f32::from(p[3]) / 255.0;
                    [
                        srgb(tint[0] * ao),
                        srgb(tint[1] * ao),
                        srgb(tint[2] * ao),
                        255,
                    ]
                })
                .collect();
            ImageLevels {
                format: 29,
                width: *w,
                height: *h,
                levels: 1,
                data,
                part: None,
            }
        }
        None => ImageLevels {
            format: 29,
            width: 4,
            height: 4,
            levels: 1,
            data: [px.tint[0], px.tint[1], px.tint[2], 255].repeat(16),
            part: None,
        },
    }
}

/// MWIII's packed normal (the 0x37 / format 0x2f maps' green `g` and alpha
/// `a`; read off its own pixel programs, which decode every normal map
/// this way): the two channels are the normal's diamond axes,
/// `p = 2g - 1`, `q = 2a - 1`, `x = (p + q) / 2`, `y = (p - q) / 2`,
/// `z = 1 - |x| - |y|` (an octahedral fold, never negative), then
/// normalised. Returns the tangent-space (x, y) of the unit normal.
/// (Reading alpha as x and green as y, as before, turned every normal 45
/// degrees and stretched it.)
pub(crate) fn unpack_normal(g: u8, a: u8) -> [f32; 2] {
    let p = f32::from(g) / 255.0 * 2.0 - 1.0;
    let q = f32::from(a) / 255.0 * 2.0 - 1.0;
    let (x, y) = ((p + q) * 0.5, (p - q) * 0.5);
    let z = 1.0 - x.abs() - y.abs();
    let inv = (x * x + y * y + z * z).sqrt().max(1e-6).recip();
    [x * inv, y * inv]
}

/// A unit-range component (-1..1) as a byte (0.5 is 128).
fn unit_byte(v: f32) -> u8 {
    ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A gun's packed normal map (format 0x2f, usage 0x37): gloss in red, the
/// normal's y in green, occlusion in blue, its x in alpha. Sampled at
/// (x, y) of a `w` x `h` picture: (gloss, occlusion).
fn gloss_occlusion(px: &MaterialPixels, x: u32, y: u32, w: u32, h: u32) -> Option<(f32, f32)> {
    let (nw, nh, rgba) = px.normal.as_ref()?;
    let (sx, sy) = (x * nw / w, y * nh / h);
    let i = ((sy * nw + sx) * 4) as usize;
    rgba.get(i..i + 4)
        .map(|p| (f32::from(p[0]) / 255.0, f32::from(p[2]) / 255.0))
}

/// A gun's colour: its finish by the part mask (green the metal, magenta
/// the polymer, blue trim; plain without one), under the occlusion its
/// packed normal map holds. Its 0x36 picture is not its colour but a
/// wear and pattern layer MWIII lays its finishes through (smeared tan on
/// the KATT-AMR, a splatter on the Renetti): left out.
fn gun_colour(px: &MaterialPixels) -> Option<ImageLevels> {
    let (w, h) = match (&px.mask, &px.normal) {
        (Some((w, h, _)), _) | (None, Some((w, h, _))) => (*w, *h),
        (None, None) => return None,
    };
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = Finish::of(mask_at(px, x, y, w, h)).albedo().map(linear);
            let ao = gloss_occlusion(px, x, y, w, h).map_or(1.0, |(_, ao)| ao);
            data.extend([srgb(c[0] * ao), srgb(c[1] * ao), srgb(c[2] * ao), 255]);
        }
    }
    Some(ImageLevels {
        format: 29,
        width: w,
        height: h,
        levels: 1,
        data,
        part: None,
    })
}

/// The part mask's pixel at (x, y) of a `w` x `h` picture.
fn mask_at(px: &MaterialPixels, x: u32, y: u32, w: u32, h: u32) -> Option<[u8; 4]> {
    let (mw, mh, rgba) = px.mask.as_ref()?;
    let (sx, sy) = (x * mw / w, y * mh / h);
    let i = ((sy * mw + sx) * 4) as usize;
    rgba.get(i..i + 4).map(|p| [p[0], p[1], p[2], p[3]])
}

/// What a gun surface is made of, by its part mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finish {
    /// Green: the finished (blued, coated) metal.
    Metal,
    /// Magenta or black: polymer, rubber.
    Polymer,
    /// White or blue: lighter machined trim.
    Trim,
    /// A material without a mask.
    Plain,
}

impl Finish {
    fn of(m: Option<[u8; 4]>) -> Self {
        match m.map(|p| (p[0] > 127, p[1] > 127, p[2] > 127)) {
            Some((false, true, false)) => Self::Metal,
            Some((false, false, true) | (true, true, true)) => Self::Trim,
            Some(_) => Self::Polymer,
            None => Self::Plain,
        }
    }

    /// Its colour (sRGB): dark coated metal, black polymer, lighter trim.
    fn albedo(self) -> [u8; 3] {
        match self {
            Self::Metal => [0x44, 0x46, 0x4a],
            Self::Polymer => [0x2e, 0x2e, 0x30],
            Self::Trim => [0x5c, 0x5d, 0x60],
            Self::Plain => METAL,
        }
    }

    /// Its reflectance at normal incidence: a coated gun's metal reflects
    /// little more than its polymer; only machined trim reads as metal.
    fn reflectance(self) -> f32 {
        match self {
            Self::Metal => 0.10,
            Self::Polymer => 0.04,
            Self::Trim => 0.20,
            Self::Plain => 0.06,
        }
    }

    /// Its gloss without a packed normal map, and the most it takes from
    /// one: no gun surface is a mirror.
    fn gloss(self) -> (f32, f32) {
        match self {
            Self::Metal => (0.5, 0.62),
            Self::Polymer => (0.35, 0.45),
            Self::Trim => (0.55, 0.7),
            Self::Plain => (0.4, 0.5),
        }
    }
}

/// The specular byte Black Ops II's lit draw reads as reflectance `f0`: it
/// takes the byte's sRGB encoding squared (`raw_texel(v)^2`), so a
/// dielectric's 0.04 is a byte of 8 or 9.
fn spec_byte(f0: f32) -> u8 {
    let s = f0.clamp(0.0, 1.0).sqrt();
    let l = if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    };
    (l * 255.0).round() as u8
}

/// The specular map (Black Ops II's: specular colour, gloss in alpha) of a
/// character (dielectric cloth and gear, gloss from its 0x32 picture) or a
/// gun (each finish its own reflectance and gloss, by its mask).
fn specular_levels(px: &MaterialPixels) -> Option<ImageLevels> {
    if px.gun {
        // (Gloss is the packed normal map's red where there is one, held
        // under the finish's most; its occlusion darkens the specular.)
        let (w, h) = match (&px.mask, &px.normal) {
            (Some((w, h, _)), _) | (None, Some((w, h, _))) => (*w, *h),
            (None, None) => return None,
        };
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let f = Finish::of(mask_at(px, x, y, w, h));
                let (default_gloss, most) = f.gloss();
                let (gloss, ao) = gloss_occlusion(px, x, y, w, h).unwrap_or((default_gloss, 1.0));
                let c = spec_byte(f.reflectance() * ao);
                data.extend([c, c, c, (gloss.min(most) * 255.0).round() as u8]);
            }
        }
        return Some(ImageLevels {
            format: 28,
            width: w,
            height: h,
            levels: 1,
            data,
            part: None,
        });
    }
    let (w, h, rgba) = px.gloss.as_ref()?;
    let data = rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let g = (u32::from(p[0]) * 2 + u32::from(p[1]) * 5 + u32::from(p[2])) / 8;
            [0x38, 0x38, 0x38, (g * 7 / 8) as u8]
        })
        .collect();
    Some(ImageLevels {
        format: 28,
        width: *w,
        height: *h,
        levels: 1,
        data,
        part: None,
    })
}

/// Dark gun metal, for surfaces whose material is not read.
const GUN_METAL: [u8; 3] = [0x1c, 0x1c, 0x1e];

/// Metal under a material's occlusion when it has no colour picture.
const METAL: [u8; 3] = [0x3a, 0x3b, 0x3d];

pub(super) struct Builder {
    catalog: MaterialCatalog,
    owner: ZoneOwner,
    images: HashMap<String, Option<usize>>,
    materials: HashMap<(Option<u64>, u32), Option<usize>>,
    failures: Vec<String>,
}

impl Builder {
    /// A builder linking into `catalog`, its pictures owned by the
    /// install's `global_stream_mp`.
    pub(super) fn new(catalog: MaterialCatalog, install: &std::path::Path) -> Self {
        Self {
            catalog,
            owner: ZoneOwner::from_zone_path(&install.join("zone").join("global_stream_mp.ff")),
            images: HashMap::new(),
            materials: HashMap::new(),
            failures: Vec::new(),
        }
    }

    /// The catalog being linked into.
    pub(super) fn catalog_ref(&self) -> &MaterialCatalog {
        &self.catalog
    }

    /// The catalog with what was linked, and what failed.
    pub(super) fn finish(self) -> (MaterialCatalog, Vec<String>) {
        (self.catalog, self.failures)
    }

    fn image(
        &mut self,
        key: String,
        semantic: u8,
        make: impl FnOnce() -> Option<ImageLevels>,
    ) -> Option<usize> {
        if let Some(slot) = self.images.get(&key) {
            return *slot;
        }
        let colour = semantic == TS_COLOR_MAP;
        let slot = make().and_then(|l| {
            let px = fastfile_iw6::image::ImagePixels {
                width: l.width,
                height: l.height,
                levels: l.levels,
                dxgi_format: l.format,
                data: l.data,
            };
            match super::iw6::gpu_image(&px, colour) {
                Ok(gpu) => Some(self.catalog.link_image(AuthoredImage {
                    namespace: NS,
                    name: AssetRef::Real(key.clone()),
                    map_type: 0,
                    semantic,
                    category: 0,
                    use_srgb_reads: colour,
                    width: px.width as u16,
                    height: px.height as u16,
                    depth: 1,
                    level_count: px.levels as u8,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(gpu)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                })),
                Err(e) => {
                    self.failures.push(format!("{key}: {e}"));
                    None
                }
            }
        });
        self.images.insert(key, slot);
        slot
    }

    /// A material of the gun into the catalog (`None`: dark gun metal).
    pub(super) fn material(
        &mut self,
        hash: Option<u64>,
        max: u32,
        read: &HashMap<(u64, u32), MaterialPixels>,
    ) -> Option<usize> {
        if let Some(&m) = self.materials.get(&(hash, max)) {
            return m;
        }
        let fallback = MaterialPixels {
            tint: GUN_METAL,
            ..Default::default()
        };
        let px = hash.and_then(|h| read.get(&(h, max))).unwrap_or(&fallback);
        let name = hash.map_or_else(|| "jup_gun_metal".to_owned(), |h| format!("jup_{h:016x}"));
        let mut textures = Vec::new();
        if let Some(slot) = self.image(format!("{name}#{max}"), TS_COLOR_MAP, || {
            Some(colour_levels(px))
        }) {
            textures.push(MaterialTextureBinding {
                name_hash: COLOR_MAP_HASH,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x13,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
        }
        if let Some((w, h, rgba)) = &px.normal {
            // (The packed normal map: green and alpha hold the normal in
            // MWIII's diamond form, see `unpack_normal`; red and blue are
            // gloss and occlusion.)
            let levels = ImageLevels {
                format: 28,
                width: *w,
                height: *h,
                levels: 1,
                data: rgba
                    .chunks_exact(4)
                    .flat_map(|p| {
                        let [x, y] = unpack_normal(p[1], p[3]);
                        [unit_byte(x), unit_byte(y), 0, 255]
                    })
                    .collect(),
                part: None,
            };
            if let Some(slot) = self.image(format!("{name}#{max}#n"), TS_NORMAL_MAP, || {
                fastfile_t7::texture::normal_map_bc5(&levels)
            }) {
                textures.push(MaterialTextureBinding {
                    name_hash: NORMAL_MAP_HASH,
                    name_start: 0,
                    name_end: 0,
                    sampler_state: 0x13,
                    semantic: TS_NORMAL_MAP,
                    image: Some(slot),
                });
            }
        }
        if let Some(levels) = specular_levels(px)
            && let Some(slot) = self.image(format!("{name}#{max}#s"), TS_SPECULAR_MAP, || Some(levels))
        {
            textures.push(MaterialTextureBinding {
                name_hash: SPECULAR_MAP_HASH,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x13,
                semantic: TS_SPECULAR_MAP,
                image: Some(slot),
            });
        }
        let draw = t7_draw(&name, "", true);
        let index = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(format!("{name}#{max}")),
            namespace: NS,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: Vec::new(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        self.materials.insert((hash, max), Some(index));
        Some(index)
    }
}


fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Each vertex's binormal sign: whether cross(normal, tangent) runs along
/// the UVs' v on its triangles.
pub(super) fn binormal_signs(s: &gun::PartSurface) -> Vec<f32> {
    let mut acc = vec![[0.0f32; 3]; s.positions.len()];
    for t in s.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize);
        if a.max(b).max(c) >= s.positions.len() {
            continue;
        }
        let (e1, e2) = (
            sub(s.positions[b], s.positions[a]),
            sub(s.positions[c], s.positions[a]),
        );
        let (du1, dv1) = (s.uvs[b][0] - s.uvs[a][0], s.uvs[b][1] - s.uvs[a][1]);
        let (du2, dv2) = (s.uvs[c][0] - s.uvs[a][0], s.uvs[c][1] - s.uvs[a][1]);
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1e-12 {
            continue;
        }
        let bt = [
            (e2[0] * du1 - e1[0] * du2) / r,
            (e2[1] * du1 - e1[1] * du2) / r,
            (e2[2] * du1 - e1[2] * du2) / r,
        ];
        for &v in &[a, b, c] {
            for k in 0..3 {
                acc[v][k] += bt[k];
            }
        }
    }
    (0..s.positions.len())
        .map(|v| {
            if dot(cross(s.normals[v], s.tangents[v]), acc[v]) >= 0.0 {
                1.0
            } else {
                -1.0
            }
        })
        .collect()
}

/// Where the gun's model space puts Black Ops II's `tag_weapon` (the
/// hand): the receiver's grip tag, across the gun on the receiver's centre
/// line (Black Ops II's guns are modelled with the hand at the origin and
/// the bore over it; Modern Warfare III's receivers sit ahead of theirs).
/// Without a grip tag, the gun's origin.
pub(super) fn hand_anchor(g: &Gun) -> [f32; 3] {
    let mut at = grip_tag(g).unwrap_or([0.0; 3]);
    if let Some((lo, hi)) = receiver_bounds(g) {
        at[1] = 0.5 * (lo[1] + hi[1]);
    }
    at
}

/// The receiver's grip tag, or for a gun held by its stock's wrist (the
/// SO-14's rifle stock) just ahead of where the stock meets the receiver.
pub(super) fn grip_tag(g: &Gun) -> Option<[f32; 3]> {
    if let Some(b) = g.bone("tag_pistolgrip_attach") {
        return Some(b.pos);
    }
    g.bone("tag_stock_attach")
        .map(|b| [b.pos[0] + STOCK_WRIST, b.pos[1], b.pos[2]])
}

/// The receiver's mesh bounds (its own surfaces, not the parts').
pub(super) fn receiver_bounds(g: &Gun) -> Option<([f32; 3], [f32; 3])> {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in g.surfaces[..g.receiver_surfaces.min(g.surfaces.len())]
        .iter()
        .flat_map(|s| &s.positions)
    {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// How far ahead of `tag_stock_attach` a rifle stock's wrist is held.
const STOCK_WRIST: f32 = 1.5;

/// Where a Black Ops II donor's pistol grip is along its first-person
/// model (the middle of its grip a little below the receiver, measured on
/// `t6_wpn_*_view` in common_mp).
pub(super) fn donor_grip(donor: &str) -> Option<f32> {
    Some(match donor {
        "beretta93r_mp" => -2.5,
        "an94_mp" | "hk416_mp" | "xm8_mp" | "type95_mp" | "qbb95_mp" => -2.0,
        "scar_mp" | "ballista_mp" | "svu_mp" => -1.0,
        "tar21_mp" => -2.5,
        "mk48_mp" => 0.0,
        "saiga12_mp" => -5.0,
        "pdw57_mp" => -3.0,
        "mp7_mp" => -0.5,
        "dsr50_mp" => -1.5,
        _ => return None,
    })
}

/// How far the middle of a grip sits behind the tag it hangs from
/// (`tag_pistolgrip_attach`, the grip's top): grips slant back.
pub(super) const GRIP_SLANT: f32 = 1.0;

/// How much larger a donor's first-person model is drawn than the gun:
/// Black Ops II's pistols are about a third larger than life.
pub(super) fn donor_view_scale(donor: &str) -> f32 {
    if donor == "beretta93r_mp" { 1.3 } else { 1.0 }
}

/// The top of a Black Ops II donor's first-person model above its origin
/// (its sights; the bounds of `t6_wpn_*_view` in common_mp).
pub(super) fn donor_view_top(donor: &str) -> Option<f32> {
    Some(match donor {
        "beretta93r_mp" => 2.38,
        "an94_mp" => 5.28,
        "hk416_mp" => 5.43,
        "scar_mp" => 5.43,
        "xm8_mp" => 3.99,
        "type95_mp" => 4.90,
        "tar21_mp" => 4.20,
        "ballista_mp" => 4.62,
        "mk48_mp" => 6.27,
        "qbb95_mp" => 4.70,
        "saiga12_mp" => 3.40,
        "pdw57_mp" => 3.78,
        "mp7_mp" => 3.40,
        "svu_mp" => 4.34,
        "dsr50_mp" => 4.99,
        _ => return None,
    })
}

/// Where a Black Ops II donor's hands hold its first-person model: the
/// centres of its arms' two palms (the mean of the wrist, the middle and
/// index finger bases, the pinky and ring palm bones and the thumb's base)
/// in the gun's root space, (right, left), with the arms posed by the
/// donor's idle animation at its first frame (measured by posing the arms'
/// skeleton from common_mp with `viewmodel_*_idle`).
pub(super) fn donor_hands(donor: &str) -> Option<([f32; 3], [f32; 3])> {
    Some(match donor {
        "beretta93r_mp" => ([-3.04, -1.23, -2.10], [-2.27, 1.39, -4.07]),
        "an94_mp" => ([-2.41, -1.47, -1.51], [8.44, 1.23, -1.22]),
        "hk416_mp" => ([-2.15, -0.94, -0.83], [9.10, 1.03, 0.10]),
        "xm8_mp" => ([-2.94, -1.08, -2.51], [10.34, 1.19, -2.77]),
        "type95_mp" => ([-2.51, -1.60, -3.23], [4.75, 1.41, -2.88]),
        "qbb95_mp" => ([-2.51, -1.61, -3.23], [6.43, 1.33, -2.17]),
        "scar_mp" => ([-2.76, -1.35, -2.09], [9.46, 0.53, -0.76]),
        "ballista_mp" => ([-1.22, -1.58, -2.12], [10.77, 0.98, -1.07]),
        "svu_mp" => ([-1.35, -1.36, -1.61], [5.38, 1.05, -1.68]),
        "tar21_mp" => ([-4.57, -1.21, -2.63], [4.66, 0.96, -2.64]),
        "mk48_mp" => ([-0.17, -1.24, -2.78], [14.55, 1.73, -0.71]),
        "saiga12_mp" => ([-5.17, -1.20, -3.57], [7.77, 0.84, -3.35]),
        "pdw57_mp" => ([-3.28, -1.22, -2.52], [3.79, 1.92, -2.15]),
        "mp7_mp" => ([-2.14, -1.21, -2.45], [4.59, 0.88, -2.43]),
        "dsr50_mp" => ([-1.56, -1.30, -1.48], [6.03, 1.27, -3.16]),
        _ => return None,
    })
}

/// The rear and front of a Black Ops II donor's first-person model along
/// its length (the bounds of `t6_wpn_*_view` in common_mp).
pub(super) fn donor_view_span(donor: &str) -> Option<(f32, f32)> {
    Some(match donor {
        "beretta93r_mp" => (-4.50, 8.70),
        "an94_mp" => (-15.68, 27.95),
        "hk416_mp" => (-12.24, 23.29),
        "xm8_mp" => (-7.99, 26.39),
        "type95_mp" => (-17.36, 18.65),
        "qbb95_mp" => (-17.36, 30.99),
        "scar_mp" => (-8.89, 25.04),
        "ballista_mp" => (-12.18, 35.20),
        "svu_mp" => (-15.80, 23.31),
        "tar21_mp" => (-18.26, 15.51),
        "mk48_mp" => (-8.64, 32.43),
        "saiga12_mp" => (-14.97, 31.54),
        "pdw57_mp" => (-9.13, 11.23),
        "mp7_mp" => (-14.32, 13.10),
        "dsr50_mp" => (-13.98, 33.13),
        _ => return None,
    })
}

/// How a gun's first-person model sits on its donor's `tag_weapon`.
#[derive(Clone, Copy, Debug)]
pub(super) struct ViewFit {
    /// Uniform scale about the hand anchor.
    pub scale: f32,
    /// Added after the anchor is taken to the origin (`ahead`, across, `lift`).
    pub offset: [f32; 3],
}

/// The most a gun is shrunk to bring its forend to the donor's left hand.
const MAX_FOREND_SHRINK: f32 = 0.8;

/// (The donor's animations hold the view model by its origin and aim
/// down its sights: the gun's receiver top goes where the donor's
/// sights are, else it sits high and fills the screen aimed.)
/// (Its grip goes where the donor's is: the donor's hand holds it
/// there. A pistol is drawn larger, as Black Ops II's are.)
pub(super) fn view_fit(g: &JupGun, view: &Gun, view_top: f32) -> ViewFit {
    let ahead_at = |scale: f32| {
        if grip_tag(view).is_none() {
            0.0
        } else {
            donor_grip(g.donor).map_or(0.0, |x| x + GRIP_SLANT * scale)
        }
    };
    let mut scale = donor_view_scale(g.donor);
    // (The donor's left hand holds the forend: a gun whose forend stands
    // farther ahead of its grip than the donor's does is drawn smaller,
    // about its grip, until the forend is in the hand, by at most
    // `MAX_FOREND_SHRINK`. A gun whose forend is short of the hand is not
    // stretched.)
    if let (Some(a), Some(f), Some((_, left))) = (view.bone("tag_barrel_attach"), view.bone("tag_flash"), donor_hands(g.donor)) {
        let den = 0.5 * (a.pos[0] + f.pos[0]) - hand_anchor(view)[0];
        let num = left[0] - ahead_at(scale);
        if den > 1.0 && num > 0.0 {
            scale *= (num / den / scale).clamp(MAX_FOREND_SHRINK, 1.0);
        }
    }
    let lift = donor_view_top(g.donor).map_or(0.0, |top| top - (view_top - hand_anchor(view)[2]) * scale);
    let ahead = ahead_at(scale);
    ViewFit {
        scale,
        offset: [ahead, 0.0, lift],
    }
}

/// The gun as a Black Ops II model: its surfaces on `tag_weapon`, and the
/// tags Black Ops II's effects and shells come out of.
fn model_ref(name: &str, g: &Gun, offset: [f32; 3], scale: f32) -> Option<asset_t6::XModelRef> {
    let anchor = hand_anchor(g);
    let place = |p: [f32; 3]| {
        [
            (p[0] - anchor[0]) * scale + offset[0],
            (p[1] - anchor[1]) * scale + offset[1],
            (p[2] - anchor[2]) * scale + offset[2],
        ]
    };
    let mut bone_names = vec!["tag_weapon".to_owned()];
    let mut base_mat: Vec<([f32; 4], [f32; 3])> = vec![([0.0, 0.0, 0.0, 1.0], [0.0; 3])];
    for tag in ["tag_flash", "tag_brass"] {
        if let Some(b) = g.bone(tag) {
            bone_names.push(tag.to_owned());
            base_mat.push((b.rot, place(b.pos)));
        }
    }
    let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut lod0 = Vec::new();
    for s in &g.surfaces {
        let n = s.positions.len();
        if n == 0 || n > usize::from(u16::MAX) {
            continue;
        }
        let signs = binormal_signs(s);
        let mut verts = Vec::with_capacity(n * 32);
        for v in 0..n {
            let p = place(s.positions[v]);
            for k in 0..3 {
                mins[k] = mins[k].min(p[k]);
                maxs[k] = maxs[k].max(p[k]);
            }
            verts.extend_from_slice(&super::t7_map::t6_vertex_signed(
                p,
                signs[v],
                s.uvs[v],
                s.normals[v],
                s.tangents[v],
            ));
        }
        lod0.push(asset_t6::XSurfaceRef {
            verts,
            vert_count: n as u16,
            indices: s.indices.iter().map(|&i| i as u16).collect(),
            vert_lists: Vec::new(),
            flags: 0,
            material: None,
            blend_counts: [n as i16, 0, 0, 0],
            blend: vec![0; n],
            part_bits: [1, 0, 0, 0, 0, 0],
        });
    }
    if lod0.is_empty() {
        return None;
    }
    let n_bones = bone_names.len();
    let radius = (0..3)
        .map(|c| mins[c].abs().max(maxs[c].abs()).powi(2))
        .sum::<f32>()
        .sqrt();
    Some(asset_t6::XModelRef {
        name: name.to_owned(),
        numsurfs: lod0.len().min(255) as u8,
        materials: Vec::new(),
        num_bones: n_bones as u8,
        num_lods: 1,
        mins,
        maxs,
        radius,
        lighting_origin_offset: [0.0; 3],
        lighting_origin_range: 0.0,
        lods: vec![(0.0, lod0.len() as u16, 0)],
        lod0,
        num_root_bones: 1,
        parent_list: (1..n_bones).map(|i| i as u8).collect(),
        quats: base_mat[1..]
            .iter()
            .map(|(q, _)| q.map(|x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16))
            .collect(),
        trans: base_mat[1..].iter().map(|(_, t)| *t).collect(),
        bone_names,
        part_classification: vec![0; n_bones],
        bone_info: vec![None; n_bones],
        base_mat,
        contents: 0,
        coll_lod: -1,
        coll_surfs: Vec::new(),
        phys_preset: None,
    })
}

/// bo2mp: the Modern Warfare III guest guns' models from what
/// `prefetch_jup_guest_guns` read, their materials linked into
/// `material_seed`.
pub(crate) fn build_jup_guest_guns(
    prefetch: JupPrefetch,
    material_seed: MaterialCatalog,
) -> JupGuests {
    let Some(files) = jup_source_files(&prefetch) else {
        return build_jup_guest_guns_now(prefetch, material_seed);
    };
    let caps = [("fp", FIRST_PERSON_MAX), ("world", WORLD_MAX)];
    let code = [
        include_str!("finished_gun_cache.rs"),
        include_str!("jup_gun.rs"),
        include_str!("../../../asset_game/src/weapon_camo_models.rs"),
    ];
    let names = vec!["guns".to_owned()];
    let src = super::finished_gun_cache::Sources {
        lane: "jup",
        name: "guns",
        files: &files,
        caps: &caps,
        code: &code,
        zones: &names,
        note: "",
        missing_ok: false,
    };
    let guests = super::finished_gun_cache::load_or_build(
        &src,
        material_seed,
        |seed| build_jup_guest_guns_now(prefetch, seed),
        pack_jup,
        unpack_jup,
    );
    reinstall_jup_patterns(&guests.material_seed);
    guests
}

fn jup_source_files(prefetch: &JupPrefetch) -> Option<Vec<PathBuf>> {
    let JupPrefetch::Ready(read) = prefetch else {
        return None;
    };
    let mut files = Vec::new();
    let dir = read.install.join("zone");
    if let Ok(found) = std::fs::read_dir(&dir) {
        for entry in found.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".ff") || name.ends_with(".pak") {
                files.push(entry.path());
            }
        }
    }
    (!files.is_empty()).then_some(files)
}

fn pack_jup(guests: &JupGuests, at: super::finished_gun_cache::MatAt) -> Result<Vec<u8>, String> {
    let weapons = asset_game::WeaponBuild::default();
    let projectiles = asset_model::ProjectileMeshBuild::default();
    let xanims = asset_anim::XAnimBuild::default();
    let tracers = asset_game::TracerCatalog::default();
    super::finished_gun_cache::pack_parts(&super::finished_gun_cache::Parts {
        weapons: &weapons,
        fpv: &guests.fpv,
        world: &guests.world_guns,
        projectiles: &projectiles,
        xanims: &xanims,
        fx: &guests.fx,
        fx_models: &guests.fx_models,
        tracers: &tracers,
        impact: None,
        materials: &guests.material_seed,
        mat_at: at,
        reports: &guests.report,
        extra: &[],
    })
}

fn unpack_jup(bytes: &[u8], seed: MaterialCatalog) -> Result<JupGuests, MaterialCatalog> {
    match super::finished_gun_cache::unpack_parts(bytes, seed) {
        Ok(rest) => Ok(JupGuests {
            fpv: rest.fpv,
            world_guns: rest.world,
            material_seed: rest.materials,
            fx: rest.fx,
            fx_models: rest.fx_models,
            report: rest.reports,
        }),
        Err(seed) => Err(seed),
    }
}

fn reinstall_jup_patterns(catalog: &MaterialCatalog) {
    let mut patterns = HashMap::new();
    for (first, list) in [
        (asset_core::weapon_camos::JUP_FIRST, &asset_game::weapon_camo_models::JUP_CAMOS[..]),
        (asset_core::weapon_camos::IW9_FIRST, &asset_game::weapon_camo_models::IW9_CAMOS[..]),
    ] {
        for (n, name) in list.iter().enumerate() {
            let image = format!("jup_camo:{name}");
            if let Some(slot) = catalog.images.iter().position(|img| img.name.as_str() == image) {
                patterns.insert(
                    first + n as u32,
                    asset_core::weapon_camos::PatternCamo::still(slot as u32, 2.0, true),
                );
            }
        }
    }
    asset_core::weapon_camos::install_jup_patterns(patterns);
}

fn build_jup_guest_guns_now(
    prefetch: JupPrefetch,
    material_seed: MaterialCatalog,
) -> JupGuests {
    let read = match prefetch {
        JupPrefetch::Off(report) => {
            return JupGuests {
                fpv: FpvMeshBuild::default(),
                world_guns: WorldWeaponBuild::default(),
                material_seed,
                fx: asset_game::FxCatalog::default(),
                fx_models: asset_game::FxModelCatalog::default(),
                report,
            };
        }
        JupPrefetch::Ready(r) => r,
    };
    let JupRead {
        install,
        guns,
        materials,
        camos,
        mut report,
    } = *read;
    let mut b = Builder {
        catalog: material_seed,
        owner: ZoneOwner::from_zone_path(&install.join("zone").join("global_stream_mp.ff")),
        images: HashMap::new(),
        materials: HashMap::new(),
        failures: Vec::new(),
    };
    let mut fpv = FpvMeshBuild::default();
    fpv.set_capture_ns(NS);
    let mut world_guns = WorldWeaponBuild::default();
    world_guns.set_capture_ns(NS);
    for (g, view, world, view_top) in &guns {
        let ViewFit {
            scale,
            offset: [ahead, _, lift],
        } = view_fit(g, view, *view_top);
        let r1 = |v: [f32; 3]| v.map(|c| (c * 10.0).round() / 10.0);
        report.push(format!(
            "jup guest guns: {}: view grip tag {:?}, hand anchor {:?}, receiver {:?}, top {:.1}; on {}: ahead {ahead:.1}, lift {lift:.1}",
            g.gun,
            grip_tag(view).map(r1),
            r1(hand_anchor(view)),
            receiver_bounds(view).map(|(lo, hi)| (r1(lo), r1(hi))),
            view_top,
            g.donor
        ));
        for (gun, name, max, first, offset, scale) in [
            (view, view_model(g.gun), FIRST_PERSON_MAX, true, [ahead, 0.0, lift], scale),
            (world, world_model(g.gun), WORLD_MAX, false, [0.0; 3], 1.0),
        ] {
            let Some(mut xref) = model_ref(&name, gun, offset, scale) else {
                report.push(format!("jup guest guns: {name}: no surfaces"));
                continue;
            };
            let linked: Vec<Option<usize>> = gun
                .surfaces
                .iter()
                .filter(|s| !s.positions.is_empty())
                .map(|s| b.material(s.material, max, &materials))
                .collect();
            let key = |k: usize| asset_t6::AssetKey {
                ty: fastfile_t6::AssetType::Material,
                index: k,
            };
            for (i, s) in xref.lod0.iter_mut().enumerate() {
                s.material = linked.get(i).copied().flatten().map(|_| key(i));
            }
            xref.materials = xref.lod0.iter().map(|s| s.material).collect();
            match asset_model::capture_model_skel_t6(&xref, |k| {
                linked.get(k.index).copied().flatten()
            }) {
                Some(skel) if first => fpv.insert_in(NS, skel, Some(&b.catalog)),
                Some(skel) => world_guns.insert_in(NS, skel, Some(&b.catalog)),
                None => report.push(format!("jup guest guns: {name}: model not built")),
            }
            report.push(format!(
                "jup guest guns: {name}: {} surfaces, bounds {:?}..{:?}, bones {:?}",
                xref.lod0.len(),
                xref.mins.map(|v| (v * 10.0).round() / 10.0),
                xref.maxs.map(|v| (v * 10.0).round() / 10.0),
                xref.bone_names
            ));
        }
    }
    // bo2mp: Modern Warfare III's and II's camos, each its picture laid
    // over a whole gun material, on every game's guns
    // (`asset_core::weapon_camos::JUP_FIRST`, `IW9_FIRST`).
    let mut patterns = HashMap::new();
    for (first, list) in [
        (asset_core::weapon_camos::JUP_FIRST, &asset_game::weapon_camo_models::JUP_CAMOS[..]),
        (asset_core::weapon_camos::IW9_FIRST, &asset_game::weapon_camo_models::IW9_CAMOS[..]),
    ] {
        for (n, name) in list.iter().enumerate() {
            let Some((w, h, rgba)) = camos.get(*name) else {
                continue;
            };
            let levels = ImageLevels {
                format: 29,
                width: *w,
                height: *h,
                levels: 1,
                data: rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
                part: None,
            };
            if let Some(slot) = b.image(format!("jup_camo:{name}"), TS_COLOR_MAP, || Some(levels)) {
                patterns.insert(
                    first + n as u32,
                    asset_core::weapon_camos::PatternCamo::still(slot as u32, 2.0, true),
                );
            }
        }
    }
    report.push(format!("jup camos: {} drawn on every gun", patterns.len()));
    asset_core::weapon_camos::install_jup_patterns(patterns);
    if !b.failures.is_empty() {
        report.push(format!(
            "jup guest guns: pictures not built: {:?}",
            &b.failures[..b.failures.len().min(6)]
        ));
    }
    let Builder {
        catalog: material_seed,
        ..
    } = b;
    // bo2mp: the effects their stand-ins play, read from the games they
    // come from (`super::fx_standin`).
    let standin = super::fx_standin::build_standin_fx(material_seed);
    report.extend(standin.report);
    JupGuests {
        fpv,
        world_guns,
        material_seed: standin.material_seed,
        fx: standin.fx,
        fx_models: standin.fx_models,
        report,
    }
}

pub use export::JupGunExport;
pub(crate) use export::{check as check_jup_export, png as png_rgba};

/// bo2mp `CodLegacy export-jup`: the Modern Warfare III guest guns read from the
/// install as a match reads them (`prefetch`), and written to `dest` in our
/// own files (`jup_export`), to be read back without the game. `say` hears
/// how far it is.
pub(crate) fn export_jup_guns(dest: &std::path::Path, say: &mut dyn FnMut(&str)) -> Result<JupGunExport, String> {
    if asset_audio::jup_install().is_none() {
        return Err("Modern Warfare III's folder is not set (game_paths.cfg's jup line, or BO2_JUP_INSTALL)".to_owned());
    }
    say("reading every Modern Warfare III gun as a match does (a few minutes)...");
    match prefetch(true, false) {
        JupPrefetch::Off(report) => Err(report.join("; ")),
        JupPrefetch::Ready(read) => {
            for line in read.report.iter().filter(|l| l.contains("not placed") || l.contains("not in") || l.contains(": no ") || l.contains("read in")) {
                say(line);
            }
            export::write(&read, dest, say)
        }
    }
}

/// bo2mp `CodLegacy check-guns`: the Modern Warfare III guest guns read and
/// built as a match does (their weapons are copies made in the match:
/// the donor each is made from is named).
pub(crate) fn jup_check() -> Vec<String> {
    let guests = build_jup_guest_guns(prefetch_jup_guest_guns(true), MaterialCatalog::default());
    let mut out: Vec<String> = guests.report.iter().map(|l| format!("  {l}")).collect();
    for g in all_guns() {
        let view = guests.fpv.index_by_name(NS, &view_model(g.gun)).is_some();
        let world = guests
            .world_guns
            .index_by_name(NS, &world_model(g.gun))
            .is_some();
        out.push(format!(
            "jup_{:<20} {} first person {}, world {}, a copy of {} with its handling (fire type {}, {} rounds)",
            g.gun,
            if view && world { "OK" } else { "MISSING" },
            if view { "built" } else { "missing" },
            if world { "built" } else { "missing" },
            g.donor,
            g.fire_type,
            g.clip_size
        ));
    }
    out
}

/// A gun's own sounds (`asset_audio::JUP_GUN_SOUNDS`, those the install
/// has) in its donor's slots, and its donor's notetrack sounds replaced, as
/// `jup:#<hash>`.
fn guest_sounds(
    gun: &str,
) -> (
    Vec<(asset_game::WeaponSoundSlot, String)>,
    Vec<(String, String)>,
) {
    use asset_game::WeaponSoundSlot as S;
    let Some(s) = asset_audio::JUP_GUN_SOUNDS.iter().find(|s| s.gun == gun) else {
        return Default::default();
    };
    // (A sound the install lacks keeps the donor's.)
    let has = |a: &str| !a.is_empty() && asset_audio::jup_gun_has(a);
    let key = |a: &str| {
        if a.is_empty() {
            String::new()
        } else {
            format!("jup:{}", asset_audio::jup_alias_key(a))
        }
    };
    let slots = [
        (S::Fire, s.fire),
        (S::FirePlayer, s.fire_player),
        // (The donor's last-shot sounds would play otherwise.)
        (S::FireLast, s.fire),
        (
            S::FireLastPlayer,
            if has(s.fire_last_player) {
                s.fire_last_player
            } else {
                s.fire_player
            },
        ),
        (S::RaisePlayer, s.raise_player),
        (S::FirstRaisePlayer, s.first_raise_player),
        (S::PutawayPlayer, s.putaway_player),
        (S::Pickup, s.pickup),
        (S::PickupPlayer, s.pickup_player),
        (S::AdsRaisePlayer, s.ads_up),
        (S::AdsLowerPlayer, s.ads_down),
    ]
    .into_iter()
    .filter(|(_, a)| has(a))
    .map(|(slot, a)| (slot, key(a)))
    .collect();
    let notes = s
        .notes
        .iter()
        .filter(|(_, a)| has(a))
        .map(|(word, a)| ((*word).to_owned(), key(a)))
        .collect();
    (slots, notes)
}

/// bo2mp: the Modern Warfare III guest guns as weapons: each a copy of its
/// Black Ops II donor (animations, arms, effects, sounds, damage) with its
/// own models, name and handling. Before the guest names are given and
/// anything resolves against the catalog.
pub(crate) fn jup_guest_rows(weapons: &mut asset_game::WeaponBuild, report: &mut Vec<String>) {
    for g in all_guns() {
        let Some(donor) = weapons
            .resolve_index(g.donor)
            .ok()
            .flatten()
            .filter(|&i| i != 0)
        else {
            report.push(format!("jup guest guns: {}: no {} to copy", g.gun, g.donor));
            continue;
        };
        let (sounds, notetrack_remap) = guest_sounds(g.gun);
        let id = weapons.add_guest_copy(
            donor,
            asset_game::GuestCopy {
                namespace: NS,
                name: format!("{}_mp", g.gun),
                display_name_key: format!("WEAPON_JUP_{}", g.gun.to_ascii_uppercase()),
                gun_xmodel: view_model(g.gun),
                world_model: world_model(g.gun),
                fire_type: g.fire_type,
                fire_time_ms: g.fire_time_ms,
                clip_size: g.clip_size,
                max_ammo: g.max_ammo,
                sounds,
                notetrack_remap,
            },
        );
        // Its attachments (Create-a-Class's, Black Ops II's words): a sight
        // is its donor's sight variant on it; the others Black Ops II's
        // numbers.
        let Some(listed) = bo2_profile::guest::JUP.iter().find(|r| r.gun == g.gun) else {
            continue;
        };
        // (Its flash, brass, impacts and explosion: Modern Warfare
        // Remastered's and Black Ops III's, over its donor's.)
        let effects = super::fx_standin::guest_effects(g.gun, listed.group);
        if !effects.is_empty() {
            weapons.set_guest_effects(id, effects);
        }
        let class = super::guest_attach::GunClass::from_group(listed.group);
        let donor = g.donor.strip_suffix("_mp").unwrap_or(g.donor);
        let offered: Vec<asset_game::GuestAttachment> = listed
            .attachments
            .split_whitespace()
            .filter_map(|word| {
                let bo2 = super::guest_attach::bo2_attachment(word, class).to_owned();
                if !asset_game::T6_SIGHT_ATTACHMENTS.contains(&word) {
                    return Some(asset_game::GuestAttachment {
                        word: word.to_owned(),
                        bo2: Some(bo2),
                        sounds_from: (word == "silencer").then(|| format!("{donor}_silencer")),
                        ..Default::default()
                    });
                }
                let copy = format!("{donor}_mp+{word}");
                weapons.resolve_index(&copy).ok().flatten().filter(|&i| i != 0)?;
                Some(asset_game::GuestAttachment {
                    word: word.to_owned(),
                    sight: true,
                    sight_from: Some(copy.clone()),
                    copy_of: Some(copy),
                    ..Default::default()
                })
            })
            .collect();
        report.push(format!(
            "jup guest guns: {}_mp from {} (row {id}); attachments: {}",
            g.gun,
            g.donor,
            super::guest_attach::words(&offered)
        ));
        weapons.offer_guest_attachments(id, offered);
    }
    report.extend(super::fx_standin::report_lines());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reflectance the lit draw reads from a specular byte
    /// (`raw_texel(v)^2`).
    fn read_back(b: u8) -> f32 {
        let s = srgb(f32::from(b) / 255.0);
        (f32::from(s) / 255.0).powi(2)
    }

    #[test]
    fn specular_bytes_read_back_as_their_reflectance() {
        // A dielectric's byte is the one the other guest lanes use (9).
        assert!(spec_byte(0.04).abs_diff(9) <= 1);
        for f0 in [0.04, 0.10, 0.20] {
            assert!((read_back(spec_byte(f0)) - f0).abs() < 0.01, "{f0}");
        }
    }

    fn material(mask: [u8; 4], normal: [u8; 4]) -> MaterialPixels {
        MaterialPixels {
            mask: Some((1, 1, mask.to_vec())),
            normal: Some((1, 1, normal.to_vec())),
            gun: true,
            ..MaterialPixels::default()
        }
    }

    #[test]
    fn no_gun_surface_is_a_mirror() {
        // A glossy packed normal map (red 255) and no occlusion (blue 255).
        let normal = [255, 128, 255, 128];
        for (mask, f) in [
            ([0, 255, 0, 255], Finish::Metal),
            ([255, 0, 255, 0], Finish::Polymer),
            ([255, 255, 255, 0], Finish::Trim),
        ] {
            assert_eq!(Finish::of(Some(mask)), f);
            let s = specular_levels(&material(mask, normal)).unwrap();
            assert!(read_back(s.data[0]) <= 0.21, "{f:?} reflects {}", read_back(s.data[0]));
            assert!(s.data[3] <= 180, "{f:?} gloss {}", s.data[3]);
        }
        // Polymer is a dielectric; without a mask nothing is metal.
        let poly = specular_levels(&material([255, 0, 255, 0], normal)).unwrap();
        assert!((read_back(poly.data[0]) - 0.04).abs() < 0.01);
        assert_eq!(Finish::of(None), Finish::Plain);
        assert!(Finish::Plain.reflectance() < Finish::Metal.reflectance());
    }

    #[test]
    fn a_gun_s_colour_follows_its_mask() {
        let c = gun_colour(&material([255, 0, 255, 0], [128, 128, 255, 128])).unwrap();
        assert_eq!(&c.data[..3], &Finish::Polymer.albedo());
    }

    fn surface(points: &[[f32; 3]]) -> gun::PartSurface {
        gun::PartSurface {
            positions: points.to_vec(),
            ..gun::PartSurface::default()
        }
    }

    #[test]
    fn the_hand_holds_the_grip_tag_on_the_receiver_s_centre_line() {
        let grip = gun::Bone {
            name: gun::bone_hash("tag_pistolgrip_attach"),
            rot: [0.0, 0.0, 0.0, 1.0],
            pos: [3.0, 0.4, -1.0],
        };
        let g = Gun {
            surfaces: vec![
                surface(&[[-5.0, -1.0, -2.0], [10.0, 1.0, 2.0]]),
                // (A bolt handle off to the right is not the centre line.)
                surface(&[[2.0, -4.0, 0.0]]),
            ],
            bones: vec![grip],
            missing: Vec::new(),
            receiver_surfaces: 1,
        };
        assert_eq!(grip_tag(&g), Some([3.0, 0.4, -1.0]));
        assert_eq!(hand_anchor(&g), [3.0, 0.0, -1.0]);
    }
}

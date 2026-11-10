use std::path::Path;
use std::sync::Arc;

use fastfile_iw4::{
    AssetLinkSink, AssetSink, AssetType, Ptr, ScriptStrings, ZoneStream, load_asset_at_observed,
    load_zone,
};

use crate::discover::{GamesRoot, find_runtime_zone, find_zone_file, find_zone_file_version};
use asset_transport::discover::find_t5_localized_zone;
use crate::sound_catalog::SoundCatalog;
use crate::zone::{ZoneMemory, open_zone_shared};
use crate::zone_sound::{ZoneSoundOrigin, ensure_zone_sound};
use crate::{AssetNamespace, ZoneGame};

pub fn namespace_for_zone(games: &GamesRoot, zone: &str) -> AssetNamespace {
    find_zone_file(games, zone)
        .ok()
        .and_then(|found| crate::zone_game_for_path(&found.path))
        .map(AssetNamespace::from_zone_game)
        .unwrap_or(AssetNamespace::Iw4)
}

struct SoundSink {
    catalog: SoundCatalog,
    walked: usize,
    stopped_at: Option<(usize, &'static str)>,
}

impl AssetSink for SoundSink {
    fn set_script_strings(&mut self, _strings: ScriptStrings) {}

    fn load_asset(
        &mut self,
        s: &mut ZoneStream<'_>,
        index: usize,
        ty: AssetType,
        slot: Ptr,
    ) -> fastfile_iw4::Result<()> {
        self.stopped_at = Some((index, ty.name()));
        load_asset_at_observed(s, ty, slot, self)?;
        self.walked += 1;
        self.stopped_at = None;
        Ok(())
    }
}

impl AssetLinkSink for SoundSink {
    fn loaded(
        &mut self,
        s: &ZoneStream<'_>,
        ty: AssetType,
        slot: Ptr,
        insert_slot: Option<Ptr>,
    ) -> fastfile_iw4::Result<()> {
        self.catalog.loaded(s, ty, slot, insert_slot)
    }

    fn alias(&mut self, ty: AssetType, slot: Ptr, target: Ptr) -> fastfile_iw4::Result<()> {
        self.catalog.alias(ty, slot, target)
    }

    fn capture_loaded_sound(
        &mut self,
        s: &ZoneStream<'_>,
        header: Ptr,
        pcm: Ptr,
        data_len: usize,
    ) -> fastfile_iw4::Result<()> {
        self.catalog.capture_loaded_sound(s, header, pcm, data_len)
    }

    fn bind_last_loaded_to_sound_file(&mut self, file: Ptr) -> fastfile_iw4::Result<()> {
        self.catalog.bind_last_loaded_to_sound_file(file)
    }

    fn bind_streamed_sound_file(
        &mut self,
        file: Ptr,
        dir: &str,
        name: &str,
    ) -> fastfile_iw4::Result<()> {
        self.catalog.bind_streamed_sound_file(file, dir, name)
    }

    fn capture_raw_file(
        &mut self,
        name: &str,
        data: &[u8],
        zlib_compressed: bool,
    ) -> fastfile_iw4::Result<()> {
        self.catalog.ingest_rawfile(name, data, zlib_compressed);
        Ok(())
    }

    fn capture_sound(
        &mut self,
        s: &ZoneStream<'_>,
        list: Ptr,
        count: usize,
        head: Option<Ptr>,
    ) -> fastfile_iw4::Result<()> {
        self.catalog.capture_sound(s, list, count, head)
    }

    fn capture_snd_curve(&mut self, s: &ZoneStream<'_>, header: Ptr) -> fastfile_iw4::Result<()> {
        self.catalog.capture_snd_curve(s, header)
    }
}

pub fn load_sound_catalog(path: &Path) -> Result<SoundCatalog, String> {
    let image = open_zone_shared(path).map_err(|e| e.to_string())?;
    let header = image.header().map_err(|e| e.to_string())?;
    let mut memory = ZoneMemory::for_header(&header);
    let mut stream = memory.stream(&image.bytes).map_err(|e| e.to_string())?;
    let mut sink = SoundSink {
        catalog: SoundCatalog::default(),
        walked: 0,
        stopped_at: None,
    };
    sink.catalog
        .set_capture_zone(crate::ZoneOwner::from_zone_path(path));
    sink.catalog.set_capture_game(crate::ZoneGame::Iw4);
    load_zone(&mut stream, &mut sink).map_err(|e| {
        format!(
            "sound catalog failed after {} assets at {:?}: {e}",
            sink.walked, sink.stopped_at
        )
    })?;
    sink.catalog.resolve_curve_knots();
    sink.catalog.resolve_ent_channels();
    sink.catalog.publish();
    Ok(sink.catalog)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundZoneGap {
    pub namespace: AssetNamespace,

    pub zone: String,
    pub reason: String,
}

#[derive(Default)]
pub struct LoadedSoundBank {
    pub catalog: SoundCatalog,
    pub gaps: Vec<SoundZoneGap>,
}

fn namespace_of_game(game: ZoneGame) -> AssetNamespace {
    AssetNamespace::from_zone_game(game)
}

pub(crate) fn walk_zone_sound(path: &Path) -> Result<SoundCatalog, String> {
    let game = crate::zone_game_for_path(path)
        .ok_or_else(|| format!("{} is not a readable IWff envelope", path.display()))?;
    match game {
        ZoneGame::Iw4 => load_sound_catalog(path),
        ZoneGame::Iw5 => crate::sound_load_iw5::load_sound_catalog_iw5(path),
        ZoneGame::T5 => crate::sound_load_t5::load_sound_catalog_t5(path),
        ZoneGame::T6 => Err(format!(
            "{}: Black Ops II sound banks are not read yet (bo2zm)",
            path.display()
        )),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Merge {
    Override,
    Missing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Absent {
    Skip,
    Gap,
}

struct SoundSource {
    namespace: AssetNamespace,
    name: &'static str,
    found: Result<std::path::PathBuf, String>,
    merge: Merge,
    absent: Absent,
}

fn sound_sources(games: &GamesRoot, map: &Path) -> (Vec<SoundSource>, Vec<SoundSource>) {
    let runtime = |name: &'static str, merge, absent| SoundSource {
        namespace: AssetNamespace::Iw4,
        name,
        found: find_runtime_zone(games, map, name).map(|zone| zone.path),
        merge,
        absent,
    };
    let donor = |namespace, name: &'static str, version| SoundSource {
        namespace,
        name,
        found: find_zone_file_version(games, name, version).map(|zone| zone.path),
        merge: Merge::Missing,
        absent: Absent::Skip,
    };
    let before_map = vec![
        runtime("code_post_gfx_mp", Merge::Override, Absent::Skip),
        runtime("localized_code_post_gfx_mp", Merge::Override, Absent::Skip),
        runtime("patch_mp", Merge::Override, Absent::Skip),
        runtime("common_mp", Merge::Override, Absent::Gap),
        runtime("localized_common_mp", Merge::Override, Absent::Gap),
    ];
    // bo2mp: MW2's guns as extra weapons in a Black Ops II match: its
    // common sounds too, when the map did not already bring MW2's common.
    let runtime_common = before_map
        .iter()
        .find(|s| s.name == "common_mp")
        .and_then(|s| s.found.as_ref().ok().cloned());
    let mw2_guest = |name: &'static str| {
        let found = find_zone_file_version(games, name, fastfile_iw4::ZONE_VERSION_PC).map(|zone| zone.path);
        let already = found.as_ref().ok().zip(runtime_common.as_ref()).is_some_and(|(f, r)| f == r)
            || (name == "localized_common_mp" && runtime_common.as_ref().is_some_and(|r| {
                found.as_ref().is_ok_and(|f| f.parent() == r.parent())
            }));
        SoundSource {
            namespace: AssetNamespace::Iw4,
            name,
            found: if already { Err("MW2's common sounds are already the runtime's".to_owned()) } else { found },
            merge: Merge::Missing,
            absent: Absent::Skip,
        }
    };
    // bo2mp: on a Black Ops II map, another game's gun sounds are read only
    // when one of his classes holds that game's guns (walked for every
    // installed game, they kept a Takeoff load on the HDD for minutes).
    let bo2_map = crate::zone_game_for_path(map) == Some(ZoneGame::T6);
    let guest = |game: &str| !bo2_map || bo2_profile::guest::match_games() & bo2_profile::guest::game_bit(game) != 0;
    let mut after_map = Vec::new();
    if guest("iw4") {
        after_map.extend([mw2_guest("common_mp"), mw2_guest("localized_common_mp")]);
    }
    if guest("iw5") {
        after_map.extend([
            donor(AssetNamespace::Iw5, "common_mp", fastfile_iw5::ZONE_VERSION_PC),
            donor(AssetNamespace::Iw5, "localized_common_mp", fastfile_iw5::ZONE_VERSION_PC),
        ]);
    }
    if guest("t5") {
        after_map.extend([
            donor(AssetNamespace::T5, "code_post_gfx_mp", fastfile_t5::ZONE_VERSION_PC),
            donor(AssetNamespace::T5, "common_mp", fastfile_t5::ZONE_VERSION_PC),
            donor(AssetNamespace::T5, "localized_common_mp", fastfile_t5::ZONE_VERSION_PC),
        ]);
    }
    after_map.extend([
        bo1_announcer_source(games),
        mw3_voice_source("localized_common_mp", "localized_common_mp (MW3 announcer)"),
        mw3_voice_source("localized_code_post_gfx_mp", "localized_code_post_gfx_mp (MW3 UI sounds)"),
    ]);
    // bo2mp: Ghosts' guns as extra weapons: their sounds, from its zones
    // (read by `fastfile_iw6`, kept under the Ghosts namespace).
    if std::env::var("BO2MP_GUEST_GUNS").map_or(true, |v| v.trim() != "0") {
        if guest("iw6") {
            after_map.extend(ghosts_sound_sources(games));
        }
        // bo2mp: World at War's guns: their sounds, from its common zone
        // (read by `t4`, kept under the World at War namespace). Nothing
        // when World at War is not installed.
        if guest("t4") {
            after_map.extend(waw_sound_source(games));
        }
        // bo2mp: Advanced Warfare's guns: their sounds, from its common_mp
        // (read by `fastfile_s1`, kept under the AW namespace).
        if guest("s1") {
            after_map.extend(aw_sound_source(games));
        }
        // bo2mp: Horizon MW's guns: their sounds, from its weapon zones
        // (read by `fastfile_h1`, kept under the H1 namespace).
        if guest("h1") {
            after_map.extend(hmw_sound_sources());
            after_map.extend(hmw_killstreak_sources());
        }
        // bo2mp: Black Ops III's guns: their sounds, from its mp_common and
        // its sound banks (read by `fastfile_t7`, kept under the BO3
        // namespace).
        let bo3 = ["t7", "t7cw", "t7vg", "t7mw", "t7cod2", "t7ol", "t7ol2"].iter().any(|g| guest(g));
        if bo3 {
            after_map.extend(bo3_sound_source(games));
        }
        // bo2mp: the Black Ops III Workshop weapon packs' guns: their own
        // sounds, from the packs' sound banks (BO3 namespace too).
        if bo3 && bo3_sound_source(games).is_some() {
            after_map.extend(bo3_pack_sound_sources());
        }
        // bo2mp: Modern Warfare III's (2023) guns: their sounds, from its
        // `ingame` (read by `fastfile_jup`, kept under the jup namespace).
        if guest("jup") {
            after_map.extend(jup_gun_sound_source());
        }
        // bo2mp: Infinite Warfare's guns: their sounds, from its zones'
        // sound banks (`sound_load_iw7`, kept under the IW7 namespace).
        if guest("iw7") {
            after_map.extend(iw7_gun_sound_source());
        }
    }
    // bo2mp: Resurgence's sounds from Modern Warfare III (2023), when the
    // match is Resurgence and the install is set.
    after_map.extend(jup_sound_source());
    // bo2mp: a Modern Warfare III map's own sounds (its ambience beds).
    after_map.extend(jup_map_sound_source());
    (before_map, after_map)
}

/// bo2mp: Infinite Warfare's zones, its guns' sounds, when they are
/// there.
fn iw7_gun_sound_source() -> Option<SoundSource> {
    Some(SoundSource {
        namespace: AssetNamespace::Iw7,
        name: "common_mp sound banks (IW7 guns)",
        found: Ok(crate::iw7_zone_dir()?),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: the name of the source of Modern Warfare III's (2023) gun sounds.
const JUP_GUN_SOURCE: &str = "ingame (MW3 2023 guns)";

/// bo2mp: Modern Warfare III's (2023) install, for its guns' sounds, when
/// it is set.
fn jup_gun_sound_source() -> Option<SoundSource> {
    Some(SoundSource {
        namespace: AssetNamespace::Jup,
        name: JUP_GUN_SOURCE,
        // (The install, else the folder the guns were exported to.)
        found: Ok(crate::jup_gun_source()?),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: the name of the source of a Modern Warfare III map's sounds.
const JUP_MAP_SOURCE: &str = "map zone (MW3 2023 ambience)";

/// bo2mp: Modern Warfare III's (2023) install, for the map being played's
/// own sounds, when it is one of the maps with ambience beds.
fn jup_map_sound_source() -> Option<SoundSource> {
    crate::jup_map_wanted()?;
    Some(SoundSource {
        namespace: AssetNamespace::Jup,
        name: JUP_MAP_SOURCE,
        found: crate::jup_install()
            .ok_or_else(|| format!("{} is not set to a Modern Warfare III (2023) install", crate::JUP_INSTALL_ENV)),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: Modern Warfare III's (2023) install, for Resurgence's sounds.
fn jup_sound_source() -> Option<SoundSource> {
    crate::jup_sounds_wanted()?;
    Some(SoundSource {
        namespace: AssetNamespace::Jup,
        name: "Resurgence (MW3 2023)",
        found: crate::jup_install().ok_or_else(|| {
            format!("{} is not set to a Modern Warfare III (2023) install", crate::JUP_INSTALL_ENV)
        }),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: World at War's common zone, its guns' sounds, when it is
/// installed.
fn waw_sound_source(games: &GamesRoot) -> Option<SoundSource> {
    Some(SoundSource {
        namespace: AssetNamespace::T4,
        name: "common_mp (World at War guns)",
        found: Ok(crate::waw_common_mp(games)?),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: Advanced Warfare's common_mp, its guns' sounds, when AW is
/// installed.
fn aw_sound_source(games: &GamesRoot) -> Option<SoundSource> {
    let path = crate::aw_install(Some(games.0.as_path()))?.join("zone").join("common_mp.ff");
    Some(SoundSource {
        namespace: AssetNamespace::S1,
        name: "common_mp (AW guns)",
        found: Ok(path),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: Horizon MW's weapon zones, its guns' sounds, when they are
/// there.
fn hmw_sound_sources() -> Vec<SoundSource> {
    let Some(dir) = crate::hmw_zone_dir() else {
        return Vec::new();
    };
    crate::hmw_gun_zones(&dir)
        .into_iter()
        .map(|path| {
            let name: &'static str = match path.file_stem().and_then(|s| s.to_str()) {
                Some("hmw_ar1") => "hmw_ar1 (Horizon MW guns)",
                Some("hmw_smg") => "hmw_smg (Horizon MW guns)",
                _ => "hmw_shotgun (Horizon MW guns)",
            };
            SoundSource {
                namespace: AssetNamespace::H1,
                name,
                found: Ok(path),
                merge: Merge::Missing,
                absent: Absent::Skip,
            }
        })
        .collect()
}

/// bo2mp: Horizon MW's killstreak zones, the Tactical Nuke's announcer lines
/// (Modern Warfare 2 Remastered's), when they are there.
fn hmw_killstreak_sources() -> Vec<SoundSource> {
    let Some(dir) = crate::hmw_zone_dir() else {
        return Vec::new();
    };
    crate::hmw_killstreak_zones(&dir)
        .into_iter()
        .map(|path| SoundSource {
            namespace: AssetNamespace::H1,
            name: KILLSTREAK_SOURCE,
            found: Ok(path),
            merge: Merge::Missing,
            absent: Absent::Skip,
        })
        .collect()
}

/// The name of a Horizon MW killstreak zone's sound source.
const KILLSTREAK_SOURCE: &str = "hmw killstreak zones (Tactical Nuke announcer)";

/// bo2mp: Black Ops III's mp_common, its guns' sounds, when it is
/// installed.
fn bo3_sound_source(games: &GamesRoot) -> Option<SoundSource> {
    Some(SoundSource {
        namespace: AssetNamespace::T7,
        name: "mp_common (BO3 guns)",
        found: Ok(crate::bo3_mp_common(games)?),
        merge: Merge::Missing,
        absent: Absent::Skip,
    })
}

/// bo2mp: the Black Ops III Workshop weapon packs' zones with their guns'
/// sound aliases (`bo2_profile::guest::WORKSHOP_PACKS`, those installed):
/// a pack's zone whose bank is beside it (`snd/all/<zone>.all.sabl`), else
/// its every zone (Modern Warfare 2019's bank is `zm_weapon2019.all`). Off
/// with `BO2MP_WORKSHOP_GUNS=0`.
fn bo3_pack_sound_sources() -> Vec<SoundSource> {
    if std::env::var("BO2MP_WORKSHOP_GUNS").is_ok_and(|v| v.trim() == "0") {
        return Vec::new();
    }
    bo3_pack_sound_zones()
        .into_iter()
        .map(|(name, zone)| SoundSource { namespace: AssetNamespace::T7, name, found: Ok(zone), merge: Merge::Missing, absent: Absent::Skip })
        .collect()
}

/// bo2mp: the installed Workshop weapon packs' zones with their sound
/// aliases, each with its pack's tab (`bo3_pack_sound_sources`).
pub fn bo3_pack_sound_zones() -> Vec<(&'static str, std::path::PathBuf)> {
    let mut out = Vec::new();
    for pack in &bo2_profile::guest::WORKSHOP_PACKS {
        let Some(m) = asset_transport::workshop::bo3_mod(pack.workshop_id) else { continue };
        let banked = |z: &&std::path::PathBuf| {
            let stem = z.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            m.dir.join("snd").join("all").join(format!("{stem}.all.sabl")).is_file()
        };
        let mut zones: Vec<&std::path::PathBuf> = m.zones.iter().filter(banked).collect();
        if zones.is_empty() {
            zones = m.zones.iter().collect();
        }
        out.extend(zones.into_iter().map(|z| (pack.tab, z.clone())));
    }
    out
}

/// bo2mp: the Ghosts zones its guns' sounds come from, when Ghosts is
/// installed.
fn ghosts_sound_sources(games: &GamesRoot) -> Vec<SoundSource> {
    let Some(install) = crate::ghosts_install(Some(games.0.as_path())) else {
        return Vec::new();
    };
    crate::ghosts_sound_zones(&install)
        .into_iter()
        .zip(["common_mp (Ghosts guns)", "common_core_mp (Ghosts guns)"])
        .map(|(path, name)| SoundSource {
            namespace: AssetNamespace::Iw6,
            name,
            found: Ok(path),
            merge: Merge::Missing,
            absent: Absent::Skip,
        })
        .collect()
}

/// BO1's multiplayer announcer (`vox_ops_*`, `vox_rus_*`) is carried by its
/// maps' language zones, not by `common_mp`. Nuketown's holds both the Black
/// Ops and Spetsnaz voices, which the BO1 presentation speaks with.
pub const BO1_ANNOUNCER_ZONE: &str = "mp_nuked";

/// `CODLEGACY_VOICE_GAMES`: a folder of games lent for their announcer only (MW3,
/// for lines BO1 never had). Nothing else is read from it, so a match does
/// not take in that game's weapons or materials.
pub fn voice_games_root() -> Option<GamesRoot> {
    codlegacy_env::var_os("CODLEGACY_VOICE_GAMES")
        .filter(|root| !root.is_empty())
        .map(|root| GamesRoot(std::path::PathBuf::from(root)))
}

/// An MW3 zone from the voice-only folder: its announcer
/// (`localized_common_mp`) or its interface sounds
/// (`localized_code_post_gfx_mp`).
fn mw3_voice_source(zone: &'static str, name: &'static str) -> SoundSource {
    let found = voice_games_root()
        .ok_or_else(|| "CODLEGACY_VOICE_GAMES is not set".to_owned())
        .and_then(|root| find_zone_file_version(&root, zone, fastfile_iw5::ZONE_VERSION_PC))
        .map(|zone| zone.path);
    SoundSource {
        namespace: AssetNamespace::Iw5,
        name,
        found,
        merge: Merge::Missing,
        absent: Absent::Skip,
    }
}

fn bo1_announcer_source(games: &GamesRoot) -> SoundSource {
    let found = find_zone_file_version(games, "common_mp", fastfile_t5::ZONE_VERSION_PC)
        .and_then(|common| find_t5_localized_zone(&common.path, Some("english"), BO1_ANNOUNCER_ZONE))
        .and_then(|zone| {
            zone.map(|zone| zone.path)
                .ok_or_else(|| format!("no language zone for {BO1_ANNOUNCER_ZONE}"))
        });
    SoundSource {
        namespace: AssetNamespace::T5,
        name: "localized_mp_nuked",
        found,
        merge: Merge::Missing,
        absent: Absent::Skip,
    }
}

pub struct SoundSources {
    before_map: SoundCatalog,
    after_map: Vec<(SoundSource, Arc<SoundCatalog>)>,
    gaps: Vec<SoundZoneGap>,
}

fn origin_label(origin: ZoneSoundOrigin) -> String {
    match origin {
        ZoneSoundOrigin::Shared(walk) => format!("{walk} walk"),
        ZoneSoundOrigin::AudioOnly => "audio-only walk".to_owned(),
    }
}

pub fn gather_sound_sources(games: &GamesRoot, map: &Path) -> SoundSources {
    let (before, after) = sound_sources(games, map);
    let mut sources = SoundSources {
        before_map: SoundCatalog::default(),
        after_map: Vec::new(),
        gaps: Vec::new(),
    };
    for source in before {
        if let Some(catalog) = sources.take(&source) {
            sources.before_map.absorb_unresolved((*catalog).clone());
        }
    }
    for source in after {
        if let Some(catalog) = sources.take(&source) {
            sources.after_map.push((source, catalog));
        }
    }
    sources
}

impl SoundSources {
    fn take(&mut self, source: &SoundSource) -> Option<Arc<SoundCatalog>> {
        let path = match &source.found {
            Ok(path) => path,
            Err(error) => {
                match source.absent {
                    Absent::Skip => diag::info!(
                        Zone,
                        "sound bank: {} {} missing ({error})",
                        source.namespace.as_str(),
                        source.name
                    ),
                    Absent::Gap => self.gap(source.namespace, source.name, error.clone()),
                }
                return None;
            }
        };
        // bo2mp: Ghosts zones are read by `fastfile_iw6`, World at War's by
        // `t4` (neither has a `ZoneGame`).
        let (catalog, origin) = match source.namespace {
            AssetNamespace::Iw6 => (crate::ghosts_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::T4 => (crate::waw_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::S1 => (crate::aw_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::H1 if source.name == KILLSTREAK_SOURCE => (crate::hmw_killstreak_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::H1 => (crate::hmw_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::T7 => (crate::bo3_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::Iw7 => (crate::iw7_weapon_sounds(path), ZoneSoundOrigin::AudioOnly),
            AssetNamespace::Jup if source.name == JUP_GUN_SOURCE => {
                (crate::jup_gun_sounds(path), ZoneSoundOrigin::AudioOnly)
            }
            AssetNamespace::Jup if source.name == JUP_MAP_SOURCE => (
                crate::jup_map_wanted()
                    .ok_or_else(|| "no MW3 map wanted".to_owned())
                    .and_then(|zone| crate::jup_map_sounds(path, &zone)),
                ZoneSoundOrigin::AudioOnly,
            ),
            AssetNamespace::Jup => (
                crate::jup_resurgence_sounds(path, crate::jup_sounds_wanted().unwrap_or(false)),
                ZoneSoundOrigin::AudioOnly,
            ),
            _ => ensure_zone_sound(path),
        };
        match catalog {
            Ok(catalog) => {
                diag::info!(
                    Zone,
                    "sound bank: {} {} from the {} — {} aliases {} loaded",
                    source.namespace.as_str(),
                    source.name,
                    origin_label(origin),
                    catalog.sounds.len(),
                    catalog.loaded.len()
                );
                Some(catalog)
            }
            Err(error) => {
                self.gap(source.namespace, source.name, error);
                None
            }
        }
    }

    fn gap(&mut self, namespace: AssetNamespace, zone: &str, reason: String) {
        self.gaps.push(SoundZoneGap {
            namespace,
            zone: zone.to_owned(),
            reason,
        });
    }
}

pub fn compose_sound_bank(
    sources: SoundSources,
    zone: &str,
    namespace: AssetNamespace,
    map: Result<SoundCatalog, String>,
) -> LoadedSoundBank {
    let SoundSources {
        before_map,
        after_map,
        gaps,
    } = sources;
    let started = std::time::Instant::now();
    let mut bank = LoadedSoundBank {
        catalog: before_map,
        gaps,
    };
    match map {
        Ok(map) => {
            diag::info!(
                Zone,
                "sound bank: map zone {zone} ({}) — {} rawfiles, {} aliases",
                namespace.as_str(),
                map.rawfiles.len(),
                map.sounds.len()
            );
            match namespace {
                AssetNamespace::Iw4 => bank.catalog.absorb_unresolved(map),
                _ => bank.catalog.absorb_missing_aliases_unresolved(map),
            }
        }
        Err(error) => bank.gap(namespace, zone, error),
    }
    for (source, catalog) in after_map {
        let catalog = (*catalog).clone();
        match source.merge {
            Merge::Override => bank.catalog.absorb_unresolved(catalog),
            Merge::Missing => bank.catalog.absorb_missing_aliases_unresolved(catalog),
        }
    }
    let merged = started.elapsed().as_secs_f32();
    bank.catalog.resolve_curve_knots();
    let filled = bank.catalog.fill_guest_falloff();
    if filled > 0 {
        diag::info!(
            Zone,
            "sound bank: {filled} guest aliases without a falloff curve take the default one"
        );
    }
    let before_finalize = started.elapsed().as_secs_f32();
    bank.catalog.finalize();
    // bo2mp: each step's time, so a slow compose names itself.
    diag::info!(
        Zone,
        "sound bank: {zone} composed in {:.1} s (merge {merged:.1} s, curves {:.1} s, finalize {:.1} s), {} aliases",
        started.elapsed().as_secs_f32(),
        before_finalize - merged,
        started.elapsed().as_secs_f32() - before_finalize,
        bank.catalog.sounds.len()
    );
    for gap in &bank.gaps {
        diag::warn!(
            Zone,
            "sound bank gap: {} `{}` — {}",
            gap.namespace.as_str(),
            gap.zone,
            gap.reason
        );
    }
    bank
}

pub fn load_mp_sound_bank(games: &GamesRoot, zone: &str) -> Result<LoadedSoundBank, String> {
    let zone_ff = find_zone_file(games, zone)?;
    let sources = gather_sound_sources(games, &zone_ff.path);
    let namespace =
        crate::zone_game_for_path(&zone_ff.path).map_or(AssetNamespace::Iw4, namespace_of_game);
    let (map, _) = ensure_zone_sound(&zone_ff.path);
    let map = map.map(|catalog| (*catalog).clone());
    Ok(compose_sound_bank(sources, zone, namespace, map))
}

impl LoadedSoundBank {
    fn gap(&mut self, namespace: AssetNamespace, zone: &str, reason: String) {
        self.gaps.push(SoundZoneGap {
            namespace,
            zone: zone.to_owned(),
            reason,
        });
    }

    pub fn gap_lines(&self) -> Vec<String> {
        self.gaps
            .iter()
            .map(|g| {
                format!(
                    "sound bank gap: {} `{}` — {}",
                    g.namespace.as_str(),
                    g.zone,
                    g.reason
                )
            })
            .collect()
    }
}

/// The other games' main-menu music for the Black Ops II front end, as
/// sound catalogs to take in: BO1's (`common_mp`, from the games folder),
/// MW2's and MW3's (`localized_code_post_gfx_mp`, from the voice-only
/// folder, so neither game is otherwise loaded). One log line per source.
pub fn menu_music_guests(games: &GamesRoot) -> (Vec<SoundCatalog>, Vec<String>) {
    let voice = voice_games_root();
    let mut catalogs = Vec::new();
    let mut lines = Vec::new();
    let mut take = |label: &str, found: Result<std::path::PathBuf, String>, load: fn(&Path) -> Result<SoundCatalog, String>| {
        match found.and_then(|path| load(&path)) {
            Ok(catalog) => {
                lines.push(format!(
                    "menu music: {label} — {} aliases",
                    catalog.sounds.len()
                ));
                catalogs.push(catalog);
            }
            Err(error) => lines.push(format!("menu music: {label} not available ({error})")),
        }
    };
    // Black Ops' menu sounds (`uin_navigation_*`) are in whichever of
    // these carries its interface bank; each is taken when it loads.
    for zone in ["common_mp", "code_post_gfx_mp", "ui_mp"] {
        take(
            &format!("Black Ops {zone}"),
            find_zone_file_version(games, zone, fastfile_t5::ZONE_VERSION_PC).map(|z| z.path),
            crate::sound_load_t5::load_sound_catalog_t5,
        );
    }
    for (label, version, load) in [
        (
            "Modern Warfare 2 localized_code_post_gfx_mp",
            fastfile_iw4::ZONE_VERSION_PC,
            load_sound_catalog as fn(&Path) -> Result<SoundCatalog, String>,
        ),
        (
            "Modern Warfare 3 localized_code_post_gfx_mp",
            fastfile_iw5::ZONE_VERSION_PC,
            crate::sound_load_iw5::load_sound_catalog_iw5,
        ),
    ] {
        let found = voice
            .as_ref()
            .ok_or_else(|| "CODLEGACY_VOICE_GAMES is not set".to_owned())
            .and_then(|root| find_zone_file_version(root, "localized_code_post_gfx_mp", version))
            .map(|z| z.path);
        take(label, found, load);
    }
    // bo2mp: Modern Warfare 3's menu sounds (`mouse_over`, `mouse_click`,
    // `mp_ui_decline`) for the shell's menus, from its interface zones.
    for zone in ["code_post_gfx_mp", "ui_mp"] {
        let found = voice
            .as_ref()
            .ok_or_else(|| "CODLEGACY_VOICE_GAMES is not set".to_owned())
            .and_then(|root| find_zone_file_version(root, zone, fastfile_iw5::ZONE_VERSION_PC))
            .map(|z| z.path);
        take(&format!("Modern Warfare 3 {zone} (menu sounds)"), found, crate::sound_load_iw5::load_sound_catalog_iw5);
    }
    (catalogs, lines)
}

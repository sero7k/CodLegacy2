//! bo2mp: Call of Duty: Ghosts' weapon sounds for its guest guns in a
//! Black Ops II match.
//!
//! `fastfile_iw6`'s exact walk reads a Ghosts zone's sound alias lists with
//! every shared loaded sound and sound file resolved. The aliases go into
//! the catalog under the Ghosts namespace (`AssetNamespace::Iw6`, which
//! the Ghosts guns' weapon rows carry), so they never take the name of a
//! Black Ops II, Black Ops or MW2/MW3 alias. Ghosts' gun sounds are loaded
//! 16-bit PCM in the zone (`ERiffAudioFormat` 1); streamed ones are FLAC in
//! `zone/soundfile<N>.pak` and are named by pak path and range
//! (`<pak>`, `<offset>:<length>`), decoded when played.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_iw6::sound::{LoadedRef, LoadedSound, SoundAlias, SoundData, StreamedSound};

use crate::sound_catalog::{CapturedAlias, CapturedSndCurve, CapturedSound, LoadedSoundPcm, SoundCatalog};
use crate::{AssetNamespace, ZoneGame, ZoneOwner};

/// The alias name prefixes kept (weapons' fire, reload, handling and
/// melee sounds, and what their notetracks play; bullet pass-bys and
/// impacts by surface), with every alias they chain to.
const KEEP_PREFIXES: &[&str] = &["whizby", "bullet_", "weap_", "wpn_", "melee_", "gear_", "foly_", "mvmt_", "US_1mc_achieve_kem", "US_1mc_use_tnuke", "US_1mc_enemy_tnuke", "ui_mp_kem_timer", "kem_", "mp_killstreak_kem"];

/// Where a Ghosts install is, from the games root and game_paths.cfg.
pub fn ghosts_install(games_root: Option<&Path>) -> Option<PathBuf> {
    let extra: Vec<PathBuf> = asset_transport::game_paths::get("ghosts")
        .into_iter()
        .collect();
    fastfile_iw6::install::find_install(games_root.filter(|p| !p.as_os_str().is_empty()), &extra)
}

/// The Ghosts zones its guns' sounds come from (`common_mp` holds them;
/// `common_core_mp` some more), those that exist.
pub fn ghosts_sound_zones(install: &Path) -> Vec<PathBuf> {
    ["common_mp", "common_core_mp"]
        .into_iter()
        .map(|z| install.join("zone").join(format!("{z}.ff")))
        .filter(|p| p.is_file())
        .collect()
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<SoundCatalog>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<SoundCatalog>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The weapon sounds of one Ghosts zone (read once per process).
pub fn ghosts_weapon_sounds(path: &Path) -> Result<Arc<SoundCatalog>, String> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(path).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_sound_catalog_iw6(path)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(path.to_path_buf(), catalog.clone());
    }
    Ok(catalog)
}

fn text(name: &fastfile_iw6::sound::Name) -> Option<String> {
    name.text().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// A loaded sound's catalog name: its own, else one from where it is.
fn loaded_name(l: &LoadedSound) -> String {
    text(&l.name).unwrap_or_else(|| format!("iw6_loaded_{:x}", l.data_offset.unwrap_or(0)))
}

fn stream_ref(zone_dir: &Path, s: &StreamedSound) -> Option<(String, String)> {
    if s.file_index == 0 || s.length == 0 {
        return None;
    }
    let pak = if s.localized {
        // (Localized streams live in the language folder; English.)
        zone_dir
            .join("english")
            .join(format!("eng_soundfile{}.pak", s.file_index))
    } else {
        zone_dir.join(format!("soundfile{}.pak", s.file_index))
    };
    Some((
        pak.to_string_lossy().into_owned(),
        format!("{}:{}", s.offset, s.length),
    ))
}

/// One Ghosts zone's weapon sound aliases (see the module docs).
pub fn load_sound_catalog_iw6(path: &Path) -> Result<SoundCatalog, String> {
    let zone = fastfile_iw6::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    if let Some(stop) = &stop {
        diag::warn!(
            Zone,
            "sound bank: Ghosts {} walked {} of {} assets (stopped at {:?}: {})",
            path.display(),
            stop.asset_index,
            zone.assets.assets.len(),
            stop.asset_type,
            stop.error
        );
    }
    let catalog = catalog_from_capture(path, &zone, &cap, false);
    diag::info!(
        Zone,
        "sound bank: Ghosts {}: {} weapon alias lists of {}, {} loaded clips",
        path.display(),
        catalog.sounds.len(),
        cap.sounds.len(),
        catalog.loaded.len()
    );
    Ok(catalog)
}

/// bo2mp: a Ghosts map zone's own sound aliases, all of them (ambience,
/// emitters, movement and impacts by surface, script sounds), from the
/// walk the map lane already made, under the Ghosts namespace.
pub fn ghosts_map_catalog(path: &Path, zone: &fastfile_iw6::Zone, cap: &fastfile_iw6::walk::Capture) -> SoundCatalog {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (zones, line) = ghosts_map_sound_tables(path, cap);
    diag::info!(Zone, "{line}");
    crate::iw6_rules::set_map_zones(zones, &stem);
    catalog_from_capture(path, zone, cap, true)
}

/// bo2mp: the zone's sound tables' zones and reverbs
/// (`soundtables/mp_common_zone.csv`, `soundtables/mp_common_reverb.csv`),
/// when it holds them.
pub fn ghosts_sound_zones_of(cap: &fastfile_iw6::walk::Capture) -> Option<crate::iw6_rules::SoundZones> {
    let tables: Vec<&fastfile_iw6::walk::StringTableRec> = cap.string_tables.iter().collect();
    sound_zones_from(&tables, "")
}

/// The zones of a map (its own `soundtables/<map>.csv` zone section first,
/// then `mp_common_zone.csv`'s) and the reverbs, from string tables in
/// look-up order (a name's first table wins). `None` without a zone or
/// reverb table.
fn sound_zones_from(tables: &[&fastfile_iw6::walk::StringTableRec], map: &str) -> Option<crate::iw6_rules::SoundZones> {
    let table = |name: &str| {
        tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .map(|t| crate::iw6_rules::table_cells(t.columns, &t.cells))
    };
    let mut zones = table(&format!("soundtables/{map}.csv"))
        .map(|t| crate::iw6_rules::parse_typed_zone_rows(&t))
        .unwrap_or_default();
    if let Some(common) = table("soundtables/mp_common_zone.csv") {
        zones.extend(crate::iw6_rules::parse_zone_rows(&common));
    }
    let reverbs = table("soundtables/mp_common_reverb.csv").map(|t| crate::iw6_rules::parse_reverb_rows(&t)).unwrap_or_default();
    (!zones.is_empty() || !reverbs.is_empty()).then_some(crate::iw6_rules::SoundZones { zones, reverbs })
}

/// `BO2MP_IW6_SOUND_TABLES` (on): a Ghosts map's sound tables are read from
/// the install's localized zones (`english/eng_patch_<map>`, `eng_<map>`,
/// `eng_patch_common_mp`, `eng_common_mp`), where Ghosts keeps them (not in
/// `common_mp`).
pub fn sound_tables_on() -> bool {
    crate::iw6_rules::switch("SOUND_TABLES", true)
}

/// The sound tables of a Ghosts map and the zones they name: the map
/// zone's own string tables, then the localized zones' (see
/// `sound_tables_on`); with one line on what was found.
pub fn ghosts_map_sound_tables(path: &Path, cap: &fastfile_iw6::walk::Capture) -> (Option<crate::iw6_rules::SoundZones>, String) {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut owned: Vec<fastfile_iw6::walk::StringTableRec> = cap.string_tables.iter().filter(|t| t.name.starts_with("soundtables/")).cloned().collect();
    let mut read = Vec::new();
    if sound_tables_on() {
        let dir = path.parent().unwrap_or(Path::new(".")).join("english");
        for z in [format!("eng_patch_{stem}"), format!("eng_{stem}"), "eng_patch_common_mp".to_owned(), "eng_common_mp".to_owned()] {
            let p = dir.join(format!("{z}.ff"));
            let Ok(zone) = fastfile_iw6::Zone::open(&p) else {
                continue;
            };
            let (zcap, _) = fastfile_iw6::walk::walk_zone(&zone);
            let mut n = 0usize;
            for t in zcap.string_tables.into_iter().filter(|t| t.name.starts_with("soundtables/")) {
                n += 1;
                owned.push(t);
            }
            read.push(format!("{z} {n}"));
        }
    }
    let refs: Vec<&fastfile_iw6::walk::StringTableRec> = owned.iter().collect();
    let zones = sound_zones_from(&refs, &stem);
    let mut names: Vec<String> = refs.iter().map(|t| format!("{} {}x{}", t.name.trim_start_matches("soundtables/"), t.rows, t.columns)).collect();
    names.dedup();
    let start = zones.as_ref().and_then(|z| {
        let s = z.initial_zone(&stem)?;
        Some(format!("starts in zone `{}`, reverb `{}` {:?}", s.name, s.reverb, z.reverb_of(s).map(|r| (r.preset.clone(), r.dry, r.wet))))
    });
    let line = format!(
        "iw6 sound tables: {stem}: {} (BO2MP_IW6_SOUND_TABLES), localized zones read {:?}, tables {:?}; {} zones, {} reverbs; {}",
        if sound_tables_on() { "on" } else { "off" },
        read,
        names,
        zones.as_ref().map_or(0, |z| z.zones.len()),
        zones.as_ref().map_or(0, |z| z.reverbs.len()),
        start.unwrap_or_else(|| "no start zone".into())
    );
    (zones, line)
}

/// bo2mp: the name of a Ghosts map's ambient bed (`ambient_<map>`), when its
/// zone holds that alias list.
pub fn ghosts_ambient_alias(cap: &fastfile_iw6::walk::Capture, map: &str) -> Option<String> {
    let want = format!("ambient_{}", map.to_ascii_lowercase());
    cap.sounds
        .iter()
        .filter_map(|l| l.name.text())
        .find(|n| n.eq_ignore_ascii_case(&want))
        .map(str::to_owned)
}

/// The catalog of a walked Ghosts zone: every list (`all`) or the weapon
/// prefixes and what they chain to; with the zone's curves.
fn catalog_from_capture(
    path: &Path,
    zone: &fastfile_iw6::Zone,
    cap: &fastfile_iw6::walk::Capture,
    all: bool,
) -> SoundCatalog {
    let zone_dir = path.parent().unwrap_or(Path::new("."));
    let owner = ZoneOwner::from_zone_path(path);
    let switches = crate::iw6_rules::Iw6Switches::get();
    // The lists kept: the weapon prefixes, and everything they chain to.
    let by_name: HashMap<&str, usize> = cap
        .sounds
        .iter()
        .enumerate()
        .filter_map(|(i, l)| Some((l.name.text()?, i)))
        .collect();
    let mut keep: BTreeSet<usize> = BTreeSet::new();
    let mut todo: Vec<usize> = if all {
        (0..cap.sounds.len()).collect()
    } else {
        by_name
            .iter()
            .filter(|(n, _)| KEEP_PREFIXES.iter().any(|p| n.starts_with(p)))
            .map(|(_, &i)| i)
            .collect()
    };
    while let Some(i) = todo.pop() {
        if !keep.insert(i) {
            continue;
        }
        for a in &cap.sounds[i].aliases {
            for next in [&a.secondary, &a.chain] {
                if let Some(&j) = next.text().and_then(|n| by_name.get(n)) {
                    todo.push(j);
                }
            }
        }
    }

    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    // bo2mp: the zone's own curves (a curve another zone holds is a name
    // here; it resolves from that zone's when the banks are composed).
    for curve in cap.snd_curves.values().filter(|c| !c.knots.is_empty()) {
        catalog.ingest_curve(CapturedSndCurve {
            name: crate::iw6_rules::curve_key(&curve.name),
            knots: curve.knots.clone(),
        });
    }
    let mut pcm_seen: HashMap<String, ()> = HashMap::new();
    for &i in &keep {
        let list = &cap.sounds[i];
        let Some(name) = text(&list.name) else {
            continue;
        };
        let mut aliases = Vec::with_capacity(list.aliases.len());
        for a in &list.aliases {
            let mut row = alias_row(&name, a, switches.curves);
            match a.sound.get().map(|f| &f.data) {
                Some(SoundData::Loaded(LoadedRef::Inline(l)))
                    if l.format == 1 && l.bits == 16 && l.data_len > 0 =>
                {
                    let lname = loaded_name(l);
                    row.file_type = Some(1);
                    row.file_exists = Some(1);
                    row.loaded_name = Some(lname.clone());
                    if pcm_seen.insert(lname.clone(), ()).is_none()
                        && let Some(at) = l.data_offset
                        && let Some(bytes) = zone.image.get(at..at + l.data_len as usize)
                    {
                        let frames = l.data_len / (2 * u32::from(l.channels.max(1)));
                        let mut pcm = LoadedSoundPcm::pcm16(
                            lname,
                            l.sample_rate,
                            i32::from(l.channels.max(1)),
                            frames,
                            bytes.to_vec(),
                        );
                        pcm.namespace = Some(AssetNamespace::Iw6);
                        pcm.zone = owner;
                        catalog.ingest_loaded(pcm);
                    }
                }
                Some(SoundData::Streamed(s)) => {
                    row.file_type = Some(2);
                    row.streamed = stream_ref(zone_dir, s);
                    row.file_exists = Some(u8::from(row.streamed.is_some()));
                }
                Some(SoundData::Primed { stream, .. }) => {
                    row.file_type = Some(3);
                    row.streamed = stream_ref(zone_dir, stream);
                    row.file_exists = Some(u8::from(row.streamed.is_some()));
                }
                // (Silent: a null loaded sound, or no file.)
                _ => {
                    row.file_type = Some(1);
                    row.file_name = Some("null".to_owned());
                }
            }
            aliases.push(row);
        }
        catalog.ingest_sound(CapturedSound {
            name,
            aliases,
            game: ZoneGame::T6,
            zone: owner,
            namespace: Some(AssetNamespace::Iw6),
        });
    }
    catalog.finalize();
    catalog
}

fn alias_row(list_name: &str, a: &SoundAlias, own_curves: bool) -> CapturedAlias {
    let unit = |v: f32, fallback: f32| {
        if v.is_finite() && v > 0.0 {
            v
        } else {
            fallback
        }
    };
    let p = &a.params;
    let curve = |c: &Option<fastfile_iw6::sound::SndCurve>| {
        c.as_ref().map(|c| CapturedSndCurve {
            name: crate::iw6_rules::curve_key(&c.name),
            knots: c.knots.clone(),
        })
    };
    let iw6 = crate::iw6_rules::Iw6Params {
        flags: a.flags,
        velocity_min: p.velocity_min,
        wet_mix_override: p.wet_mix_override,
        allow_doppler: p.allow_doppler,
        master_priority: p.master_priority,
        reverb_send: curve(&p.reverb_send),
        lpf_curve: p.lpf.as_ref().map(|c| c.name.clone()),
        speaker_map: p.speaker_map.clone(),
        occlusion: p.occlusion.map(|o| crate::iw6_rules::OcclusionShape {
            is_default: o.is_default,
            inner_angle: o.inner_angle,
            outer_angle: o.outer_angle,
            occluded_volume: o.occluded_volume,
        }),
        doppler: p.doppler.as_ref().map(|d| crate::iw6_rules::DopplerPreset {
            speed_of_sound: d.speed_of_sound,
            player_velocity_scale: d.player_velocity_scale,
            min_pitch: d.min_pitch,
            max_pitch: d.max_pitch,
            smoothing: d.smoothing,
        }),
    };
    CapturedAlias {
        alias_name: list_name.to_owned(),
        subtitle: text(&a.subtitle),
        secondary: text(&a.secondary),
        chain: text(&a.chain),
        mixer_group: text(&a.mixer_group),
        sequence: a.sequence,
        vol_min: a.vol_min.max(0.0),
        vol_max: a.vol_max.max(0.0),
        pitch_min: unit(a.pitch_min, 1.0),
        pitch_max: unit(a.pitch_max, 1.0),
        dist_min: a.dist_min.max(0.0),
        dist_max: a.dist_max.max(0.0),
        velocity_min: p.velocity_min,
        // (Ghosts' flag word is not IW4's: kept in `iw6`, so nothing reads
        // it as a loop or a channel it is not.)
        flags: None,
        probability: if a.probability > 0.0 {
            a.probability
        } else {
            1.0
        },
        slave_percentage: p.slave_percentage,
        lfe_percentage: p.lfe_percentage,
        center_percentage: p.center_percentage,
        envelop_min: p.envelop[0],
        envelop_max: p.envelop[1],
        envelop_percentage: p.envelop[2],
        start_delay: a.start_delay,
        // bo2mp: the alias's own falloff curve (`BO2MP_IW6_CURVES=0`: the
        // guest default every guest alias took before).
        volume_falloff: if own_curves { curve(&p.volume_falloff) } else { None },
        speaker_map: p.speaker_map.clone().filter(|s| !s.is_empty()),
        iw6: Some(Box::new(iw6)),
        ..CapturedAlias::default()
    }
}

/// Whether the Ghosts sound file reference is one `prepare` can play:
/// (`<pak path>`, `<offset>:<length>`).
pub fn ghosts_stream_range(dir: &str, name: &str) -> Option<(PathBuf, u64, u64)> {
    let (offset, length) = name.split_once(':')?;
    Some((
        PathBuf::from(dir),
        offset.parse().ok()?,
        length.parse().ok()?,
    ))
}

/// The bytes of a Ghosts streamed sound (a FLAC stream) from its pak.
pub fn read_ghosts_stream(dir: &str, name: &str) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let (pak, offset, length) =
        ghosts_stream_range(dir, name).ok_or_else(|| format!("bad Ghosts stream `{name}`"))?;
    let mut f = std::fs::File::open(&pak).map_err(|e| format!("{}: {e}", pak.display()))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut out = vec![0u8; length as usize];
    f.read_exact(&mut out)
        .map_err(|e| format!("{}: {e}", pak.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// bo2mp: `IW6_ZONE_DIR=<ghosts>/zone cargo test -p asset_audio --release
    /// ghosts_map -- --ignored --nocapture`: mp_frag's own sounds under the
    /// Ghosts namespace, its ambient bed, and the Ghosts values on its rows.
    #[test]
    #[ignore]
    fn ghosts_map_catalog_reads_mp_frag() {
        let Some(path) = std::env::var_os("IW6_ZONE_DIR")
            .map(|d| PathBuf::from(d).join("mp_frag.ff"))
            .filter(|p| p.is_file())
        else {
            return;
        };
        let zone = fastfile_iw6::Zone::open(&path).unwrap();
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        assert_eq!(ghosts_ambient_alias(&cap, "mp_frag").as_deref(), Some("ambient_mp_frag"));
        let catalog = ghosts_map_catalog(&path, &zone, &cap);
        assert!(catalog.curves.contains_key("iw6/curve/reactive_emitters"));
        let ambient = catalog.sound_in(AssetNamespace::Iw6, "ambient_mp_frag").unwrap();
        let row = &ambient.aliases[0];
        assert_eq!((row.vol_min, row.dist_min, row.dist_max), (0.375, 5000.0, 25000.0));
        let p = row.iw6.as_deref().unwrap();
        assert_eq!(p.flags & crate::iw6_rules::flags::LOOPING, 1);
        assert_eq!(row.volume_falloff.as_ref().unwrap().name, "iw6/curve/$default");
        let step = catalog.sound_in(AssetNamespace::Iw6, "step_run_metal").unwrap();
        let p = step.aliases[0].iw6.as_deref().unwrap();
        assert_eq!(crate::iw6_rules::flags::channel(p.flags), 42);
        assert!((p.wet_mix_override - 1.1554).abs() < 1e-4);
        eprintln!("mp_frag: {} lists, {} clips", catalog.sounds.len(), catalog.loaded.len());
    }

    #[test]
    fn rows_carry_ghosts_values_and_own_curve_names() {
        let mut a = fastfile_iw6::sound::SoundAlias {
            name: fastfile_iw6::sound::Name::None,
            subtitle: fastfile_iw6::sound::Name::None,
            secondary: fastfile_iw6::sound::Name::None,
            chain: fastfile_iw6::sound::Name::None,
            mixer_group: fastfile_iw6::sound::Name::None,
            sound: fastfile_iw6::sound::SoundFileRef::None,
            sequence: 0,
            vol_min: 0.1,
            vol_max: 0.2,
            vol_mod_index: 0,
            pitch_min: 0.0,
            pitch_max: 0.0,
            dist_min: 50.0,
            dist_max: 1200.0,
            flags: 0x5490,
            probability: 0.0,
            start_delay: 0,
            params: Default::default(),
        };
        a.params.volume_falloff = Some(fastfile_iw6::sound::SndCurve { name: ",footsteps".into(), knots: vec![] });
        let row = alias_row("step_run_flesh", &a, true);
        assert_eq!(row.volume_falloff.as_ref().unwrap().name, "iw6/curve/footsteps");
        assert_eq!(row.flags, None);
        assert_eq!((row.pitch_min, row.probability), (1.0, 1.0));
        assert_eq!(row.iw6.as_ref().unwrap().flags, 0x5490);
        // Curves off: the guest default fills it later.
        assert!(alias_row("x", &a, false).volume_falloff.is_none());
    }
}

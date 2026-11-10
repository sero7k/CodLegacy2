//! bo2mp: Horizon MW's weapon sounds (Modern Warfare 2's guns remastered
//! for Modern Warfare Remastered) for its guest guns in a Black Ops II
//! match.
//!
//! `fastfile_h1`'s exact walk reads a weapon zone's (`hmw-mod/zone/
//! hmw_*.ff`) alias lists and loaded sounds. Horizon MW's are all in the
//! zone: 16-bit PCM (format 1) or FLAC streams (format 6, kept whole and
//! decoded when played, `H1_FLAC`). The aliases kept are the ones its
//! weapons and their effects name, their notetracks play and the weapon
//! prefixes, with every alias they chain to, under the H1 namespace.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_h1::walk::sound::SoundAlias;

use crate::sound_catalog::{
    CapturedAlias, CapturedSndCurve, CapturedSound, LoadedSoundPcm, SoundCatalog,
};
use crate::{AssetNamespace, ZoneGame, ZoneOwner};

/// `SoundFile::kind` of a primed sound (a loaded head, then a stream).
const SAT_PRIMED: u8 = 3;

/// A loaded sound kept as the FLAC stream the zone holds.
pub const H1_FLAC: i32 = 0x6831;

const KEEP_PREFIXES: &[&str] = &[
    "weap_", "wpn_", "weapons_", "melee_", "gear_", "foly_", "mvmt_", "attach_",
];

/// Horizon MW's weapon zones that hold its listed guns, in the order they
/// are read (`hmw_attachments` holds its melee weapons and its attachments'
/// own animations, which no guest gun uses).
pub const HMW_GUN_ZONES: [&str; 3] = ["hmw_ar1", "hmw_smg", "hmw_shotgun"];

/// Horizon MW's killstreak zones: the Tactical Nuke's announcer lines are in
/// `hmw_dlc_killstreaks` (its factions' `<faction>_1mc_use_tnuke`,
/// `_enemy_tnuke` and `_achieve_nuke`).
pub const HMW_KILLSTREAK_ZONES: [&str; 2] = ["hmw_killstreak", "hmw_dlc_killstreaks"];

/// The killstreak aliases kept: the Tactical Nuke's announcer lines.
fn killstreak_alias(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("_1mc_") && (n.ends_with("_use_tnuke") || n.ends_with("_enemy_tnuke") || n.ends_with("_achieve_nuke"))
}

/// Where Horizon MW's weapon zones are: `<mwr>/hmw-mod/zone`, the
/// Modern Warfare Remastered folder set in Game Folders (`mwr`), else
/// `CODLEGACY_HMW_ZONE`.
pub fn hmw_zone_dir() -> Option<PathBuf> {
    let has = |d: &Path| {
        HMW_GUN_ZONES
            .iter()
            .any(|z| d.join(format!("{z}.ff")).is_file())
    };
    if let Some(dir) = codlegacy_env::var_os("CODLEGACY_HMW_ZONE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        && has(&dir)
    {
        return Some(dir);
    }
    let set = asset_transport::game_paths::get("mwr")?;
    [
        set.join("hmw-mod").join("zone"),
        set.join("zone"),
        set.clone(),
        set.parent()?.join("hmw-mod").join("zone"),
    ]
    .into_iter()
    .find(|d| has(d))
}

/// The Horizon MW weapon zones that exist in `dir`.
pub fn hmw_gun_zones(dir: &Path) -> Vec<PathBuf> {
    HMW_GUN_ZONES
        .iter()
        .map(|z| dir.join(format!("{z}.ff")))
        .filter(|p| p.is_file())
        .collect()
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<SoundCatalog>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<SoundCatalog>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The weapon sounds of one Horizon MW zone (read once per process).
pub fn hmw_weapon_sounds(path: &Path) -> Result<Arc<SoundCatalog>, String> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(path).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_sound_catalog_h1(path)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(path.to_path_buf(), catalog.clone());
    }
    Ok(catalog)
}

/// One Horizon MW zone's weapon sound aliases (see the module docs).
pub fn load_sound_catalog_h1(path: &Path) -> Result<SoundCatalog, String> {
    load_h1(path, None, false)
}

/// The Horizon MW killstreak zones that exist in `dir`.
pub fn hmw_killstreak_zones(dir: &Path) -> Vec<PathBuf> {
    HMW_KILLSTREAK_ZONES
        .iter()
        .map(|z| dir.join(format!("{z}.ff")))
        .filter(|p| p.is_file())
        .collect()
}

/// The Tactical Nuke's announcer lines of one Horizon MW killstreak zone
/// (read once per process).
pub fn hmw_killstreak_sounds(path: &Path) -> Result<Arc<SoundCatalog>, String> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(path).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_h1(path, None, true)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(path.to_path_buf(), catalog.clone());
    }
    Ok(catalog)
}

/// A zone's sound aliases: the weapon ones (`wanted` None), or exactly the
/// lists named in `wanted` and every alias they chain to (a map's ambient
/// sounds).
fn load_h1(path: &Path, wanted: Option<&BTreeSet<String>>, killstreak: bool) -> Result<SoundCatalog, String> {
    let zone = fastfile_h1::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
    if let Some(stop) = &stop {
        diag::warn!(
            Zone,
            "sound bank: Horizon MW {} stopped at asset {} ({:?}): {}",
            path.display(),
            stop.asset_index,
            stop.asset_type,
            stop.error
        );
    }
    let strings = &zone.assets.script_strings;
    let string = |i: u32| {
        strings
            .get(i as usize)
            .cloned()
            .flatten()
            .unwrap_or_else(String::new)
    };
    let owner = ZoneOwner::from_zone_path(path);
    let by_name: HashMap<&str, usize> = cap
        .sounds
        .iter()
        .enumerate()
        .map(|(i, l)| (l.name.as_str(), i))
        .collect();
    let named: BTreeSet<String> = cap
        .weapons
        .iter()
        .flat_map(|w| {
            w.sounds
                .iter()
                .map(|(_, n)| n.clone())
                .chain(w.notetrack_sound_values.iter().map(|&v| string(v)))
        })
        .chain(
            cap.fx
                .iter()
                .flat_map(|f| f.elems.iter().flat_map(|e| e.sounds.iter().cloned())),
        )
        .collect();
    let mut todo: Vec<usize> = by_name
        .iter()
        .filter(|(n, _)| match wanted {
            Some(w) => w.contains(**n),
            None if killstreak => killstreak_alias(n),
            None => KEEP_PREFIXES.iter().any(|p| n.starts_with(p)) || named.contains(**n),
        })
        .map(|(_, &i)| i)
        .collect();
    let mut keep: BTreeSet<usize> = BTreeSet::new();
    while let Some(i) = todo.pop() {
        if !keep.insert(i) {
            continue;
        }
        for a in &cap.sounds[i].aliases {
            for next in [&a.secondary, &a.chain] {
                if let Some(&j) = next.as_deref().and_then(|n| by_name.get(n)) {
                    todo.push(j);
                }
            }
        }
    }

    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    let mut seen: HashMap<u32, ()> = HashMap::new();
    let (mut lists, mut clips, mut silent, mut primed) = (0usize, 0usize, 0usize, 0usize);
    for &i in &keep {
        let list = &cap.sounds[i];
        if list.name.is_empty() {
            continue;
        }
        let mut aliases = Vec::with_capacity(list.aliases.len());
        for a in outdoor_records(&list.aliases) {
            let mut row = alias_row(&list.name, a);
            row.volume_falloff = a.falloff.as_ref().and_then(|name| {
                Some(CapturedSndCurve {
                    name: format!("h1/curve/{name}"),
                    knots: cap.snd_curves.get(name)?.iter().map(|k| (k[0], k[1])).collect(),
                })
            });
            let file = a.file.and_then(|v| cap.sound_files.get(&v));
            // (A primed sound's loaded part is only the head of a stream in a
            // sound pak, which Horizon MW's weapon zones do not come with.)
            primed += usize::from(file.is_some_and(|f| f.kind == SAT_PRIMED));
            let loaded = file
                .and_then(|f| f.loaded)
                .and_then(|id| cap.loaded.get(id.index as usize).map(|l| (id.index, l)));
            let bytes = loaded
                .and_then(|(k, l)| Some((k, l, zone.image.get(l.data?.0..l.data?.0 + l.data?.1)?)));
            match bytes {
                Some((k, l, data))
                    if !data.is_empty() && (l.format == 1 && l.bits == 16 || l.format == 6) =>
                {
                    let name = if l.name.is_empty() {
                        format!("h1_loaded_{k}")
                    } else {
                        l.name.clone()
                    };
                    row.file_type = Some(1);
                    row.file_exists = Some(1);
                    row.loaded_name = Some(name.clone());
                    if seen.insert(k, ()).is_none() {
                        let channels = i32::from(l.channels.max(1));
                        let mut pcm = if l.format == 1 {
                            let frames = data.len() as u32 / (2 * channels as u32);
                            LoadedSoundPcm::pcm16(
                                name,
                                l.sample_rate,
                                channels,
                                frames,
                                data.to_vec(),
                            )
                        } else {
                            let mut s = LoadedSoundPcm::captured(
                                name,
                                H1_FLAC,
                                l.sample_rate,
                                channels,
                                data.to_vec(),
                                Vec::new(),
                            );
                            s.samples = l.samples;
                            s
                        };
                        pcm.namespace = Some(AssetNamespace::H1);
                        pcm.zone = owner;
                        catalog.ingest_loaded(pcm);
                        clips += 1;
                    }
                }
                _ => {
                    silent += 1;
                    row.file_type = Some(1);
                    row.file_name = Some("null".to_owned());
                }
            }
            aliases.push(row);
        }
        lists += 1;
        catalog.ingest_sound(CapturedSound {
            name: list.name.clone(),
            aliases,
            game: ZoneGame::T6,
            zone: owner,
            namespace: Some(AssetNamespace::H1),
        });
    }
    catalog.finalize();
    diag::info!(
        Zone,
        "sound bank: Horizon MW {}: {lists} {} alias lists of {}, {clips} clips, {silent} aliases silent, {primed} primed heads only",
        path.display(),
        if wanted.is_some() { "map" } else if killstreak { "killstreak" } else { "weapon" },
        cap.sounds.len(),
    );
    Ok(catalog)
}

/// A Modern Warfare Remastered map's own sounds, from its createfx script
/// (`maps/createfx/<map>_fx.gsc`): loops that play for good and sounds
/// that play now and then, each at its place, and the zones that hold the
/// alias lists (the map, its patch, the common zones). Made by the map
/// lane, taken by the match's sound catalog (`take_h1_ambience`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct H1MapAmbience {
    pub zones: Vec<PathBuf>,
    /// (alias, origin in inches).
    pub loops: Vec<(String, [f32; 3])>,
    /// (alias, origin, wait range in seconds).
    pub intervals: Vec<(String, [f32; 3], [f32; 2])>,
}

static PENDING_AMBIENCE: std::sync::Mutex<Option<H1MapAmbience>> = std::sync::Mutex::new(None);

/// Set by the map lane for the map being loaded (None: not an MWR map).
pub fn set_h1_ambience(a: Option<H1MapAmbience>) {
    if let Ok(mut p) = PENDING_AMBIENCE.lock() {
        *p = a;
    }
}

/// Taken once by the match's sound catalog.
pub fn take_h1_ambience() -> Option<H1MapAmbience> {
    PENDING_AMBIENCE.lock().ok().and_then(|mut p| p.take())
}

/// The prefix that names an alias of the H1 namespace in a placement
/// (`asset_game::alias_namespace`).
pub const H1_ALIAS_PREFIX: &str = "h1:";

/// What a map's ambience adds to the match's sound catalog: the alias
/// lists its placements name (first zone first, a later zone only adds
/// lists the earlier ones lack), and the placements themselves, their
/// aliases under the H1 namespace prefix. Returns the catalog and a report
/// line.
pub fn h1_ambience_catalog(a: &H1MapAmbience) -> (SoundCatalog, crate::createfx::ScriptedMapFx, String) {
    let wanted: BTreeSet<String> = a
        .loops
        .iter()
        .map(|(n, _)| n.clone())
        .chain(a.intervals.iter().map(|(n, _, _)| n.clone()))
        .collect();
    let mut catalog = SoundCatalog::default();
    let mut read = 0usize;
    for zone in &a.zones {
        match load_h1(zone, Some(&wanted), false) {
            Ok(c) => {
                catalog.absorb_missing_aliases_unresolved(c);
                read += 1;
            }
            Err(e) => diag::warn!(Zone, "sound bank: map ambience: {e}"),
        }
    }
    catalog.finalize();
    let prefixed = |n: &str| format!("{H1_ALIAS_PREFIX}{n}");
    let mut fx = crate::createfx::ScriptedMapFx::default();
    for (alias, origin) in &a.loops {
        fx.loop_sounds.push(crate::createfx::CreateFxLoopSound {
            soundalias: prefixed(alias),
            origin_inches: *origin,
            line_end_inches: None,
        });
    }
    for (alias, origin, wait) in &a.intervals {
        fx.random_sounds.push(crate::createfx::RandomPointSound {
            soundalias: prefixed(alias),
            wait_secs: *wait,
            places_inches: vec![*origin],
        });
    }
    let found = wanted
        .iter()
        .filter(|n| catalog.index_in(AssetNamespace::H1, n).is_some())
        .count();
    let line = format!(
        "h1 ambience: {} loops and {} interval sounds ({} aliases named, {found} found in {read} of {} zones)",
        a.loops.len(),
        a.intervals.len(),
        wanted.len(),
        a.zones.len()
    );
    (catalog, fx, line)
}

/// The records of an alias list a listener outdoors hears. Beside its plain
/// records a list can keep some for a sound context (`wpn_int_med`,
/// `wpn_int_sml`, `wpn_semi_open`: a gun's tail in a small or a large
/// room), which the game plays only in that context: they are not variants
/// of the plain sound. A list of context records only keeps them all.
pub fn outdoor_records<'a, A: HasSoundContext>(aliases: &'a [A]) -> Vec<&'a A> {
    let plain: Vec<&A> = aliases.iter().filter(|a| !a.in_context()).collect();
    if plain.is_empty() {
        aliases.iter().collect()
    } else {
        plain
    }
}

/// An alias record that may belong to a sound context.
pub trait HasSoundContext {
    fn in_context(&self) -> bool;
}

impl HasSoundContext for SoundAlias {
    fn in_context(&self) -> bool {
        self.context.is_some()
    }
}

impl HasSoundContext for fastfile_s1::walk::sound::SoundAlias {
    fn in_context(&self) -> bool {
        self.context.is_some()
    }
}

fn alias_row(list_name: &str, a: &SoundAlias) -> CapturedAlias {
    let unit = |v: f32, fallback: f32| {
        if v.is_finite() && v > 0.0 {
            v
        } else {
            fallback
        }
    };
    CapturedAlias {
        alias_name: list_name.to_owned(),
        secondary: a.secondary.clone(),
        chain: a.chain.clone(),
        vol_min: a.vol_min.max(0.0),
        vol_max: a.vol_max.max(0.0),
        pitch_min: unit(a.pitch_min, 1.0),
        pitch_max: unit(a.pitch_max, 1.0),
        dist_min: a.dist_min.max(0.0),
        dist_max: a.dist_max.max(0.0),
        // (H1's flag word is not IW4's: left unread.)
        flags: None,
        probability: if a.probability > 0.0 {
            a.probability
        } else {
            1.0
        },
        ..CapturedAlias::default()
    }
}

impl LoadedSoundPcm {
    /// bo2mp: a Horizon MW clip kept as its FLAC stream.
    pub fn is_h1_flac(&self) -> bool {
        self.format() == H1_FLAC
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    #[test]
    #[ignore]
    fn hmw_fire_alias_probe() {
        let dir = codlegacy_env::var("CODLEGACY_HMW_ZONE").expect("CODLEGACY_HMW_ZONE");
        let out = std::env::var("PROBE_OUT").ok().map(PathBuf::from);
        let only = std::env::var("PROBE_GUN").ok();
        for z in HMW_GUN_ZONES {
            let path = Path::new(&dir).join(format!("{z}.ff"));
            if !path.is_file() {
                continue;
            }
            let zone = fastfile_h1::Zone::open(&path).unwrap();
            let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
            let by_name: HashMap<&str, usize> = cap
                .sounds
                .iter()
                .enumerate()
                .map(|(i, l)| (l.name.as_str(), i))
                .collect();
            use fastfile_h1::layout::WeaponDef as d;
            let fields = [
                ("fire", d::fireSound),
                ("firePlr", d::fireSoundPlayer),
                ("fireAkimbo", d::fireSoundPlayerAkimbo),
                ("fireLoopPlr", d::fireLoopSoundPlayer),
                ("fireLastPlr", d::fireLastSoundPlayer),
                ("fireFirstPlr", d::fireFirstSoundPlayer),
                ("fireMedPlr", d::fireMedSoundPlayer),
                ("fireHighPlr", d::fireHighSoundPlayer),
                ("fireCustomPlr", d::fireCustomSoundPlayer),
                ("reloadPlr", d::reloadSoundPlayer),
                ("raisePlr", d::raiseSoundPlayer),
            ];
            for (name, knots) in &cap.snd_curves {
                println!("curve {name}: {knots:?}");
            }
            let bank = load_sound_catalog_h1(&path).unwrap();
            let rows = bank.sounds.iter().flat_map(|s| &s.aliases);
            let (all, curved) = rows.fold((0, 0), |(a, c), r| {
                (a + 1, c + usize::from(r.volume_falloff.is_some()))
            });
            let dropped: usize = cap
                .sounds
                .iter()
                .filter(|l| bank.sound_in(AssetNamespace::H1, &l.name).is_some())
                .map(|l| l.aliases.len() - outdoor_records(&l.aliases).len())
                .sum();
            println!(
                "bank {z}: {all} records kept, {curved} with their own falloff curve, {dropped} context records left out"
            );
            for w in &cap.weapons {
                if only.as_deref().is_some_and(|g| !w.name.contains(g)) {
                    continue;
                }
                println!("== {} [{z}]", w.name);
                for (label, off) in fields {
                    let Some((_, n)) = w.sounds.iter().find(|(o, _)| *o == off) else {
                        continue;
                    };
                    if n.is_empty() {
                        continue;
                    }
                    let mut stack = vec![(n.clone(), 0usize)];
                    let mut seen = BTreeSet::new();
                    while let Some((n, depth)) = stack.pop() {
                        if !seen.insert(n.clone()) || depth > 6 {
                            continue;
                        }
                        let pad = " ".repeat(depth * 2 + 1);
                        let Some(&i) = by_name.get(n.as_str()) else {
                            println!("  {label}{pad}{n}: NOT IN ZONE");
                            continue;
                        };
                        for a in &cap.sounds[i].aliases {
                            let f = a.file.and_then(|v| cap.sound_files.get(&v));
                            let l = f
                                .and_then(|f| f.loaded)
                                .and_then(|id| cap.loaded.get(id.index as usize));
                            let clip = l.map_or_else(
                                || {
                                    format!(
                                        "no loaded (kind {:?} stream {:?})",
                                        f.map(|f| f.kind),
                                        f.and_then(|f| f.stream)
                                    )
                                },
                                |l| {
                                    format!(
                                        "{} fmt{} {}Hz {}ch {}b samples={} bytes={} inline={} stream={:?}",
                                        l.name,
                                        l.format,
                                        l.sample_rate,
                                        l.channels,
                                        l.bits,
                                        l.samples,
                                        l.data_bytes,
                                        l.data.map_or(0, |d| d.1),
                                        l.stream
                                    )
                                },
                            );
                            println!(
                                "  {label}{pad}{n} vol {:.2}-{:.2} dist {:.0}-{:.0} flags {:#010x} p={:.2} ctx={:?} curve={:?} lpf={:?} list_ctx={:02x?} sec={:?} chain={:?} | {clip}",
                                a.vol_min,
                                a.vol_max,
                                a.dist_min,
                                a.dist_max,
                                a.flags,
                                a.probability,
                                a.context,
                                a.falloff,
                                a.lpf,
                                cap.sounds[i].context_list.map(|(at, len)| &zone.image[at..at + len]),
                                a.secondary,
                                a.chain
                            );
                            if let (Some(out), Some(l)) = (&out, l)
                                && let Some((at, len)) = l.data
                            {
                                let data = &zone.image[at..at + len];
                                let stem = l.name.replace(['/', '\\'], "_");
                                let ext = if l.format == 6 { "flac" } else { "raw" };
                                let _ = std::fs::write(out.join(format!("{stem}.{ext}")), data);
                                if l.format == 6 && data.len() > 26 && &data[..4] == b"fLaC" {
                                    let si = &data[8..];
                                    let rate = (u32::from(si[10]) << 12)
                                        | (u32::from(si[11]) << 4)
                                        | (u32::from(si[12]) >> 4);
                                    let ch = ((si[12] >> 1) & 7) + 1;
                                    let bps = (((si[12] & 1) << 4) | (si[13] >> 4)) + 1;
                                    let total = (u64::from(si[13] & 0xf) << 32)
                                        | u64::from(u32::from_be_bytes([
                                            si[14], si[15], si[16], si[17],
                                        ]));
                                    println!("  {pad}  STREAMINFO {rate}Hz {ch}ch {bps}b total={total}");
                                } else if l.format == 6 {
                                    println!("  {pad}  NOT fLaC: {:02x?}", &data[..8.min(data.len())]);
                                }
                            }
                            for next in [&a.secondary, &a.chain].into_iter().flatten() {
                                stack.push((next.clone(), depth + 1));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tactical_nuke_lines_are_the_ones_kept() {
        assert!(killstreak_alias("PC_1mc_use_tnuke"));
        assert!(killstreak_alias("FR_1mc_enemy_tnuke"));
        assert!(killstreak_alias("AF_1mc_achieve_nuke"));
        assert!(!killstreak_alias("PC_1mc_use_agm"));
        assert!(!killstreak_alias("h2_nuke_timer"));
    }

    /// With Horizon MW's zones (`CODLEGACY_HMW_ZONE=<hmw-mod/zone>`): the
    /// announcer's three Tactical Nuke lines come in with their clips.
    #[test]
    #[ignore = "needs Horizon MW's hmw_dlc_killstreaks.ff"]
    fn the_dlc_zone_has_the_nuke_lines() {
        let dir = PathBuf::from(std::env::var("CODLEGACY_HMW_ZONE").expect("CODLEGACY_HMW_ZONE"));
        let bank = hmw_killstreak_sounds(&dir.join("hmw_dlc_killstreaks.ff")).expect("zone");
        for line in ["pc_1mc_use_tnuke", "pc_1mc_enemy_tnuke", "pc_1mc_achieve_nuke"] {
            assert!(bank.sound_in(AssetNamespace::H1, line).is_some_and(|s| !s.aliases.is_empty()), "{line}");
        }
    }
}

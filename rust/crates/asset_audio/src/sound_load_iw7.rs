//! bo2mp: Infinite Warfare's (IW7) weapon sounds for its guest guns in a
//! Black Ops II match.
//!
//! Each zone's sound bank (`fastfile_iw7::walk::sound_bank`) lists its
//! aliases; each alias record names its clip by `assetId`, an entry of the
//! zone's `.sabl` (loaded) / `.sabs` (streamed) bank beside it
//! (`fastfile_iw7::sab`): FLAC frames, decoded when played under the IW7
//! namespace (`read_iw7_stream`). The patch zones' aliases come first.
//!
//! A record is 200 bytes: its own id (0x28, `SND_HashName` of its list's
//! name), its secondary's (0x2c), its clip's (0x34), one (context type,
//! value) pair (0x38, 0x3c), volume (0x48, 0x4c), pitch (0x54, 0x58) and
//! distances (0x60, 0x64) as floats. A list's records are variants of one
//! sound but for their contexts: a gun's fire keeps its killcam slow-motion
//! shot (`atmosphere` `killcam`) beside the real one, so only the records
//! of the default context are kept (`default_rows`). A list whose name the
//! walk does not resolve is named by its id (`#1a2b3c4d`), as secondaries
//! are found.
//!
//! A record whose clip is in a bank that is not there is left out; an
//! alias none of whose records has its clip is not kept, so a gun plays its
//! Black Ops II donor's sound for it (`iw7_alias_playable`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_iw7::sab::{SabFile, bank_paths};
use fastfile_iw7::walk::SoundBankRec;

use crate::sound_catalog::{CapturedAlias, CapturedSound, SoundCatalog};
use crate::{AssetNamespace, ZoneGame, ZoneOwner};

/// The zones whose banks hold the guns' sounds, in the order an alias is
/// taken from (a patch before the zone it patches).
pub const IW7_SOUND_ZONES: [&str; 4] = ["patch_common_mp", "patch_common_core_mp", "common_core_mp", "common_mp"];

/// The alias name prefixes kept (weapons' fire, handling and reload foley),
/// with every alias they chain to.
const KEEP_PREFIXES: &[&str] = &["weap_", "wpn_", "vestgear_", "iw7_", "foley_", "proj_", "exp_"];

/// What one IW7 folder's banks came to.
#[derive(Clone, Debug, Default)]
pub struct Iw7SoundCensus {
    /// Alias lists kept; records with their clip; records whose clip is in
    /// a bank that is not there.
    pub lists: usize,
    pub playable: usize,
    pub absent: usize,
    /// Per zone: its bank files found, and the records naming a clip that
    /// none of them holds.
    pub banks: Vec<(String, Vec<String>, usize)>,
}

struct Iw7Sounds {
    catalog: Arc<SoundCatalog>,
    playable: HashSet<String>,
    census: Iw7SoundCensus,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<Iw7Sounds>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Iw7Sounds>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cached(dir: &Path) -> Option<Arc<Iw7Sounds>> {
    cache().lock().ok().and_then(|c| c.get(dir).cloned())
}

fn remember(dir: &Path, s: Iw7Sounds) -> Arc<Iw7Sounds> {
    let s = Arc::new(s);
    if let Ok(mut c) = cache().lock() {
        return c.entry(dir.to_path_buf()).or_insert(s).clone();
    }
    s
}

/// The weapon sounds of Infinite Warfare's zones in `dir` (read once per
/// process: walked here unless `iw7_sounds_from_banks` gave them first).
pub fn iw7_weapon_sounds(dir: &Path) -> Result<Arc<SoundCatalog>, String> {
    if let Some(hit) = cached(dir) {
        return Ok(hit.catalog.clone());
    }
    let mut banks: Vec<(String, SoundBankRec)> = Vec::new();
    for zone in IW7_SOUND_ZONES {
        let Some(path) = fastfile_iw7::zone_path(dir, zone) else { continue };
        let z = fastfile_iw7::Zone::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (mut cap, stop) = fastfile_iw7::walk::walk_zone(&z);
        if let Some(s) = &stop
            && let Some(bank) = fastfile_iw7::walk::recover_sound_bank(&z, s)
        {
            cap.sound_banks.push(bank);
        }
        banks.extend(cap.sound_banks.into_iter().map(|b| (zone.to_owned(), b)));
    }
    if banks.is_empty() {
        return Err(format!("no Infinite Warfare sound banks in {}", dir.display()));
    }
    let refs: Vec<(&str, &SoundBankRec)> = banks.iter().map(|(z, b)| (z.as_str(), b)).collect();
    Ok(iw7_sounds_from_banks(dir, &refs).0)
}

/// The weapon sounds of the zones' banks already walked (`(zone, bank)`,
/// in `IW7_SOUND_ZONES`' order), kept for `iw7_weapon_sounds`, and what
/// they came to.
pub fn iw7_sounds_from_banks(dir: &Path, banks: &[(&str, &SoundBankRec)]) -> (Arc<SoundCatalog>, Iw7SoundCensus) {
    if let Some(hit) = cached(dir) {
        return (hit.catalog.clone(), hit.census.clone());
    }
    let s = remember(dir, build(dir, banks));
    (s.catalog.clone(), s.census.clone())
}

/// Whether an alias of the IW7 guns plays one of IW7's own clips (its
/// banks read by `iw7_weapon_sounds` or `iw7_sounds_from_banks`).
pub fn iw7_alias_playable(dir: &Path, alias: &str) -> bool {
    cached(dir).is_some_and(|s| s.playable.contains(alias))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('<') && n.bytes().all(|b| b.is_ascii_graphic())
}

fn hash_name(id: u32) -> String {
    format!("#{id:08x}")
}

/// The context values a Black Ops II listener is in: not in a killcam,
/// above water, outdoors, moving normally, standing, the player's own (and
/// 0: the context unset).
fn default_context(value: u32) -> bool {
    static DEFAULTS: OnceLock<[u32; 5]> = OnceLock::new();
    value == 0 || DEFAULTS.get_or_init(|| ["over", "outdoor", "normal", "stand", "player"].map(sab_t6::sound_hash)).contains(&value)
}

/// The context type a killcam's slow-motion sounds are told apart by
/// (none of its values is a live match's).
fn atmosphere() -> u32 {
    static T: OnceLock<u32> = OnceLock::new();
    *T.get_or_init(|| sab_t6::sound_hash("atmosphere"))
}

/// The records of one list the game would play: per context type, those of
/// its default value; when none has one, the records without that type, or
/// (none without it) the first record's value (none for `atmosphere`). A record without a context
/// always plays.
fn default_rows(records: &[&[u8]]) -> Vec<usize> {
    let ctx = |r: &[u8]| (u32_at(r, 0x38), u32_at(r, 0x3c));
    let mut chosen: Vec<(u32, Option<u32>)> = Vec::new();
    for r in records {
        let (t, _) = ctx(r);
        if t == 0 || chosen.iter().any(|&(c, _)| c == t) {
            continue;
        }
        let values = || records.iter().map(|r| ctx(r)).filter(|&(c, _)| c == t).map(|(_, v)| v);
        let untyped = records.iter().any(|r| ctx(r).0 != t);
        let pick = values().find(|&v| default_context(v)).or_else(|| if untyped || t == atmosphere() { None } else { values().next() });
        chosen.push((t, pick));
    }
    (0..records.len())
        .filter(|&i| {
            let (t, v) = ctx(records[i]);
            t == 0 || chosen.iter().any(|&(c, p)| c == t && p == Some(v))
        })
        .collect()
}

/// One alias list as the zones hold it: its records' bytes, in the zone
/// that holds them.
struct List<'a> {
    zone: usize,
    records: Vec<&'a [u8]>,
}

fn build(dir: &Path, banks: &[(&str, &SoundBankRec)]) -> Iw7Sounds {
    build_with(dir, banks, &|n: &str| KEEP_PREFIXES.iter().any(|p| n.starts_with(p)), "common_mp.ff", &[])
}

/// `build` keeping the lists `keep` names (and their chains), owned by the
/// zone `owner_zone` in `dir`.
fn build_with(dir: &Path, banks: &[(&str, &SoundBankRec)], keep_list: &dyn Fn(&str) -> bool, owner_zone: &str, more_banks: &[&str]) -> Iw7Sounds {
    let started = std::time::Instant::now();
    // The clip banks, per zone.
    let mut zones: Vec<&str> = Vec::new();
    let mut files: Vec<Vec<SabFile>> = Vec::new();
    for (zone, _) in banks {
        if zones.contains(zone) {
            continue;
        }
        zones.push(zone);
        files.push(
            bank_paths(dir, zone)
                .into_iter()
                .filter_map(|p| match SabFile::open(&p) {
                    Ok(b) => Some(b),
                    Err(e) => {
                        diag::warn!(Zone, "sound bank: IW7 {e}");
                        None
                    }
                })
                .collect(),
        );
    }
    // (Banks with no alias table of their own, searched for clips after
    // the zones' own: the shared streamed banks.)
    for zone in more_banks {
        if zones.contains(zone) {
            continue;
        }
        let found: Vec<SabFile> = bank_paths(dir, zone).into_iter().filter_map(|p| SabFile::open(&p).ok()).filter(|b| !b.entries.is_empty()).collect();
        if !found.is_empty() {
            zones.push(zone);
            files.push(found);
        }
    }
    // Lists by name (the first zone's), and names by id.
    let mut lists: HashMap<String, List> = HashMap::new();
    let mut by_id: HashMap<u32, String> = HashMap::new();
    for (zone, bank) in banks {
        let z = zones.iter().position(|n| n == zone).unwrap_or(0);
        for a in &bank.aliases {
            let records: Vec<&[u8]> = a.records.iter().map(|(_, r)| r.as_slice()).filter(|r| r.len() >= 0xc8).collect();
            let Some(first) = records.first() else { continue };
            let id = u32_at(first, 0x28);
            let name = if valid_name(&a.name) { a.name.clone() } else { hash_name(id) };
            if valid_name(&a.name) || !by_id.contains_key(&id) {
                by_id.insert(id, name.clone());
            }
            lists.entry(name).or_insert(List { zone: z, records });
        }
    }
    let secondary = |r: &[u8]| Some(u32_at(r, 0x2c)).filter(|&s| s != 0).and_then(|s| by_id.get(&s)).cloned();
    let mut keep: BTreeSet<&str> = BTreeSet::new();
    let mut todo: Vec<&str> = lists.keys().map(String::as_str).filter(|n| keep_list(n)).collect();
    while let Some(n) = todo.pop() {
        if !keep.insert(n) {
            continue;
        }
        let l = &lists[n];
        for i in default_rows(&l.records) {
            if let Some(s) = secondary(l.records[i])
                && let Some((k, _)) = lists.get_key_value(&s)
            {
                todo.push(k.as_str());
            }
        }
    }
    let owner = ZoneOwner::from_zone_path(&dir.join(owner_zone));
    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    let mut census = Iw7SoundCensus {
        banks: zones.iter().zip(&files).map(|(z, f)| ((*z).to_owned(), f.iter().map(|b| b.path.display().to_string()).collect(), 0)).collect(),
        ..Iw7SoundCensus::default()
    };
    // A list's playable rows, and whether its own clips play (a list with
    // none, only a chain to another, is a layer's: kept for the layer).
    let mut rows_of: Vec<(&str, Vec<CapturedAlias>, bool)> = Vec::new();
    for name in &keep {
        let l = &lists[*name];
        let mut rows = Vec::new();
        let mut own = false;
        for i in default_rows(&l.records) {
            let r = l.records[i];
            let asset = u32_at(r, 0x34);
            let hit = (asset != 0)
                .then(|| files.iter().skip(l.zone).chain(files.iter().take(l.zone)).flatten().find_map(|b| b.find(asset).map(|e| (b, *e))))
                .flatten()
                .filter(|(_, e)| e.format == fastfile_iw7::sab::FORMAT_FLAC && e.size > 0);
            let sec = secondary(r);
            match hit {
                Some((bank, e)) => {
                    let (offset, length) = e.frames_range();
                    let mut row = alias_row(name, r, sec);
                    row.file_type = Some(2);
                    row.file_exists = Some(1);
                    row.streamed = Some((bank.path.to_string_lossy().into_owned(), format!("{offset}:{length}:{}:{}", e.frames, e.rate)));
                    row.flags = Some(u32::from(e.looping));
                    rows.push(row);
                    own = true;
                    census.playable += 1;
                }
                None if asset != 0 => {
                    census.absent += 1;
                    if let Some(b) = census.banks.get_mut(l.zone) {
                        b.2 += 1;
                    }
                }
                // (A record with no clip of its own: only its secondary.)
                None if sec.is_some() => {
                    let mut row = alias_row(name, r, sec);
                    row.file_type = Some(1);
                    row.file_name = Some("null".to_owned());
                    rows.push(row);
                }
                None => {}
            }
        }
        rows_of.push((name, rows, own));
    }
    // Secondaries to a list that plays nothing are dropped.
    let sounding: HashSet<&str> = rows_of.iter().filter(|(_, _, own)| *own).map(|(n, _, _)| *n).collect();
    let mut playable = HashSet::new();
    for (name, mut rows, own) in rows_of {
        for row in &mut rows {
            if row.secondary.as_deref().is_some_and(|s| !sounding.contains(s)) {
                row.secondary = None;
            }
        }
        rows.retain(|r| r.file_exists == Some(1) || r.secondary.is_some());
        if rows.is_empty() {
            continue;
        }
        if own {
            playable.insert(name.to_owned());
        }
        census.lists += 1;
        catalog.ingest_sound(CapturedSound {
            name: name.to_owned(),
            aliases: rows,
            game: ZoneGame::T6,
            zone: owner,
            namespace: Some(AssetNamespace::Iw7),
        });
    }
    catalog.finalize();
    diag::info!(
        Zone,
        "sound bank: IW7 {}: {} weapon alias lists of {}, {} records with a clip, {} whose clip's bank is not there [{:.1} s]",
        dir.display(),
        census.lists,
        lists.len(),
        census.playable,
        census.absent,
        started.elapsed().as_secs_f64()
    );
    Iw7Sounds { catalog: Arc::new(catalog), playable, census }
}

fn alias_row(name: &str, r: &[u8], secondary: Option<String>) -> CapturedAlias {
    let unit = |v: f32, fallback: f32| if v.is_finite() && v > 0.0 { v } else { fallback };
    let (vol_min, vol_max) = (unit(f32_at(r, 0x48), 1.0), unit(f32_at(r, 0x4c), 1.0));
    let dist_min = f32_at(r, 0x60).max(0.0);
    let dist_max = f32_at(r, 0x64).max(0.0);
    CapturedAlias {
        alias_name: name.to_owned(),
        secondary,
        vol_min,
        vol_max: vol_max.max(vol_min),
        pitch_min: unit(f32_at(r, 0x54), 1.0),
        pitch_max: unit(f32_at(r, 0x58), 1.0),
        dist_min,
        // (A gun heard across the map, as Black Ops II's are.)
        dist_max: if name.ends_with("_npc") && name.starts_with("weap_") { dist_max.max(5000.0) } else { dist_max.max(dist_min + 1.0) },
        flags: None,
        probability: 1.0,
        ..CapturedAlias::default()
    }
}

/// An IW7 clip's bytes as a FLAC stream: `dir` its bank file, `name`
/// `<offset>:<length>:<frames>:<rate>` of its FLAC frames there.
pub fn read_iw7_stream(dir: &str, name: &str) -> Result<Vec<u8>, String> {
    let mut it = name.split(':').map(str::parse::<u64>);
    let (Some(Ok(offset)), Some(Ok(length)), Some(Ok(frames)), Some(Ok(rate))) = (it.next(), it.next(), it.next(), it.next()) else {
        return Err(format!("bad IW7 clip `{name}`"));
    };
    let length = u32::try_from(length).map_err(|_| format!("bad IW7 clip `{name}`"))?;
    let rate = u32::try_from(rate).map_err(|_| format!("bad IW7 clip `{name}`"))?;
    fastfile_iw7::sab::read_flac(Path::new(dir), offset, length, frames, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(ctx: (u32, u32)) -> Vec<u8> {
        let mut r = vec![0u8; 200];
        r[0x38..0x3c].copy_from_slice(&ctx.0.to_le_bytes());
        r[0x3c..0x40].copy_from_slice(&ctx.1.to_le_bytes());
        r
    }

    #[test]
    fn killcam_records_are_left_out() {
        let atmosphere = sab_t6::sound_hash("atmosphere");
        let killcam = sab_t6::sound_hash("killcam");
        assert_eq!(atmosphere, 0x74d3_aac7);
        let rs = [record((atmosphere, killcam)), record((atmosphere, killcam)), record((atmosphere, 0)), record((0, 0))];
        let refs: Vec<&[u8]> = rs.iter().map(Vec::as_slice).collect();
        assert_eq!(default_rows(&refs), [2, 3]);
        // A killcam's sound only: nothing; another context's only: it.
        let rs = [record((atmosphere, killcam))];
        let refs: Vec<&[u8]> = rs.iter().map(Vec::as_slice).collect();
        assert!(default_rows(&refs).is_empty());
        let rs = [record((7, 9))];
        let refs: Vec<&[u8]> = rs.iter().map(Vec::as_slice).collect();
        assert_eq!(default_rows(&refs), [0]);
        assert_eq!(hash_name(0xab), "#000000ab");
        assert!(read_iw7_stream("x", "1:2:3").is_err());
    }
}

/// bo2mp: whether an Infinite Warfare map plays its own ambience
/// (`BO2MP_IW7_AMBIENCE=0` off; on by default).
pub fn iw7_map_ambience_enabled() -> bool {
    !std::env::var("BO2MP_IW7_AMBIENCE").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

/// bo2mp: an Infinite Warfare map's ambience, from its own zone and bank,
/// as [`crate::bo3_map_sounds`] reads a Black Ops III map's.
///
/// The map's bank (`<map>.all`, clips in `<map>.sabl` / `.sabs` beside the
/// zone) holds its aliases and its sound table
/// (`fastfile_iw7::walk::SoundTableRec`): the sound zones, each with the loop
/// that plays in it (`ambientStream`, the room tone), its reverb and its mix.
/// See [`iw7_map_fx`] for which zones are the map's rooms.
pub fn iw7_map_sounds(path: &Path) -> Result<SoundCatalog, String> {
    let zone = fastfile_iw7::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, stop) = fastfile_iw7::walk::walk_fitted(&zone);
    if let Some(stop) = stop {
        diag::warn!(Zone, "sound bank: IW7 map {} walked as far as {stop:?}", path.display());
    }
    let map = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    let bank = w
        .cap
        .sound_banks
        .iter()
        .find(|b| b.zone.eq_ignore_ascii_case(&map) || b.name.eq_ignore_ascii_case(&format!("{map}.all")))
        .ok_or_else(|| format!("{}: no `{map}.all` sound bank", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    // (A map's long loops can be streamed from a shared bank: riot's
    // `amb_mp_riot_lr` is not in `mp_riot.sabs`, which is empty here, while
    // geneva's own `.sabs` holds its tone. The patch and shared streamed
    // banks are searched after the map's own.)
    let patch = format!("patch_{map}");
    let more = [patch.as_str(), "patch_code_post_gfx", "code_post_gfx", "patch_common_mp", "common_mp"];
    let built = build_with(dir, &[(map.as_str(), bank)], &|_| true, &format!("{map}.ff"), &more);
    let mut catalog = Arc::try_unwrap(built.catalog).unwrap_or_else(|a| (*a).clone());
    let mut fx = iw7_map_fx(&map, &bank.table);
    if let Some(m) = w.cap.map_ents.first() {
        let rooms: Vec<String> = fx.rooms.iter().map(|r| r.0.clone()).collect();
        fx.room_volumes = iw7_room_volumes(&zone.image, &m.client_triggers, &rooms);
    }
    diag::info!(
        Zone,
        "sound bank: IW7 map {}: {} alias lists with a clip; rooms {:?}; tones {:?}; {} room volumes",
        path.display(),
        built.census.lists,
        fx.rooms.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        fx.room_tones,
        fx.room_volumes.len()
    );
    // The rooms' echoes: the bank's reverb presets as radverbs.
    for e in &bank.send_effects {
        if !e.name.is_empty() {
            catalog.radverbs.insert(e.name.clone(), iw7_radverb(&e.values));
        }
    }
    catalog.scripted_map_fx = Some(fx);
    Ok(catalog)
}

/// An IW7 reverb preset (`SndSendEffectParams`' 17 values) as the 16 a
/// radverb holds (`audio::room_echo::params_from`'s order, Black Ops II's
/// `SndRadverb`). IW7 has a proximity late gain after the late gain, which
/// is left out, and a rear level where BO2 has a return high-pass, which is
/// left at 0.
pub fn iw7_radverb(v: &[f32; 17]) -> [f32; 16] {
    let [smoothing, early_time, late_time, early_gain, late_gain, _late_gain_prox, return_gain, early_lpf, late_lpf, input_lpf, damp_lpf, wall_reflect, dry_gain, early_size, late_size, diffusion, _rear_level] = *v;
    [smoothing, early_time, late_time, early_gain, late_gain, return_gain, early_lpf, late_lpf, input_lpf, damp_lpf, wall_reflect, dry_gain, early_size, late_size, diffusion, 0.0]
}

/// The rooms and room tones of an Infinite Warfare map: its sound zones
/// whose loop is the map's own (`amb_<map>_...`, e.g. `amb_mp_riot_lr`),
/// each with its reverb's room type and levels. The bank's other zones are
/// the game's states (shell shock, killcam, killstreak cockpits): left out.
///
/// The default room (first) is the zone named after the map, else one with
/// the `ext_street` reverb, else the first. Which zone a player stands in
/// comes from the map's client triggers, not read yet, so a player hears
/// the default room's tone everywhere; on the maps read the map's zones all
/// loop the same tone, so only the reverb would differ, and IW7's reverb
/// presets (`gen_sm_room`, ...) are named in the banks but their values are
/// not in the zones: the rooms carry no echo (`radverbs` has none of them).
pub fn iw7_map_fx(map: &str, table: &fastfile_iw7::walk::SoundTableRec) -> crate::createfx::ScriptedMapFx {
    let own = format!("amb_{map}_");
    let mut zones: Vec<&fastfile_iw7::walk::SndZoneRec> = table.zones.iter().filter(|z| z.ambient_stream.starts_with(&own)).collect();
    let rank = |z: &fastfile_iw7::walk::SndZoneRec| {
        if z.name == map {
            0
        } else if z.reverb.ends_with("ext_street") {
            1
        } else {
            2
        }
    };
    zones.sort_by_key(|z| rank(z));
    let mut fx = crate::createfx::ScriptedMapFx::default();
    for z in zones {
        if fx.rooms.iter().any(|r| r.0 == z.name) {
            continue;
        }
        let (preset, dry, wet) = table.reverbs.get(usize::from(z.reverb_index)).map_or((String::new(), 1.0, 0.0), |r| (r.room_type.clone(), r.dry, r.wet));
        fx.rooms.push((z.name.clone(), preset, dry, wet));
        fx.room_tones.push((z.name.clone(), z.ambient_stream.clone()));
    }
    fx
}

#[cfg(test)]
mod map_tests {
    /// A map's zones: the map's own (looping `amb_<map>_lr`) become rooms,
    /// the default first (named after the map, else `ext_street`), each
    /// with its reverb; the game's state zones (shock, killcam) are left out.
    #[test]
    fn rooms_are_the_zones_that_loop_the_maps_own_tone() {
        use fastfile_iw7::walk::{ReverbRec, SndZoneRec, SoundTableRec};
        let z = |name: &str, stream: &str, reverb: &str, i: u16| SndZoneRec { name: name.into(), ambient_stream: stream.into(), reverb: reverb.into(), reverb_index: i, ..Default::default() };
        let table = SoundTableRec {
            zones: vec![
                z("flashbang_mp_shock", "", "flashbang_mp_shock_reverb", 0),
                z("geneva_lg_room", "amb_mp_geneva_lr", "lg_room", 1),
                z("geneva_ext_street", "amb_mp_geneva_lr", "ext_street", 2),
                z("helo_gunner", "ambient_helogunner_lr", "sm_room", 1),
            ],
            reverbs: vec![
                ReverbRec { room_type: "flash".into(), dry: 0.5, wet: 0.5, ..Default::default() },
                ReverbRec { room_type: "gen_lg_room".into(), dry: 1.0, wet: 0.9, ..Default::default() },
                ReverbRec { room_type: "gen_ext_street".into(), dry: 1.0, wet: 0.9, ..Default::default() },
            ],
            ..Default::default()
        };
        let fx = super::iw7_map_fx("mp_geneva", &table);
        assert_eq!(fx.rooms, vec![("geneva_ext_street".to_owned(), "gen_ext_street".to_owned(), 1.0, 0.9), ("geneva_lg_room".to_owned(), "gen_lg_room".to_owned(), 1.0, 0.9)]);
        assert_eq!(fx.room_tones, vec![("geneva_ext_street".to_owned(), "amb_mp_geneva_lr".to_owned()), ("geneva_lg_room".to_owned(), "amb_mp_geneva_lr".to_owned())]);
    }

    /// IW7's 17 preset values become a radverb's 16: the proximity late
    /// gain dropped, the rear level in place of the return high-pass.
    #[test]
    fn a_reverb_preset_becomes_a_radverb() {
        let v: [f32; 17] = std::array::from_fn(|i| i as f32);
        assert_eq!(super::iw7_radverb(&v), [0.0, 1.0, 2.0, 3.0, 4.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 0.0]);
    }

    /// The staged maps (`CODLEGACY_IW7_ZONE`, default
    /// `/mnt/user-data/uploads/iw_game_files`, with `<map>.sabl` beside):
    /// their rooms and tones read, and the default room's tone in the map's
    /// own bank with a clip.
    #[test]
    #[ignore = "needs staged IW7 map zones and banks"]
    fn staged_maps_rooms_and_tones() {
        let dir = std::path::PathBuf::from(
            codlegacy_env::var("CODLEGACY_IW7_ZONE")
                .unwrap_or_else(|_| "/mnt/user-data/uploads/iw_game_files".into()),
        );
        for map in ["mp_riot", "mp_geneva"] {
            let path = dir.join(format!("{map}.ff"));
            if !path.exists() || !dir.join(format!("{map}.sabl")).exists() {
                eprintln!("skipped {map}");
                continue;
            }
            let c = super::iw7_map_sounds(&path).expect("map sounds");
            let fx = c.scripted_map_fx.as_ref().expect("fx");
            println!("{map}: rooms {:?}", fx.rooms);
            let (room, tone) = fx.room_tones.first().expect("a tone");
            println!("{map}: default room {room} tone {tone}; {} room volumes; {} reverb presets", fx.room_volumes.len(), c.radverbs.len());
            let with_echo = fx.rooms.iter().filter(|r| c.radverbs.contains_key(&r.1)).count();
            println!("{map}: {with_echo} of {} rooms have their preset; {} = {:?}", fx.rooms.len(), fx.rooms[0].1, c.radverbs.get(&fx.rooms[0].1));
            assert_eq!(with_echo, fx.rooms.len(), "{map}: every room's preset is in the bank");
            assert!(fx.room_volumes.len() > 20, "{map}: room volumes");
            // (A trigger's own origin is inside its volume.)
            let v = &fx.room_volumes[fx.room_volumes.len() / 2];
            assert_eq!(fx.room_at(v.hulls[0].mid), Some(v.room.as_str()).or(fx.room_at(v.hulls[0].mid)));
            println!("{map}: {} at {:?}", v.room, v.hulls[0].mid);
            // (Riot's tone clip is in none of the banks staged here, not in
            // `mp_riot.sabs` (empty) or `patch_code_post_gfx.sabs`: probably
            // `code_post_gfx.sabs`, 8 GB, not staged.)
            let Some(s) = c.sound_in(crate::AssetNamespace::Iw7, tone) else {
                println!("{map}: {tone} has no clip in the staged banks");
                assert_ne!(map, "mp_geneva", "geneva's tone is in its own .sabs");
                continue;
            };
            let a = s.aliases.iter().find(|a| a.streamed.is_some()).expect("tone has a clip in the map's bank");
            let (d, n) = a.streamed.as_ref().unwrap();
            let bytes = super::read_iw7_stream(d, n).expect("clip reads");
            let f = sab_t6::decode_flac(&bytes).expect("tone decodes");
            println!("{map}: {tone}: {} bytes, {} Hz, {} ch, {:.1} s, vol {}..{}", bytes.len(), f.pcm.rate, f.pcm.channels, f.pcm.frames() as f64 / f64::from(f.pcm.rate), a.vol_min, a.vol_max);
            assert!(f.pcm.frames() > 44_100);
        }
    }
}

/// The rooms' volumes from a map's client triggers
/// (`fastfile_iw7::walk::ClientTriggersRec`): a trigger whose audio trigger is
/// set (types 2 and 66 on the maps read) names its sound zone by an offset
/// into the triggers' string table (geneva: 21 `geneva_ext_reflect`, 60
/// `geneva_med_room`); its hulls (box and slabs) are the volume. Triggers of
/// a zone that is not one of `rooms` are left out. Where volumes overlap
/// the smaller is tried first (the game's own order is not read).
pub fn iw7_room_volumes(image: &[u8], c: &fastfile_iw7::walk::ClientTriggersRec, rooms: &[String]) -> Vec<crate::createfx::RoomVolume> {
    let (Some(models), Some(hulls), Some(audio), Some(strings)) = (c.models, c.hulls, c.audio, c.strings) else { return Vec::new() };
    let get = |pos: usize, len: usize| image.get(pos..pos + len);
    let f = |at: usize| get(at, 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap_or_default()));
    let u16_at = |at: usize| get(at, 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let v3 = |at: usize| [f(at), f(at + 4), f(at + 8)];
    let table = get(strings.pos, strings.len).unwrap_or_default();
    let name_at = |off: usize| table.get(off..).map(|s| String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]).into_owned());
    let hull_count = hulls.len / 32;
    let slab_count = c.slabs.map_or(0, |s| s.len / 20);
    let mut out: Vec<(f32, crate::createfx::RoomVolume)> = Vec::new();
    for i in 0..c.count as usize {
        let a = u16_at(audio.pos + 2 * i);
        if a == u16::MAX || 2 * i + 2 > audio.len {
            continue;
        }
        let Some(room) = name_at(usize::from(a)).filter(|r| rooms.iter().any(|x| x == r)) else { continue };
        let m = models.pos + 32 * i;
        let (n, first) = (usize::from(u16_at(m + 4)), usize::from(u16_at(m + 6)));
        let mut volume = crate::createfx::RoomVolume { room, hulls: Vec::new() };
        let mut size = 0.0;
        for h in first..(first + n).min(hull_count) {
            let at = hulls.pos + 32 * h;
            let (sn, sf) = (usize::from(u16_at(at + 28)), usize::from(u16_at(at + 30)));
            let slabs = c
                .slabs
                .map(|s| (sf..(sf + sn).min(slab_count)).map(|k| (v3(s.pos + 20 * k), f(s.pos + 20 * k + 12), f(s.pos + 20 * k + 16))).collect())
                .unwrap_or_default();
            let half = v3(at + 12);
            size += half[0] * half[1] * half[2];
            volume.hulls.push(crate::createfx::RoomHull { mid: v3(at), half, slabs });
        }
        if !volume.hulls.is_empty() {
            out.push((size, volume));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, v)| v).collect()
}

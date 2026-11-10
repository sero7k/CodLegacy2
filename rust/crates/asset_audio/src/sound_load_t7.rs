//! bo2mp: Call of Duty: Black Ops III's weapon sounds for its guest guns in
//! a Black Ops II match.
//!
//! `fastfile_t7`'s walk reads `mp_common`'s sound bank (every multiplayer
//! gun's aliases: `wpn_ar_standard_fire_npc` and the layers its secondary
//! aliases chain to, the reload foley its animations' notetracks name,
//! `fly_arak_mag_out`). Each alias names its sound by `assetId`, an entry
//! of the `.sabl`/`.sabs` banks in `zone/snd/all` (`fastfile_t7::sab`):
//! every one a FLAC stream, named here by bank path and range (`<bank>`,
//! `<offset>:<length>`, as Ghosts' streamed sounds are) and decoded when
//! played, under the Black Ops III namespace.
//!
//! Of the alias record (0xd8) only the volume (u16 fractions at 0xb4), the
//! distances (u16 at 0xb8, 0xba) and the context pairs are read; the rest
//! of its flags and curves are left to Black Ops II's defaults.
//!
//! An alias list's records are not all variants of one sound: each carries
//! up to four (context type, value) pairs (u32 `SND_HashName`s at 0x74,
//! 0x7c, 0x84, 0x8c) and the game plays only the records whose contexts
//! match the listener's (`water`: `over`/`under`; `ringoff_plr`:
//! `outdoor`/`indoor`/`underwater`; `movement`: `normal`/`quiet`/`loud`;
//! `plr_stance`; `perspective`; `boot`). A gun's fire list keeps its
//! underwater shot beside the real one and its decay its underwater and
//! interior tails, so only the records of the dry, outdoor, normal context
//! are kept (`context_rows`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_t7::walk::capture::SoundAliasRec;

use crate::sound_catalog::{CapturedAlias, CapturedSound, SoundCatalog};
use crate::{AssetNamespace, ZoneGame, ZoneOwner};

/// The alias name prefixes kept (weapons' fire and handling, the foley
/// their notetracks play, projectiles and shell casings; bo2mp: the
/// out-of-bounds loops, `mpl_oob_static_lp` (`sim::t6::oob`)), with every
/// alias they chain to.
/// (bo2mp: `weap_`, the CoD Online 2 Workshop pack's guns' own.)
const KEEP_PREFIXES: &[&str] = &["wpn_", "fly_", "prj_", "fly_reload", "mpl_oob", "uin_out_of_bounds", "weap_"];

/// Black Ops III's multiplayer zone with the guns' sounds, when it is
/// installed (`zone/mp_common.ff` beside core_common).
pub fn bo3_mp_common(games: &crate::GamesRoot) -> Option<PathBuf> {
    let zone = asset_transport::find_zone_file_version(games, "mp_common", asset_transport::T7_ZONE_VERSION).ok()?;
    Some(zone.path)
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<SoundCatalog>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<SoundCatalog>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The weapon sounds of Black Ops III's mp_common (read once per process).
pub fn bo3_weapon_sounds(path: &Path) -> Result<Arc<SoundCatalog>, String> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(path).cloned()) {
        return Ok(hit);
    }
    let catalog = Arc::new(load_sound_catalog_t7(path)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(path.to_path_buf(), catalog.clone());
    }
    Ok(catalog)
}

/// The banks of a zone that open: the patch banks first (`<zone>.all.p.sabl`,
/// `.p.sabs`: the sounds a title update replaced), then `snd/all/<zone>.all.sabl`,
/// `.sabs`.
fn banks(zone_path: &Path) -> Vec<fastfile_t7::sab::SabFile> {
    let dir = zone_path.parent().unwrap_or(Path::new(".")).join("snd").join("all");
    let stem = zone_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // bo2mp: a Workshop mod's bank need not be named for its zone (Modern
    // Warfare 2019's pack: `zm_mod.ff`, `zm_weapon2019.all.sabl`): with
    // none named for it, every bank in the folder (never in Black Ops III's
    // own `zone` folder, beside core_common).
    let retail = zone_path.with_file_name("core_common.ff").is_file();
    let stems: Vec<String> = if retail || dir.join(format!("{stem}.all.sabl")).is_file() || !dir.is_dir() {
        vec![stem.clone()]
    } else {
        let mut v: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".all.sabl").map(str::to_owned))
            .collect();
        v.sort();
        v
    };
    // bo2mp: a language zone's bank (`en_zm_zod.ff`: the characters'
    // voice lines, `vox_plr_<n>_*`, `vox_shad_*`) is `<zone>.<lang>.sabl` /
    // `.sabs` (as `mp_common.en.sabs`), in any folder under `snd`.
    let mut localized: Vec<std::path::PathBuf> = Vec::new();
    if let Some((lang, rest)) = stem.split_once('_').filter(|(l, _)| l.len() == 2 && l.bytes().all(|b| b.is_ascii_lowercase())) {
        let snd = zone_path.parent().unwrap_or(Path::new(".")).join("snd");
        let dirs: Vec<std::path::PathBuf> = std::iter::once(snd.clone())
            .chain(std::fs::read_dir(&snd).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()))
            .collect();
        for d in dirs {
            for ext in ["sabl", "sabs"] {
                let p = d.join(format!("{rest}.{lang}.{ext}"));
                if p.is_file() {
                    localized.push(p);
                }
            }
        }
    }
    let localized = localized.into_iter().filter_map(|path| match fastfile_t7::sab::SabFile::open(&path) {
        Ok(b) => Some(b),
        Err(e) => {
            diag::warn!(Zone, "sound bank: BO3 {e}");
            None
        }
    });
    stems
        .iter()
        .flat_map(|stem| ["p.sabl", "p.sabs", "sabl", "sabs"].iter().map(move |ext| (stem, ext)))
        .filter_map(|(stem, ext)| {
            let path = dir.join(format!("{stem}.all.{ext}"));
            if ext.starts_with("p.") && !path.is_file() {
                return None;
            }
            match fastfile_t7::sab::SabFile::open(&path) {
                Ok(b) => Some(b),
                Err(e) => {
                    diag::warn!(Zone, "sound bank: BO3 {e}");
                    None
                }
            }
        })
        .chain(localized)
        .collect()
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// An alias record's (context type, value) pairs (types never 0).
fn contexts(record: &[u8]) -> impl Iterator<Item = (u32, u32)> + '_ {
    (0..4).map(move |k| (u32_at(record, 0x74 + 8 * k), u32_at(record, 0x78 + 8 * k))).filter(|&(t, _)| t != 0)
}

/// The context values a Black Ops II listener is in: above water, outdoors,
/// moving normally, standing, the player's own (and 0: the context unset,
/// `boot` dry, `plr_vehicle` on foot).
fn default_context(value: u32) -> bool {
    static DEFAULTS: OnceLock<[u32; 5]> = OnceLock::new();
    value == 0 || DEFAULTS.get_or_init(|| ["over", "outdoor", "normal", "stand", "player"].map(sab_t6::sound_hash)).contains(&value)
}

/// The context types whose listener value is known (`default_context`): a
/// list with none of its records in that value plays nothing (an indoor
/// tail heard outdoors).
fn known_context(t: u32) -> bool {
    static KNOWN: OnceLock<[u32; 5]> = OnceLock::new();
    KNOWN.get_or_init(|| ["water", "ringoff_plr", "movement", "plr_stance", "perspective"].map(sab_t6::sound_hash)).contains(&t)
}

/// The records of one alias list the game would play (see the module
/// docs): per context type named, the records of its default value; when
/// none has it, none of that type (the records without it, or nothing for
/// a known type), else the first record's value. A record without
/// contexts always plays.
fn context_rows(aliases: &[SoundAliasRec], rows: &[usize]) -> Vec<usize> {
    let mut chosen: Vec<(u32, Option<u32>)> = Vec::new();
    for &i in rows {
        for (t, _) in contexts(&aliases[i].record) {
            if chosen.iter().any(|&(c, _)| c == t) {
                continue;
            }
            let values = || rows.iter().filter_map(|&j| contexts(&aliases[j].record).find(|&(c, _)| c == t).map(|(_, v)| v));
            let untyped = rows.iter().any(|&j| contexts(&aliases[j].record).all(|(c, _)| c != t));
            let pick = values().find(|&v| default_context(v)).or_else(|| if untyped || known_context(t) { None } else { values().next() });
            chosen.push((t, pick));
        }
    }
    rows.iter()
        .copied()
        .filter(|&i| contexts(&aliases[i].record).all(|(t, v)| chosen.iter().any(|&(c, p)| c == t && p == Some(v))))
        .collect()
}

fn alias_row(a: &SoundAliasRec) -> CapturedAlias {
    let r = &a.record;
    let vol = |at: usize| f32::from(u16_at(r, at)) / 32767.0;
    let (vol_min, vol_max) = (vol(0xb4), vol(0xb6));
    let dist_min = f32::from(u16_at(r, 0xb8));
    let dist_max = f32::from(u16_at(r, 0xba).max(u16_at(r, 0xbc)));
    CapturedAlias {
        alias_name: a.name.clone(),
        secondary: Some(a.secondary.clone()).filter(|s| !s.is_empty()),
        vol_min: if vol_min > 0.0 { vol_min } else { 1.0 },
        vol_max: if vol_max > 0.0 { vol_max } else { 1.0 },
        pitch_min: 1.0,
        pitch_max: 1.0,
        dist_min,
        // (A gun heard across the map, as Black Ops II's are.)
        dist_max: if a.name.starts_with("wpn_") && a.name.ends_with("_npc") { dist_max.max(5000.0) } else { dist_max.max(dist_min + 1.0) },
        flags: None,
        probability: 1.0,
        ..CapturedAlias::default()
    }
}

/// One Black Ops III zone's weapon sound aliases (see the module docs).
pub fn load_sound_catalog_t7(path: &Path) -> Result<SoundCatalog, String> {
    let zone = fastfile_t7::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    if !matches!(w.stop, fastfile_t7::walk::Stop::Done { .. }) {
        diag::warn!(Zone, "sound bank: BO3 {} walked as far as {:?}", path.display(), w.stop);
    }
    let aliases = &w.capture.sound_aliases;
    let banks = banks(path);
    let owner = ZoneOwner::from_zone_path(path);
    // Lists by name (an alias list is its consecutive records of one name).
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, a) in aliases.iter().enumerate() {
        by_name.entry(a.name.as_str()).or_default().push(i);
    }
    let mut keep: BTreeSet<&str> = BTreeSet::new();
    let mut todo: Vec<&str> = by_name.keys().copied().filter(|n| KEEP_PREFIXES.iter().any(|p| n.starts_with(p))).collect();
    while let Some(n) = todo.pop() {
        if !keep.insert(n) {
            continue;
        }
        for i in context_rows(aliases, by_name.get(n).map(Vec::as_slice).unwrap_or(&[])) {
            let s = aliases[i].secondary.as_str();
            if !s.is_empty() && by_name.contains_key(s) {
                todo.push(s);
            }
        }
    }
    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    let (mut lists, mut playable, mut missing) = (0usize, 0usize, 0usize);
    for name in &keep {
        let mut rows = Vec::new();
        for i in context_rows(aliases, &by_name[name]) {
            let a = &aliases[i];
            let mut row = alias_row(a);
            let hit = banks.iter().find_map(|b| b.find(a.asset_id).map(|e| (b, *e)));
            match hit.filter(|(_, e)| e.format == 8 && e.size > 0) {
                Some((bank, e)) => {
                    row.file_type = Some(2);
                    row.file_exists = Some(1);
                    row.streamed = Some((bank.path.to_string_lossy().into_owned(), format!("{}:{}", e.offset, e.size)));
                    row.flags = Some(u32::from(e.looping));
                    playable += 1;
                }
                None => {
                    missing += 1;
                    row.file_type = Some(1);
                    row.file_name = Some("null".to_owned());
                }
            }
            rows.push(row);
        }
        lists += 1;
        catalog.ingest_sound(CapturedSound {
            name: (*name).to_owned(),
            aliases: rows,
            game: ZoneGame::T6,
            zone: owner,
            namespace: Some(AssetNamespace::T7),
        });
    }
    catalog.finalize();
    diag::info!(
        Zone,
        "sound bank: BO3 {}: {lists} weapon alias lists of {} aliases, {playable} with a clip, {missing} not in the banks",
        path.display(),
        aliases.len(),
    );
    Ok(catalog)
}

/// bo2mp: whether a Black Ops III map plays its own ambience
/// (`BO2MP_T7_AMBIENCE=0` off; on by default).
pub fn t7_map_ambience_enabled() -> bool {
    !std::env::var("BO2MP_T7_AMBIENCE").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

/// bo2mp: a Black Ops III map's ambience, from its own zone and banks.
///
/// The map bank (`<map>.all`, `SndBank` in the map zone) holds the map's
/// aliases (room tones `amb_wind_exterior_2d`, emitters `amb_gust_cold`,
/// destruction) and its ambient rooms (0x184 records,
/// `fastfile_t7::walk::capture::SoundRoomRec`): name, radverb, dry, wet,
/// the room tone's alias id and the snapshot; word 0x44 set on the map's
/// default room (`mp_metro_outdoor`). The map's `ambient_package` triggers
/// (`script_ambientroom`, `script_ambientpriority`) choose the room (the
/// server already follows them for Black Ops II maps: client dvar
/// `bo2zm_room`); its `script_struct`s with `script_sound` are the same
/// emitters Black Ops II's client script runs (`random` with
/// `script_wait_min/max`, `looper`, `line_emitter` to its target).
pub fn bo3_map_sounds(path: &Path) -> Result<SoundCatalog, String> {
    let zone = fastfile_t7::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    if !matches!(w.stop, fastfile_t7::walk::Stop::Done { .. }) {
        diag::warn!(Zone, "sound bank: BO3 map {} walked as far as {:?}", path.display(), w.stop);
    }
    let mut aliases: Vec<fastfile_t7::walk::capture::SoundAliasRec> = w.capture.sound_aliases.clone();
    // (The map's banks, then mp_common's: the shared ambience, the room
    // tones `amb_wind_exterior_2d` / `_interior_2d`, lives there.)
    let mut map_banks = banks(path);
    // bo2mp: a Zombies map's sounds are also in its patch zone and the
    // shared Zombies zones (Shadows of Evil's round-start music states
    // `mus_zod_roundstart_<n>_intro` are only in `zm_zod_patch`; the
    // shared stingers and VO in `zm_patch` / `zm_common`): their aliases
    // (a name the map zone has stays the map's; the patch before the
    // common zones) and their banks.
    for zone in zm_sound_zones(path) {
        match fastfile_t7::Zone::open(&zone) {
            Ok(z) => {
                let more = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
                let have: std::collections::HashSet<String> = aliases.iter().map(|a| a.name.clone()).collect();
                let added: Vec<_> = more.capture.sound_aliases.into_iter().filter(|a| !have.contains(&a.name)).collect();
                diag::info!(Zone, "sound bank: BO3 map {}: {} aliases from {}", path.display(), added.len(), zone.display());
                aliases.extend(added);
                map_banks.extend(banks(&zone));
            }
            Err(e) => diag::warn!(Zone, "sound bank: BO3 map {}: {} not read ({e})", path.display(), zone.display()),
        }
    }
    map_banks.extend(banks(&path.with_file_name("mp_common.ff")));
    let banks = map_banks;
    let aliases = &aliases;
    let owner = ZoneOwner::from_zone_path(path);
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut names_by_id: HashMap<u32, String> = HashMap::new();
    for (i, a) in aliases.iter().enumerate() {
        by_name.entry(a.name.as_str()).or_default().push(i);
        names_by_id.entry(u32_at(&a.record, 8)).or_insert_with(|| a.name.clone());
    }
    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(owner);
    let (mut lists, mut playable, mut missing) = (0usize, 0usize, 0usize);
    let mut names: Vec<&str> = by_name.keys().copied().collect();
    names.sort_unstable();
    for name in names {
        let mut rows = Vec::new();
        for i in context_rows(aliases, &by_name[name]) {
            let a = &aliases[i];
            let mut row = alias_row(a);
            match banks.iter().find_map(|b| b.find(a.asset_id).map(|e| (b, *e))).filter(|(_, e)| e.format == 8 && e.size > 0) {
                Some((bank, e)) => {
                    row.file_type = Some(2);
                    row.file_exists = Some(1);
                    row.streamed = Some((bank.path.to_string_lossy().into_owned(), format!("{}:{}", e.offset, e.size)));
                    row.flags = Some(u32::from(e.looping));
                    playable += 1;
                }
                None => {
                    missing += 1;
                    row.file_type = Some(1);
                    row.file_name = Some("null".to_owned());
                }
            }
            rows.push(row);
        }
        lists += 1;
        catalog.ingest_sound(CapturedSound { name: name.to_owned(), aliases: rows, game: ZoneGame::T6, zone: owner, namespace: Some(AssetNamespace::T7) });
    }
    let entities = w
        .capture
        .world
        .map_ents
        .first()
        .and_then(|m| m.entity_string)
        .and_then(|(at, n)| zone.image.get(at..at + n))
        .map(|raw| String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())]).into_owned())
        .unwrap_or_default();
    let fx = t7_map_fx(&entities, &w.capture.sound_rooms, &names_by_id);
    diag::info!(
        Zone,
        "sound bank: BO3 map {}: {lists} alias lists ({playable} with a clip, {missing} not in the banks); rooms {:?}; tones {:?}; {} loops, {} now-and-then sounds",
        path.display(),
        fx.rooms,
        fx.room_tones,
        fx.loop_sounds.len(),
        fx.random_sounds.len()
    );
    catalog.scripted_map_fx = Some(fx);
    catalog.finalize();
    Ok(catalog)
}

/// bo2mp: the zones beside a Black Ops III Zombies map's that hold its
/// other sounds, in the order a name is taken from: `<map>_patch`,
/// `zm_patch`, `zm_common`, then the English language zones of the map and
/// those (`en_<map>`: the characters' and the Shadow Man's voice lines;
/// those installed beside it); none for another map.
pub fn zm_sound_zones(map_zone: &Path) -> Vec<PathBuf> {
    let stem = map_zone.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !stem.starts_with("zm_") || stem.ends_with("_patch") || stem == "zm_common" || stem == "zm_patch" {
        return Vec::new();
    }
    // (And their language zones, the voice lines: `en_<map>`, ...)
    let plain = [format!("{stem}_patch"), "zm_patch".to_owned(), "zm_common".to_owned()];
    let lang: Vec<String> = std::iter::once(stem.clone()).chain(plain.iter().cloned()).map(|s| format!("en_{s}")).collect();
    plain
        .into_iter()
        .chain(lang)
        .map(|s| map_zone.with_file_name(format!("{s}.ff")))
        .filter(|p| p.is_file())
        .collect()
}

/// The rooms, room tones and emitters of a Black Ops III map (see
/// [`bo3_map_sounds`]): the default room first, its killcam room left out;
/// a tone whose alias id is not among the zone's aliases is left out.
pub fn t7_map_fx(entities: &str, rooms: &[fastfile_t7::walk::capture::SoundRoomRec], names_by_id: &HashMap<u32, String>) -> crate::createfx::ScriptedMapFx {
    let is_default = |r: &fastfile_t7::walk::capture::SoundRoomRec| r.record.get(0x44..0x48).is_some_and(|b| b != [0, 0, 0, 0]);
    let mut ordered: Vec<&fastfile_t7::walk::capture::SoundRoomRec> = rooms.iter().filter(|r| r.name != "final_kill_cam").collect();
    ordered.sort_by_key(|r| !is_default(r));
    let mut fx = crate::createfx::ScriptedMapFx::default();
    for r in ordered {
        if fx.rooms.iter().any(|x| x.0 == r.name) {
            continue;
        }
        fx.rooms.push((r.name.clone(), r.radverb.clone(), r.dry, r.wet));
        if let Some(alias) = names_by_id.get(&r.tone_id) {
            fx.room_tones.push((r.name.clone(), alias.clone()));
        }
    }
    let ents = asset_t6::parse_entities(entities);
    for e in &ents {
        if e.classname() != "script_struct" {
            continue;
        }
        let (Some(alias), Some(origin)) = (e.get("script_sound"), e.vec3("origin")) else { continue };
        let alias = alias.to_owned();
        match e.get("script_label") {
            Some("random") => {
                let f = |k: &str, d: f32| e.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
                fx.random_sounds.push(crate::createfx::RandomPointSound { soundalias: alias, wait_secs: [f("script_wait_min", 1.0), f("script_wait_max", 3.0)], places_inches: vec![origin] });
            }
            Some("looper") => fx.loop_sounds.push(crate::createfx::CreateFxLoopSound { soundalias: alias, origin_inches: origin, line_end_inches: None }),
            Some("line_emitter") => {
                let end = e.get("target").and_then(|t| ents.iter().find(|o| o.get("targetname") == Some(t)).and_then(|o| o.vec3("origin"))).or_else(|| e.vec3("targetOrigin"));
                fx.loop_sounds.push(crate::createfx::CreateFxLoopSound { soundalias: alias, origin_inches: origin, line_end_inches: end });
            }
            _ => {}
        }
    }
    fx
}

#[cfg(test)]
mod tests {
    /// `CODLEGACY_GAMES=<root> cargo test -p asset_audio bo3_sounds -- --ignored
    /// --nocapture`: the BO3 guns' sounds read and one of each gun's fire
    /// aliases decoded.
    fn room(name: &str, radverb: &str, dry: f32, tone: u32, default: bool) -> fastfile_t7::walk::capture::SoundRoomRec {
        let mut record = vec![0u8; 0x184];
        if default {
            record[0x44] = 1;
        }
        fastfile_t7::walk::capture::SoundRoomRec { name: name.into(), radverb: radverb.into(), dry, wet: 1.0, tone_id: tone, record, ..Default::default() }
    }

    /// A Zombies map's other sound zones: its patch, the shared Zombies
    /// zones, then their English zones, those installed; none for a
    /// multiplayer map or a patch zone itself.
    #[test]
    fn zombies_sound_zones() {
        let dir = std::env::temp_dir().join(format!("bo2mp_zm_snd_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for z in ["zm_zod", "zm_zod_patch", "zm_common", "en_zm_zod", "en_zm_common", "mp_metro"] {
            std::fs::write(dir.join(format!("{z}.ff")), b"").unwrap();
        }
        let names = |p: &str| super::zm_sound_zones(&dir.join(p)).iter().map(|x| x.file_stem().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names("zm_zod.ff"), ["zm_zod_patch", "zm_common", "en_zm_zod", "en_zm_common"]);
        assert!(names("mp_metro.ff").is_empty());
        assert!(names("zm_zod_patch.ff").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Metro's rooms as its bank holds them and three of its structs: the
    /// default room first, the killcam room left out, each room's tone by
    /// its alias id, a random sound, a looper and a line emitter.
    #[test]
    fn metro_rooms_and_emitters() {
        let rooms = [
            room("final_kill_cam", "mp_final_killcam", 1.0, 0xf54926b1, false),
            room("mp_metro_main_train_rm", "global_hangar", 0.7, 0x738305f8, false),
            room("mp_metro_outdoor", "global_urban_outdoor", 1.0, 0xc683666a, true),
            room("mp_metro_underwater", "global_underwater", 1.0, 0xbfecd250, false),
        ];
        let ids: super::HashMap<u32, String> = [(0x738305f8, "amb_wind_interior_2d"), (0xc683666a, "amb_wind_exterior_2d")].into_iter().map(|(k, v)| (k, v.to_owned())).collect();
        let ents = r#"{ "classname" "worldspawn" }
{ "script_wait_max" "8" "script_wait_min" "2" "script_label" "random" "script_sound" "amb_gust_cold" "origin" "1 2 3" "classname" "script_struct" }
{ "script_sound" "amb_water_lap" "script_looping" "1" "script_label" "looper" "origin" "4 5 6" "classname" "script_struct" }
{ "target" "e1" "script_sound" "amb_air_r" "script_label" "line_emitter" "origin" "0 0 0" "classname" "script_struct" }
{ "targetname" "e1" "origin" "10 0 0" "classname" "script_struct" }"#;
        let fx = super::t7_map_fx(ents, &rooms, &ids);
        assert_eq!(fx.rooms[0], ("mp_metro_outdoor".to_owned(), "global_urban_outdoor".to_owned(), 1.0, 1.0));
        assert_eq!(fx.rooms.len(), 3);
        assert_eq!(fx.room_tones, vec![("mp_metro_outdoor".to_owned(), "amb_wind_exterior_2d".to_owned()), ("mp_metro_main_train_rm".to_owned(), "amb_wind_interior_2d".to_owned())]);
        assert_eq!(fx.random_sounds.len(), 1);
        assert_eq!(fx.random_sounds[0].wait_secs, [2.0, 8.0]);
        assert_eq!(fx.loop_sounds.len(), 2);
        assert_eq!(fx.loop_sounds[1].line_end_inches, Some([10.0, 0.0, 0.0]));
    }

    /// The staged Metro zone and bank (`T7_ZONE_DIR`, default
    /// `/mnt/user-data/uploads/zone`, with `snd/all/mp_metro.all.sabl`):
    /// its rooms, tones and emitters read, and the default room's tone
    /// decoded from the bank.
    #[test]
    fn metro_map_sounds() {
        let dir = std::path::PathBuf::from(std::env::var("T7_ZONE_DIR").unwrap_or_else(|_| "/mnt/user-data/uploads/zone".into()));
        let zone = dir.join("mp_metro.ff");
        if !zone.exists() || !dir.join("snd/all/mp_metro.all.sabl").exists() {
            return eprintln!("skipped: no staged mp_metro");
        }
        let c = super::bo3_map_sounds(&zone).expect("map sounds");
        let fx = c.scripted_map_fx.as_ref().expect("fx");
        println!("rooms {:?}\ntones {:?}\nloops {} randoms {}", fx.rooms, fx.room_tones, fx.loop_sounds.len(), fx.random_sounds.len());
        assert_eq!(fx.rooms[0].0, "mp_metro_outdoor");
        assert!(fx.room_tones.iter().any(|(r, a)| r == "mp_metro_outdoor" && a == "amb_wind_exterior_2d"));
        assert!(fx.loop_sounds.len() > 20 && fx.random_sounds.len() > 200, "{} {}", fx.loop_sounds.len(), fx.random_sounds.len());
        // (The tones' clips are in mp_common's banks, staged or not.)
        let tone = c.sound_in(crate::AssetNamespace::T7, "amb_wind_exterior_2d").expect("tone alias");
        println!("tone amb_wind_exterior_2d: in the staged banks {}", tone.aliases.iter().any(|a| a.streamed.is_some()));
        let gust = c.sound_in(crate::AssetNamespace::T7, "amb_gust_cold").expect("emitter alias");
        let a = gust.aliases.iter().find(|a| a.streamed.is_some()).expect("in the map's bank");
        let (dir, n) = a.streamed.as_ref().unwrap();
        let f = crate::read_ghosts_stream(dir, n).and_then(|b| sab_t6::decode_flac(&b).map_err(|e| e.to_string())).expect("decodes");
        println!("amb_gust_cold: {} Hz, {} ch, {} frames, vol {}, dist {}..{}", f.pcm.rate, f.pcm.channels, f.pcm.frames(), a.vol_min, a.dist_min, a.dist_max);
        assert!(f.pcm.frames() > 1000);
        // Every emitter's alias is in the map's bank with a clip.
        let unresolved: Vec<&str> = fx.loop_sounds.iter().map(|l| l.soundalias.as_str()).chain(fx.random_sounds.iter().map(|r| r.soundalias.as_str())).filter(|n| c.sound_in(crate::AssetNamespace::T7, n).is_none_or(|s| s.aliases.iter().all(|a| a.streamed.is_none()))).collect();
        let mut u = unresolved.clone();
        u.sort_unstable();
        u.dedup();
        println!("emitter aliases without a clip: {} placements of {u:?}", unresolved.len());
        assert!(unresolved.len() * 10 < fx.loop_sounds.len() + fx.random_sounds.len());
    }

    #[test]
    #[ignore]
    fn bo3_sounds() {
        let games = crate::games_root_from_env().expect("games root");
        let path = super::bo3_mp_common(&games).expect("Black Ops III's mp_common");
        let c = super::load_sound_catalog_t7(&path).expect("sounds");
        println!("{} lists", c.sounds.len());
        for name in ["wpn_ar_standard_fire_npc", "wpn_ar_standard_fire_plr", "fly_arak_mag_out", "wpn_ar_accurate_fire_plr"] {
            let Some(s) = c.sound_in(crate::AssetNamespace::T7, name) else {
                println!("{name}: not kept");
                continue;
            };
            for a in &s.aliases {
                let decoded = a.streamed.as_ref().map(|(dir, n)| {
                    crate::read_ghosts_stream(dir, n).and_then(|b| sab_t6::decode_flac(&b).map(|f| (f.pcm.rate, f.pcm.channels, f.pcm.frames())).map_err(|e| e.to_string()))
                });
                println!(
                    "{name}: secondary {:?} vol {}..{} dist {}..{} flags {:?} -> {decoded:?}",
                    a.secondary, a.vol_min, a.vol_max, a.dist_min, a.dist_max, a.flags
                );
            }
        }
    }
}

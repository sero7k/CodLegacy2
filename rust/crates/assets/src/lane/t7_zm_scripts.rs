//! bo2mp: a Black Ops III Zombies map played with Black Ops III's own
//! scripts (on by default; `BO2MP_T7_SCRIPTS=0` turns them off; see
//! `docs/T7_ZOMBIES_PLAN.md`). The compiled scripts are only in the
//! patch zones: `core_patch`, `zm_patch` and `<map>_patch` hold every one
//! Shadows of Evil links (755 objects). They go to the match as raw
//! objects (`T6ScriptSet::t7_objects`, translated by `sim::t6::t7`), with
//! the map's own entity string in place of the Black Ops II kit's
//! translation. The kit (zombies' models and animations, weapons,
//! sounds) stays until Black Ops III's own are read.

use crate::script_sources::T6ScriptSet;

// The nav mesh's packfile reader (`nav_t7`, included: no new crate).
#[path = "../../../nav_t7/src/hkx.rs"]
#[allow(dead_code)]
mod hkx;

/// The Havok packfiles in a zone image that hold a nav mesh, one after
/// another.
fn nav_packfiles(image: &[u8]) -> Vec<u8> {
    hkx::find_all(image).into_iter().filter(|p| !p.objects_of("hkaiNavMesh").is_empty()).flat_map(|p| p.bytes.to_vec()).collect()
}

/// A zone's string tables (`tablelookup` & co. read them: the scripts'
/// weapons, levels and stats tables) after the kit's, so Black Ops III's
/// win a name.
fn add_tables(set: &mut T6ScriptSet, z: &fastfile_t7::Zone) -> usize {
    let w = fastfile_t7::walk::walk_with(z, fastfile_t7::walk::Options { resync: true, keep_anims: false, ..Default::default() });
    let n = w.capture.tables.len();
    for (name, cols, cells) in w.capture.tables {
        let rows = cells.len() / cols.max(1);
        set.tables.push((name, cols, rows, cells));
    }
    // (And its script bundles: `level.scriptbundles`, the scenes' and
    // other systems' settings.)
    let strings = &z.assets.script_strings;
    let ss = |i: u32| strings.get(i as usize).cloned().flatten().unwrap_or_else(String::new);
    for ((ty, name), links) in &w.capture.links {
        if *ty != fastfile_t7::asset_type::AssetType::ScriptBundle {
            continue;
        }
        let mut pairs = Vec::new();
        for (label, v) in links {
            let mut parts = label.split('#');
            let (who, key, kind) = (parts.next().unwrap_or(""), parts.next().and_then(|k| k.parse().ok()).unwrap_or(0), parts.next().and_then(|k| k.parse().ok()).unwrap_or(0));
            let object: i32 = who.strip_prefix("object").and_then(|n| n.parse().ok()).unwrap_or(-1);
            let (value, data) = match v {
                fastfile_t7::walk::capture::FieldValue::Str(t) if t.starts_with("ss:") => {
                    let mut w = t[3..].split(' ');
                    let n: u32 = w.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let d = w.next().and_then(|x| x.strip_prefix("data:0x")).and_then(|x| u32::from_str_radix(x, 16).ok()).unwrap_or(0);
                    (ss(n), d)
                }
                fastfile_t7::walk::capture::FieldValue::Asset { name, .. } => (name.clone(), 0),
                fastfile_t7::walk::capture::FieldValue::Str(t) => (t.clone(), 0),
            };
            pairs.push((object, ss(key), kind, value, data));
        }
        set.t7_bundles.push((name.clone(), pairs));
    }
    n
}

/// The scripts are wanted: on by default for every Black Ops III Zombies
/// map; `BO2MP_T7_SCRIPTS=0` turns them off (the Black Ops II kit's
/// scripts then run on the map, as before).
pub(crate) fn enabled() -> bool {
    enabled_with(std::env::var("BO2MP_T7_SCRIPTS").ok().as_deref())
}

fn enabled_with(v: Option<&str>) -> bool {
    v.is_none_or(|v| !matches!(v.trim(), "0" | "off" | "false"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn black_ops_iii_scripts_are_on_unless_switched_off() {
        assert!(super::enabled_with(None));
        assert!(super::enabled_with(Some("1")));
        assert!(!super::enabled_with(Some("0")));
        assert!(!super::enabled_with(Some(" off ")));
        // (A Workshop map has no `<map>_patch`; the map zone is last.)
        let z = super::script_zones("zm_prison");
        assert_eq!(z, ["core_patch", "zm_patch", "zm_levelcommon", "zm_prison", "zm_prison_patch"]);
        assert_eq!(super::script_zones("zm_zod"), ["core_patch", "zm_patch", "zm_zod", "zm_zod_patch"]);
    }
}

/// The zones that hold a map's scripts, in load order.
fn script_zones(map: &str) -> Vec<String> {
    // (Every map but Shadows of Evil also takes the shared level zone's:
    // the perks, power-ups and wonder weapons its patch does not repeat.)
    let mut zones = vec!["core_patch".to_owned(), "zm_patch".to_owned()];
    if map != "zm_zod" {
        zones.push("zm_levelcommon".to_owned());
    }
    // (The map zone's own scripts, before its patch: the ones no patch
    // repeats, e.g. Gorod Krovi's sentinel drone, dragon whelp and Mangler.
    // A community map, a Steam Workshop item or a usermap, has no patch
    // zone: the mod tools compile its scripts into its own zone, at the
    // patches' version, 0x1c.)
    zones.push(map.to_owned());
    if asset_transport::workshop::bo3_map(map).is_none() {
        zones.push(format!("{map}_patch"));
    }
    zones
}

/// Each compiled script object in a zone image (by its magic; cut at the
/// next one, or 2 MB on: the object's tables are read by their offsets).
fn objects_in(image: &[u8]) -> Vec<Vec<u8>> {
    const MAGIC: &[u8; 8] = b"\x80GSC\r\n\x00\x1c";
    let mut starts = Vec::new();
    let mut i = 0;
    while let Some(p) = image[i..].windows(8).position(|w| w == MAGIC) {
        starts.push(i + p);
        i += p + 8;
    }
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let end = starts.get(k + 1).copied().unwrap_or(image.len()).min(s + (2 << 20));
            image[s..end].to_vec()
        })
        .collect()
}

/// Put the map's scripts and its own entities into the set the kit made.
pub(crate) fn attach(set: &mut T6ScriptSet, own_entities: Option<String>, map: &str, report: &mut Vec<String>) {
    let root = match asset_transport::games_root_from_env() {
        Ok(r) => r,
        Err(e) => {
            report.push(format!("t7 scripts gap: no games root ({e})"));
            return;
        }
    };
    let mut objects = Vec::new();
    for zone in script_zones(map) {
        let found = match asset_transport::find_zone_file(&root, &format!("t7:{zone}")) {
            Ok(f) => f,
            // (A Workshop map is one zone with its scripts inside: it has no
            // `<map>_patch`; only the stock zones are required.)
            Err(_) if zone == format!("{map}_patch") => {
                report.push(format!("t7 scripts: {zone} not present (a Workshop map keeps its scripts in {map})"));
                continue;
            }
            Err(e) => {
                report.push(format!("t7 scripts gap: {zone} not found ({e}); the Black Ops II kit's scripts run"));
                return;
            }
        };
        match fastfile_t7::Zone::open(&found.path) {
            Ok(z) => {
                let before = objects.len();
                objects.extend(objects_in(&z.image));
                // (The map zone's tables are added with its nav mesh below.)
                let t = if zone == map { 0 } else { add_tables(set, &z) };
                report.push(format!("t7 scripts: {zone}: {} objects, {t} string tables", objects.len() - before));
            }
            Err(e) => {
                report.push(format!("t7 scripts gap: {zone} not read ({e}); the Black Ops II kit's scripts run"));
                return;
            }
        }
    }
    let Some(entities) = own_entities else {
        report.push("t7 scripts gap: the map's entities were not kept; the Black Ops II kit's scripts run".to_owned());
        return;
    };
    set.t7_objects = latest_by_name(objects);
    set.entities = vec![entities];
    // The map's nav mesh (its zone, not the patch).
    match asset_transport::find_zone_file(&root, &format!("t7:{map}")).map_err(|e| e.to_string()).and_then(|f| fastfile_t7::Zone::open(&f.path).map_err(|e| e.to_string())) {
        Ok(z) => {
            set.t7_nav = nav_packfiles(&z.image);
            let t = add_tables(set, &z);
            report.push(format!("t7 scripts: {map} nav mesh packfiles: {} bytes, {t} string tables", set.t7_nav.len()));
        }
        Err(e) => report.push(format!("t7 scripts gap: {map} not read for its nav mesh ({e}); zombies stand where they spawn")),
    }
    let n = add_localize(set, &root, map, report);
    report.push(format!("t7 scripts: {n} Black Ops III strings (hints, weapon and perk names)"));
}

/// Black Ops III's English strings for a Zombies map: the use hints
/// ("Hold [{+activate}] to use Zipline [Cost: &&1]"), the shared Zombies
/// ones (`ZOMBIE_BUTTON_BUY_*`) and the core's (weapon and perk names), from
/// the `en_` zones' localize entries, after the kit's so Black Ops III's
/// text wins a key both have. Returns how many were read.
fn add_localize(set: &mut T6ScriptSet, root: &asset_transport::GamesRoot, map: &str, report: &mut Vec<String>) -> usize {
    let mut n = 0;
    for zone in ["en_core_patch".to_owned(), "en_zm_patch".to_owned(), "en_zm_levelcommon".to_owned(), format!("en_{map}"), format!("en_{map}_patch")] {
        let Ok(found) = asset_transport::find_zone_file(root, &format!("t7:{zone}")) else { continue };
        let Ok(z) = fastfile_t7::Zone::open(&found.path) else {
            report.push(format!("t7 scripts gap: {zone} not read for its strings"));
            continue;
        };
        let w = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { resync: true, keep_anims: true, ..Default::default() });
        n += w.capture.localize.len();
        set.strings.extend(w.capture.localize);
    }
    n
}

/// An object's script name (its header's name offset, word 11 after the
/// 8-byte magic).
fn object_name(bytes: &[u8]) -> Option<&[u8]> {
    let off = u32::from_le_bytes(bytes.get(0x34..0x38)?.try_into().ok()?) as usize;
    let rest = bytes.get(off..)?;
    Some(&rest[..rest.iter().position(|&b| b == 0)?])
}

/// The zones' objects with a later zone's object replacing an earlier one
/// of the same name (the map's patch over the shared level zone), in the
/// order each name first came.
fn latest_by_name(objects: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut order: Vec<Vec<u8>> = Vec::new();
    let mut latest: std::collections::HashMap<Vec<u8>, Vec<u8>> = std::collections::HashMap::new();
    let mut unnamed = Vec::new();
    for o in objects {
        match object_name(&o).map(<[u8]>::to_vec) {
            Some(n) => {
                if !latest.contains_key(&n) {
                    order.push(n.clone());
                }
                latest.insert(n, o);
            }
            None => unnamed.push(o),
        }
    }
    order.into_iter().filter_map(|n| latest.remove(&n)).chain(unnamed).collect()
}

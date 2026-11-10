//! bo2mp: which of a Black Ops III map's entity blocks the game spawns, and
//! as what, by the exe's own rules (its map-load entity spawn, read from
//! the November 2024 PC build; see docs/T7_NATIVE.md "Map logic").
//!
//! The exe walks the map's entity string block by block and, for each
//! block's `classname` (its `spawnflags` read as a number):
//!
//! 1. a spawn point (`info_player_start`, or in a multiplayer game an
//!    `mp_*` class containing `_spawn`, `_respawn` or `_intermission`;
//!    `cp_*` in a campaign game) goes to the spawn point list;
//! 2. `dyn_*` is dropped; `node_*` becomes a path node; `actor_*` an actor;
//!    `zbarrier_*` a Zombies barrier; `scriptbundle_*` (any case) goes to
//!    the script bundle system; `info_vehicle_node[_rotate]` to the vehicle
//!    paths; `weapon_<weapon>` a weapon pickup;
//! 3. a client-only model is not spawned on the server: a `script_model`
//!    without a destructible whose `client_server` is `ClientSide` or whose
//!    spawnflags have bit 2, a `script_origin` with spawnflags bit 2 or
//!    `clientside` 1;
//! 4. these are dropped: `occlusion_override`, `rope*`, `umbra_volume`,
//!    `nav_volume`, `volume_fpstool`, `wave_water`, `glass*`, `fx`;
//!    `heli_height_lock` sets the helicopter height limit (no entity);
//! 5. a class in the spawnable table or the map-only table spawns an
//!    entity (`script_struct` becomes a script struct);
//! 6. anything else has no spawn function and is freed (`volume_worldfog`,
//!    `volume_litfog`, `volume_sun`, `nav_volume_passable`, ...: the client
//!    or the tools read those, the game does not).

/// What the game does with an entity block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T7Spawn {
    /// An entity (with the spawn function of its class).
    Entity,
    /// A script struct.
    Struct,
    /// A spawn point (the spawn point list, not an entity).
    SpawnPoint,
    /// A path node.
    PathNode,
    /// Read by another system, no entity: script bundles, vehicle nodes,
    /// the helicopter height lock.
    Consumed,
    /// Drawn by the client only.
    ClientOnly,
    /// Never spawned.
    Dropped,
    /// No spawn function: freed.
    NoSpawnFunction,
    /// The world (`worldspawn`, read before the entities).
    World,
}

/// Classes `spawn()` can make (the spawnable table, also searched first at
/// map load).
pub const SPAWNABLE: [&str; 12] = [
    "info_notnull",
    "info_notnull_big",
    "info_volume",
    "trigger_radius",
    "trigger_box",
    "trigger_box_use",
    "trigger_radius_use",
    "trigger_damage",
    "trigger_radius_hurt",
    "trigger_radius_out_of_bounds",
    "script_model",
    "script_origin",
];

/// Classes only a map can place (the map-only table).
pub const MAP_ONLY: [&str; 17] = [
    "trigger_use",
    "trigger_use_touch",
    "trigger_multiple",
    "trigger_disk",
    "trigger_hurt",
    "trigger_once",
    "trigger_lookat",
    "trigger_out_of_bounds",
    "trigger_ik_playerclip_terrain",
    "trigger_navmesh",
    "light",
    "lensflare",
    "reflection_probe",
    "script_brushmodel",
    "script_struct",
    "script_vehicle",
    "undertow_volume",
];

/// A key's value in a block (`None` when absent).
fn value<'a>(keys: &'a [(String, String)], key: &str) -> Option<&'a str> {
    keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn is_spawn_point(classname: &str, mp: bool) -> bool {
    let prefix = if mp { "mp_" } else { "cp_" };
    classname == "info_player_start"
        || (classname.starts_with(prefix)
            && (classname.contains("_spawn") || classname.contains("_respawn") || classname.contains("_intermission")))
}

/// What the game does with a block of keys (`mp`: a multiplayer game;
/// else a campaign one).
#[must_use]
pub fn spawn_class(keys: &[(String, String)], mp: bool) -> T7Spawn {
    let Some(classname) = value(keys, "classname") else {
        return T7Spawn::Dropped;
    };
    let spawnflags: i32 = value(keys, "spawnflags").map_or(0, |v| v.trim().parse().unwrap_or(0));
    if classname == "worldspawn" {
        return T7Spawn::World;
    }
    if is_spawn_point(classname, mp) {
        return T7Spawn::SpawnPoint;
    }
    if classname.starts_with("dyn_") {
        return T7Spawn::Dropped;
    }
    if classname.starts_with("node_") {
        return T7Spawn::PathNode;
    }
    if classname.starts_with("actor_") || classname.starts_with("zbarrier_") || classname.starts_with("weapon_") {
        return T7Spawn::Entity;
    }
    if classname.len() >= 13 && classname[..13].eq_ignore_ascii_case("scriptbundle_") {
        return T7Spawn::Consumed;
    }
    if matches!(classname, "info_vehicle_node" | "info_vehicle_node_rotate" | "heli_height_lock") {
        return T7Spawn::Consumed;
    }
    // (Client-only models and origins.)
    if classname == "script_model" && value(keys, "destructibledef").is_none_or(str::is_empty) {
        if value(keys, "client_server") == Some("ClientSide") || spawnflags & 2 != 0 {
            return T7Spawn::ClientOnly;
        }
    }
    if classname == "script_origin"
        && (spawnflags & 2 != 0 || value(keys, "clientside").and_then(|v| v.trim().parse::<f32>().ok()) == Some(1.0))
    {
        return T7Spawn::ClientOnly;
    }
    if matches!(classname, "occlusion_override" | "umbra_volume" | "nav_volume" | "volume_fpstool" | "wave_water" | "fx")
        || classname.starts_with("rope")
        || classname.starts_with("glass")
    {
        return T7Spawn::Dropped;
    }
    if classname == "script_struct" {
        return T7Spawn::Struct;
    }
    if SPAWNABLE.contains(&classname) || MAP_ONLY.contains(&classname) {
        return T7Spawn::Entity;
    }
    T7Spawn::NoSpawnFunction
}

/// A map's entity blocks as key lists, in order (the exe's tokenizer:
/// `{`, then quoted key/value pairs, then `}`).
#[must_use]
pub fn blocks(text: &str) -> Vec<(std::ops::Range<usize>, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(open) = text[i..].find('{').map(|o| o + i) {
        let mut keys = Vec::new();
        let mut quoted: Vec<String> = Vec::new();
        let mut j = open + 1;
        let mut closed = None;
        while j < b.len() {
            match b[j] {
                b'}' => {
                    closed = Some(j);
                    break;
                }
                b'"' => {
                    let end = text[j + 1..].find('"').map_or(b.len(), |e| e + j + 1);
                    quoted.push(text[j + 1..end.min(b.len())].to_owned());
                    if quoted.len() == 2 {
                        let v = quoted.pop().unwrap_or_default();
                        let k = quoted.pop().unwrap_or_default();
                        keys.push((k, v));
                    }
                    j = end + 1;
                    continue;
                }
                _ => {}
            }
            j += 1;
        }
        let Some(close) = closed else { break };
        out.push((open..close + 1, keys));
        i = close + 1;
    }
    out
}

/// The entity text with only the blocks the game keeps: the world,
/// entities, structs, spawn points and path nodes (Black Ops II's scripts
/// find spawn points and nodes by class, so those stay) and the client-only
/// models (the client draws them; here the map's models are drawn from
/// their entities), and what was left out, by class (sorted).
#[must_use]
pub fn as_spawned(text: &str, mp: bool) -> (String, Vec<(String, usize)>) {
    let mut out = String::with_capacity(text.len());
    let mut dropped: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut last = 0;
    for (range, keys) in blocks(text) {
        out.push_str(&text[last..range.start]);
        last = range.end;
        let keep = matches!(
            spawn_class(&keys, mp),
            T7Spawn::World | T7Spawn::Entity | T7Spawn::Struct | T7Spawn::SpawnPoint | T7Spawn::PathNode | T7Spawn::ClientOnly
        );
        if keep {
            out.push_str(&text[range]);
        } else {
            *dropped.entry(value(&keys, "classname").unwrap_or("").to_owned()).or_default() += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, dropped.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn classes_spawn_as_the_exe_spawns_them() {
        let c = |cls: &str| spawn_class(&keys(&[("classname", cls)]), true);
        assert_eq!(c("mp_tdm_spawn"), T7Spawn::SpawnPoint);
        assert_eq!(c("mp_global_intermission"), T7Spawn::SpawnPoint);
        assert_eq!(c("info_player_start"), T7Spawn::SpawnPoint);
        assert_eq!(c("cp_spawn"), T7Spawn::NoSpawnFunction);
        assert_eq!(c("node_pathnode"), T7Spawn::PathNode);
        assert_eq!(c("trigger_box"), T7Spawn::Entity);
        assert_eq!(c("trigger_disk"), T7Spawn::Entity);
        assert_eq!(c("script_brushmodel"), T7Spawn::Entity);
        assert_eq!(c("script_struct"), T7Spawn::Struct);
        assert_eq!(c("ScriptBundle_scene"), T7Spawn::Consumed);
        assert_eq!(c("heli_height_lock"), T7Spawn::Consumed);
        for dropped in ["fx", "nav_volume", "umbra_volume", "volume_fpstool", "glass", "rope_x", "dyn_brush"] {
            assert_eq!(c(dropped), T7Spawn::Dropped, "{dropped}");
        }
        for freed in ["nav_volume_passable", "volume_worldfog", "volume_litfog", "volume_sun", "klf", "volume_weathergrime"] {
            assert_eq!(c(freed), T7Spawn::NoSpawnFunction, "{freed}");
        }
        assert_eq!(spawn_class(&keys(&[("classname", "script_model"), ("spawnflags", "2")]), true), T7Spawn::ClientOnly);
        assert_eq!(
            spawn_class(&keys(&[("classname", "script_model"), ("spawnflags", "2"), ("destructibledef", "d")]), true),
            T7Spawn::Entity
        );
        assert_eq!(spawn_class(&keys(&[("classname", "script_origin"), ("clientside", "1")]), true), T7Spawn::ClientOnly);
        assert_eq!(spawn_class(&keys(&[("classname", "script_model")]), true), T7Spawn::Entity);
    }

    #[test]
    fn freed_blocks_leave_the_text() {
        let text = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"model\" \"*8\"\n\"classname\" \"nav_volume\"\n}\n{\n\"classname\" \"trigger_box\"\n\"width\" \"48\"\n}\n{\n\"classname\" \"fx\"\n}\n";
        let (kept, dropped) = as_spawned(text, true);
        assert!(kept.contains("worldspawn") && kept.contains("trigger_box"));
        assert!(!kept.contains("nav_volume") && !kept.contains("\"fx\""));
        assert_eq!(dropped, vec![("fx".to_owned(), 1), ("nav_volume".to_owned(), 1)]);
    }
}

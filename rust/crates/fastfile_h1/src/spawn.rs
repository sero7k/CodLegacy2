//! bo2mp: what Modern Warfare Remastered's map load makes of a map's entity
//! blocks, by the game's own rules (its map-load entity spawn, read from the
//! multiplayer exe; see docs/H1_NATIVE.md "Map logic").
//!
//! The exe reads the entity text block by block. The first block must be
//! `worldspawn` (its `northyaw`, music and spawnflags keys are read; nothing
//! else). Then for each block, by its `classname`:
//!
//! 1. `node_*` (any case): a path node (the path node table, no entity);
//! 2. `info_vehicle_node` or `info_vehicle_node_*`: a vehicle path node;
//! 3. `actor_*`: nothing in multiplayer;
//! 4. `weapon_<weapon>` when `<weapon>` is a weapon the game has loaded: a
//!    weapon pickup entity;
//! 5. a class of the spawnable table, then of the map-only table
//!    ([`SPAWNABLE`], [`MAP_ONLY`]; matched case-insensitively, the table's
//!    name followed by the end of the class name or by `_`: `light_spot`
//!    takes `light`'s spawn, `script_struct_heli` `script_struct`'s):
//!    `script_struct` makes no entity (a struct), the rest an entity whose
//!    spawn function is the class's;
//! 6. **anything else is still an entity**, with no spawn function (the
//!    class name and the keys are kept: scripts find it with
//!    `getentarray`). Unlike Black Ops II and III, nothing is freed for
//!    having no spawn function, and there is no spawn point branch: a map's
//!    spawn points are the `MapEnts` spawn list, which `getspawnarray`
//!    reads; the usermaps' copies in the text are plain entities.
//!
//! Every entity's keys go through the game's field table first (classname,
//! origin, model, spawnflags, target, targetname, health, dmg, angles,
//! script_linkname, ...); `model` `*N` is a brush model, `?N` a trigger
//! model, any other name an `XModel`.
//!
//! What the spawn functions read is in docs/H1_NATIVE.md (trigger shapes,
//! contents, the hurt touch).

/// What the game makes of an entity block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H1Spawn {
    /// `worldspawn`.
    World,
    /// An entity with the spawn function of this table class.
    Entity(&'static str),
    /// An entity with no spawn function (kept as placed).
    Plain,
    /// A weapon pickup (`weapon_<weapon>`).
    Weapon,
    /// A script struct (`script_struct` / `script_struct_*`): no entity.
    Struct,
    /// A path node.
    PathNode,
    /// A vehicle path node (no entity).
    VehicleNode,
    /// An actor: not in multiplayer.
    Actor,
}

/// The spawnable table (searched first; `spawn()` can make these).
pub const SPAWNABLE: [&str; 5] = [
    "info_notnull",
    "trigger_radius",
    "script_model",
    "script_origin",
    "script_vehicle_collmap",
];

/// The map-only table, in the exe's order (`trigger_use_touch` is looked at
/// before `trigger_use`, which its name would also match).
pub const MAP_ONLY: [&str; 15] = [
    "trigger_use_touch",
    "trigger_use",
    "trigger_multiple",
    "trigger_disk",
    "trigger_hurt",
    "trigger_once",
    "trigger_damage",
    "light",
    "light_spot",
    "light_omni",
    "misc_mg42",
    "misc_turret",
    "script_brushmodel",
    "script_struct",
    "script_vehicle",
];

/// The exe's class match: `name` is `table` (any case) or `table` followed
/// by `_`.
#[must_use]
pub fn class_matches(name: &str, table: &str) -> bool {
    let (n, t) = (name.as_bytes(), table.as_bytes());
    n.len() >= t.len()
        && n[..t.len()].eq_ignore_ascii_case(t)
        && n.get(t.len()).is_none_or(|&c| c == b'_')
}

fn prefix_ci(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len()
        && name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// The table class whose spawn function `classname` takes, if any.
#[must_use]
pub fn table_class(classname: &str) -> Option<&'static str> {
    SPAWNABLE
        .iter()
        .chain(MAP_ONLY.iter())
        .find(|t| class_matches(classname, t))
        .copied()
}

/// What the game makes of a block of class `classname` (`weapon_known`:
/// whether a `weapon_<name>` names a loaded weapon; the map's own pickups
/// always do).
#[must_use]
pub fn spawn_class(classname: &str, weapon_known: bool) -> H1Spawn {
    if classname == "worldspawn" {
        return H1Spawn::World;
    }
    if prefix_ci(classname, "node_") {
        return H1Spawn::PathNode;
    }
    if class_matches(classname, "info_vehicle_node") {
        return H1Spawn::VehicleNode;
    }
    if prefix_ci(classname, "actor_") {
        return H1Spawn::Actor;
    }
    if classname.starts_with("weapon_") && weapon_known {
        return H1Spawn::Weapon;
    }
    match table_class(classname) {
        Some("script_struct") => H1Spawn::Struct,
        Some(t) => H1Spawn::Entity(t),
        None => H1Spawn::Plain,
    }
}

/// `trigger_hurt`: damage per touch when the map gives none.
pub const HURT_DEFAULT_DMG: i32 = 5;
/// `trigger_hurt`: a touch hurts again after 50 ms, or 1000 ms with
/// spawnflag 16.
pub const HURT_INTERVAL_MS: i32 = 50;
pub const HURT_SLOW_INTERVAL_MS: i32 = 1000;
/// `trigger_hurt` spawnflags: 1 starts off (no touch until used), 16 slow,
/// 32 turns off after its first hurt.
pub const HURT_START_OFF: i32 = 1;
pub const HURT_SLOW: i32 = 16;
pub const HURT_ONCE: i32 = 32;
/// `trigger_disk`: the radius reaches this much further, and the disk from
/// 100000 below to 100000 above.
pub const DISK_RADIUS_EXTRA: f32 = 64.0;
pub const DISK_HALF_HEIGHT: f32 = 100_000.0;

/// One trigger's volume as its spawn function sets it, relative to the
/// entity's origin (`Bounds`: middle, half size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerShape {
    pub mid: [f32; 3],
    pub half: [f32; 3],
}

/// `trigger_radius` (`radius`, `height`): from the origin up `height`,
/// `radius` round it.
#[must_use]
pub fn radius_shape(radius: f32, height: f32) -> TriggerShape {
    TriggerShape {
        mid: [0.0, 0.0, height * 0.5],
        half: [radius, radius, height * 0.5],
    }
}

/// `trigger_disk` (`radius`).
#[must_use]
pub fn disk_shape(radius: f32) -> TriggerShape {
    let r = radius + DISK_RADIUS_EXTRA;
    TriggerShape {
        mid: [0.0; 3],
        half: [r, r, DISK_HALF_HEIGHT],
    }
}

/// When a `trigger_hurt` may hurt the same player again (ms after this
/// hurt), by its spawnflags.
#[must_use]
pub fn hurt_interval_ms(spawnflags: i32) -> i32 {
    if spawnflags & HURT_SLOW != 0 {
        HURT_SLOW_INTERVAL_MS
    } else {
        HURT_INTERVAL_MS
    }
}

/// The damage a `trigger_hurt` deals per touch (`dmg`, 5 when 0).
#[must_use]
pub fn hurt_damage(dmg: i32) -> i32 {
    if dmg == 0 { HURT_DEFAULT_DMG } else { dmg }
}

/// A touch trigger's `wait` of 0 or less makes it fire once (as
/// `trigger_once`, which always does).
#[must_use]
pub fn fires_once(classname: &str, wait: Option<f32>) -> bool {
    class_matches(classname, "trigger_once") || wait.is_some_and(|w| w <= 0.0)
}

/// One entity block: its byte range in the text and its keys in order.
pub type Block = (std::ops::Range<usize>, Vec<(String, String)>);

/// A map's entity blocks as key lists, in order, with each block's byte
/// range in `text` (`{`, quoted key/value pairs, `}`).
#[must_use]
pub fn blocks(text: &str) -> Vec<Block> {
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
                b'\n' => quoted.clear(),
                b'"' => {
                    // (A stray quote ends at its line: one bad value must not
                    // swallow the blocks after it.)
                    let line_end = text[j + 1..].find('\n').map_or(b.len(), |e| e + j + 1);
                    let closing = text[j + 1..line_end].find('"').map(|e| e + j + 1);
                    let end = closing.unwrap_or(line_end);
                    quoted.push(text[j + 1..end.min(b.len())].to_owned());
                    if let [k, v] = &mut quoted[..] {
                        keys.push((std::mem::take(k), std::mem::take(v)));
                        quoted.clear();
                    }
                    // (No closing quote: stop at the line's end, which the
                    // loop then sees.)
                    j = if closing.is_some() { end + 1 } else { end };
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

fn value<'a>(keys: &'a [(String, String)], key: &str) -> Option<&'a str> {
    keys.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// What [`as_spawned`] did, for the load report and the tests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapLogic {
    /// Blocks by what the game makes of them.
    pub entities: usize,
    pub plain: usize,
    pub structs: usize,
    pub path_nodes: usize,
    pub weapons: usize,
    /// Left out: vehicle nodes and actors (no entity in multiplayer).
    pub not_spawned: usize,
    /// Classes given their table class's name (`script_struct_heli` ->
    /// `script_struct`), the map's kept under `h1_classname`.
    pub renamed: Vec<(String, &'static str)>,
    /// Left out as the map's scripts leave the match without them (see
    /// [`as_spawned`]): (targetname, count).
    pub script_removed: Vec<(String, usize)>,
}

impl MapLogic {
    /// One line for the load report.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "h1 map logic: {} entities with a spawn function, {} without, {} structs, {} path nodes, {} weapon pickups, {} not spawned (vehicle nodes, actors); renamed {:?}; left out as the scripts delete them {:?}",
            self.entities,
            self.plain,
            self.structs,
            self.path_nodes,
            self.weapons,
            self.not_spawned,
            self.renamed,
            self.script_removed
        )
    }
}

/// Targetnames whose blocks the map's start scripts delete in every
/// ordinary match (inferred from Call of Duty 4's multiplayer scripts, which
/// Modern Warfare Remastered remasters; its own are not in the staged
/// zones): `oldschool_pickup` (the Old School mode's weapon and perk pickups,
/// with what they target), `delete_on_load` (the editor's traversal
/// previews), and the `exploder` models (hidden until their exploder fires:
/// the destroyed bomb objectives standing inside the whole ones).
pub const SCRIPT_DELETED: [&str; 3] = ["oldschool_pickup", "delete_on_load", "exploder"];

/// The entity text with the blocks the game spawns, by the rules above:
/// vehicle nodes and actors left out, a class matched to a table class by
/// the exe's rule given that class's name (the map's in `h1_classname`),
/// and, when `scripts_deletes` is on, the blocks of [`SCRIPT_DELETED`]
/// (and the blocks an `oldschool_pickup` targets) left out.
#[must_use]
pub fn as_spawned(text: &str, scripts_deletes: bool) -> (String, MapLogic) {
    let all = blocks(text);
    let mut report = MapLogic::default();
    // The targets of the Old School pickups go with them.
    let pickup_targets: std::collections::HashSet<String> = if scripts_deletes {
        all.iter()
            .filter(|(_, k)| value(k, "targetname") == Some("oldschool_pickup"))
            .filter_map(|(_, k)| value(k, "target").map(str::to_owned))
            .collect()
    } else {
        Default::default()
    };
    let mut removed: std::collections::BTreeMap<String, usize> = Default::default();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (range, keys) in all {
        out.push_str(&text[last..range.start]);
        last = range.end;
        let class = value(&keys, "classname").unwrap_or("");
        let targetname = value(&keys, "targetname").unwrap_or("");
        if scripts_deletes {
            let deleted = SCRIPT_DELETED
                .iter()
                .find(|t| **t == targetname)
                .map(|t| (*t).to_owned())
                .or_else(|| {
                    pickup_targets
                        .contains(targetname)
                        .then(|| format!("{targetname} (an oldschool_pickup's target)"))
                });
            // (Only models are hidden as exploders: an `exploder` trigger
            // or origin is the scripts' to use.)
            let deleted =
                deleted.filter(|t| t != "exploder" || class.eq_ignore_ascii_case("script_model"));
            if let Some(t) = deleted {
                *removed.entry(t).or_default() += 1;
                continue;
            }
        }
        let kind = spawn_class(class, true);
        match kind {
            H1Spawn::VehicleNode | H1Spawn::Actor => {
                report.not_spawned += 1;
                continue;
            }
            H1Spawn::PathNode => report.path_nodes += 1,
            H1Spawn::Weapon => report.weapons += 1,
            H1Spawn::Struct => report.structs += 1,
            H1Spawn::Plain => report.plain += 1,
            H1Spawn::Entity(_) => report.entities += 1,
            H1Spawn::World => {}
        }
        let table = match kind {
            H1Spawn::Struct => Some("script_struct"),
            H1Spawn::Entity(t) => Some(t),
            _ => None,
        };
        match table.filter(|t| *t != class) {
            Some(t) => {
                // Written again with the table's class name.
                out.push_str("{\n");
                for (k, v) in &keys {
                    if k.eq_ignore_ascii_case("classname") {
                        out.push_str(&format!(
                            "\"classname\" \"{t}\"\n\"h1_classname\" \"{v}\"\n"
                        ));
                    } else {
                        out.push_str(&format!("\"{k}\" \"{v}\"\n"));
                    }
                }
                out.push('}');
                if !report.renamed.iter().any(|(c, _)| c == class) {
                    report.renamed.push((class.to_owned(), t));
                }
            }
            None => out.push_str(&text[range]),
        }
    }
    out.push_str(&text[last..]);
    report.script_removed = removed.into_iter().collect();
    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_match_as_the_exe_matches_them() {
        assert!(class_matches("trigger_multiple", "trigger_multiple"));
        assert!(class_matches("TRIGGER_Multiple_x", "trigger_multiple"));
        assert!(!class_matches("trigger_multiplex", "trigger_multiple"));
        assert_eq!(table_class("trigger_use_touch"), Some("trigger_use_touch"));
        assert_eq!(table_class("trigger_use"), Some("trigger_use"));
        // (`light` comes first in the table and takes `light_spot` by the
        // `_` rule: the same spawn function.)
        assert_eq!(table_class("light_spot"), Some("light"));
        assert_eq!(table_class("script_struct_heli"), Some("script_struct"));
        assert_eq!(table_class("mp_tdm_spawn"), None);
        assert_eq!(spawn_class("worldspawn", true), H1Spawn::World);
        assert_eq!(spawn_class("node_pathnode", true), H1Spawn::PathNode);
        assert_eq!(spawn_class("Node_cover_left", true), H1Spawn::PathNode);
        assert_eq!(
            spawn_class("info_vehicle_node_rotate", true),
            H1Spawn::VehicleNode
        );
        assert_eq!(spawn_class("info_vehicle_nodes", true), H1Spawn::Plain);
        assert_eq!(spawn_class("actor_enemy", true), H1Spawn::Actor);
        assert_eq!(spawn_class("weapon_h1_ak47_mp", true), H1Spawn::Weapon);
        assert_eq!(spawn_class("weapon_nothing", false), H1Spawn::Plain);
        assert_eq!(spawn_class("script_struct", true), H1Spawn::Struct);
        assert_eq!(
            spawn_class("trigger_hurt", true),
            H1Spawn::Entity("trigger_hurt")
        );
        assert_eq!(
            spawn_class("info_notnull_big", true),
            H1Spawn::Entity("info_notnull")
        );
        // No spawn function: still an entity.
        assert_eq!(spawn_class("mp_tdm_spawn", true), H1Spawn::Plain);
        assert_eq!(
            spawn_class("trigger_radius_out_of_bounds", true),
            H1Spawn::Entity("trigger_radius")
        );
    }

    #[test]
    fn trigger_shapes_and_hurt_timing() {
        let r = radius_shape(128.0, 72.0);
        assert_eq!(r.mid, [0.0, 0.0, 36.0]);
        assert_eq!(r.half, [128.0, 128.0, 36.0]);
        let d = disk_shape(100.0);
        assert_eq!(d.half, [164.0, 164.0, 100_000.0]);
        assert_eq!(hurt_damage(0), 5);
        assert_eq!(hurt_damage(10_000), 10_000);
        assert_eq!(hurt_interval_ms(0), 50);
        assert_eq!(hurt_interval_ms(HURT_SLOW), 1000);
        assert!(fires_once("trigger_once", None));
        assert!(fires_once("trigger_multiple", Some(0.0)));
        assert!(!fires_once("trigger_multiple", Some(0.5)));
        assert!(!fires_once("trigger_multiple", None));
    }

    #[test]
    fn spawned_text_keeps_order_renames_and_leaves_out() {
        let text = "{\n\"classname\" \"worldspawn\"\n}\n\
            {\n\"classname\" \"script_struct_heli\"\n\"targetname\" \"heli_start\"\n}\n\
            {\n\"classname\" \"info_vehicle_node\"\n}\n\
            {\n\"classname\" \"weapon_h1_p90_mp\"\n\"targetname\" \"oldschool_pickup\"\n\"target\" \"t1\"\n}\n\
            {\n\"classname\" \"trigger_use\"\n\"targetname\" \"t1\"\n\"model\" \"?3\"\n}\n\
            {\n\"classname\" \"script_model\"\n\"targetname\" \"exploder\"\n\"model\" \"com_bomb_objective_d\"\n}\n\
            {\n\"classname\" \"script_origin\"\n\"targetname\" \"exploder\"\n}\n\
            {\n\"classname\" \"trigger_hurt\"\n\"dmg\" \"10000\"\n\"model\" \"?1\"\n}\n\
            {\n\"classname\" \"mp_tdm_spawn\"\n\"origin\" \"1 2 3\"\n}\n";
        let (out, r) = as_spawned(text, true);
        assert!(
            out.contains(
                "\"classname\" \"script_struct\"\n\"h1_classname\" \"script_struct_heli\""
            )
        );
        assert!(!out.contains("info_vehicle_node"));
        assert!(!out.contains("weapon_h1_p90_mp") && !out.contains("\"t1\""));
        assert!(!out.contains("com_bomb_objective_d"));
        assert!(
            out.contains("script_origin")
                && out.contains("trigger_hurt")
                && out.contains("mp_tdm_spawn")
        );
        assert_eq!(r.not_spawned, 1);
        assert_eq!(r.structs, 1);
        assert_eq!(r.entities, 2);
        assert_eq!(r.plain, 1);
        assert_eq!(
            r.script_removed,
            vec![
                ("exploder".to_owned(), 1),
                ("oldschool_pickup".to_owned(), 1),
                ("t1 (an oldschool_pickup's target)".to_owned(), 1)
            ]
        );
        // Off: only the game's own spawn rules.
        let (out, r) = as_spawned(text, false);
        assert!(out.contains("weapon_h1_p90_mp") && out.contains("com_bomb_objective_d"));
        assert!(r.script_removed.is_empty());
        assert_eq!(r.weapons, 1);
    }

    #[test]
    fn a_stray_quote_costs_only_its_line() {
        let text = "{\n\"classname\" \"worldspawn\"\n\"message\" \"say \"hi\n}\n{\n\"classname\" \"mp_tdm_spawn\"\n}\n";
        let b = blocks(text);
        assert_eq!(b.len(), 2);
        assert_eq!(value(&b[1].1, "classname"), Some("mp_tdm_spawn"));
    }
}

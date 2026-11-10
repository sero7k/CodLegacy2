//! bo2mp: a Ghosts map's path nodes.
//!
//! READ (see `docs/exe_notes/IW6_BOTS.md`): the engine keeps its nodes in one global `PathData`
//! (`pathnode_t`, 176 bytes each, at most 8192) that the zone's `PathData` asset fills, and the
//! map's `node_*` entities only bind their script keys (`targetname`, `target`, ...) to the nodes
//! of that asset in order. No staged Ghosts zone holds a `PathData` asset (mp_frag, its patch, the
//! common zones and `common_bots_mp` were all checked), so the links the engine would load are not
//! in the install. What the install does hold are the nodes themselves: the map's entity string
//! carries every node as a `node_*` entity with its origin, angles, type (by class name), script
//! keys and, for negotiation nodes, the `begin -> end` pairing (`target` / `targetname`) and the
//! traversal animation (`animscript`). This module reads those.
//!
//! The 31 node types are the engine's own table (`Error` .. `Jump Attack`, read from the exe).

use crate::maplogic;

/// The engine's node types by index (the exe's name table; index 0 is `Error`).
pub const NODE_TYPE_NAMES: [&str; 31] = [
    "Error",
    "Path",
    "Cover Stand",
    "Cover Crouch",
    "Cover Crouch Window",
    "Cover Prone",
    "Cover Right",
    "Cover Left",
    "Cover Multi",
    "Ambush",
    "Exposed",
    "Conceal Stand",
    "Conceal Crouch",
    "Conceal Prone",
    "Door",
    "Door Interior",
    "Scripted",
    "Begin",
    "End",
    "Turret",
    "Guard",
    "Path 3D",
    "Cover Up 3D",
    "Cover Right 3D",
    "Cover Left 3D",
    "Exposed 3D",
    "Scripted 3D",
    "Begin 3D",
    "End 3D",
    "Jump",
    "Jump Attack",
];

/// The engine's node limit (the node table holds this many `pathnode_t`).
pub const MAX_NODES: usize = 0x2000;

/// A node type (an index into [`NODE_TYPE_NAMES`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodeType(pub u8);

impl NodeType {
    pub const ERROR: Self = Self(0);
    pub const PATH: Self = Self(1);
    pub const BEGIN: Self = Self(17);
    pub const END: Self = Self(18);

    pub fn name(self) -> &'static str {
        NODE_TYPE_NAMES.get(self.0 as usize).copied().unwrap_or("Error")
    }

    /// A node a bot stands at to fight from cover (the `Cover ...` and `Conceal ...` types).
    pub fn is_cover(self) -> bool {
        matches!(self.0, 2..=8 | 11..=13 | 22..=24)
    }

    /// The ends of a traversal (`Begin`, `End`, and the 3D forms).
    pub fn is_negotiation(self) -> bool {
        matches!(self.0, 17 | 18 | 27 | 28)
    }

    /// A node that is part of the walkable route graph (path nodes, and the nodes that stand
    /// on the floor beside them; not the traversal ends, which only their pair uses).
    pub fn is_route(self) -> bool {
        !self.is_negotiation() && self.0 != 0
    }

    /// The type a `node_*` class name stands for: the compile tool's names (`node_pathnode`, the
    /// type name lower-cased with `_`, `node_negotiation_begin` / `_end`). INFERRED: the exe holds
    /// only the type names, not the class names (the entity classes are the editor's).
    pub fn from_class(class: &str) -> Option<Self> {
        let c = class.to_ascii_lowercase();
        let rest = c.strip_prefix("node_")?;
        let t = match rest {
            "pathnode" | "path" => 1,
            "negotiation_begin" => 17,
            "negotiation_end" => 18,
            "negotiation_begin_3d" => 27,
            "negotiation_end_3d" => 28,
            _ => {
                let want = rest.replace(' ', "_");
                NODE_TYPE_NAMES
                    .iter()
                    .position(|n| n.to_ascii_lowercase().replace(' ', "_") == want)? as u8
            }
        };
        Some(Self(t))
    }
}

/// One path node of the map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathNode {
    /// The node's place among the map's nodes (the order of the entity string; the engine binds
    /// the entities to the `PathData` nodes in this order).
    pub index: u32,
    pub ty: NodeType,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    pub spawnflags: u32,
    pub targetname: String,
    pub target: String,
    pub script_noteworthy: String,
    pub script_linkname: String,
    /// The traversal animation of a negotiation `Begin` node (`jump_up_40`, `window_over_40`).
    pub animscript: String,
}

/// A traversal: the `Begin` node, the `End` node it targets and the animation.
#[derive(Clone, Debug, PartialEq)]
pub struct Negotiation {
    pub begin: u32,
    pub end: u32,
    pub animscript: String,
}

/// A map's nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathNodes {
    pub nodes: Vec<PathNode>,
    pub negotiations: Vec<Negotiation>,
    /// `Begin` nodes whose `target` names no `End` node.
    pub dangling_begins: Vec<u32>,
}

fn vec3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|x| x.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

impl PathNodes {
    /// The nodes in a map's entity text (as in `MapEnts`, numbered keys allowed).
    pub fn from_entities(text: &str) -> Self {
        let mut out = Self::default();
        for block in maplogic::blocks(text) {
            let get = |k: &str| block.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
            let Some(ty) = get("classname").and_then(NodeType::from_class) else {
                continue;
            };
            let Some(origin) = get("origin").and_then(vec3) else {
                continue;
            };
            out.nodes.push(PathNode {
                index: out.nodes.len() as u32,
                ty,
                origin,
                angles: get("angles").or_else(|| get("script_angles")).and_then(vec3).unwrap_or([0.0; 3]),
                spawnflags: get("spawnflags").and_then(|v| v.parse().ok()).unwrap_or(0),
                targetname: get("targetname").unwrap_or("").to_owned(),
                target: get("target").unwrap_or("").to_owned(),
                script_noteworthy: get("script_noteworthy").unwrap_or("").to_owned(),
                script_linkname: get("script_linkname").unwrap_or("").to_owned(),
                animscript: get("animscript").unwrap_or("").to_owned(),
            });
        }
        out.pair_negotiations();
        out
    }

    fn pair_negotiations(&mut self) {
        let mut ends: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for n in &self.nodes {
            if matches!(n.ty.0, 18 | 28) && !n.targetname.is_empty() {
                ends.entry(n.targetname.to_ascii_lowercase()).or_insert(n.index);
            }
        }
        for n in &self.nodes {
            if !matches!(n.ty.0, 17 | 27) {
                continue;
            }
            match ends.get(&n.target.to_ascii_lowercase()) {
                Some(&end) if !n.target.is_empty() => {
                    self.negotiations.push(Negotiation { begin: n.index, end, animscript: n.animscript.clone() })
                }
                _ => self.dangling_begins.push(n.index),
            }
        }
    }

    /// Nodes per type name, in type order.
    pub fn counts(&self) -> Vec<(&'static str, usize)> {
        let mut c = [0usize; 31];
        for n in &self.nodes {
            c[(n.ty.0 as usize).min(30)] += 1;
        }
        NODE_TYPE_NAMES.iter().copied().zip(c).filter(|(_, n)| *n > 0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"
{
"classname" "worldspawn"
}
{
"origin" "10 20 30"
"classname" "node_pathnode"
}
{
"origin" "40 20 30"
"angles" "0 270 0"
"classname" "node_cover_left"
"targetname" "zone_hint"
}
{
"spawnflags" "2"
"origin" "415 454 153"
"targetname" "pf12_auto2108"
"angles" "0 270 0"
"classname" "node_negotiation_end"
}
{
"targetname" "traverse"
"classname" "node_negotiation_begin"
"angles" "0 270 0"
"animscript" "window_over_40"
"origin" "415 646 153"
"target" "pf12_auto2108"
}
{
"origin" "0 0 0"
"classname" "node_negotiation_begin"
"target" "nowhere"
}
"#;

    #[test]
    fn classes_map_to_the_engines_types() {
        assert_eq!(NodeType::from_class("node_pathnode"), Some(NodeType(1)));
        assert_eq!(NodeType::from_class("node_cover_stand"), Some(NodeType(2)));
        assert_eq!(NodeType::from_class("node_cover_crouch_window"), Some(NodeType(4)));
        assert_eq!(NodeType::from_class("node_cover_left"), Some(NodeType(7)));
        assert_eq!(NodeType::from_class("node_conceal_prone"), Some(NodeType(13)));
        assert_eq!(NodeType::from_class("node_negotiation_end"), Some(NodeType::END));
        assert_eq!(NodeType::from_class("node_jump_attack"), Some(NodeType(30)));
        assert_eq!(NodeType::from_class("script_origin"), None);
        assert_eq!(NodeType(7).name(), "Cover Left");
        assert!(NodeType(7).is_cover() && !NodeType(1).is_cover());
        assert!(NodeType::BEGIN.is_negotiation() && !NodeType::PATH.is_negotiation());
    }

    #[test]
    fn nodes_and_pairs_are_read() {
        let p = PathNodes::from_entities(TEXT);
        assert_eq!(p.nodes.len(), 5);
        assert_eq!(p.nodes[0].origin, [10.0, 20.0, 30.0]);
        assert_eq!(p.nodes[1].angles, [0.0, 270.0, 0.0]);
        assert_eq!(p.nodes[1].targetname, "zone_hint");
        assert_eq!(p.nodes[2].spawnflags, 2);
        assert_eq!(p.negotiations, vec![Negotiation { begin: 3, end: 2, animscript: "window_over_40".into() }]);
        assert_eq!(p.dangling_begins, vec![4]);
        assert_eq!(p.counts().iter().map(|c| c.1).sum::<usize>(), 5);
    }

    /// mp_frag's real nodes (skips when no Ghosts zones are staged: `BO2MP_IW6_ZONES`, else the
    /// owner's upload folder).
    #[test]
    fn mp_frag_nodes() {
        let dir = std::env::var_os("BO2MP_IW6_ZONES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into());
        let Ok(zone) = crate::Zone::open(&dir.join("mp_frag.ff")) else { return };
        let (cap, _) = crate::walk::walk_zone(&zone);
        let me = cap.map_ents.first().expect("map ents");
        let p = PathNodes::from_entities(&me.entity_string);
        let n = |t: u8| p.nodes.iter().filter(|x| x.ty.0 == t).count();
        assert_eq!(n(1), 1356);
        assert_eq!(n(2), 9);
        assert_eq!(n(3), 11);
        assert_eq!(n(4), 19);
        assert_eq!(n(5), 1);
        assert_eq!(n(6), 120);
        assert_eq!(n(7), 112);
        assert_eq!(n(17), 137);
        assert_eq!(n(18), 137);
        assert_eq!(p.nodes.len(), 1902);
        assert_eq!(p.negotiations.len(), 137);
        assert!(p.dangling_begins.is_empty());
        assert!(p.nodes.len() <= MAX_NODES);
    }
}

//! bo2mp: a Ghosts map's AI path data, read from its `<map>_path.ff` zone (the `PathData` asset).
//!
//! READ from the zone bytes (mp_frag, mp_snow) and from the engine's node code
//! (`docs/exe_notes/IW6_BOTS.md`): the asset holds every node with its type, script keys, origin
//! and links, a triangular node-to-node visibility bit matrix, the k-d tree the engine finds the
//! nearest node with, and exposure, no-peek and zone tables. The map's own `node_*` entities only
//! bind their script keys to the first nodes of this asset, in order ([`crate::pathnode`]).

use crate::cursor::ReadError;
use crate::pathnode::NodeType;
use crate::walk::{PathDataRec, PathTreeRec};
use crate::{layout as L, Zone};

/// One link of a node (`pathlink_s`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DataLink {
    /// The engine's walk cost of the link (a distance).
    pub dist: f32,
    /// The node it leads to.
    pub to: u16,
    /// How many times the link has been disconnected (0: open). At load it is 0 for every link.
    pub disconnect_count: u8,
    /// A traversal (`Begin` to `End`, jump, window): the link is not a walk.
    pub negotiation: bool,
    pub flags: u8,
}

/// One node of the asset (`pathnode_t`'s constant part).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataNode {
    pub ty: NodeType,
    pub spawnflags: u32,
    pub targetname: String,
    pub script_linkname: String,
    pub script_noteworthy: String,
    pub target: String,
    pub animscript: String,
    pub origin: [f32; 3],
    /// `fLocalAngle` (degrees) and the facing vector the engine caches from it.
    pub yaw: f32,
    pub forward: [f32; 2],
    /// The parent mover's script name index (0: none), as the asset stores it.
    pub parent: u32,
    pub overlap: [i16; 2],
    pub links: Vec<DataLink>,
}

/// A map's path data.
#[derive(Clone, Debug, Default)]
pub struct PathData {
    pub name: String,
    pub version: u16,
    pub nodes: Vec<DataNode>,
    /// The triangular visibility bits, `n * (n - 1) / 2` of them.
    pub vis: Vec<u8>,
    pub tree: Vec<PathTreeRec>,
    /// Dynamic node groups: (parent mover index, tree of the group's nodes).
    pub dyn_groups: Vec<(u32, Vec<PathTreeRec>)>,
    pub zone_count: u32,
    /// One byte per node (`zonesBytes` equals the node count on every staged map).
    pub zones: Vec<u8>,
    pub exposure_bytes: usize,
    pub no_peek_bytes: usize,
    pub dyn_state_bytes: usize,
}

fn f32le(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

impl PathData {
    /// The path data of a `_path` zone.
    pub fn from_zone(zone: &Zone) -> Result<Self, String> {
        let (cap, stop) = crate::walk::walk_zone(zone);
        if let Some(s) = stop {
            return Err(format!("path zone: walk stopped at asset {}: {}", s.asset_index, s.error));
        }
        let rec = cap.path_data.first().ok_or("path zone: no PathData asset")?;
        Ok(Self::from_rec(rec, &zone.image, &zone.assets.script_strings))
    }

    /// The asset's record turned into nodes (script strings by the zone's list).
    pub fn from_rec(rec: &PathDataRec, image: &[u8], strings: &[Option<String>]) -> Self {
        let s = |id: u32| -> String { strings.get(id as usize).cloned().flatten().unwrap_or_default() };
        let mut nodes = Vec::with_capacity(rec.nodes.len());
        for (i, span) in rec.nodes.iter().enumerate() {
            let b = span.get(image);
            let mut links = Vec::new();
            if let Some((Some(ls), count)) = rec.links.get(i) {
                let lb = ls.get(image);
                for k in 0..*count {
                    let o = k * L::pathlink_s::SIZE;
                    links.push(DataLink {
                        dist: f32le(lb, o + L::pathlink_s::fDist),
                        to: u16le(lb, o + L::pathlink_s::nodeNum),
                        disconnect_count: lb[o + L::pathlink_s::disconnectCount],
                        negotiation: lb[o + L::pathlink_s::negotiationLink] != 0,
                        flags: lb[o + L::pathlink_s::flags],
                    });
                }
            }
            use L::pathnode_t as N;
            nodes.push(DataNode {
                ty: NodeType(u32le(b, N::constant__type).min(255) as u8),
                spawnflags: u32le(b, N::constant__spawnflags),
                targetname: s(u32le(b, N::constant__targetname)),
                script_linkname: s(u32le(b, N::constant__script_linkName)),
                script_noteworthy: s(u32le(b, N::constant__script_noteworthy)),
                target: s(u32le(b, N::constant__target)),
                animscript: s(u32le(b, N::constant__animscript)),
                origin: [f32le(b, N::constant__vLocalOrigin), f32le(b, N::constant__vLocalOrigin + 4), f32le(b, N::constant__vLocalOrigin + 8)],
                yaw: f32le(b, N::constant_____u9__yaw_orient__fLocalAngle),
                forward: [f32le(b, N::constant_____u9__yaw_orient__localForward), f32le(b, N::constant_____u9__yaw_orient__localForward + 4)],
                parent: u32le(b, N::constant__parent),
                overlap: [i16::from_le_bytes([b[N::constant__wOverlapNode], b[N::constant__wOverlapNode + 1]]), i16::from_le_bytes([b[N::constant__wOverlapNode + 2], b[N::constant__wOverlapNode + 3]])],
                links,
            });
        }
        let blob = |s: &Option<crate::walk::Span>| s.map(|s| s.get(image).to_vec()).unwrap_or_default();
        Self {
            name: rec.name.clone(),
            version: rec.version,
            nodes,
            vis: blob(&rec.vis),
            tree: rec.tree.clone(),
            dyn_groups: rec.dyn_groups.clone(),
            zone_count: rec.zone_count,
            zones: blob(&rec.zones),
            exposure_bytes: rec.exposure.map_or(0, |s| s.len),
            no_peek_bytes: rec.no_peek_vis.map_or(0, |s| s.len),
            dyn_state_bytes: rec.dyn_states.map_or(0, |s| s.len),
        }
    }

    /// The path zone of a map in the zone folder (`<map>_path.ff`).
    pub fn open_map(zone_dir: &std::path::Path, map: &str) -> Result<Self, String> {
        let path = zone_dir.join(format!("{map}_path.ff"));
        let zone = Zone::open(&path).map_err(|e| format!("{}: {e:?}", path.display()))?;
        Self::from_zone(&zone)
    }

    pub fn link_count(&self) -> usize {
        self.nodes.iter().map(|n| n.links.len()).sum()
    }

    /// Links that are traversals (the `negotiationLink` byte).
    pub fn negotiation_links(&self) -> usize {
        self.nodes.iter().flat_map(|n| &n.links).filter(|l| l.negotiation).count()
    }

    /// Whether the asset is self-consistent: every link leads to a node, the tree's leaves list
    /// each tree node once, the visibility bit count fits the node count.
    pub fn check(&self) -> Result<(), String> {
        let n = self.nodes.len();
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(l) = node.links.iter().find(|l| l.to as usize >= n) {
                return Err(format!("node {i}: link to node {} of {n}", l.to));
            }
        }
        let want = (n * n.saturating_sub(1) / 2).div_ceil(8);
        if self.vis.len() != want {
            return Err(format!("visibility bytes {} != {want} for {n} nodes", self.vis.len()));
        }
        for (g, (_, tree)) in self.dyn_groups.iter().enumerate() {
            for e in tree {
                if let Some(&id) = e.leaf.iter().find(|&&id| id as usize >= n) {
                    return Err(format!("dynamic group {g}: leaf lists node {id} of {n}"));
                }
            }
        }
        let mut seen = vec![0u8; n];
        for e in &self.tree {
            for &id in &e.leaf {
                *seen.get_mut(id as usize).ok_or_else(|| format!("tree leaf lists node {id} of {n}"))? += 1;
            }
        }
        if let Some(i) = seen.iter().position(|&c| c > 1) {
            return Err(format!("tree lists node {i} {} times", seen[i]));
        }
        Ok(())
    }

    /// Nodes the tree lists (the entities' nodes; the asset can hold more).
    pub fn tree_nodes(&self) -> usize {
        self.tree.iter().map(|e| e.leaf.len()).sum()
    }

    /// The leaf of the tree a point falls in: the nodes near it (the engine's nearest-node
    /// query starts here). Splits are on `axis` at `dist`; the first child is the side below.
    pub fn leaf_at(&self, p: [f32; 3]) -> &[u16] {
        let mut i = 0usize;
        for _ in 0..64 {
            let Some(e) = self.tree.get(i) else { return &[] };
            if e.axis < 0 {
                return &e.leaf;
            }
            let side = usize::from(p[(e.axis as usize).min(2)] >= e.dist);
            match e.children[side] {
                Some(c) => i = c as usize,
                None => return &[],
            }
        }
        &[]
    }

    /// Connected components over the open links (a link is followed both ways, as the route
    /// search treats undirected walks). Returns the component number of each node.
    pub fn components(&self, include_negotiation: bool) -> Vec<u32> {
        let n = self.nodes.len();
        let mut adj = vec![Vec::new(); n];
        for (i, node) in self.nodes.iter().enumerate() {
            for l in &node.links {
                if l.disconnect_count == 0 && (include_negotiation || !l.negotiation) && (l.to as usize) < n {
                    adj[i].push(l.to as usize);
                    adj[l.to as usize].push(i);
                }
            }
        }
        let mut comp = vec![u32::MAX; n];
        let mut next = 0;
        for s in 0..n {
            if comp[s] != u32::MAX {
                continue;
            }
            let mut stack = vec![s];
            comp[s] = next;
            while let Some(i) = stack.pop() {
                for &j in &adj[i] {
                    if comp[j] == u32::MAX {
                        comp[j] = next;
                        stack.push(j);
                    }
                }
            }
            next += 1;
        }
        comp
    }
}

impl From<ReadError> for PathDataError {
    fn from(e: ReadError) -> Self {
        Self(format!("{e:?}"))
    }
}

/// A path zone that could not be read.
#[derive(Debug)]
pub struct PathDataError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_dir() -> std::path::PathBuf {
        std::env::var_os("BO2MP_IW6_ZONES").map(Into::into).unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into())
    }

    fn open(map: &str) -> Option<PathData> {
        PathData::open_map(&zone_dir(), map).ok()
    }

    #[test]
    fn tree_leaf_and_components_on_synthetic_data() {
        let node = |links: &[u16]| DataNode { links: links.iter().map(|&to| DataLink { dist: 1.0, to, disconnect_count: 0, negotiation: false, flags: 0 }).collect(), ..Default::default() };
        let tree = vec![
            PathTreeRec { axis: 0, dist: 10.0, children: [Some(1), Some(2)], leaf: vec![] },
            PathTreeRec { axis: -1, dist: 0.0, children: [None, None], leaf: vec![0, 1] },
            PathTreeRec { axis: -1, dist: 0.0, children: [None, None], leaf: vec![2] },
        ];
        let p = PathData { nodes: vec![node(&[1]), node(&[0]), node(&[])], tree, vis: vec![0], ..Default::default() };
        assert_eq!(p.leaf_at([5.0, 0.0, 0.0]), &[0, 1]);
        assert_eq!(p.leaf_at([15.0, 0.0, 0.0]), &[2]);
        assert_eq!(p.components(true), vec![0, 0, 1]);
        assert!(p.check().is_ok());
        let mut bad = p.clone();
        bad.nodes[0].links[0].to = 9;
        assert!(bad.check().is_err());
    }

    /// Real data: counts proven on both staged maps (skips when the zones are not staged).
    #[test]
    fn mp_frag_and_mp_snow_path_zones() {
        for (map, nodes, in_tree, links, negotiation, zones) in [("mp_frag", 1965, 1902, 26858, 137, 63), ("mp_snow", 2178, 2079, 18268, 352, 99)] {
            let Some(p) = open(map) else { return };
            assert_eq!((p.nodes.len(), p.tree_nodes(), p.link_count(), p.negotiation_links(), p.zone_count), (nodes, in_tree, links, negotiation, zones), "{map}");
            p.check().unwrap();
            // The nodes past the tree's are the zones' pseudo nodes (type Error), apart from the walk graph.
            assert!(p.nodes[in_tree..].iter().all(|n| n.ty == NodeType::ERROR), "{map}");
            assert!(p.nodes[..in_tree].iter().all(|n| n.ty != NodeType::ERROR), "{map}");
            // Every real node reaches every other over the links (one component of exactly the tree's nodes).
            let comp = p.components(true);
            let main = comp[0];
            assert_eq!(comp.iter().filter(|&&c| c == main).count(), in_tree, "{map}: the walk graph is one piece");
            // A negotiation link joins a Begin node to an End node.
            for (i, n) in p.nodes.iter().enumerate() {
                for l in n.links.iter().filter(|l| l.negotiation) {
                    assert!(n.ty.is_negotiation() && p.nodes[l.to as usize].ty.is_negotiation(), "{map}: node {i}");
                }
            }
        }
    }

    /// The map's `node_*` entities are the first nodes of the asset, in order (the engine binds them so).
    #[test]
    fn the_entities_nodes_are_the_assets_first_nodes() {
        for map in ["mp_frag", "mp_snow"] {
            let Some(p) = open(map) else { return };
            let Ok(zone) = crate::Zone::open(&zone_dir().join(format!("{map}.ff"))) else { return };
            let (cap, _) = crate::walk::walk_zone(&zone);
            let ents = crate::pathnode::PathNodes::from_entities(&cap.map_ents.first().expect("ents").entity_string);
            assert_eq!(ents.nodes.len(), p.tree_nodes(), "{map}");
            for (e, n) in ents.nodes.iter().zip(&p.nodes) {
                assert_eq!(e.ty, n.ty, "{map} node {}", e.index);
                // (The asset's z is the engine's seat for the node, the editor's z lies above it.)
                assert!((0..2).all(|k| (e.origin[k] - n.origin[k]).abs() < 0.5) && n.origin[2] <= e.origin[2] + 0.5, "{map} node {}", e.index);
                assert_eq!(e.targetname, n.targetname, "{map} node {}", e.index);
                assert_eq!(e.animscript, n.animscript, "{map} node {}", e.index);
            }
            // Each entity traversal is a negotiation link of the asset.
            for neg in &ents.negotiations {
                assert!(p.nodes[neg.begin as usize].links.iter().any(|l| l.negotiation && u32::from(l.to) == neg.end), "{map}: {neg:?}");
            }
        }
    }

    #[test]
    fn every_staged_path_zone_reads_and_checks() {
        let Ok(dir) = std::fs::read_dir(zone_dir()) else { return };
        let mut seen = 0;
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(map) = name.strip_suffix("_path.ff") else { continue };
            let p = PathData::open_map(&zone_dir(), map).unwrap_or_else(|e| panic!("{map}: {e}"));
            p.check().unwrap_or_else(|e| panic!("{map}: {e}"));
            assert!(p.nodes.len() > 100 && p.link_count() > p.nodes.len(), "{map}");
            seen += 1;
        }
        eprintln!("{seen} path zones read");
    }
}

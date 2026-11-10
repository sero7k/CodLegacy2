//! bo2mp: a Black Ops multiplayer map's own AI path nodes (`GameWorldMp`'s
//! `PathData`), as Black Ops II path nodes, so bots route over the nodes the
//! map's designers placed instead of a baked walk graph.
//!
//! A node is 128 bytes: its constant part first, the link count at 0x3e and
//! the links at 0x40. Where the fields before them sit is told apart by the
//! links: each link stores the distance to the node it joins, so the layout
//! whose origins give those distances is the one.

use asset_t6::PathNodeRef;
use fastfile_t5::{Ptr, ScriptStrings, ZonePtr, ZoneStream};

const NODE: usize = 128;
const LINK: usize = 12;
const LINK_COUNT: usize = 0x3e;
const LINKS: usize = 0x40;
/// Black Ops' node kinds run as Black Ops II's (path, the covers,
/// negotiation begin and end...): 21 of them.
const NODE_KINDS: u32 = 21;

#[derive(Clone, Copy)]
struct Layout {
    spawnflags: usize,
    targetname: usize,
    script_linkname: usize,
    script_noteworthy: usize,
    target: usize,
    animscript: usize,
    origin: usize,
    angle: usize,
    radius: usize,
}

const LAYOUTS: [Layout; 2] = [
    Layout {
        spawnflags: 4,
        targetname: 6,
        script_linkname: 8,
        script_noteworthy: 10,
        target: 12,
        animscript: 14,
        origin: 20,
        angle: 32,
        radius: 44,
    },
    Layout {
        spawnflags: 4,
        targetname: 8,
        script_linkname: 10,
        script_noteworthy: 12,
        target: 14,
        animscript: 16,
        origin: 24,
        angle: 36,
        radius: 48,
    },
];

fn links_of(s: &ZoneStream<'_>, node: Ptr) -> Vec<(u16, f32, bool)> {
    let count = s.u16_at(node, LINK_COUNT).unwrap_or(0) as usize;
    let Ok(ZonePtr::Offset(p)) = s.ptr_at(node, LINKS) else {
        return Vec::new();
    };
    let p = s.resolve_alias(p);
    (0..count)
        .filter_map(|k| {
            let at = p.at(k * LINK);
            Some((s.u16_at(at, 4).ok()?, s.f32_at(at, 0).ok()?, s.u8_at(at, 7).ok()? != 0))
        })
        .collect()
}

fn origin(s: &ZoneStream<'_>, node: Ptr, l: &Layout) -> Option<[f32; 3]> {
    let o = [
        s.f32_at(node, l.origin).ok()?,
        s.f32_at(node, l.origin + 4).ok()?,
        s.f32_at(node, l.origin + 8).ok()?,
    ];
    o.iter().all(|v| v.is_finite() && v.abs() < 1.0e6).then_some(o)
}

/// How many of the first links measure what this layout's origins say.
fn score(s: &ZoneStream<'_>, nodes: Ptr, count: usize, l: &Layout) -> (usize, usize) {
    let (mut good, mut seen) = (0, 0);
    for i in 0..count {
        let node = nodes.at(i * NODE);
        let Some(a) = origin(s, node, l) else { continue };
        for (j, dist, _) in links_of(s, node) {
            if usize::from(j) >= count {
                continue;
            }
            let Some(b) = origin(s, nodes.at(usize::from(j) * NODE), l) else { continue };
            let d3 = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            let d2 = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            seen += 1;
            if (dist - d3).abs() <= 1.0 + d3 * 0.01 || (dist - d2).abs() <= 1.0 + d2 * 0.01 {
                good += 1;
            }
        }
        if seen >= 400 {
            break;
        }
    }
    (good, seen)
}

/// The map's path nodes (none when it has none, or they cannot be read).
pub fn path_nodes_t5(s: &ZoneStream<'_>, strings: &ScriptStrings) -> Vec<PathNodeRef> {
    let Some(data) = s.path_data() else { return Vec::new() };
    let (Some(nodes), count) = (data.nodes, data.node_count) else { return Vec::new() };
    if count == 0 {
        return Vec::new();
    }
    let best = LAYOUTS
        .iter()
        .map(|l| (l, score(s, nodes, count, l)))
        .max_by_key(|(_, (good, _))| *good);
    let Some((l, (good, seen))) = best else { return Vec::new() };
    if seen == 0 || good * 2 < seen {
        return Vec::new();
    }
    let text = |node: Ptr, off: usize| -> String {
        s.u16_at(node, off)
            .ok()
            .and_then(|i| strings.get(s, i))
            .unwrap_or("")
            .to_owned()
    };
    (0..count)
        .map(|i| {
            let node = nodes.at(i * NODE);
            let ty = s.u32_at(node, 0).unwrap_or(0);
            PathNodeRef {
                ty: if ty < NODE_KINDS { ty } else { 0 },
                spawnflags: u32::from(s.u16_at(node, l.spawnflags).unwrap_or(0)),
                targetname: text(node, l.targetname),
                target: text(node, l.target),
                script_noteworthy: text(node, l.script_noteworthy),
                script_linkname: text(node, l.script_linkname),
                animscript: text(node, l.animscript),
                origin: origin(s, node, l).unwrap_or([0.0; 3]),
                angle: s.f32_at(node, l.angle).unwrap_or(0.0),
                radius: s.f32_at(node, l.radius).unwrap_or(0.0),
                links: links_of(s, node)
                    .into_iter()
                    .filter(|(j, _, _)| usize::from(*j) < count)
                    .collect(),
            }
        })
        .collect()
}

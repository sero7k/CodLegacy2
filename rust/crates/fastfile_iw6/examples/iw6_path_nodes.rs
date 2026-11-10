//! A zone's path nodes (`node_*` entities): counts by type, spacing and the traversals:
//! `iw6_path_nodes <zone.ff>`.
use fastfile_iw6::pathnode::PathNodes;
fn main() {
    let path = std::env::args().nth(1).expect("zone");
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    for me in &cap.map_ents {
        let p = PathNodes::from_entities(&me.entity_string);
        println!("{}: {} nodes, {} traversals, {} unpaired begins", me.name, p.nodes.len(), p.negotiations.len(), p.dangling_begins.len());
        for (n, c) in p.counts() {
            println!("  {n}: {c}");
        }
        let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let mut nn: Vec<f32> = p
            .nodes
            .iter()
            .map(|a| p.nodes.iter().filter(|b| b.index != a.index).map(|b| d(a.origin, b.origin)).fold(f32::MAX, f32::min))
            .collect();
        nn.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f32| nn[((nn.len() - 1) as f32 * f) as usize];
        println!("  nearest-neighbour distance: min {:.0} p10 {:.0} p50 {:.0} p90 {:.0} max {:.0}", q(0.0), q(0.1), q(0.5), q(0.9), q(1.0));
        let mut anims: std::collections::BTreeMap<&str, usize> = Default::default();
        for n in &p.negotiations {
            *anims.entry(n.animscript.as_str()).or_default() += 1;
        }
        println!("  traversal animations: {anims:?}");
        let mut lens: Vec<f32> = p.negotiations.iter().map(|n| d(p.nodes[n.begin as usize].origin, p.nodes[n.end as usize].origin)).collect();
        lens.sort_by(|a, b| a.total_cmp(b));
        if !lens.is_empty() {
            println!("  traversal length: min {:.0} median {:.0} max {:.0}", lens[0], lens[lens.len() / 2], lens[lens.len() - 1]);
        }
    }
}

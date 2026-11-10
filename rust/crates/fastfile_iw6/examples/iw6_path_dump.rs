//! Summarises a map's path zone: `iw6_path_dump <zone dir> <map>...`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for map in &a[2..] {
        match fastfile_iw6::pathdata::PathData::open_map(std::path::Path::new(&a[1]), map) {
            Ok(p) => {
                let comp = p.components(true);
                let ncomp = comp.iter().copied().max().map_or(0, |m| m + 1);
                let mut sizes = vec![0usize; ncomp as usize];
                for c in &comp { sizes[*c as usize] += 1; }
                sizes.sort_by(|x, y| y.cmp(x));
                println!("{map}: {} nodes ({} in the tree), {} links ({} negotiation), {} zones, {} tree entries, check {:?}, components {} biggest {:?}", p.nodes.len(), p.tree_nodes(), p.link_count(), p.negotiation_links(), p.zone_count, p.tree.len(), p.check(), ncomp, &sizes[..sizes.len().min(4)]);
                for i in [0usize, 1, 1901, 1902, 1964].iter().filter(|&&i| i < p.nodes.len()) { let n = &p.nodes[*i]; println!("  node {i}: {} {:?} yaw {} flags {:#x} tn {:?} tgt {:?} anim {:?} links {}", n.ty.name(), n.origin, n.yaw, n.spawnflags, n.targetname, n.target, n.animscript, n.links.len()); }
            }
            Err(e) => println!("{map}: {e}"),
        }
    }
}

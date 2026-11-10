//! A BO3 clip map's topology: `t7_cliptopo <map.ff>` prints the BSP's
//! reachable leaves, their brushes and AABB tree ranges, and each
//! sub-model's own leaf.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let c = map.clip.as_ref().expect("clip");
    println!("nodes {} leafs {} aabb nodes {} partitions {} cmodels {}", c.nodes.len(), c.leafs.len(), c.aabb_trees.len(), c.partitions.len(), c.models.len());
    let mut reach = vec![false; c.leafs.len()];
    let mut stack = vec![0i32];
    if c.nodes.is_empty() { if !reach.is_empty() { reach[0] = true; } stack.clear(); }
    while let Some(n) = stack.pop() {
        if n < 0 { let l = (-1 - n) as usize; if l < reach.len() { reach[l] = true; } continue; }
        if let Some(node) = c.nodes.get(n as usize) { stack.extend(node.children); }
    }
    let r: Vec<usize> = (0..c.leafs.len()).filter(|&i| reach[i]).collect();
    println!("reachable leaves {:?}", r.len());
    for (i, l) in c.leafs.iter().enumerate() {
        println!("leaf {i} reach {} brushes {} aabb {}+{} mins {:?} maxs {:?}", reach[i], l.brushes.len(), l.first_coll_aabb, l.coll_aabb_count, l.mins, l.maxs);
    }
    for (i, m) in c.models.iter().enumerate().take(100) {
        println!("cmodel {i} brushes {} aabb {}+{}", m.leaf.brushes.len(), m.leaf.first_coll_aabb, m.leaf.coll_aabb_count);
    }
}

//! The decal range's materials: technique and state for the lit slots.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    let map = fastfile_h1::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    let mut seen = std::collections::BTreeMap::new();
    let (b, e) = w.ranges[1];
    for s in &w.surfaces[b as usize..e as usize] {
        if let Some(m) = s.material {
            *seen.entry(m.index).or_insert(0) += 1;
        }
    }
    for (mi, n) in seen.iter().take(12) {
        let m = &cap.materials[*mi as usize];
        let ts = m
            .techset
            .map(|t| cap.name_of(t).to_string())
            .unwrap_or_default();
        let e = m.state_bits_entry.get(15).copied().unwrap_or(255);
        let st = m
            .state_bits
            .get(e as usize)
            .map(|b| render_h1::state::PassState::from_bits(b));
        let tex: Vec<String> = m
            .textures
            .iter()
            .map(|t| {
                format!(
                    "{:08x}:{}:{}",
                    t.name_hash,
                    t.semantic,
                    t.image
                        .map(|i| cap.name_of(i).to_string())
                        .unwrap_or_default()
                )
            })
            .collect();
        println!(
            "{} x{n} sort {} techset {ts}\n   state {st:?}\n   tex {tex:?}\n   consts {:?}",
            m.name,
            m.sort_key,
            m.constants
                .iter()
                .map(|c| (c.name.clone(), c.literal))
                .collect::<Vec<_>>()
        );
    }
}

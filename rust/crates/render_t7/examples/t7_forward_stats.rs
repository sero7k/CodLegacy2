//! `t7_forward_stats <map.ff>`: the draws the exe draws with its forward
//! technique (slot 6: no G-buffer technique), by technique set, and the code
//! textures, buffers and constants those techniques bind.
use std::collections::BTreeMap;
fn main() {
    let zone = std::path::PathBuf::from(std::env::args().nth(1).expect("map zone"));
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    let mut sets: BTreeMap<String, usize> = BTreeMap::new();
    let mut tex: BTreeMap<u32, usize> = BTreeMap::new();
    let mut buf: BTreeMap<u32, usize> = BTreeMap::new();
    let mut cst: BTreeMap<u16, usize> = BTreeMap::new();
    let (mut fwd, mut none, mut pre) = (0, 0, 0);
    for d in &scene.draws {
        let m = &scene.materials[d.material];
        if m.prepass.is_some() {
            pre += 1;
        }
        let Some(t) = &m.forward else {
            if m.technique.is_none() {
                none += 1;
            }
            continue;
        };
        fwd += 1;
        *sets.entry(format!("{} key {:016x}", m.techset, m.sort_key)).or_default() += 1;
        for a in &t.args {
            match a.ty & 0xff00 {
                0x500 => *tex.entry(a.u[0]).or_default() += 1,
                0x600 => *buf.entry(a.u[0]).or_default() += 1,
                0x400 => *cst.entry((a.u[0] & 0xff) as u16).or_default() += 1,
                _ => {}
            }
        }
    }
    println!("draws {} forward {fwd} with nothing {none} with prepass {pre}", scene.draws.len());
    for (k, n) in &sets {
        println!("set {n:5} {k}");
    }
    println!("code textures: {tex:x?}");
    println!("code buffers: {buf:?}");
    let named: Vec<String> = cst.iter().map(|(i, n)| format!("{i:#x} {} ({n})", render_t7::code::name(*i))).collect();
    println!("code constants: {}", named.join(", "));
}

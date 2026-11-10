//! Vertex layer data per world surface: the technique set's world vertex
//! format, the bytes per vertex the layer stream gives it.
//!
//! `cargo run -p render_h1 --example h1_layers -- <map.ff> <techsets.ff>...`

use std::collections::{BTreeMap, HashMap};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let mut wvf: HashMap<String, u8> = HashMap::new();
    let mut map = None;
    let mut techset_of: HashMap<String, String> = HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(p)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        for t in &cap.techsets {
            wvf.insert(render_h1::pool::key(&t.name), t.world_vert_format);
        }
        for mm in &cap.materials {
            if let Some(ts) = mm.techset {
                techset_of
                    .entry(render_h1::pool::key(&mm.name))
                    .or_insert_with(|| render_h1::pool::key(cap.name_of(ts)));
            }
        }
        if i == 0 {
            map = Some((zone, cap));
        }
    }
    let (zone, cap) = map.expect("map");
    let m = fastfile_h1::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let w = m.world.expect("world");
    eprintln!(
        "layer bytes {}, vertices {}",
        w.layer_bytes.len(),
        w.vertex_bytes.len() / 44
    );
    // Distinct layer offsets in order, with the vertex block each starts.
    let mut blocks: BTreeMap<u32, (u32, u16, u8, String)> = BTreeMap::new();
    for s in &w.surfaces {
        let mat = s
            .material
            .map(|id| render_h1::pool::key(cap.name_of(id)))
            .unwrap_or_default();
        let ts = techset_of.get(&mat).cloned().unwrap_or_default();
        let f = wvf.get(&ts).copied().unwrap_or(255);
        let e = blocks
            .entry(s.vertex_layer_data)
            .or_insert((s.first_vertex, 0, f, ts.clone()));
        e.1 = e.1.max(s.vertex_count);
    }
    let keys: Vec<u32> = blocks.keys().copied().collect();
    let mut per_format: BTreeMap<u8, BTreeMap<String, usize>> = BTreeMap::new();
    for (i, k) in keys.iter().enumerate() {
        let next = keys
            .get(i + 1)
            .copied()
            .unwrap_or(w.layer_bytes.len() as u32);
        let (_, vc, f, ts) = &blocks[k];
        let stride = if *vc > 0 {
            format!("{:.2}", f64::from(next - k) / f64::from(*vc))
        } else {
            "-".into()
        };
        let _ = ts;
        *per_format.entry(*f).or_default().entry(stride).or_default() += 1;
    }
    let mut v: Vec<_> = w
        .surfaces
        .iter()
        .map(|s| {
            let mat = s
                .material
                .map(|id| render_h1::pool::key(cap.name_of(id)))
                .unwrap_or_default();
            let ts = techset_of.get(&mat).cloned().unwrap_or_default();
            (
                s.vertex_layer_data,
                s.first_vertex,
                s.vertex_count,
                wvf.get(&ts).copied().unwrap_or(255),
                ts,
            )
        })
        .collect();
    v.sort();
    v.dedup_by_key(|x| (x.0, x.1));
    let mut shown = std::collections::HashSet::new();
    for x in v.iter().filter(|x| x.3 != 0) {
        if !shown.insert(x.3) || x.2 < 3 {
            continue;
        }
        let o = x.0 as usize;
        let row = |k: usize| -> String {
            let b = &w.layer_bytes[o + k * 4..o + k * 4 + 4];
            let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            format!("{:02x}{:02x}{:02x}{:02x}({f:.3})", b[0], b[1], b[2], b[3])
        };
        println!(
            "wvf {}: {}",
            x.3,
            (0..13).map(row).collect::<Vec<_>>().join(" ")
        );
    }
    for (f, s) in per_format {
        println!("wvf {f}: {s:?}");
    }
}

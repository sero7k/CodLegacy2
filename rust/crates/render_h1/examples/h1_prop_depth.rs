//! Placed models' depth and blend state: for every model draw the lit
//! technique picked, its first pass's state (blend, depth write and test,
//! colour writes, cull), and whether the material has a depth pre-pass
//! technique (`zprepass`). An opaque model whose lit pass writes no depth
//! and has no pre-pass is drawn over by whatever comes later.
//!
//! `cargo run -p render_h1 --release --example h1_prop_depth -- <map.ff> <zone.ff>...`
use std::collections::BTreeMap;

use render_h1::world::{pick, slot};

fn main() {
    let paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    let scene = render_h1::scene::Scene::load(&paths).expect("scene");
    for n in &scene.notes {
        println!("note: {n}");
    }
    let lit: &[usize] = &render_h1::host::LIT_SLOTS_SHADOW;
    // (material name, techset) -> (draws, lit slot, lit state, prepass slot, prepass state)
    let mut rows: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let (mut opaque, mut opaque_no_prepass, mut opaque_no_depth_write_no_prepass, mut blended, mut no_tech) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for d in scene.draws.iter().filter(|d| d.mesh == 1) {
        let m = &scene.materials[d.material];
        let Some((ls, lt)) = pick(&scene.pool, &m.techset, lit) else {
            no_tech += 1;
            rows.entry(format!("{} [{}]", m.name, m.techset)).or_insert((0, "NO LIT TECHNIQUE".into())).0 += 1;
            continue;
        };
        let st = m.state(ls, 0);
        let is_opaque = st.blend_state().is_none();
        let pre = pick(&scene.pool, &m.techset, &[slot::ZPREPASS]);
        let pre_tess = pre.is_some_and(|(_, t)| t.pass_info.iter().any(|p| p.hull_shader.is_some()));
        let pre_state = pre.map(|(s, _)| m.state(s, 0));
        if is_opaque {
            opaque += 1;
            if pre.is_none() || pre_tess {
                opaque_no_prepass += 1;
                if !st.depth_write {
                    opaque_no_depth_write_no_prepass += 1;
                }
            }
        } else {
            blended += 1;
        }
        let e = rows.entry(format!("{} [{}]", m.name, m.techset)).or_insert_with(|| {
            (
                0,
                format!(
                    "sort {} lit slot {ls} {} passes {}: blend {:?} dw {} dt {} wm {:#x} cull {} po {} | prepass {} {:?}",
                    m.sort_key,
                    lt.name,
                    lt.pass_info.len(),
                    st.blend,
                    st.depth_write,
                    st.depth_test,
                    st.write_mask,
                    st.cull,
                    st.polygon_offset,
                    match pre {
                        None => "NONE".to_string(),
                        Some((s, t)) => format!("slot {s} {}{}", t.name, if pre_tess { " TESS" } else { "" }),
                    },
                    pre_state.map(|p| (p.depth_write, p.depth_test, p.write_mask)),
                ),
            )
        });
        e.0 += 1;
    }
    println!(
        "model draws: opaque {opaque} (no pre-pass {opaque_no_prepass}, of which lit pass writes no depth {opaque_no_depth_write_no_prepass}), blended {blended}, no lit technique {no_tech}"
    );
    let filter = std::env::var("H1_PROP_FILTER").unwrap_or_default();
    for (name, (n, line)) in &rows {
        if filter.is_empty() || name.contains(&filter) || line.contains(&filter) {
            println!("{n:6} {name}: {line}");
        }
    }
}

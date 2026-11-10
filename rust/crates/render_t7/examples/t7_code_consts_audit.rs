//! `t7_code_consts_audit <map.ff>...`: CPU only. Which engine code
//! constants the map's material techniques read (their arguments of type
//! 0x4xx), per pass, against what `render_t7` fills for that pass: the
//! G-buffer pass (`CodeConsts::view` + `set_time`, as `Host` fills it) and
//! the forward lists (the lighting stage's `view_consts` + sky rotation and
//! sky/sun matrices, as `Host` / `Forward::update` fill them). A constant a
//! program reads that its pass does not fill is bound as zeros: printed with
//! the technique sets and material counts that read it.
use std::collections::{BTreeMap, BTreeSet};

fn main() {
    let zones: Vec<String> = std::env::args().skip(1).collect();
    let view = render_h1::code::View { eye: glam::Vec3::ZERO, angles: glam::Vec3::ZERO, fov_x: 80.0, width: 64, height: 36, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    let mut g = render_t7::code::CodeConsts::view(&cam);
    render_t7::lighting_globals::set_time(&mut g, 0.0, 0);
    let mut f = render_t7::lighting_globals::view_consts(&cam, 64, 36, 0.0, 0);
    for k in [render_t7::lighting_globals::REL_HDR_EXPOSURE, render_t7::lighting_globals::HDR_CONTROL0, render_t7::lighting_globals::HDR_CONTROL1, render_t7::code::SKY_ROTATION_TRANSITION] {
        f.set(k, vec![[0.0; 4]]);
    }
    for k in 0..6 {
        f.set(render_t7::code::SKY_MX_R + k, vec![[0.0; 4]]);
    }
    // The lighting stage's compute programs (core_patch), filled from the
    // same view constants as the forward lists.
    if let Some(dir) = zones.first().and_then(|z| std::path::Path::new(z).parent())
        && let Some(patch) = fastfile_t7::shared::zone(dir, "core_patch")
    {
        let shaders = render_t7::compute::code_shaders(&patch.image, &patch.capture);
        let mut miss: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
        let mut names: Vec<&String> = shaders.keys().filter(|n| n.starts_with("deferred_") || n.starts_with("light_culling")).collect();
        names.sort();
        for n in &names {
            for a in shaders[*n].args.iter().filter(|a| a.ty & 0xff00 == 0x400) {
                let k = (a.u[0] & 0xff) as u16;
                if f.get(k).is_none() {
                    miss.entry(k).or_default().insert((*n).clone());
                }
            }
        }
        println!("lighting programs audited: {}", names.len());
        for (k, ns) in &miss {
            println!("lighting: NOT FILLED: {k:#x} {} read by {}", render_t7::code::name(*k), ns.iter().take(6).cloned().collect::<Vec<_>>().join(", "));
        }
    }
    for z in &zones {
        let scene = render_t7::scene::Scene::load(std::path::Path::new(z)).expect("scene");
        // (pass, constant) -> technique set -> materials.
        let mut missing: BTreeMap<(&str, u16), BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
        let mut read: BTreeMap<(&str, u16), usize> = BTreeMap::new();
        let used: BTreeSet<usize> = scene.draws.iter().map(|d| d.material).collect();
        for &mi in &used {
            let m = &scene.materials[mi];
            for (pass, t, filled) in [("gbuffer", m.gbuffer_pass(), &g), ("forward", m.forward.as_ref(), &f)] {
                let Some(t) = t else { continue };
                for a in t.args.iter().filter(|a| a.ty & 0xff00 == 0x400) {
                    let k = (a.u[0] & 0xff) as u16;
                    *read.entry((pass, k)).or_default() += 1;
                    if filled.get(k).is_none() {
                        missing.entry((pass, k)).or_default().entry(t.techset.clone()).or_default().insert(m.name.clone());
                    }
                }
            }
        }
        println!("{}: code constants read (pass, index name: materials): {}", scene.name, read.iter().map(|((p, k), n)| format!("{p} {k:#x} {}: {n}", render_t7::code::name(*k))).collect::<Vec<_>>().join(", "));
        if missing.is_empty() {
            println!("{}: every code constant read is filled", scene.name);
        }
        for ((pass, k), sets) in &missing {
            let n: usize = sets.values().map(BTreeSet::len).sum();
            let list: Vec<String> = sets.iter().map(|(s, ms)| format!("{s} ({}: {})", ms.len(), ms.iter().take(3).cloned().collect::<Vec<_>>().join(", "))).collect();
            println!("{}: NOT FILLED in the {pass} pass: {k:#x} {} read by {n} materials: {}", scene.name, render_t7::code::name(*k), list.join("; "));
        }
    }
}

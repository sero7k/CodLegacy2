//! bo2mp: Call of Duty: Modern Warfare III (2023) operators as Black Ops II's
//! soldiers: Task Force 141 (Price, Ghost, Soap, Gaz) for the allies and
//! Makarov's men for the axis, in their plain outfits.
//!
//! An operator is a body and a head model of its own game
//! (`fastfile_jup::character`: the head built in the body's `j_spine4`).
//! Each is moved onto Black Ops II's soldier skeleton (the match's own
//! `*_fb` body's bones, names and poses) so Black Ops II's animations,
//! tags and hit locations drive it unchanged:
//!
//! - a bone of Black Ops II's name binds to that bone; another (twist,
//!   corrective, face) to the nearest that has one;
//! - each vertex moves from its bone's place in the operator's bind pose to
//!   that bone's in Black Ops II's, turned by the bone's swing: the
//!   shortest turn taking the operator's limb (the bone to the next down
//!   its chain) onto Black Ops II's. The bones' own axes are not used: the
//!   two games point them differently on some limbs.
//!
//! Its materials are the guns' kind (`jup_gun::read_materials`, from
//! `global_stream_mp`). Nothing of the game is kept: it is read from the
//! install for each run.

use std::collections::HashMap;
use std::sync::OnceLock;

use asset_material::MaterialCatalog;
use asset_model::FpvMeshBuild;
use fastfile_jup::character::{Character, read_character};
use fastfile_jup::gun::{Bone, PartSurface, bone_hash, rotate};

use super::jup_gun::{self, Builder, MaterialPixels};

/// Task Force 141, the allies' (their default outfits).
pub const ALLIES: &[&str] = &["price_10", "ghost_10", "soap_10", "gaz_10"];
/// Makarov's men, the axis' (his field outfits; not his bare base model or
/// his plain clothes).
pub const AXIS: &[&str] = &["makarov_13", "makarov_14", "makarov_21", "makarov_15"];

/// The largest picture side kept.
const MAX: u32 = 1024;

/// The model an operator is drawn as.
#[must_use]
pub fn model_name(operator: &str) -> String {
    format!("jup_op_{operator}")
}

/// The limbs the swing is measured along: each bone to the next.
const CHAINS: &[&[&str]] = &[
    &["j_mainroot", "j_spinelower", "j_spineupper", "j_spine4", "j_neck", "j_head", "j_helmet"],
    &["j_hip_le", "j_knee_le", "j_ankle_le", "j_ball_le"],
    &["j_hip_ri", "j_knee_ri", "j_ankle_ri", "j_ball_ri"],
    &["j_clavicle_le", "j_shoulder_le", "j_elbow_le", "j_wrist_le", "j_mid_le_1", "j_mid_le_2", "j_mid_le_3"],
    &["j_clavicle_ri", "j_shoulder_ri", "j_elbow_ri", "j_wrist_ri", "j_mid_ri_1", "j_mid_ri_2", "j_mid_ri_3"],
    &["j_index_le_1", "j_index_le_2", "j_index_le_3"],
    &["j_index_ri_1", "j_index_ri_2", "j_index_ri_3"],
    &["j_ring_le_1", "j_ring_le_2", "j_ring_le_3"],
    &["j_ring_ri_1", "j_ring_ri_2", "j_ring_ri_3"],
    &["j_pinky_le_1", "j_pinky_le_2", "j_pinky_le_3"],
    &["j_pinky_ri_1", "j_pinky_ri_2", "j_pinky_ri_3"],
    &["j_thumb_le_1", "j_thumb_le_2", "j_thumb_le_3"],
    &["j_thumb_ri_1", "j_thumb_ri_2", "j_thumb_ri_3"],
];

/// An operator read: its body, then its head (moved into the body's space).
struct Operator {
    name: &'static str,
    parts: Vec<Character>,
}

/// What is read from the install (once a run).
struct Read {
    install: std::path::PathBuf,
    operators: Vec<Operator>,
    materials: HashMap<(u64, u32), MaterialPixels>,
    report: Vec<String>,
}

fn read() -> Option<&'static Read> {
    static READ: OnceLock<Option<Read>> = OnceLock::new();
    READ.get_or_init(read_install).as_ref()
}

/// Reads the operators ahead (off the match's critical path).
pub(crate) fn prefetch() {
    let _ = read();
}

fn read_install() -> Option<Read> {
    let install = asset_audio::jup_install()?;
    let oodle = jup_gun::oodle(&install).ok()?;
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let mut operators = Vec::new();
    let part = |zone: String| -> Result<Character, String> {
        let path = install.join("zone").join(format!("{zone}.ff"));
        let (_, z) = fastfile_jup::read_zone(&path, oodle.as_ref()).map_err(|e| format!("{zone}: {e}"))?;
        read_character(&z).map_err(|e| format!("{zone}: {e}"))
    };
    for &name in ALLIES.iter().chain(AXIS) {
        let body = match part(format!("mp_bd_body_c_jup_mp_{name}_lod0_tr")) {
            Ok(b) => b,
            Err(e) => {
                report.push(format!("jup operators: {name}: {e}"));
                continue;
            }
        };
        let mut parts = vec![body];
        match part(format!("mp_hd_head_c_jup_mp_{name}_lod0_tr")) {
            Ok(mut head) => {
                // (A head is built in the body's bone its root names.)
                let at = head
                    .model
                    .bones
                    .first()
                    .and_then(|r| parts[0].model.bones.iter().find(|b| b.name == r.name))
                    .copied();
                match at {
                    Some(at) => {
                        for b in &mut head.model.bones {
                            *b = at.then(b);
                        }
                        for s in &mut head.model.surfaces {
                            for p in &mut s.positions {
                                *p = at.apply(*p);
                            }
                            for n in s.normals.iter_mut().chain(s.tangents.iter_mut()) {
                                *n = rotate(at.rot, *n);
                            }
                        }
                        parts.push(head);
                    }
                    None => report.push(format!("jup operators: {name}: the head's root is not the body's")),
                }
            }
            Err(e) => report.push(format!("jup operators: {name}: no head ({e})")),
        }
        operators.push(Operator { name, parts });
    }
    let mut wanted: Vec<(u64, u32)> = operators
        .iter()
        .flat_map(|o| o.parts.iter())
        .flat_map(|p| p.model.surfaces.iter())
        .filter_map(|s| s.material.map(|m| (m, MAX)))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let materials = match jup_gun::read_materials(&install, oodle.as_ref(), &wanted, false, &mut report) {
        Ok(m) => m,
        Err(e) => {
            report.push(format!("jup operators: {e} (drawn without pictures)"));
            HashMap::new()
        }
    };
    report.push(format!(
        "jup operators: {} read in {:.1} s",
        operators.len(),
        started.elapsed().as_secs_f32()
    ));
    Some(Read {
        install,
        operators,
        materials,
        report,
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// The shortest turn taking direction `a` onto `b` (x, y, z, w).
fn swing(a: [f32; 3], b: [f32; 3]) -> [f32; 4] {
    let (a, b) = (unit(a), unit(b));
    let c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    if d < -0.9999 {
        return [0.0, 0.0, 1.0, 0.0];
    }
    let q = [c[0], c[1], c[2], 1.0 + d];
    let l = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    q.map(|x| x / l)
}

/// One operator on Black Ops II's skeleton (`bo2`: a soldier body).
fn retarget(op: &Operator, bo2: &asset_t6::XModelRef) -> Option<(asset_t6::XModelRef, Vec<Option<u64>>)> {
    let bo2_of: HashMap<u32, usize> = bo2.bone_names.iter().enumerate().map(|(i, n)| (bone_hash(n), i)).collect();
    let bo2_pos = |name: &str| bo2.bone_names.iter().position(|n| n == name).and_then(|i| bo2.base_mat.get(i)).map(|m| m.1);
    let body: &[Bone] = &op.parts.first()?.model.bones;
    let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut lod0 = Vec::new();
    let mut materials = Vec::new();
    for part in &op.parts {
        let bones = &part.model.bones;
        let mw_pos = |name: &str| {
            let h = bone_hash(name);
            bones.iter().chain(body).find(|b| b.name == h).map(|b| b.pos)
        };
        // Each bone: (the bone whose place it moves with, its BO2 bone).
        let direct: Vec<Option<usize>> = bones.iter().map(|b| bo2_of.get(&b.name).copied()).collect();
        let named: Vec<usize> = (0..bones.len()).filter(|&i| direct[i].is_some()).collect();
        if named.is_empty() {
            continue;
        }
        let proxy: Vec<usize> = (0..bones.len())
            .map(|i| match direct[i] {
                Some(_) => i,
                None => *named
                    .iter()
                    .min_by(|&&a, &&b| dist2(bones[a].pos, bones[i].pos).total_cmp(&dist2(bones[b].pos, bones[i].pos)))
                    .unwrap_or(&named[0]),
            })
            .collect();
        let mut turn: HashMap<usize, [f32; 4]> = HashMap::new();
        for chain in CHAINS {
            for w in chain.windows(2) {
                let (Some(a0), Some(a1), Some(b0), Some(b1)) = (mw_pos(w[0]), mw_pos(w[1]), bo2_pos(w[0]), bo2_pos(w[1])) else {
                    continue;
                };
                if let Some(i) = bones.iter().position(|b| b.name == bone_hash(w[0])) {
                    turn.insert(i, swing(sub(a1, a0), sub(b1, b0)));
                }
            }
        }
        let turn_of = |i: usize| -> [f32; 4] {
            turn.get(&i).copied().unwrap_or_else(|| {
                turn.iter()
                    .min_by(|x, y| dist2(bones[*x.0].pos, bones[i].pos).total_cmp(&dist2(bones[*y.0].pos, bones[i].pos)))
                    .map_or([0.0, 0.0, 0.0, 1.0], |x| *x.1)
            })
        };
        let turns: Vec<[f32; 4]> = (0..bones.len()).map(|i| turn_of(proxy[i])).collect();
        for (s, weights) in part.model.surfaces.iter().zip(&part.weights) {
            let n = s.positions.len();
            if n == 0 || n > usize::from(u16::MAX) {
                continue;
            }
            // Each vertex moved, and its BO2 bones (merged, heaviest four).
            let mut moved = PartSurface {
                positions: Vec::with_capacity(n),
                normals: Vec::with_capacity(n),
                tangents: Vec::with_capacity(n),
                uvs: s.uvs.clone(),
                colours: Vec::new(),
                indices: s.indices.clone(),
                material: s.material,
            };
            let mut infl: Vec<Vec<(u16, f32)>> = Vec::with_capacity(n);
            for v in 0..n {
                let (mut p, mut nn, mut tt) = ([0f32; 3], [0f32; 3], [0f32; 3]);
                let mut w: Vec<(u16, f32)> = Vec::new();
                for &(b, x) in &weights[v] {
                    let b = usize::from(b);
                    let Some(&src) = proxy.get(b) else { continue };
                    let Some(t) = direct[src] else { continue };
                    let q = turns[b];
                    let r = rotate(q, sub(s.positions[v], bones[src].pos));
                    let to = bo2.base_mat[t].1;
                    let rn = rotate(q, s.normals[v]);
                    let rt = rotate(q, s.tangents[v]);
                    for k in 0..3 {
                        p[k] += x * (r[k] + to[k]);
                        nn[k] += x * rn[k];
                        tt[k] += x * rt[k];
                    }
                    match w.iter_mut().find(|e| usize::from(e.0) == t) {
                        Some(e) => e.1 += x,
                        None => w.push((t as u16, x)),
                    }
                }
                if w.is_empty() {
                    w.push((0, 1.0));
                    p = s.positions[v];
                }
                w.sort_by(|a, b| b.1.total_cmp(&a.1));
                w.truncate(4);
                let sum: f32 = w.iter().map(|e| e.1).sum();
                for e in &mut w {
                    e.1 /= sum.max(1e-6);
                }
                for k in 0..3 {
                    mins[k] = mins[k].min(p[k]);
                    maxs[k] = maxs[k].max(p[k]);
                }
                moved.positions.push(p);
                moved.normals.push(unit(nn));
                moved.tangents.push(unit(tt));
                infl.push(w);
            }
            let signs = jup_gun::binormal_signs(&moved);
            // Black Ops II orders a surface's vertices by their bone count.
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|&i| infl[i].len());
            let mut new_of = vec![0u16; n];
            for (new, &old) in order.iter().enumerate() {
                new_of[old] = new as u16;
            }
            let mut verts = Vec::with_capacity(n * 32);
            let mut part_bits = [0u32; 6];
            let mut blend_counts = [0i16; 4];
            let mut blend = Vec::new();
            for &old in &order {
                verts.extend_from_slice(&super::t7_map::t6_vertex_signed(
                    moved.positions[old],
                    signs[old],
                    moved.uvs.get(old).copied().unwrap_or([0.0; 2]),
                    moved.normals[old],
                    moved.tangents[old],
                ));
                let w = &infl[old];
                for &(bone, _) in w {
                    if let Some(word) = part_bits.get_mut(usize::from(bone) / 32) {
                        *word |= 1 << (bone % 32);
                    }
                }
                blend_counts[w.len() - 1] += 1;
                blend.push(w[0].0 * 64);
                for &(b, x) in &w[1..] {
                    blend.push(b * 64);
                    blend.push((x * 65536.0).round().clamp(0.0, 65535.0) as u16);
                }
            }
            lod0.push(asset_t6::XSurfaceRef {
                verts,
                vert_count: n as u16,
                indices: moved.indices.iter().map(|&i| new_of.get(i as usize).copied().unwrap_or(0)).collect(),
                vert_lists: Vec::new(),
                flags: 0,
                material: None,
                blend_counts,
                blend,
                part_bits,
            });
            materials.push(s.material);
        }
    }
    if lod0.is_empty() {
        return None;
    }
    let radius = (0..3).map(|c| mins[c].abs().max(maxs[c].abs()).powi(2)).sum::<f32>().sqrt();
    Some((
        asset_t6::XModelRef {
            name: model_name(op.name),
            numsurfs: lod0.len().min(255) as u8,
            materials: Vec::new(),
            num_bones: bo2.num_bones,
            num_lods: 1,
            mins,
            maxs,
            radius,
            lighting_origin_offset: bo2.lighting_origin_offset,
            lighting_origin_range: bo2.lighting_origin_range,
            lods: vec![(0.0, lod0.len() as u16, 0)],
            lod0,
            bone_names: bo2.bone_names.clone(),
            num_root_bones: bo2.num_root_bones,
            parent_list: bo2.parent_list.clone(),
            quats: bo2.quats.clone(),
            trans: bo2.trans.clone(),
            base_mat: bo2.base_mat.clone(),
            part_classification: bo2.part_classification.clone(),
            bone_info: bo2.bone_info.clone(),
            contents: bo2.contents,
            coll_lod: -1,
            coll_surfs: Vec::new(),
            phys_preset: None,
        },
        materials,
    ))
}

pub(crate) use super::t5_soldiers::soldier_skeleton;

/// The operators that were built for this match (their model names), by
/// team (`false`: the allies').
static BUILT: std::sync::Mutex<(Vec<String>, Vec<String>)> = std::sync::Mutex::new((Vec::new(), Vec::new()));

/// The operator model a player of a team is drawn as (by his number, so a
/// team is several of them), when the operators were built.
#[must_use]
pub fn operator_for(axis: bool, client: u32) -> Option<String> {
    let built = BUILT.lock().ok()?;
    let list = if axis { &built.1 } else { &built.0 };
    (!list.is_empty()).then(|| list[client as usize % list.len()].clone())
}

/// bo2mp: the operators as models of the match's first-person and body
/// catalog (`fpv`, Black Ops II's namespace), on `bo2`'s skeleton, their
/// materials linked into `catalog`.
pub(crate) fn add_operators(fpv: &mut FpvMeshBuild, catalog: &mut MaterialCatalog, bo2: Option<&asset_t6::XModelRef>, report: &mut Vec<String>) {
    if let Ok(mut b) = BUILT.lock() {
        *b = Default::default();
    }
    if !crate::mw3_operators() {
        report.push("jup operators: off (Options: SOLDIERS)".into());
        return;
    }
    let Some(read) = read() else {
        report.push("jup operators: Modern Warfare III (2023) is not installed".into());
        return;
    };
    let Some(bo2) = bo2 else {
        report.push("jup operators: no Black Ops II soldier body in the match to take the skeleton from".into());
        return;
    };
    report.extend(read.report.iter().cloned());
    let mut b = Builder::new(std::mem::take(catalog), &read.install);
    let (mut allies, mut axis) = (Vec::new(), Vec::new());
    for op in &read.operators {
        let Some((mut xref, mats)) = retarget(op, bo2) else {
            report.push(format!("jup operators: {}: nothing on the skeleton", op.name));
            continue;
        };
        let linked: Vec<Option<usize>> = mats.iter().map(|&m| b.material(m, MAX, &read.materials)).collect();
        for (i, s) in xref.lod0.iter_mut().enumerate() {
            s.material = linked.get(i).copied().flatten().map(|_| asset_t6::AssetKey {
                ty: fastfile_t6::AssetType::Material,
                index: i,
            });
        }
        xref.materials = xref.lod0.iter().map(|s| s.material).collect();
        let name = xref.name.clone();
        match asset_model::capture_model_skel_t6(&xref, |k| linked.get(k.index).copied().flatten()) {
            Some(skel) => {
                report.push(format!(
                    "jup operators: {name}: {} surfaces, {} vertices, on {}'s skeleton",
                    xref.lod0.len(),
                    xref.lod0.iter().map(|s| usize::from(s.vert_count)).sum::<usize>(),
                    bo2.name
                ));
                fpv.insert_in(asset_core::AssetNamespace::T6, skel, Some(b.catalog_ref()));
                if AXIS.contains(&op.name) {
                    axis.push(name);
                } else {
                    allies.push(name);
                }
            }
            None => report.push(format!("jup operators: {name}: model not built")),
        }
    }
    let (cat, failures) = b.finish();
    *catalog = cat;
    report.extend(failures.into_iter().take(8).map(|f| format!("jup operators: {f}")));
    if let Ok(mut built) = BUILT.lock() {
        *built = (allies, axis);
    }
}

//! bo2mp: parts of a skinned Ghosts model, for the script models that play animations (the crane,
//! the gate's chain).
//!
//! A surface's vertices are skinned by the model's bones: first the blend vertices (counts per
//! number of influences, `blendVerts` in shorts: one short, the bone's offset, for a single bone;
//! the first bone's offset then an (offset, weight) pair per further bone otherwise), then the rigid
//! vertex lists (a bone offset, a vertex count, the triangles of that list). A bone offset is the
//! bone's index times 64 (the size of the bone matrix the engine skins with). Vertices are stored
//! in model space; a bone moves them by `pose * bind^-1`.
//!
//! The renderer has no skinning in its model techniques, so a skinned model is drawn in parts: the
//! triangles are grouped by bone, one object per bone, each moved by that bone's matrix. A triangle
//! of a rigid list belongs to its list's bone; a triangle of the blend vertices to the bone most of
//! its vertices (by weight) follow. A triangle whose vertices follow different bones is drawn whole
//! by one of them (a seam of a link the chain's joints show; APPROXIMATE: the engine skins each
//! vertex by its own weights).

use fastfile_iw6::walk::SurfaceRec;

/// The bone each vertex of the surface follows most (by weight), by index.
pub fn vertex_bones(surf: &SurfaceRec, image: &[u8], num_bones: usize) -> Vec<u16> {
    let n = usize::from(surf.vert_count);
    let mut out = vec![0u16; n];
    let shorts: Vec<u16> = surf
        .blend_verts
        .map(|s| s.get(image).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
        .unwrap_or_default();
    let mut at = 0usize;
    let mut v = 0usize;
    let clamp = |b: u16| -> u16 {
        let i = b / 64;
        if usize::from(i) < num_bones { i } else { 0 }
    };
    for (k, &count) in surf.blend_vert_counts.iter().enumerate() {
        for _ in 0..count {
            if v >= n || at >= shorts.len() {
                return out;
            }
            // k + 1 influences: the first bone takes what the others leave.
            let first = shorts[at];
            let mut best = (clamp(first), 1.0f32);
            let mut rest = 0.0f32;
            let mut others = Vec::new();
            for j in 0..k {
                let (b, w) = (shorts.get(at + 1 + 2 * j).copied().unwrap_or(0), shorts.get(at + 2 + 2 * j).copied().unwrap_or(0));
                let w = f32::from(w) / 65536.0;
                rest += w;
                others.push((clamp(b), w));
            }
            best.1 = (1.0 - rest).max(0.0);
            for (b, w) in others {
                if w > best.1 {
                    best = (b, w);
                }
            }
            out[v] = best.0;
            v += 1;
            at += 1 + 2 * k;
        }
    }
    for &(bone_offset, count, _, _) in &surf.rigid {
        let b = clamp(bone_offset);
        for _ in 0..count {
            if v < n {
                out[v] = b;
                v += 1;
            }
        }
    }
    out
}

/// The surface's triangles grouped by bone: (bone, triangle indices into the surface's vertices).
pub fn bone_triangles(surf: &SurfaceRec, image: &[u8], num_bones: usize) -> Vec<(u16, Vec<u16>)> {
    let bones = vertex_bones(surf, image, num_bones);
    let tris: Vec<u16> = surf.tris.map(|t| t.get(image).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()).unwrap_or_default();
    let count = tris.len() / 3;
    // The rigid lists name their triangles' bone.
    let mut owner: Vec<Option<u16>> = vec![None; count];
    for &(bone_offset, _, tri_offset, tri_count) in &surf.rigid {
        let b = if usize::from(bone_offset / 64) < num_bones { bone_offset / 64 } else { 0 };
        for t in usize::from(tri_offset)..usize::from(tri_offset) + usize::from(tri_count) {
            if let Some(o) = owner.get_mut(t) {
                *o = Some(b);
            }
        }
    }
    let mut groups: Vec<(u16, Vec<u16>)> = Vec::new();
    for t in 0..count {
        let tri = &tris[3 * t..3 * t + 3];
        let b = owner[t].unwrap_or_else(|| {
            let bs: Vec<u16> = tri.iter().map(|&i| bones.get(usize::from(i)).copied().unwrap_or(0)).collect();
            // The bone most of the three follow; the first vertex's on a three-way tie.
            if bs[1] == bs[2] { bs[1] } else { bs[0] }
        });
        match groups.iter_mut().find(|g| g.0 == b) {
            Some(g) => g.1.extend_from_slice(tri),
            None => groups.push((b, tri.to_vec())),
        }
    }
    groups.sort_by_key(|g| g.0);
    groups
}

/// One bone's draws: (material, base vertex, first index, index count) per surface.
pub type BoneDraws = Vec<(Option<fastfile_iw6::walk::AssetId>, i32, u32, u32)>;

/// The model's first LOD appended to the model mesh, split by bone: for each bone that has
/// triangles, its draws (the surfaces' vertices once, an index list per surface and bone).
pub fn model_bone_draws(cap: &fastfile_iw6::walk::Capture, image: &[u8], xm: &fastfile_iw6::walk::XModelRec, model_verts: &mut Vec<u8>, model_idx: &mut Vec<u16>) -> Vec<(u16, BoneDraws)> {
    let mut out: Vec<(u16, BoneDraws)> = Vec::new();
    let Some(lod) = xm.lods.first() else { return out };
    let Some(ms) = lod.surfs.and_then(|s| cap.model_surfs.get(s.index as usize)) else { return out };
    for (j, surf) in ms.surfs.iter().enumerate() {
        let Some(v) = surf.verts else { continue };
        let base = (model_verts.len() / 32) as i32;
        model_verts.extend_from_slice(v.get(image));
        let mat = xm.materials.get(usize::from(lod.surf_index) + j).copied().flatten();
        for (bone, idx) in bone_triangles(surf, image, usize::from(xm.num_bones)) {
            let first = model_idx.len() as u32;
            model_idx.extend_from_slice(&idx);
            let draw = (mat, base, first, idx.len() as u32);
            match out.iter_mut().find(|g| g.0 == bone) {
                Some(g) => g.1.push(draw),
                None => out.push((bone, vec![draw])),
            }
        }
    }
    out.sort_by_key(|g| g.0);
    out
}

/// Whether any surface of the LOD follows a bone other than the first (a model that can be
/// animated visibly, not just carry tags).
pub fn is_skinned(surfs: &[SurfaceRec], image: &[u8], num_bones: usize) -> bool {
    num_bones > 1 && surfs.iter().any(|s| bone_triangles(s, image, num_bones).iter().any(|g| g.0 != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_dir() -> Option<std::path::PathBuf> {
        let p = std::env::var_os("BO2MP_IW6_ZONES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into());
        p.join("mp_frag.ff").is_file().then_some(p)
    }

    #[test]
    fn mp_frags_skinned_models_split_into_their_bones() {
        let Some(dir) = zone_dir() else {
            eprintln!("mp_frag.ff not installed: skipped");
            return;
        };
        let zone = fastfile_iw6::Zone::open(&dir.join("mp_frag.ff")).unwrap();
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let by_name = |n: &str| cap.xmodels.iter().find(|m| m.name == n).unwrap_or_else(|| panic!("no {n}"));
        let surfs = |m: &fastfile_iw6::walk::XModelRec| -> Vec<SurfaceRec> {
            let ms = &cap.model_surfs[m.lods[0].surfs.unwrap().index as usize];
            ms.surfs.clone()
        };
        for (name, bones_used) in [("mp_frag_crane_anim", vec![1u16, 2]), ("pb_gate_chain", (1..=11).collect::<Vec<_>>())] {
            let m = by_name(name);
            let ss = surfs(m);
            assert!(is_skinned(&ss, &zone.image, usize::from(m.num_bones)), "{name}");
            let mut seen = std::collections::BTreeSet::new();
            for s in &ss {
                let groups = bone_triangles(s, &zone.image, usize::from(m.num_bones));
                let total: usize = groups.iter().map(|g| g.1.len()).sum();
                assert_eq!(total, 3 * usize::from(s.tri_count), "{name}: every triangle belongs to a bone");
                assert!(groups.iter().all(|g| usize::from(g.0) < usize::from(m.num_bones)));
                seen.extend(groups.iter().map(|g| g.0));
                for b in vertex_bones(s, &zone.image, usize::from(m.num_bones)) {
                    assert!(usize::from(b) < usize::from(m.num_bones));
                }
            }
            eprintln!("{name}: bones with triangles {seen:?}");
            for b in bones_used {
                assert!(seen.contains(&b), "{name}: no triangles on bone {b}: {seen:?}");
            }
        }
        // A prop whose geometry is all on the first bone is not split.
        let t = by_name("utility_transformer_small01");
        assert!(!is_skinned(&surfs(t), &zone.image, usize::from(t.num_bones)));
    }
}

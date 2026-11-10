//! Ghosts (IW6) placed-model collision for line traces.
//!
//! A placed prop is met only by point traces that ask for it (bullets,
//! sight): a ray against the triangles of the model's collision level of
//! detail. The player's capsule never sees it; it goes through the map's
//! brushes and terrain alone, so movement is unchanged. The spec is
//! `docs/exe_notes/IW6_COLLISION.md` (sections 1.9 and 4).
//!
//! `BO2MP_IW6_MODEL_COLLISION=0` leaves the placed models out (bullets then
//! meet the world's brushes and terrain only, as before).

use std::collections::HashMap;

use fastfile_iw6::walk::{Capture, Span, SurfaceRec, XModelRec};

use super::h1_map::{coll_tri, invert3};

/// The distance a triangle hit is backed off along its normal, and the
/// slack of every surface test.
pub(super) const IW6_MODEL_TRACE_OFFSET: f32 = clipmap_iw4::XMODEL_TRACE_OFFSET;

/// The contents a bullet trace asks for (solid, glass, water, sky,
/// clip-shot and the body types).
pub(super) const IW6_BULLET_MASK: u32 = 0x0280_E831;

/// A bullet goes through at most this many surfaces.
pub(super) const IW6_MAX_PENETRATIONS: usize = 5;

/// The surface flag that stops a bullet at the surface (`nopenetrate`).
const SURF_NOPENETRATE: u32 = 0x100;

/// Whether placed models are traced (default yes).
pub(super) fn enabled() -> bool {
    !std::env::var("BO2MP_IW6_MODEL_COLLISION").is_ok_and(|v| {
        let v = v.trim();
        v == "0" || v.eq_ignore_ascii_case("off")
    })
}

/// The surface type a trace reports: bits 20..24 of the surface flags; a
/// `nopenetrate` surface reports 0 (the default type).
pub(super) fn hit_surface_type(surface_flags: u32) -> u8 {
    if surface_flags & SURF_NOPENETRATE != 0 {
        0
    } else {
        ((surface_flags >> 20) & 0x1f) as u8
    }
}

/// One `XModelCollSurf_s` (36 bytes): bounds (mid, half), bone, contents,
/// surface flags.
struct CollSurf {
    mid: [f32; 3],
    half: [f32; 3],
    bone: i32,
    contents: u32,
    surf_flags: u32,
}

fn read_coll_surfs(bytes: &[u8]) -> Vec<CollSurf> {
    let f = |c: &[u8], o: usize| f32::from_le_bytes([c[o], c[o + 1], c[o + 2], c[o + 3]]);
    let u = |c: &[u8], o: usize| u32::from_le_bytes([c[o], c[o + 1], c[o + 2], c[o + 3]]);
    bytes
        .chunks_exact(36)
        .map(|c| CollSurf {
            mid: [f(c, 0), f(c, 4), f(c, 8)],
            half: [f(c, 12), f(c, 16), f(c, 20)],
            bone: u(c, 24) as i32,
            contents: u(c, 28),
            surf_flags: u(c, 32),
        })
        .collect()
}

/// A model's collision from its collision-level surfaces: each rigid
/// vertex list, in order, takes the model's next collision surface (its
/// bounds, contents and surface flags); the list's triangles index the
/// surface's vertices (32-byte records, position first). Lists left over
/// without a collision surface are counted and left out.
fn coll_from_parts(
    surfs: &[SurfaceRec],
    coll_surfs: &[CollSurf],
    image: &[u8],
    coll_lod: i8,
    contents: u32,
) -> (clipmap_iw4::XModelColl, usize) {
    let mut out_surfs: Vec<clipmap_iw4::XModelCollSurf> = Vec::new();
    let mut unpaired = 0usize;
    for s in surfs {
        let verts = s.verts.map_or(&[][..], |sp| sp.get(image));
        let tris = s.tris.map_or(&[][..], |sp| sp.get(image));
        let pos = |i: u16| -> Option<[f32; 3]> {
            let at = 32 * usize::from(i);
            let v = verts.get(at..at + 12)?;
            Some([0, 4, 8].map(|o| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]])))
        };
        for &(_, _, tri_offset, tri_count) in &s.rigid {
            let Some(cs) = coll_surfs.get(out_surfs.len()) else {
                unpaired += 1;
                continue;
            };
            let first = usize::from(tri_offset);
            let tris = (first..first + usize::from(tri_count))
                .filter_map(|t| {
                    let ix = tris.get(6 * t..6 * t + 6)?;
                    let ix = [0, 2, 4].map(|o| u16::from_le_bytes([ix[o], ix[o + 1]]));
                    coll_tri(pos(ix[0])?, pos(ix[1])?, pos(ix[2])?)
                })
                .collect();
            out_surfs.push(clipmap_iw4::XModelCollSurf {
                tris,
                midpoint: cs.mid,
                half_size: cs.half,
                bone_idx: cs.bone,
                contents: cs.contents,
                surf_flags: cs.surf_flags,
            });
        }
    }
    (
        clipmap_iw4::XModelColl {
            trace_offset: IW6_MODEL_TRACE_OFFSET,
            coll_lod: i16::from(coll_lod),
            contents,
            surfs: out_surfs.into(),
        },
        unpaired,
    )
}

fn xmodel_coll(cap: &Capture, image: &[u8], model: &XModelRec) -> Option<(clipmap_iw4::XModelColl, usize)> {
    if model.coll_lod < 0 {
        return None;
    }
    let surfs = model
        .lods
        .get(model.coll_lod as usize)
        .and_then(|l| l.surfs)
        .and_then(|s| cap.model_surfs.get(s.index as usize))?;
    let raw: &[u8] = model.coll_surfs.as_ref().map_or(&[][..], |sp: &Span| sp.get(image));
    Some(coll_from_parts(&surfs.surfs, &read_coll_surfs(raw), image, model.coll_lod, model.contents as u32))
}

/// What the clip map's placed models give the line trace.
#[derive(Default)]
pub(super) struct ModelCollision {
    pub models: Vec<asset_world::ClipPlacedStaticModel>,
    pub no_lod: usize,
    pub skipped: usize,
    pub tris: usize,
    pub unpaired: usize,
}

pub(super) fn model_collision(cap: &Capture, image: &[u8], clip: &fastfile_iw6::map::Clip) -> ModelCollision {
    let mut out = ModelCollision::default();
    let mut cache: HashMap<u32, Option<(clipmap_iw4::XModelColl, usize)>> = HashMap::new();
    for (index, sm) in clip.static_models.iter().enumerate() {
        let Some(id) = sm.model else {
            out.skipped += 1;
            continue;
        };
        let Some(model) = cap.xmodels.get(id.index as usize) else {
            out.skipped += 1;
            continue;
        };
        let Some((coll, unpaired)) = cache.entry(id.index).or_insert_with(|| xmodel_coll(cap, image, model)).clone()
        else {
            out.no_lod += 1;
            continue;
        };
        out.unpaired += unpaired;
        out.tris += coll.surfs.iter().map(|s| s.tris.len()).sum::<usize>();
        out.models.push(asset_world::ClipPlacedStaticModel {
            index: index as u32,
            name: model.name.clone(),
            model: clipmap_iw4::ClipStaticModel {
                origin: sm.origin,
                inv_scaled_axis: sm.inv_scaled_axis,
                bounds_mid: std::array::from_fn(|k| (sm.mins[k] + sm.maxs[k]) * 0.5),
                bounds_half: std::array::from_fn(|k| ((sm.maxs[k] - sm.mins[k]) * 0.5).max(0.0)),
                coll,
            },
        });
    }
    out
}

/// Whether the props that are not static models (dynamic entities,
/// scriptables, script models) are traced too (default yes).
pub(super) fn props_enabled() -> bool {
    !std::env::var("BO2MP_IW6_PROP_COLLISION").is_ok_and(|v| {
        let v = v.trim();
        v == "0" || v.eq_ignore_ascii_case("off")
    })
}

/// The props `fastfile_iw6::map::extra_props` places (clutter, destructibles,
/// the scriptables' bodies, script models) as placed models for line
/// traces: each model's own collision LOD at the prop's pose, the same test
/// as a static model's (bullets and sight meet them, the player's capsule
/// does not). Numbered from `first_index` on.
pub(super) fn prop_collision(
    cap: &Capture,
    image: &[u8],
    props: &[fastfile_iw6::map::ExtraProp],
    first_index: u32,
) -> ModelCollision {
    let mut out = ModelCollision::default();
    let mut cache: HashMap<u32, Option<(clipmap_iw4::XModelColl, usize)>> = HashMap::new();
    for (k, p) in props.iter().enumerate() {
        let Some(model) = cap.xmodels.get(p.model as usize) else {
            out.skipped += 1;
            continue;
        };
        let Some((coll, unpaired)) = cache.entry(p.model).or_insert_with(|| xmodel_coll(cap, image, model)).clone()
        else {
            out.no_lod += 1;
            continue;
        };
        if coll.surfs.is_empty() {
            out.no_lod += 1;
            continue;
        }
        out.unpaired += unpaired;
        out.tris += coll.surfs.iter().map(|s| s.tris.len()).sum::<usize>();
        let a = p.axis;
        // world = origin + local . axis rows; the inverse of the rotation is
        // its transpose.
        let inv = [[a[0][0], a[1][0], a[2][0]], [a[0][1], a[1][1], a[2][1]], [a[0][2], a[1][2], a[2][2]]];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for s in coll.surfs.iter() {
            for c in 0..8 {
                let l: [f32; 3] = std::array::from_fn(|i| {
                    s.midpoint[i] + if c & (1 << i) == 0 { -s.half_size[i] } else { s.half_size[i] }
                });
                for i in 0..3 {
                    let w = p.origin[i] + l[0] * a[0][i] + l[1] * a[1][i] + l[2] * a[2][i];
                    lo[i] = lo[i].min(w);
                    hi[i] = hi[i].max(w);
                }
            }
        }
        out.models.push(asset_world::ClipPlacedStaticModel {
            index: first_index + k as u32,
            name: model.name.clone(),
            model: clipmap_iw4::ClipStaticModel {
                origin: p.origin,
                inv_scaled_axis: inv,
                bounds_mid: std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5),
                bounds_half: std::array::from_fn(|i| ((hi[i] - lo[i]) * 0.5).max(0.0)),
                coll,
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullet_mask_and_penetration_cap() {
        assert_eq!(IW6_BULLET_MASK, 0x0280_E831);
        assert_eq!(IW6_MAX_PENETRATIONS, 5);
        assert_eq!(IW6_MODEL_TRACE_OFFSET, 0.125);
    }

    #[test]
    fn surface_type_from_flags() {
        // Bits 20..24; the rest of the word is ignored.
        assert_eq!(hit_surface_type(13 << 20), 13);
        assert_eq!(hit_surface_type((22 << 20) | 0x2 | 0x8000_0000), 22);
        // nopenetrate reports the default type.
        assert_eq!(hit_surface_type((13 << 20) | 0x100), 0);
        assert_eq!(hit_surface_type(0), 0);
    }

    fn f(v: f32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// One 10 x 10 triangle in z = 0, seen clockwise from above, in one
    /// rigid list, with one collision surface of surface type 13 (metal).
    fn fixture() -> (Vec<u8>, Vec<SurfaceRec>, Vec<CollSurf>) {
        let mut image = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 0.0, 0.0]] {
            let mut v = Vec::new();
            for c in p {
                v.extend(f(c));
            }
            v.resize(32, 0);
            image.extend(v);
        }
        let tri_at = image.len();
        for i in [0u16, 1, 2] {
            image.extend(i.to_le_bytes());
        }
        let surf = SurfaceRec {
            verts: Some(Span { pos: 0, len: 96 }),
            tris: Some(Span { pos: tri_at, len: 6 }),
            rigid: vec![(0, 3, 0, 1)],
            ..Default::default()
        };
        let cs = CollSurf {
            mid: [5.0, 5.0, 0.0],
            half: [5.0, 5.0, 0.5],
            bone: 0,
            contents: 1,
            surf_flags: 13 << 20,
        };
        (image, vec![surf], vec![cs])
    }

    #[test]
    fn placed_model_is_hit_by_a_line_and_reports_its_surface() {
        let (image, surfs, cs) = fixture();
        let (coll, unpaired) = coll_from_parts(&surfs, &cs, &image, 0, 1);
        assert_eq!(unpaired, 0);
        assert_eq!(coll.surfs.len(), 1);
        assert_eq!(coll.surfs[0].tris.len(), 1);
        let sm = clipmap_iw4::ClipStaticModel {
            origin: [100.0, 0.0, 0.0],
            inv_scaled_axis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            bounds_mid: [105.0, 5.0, 0.0],
            bounds_half: [6.0, 6.0, 1.0],
            coll,
        };
        let shoot = |from: f32, to: f32, x: f32, mask: u32| {
            let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
            let hit = clipmap_iw4::point_trace_static_models([&sm], [x, 3.0, from], [x, 3.0, to], mask, &mut tr);
            (hit.is_some(), tr)
        };
        let (hit, tr) = shoot(50.0, -50.0, 103.0, IW6_BULLET_MASK);
        assert!(hit);
        // Backed off the plane by the offset along the normal (up), so a
        // little short of the crossing at fraction 0.5.
        assert!(tr.fraction < 0.5 && tr.fraction > 0.49, "{}", tr.fraction);
        assert!(tr.normal[2] > 0.99);
        assert_eq!(hit_surface_type(tr.surface_flags), 13);
        assert!(!shoot(-50.0, 50.0, 103.0, IW6_BULLET_MASK).0, "from behind");
        assert!(!shoot(50.0, -50.0, 130.0, IW6_BULLET_MASK).0, "beside");
        assert!(!shoot(50.0, -50.0, 103.0, 0x2).0, "contents not in the mask");
    }

    #[test]
    fn a_turned_prop_is_hit_where_it_stands() {
        let (image, surfs, cs) = fixture();
        let (coll, _) = coll_from_parts(&surfs, &cs, &image, 0, 1);
        // Turned 90 degrees about z (forward = +y, left = -x), at (100, 0, 0).
        let axis = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let inv = [[axis[0][0], axis[1][0], axis[2][0]], [axis[0][1], axis[1][1], axis[2][1]], [axis[0][2], axis[1][2], axis[2][2]]];
        let sm = clipmap_iw4::ClipStaticModel {
            origin: [100.0, 0.0, 0.0],
            inv_scaled_axis: inv,
            bounds_mid: [95.0, 5.0, 0.0],
            bounds_half: [6.0, 6.0, 1.0],
            coll,
        };
        // Local (3, 3) is world (100 - 3, 3).
        let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
        assert!(clipmap_iw4::point_trace_static_models([&sm], [97.0, 3.0, 50.0], [97.0, 3.0, -50.0], IW6_BULLET_MASK, &mut tr).is_some());
        let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
        assert!(clipmap_iw4::point_trace_static_models([&sm], [103.0, 3.0, 50.0], [103.0, 3.0, -50.0], IW6_BULLET_MASK, &mut tr).is_none());
    }

    #[test]
    fn unpaired_lists_are_counted() {
        let (image, surfs, _) = fixture();
        let (coll, unpaired) = coll_from_parts(&surfs, &[], &image, 0, 1);
        assert_eq!((coll.surfs.len(), unpaired), (0, 1));
    }

    #[test]
    fn collision_surface_records_are_read() {
        let mut b = Vec::new();
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
            b.extend(f(v));
        }
        b.extend(7i32.to_le_bytes());
        b.extend(0x2000u32.to_le_bytes());
        b.extend(0x0050_0000u32.to_le_bytes());
        let cs = read_coll_surfs(&b);
        assert_eq!(cs.len(), 1);
        assert_eq!((cs[0].bone, cs[0].contents), (7, 0x2000));
        assert_eq!(hit_surface_type(cs[0].surf_flags), 5);
        assert_eq!(cs[0].half, [4.0, 5.0, 6.0]);
    }

    /// Real-zone check (`GHOSTS_TEST_MAP=<mp_frag.ff> cargo test -p assets --lib
    /// iw6_model_collision_real_zone -- --ignored --nocapture`): counts, triangle
    /// bounds against the collision surface and placement bounds, sample traces.
    #[test]
    #[ignore]
    fn iw6_model_collision_real_zone() {
        let Ok(path) = std::env::var("GHOSTS_TEST_MAP") else { return };
        let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).unwrap();
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
        let src = map.clip.as_ref().expect("clip");
        let mc = model_collision(&cap, &zone.image, src);
        println!(
            "placements {} -> models {} tris {} no_lod {} skipped {} unpaired {}",
            src.static_models.len(), mc.models.len(), mc.tris, mc.no_lod, mc.skipped, mc.unpaired
        );
        let (mut tri_n, mut out_surf, mut out_place, mut empty_surfs) = (0usize, 0usize, 0usize, 0usize);
        let mut worst = 0.0f32;
        for pm in &mc.models {
            let sm = &pm.model;
            let sp = &src.static_models[pm.index as usize];
            // local = inv * (world - origin)  =>  world = origin + inv^-1 * local
            let axis = invert3(sm.inv_scaled_axis);
            for surf in sm.coll.surfs.iter() {
                if surf.tris.is_empty() { empty_surfs += 1; }
                for t in surf.tris.iter() {
                    // Recover a vertex: plane point a = n*d is not a vertex; use the
                    // barycentric rows instead: sample the centroid of the plane patch.
                    let _ = t;
                }
            }
            let _ = (axis, sp);
        }
        // Vertex-level check needs the corners: recompute from the zone.
        let mut cache: HashMap<u32, Vec<Vec<[[f32; 3]; 3]>>> = HashMap::new();
        for pm in &mc.models {
            let sp = &src.static_models[pm.index as usize];
            let id = sp.model.unwrap();
            let model = &cap.xmodels[id.index as usize];
            let corners = cache.entry(id.index).or_insert_with(|| {
                let surfs = &cap.model_surfs[model.lods[model.coll_lod as usize].surfs.unwrap().index as usize];
                let mut out = Vec::new();
                for s in &surfs.surfs {
                    let verts = s.verts.map_or(&[][..], |x| x.get(&zone.image));
                    let tris = s.tris.map_or(&[][..], |x| x.get(&zone.image));
                    for &(_, _, off, cnt) in &s.rigid {
                        let mut list = Vec::new();
                        for t in usize::from(off)..usize::from(off) + usize::from(cnt) {
                            let Some(ix) = tris.get(6 * t..6 * t + 6) else { continue };
                            let p = |o: usize| {
                                let i = usize::from(u16::from_le_bytes([ix[o], ix[o + 1]]));
                                let v = &verts[32 * i..32 * i + 12];
                                [0, 4, 8].map(|k| f32::from_le_bytes([v[k], v[k + 1], v[k + 2], v[k + 3]]))
                            };
                            list.push([p(0), p(2), p(4)]);
                        }
                        out.push(list);
                    }
                }
                out
            });
            let csurfs = pm.model.coll.surfs.clone();
            let (lo, hi): ([f32; 3], [f32; 3]) = (sp.mins, sp.maxs);
            let axis = invert3(sp.inv_scaled_axis).unwrap();
            for (cs, list) in csurfs.iter().zip(corners.iter()) {
                for tri in list {
                    tri_n += 1;
                    for v in tri {
                        let over = (0..3).map(|k| ((v[k] - cs.midpoint[k]).abs() - cs.half_size[k]).max(0.0)).fold(0.0, f32::max);
                        worst = worst.max(over);
                        if over > 0.5 { out_surf += 1; }
                        let w: [f32; 3] = std::array::from_fn(|k| sp.origin[k] + v[0] * axis[0][k] + v[1] * axis[1][k] + v[2] * axis[2][k]);
                        if (0..3).any(|k| w[k] < lo[k] - 2.0 || w[k] > hi[k] + 2.0) { out_place += 1; }
                    }
                }
            }
        }
        println!("verts outside coll-surf bounds (>0.5): {out_surf} (worst {worst}); outside placement bounds (+2): {out_place}; tris {tri_n}; surfaces with no tris {empty_surfs}");
        // Sample traces: down through and sideways into the first models with tris.
        let mut shown = 0;
        for pm in mc.models.iter().filter(|m| m.model.coll.surfs.iter().any(|s| !s.tris.is_empty())) {
            let sm = &pm.model;
            let c = sm.bounds_mid;
            let h = sm.bounds_half;
            let mut run = |a: [f32; 3], b: [f32; 3]| {
                let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
                let hit = clipmap_iw4::point_trace_static_models([sm], a, b, IW6_BULLET_MASK, &mut tr);
                (hit.is_some(), tr.fraction, hit_surface_type(tr.surface_flags))
            };
            let top = c[2] + h[2] + 20.0;
            let down = run([c[0], c[1], top], [c[0], c[1], c[2] - h[2] - 20.0]);
            let side = run([c[0] - h[0] - 30.0, c[1], c[2]], [c[0] + h[0] + 30.0, c[1], c[2]]);
            println!(
                "{} half {:?}: down hit {} at {:.1} (type {}); side hit {} at {:.1} of {:.1}",
                pm.name, h.map(|x| (x * 10.0).round() / 10.0), down.0,
                down.1 * (2.0 * h[2] + 40.0), down.2, side.0, side.1 * (2.0 * h[0] + 60.0), 2.0 * h[0] + 60.0
            );
            shown += 1;
            if shown >= 12 { break; }
        }
        // Hit rate over all models: a down trace through each centre.
        let (mut hits, mut tot) = (0, 0);
        for pm in mc.models.iter() {
            let sm = &pm.model;
            let (c, h) = (sm.bounds_mid, sm.bounds_half);
            let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
            tot += 1;
            if clipmap_iw4::point_trace_static_models([sm], [c[0], c[1], c[2] + h[2] + 20.0], [c[0], c[1], c[2] - h[2] - 20.0], IW6_BULLET_MASK, &mut tr).is_some() { hits += 1; }
        }
        println!("down-through-centre hits: {hits} of {tot}");
        // The props that are not static models.
        let props = fastfile_iw6::map::extra_props(&map, &cap, fastfile_iw6::map::alternate_level_cut(&map.spawns));
        let pc = prop_collision(&cap, &zone.image, &props, src.static_models.len() as u32);
        let (mut ph, mut pt) = (0, 0);
        for pm in &pc.models {
            let (c, h) = (pm.model.bounds_mid, pm.model.bounds_half);
            let mut tr = trace_iw4::Trace { fraction: 1.0, ..Default::default() };
            pt += 1;
            if clipmap_iw4::point_trace_static_models([&pm.model], [c[0], c[1], c[2] + h[2] + 20.0], [c[0], c[1], c[2] - h[2] - 20.0], IW6_BULLET_MASK, &mut tr).is_some() { ph += 1; }
        }
        println!("props {} -> traced {} tris {} no_lod {}; down-through-centre hits {ph} of {pt}", props.len(), pc.models.len(), pc.tris, pc.no_lod);
    }
}

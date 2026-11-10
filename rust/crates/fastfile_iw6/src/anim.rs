//! bo2mp: a Ghosts zone's animations and skeletons as `iw6_pose` data (script-model animation).
//!
//! The animations are decoded by the engine's IW-family decoder (`xmodel_runtime::AnimClip`, which
//! refuses a data stream it does not use up exactly; the same one the guns' animations go through)
//! and sampled at every frame; the skeletons come from the model's tag table (parent list, local
//! bind quaternions and positions) and its `baseMat` (the bind pose in model space).

use iw6_pose::{Clip, Skeleton};

use crate::walk::{Capture, Span, XAnimRec, XModelRec};

fn span<'a>(image: &'a [u8], s: Option<Span>) -> &'a [u8] {
    s.map_or(&[][..], |s| s.get(image))
}

fn le_u16s(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn le_u32s(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn string(strings: &[Option<String>], i: u32) -> String {
    strings.get(i as usize).cloned().flatten().unwrap_or_default()
}

/// The raw animation as the decoder takes it.
pub fn raw_parts(a: &XAnimRec, image: &[u8], strings: &[Option<String>]) -> xmodel_runtime::RawXAnimParts {
    let indices = if a.numframes >= 256 {
        le_u16s(span(image, a.indices))
    } else {
        span(image, a.indices).iter().map(|&b| u16::from(b)).collect()
    };
    xmodel_runtime::RawXAnimParts {
        name: a.name.trim_start_matches(',').to_ascii_lowercase(),
        data_byte: span(image, a.data_byte).to_vec(),
        data_short: le_u16s(span(image, a.data_short)),
        data_int: le_u32s(span(image, a.data_int)),
        random_data_byte: span(image, a.random_data_byte).to_vec(),
        random_data_short: le_u16s(span(image, a.random_data_short)),
        random_data_int: le_u32s(span(image, a.random_data_int)),
        numframes: a.numframes,
        flags: a.flags,
        bone_count: a.bone_count,
        framerate: a.framerate,
        names: a.names.iter().map(|&s| string(strings, s).to_ascii_lowercase()).collect(),
        notifies: a
            .notify
            .iter()
            .map(|&(s, t)| xmodel_runtime::ClipNotify { name: string(strings, s), time: t })
            .collect(),
        indices,
        delta_trans: delta_trans(a, image),
    }
}

fn f32_at(image: &[u8], at: usize) -> Option<f32> {
    image.get(at..at + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The root bone's delta translation as the decoder takes it (READ from the walk: a single frame of
/// three floats when `size` is 0, else mins, step, `size + 1` frame numbers and `size + 1` packed
/// frames, 3 bytes each when small, else 3 u16s). None when the animation has no delta part, or its
/// parts are shared with another asset (the frames pointer is not inline).
pub fn delta_trans(a: &XAnimRec, image: &[u8]) -> Option<xmodel_runtime::RawDeltaTrans> {
    let d = a.delta.as_ref()?;
    let (size, small, _) = d.trans?;
    let at = d.trans_at?;
    let head = at.head?;
    if size == 0 {
        let mins = [f32_at(image, head)?, f32_at(image, head + 4)?, f32_at(image, head + 8)?];
        return Some(xmodel_runtime::RawDeltaTrans { size, small, mins, step: [0.0; 3], indices: Vec::new(), packed: Vec::new() });
    }
    let n = usize::from(size) + 1;
    let mins = [f32_at(image, head)?, f32_at(image, head + 4)?, f32_at(image, head + 8)?];
    let step = [f32_at(image, head + 12)?, f32_at(image, head + 16)?, f32_at(image, head + 20)?];
    let ia = at.indices?;
    let indices: Vec<u16> = if a.numframes >= 256 {
        image.get(ia..ia + 2 * n)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    } else {
        image.get(ia..ia + n)?.iter().map(|&b| u16::from(b)).collect()
    };
    let per = if small { 3 } else { 6 };
    let packed = image.get(at.frames?..at.frames? + per * n)?.to_vec();
    Some(xmodel_runtime::RawDeltaTrans { size, small, mins, step, indices, packed })
}

/// One animation, decoded and sampled at frames `0..=numframes`.
pub fn clip(a: &XAnimRec, image: &[u8], strings: &[Option<String>]) -> Result<Clip, String> {
    let mut raw = raw_parts(a, image, strings);
    // (The delta part's presence is the data's; the decoder also wants the flag.)
    if raw.delta_trans.is_some() {
        raw.flags |= 2;
    }
    if a.names.len() != usize::from(a.bone_count[9]) {
        return Err(format!("{}: {} bone names for {} tracks", raw.name, a.names.len(), a.bone_count[9]));
    }
    let decoded = xmodel_runtime::AnimClip::from_parts(&raw).map_err(|e| format!("{}: {e:?}", raw.name))?;
    let frames = u32::from(a.numframes.max(1));
    let mut rot = Vec::new();
    let mut trans = Vec::new();
    for (t, track) in decoded.tracks.iter().enumerate() {
        let has_rot = !matches!(track.rotation, xmodel_runtime::Rotation::Default);
        let has_trans = !matches!(track.translation, xmodel_runtime::Translation::Default);
        let mut r = Vec::new();
        let mut p = Vec::new();
        for f in 0..=frames {
            let s = decoded.sample_track(t, f as f32 / a.framerate.max(f32::EPSILON)).unwrap_or_default();
            if has_rot {
                r.push(s.rotation.unwrap_or(iw6_pose::IDENTITY));
            }
            if has_trans {
                p.push(s.translation.unwrap_or([0.0; 3]));
            }
        }
        rot.push(r);
        trans.push(p);
    }
    // The root motion: the absolute delta translation at every frame (empty: the clip has none).
    let delta = if decoded.has_delta() {
        (0..=frames).map(|f| decoded.abs_delta_trans(f as f32 / frames as f32)).collect()
    } else {
        Vec::new()
    };
    Ok(Clip {
        name: raw.name,
        frames,
        rate: a.framerate,
        looping: a.flags & 1 != 0,
        bones: raw.names,
        notes: raw.notifies.into_iter().map(|n| (n.name, n.time)).collect(),
        rot,
        trans,
        delta,
    })
}

/// Every animation of the zone that decodes (the ones that do not are named in the second list).
pub fn clips(cap: &Capture, image: &[u8], strings: &[Option<String>]) -> (Vec<Clip>, Vec<String>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for a in &cap.xanims {
        // (A comma-named asset is a reference to another zone's.)
        if a.name.starts_with(',') || a.name.is_empty() {
            continue;
        }
        match clip(a, image, strings) {
            Ok(c) => ok.push(c),
            Err(e) => bad.push(e),
        }
    }
    (ok, bad)
}

/// A model's skeleton: bone names, parents, bind poses.
pub fn skeleton(xm: &XModelRec, strings: &[Option<String>]) -> Option<Skeleton> {
    let n = usize::from(xm.num_bones);
    let roots = usize::from(xm.num_root_bones);
    if n == 0 || xm.bone_names.len() != n || xm.base_mat.len() < n || xm.parent_list.len() + roots < n {
        return None;
    }
    let bones: Vec<String> = xm.bone_names.iter().map(|&s| string(strings, s).to_ascii_lowercase()).collect();
    let world: Vec<iw6_pose::Bone> = xm.base_mat[..n].iter().map(|m| (iw6_pose::normalize([m[0], m[1], m[2], m[3]]), [m[4], m[5], m[6]])).collect();
    let mut parents = Vec::with_capacity(n);
    let mut local = Vec::with_capacity(n);
    for i in 0..n {
        if i < roots {
            parents.push(None);
            local.push(world[i]);
            continue;
        }
        let k = i - roots;
        // (READ: a non-root bone's parent is the bone `parentList[k]` places before it.)
        let back = usize::from(*xm.parent_list.get(k)?);
        let parent = i.checked_sub(back)?;
        parents.push(Some(parent as u16));
        let q = xm.tag_angles.get(k)?;
        let t = xm.tag_positions.get(k)?;
        local.push((iw6_pose::normalize(q.map(|c| f32::from(c) / 32767.0)), *t));
    }
    Some(Skeleton { name: xm.name.trim_start_matches(',').to_ascii_lowercase(), bones, parents, local, world })
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
    fn mp_frags_animations_decode_and_say_what_the_data_says() {
        let Some(dir) = zone_dir() else {
            eprintln!("mp_frag.ff not installed: skipped");
            return;
        };
        let zone = crate::Zone::open(&dir.join("mp_frag.ff")).unwrap();
        let (cap, _) = crate::walk::walk_zone(&zone);
        let (clips, bad) = clips(&cap, &zone.image, &zone.assets.script_strings);
        assert!(bad.is_empty(), "{bad:?}");
        let get = |n: &str| clips.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("no {n}"));
        // The gate opens over 59 frames at 30 per second; not a loop; the data's `end` notetrack.
        let open = get("mp_frag_metal_door_open");
        assert_eq!((open.frames, open.rate, open.looping), (59, 30.0, false));
        assert_eq!(open.length_ms(), 1966);
        assert!(open.notes.iter().any(|(n, t)| n == "end" && (*t - 1.0).abs() < 1e-6));
        assert_eq!(open.bones, vec!["j_prop_1", "j_prop_2"]);
        // The gate's data carries no root motion unless a delta part says so; whichever clips have
        // one decode to a value per frame and start the walk at the origin or the data's own start.
        for c in &clips {
            assert!(c.delta.is_empty() || c.delta.len() == c.frames as usize + 1, "{}: {} delta frames", c.name, c.delta.len());
        }
        eprintln!("clips with root motion: {:?}", clips.iter().filter(|c| !c.delta.is_empty()).map(|c| (&c.name, c.delta.last().copied())).collect::<Vec<_>>());
        // The crane sways forever.
        let sway = get("mp_frag_crane_sway");
        assert!(sway.looping);
        assert_eq!(sway.bones, vec!["j_crane_top", "j_crane_container", "j_crane_hook"]);
        // The closed pose is the bind pose: no tracks.
        let closed = get("mp_frag_metal_door_closed_loop");
        assert!(closed.rot.iter().all(Vec::is_empty) && closed.trans.iter().all(Vec::is_empty));
        // The prop's skeleton and the open clip: the pieces' tags move between the first and last frame.
        let prop = cap.xmodels.iter().find(|m| m.name == "generic_prop_raven").unwrap();
        let skel = skeleton(prop, &zone.assets.script_strings).unwrap();
        assert_eq!(skel.bones, vec!["tag_origin", "j_prop_1", "j_prop_2"]);
        assert_eq!(skel.parents, vec![None, Some(0), Some(0)]);
        let first = iw6_pose::pose(&skel, Some(open), 0.0);
        let last = iw6_pose::pose(&skel, Some(open), open.time_secs(5000));
        let moved = |a: iw6_pose::Bone, b: iw6_pose::Bone| {
            let d: f32 = (0..3).map(|k| (a.1[k] - b.1[k]).powi(2)).sum::<f32>().sqrt();
            let q: f32 = a.0.iter().zip(&b.0).map(|(x, y)| x * y).sum::<f32>().abs();
            (d, q)
        };
        let (d1, q1) = moved(first[1], last[1]);
        let (d2, q2) = moved(first[2], last[2]);
        eprintln!("gate open: j_prop_1 moves {d1:.1} units (quat dot {q1:.3}), j_prop_2 {d2:.1} ({q2:.3})");
        assert!(d1 > 1.0 || q1 < 0.99, "the left piece does not move");
        assert!(d2 > 1.0 || q2 < 0.99, "the right piece does not move");
        // The crane's skeleton hangs its container below the top.
        let crane = cap.xmodels.iter().find(|m| m.name == "mp_frag_crane_anim").unwrap();
        let cs = skeleton(crane, &zone.assets.script_strings).unwrap();
        assert_eq!(cs.bones.len(), 5);
        let chain = cap.xmodels.iter().find(|m| m.name == "pb_gate_chain").unwrap();
        let ch = skeleton(chain, &zone.assets.script_strings).unwrap();
        // The composed local poses are the model's own bind pose in model space (a check of the parent rule).
        let bind = iw6_pose::pose(&ch, None, 0.0);
        for (i, (p, w)) in bind.iter().zip(&ch.world).enumerate() {
            let d: f32 = (0..3).map(|k| (p.1[k] - w.1[k]).powi(2)).sum::<f32>().sqrt();
            assert!(d < 0.05, "chain bone {i} composes to {:?}, baseMat says {:?}", p.1, w.1);
        }
    }

    fn rec_with_delta(size: u16, small: bool, numframes: u16, at: crate::walk::DeltaTransAt) -> XAnimRec {
        XAnimRec {
            numframes,
            delta: Some(crate::walk::DeltaRec { trans: Some((size, small, Span { pos: 0, len: 0 })), trans_at: Some(at), ..Default::default() }),
            ..Default::default()
        }
    }

    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn a_single_delta_frame_is_three_floats() {
        let img = floats(&[1.0, 2.0, 3.0]);
        let a = rec_with_delta(0, false, 10, crate::walk::DeltaTransAt { head: Some(0), ..Default::default() });
        let d = delta_trans(&a, &img).unwrap();
        assert_eq!((d.size, d.mins, d.indices.len(), d.packed.len()), (0, [1.0, 2.0, 3.0], 0, 0));
    }

    #[test]
    fn keyed_delta_frames_read_their_mins_steps_indices_and_values() {
        // head: mins, step, a pointer (8 bytes); then 3 u8 indices; then 3 small frames.
        let mut img = floats(&[0.0, 10.0, 20.0, 1.0, 2.0, 3.0]);
        img.extend([0u8; 8]);
        let idx = img.len();
        img.extend([0u8, 4, 9]);
        let fr = img.len();
        img.extend([1u8, 2, 3, 4, 5, 6, 7, 8, 9]);
        let at = crate::walk::DeltaTransAt { head: Some(0), indices: Some(idx), frames: Some(fr) };
        let d = delta_trans(&rec_with_delta(2, true, 10, at), &img).unwrap();
        assert_eq!((d.mins, d.step, d.indices.clone(), d.packed.len()), ([0.0, 10.0, 20.0], [1.0, 2.0, 3.0], vec![0, 4, 9], 9));
        assert_eq!(d.packed[0..3], [1, 2, 3]);
        // The same with 300 frames: u16 indices; big frames are six bytes each.
        let mut img = floats(&[0.0; 6]);
        img.extend([0u8; 8]);
        let idx = img.len();
        img.extend([0u8, 0, 200, 0, 44, 1]);
        let fr = img.len();
        img.extend([7u8; 18]);
        let at = crate::walk::DeltaTransAt { head: Some(0), indices: Some(idx), frames: Some(fr) };
        let d = delta_trans(&rec_with_delta(2, false, 300, at), &img).unwrap();
        assert_eq!((d.indices, d.packed.len()), (vec![0, 200, 300], 18));
        // A truncated image, or no delta, is none rather than a panic.
        assert!(delta_trans(&rec_with_delta(2, false, 300, at), &img[..idx + 3]).is_none());
        assert!(delta_trans(&XAnimRec::default(), &img).is_none());
    }

    #[test]
    fn root_motion_in_the_installed_zones_decodes_to_finite_values() {
        let Some(dir) = zone_dir() else {
            return;
        };
        let mut seen = 0;
        for z in ["mp_frag.ff", "common_core_mp.ff", "common_mp.ff"] {
            let Ok(zone) = crate::Zone::open(&dir.join(z)) else { continue };
            let (cap, _) = crate::walk::walk_zone(&zone);
            let (clips, _) = clips(&cap, &zone.image, &zone.assets.script_strings);
            for c in clips.iter().filter(|c| !c.delta.is_empty()) {
                seen += 1;
                assert_eq!(c.delta.len(), c.frames as usize + 1, "{}", c.name);
                assert!(c.delta.iter().all(|d| d.iter().all(|v| v.is_finite() && v.abs() < 1.0e6)), "{}", c.name);
            }
            eprintln!("{z}: {} animations, {} with root motion", clips.len(), clips.iter().filter(|c| !c.delta.is_empty()).count());
        }
        eprintln!("root motion clips seen: {seen}");
    }
}

//! bo2mp: the siege animations played on the static models
//! (`fastfile_t7::siege`): the fxanim vines, flags, sheets, banners, tarps
//! and cables loop as BlackOps3.exe plays them.
//!
//! The exe starts every `SiegeAutoPlay` record on the static model draws
//! whose id it names when the map starts, and advances them at 30 frames a
//! second each frame. The `mcs/` vertex programs skin from the instance's
//! word 4 bone: its row 3 `.w`, when not zero, is the first of the parts'
//! bone matrices (the CPU path the exe's `r_siege_uncompress` job writes),
//! else `.xy` are two frames of the `gpuSkin*` textures lerped by `.z` (the
//! GPU path; in the rest pose here). Here every playing model takes the CPU
//! path: a holder bone (row 3 = (0, 0, 0, first)) and one bone per part,
//! rewritten each frame from the pose: the compressed animation evaluated
//! as the exe's uncompress job does, or a retail map's streamed frames
//! (read from the packages) lerped as the GPU program lerps them.
//!
//! `CODLEGACY_T7_SIEGE=0`: none played (the rest pose, as before).

use fastfile_t7::siege::{Anim, Baked, Track, autoplay, read_all};

/// One playing static model instance.
#[derive(Clone, Debug)]
pub struct Playing {
    pub anim: usize,
    pub track: Track,
    /// The first of its parts' bone matrices.
    pub first_bone: u32,
    pub parts: u32,
}

/// The map's siege animations and the instances playing them.
#[derive(Clone, Debug, Default)]
pub struct Siege {
    pub anims: Vec<Anim>,
    pub playing: Vec<Playing>,
    clock: Option<f32>,
}

/// Whether siege animations are played (`CODLEGACY_T7_SIEGE=0`: not).
pub fn enabled() -> bool {
    !codlegacy_env::var("CODLEGACY_T7_SIEGE").is_ok_and(|v| v.trim() == "0")
}

fn bare(n: &str) -> &str {
    n.trim_start_matches(',')
}

/// A part pose as a bone matrix (rows; row 3 zero).
fn bone_of(m: [[f32; 4]; 3]) -> [f32; 16] {
    let mut b = [0.0f32; 16];
    for (r, row) in m.iter().enumerate() {
        b[4 * r..4 * r + 4].copy_from_slice(row);
    }
    b
}

impl Siege {
    /// Starts the auto-played siege animations: `image` and `cap` the map
    /// zone's, `instance_smodels` (instance, static model draw index) for
    /// every placed static model, `bones` / `instances` the scene's (a
    /// holder and the parts' bones are added per playing instance, and its
    /// word 4 set). `read` fetches a package entry (a retail map's streamed
    /// frames).
    pub fn build(
        image: &[u8],
        cap: &fastfile_t7::walk::capture::Capture,
        instance_smodels: &[(u32, u32)],
        bones: &mut Vec<[f32; 16]>,
        instances: &mut [[u32; 6]],
        read: &mut dyn FnMut(u64) -> Option<Vec<u8>>,
        report: &mut Vec<String>,
    ) -> Self {
        let mut s = Siege { anims: read_all(image, &cap.world.sanims), ..Default::default() };
        let Some(g) = cap.world.gfx_worlds.first() else { return s };
        let plays = autoplay(image, g.siege_autoplay);
        if plays.is_empty() || s.anims.is_empty() {
            if !s.anims.is_empty() {
                report.push(format!("t7 siege: {} siege animations, none auto-played", s.anims.len()));
            }
            return s;
        }
        if !enabled() {
            report.push(format!("t7 siege: {} auto-played siege animations left in the rest pose (CODLEGACY_T7_SIEGE=0)", plays.len()));
            return s;
        }
        // The world's siege list by name -> the animations read.
        let list: Vec<Option<usize>> = g
            .siege_anims
            .iter()
            .map(|n| n.as_deref().and_then(|n| s.anims.iter().position(|a| a.header.name == bare(n))))
            .collect();
        // Static model draw ids (+0x68) -> their placed instances.
        let mut by_id: std::collections::HashMap<u32, Vec<u32>> = Default::default();
        if let Some((at, _)) = g.smodels {
            for &(inst, j) in instance_smodels {
                let d = at + fastfile_t7::walk::gfxworld::SMODEL_DRAW * j as usize + 0x68;
                if let Some(b) = image.get(d..d + 4) {
                    by_id.entry(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).or_default().push(inst);
                }
            }
        }
        let (mut started, mut no_model, mut no_data, mut streamed) = (0usize, 0usize, 0usize, 0usize);
        let mut failed: Vec<String> = Vec::new();
        let mut tried = vec![false; s.anims.len()];
        for p in &plays {
            let Some(Some(ai)) = list.get(p.anim as usize).copied() else {
                no_data += 1;
                continue;
            };
            // A retail map's frames: from the packages, once.
            if !tried[ai] {
                tried[ai] = true;
                let a = &mut s.anims[ai];
                if a.compressed.is_none()
                    && let Some(key) = a.stream_key
                {
                    match read(key) {
                        Some(d) => match Baked::decode(&d, a.header.parts, usize::from(a.header.frames), a.header.bounds) {
                            Some(b) => {
                                streamed += 1;
                                if streamed == 1 {
                                    report.push(format!("t7 siege: streamed frames of {} read as {} ({} bytes)", a.header.name, b.layout, d.len()));
                                }
                                a.baked = Some(b);
                            }
                            None => failed.push(format!("{} ({} bytes for {} parts x {} frames: no layout fits)", a.header.name, d.len(), a.header.parts, a.header.frames)),
                        },
                        None => failed.push(format!("{} (package entry {key:#018x} not found)", a.header.name)),
                    }
                }
            }
            let a = &s.anims[ai];
            let parts = a.compressed.as_ref().map(|c| c.parts.len()).or(a.baked.as_ref().map(|b| b.parts)).unwrap_or(0);
            if parts == 0 {
                no_data += 1;
                continue;
            }
            let Some(insts) = by_id.get(&p.id) else {
                no_model += 1;
                continue;
            };
            for &inst in insts {
                let Some(w) = instances.get_mut(inst as usize) else { continue };
                let track = Track::start(&a.header, p.shot, p.start, p.id, 0.0);
                let holder = bones.len() as u32;
                let first = holder + 1;
                let mut h = [0.0f32; 16];
                h[15] = first as f32;
                bones.push(h);
                for i in 0..parts {
                    bones.push(a.part_pose(i, &track).map_or([0.0; 16], |pp| bone_of(pp.matrix())));
                }
                w[4] = holder;
                s.playing.push(Playing { anim: ai, track, first_bone: first, parts: parts as u32 });
                started += 1;
            }
        }
        report.push(format!(
            "t7 siege: {started} static models playing their siege animation ({} auto-play records; {no_model} name no placed model, {no_data} an animation without data here; {streamed} read from the packages{})",
            plays.len(),
            if failed.is_empty() { String::new() } else { format!("; not read: {}", failed.iter().take(6).cloned().collect::<Vec<_>>().join(", ")) }
        ));
        s
    }

    /// Advances every playing model to `now` (seconds) and hands each one's
    /// part bones (first bone index, matrices) to `write`.
    pub fn update(&mut self, now: f32, write: &mut dyn FnMut(u32, &[[f32; 16]])) -> usize {
        let dt = self.clock.map_or(0.0, |c| now - c);
        self.clock = Some(now);
        let mut buf = Vec::new();
        for p in &mut self.playing {
            p.track.advance(dt);
            let a = &self.anims[p.anim];
            buf.clear();
            buf.extend((0..p.parts as usize).map(|i| a.part_pose(i, &p.track).map_or([0.0; 16], |pp| bone_of(pp.matrix()))));
            write(p.first_bone, &buf);
        }
        self.playing.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastfile_t7::siege::{Compressed, Header, PartKeys};

    #[test]
    fn holder_points_the_program_at_the_parts() {
        // One animation, one part swinging 90 degrees about z over 10 frames.
        let id = [0.0, 0.0, 0.0, 1.0];
        let rz = [0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2];
        let c = Compressed {
            parts: vec![PartKeys { rot_first: 0, trans_first: 0, rot_count: 2, trans_count: 2 }],
            rot_times: vec![0, 10],
            trans_times: vec![0, 10],
            rot_keys: vec![[id, id, rz], [rz; 3]],
            trans_keys: vec![[[0.0; 3]; 3], [[0.0; 3]; 3]],
            pivots: vec![[0.0; 4]],
        };
        let header = Header { name: "a".into(), parts: 1, frames: 11, rate: 1.0, looping: true, shots: Vec::new(), bounds: None };
        let track = Track::start(&header, 0, 0.0, 1, 0.0);
        let mut s = Siege { anims: vec![Anim { header, compressed: Some(c), baked: None, stream_key: None }], playing: vec![Playing { anim: 0, track, first_bone: 5, parts: 1 }], clock: None };
        let mut got = Vec::new();
        s.update(0.0, &mut |b, m| got.push((b, m.to_vec())));
        // Frame 0: identity.
        assert_eq!(got[0].0, 5);
        assert!((got[0].1[0][0] - 1.0).abs() < 1e-5 && got[0].1[0][4].abs() < 1e-5);
        // 1/3 s later: frame 10, 90 degrees: x -> y (row 1 col 0 = 1).
        got.clear();
        s.update(1.0 / 3.0, &mut |b, m| got.push((b, m.to_vec())));
        let m = &got[0].1[0];
        assert!(m[0].abs() < 1e-3 && (m[4] - 1.0).abs() < 1e-3, "{m:?}");
    }
}

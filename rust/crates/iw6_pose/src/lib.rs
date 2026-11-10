//! bo2mp: Ghosts (IW6) script-model animation, as plain data.
//!
//! A model has a [`Skeleton`] (bone names, parents, the bind pose local to the parent and in model
//! space). An animation is a [`Clip`] sampled at every frame (the zone's key frames are read and
//! decoded by `fastfile_iw6::anim`; the sim only needs the values). A bone the clip has no track
//! for keeps its bind pose; a track overrides the bone's pose local to its parent (READ: the
//! animation's rotations and translations are parent local, as the model's own tag table is).
//!
//! Quaternions are `[x, y, z, w]`, rotating a vector the usual way (`q v q*`). The engine's
//! world axes of an entity are (forward, left, up) from its pitch / yaw / roll angles; see
//! [`axes_of_angles`] and [`angles_of_axes`].

/// A rotation, `[x, y, z, w]`.
pub type Quat = [f32; 4];
/// A bone's pose: rotation, translation.
pub type Bone = (Quat, [f32; 3]);

pub const IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// A model's bones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    /// Lower case.
    pub name: String,
    pub bones: Vec<String>,
    /// The parent bone (always an earlier one); none for a root bone.
    pub parents: Vec<Option<u16>>,
    /// Bind pose local to the parent.
    pub local: Vec<Bone>,
    /// Bind pose in model space.
    pub world: Vec<Bone>,
}

/// A sampled animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    /// Lower case.
    pub name: String,
    /// The animation's frame count.
    pub frames: u32,
    /// Frames per second.
    pub rate: f32,
    pub looping: bool,
    /// The bones it animates (lower case), one track each.
    pub bones: Vec<String>,
    /// (notetrack name, fraction of the length): `end` is at 1.
    pub notes: Vec<(String, f32)>,
    /// Per track: the rotation at frames `0..=frames` (empty: the bind pose).
    pub rot: Vec<Vec<Quat>>,
    /// Per track: the translation at frames `0..=frames` (empty: the bind pose).
    pub trans: Vec<Vec<[f32; 3]>>,
    /// The root motion: the absolute delta translation at frames `0..=frames` (empty: none).
    pub delta: Vec<[f32; 3]>,
}

impl Clip {
    /// The length the engine gives it: frames over rate, in ms (READ from the handler).
    pub fn length_ms(&self) -> i64 {
        if self.rate > 0.0 { ((self.frames as f32 / self.rate) * 1000.0) as i64 } else { 0 }
    }

    /// The animation's time for `elapsed` ms since it started: looping wraps, else it holds the end.
    pub fn time_secs(&self, elapsed_ms: i64) -> f32 {
        let len = self.length_ms().max(1);
        let ms = if self.looping { elapsed_ms.rem_euclid(len) } else { elapsed_ms.clamp(0, len) };
        ms as f32 / 1000.0
    }

    /// Whether a non-looping clip has run its length at `elapsed_ms`.
    pub fn finished(&self, elapsed_ms: i64) -> bool {
        !self.looping && elapsed_ms >= self.length_ms()
    }

    /// Track `track` at `secs` into the clip: the key frames either side, blended.
    pub fn sample(&self, track: usize, secs: f32) -> (Option<Quat>, Option<[f32; 3]>) {
        let f = (secs * self.rate).clamp(0.0, self.frames as f32);
        let (i, k) = (f.floor() as usize, f - f.floor());
        let q = self.rot.get(track).filter(|v| !v.is_empty()).map(|v| {
            let a = v[i.min(v.len() - 1)];
            let b = v[(i + 1).min(v.len() - 1)];
            nlerp(a, b, k)
        });
        let t = self.trans.get(track).filter(|v| !v.is_empty()).map(|v| {
            let a = v[i.min(v.len() - 1)];
            let b = v[(i + 1).min(v.len() - 1)];
            [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
        });
        (q, t)
    }

    /// The root motion at `secs` into the clip, relative to its start (None: no delta part). The
    /// frames either side are blended; a clip with a single frame holds it.
    pub fn delta_at(&self, secs: f32) -> Option<[f32; 3]> {
        let first = *self.delta.first()?;
        let f = (secs * self.rate).clamp(0.0, self.frames as f32);
        let (i, k) = (f.floor() as usize, f - f.floor());
        let a = self.delta[i.min(self.delta.len() - 1)];
        let b = self.delta[(i + 1).min(self.delta.len() - 1)];
        Some(std::array::from_fn(|c| a[c] + (b[c] - a[c]) * k - first[c]))
    }

    /// The root motion `elapsed_ms` after the clip started, in the clip's own frame: a looping clip
    /// adds its whole displacement for each cycle it has finished (the engine walks the delta per
    /// frame; INFERRED that a loop continues from where it ended), a plain one holds its last.
    pub fn root_offset(&self, elapsed_ms: i64) -> Option<[f32; 3]> {
        let here = self.delta_at(self.time_secs(elapsed_ms))?;
        if !self.looping {
            return Some(here);
        }
        let len = self.length_ms().max(1);
        let cycles = (elapsed_ms.max(0) / len) as f32;
        let (first, last) = (self.delta[0], *self.delta.last()?);
        Some(std::array::from_fn(|c| here[c] + cycles * (last[c] - first[c])))
    }

    /// The notetracks passed going from `from_secs` to `to_secs` of the clip (not `end`; that one
    /// is the clip's finish), by name.
    pub fn notes_between(&self, from_secs: f32, to_secs: f32) -> Vec<&str> {
        let len = self.frames as f32 / self.rate.max(f32::EPSILON);
        if len <= 0.0 {
            return Vec::new();
        }
        let (a, b) = (from_secs / len, to_secs / len);
        self.notes.iter().filter(|(n, t)| !n.is_empty() && !n.eq_ignore_ascii_case("end") && *t > a && *t <= b).map(|(n, _)| n.as_str()).collect()
    }
}

pub fn nlerp(a: Quat, mut b: Quat, k: f32) -> Quat {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if d < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
    }
    normalize(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k))
}

pub fn normalize(q: Quat) -> Quat {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1e-8 { q.map(|c| c / l) } else { IDENTITY }
}

pub fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn conj(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

pub fn rotate(q: Quat, v: [f32; 3]) -> [f32; 3] {
    let p = mul(mul(q, [v[0], v[1], v[2], 0.0]), conj(q));
    [p[0], p[1], p[2]]
}

/// `parent` then `local`.
pub fn compose(parent: Bone, local: Bone) -> Bone {
    let t = rotate(parent.0, local.1);
    (normalize(mul(parent.0, local.0)), [parent.1[0] + t[0], parent.1[1] + t[1], parent.1[2] + t[2]])
}

/// The inverse of a pose.
pub fn invert(b: Bone) -> Bone {
    let q = conj(b.0);
    let t = rotate(q, b.1);
    (q, [-t[0], -t[1], -t[2]])
}

/// Rotation matrix columns (the images of x, y, z).
pub fn axes(q: Quat) -> [[f32; 3]; 3] {
    [rotate(q, [1.0, 0.0, 0.0]), rotate(q, [0.0, 1.0, 0.0]), rotate(q, [0.0, 0.0, 1.0])]
}

/// The quaternion of orthonormal columns.
pub fn quat_of_axes(m: [[f32; 3]; 3]) -> Quat {
    let (x, y, z) = (m[0], m[1], m[2]);
    // m[col][row]; trace = m00 + m11 + m22.
    let tr = x[0] + y[1] + z[2];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(y[2] - z[1]) / s, (z[0] - x[2]) / s, (x[1] - y[0]) / s, 0.25 * s]
    } else if x[0] > y[1] && x[0] > z[2] {
        let s = (1.0 + x[0] - y[1] - z[2]).sqrt() * 2.0;
        [0.25 * s, (y[0] + x[1]) / s, (z[0] + x[2]) / s, (y[2] - z[1]) / s]
    } else if y[1] > z[2] {
        let s = (1.0 + y[1] - x[0] - z[2]).sqrt() * 2.0;
        [(y[0] + x[1]) / s, 0.25 * s, (z[1] + y[2]) / s, (z[0] - x[2]) / s]
    } else {
        let s = (1.0 + z[2] - x[0] - y[1]).sqrt() * 2.0;
        [(z[0] + x[2]) / s, (z[1] + y[2]) / s, 0.25 * s, (x[1] - y[0]) / s]
    };
    normalize(q)
}

/// The engine's axes of pitch / yaw / roll degrees: forward, left, up.
pub fn axes_of_angles(a: [f32; 3]) -> [[f32; 3]; 3] {
    let (sp, cp) = a[0].to_radians().sin_cos();
    let (sy, cy) = a[1].to_radians().sin_cos();
    let (sr, cr) = a[2].to_radians().sin_cos();
    let f = [cp * cy, cp * sy, -sp];
    let r = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let u = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    [f, [-r[0], -r[1], -r[2]], u]
}

/// Pitch / yaw / roll degrees of (forward, left, up) axes (the inverse of [`axes_of_angles`]).
pub fn angles_of_axes(m: [[f32; 3]; 3]) -> [f32; 3] {
    let (f, l, u) = (m[0], m[1], m[2]);
    let horiz = (f[0] * f[0] + f[1] * f[1]).sqrt();
    if horiz > 1e-4 {
        let yaw = f[1].atan2(f[0]).to_degrees();
        let pitch = (-f[2]).atan2(horiz).to_degrees();
        let roll = l[2].atan2(u[2]).to_degrees();
        // With left = -right: right.z = -sr*cp, up.z = cr*cp -> roll = atan2(-right.z, up.z) = atan2(left.z, up.z).
        [pitch, yaw, roll]
    } else {
        let pitch = if f[2] < 0.0 { 90.0 } else { -90.0 };
        let yaw = (-l[0]).atan2(l[1]).to_degrees();
        [pitch, yaw, 0.0]
    }
}

/// The model-space pose of every bone of `skel` with `clip` at `secs` (None: the bind pose).
pub fn pose(skel: &Skeleton, clip: Option<&Clip>, secs: f32) -> Vec<Bone> {
    compose_world(skel, &local_pose(skel, clip, secs))
}

/// One clip in a blend: the clip (None: the bind pose), its time and its weight.
pub type Layer<'a> = (Option<&'a Clip>, f32, f32);

/// The model-space pose of a blend of clips: each bone's parent-local pose is the weighted blend
/// of the layers' (rotations by normalised lerp, translations linear); the weights are
/// normalised, a layer of weight 0 or less is left out, and no layer left is the bind pose.
pub fn pose_blend(skel: &Skeleton, layers: &[Layer]) -> Vec<Bone> {
    let live: Vec<&Layer> = layers.iter().filter(|l| l.2 > 0.0).collect();
    let total: f32 = live.iter().map(|l| l.2).sum();
    if live.is_empty() || total <= 0.0 {
        return pose(skel, None, 0.0);
    }
    let poses: Vec<(Vec<Bone>, f32)> = live.iter().map(|l| (local_pose(skel, l.0, l.1), l.2 / total)).collect();
    let mut acc = poses[0].0.clone();
    let mut w = poses[0].1;
    for (p, pw) in &poses[1..] {
        // Running blend: fold each layer in by its share of the weight so far.
        let k = pw / (w + pw);
        for (a, b) in acc.iter_mut().zip(p) {
            a.0 = nlerp(a.0, b.0, k);
            for c in 0..3 {
                a.1[c] += (b.1[c] - a.1[c]) * k;
            }
        }
        w += pw;
    }
    compose_world(skel, &acc)
}

/// The parent-local pose of every bone: the clip's tracks over the bind pose.
pub fn local_pose(skel: &Skeleton, clip: Option<&Clip>, secs: f32) -> Vec<Bone> {
    let mut local = skel.local.clone();
    if let Some(c) = clip {
        for (t, name) in c.bones.iter().enumerate() {
            let Some(b) = skel.bones.iter().position(|n| n == name) else { continue };
            let (q, tr) = c.sample(t, secs);
            if let Some(q) = q {
                local[b].0 = q;
            }
            if let Some(tr) = tr {
                local[b].1 = tr;
            }
        }
    }
    local
}

fn compose_world(skel: &Skeleton, local: &[Bone]) -> Vec<Bone> {
    let mut world: Vec<Bone> = Vec::with_capacity(local.len());
    for (i, l) in local.iter().enumerate() {
        let w = match skel.parents.get(i).copied().flatten() {
            Some(p) if (p as usize) < i => compose(world[p as usize], *l),
            _ => *l,
        };
        world.push(w);
    }
    world
}

/// The skinning pose of a bone: model-space bind to the posed model space (`pose * bind^-1`).
pub fn skin(skel: &Skeleton, posed: &[Bone]) -> Vec<Bone> {
    posed.iter().zip(&skel.world).map(|(p, b)| compose(*p, invert(*b))).collect()
}

/// A model-space bone pose placed in the world by an entity at `origin` with `angles`: the bone's
/// world origin and angles.
pub fn place(origin: [f32; 3], angles: [f32; 3], b: Bone) -> ([f32; 3], [f32; 3]) {
    let q = quat_of_axes(axes_of_angles(angles));
    let w = compose((q, origin), b);
    (w.1, angles_of_axes(axes(w.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    #[test]
    fn angles_and_axes_round_trip() {
        for a in [[0.0, 0.0, 0.0], [10.0, 90.0, 0.0], [-30.0, 200.0, 45.0], [0.0, -90.0, 0.0], [5.0, 359.0, -20.0]] {
            let back = angles_of_axes(axes_of_angles(a));
            let again = axes_of_angles(back);
            let want = axes_of_angles(a);
            for k in 0..3 {
                assert!(close(again[k], want[k]), "{a:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn a_yaw_quaternion_turns_forward_into_left() {
        let h = (0.5f32).sqrt();
        let q = [0.0, 0.0, h, h];
        assert!(close(rotate(q, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        // The engine's axes of yaw 90: forward = +y.
        assert!(close(axes_of_angles([0.0, 90.0, 0.0])[0], [0.0, 1.0, 0.0]));
        assert!(close(axes(quat_of_axes(axes_of_angles([20.0, 70.0, 10.0])))[0], axes_of_angles([20.0, 70.0, 10.0])[0]));
    }

    #[test]
    fn a_chain_composes_and_the_skin_of_the_bind_pose_is_the_identity() {
        let h = (0.5f32).sqrt();
        let local = vec![(IDENTITY, [0.0; 3]), ([0.0, 0.0, h, h], [10.0, 0.0, 0.0]), (IDENTITY, [5.0, 0.0, 0.0])];
        let mut world = Vec::new();
        for (i, l) in local.iter().enumerate() {
            world.push(if i == 0 { *l } else { compose(world[i - 1], *l) });
        }
        // Bone 2 is 5 along bone 1's x, which points along +y: (10, 5, 0).
        assert!(close(world[2].1, [10.0, 5.0, 0.0]));
        let skel = Skeleton { name: "t".into(), bones: vec!["a".into(), "b".into(), "c".into()], parents: vec![None, Some(0), Some(1)], local, world };
        let posed = pose(&skel, None, 0.0);
        for (p, b) in posed.iter().zip(&skel.world) {
            assert!(close(p.1, b.1));
        }
        for s in skin(&skel, &posed) {
            assert!(close(s.1, [0.0; 3]));
            assert!(close(axes(s.0)[0], [1.0, 0.0, 0.0]));
        }
    }

    #[test]
    fn a_clip_overrides_the_bones_it_has_tracks_for() {
        let skel = Skeleton {
            name: "t".into(),
            bones: vec!["root".into(), "j_a".into()],
            parents: vec![None, Some(0)],
            local: vec![(IDENTITY, [0.0; 3]), (IDENTITY, [1.0, 0.0, 0.0])],
            world: vec![(IDENTITY, [0.0; 3]), (IDENTITY, [1.0, 0.0, 0.0])],
        };
        let clip = Clip {
            name: "c".into(),
            frames: 2,
            rate: 2.0,
            looping: false,
            bones: vec!["j_a".into()],
            notes: vec![("end".into(), 1.0), ("tick".into(), 0.5)],
            rot: vec![vec![]],
            trans: vec![vec![[1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [1.0, 4.0, 0.0]]],
            delta: Vec::new(),
        };
        assert_eq!(clip.length_ms(), 1000);
        assert!(close(pose(&skel, Some(&clip), 0.0)[1].1, [1.0, 0.0, 0.0]));
        assert!(close(pose(&skel, Some(&clip), clip.time_secs(500))[1].1, [1.0, 2.0, 0.0]));
        assert!(close(pose(&skel, Some(&clip), clip.time_secs(5000))[1].1, [1.0, 4.0, 0.0]));
        assert!(clip.finished(1000) && !clip.finished(999));
        assert_eq!(clip.notes_between(0.0, 0.6), vec!["tick"]);
        let mut looped = clip.clone();
        looped.looping = true;
        assert!(close(pose(&skel, Some(&looped), looped.time_secs(1500))[1].1, [1.0, 2.0, 0.0]));
    }

    #[test]
    fn root_motion_walks_the_delta_and_loops_add_up() {
        let mut c = Clip { name: "walk".into(), frames: 2, rate: 2.0, delta: vec![[1.0, 0.0, 0.0], [1.0, 3.0, 0.0], [1.0, 6.0, 0.0]], ..Default::default() };
        assert!(close(c.delta_at(0.0).unwrap(), [0.0; 3]));
        assert!(close(c.delta_at(0.5).unwrap(), [0.0, 3.0, 0.0]));
        assert!(close(c.root_offset(5000).unwrap(), [0.0, 6.0, 0.0]), "a plain clip holds its end");
        c.looping = true;
        // 1500 ms: one cycle (6 along y) and half of the next (3).
        assert!(close(c.root_offset(1500).unwrap(), [0.0, 9.0, 0.0]));
        assert!(Clip::default().root_offset(10).is_none());
    }

    #[test]
    fn blended_clips_meet_in_the_middle_and_a_lone_layer_is_itself() {
        let skel = Skeleton {
            name: "t".into(),
            bones: vec!["root".into(), "j_a".into()],
            parents: vec![None, Some(0)],
            local: vec![(IDENTITY, [0.0; 3]), (IDENTITY, [1.0, 0.0, 0.0])],
            world: vec![(IDENTITY, [0.0; 3]), (IDENTITY, [1.0, 0.0, 0.0])],
        };
        let mk = |y: f32| Clip { name: "c".into(), frames: 1, rate: 1.0, bones: vec!["j_a".into()], rot: vec![vec![]], trans: vec![vec![[1.0, y, 0.0], [1.0, y, 0.0]]], ..Default::default() };
        let (a, b) = (mk(0.0), mk(10.0));
        assert!(close(pose_blend(&skel, &[(Some(&a), 0.0, 1.0)])[1].1, [1.0, 0.0, 0.0]));
        assert!(close(pose_blend(&skel, &[(Some(&a), 0.0, 0.5), (Some(&b), 0.0, 0.5)])[1].1, [1.0, 5.0, 0.0]));
        assert!(close(pose_blend(&skel, &[(Some(&a), 0.0, 3.0), (Some(&b), 0.0, 1.0)])[1].1, [1.0, 2.5, 0.0]));
        // A zero weight is out; nothing left is the bind pose.
        assert!(close(pose_blend(&skel, &[(Some(&b), 0.0, 0.0)])[1].1, [1.0, 0.0, 0.0]));
        // Rotations blend as the short way round.
        let h = (0.5f32).sqrt();
        let (qa, qb) = (IDENTITY, [0.0, 0.0, h, h]);
        let mid = nlerp(qa, qb, 0.5);
        let ang = 2.0 * mid[2].atan2(mid[3]).to_degrees();
        assert!((ang - 45.0).abs() < 0.1);
    }
}

//! bo2mp: Black Ops III's siege animations (`SAnim`, the exe's
//! `GfxSiegeAnim`): the baked animations of the GPU-skinned fxanim static
//! models (vines, flags, sheets, banners, cables, fans), and their playback
//! as BlackOps3.exe runs it.
//!
//! What the exe does (read from the PC build):
//!
//! * **Map start** (`141d7d610`): every null slot of the world's siege list
//!   (`GfxWorld` +0x580, count +0x578) gets `default_sanim`; then for each
//!   `SiegeAutoPlay` record (+0x590, count +0x588; 20 bytes: u32 static
//!   model id, u32 index into the siege list, f32 start, u32 shot id, u32)
//!   every static model draw (+0x438, 0x98 each, count +0x2fc) whose id
//!   (+0x68) matches starts that animation (`141deaad0`) at the record's
//!   start, or, when it is -1, at `(float)(u32)(id * 0x302bf) + siegeTime`
//!   (a phase per model, so neighbouring flags do not wave in step).
//! * **Play** (`141deaad0`): the instance's track takes the shot whose id
//!   matches (the animation's shot list at +0x18, 12 bytes: u16 first
//!   frame, u16 end frame, u32, u32 id at +8; count u16 at +0x66), else the
//!   first, else frames `0..frames` (+0x58); the start frame is `first +
//!   fmod(start, end - first)`; the track loops when the animation's flags
//!   (+0x64) have bit 0.
//! * **Animate** (job `r_siege_animate` -> `141dea460`, each client frame
//!   with the time in seconds): `dt` clamped to 0..1 s, times 30 frames a
//!   second, times the instance's rate (1) and the animation's (+0x5c);
//!   the frame advances by that, wrapping to the first frame (looping) or
//!   holding the last; the next frame is the following one (wrapping the
//!   same way) with the same fraction.
//! * **Uncompress** (job `r_siege_uncompress` -> `141deaec0`): a frame's
//!   pose from the compressed animation (`GfxSiegeAnimCompressed`, 0x40 at
//!   +0x10: per part a 12-byte record (u32 first rotation key, u32 first
//!   translation key, u16 rotation keys, u16 translation keys), the
//!   rotation and translation key frame numbers (u16 each), rotation keys
//!   (48 bytes: three quaternions), translation keys (36: three vec3s) and
//!   per part a pivot (vec4); part count +0x54): per part the segment whose
//!   key frames hold the frame, a cubic Bezier over the key's three points
//!   and the next key's first (`(1-s)^3 P0 + 3(1-s)^2 s P1 + 3(1-s) s^2 P2
//!   + s^3 P3`), the quaternion normalised, and the part's 3x4 transform
//!   `x' = R (x - pivot) + pivot + T`. The GPU skin program does the same
//!   per vertex from the `gpuSkinBase` (pivot), `gpuSkinQuat` and
//!   `gpuSkinPos` textures.
//!
//! The mod tools' maps keep the compressed animation in the zone; a retail
//! map streams each animation's baked frames from its packages (see
//! [`Baked`]).

/// Frames a second the exe advances siege animations by.
pub const FRAMES_PER_SECOND: f32 = 30.0;

fn u16_at(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}

/// One part's key ranges (`GfxSiegeAnimCompressed` part record).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartKeys {
    pub rot_first: u32,
    pub trans_first: u32,
    pub rot_count: u16,
    pub trans_count: u16,
}

/// The compressed animation (`GfxSiegeAnimCompressed`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compressed {
    pub parts: Vec<PartKeys>,
    pub rot_times: Vec<u16>,
    pub trans_times: Vec<u16>,
    /// Three quaternions (x, y, z, w) a key.
    pub rot_keys: Vec<[[f32; 4]; 3]>,
    /// Three points a key.
    pub trans_keys: Vec<[[f32; 3]; 3]>,
    pub pivots: Vec<[f32; 4]>,
}

/// One part's pose: `x' = R(quat) (x - pivot) + pivot + pos`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartPose {
    pub quat: [f32; 4],
    pub pos: [f32; 3],
    pub pivot: [f32; 3],
}

impl PartPose {
    /// The 3x4 transform (rows; column 3 the translation), as the exe's
    /// uncompress job writes it.
    pub fn matrix(&self) -> [[f32; 4]; 3] {
        let [x, y, z, w] = self.quat;
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        let p = self.pivot;
        std::array::from_fn(|i| {
            let rp = r[i][0] * p[0] + r[i][1] * p[1] + r[i][2] * p[2];
            [r[i][0], r[i][1], r[i][2], self.pos[i] + p[i] - rp]
        })
    }

    /// A point through the pose.
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let m = self.matrix();
        std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + m[i][3])
    }
}

/// The segment of `times` that holds `frame` (`t[j] <= frame < t[j+1]`,
/// the first or last segment outside them), as the exe's binary search
/// finds it on the frame's integer part.
fn segment(times: &[u16], frame: f32) -> usize {
    if times.len() < 2 {
        return 0;
    }
    let f = frame as i32 as u16;
    let j = times.partition_point(|&t| t <= f);
    j.saturating_sub(1).min(times.len() - 2)
}

fn bezier<const N: usize>(p: [[f32; N]; 4], s: f32) -> [f32; N] {
    let u = 1.0 - s;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * s, 3.0 * u * s * s, s * s * s);
    std::array::from_fn(|k| a * p[0][k] + b * p[1][k] + c * p[2][k] + d * p[3][k])
}

impl Compressed {
    /// Reads the six arrays at their file offsets (`walk::world_misc::SAnimRec::data`).
    pub fn read(image: &[u8], data: &[Option<(usize, usize)>; 6]) -> Option<Self> {
        let (parts_at, parts_n) = data[0]?;
        let (rt_at, rt_n) = data[1]?;
        let (tt_at, tt_n) = data[2]?;
        let (rk_at, rk_n) = data[3]?;
        let (tk_at, tk_n) = data[4]?;
        let (pv_at, pv_n) = data[5]?;
        let parts = (0..parts_n)
            .map(|i| {
                let r = parts_at + 12 * i;
                PartKeys { rot_first: u32_at(image, r), trans_first: u32_at(image, r + 4), rot_count: u16_at(image, r + 8), trans_count: u16_at(image, r + 10) }
            })
            .collect();
        let rot_times = (0..rt_n).map(|i| u16_at(image, rt_at + 2 * i)).collect();
        let trans_times = (0..tt_n).map(|i| u16_at(image, tt_at + 2 * i)).collect();
        let rot_keys = (0..rk_n).map(|i| std::array::from_fn(|q| std::array::from_fn(|k| f32_at(image, rk_at + 48 * i + 16 * q + 4 * k)))).collect();
        let trans_keys = (0..tk_n).map(|i| std::array::from_fn(|q| std::array::from_fn(|k| f32_at(image, tk_at + 36 * i + 12 * q + 4 * k)))).collect();
        let pivots = (0..pv_n).map(|i| std::array::from_fn(|k| f32_at(image, pv_at + 16 * i + 4 * k))).collect();
        let c = Compressed { parts, rot_times, trans_times, rot_keys, trans_keys, pivots };
        c.valid().then_some(c)
    }

    /// Every part's keys and times are in range.
    pub fn valid(&self) -> bool {
        self.pivots.len() == self.parts.len()
            && self.rot_times.len() == self.rot_keys.len()
            && self.trans_times.len() == self.trans_keys.len()
            && self.parts.iter().all(|p| {
                let (r0, rn, t0, tn) = (p.rot_first as usize, p.rot_count as usize, p.trans_first as usize, p.trans_count as usize);
                rn >= 1 && tn >= 1 && r0 + rn <= self.rot_keys.len() && t0 + tn <= self.trans_keys.len()
            })
    }

    /// The pose of part `i` at `frame` (`141deaec0`).
    pub fn part_pose(&self, i: usize, frame: f32) -> PartPose {
        let p = self.parts[i];
        let pivot = self.pivots[i];
        let pivot = [pivot[0], pivot[1], pivot[2]];
        let (r0, rn) = (p.rot_first as usize, p.rot_count as usize);
        let (t0, tn) = (p.trans_first as usize, p.trans_count as usize);
        let local = |times: &[u16], j: usize| -> f32 {
            match (times.get(j), times.get(j + 1)) {
                (Some(&a), Some(&b)) if b > a => (frame - f32::from(a)) / f32::from(b - a),
                _ => 0.0,
            }
        };
        // Rotation.
        let rt = &self.rot_times[r0..r0 + rn];
        let j = segment(rt, frame);
        let s = local(rt, j);
        let k = self.rot_keys[r0 + j];
        let next = self.rot_keys.get(r0 + j + 1).filter(|_| j + 1 < rn).map_or(k[0], |n| n[0]);
        let q = bezier([k[0], k[1], k[2], next], s);
        let n = (q.iter().map(|c| c * c).sum::<f32>()).sqrt();
        let quat = if n > 0.0 { q.map(|c| c / n) } else { [0.0, 0.0, 0.0, 1.0] };
        // Translation.
        let tt = &self.trans_times[t0..t0 + tn];
        let j = segment(tt, frame);
        let s = local(tt, j);
        let k = self.trans_keys[t0 + j];
        let next = self.trans_keys.get(t0 + j + 1).filter(|_| j + 1 < tn).map_or(k[0], |n| n[0]);
        let pos = bezier([k[0], k[1], k[2], next], s);
        PartPose { quat, pos, pivot }
    }

    /// Every part's pose at `frame`.
    pub fn pose(&self, frame: f32) -> Vec<PartPose> {
        (0..self.parts.len()).map(|i| self.part_pose(i, frame)).collect()
    }
}

/// A retail map's baked frames, streamed from its packages: per part per
/// frame a quaternion and an offset (8 bytes each: four 16-bit values),
/// and per part a pivot (four f32s), each section 256-aligned (the sizes
/// of every streamed siege animation read fit `align256(parts * frames *
/// 8) * 2 + align256(parts * 16)` exactly). Which section comes first and
/// the 16-bit encoding are found from the data ([`Baked::decode`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Baked {
    pub parts: usize,
    pub frames: usize,
    /// `quat[frame * parts + part]`.
    pub quat: Vec<[f32; 4]>,
    pub pos: Vec<[f32; 3]>,
    pub pivots: Vec<[f32; 3]>,
    /// How the data was read (for the log).
    pub layout: &'static str,
}

fn align256(n: usize) -> usize {
    n.div_ceil(256) * 256
}

fn f16(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 0x1f);
    let m = f32::from(h & 0x3ff);
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

fn snorm16(h: u16) -> f32 {
    (f32::from(h as i16) / 32767.0).max(-1.0)
}

impl Baked {
    /// The size a streamed buffer of `parts` x `frames` has.
    pub fn expected_size(parts: usize, frames: usize) -> usize {
        align256(parts * frames * 8) * 2 + align256(parts * 16)
    }

    /// Reads a streamed buffer: tries the section orders (pivots first or
    /// last) and the quaternion encodings (snorm16, half), keeping the one
    /// whose quaternions are unit length (within 2%) on every texel; none
    /// fitting: `None`.
    /// `bounds`: the animation's (header +0x28 mins, +0x34 maxs): every
    /// pivot and offset must be within twice their extent.
    pub fn decode(data: &[u8], parts: usize, frames: usize, bounds: Option<([f32; 3], [f32; 3])>) -> Option<Self> {
        let n = parts * frames;
        if parts == 0 || frames == 0 || data.len() < Self::expected_size(parts, frames) {
            return None;
        }
        let sec = align256(n * 8);
        let piv = align256(parts * 16);
        let layouts: [(&'static str, usize, usize, usize, bool); 6] = [
            ("quat snorm16, pos half, pivots last", 0, sec, 2 * sec, true),
            ("quat half, pos half, pivots last", 0, sec, 2 * sec, false),
            ("pivots first, quat snorm16, pos half", piv, piv + sec, 0, true),
            ("pivots first, quat half, pos half", piv, piv + sec, 0, false),
            ("pos half, quat snorm16, pivots last", sec, 0, 2 * sec, true),
            ("pos half, quat half, pivots last", sec, 0, 2 * sec, false),
        ];
        for (name, qa, pa, va, snorm) in layouts {
            let quat: Vec<[f32; 4]> = (0..n).map(|i| std::array::from_fn(|k| {
                let h = u16_at(data, qa + 8 * i + 2 * k);
                if snorm { snorm16(h) } else { f16(h) }
            })).collect();
            let unit = quat.iter().all(|q| {
                let l = q.iter().map(|c| c * c).sum::<f32>().sqrt();
                (l - 1.0).abs() < 0.02
            });
            if !unit {
                continue;
            }
            let pos: Vec<[f32; 3]> = (0..n).map(|i| std::array::from_fn(|k| f16(u16_at(data, pa + 8 * i + 2 * k)))).collect();
            let pivots: Vec<[f32; 3]> = (0..parts).map(|i| std::array::from_fn(|k| f32_at(data, va + 16 * i + 4 * k))).collect();
            let reach = bounds.map_or(1.0e6, |(lo, hi)| 2.0 * (0..3).map(|k| lo[k].abs().max(hi[k].abs())).fold(1.0f32, f32::max));
            if pos.iter().chain(&pivots).flatten().all(|v| v.is_finite() && v.abs() <= reach) {
                return Some(Baked { parts, frames, quat, pos, pivots, layout: name });
            }
        }
        None
    }

    /// Part `i` at the integer frame `f` (clamped).
    pub fn part_pose(&self, i: usize, f: usize) -> PartPose {
        let f = f.min(self.frames.saturating_sub(1));
        let k = f * self.parts + i;
        PartPose { quat: self.quat[k], pos: self.pos[k], pivot: self.pivots[i] }
    }
}

/// A shot (`GfxSiegeAnimShot`): frames `first..end`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shot {
    pub first: u16,
    pub end: u16,
    pub id: u32,
}

/// What a siege animation's header says (`GfxSiegeAnim`, 0x70).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    pub name: String,
    pub parts: usize,
    pub frames: u16,
    pub rate: f32,
    pub looping: bool,
    pub shots: Vec<Shot>,
    /// The animation's bounds (+0x28 mins, +0x34 maxs).
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

impl Header {
    pub fn read(image: &[u8], rec: &crate::walk::world_misc::SAnimRec) -> Self {
        let h = rec.header_at;
        let frames = u16_at(image, h + 0x58);
        let n = usize::from(u16_at(image, h + 0x66));
        // (Shots that do not fit the animation are left out: the retail
        // zones' list position is not certain.)
        let shots = rec
            .shots
            .map(|at| {
                (0..n.min(64))
                    .map(|i| Shot { first: u16_at(image, at + 12 * i), end: u16_at(image, at + 12 * i + 2), id: u32_at(image, at + 12 * i + 8) })
                    .filter(|s| s.first < s.end && s.end <= frames)
                    .collect()
            })
            .unwrap_or_default();
        let rate = f32_at(image, h + 0x5c);
        Header {
            name: rec.name.clone(),
            parts: u32_at(image, h + 0x54) as usize,
            frames,
            rate: if rate.is_finite() && rate > 0.0 { rate } else { 1.0 },
            looping: u16_at(image, h + 0x64) & 1 != 0,
            shots,
            bounds: {
                let lo: [f32; 3] = std::array::from_fn(|k| f32_at(image, h + 0x28 + 4 * k));
                let hi: [f32; 3] = std::array::from_fn(|k| f32_at(image, h + 0x34 + 4 * k));
                lo.iter().chain(&hi).all(|v| v.is_finite()).then_some((lo, hi))
            },
        }
    }

    /// The frames a shot id plays (`141deaad0`): the matching shot, else
    /// the first, else the whole animation.
    pub fn range(&self, shot: u32) -> (u16, u16) {
        self.shots.iter().find(|s| s.id == shot).or(self.shots.first()).map_or((0, self.frames), |s| (s.first, s.end))
    }
}

/// One playing track (`141deaad0` / `141dea460`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub first: u16,
    pub end: u16,
    pub looping: bool,
    /// Frames per second of game time (30 times the animation's rate).
    pub speed: f32,
    pub frame: f32,
    pub next: f32,
}

impl Track {
    /// Starts at `start` (the auto-play record's float; -1: by `id`, as the
    /// exe phases each model) with the siege clock at `clock` seconds.
    pub fn start(h: &Header, shot: u32, start: f32, id: u32, clock: f32) -> Self {
        let (first, end) = h.range(shot);
        let start = if start == -1.0 { (id.wrapping_mul(0x302bf)) as f32 + clock } else { start };
        let len = f32::from(end.saturating_sub(first)).max(1.0);
        let off = if start >= 0.0 { start % len } else { len - (start.abs() % len) };
        let frame = f32::from(first) + off;
        let mut t = Track { first, end: end.max(first + 1), looping: h.looping, speed: FRAMES_PER_SECOND * h.rate, frame, next: frame };
        t.set_next();
        t
    }

    fn set_next(&mut self) {
        let mut n = self.frame + 1.0;
        if n >= f32::from(self.end) {
            n = if self.looping { f32::from(self.first) } else { f32::from(self.end) - 1.0 };
        }
        self.next = n.trunc() + self.frame.fract();
    }

    /// Advances by `dt` seconds (clamped to 0..1, as the exe does).
    pub fn advance(&mut self, dt: f32) {
        let d = dt.clamp(0.0, 1.0) * self.speed;
        let (first, end) = (f32::from(self.first), f32::from(self.end));
        let mut f = self.frame + d;
        if f >= end {
            if self.looping {
                let len = (end - first).max(1.0);
                f = first + (f - first) % len;
            } else {
                f = end - 1.0;
            }
        }
        self.frame = f;
        self.set_next();
    }

    /// The two integer frames the pose blends and the weight of the second.
    pub fn blend(&self) -> (usize, usize, f32) {
        (self.frame as usize, self.next as usize, self.frame.fract())
    }
}

/// Lerps two poses of a part the way the GPU program does (quaternions
/// lerped then normalised, offsets lerped).
pub fn lerp(a: &PartPose, b: &PartPose, t: f32) -> PartPose {
    let d: f32 = (0..4).map(|k| a.quat[k] * b.quat[k]).sum();
    let s = if d < 0.0 { -1.0 } else { 1.0 };
    let q: [f32; 4] = std::array::from_fn(|k| a.quat[k] * (1.0 - t) + s * b.quat[k] * t);
    let n = q.iter().map(|c| c * c).sum::<f32>().sqrt().max(1e-12);
    PartPose { quat: q.map(|c| c / n), pos: std::array::from_fn(|k| a.pos[k] * (1.0 - t) + b.pos[k] * t), pivot: a.pivot }
}

/// A map's siege animations as read: each with its header and data.
#[derive(Clone, Debug, Default)]
pub struct Anim {
    pub header: Header,
    pub compressed: Option<Compressed>,
    pub baked: Option<Baked>,
    /// The `StreamWrappedBuffer`'s package key (a retail map's).
    pub stream_key: Option<u64>,
}

impl Anim {
    /// Part `i` at a track's current blend.
    pub fn part_pose(&self, i: usize, t: &Track) -> Option<PartPose> {
        let (a, b, w) = t.blend();
        if let Some(c) = &self.compressed {
            return (i < c.parts.len()).then(|| c.part_pose(i, t.frame));
        }
        let k = self.baked.as_ref().filter(|k| i < k.parts)?;
        Some(lerp(&k.part_pose(i, a), &k.part_pose(i, b), w))
    }
}

/// Every siege animation of a walked map zone (`image` the zone's).
pub fn read_all(image: &[u8], recs: &[crate::walk::world_misc::SAnimRec]) -> Vec<Anim> {
    recs.iter()
        .map(|r| Anim {
            header: Header::read(image, r),
            compressed: r.data.as_ref().and_then(|d| Compressed::read(image, d)),
            baked: None,
            stream_key: r.stream.and_then(|b| image.get(b + 8..b + 16)).map(|s| u64::from_le_bytes(s.try_into().unwrap())).filter(|k| *k != 0),
        })
        .collect()
}

/// An auto-played siege animation: static model id, index into the
/// world's siege list, start, shot id (`SiegeAutoPlay`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoPlay {
    pub id: u32,
    pub anim: u32,
    pub start: f32,
    pub shot: u32,
}

pub fn autoplay(image: &[u8], span: Option<(usize, usize)>) -> Vec<AutoPlay> {
    let Some((at, n)) = span else { return Vec::new() };
    (0..n)
        .map(|i| {
            let r = at + 20 * i;
            AutoPlay { id: u32_at(image, r), anim: u32_at(image, r + 4), start: f32_at(image, r + 8), shot: u32_at(image, r + 12) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_key_part() -> Compressed {
        // One part: rotation from identity to 90 degrees about z over frames
        // 0..10 (Bezier control points on the endpoints), translation
        // (0,0,0) -> (10,0,0), pivot (1,0,0).
        let id = [0.0, 0.0, 0.0, 1.0];
        let rz = [0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2];
        Compressed {
            parts: vec![PartKeys { rot_first: 0, trans_first: 0, rot_count: 2, trans_count: 2 }],
            rot_times: vec![0, 10],
            trans_times: vec![0, 10],
            rot_keys: vec![[id, id, rz], [rz, rz, rz]],
            trans_keys: vec![[[0.0; 3], [0.0; 3], [10.0, 0.0, 0.0]], [[10.0, 0.0, 0.0]; 3]],
            pivots: vec![[1.0, 0.0, 0.0, 0.0]],
        }
    }

    #[test]
    fn compressed_pose_ends_and_pivot() {
        let c = two_key_part();
        assert!(c.valid());
        // Frame 0: identity, no offset: the bind position.
        let p0 = c.part_pose(0, 0.0);
        let v = p0.apply([3.0, 2.0, 1.0]);
        assert!((v[0] - 3.0).abs() < 1e-5 && (v[1] - 2.0).abs() < 1e-5 && (v[2] - 1.0).abs() < 1e-5, "{v:?}");
        // Frame 10 (the last key): 90 degrees about z around the pivot (1,0,0),
        // then +10 in x: (2,0,0) -> (1,1,0) + (10,0,0).
        let p1 = c.part_pose(0, 10.0);
        let v = p1.apply([2.0, 0.0, 0.0]);
        assert!((v[0] - 11.0).abs() < 1e-4 && (v[1] - 1.0).abs() < 1e-4 && v[2].abs() < 1e-4, "{v:?}");
        // The pivot itself only moves by the offset.
        let mid = c.part_pose(0, 5.0);
        let v = mid.apply([1.0, 0.0, 0.0]);
        assert!((v[1]).abs() < 1e-4, "{v:?}");
        // Bezier at s = 0.5 with P0 = P1 = 0, P2 = P3 = 10: 0.5^3*10 + 3*0.5^3*10 = 5.
        assert!((mid.pos[0] - 5.0).abs() < 1e-4, "{:?}", mid.pos);
    }

    #[test]
    fn track_loops_at_thirty_frames_a_second() {
        let h = Header { name: "t".into(), parts: 1, frames: 60, rate: 1.0, looping: true, shots: Vec::new(), bounds: None };
        let mut t = Track::start(&h, 0, 0.0, 7, 0.0);
        assert_eq!((t.frame, t.next), (0.0, 1.0));
        t.advance(0.5);
        assert!((t.frame - 15.0).abs() < 1e-4);
        t.advance(1.5); // clamped to 1 s: +30
        assert!((t.frame - 45.0).abs() < 1e-4);
        t.advance(0.5); // 60 -> wraps to 0
        assert!(t.frame.abs() < 1e-3, "{}", t.frame);
        // The last frame blends into the first.
        let mut u = Track::start(&h, 0, 59.25, 7, 0.0);
        assert!((u.frame - 59.25).abs() < 1e-4);
        assert!((u.next - 0.25).abs() < 1e-4, "{}", u.next);
        // Not looping: holds the last frame.
        let h2 = Header { looping: false, ..h };
        u = Track::start(&h2, 0, 0.0, 7, 0.0);
        u.advance(1.0);
        u.advance(1.0);
        u.advance(1.0);
        assert_eq!(u.frame, 59.0);
    }

    #[test]
    fn autoplay_phase_by_id() {
        let h = Header { name: "t".into(), parts: 1, frames: 120, rate: 1.0, looping: true, shots: Vec::new(), bounds: None };
        let a = Track::start(&h, 0, -1.0, 4, 0.0);
        let b = Track::start(&h, 0, -1.0, 6, 0.0);
        let want = |id: u32| ((id.wrapping_mul(0x302bf)) as f32) % 120.0;
        assert!((a.frame - want(4)).abs() < 1e-3 && (b.frame - want(6)).abs() < 1e-3);
        assert_ne!(a.frame, b.frame);
    }

    /// Small integers as half floats (exact).
    fn to_f16(v: f32) -> u16 {
        if v == 0.0 {
            return 0;
        }
        let e = v.log2().floor() as i32;
        let m = ((v / 2f32.powi(e) - 1.0) * 1024.0).round() as u16;
        (((e + 15) as u16) << 10) | m
    }

    #[test]
    fn baked_layout_found_from_unit_quaternions() {
        let (parts, frames) = (3, 5);
        let n = parts * frames;
        let sec = align256(n * 8);
        let mut d = vec![0u8; Baked::expected_size(parts, frames)];
        for i in 0..n {
            // quat (0, 0, sin, cos) as snorm16; pos (i, 0, 0) as half.
            let a = i as f32 * 0.1;
            let q = [0.0, 0.0, a.sin(), a.cos()];
            for k in 0..4 {
                let v = (q[k] * 32767.0).round() as i16;
                d[8 * i + 2 * k..8 * i + 2 * k + 2].copy_from_slice(&v.to_le_bytes());
            }
            let h = to_f16(i as f32);
            d[sec + 8 * i..sec + 8 * i + 2].copy_from_slice(&h.to_le_bytes());
        }
        for p in 0..parts {
            d[2 * sec + 16 * p..2 * sec + 16 * p + 4].copy_from_slice(&(p as f32).to_le_bytes());
        }
        let b = Baked::decode(&d, parts, frames, Some(([-20.0; 3], [20.0; 3]))).expect("decoded");
        // Offsets past the bounds: refused.
        assert!(Baked::decode(&d, parts, frames, Some(([-1.0; 3], [1.0; 3]))).is_none());
        assert_eq!(b.layout, "quat snorm16, pos half, pivots last");
        assert!((b.pos[7][0] - 7.0).abs() < 1e-3);
        assert_eq!(b.pivots[2][0], 2.0);
        // Garbage (all zero quaternions) is refused.
        assert!(Baked::decode(&vec![0u8; d.len()], parts, frames, None).is_none());
    }
}

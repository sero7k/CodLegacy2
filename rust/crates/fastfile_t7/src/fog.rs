//! bo2mp: a Black Ops III view's fog as the exe computes it, field by field
//! (`docs/T7_NATIVE.md`, "Fog").
//!
//! 1. **Bank** (`GfxConfig_Fog`, 35 floats): the world default at
//!    `GfxWorld + 0x44`, or, when the eye is inside a fog volume
//!    (`GfxWorldFogVolume`, 0x270, from `GfxWorld + 0x150`; planes at 0x160)
//!    whose lighting-state mask (word 10) and bank mask (word 11) match, that
//!    volume's bank `k` (`GfxConfig_FogBank`, 0x90, at `+0x30 + 0x90·k`; its
//!    word 10 is dropped on the way in). `k` is the lowest bit of the client's
//!    lit-fog bank mask (1 unless script changes it).
//! 2. **Client fog** (35 floats): the bank scaled by the fog modifier volume
//!    the eye is in (`GfxWorldFogModifierVolume`, 0x30, from 0x170; planes at
//!    0x180; all ones in none), with the sun angles of the current lighting
//!    state added to the bank's own.
//! 3. **Evaluation** at the eye (56 floats): BO3's exponential height fog.
//! 4. **Config** (draw state 0x170..0x23c) and the `fogConstants` block of
//!    `LightingGlobals` (0x30..0x10f), by the exe's two fixed shuffles.
//!
//! 5. **Transitions** (`FogTransitions`): the client keeps the bank and the
//!    modifier scales of the last frame and blends towards the new ones
//!    with the exe's smoothstep over time (rate: the fog volume's float at
//!    +0x20 or the world's at +0x40, per millisecond; a modifier volume's at
//!    +0x14), restarting the blend from wherever it is when the target
//!    changes, and cutting straight to the target on a cut (the eye moved
//!    over 1024 units since the last frame, or the client asked for one).
//!    A world without fog volumes uses the script-driven fog instead
//!    (`script_fog`: a linear blend between two banks over a start/end
//!    time, set by script).

use crate::native::GlobalLightSettings;

const DEG: f32 = 0.017_453_292;

fn f(image: &[u8], at: usize) -> f32 {
    image.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u(image: &[u8], at: usize) -> u32 {
    image.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Whether `eye` is inside the planes `start..start + count` (each plane
/// `dot(n, eye) + d <= 0`).
fn inside(image: &[u8], planes: usize, start: usize, count: usize, eye: [f32; 3]) -> bool {
    (start..start + count).all(|i| {
        let p = planes + 16 * i;
        eye[0] * f(image, p) + eye[1] * f(image, p + 4) + eye[2] * f(image, p + 8) + f(image, p + 12) <= 0.0
    })
}

/// The fog volume and bank in force at `eye` (none: the world default):
/// the first volume, in list order, whose lighting-state mask (word 10)
/// has the state's bit, whose bank mask (word 11) meets `bank_mask`, and
/// whose planes all have the eye behind them; bank `k` is the lowest bit of
/// `bank_mask`.
pub fn select(
    image: &[u8],
    fog_volumes: Option<(usize, usize)>,
    fog_planes: Option<(usize, usize)>,
    lighting_state: usize,
    bank_mask: u32,
    eye: [f32; 3],
) -> Option<(usize, usize)> {
    if bank_mask == 0 {
        return None;
    }
    let k = (0..4).find(|b| bank_mask >> b & 1 != 0).unwrap_or(0);
    let state_bit = 1u32 << (lighting_state.min(3) as u32);
    let ((vols, n), (planes, _)) = (fog_volumes?, fog_planes?);
    (0..n).find_map(|i| {
        let v = vols + 0x270 * i;
        if u(image, v + 40) & state_bit == 0 || u(image, v + 44) & bank_mask == 0 {
            return None;
        }
        let (start, count) = (u(image, v) as usize, u(image, v + 4) as usize);
        (count == 0 || inside(image, planes, start, count, eye)).then_some((i, k))
    })
}

/// A selection's bank (35 floats; a volume bank's float 10 dropped) and its
/// transition rate (the volume's float at +0x20, the world's at +0x40).
pub fn bank_of(image: &[u8], world_header: usize, fog_volumes: Option<(usize, usize)>, sel: Option<(usize, usize)>) -> ([f32; 35], f32) {
    let mut out = [0.0f32; 35];
    match (sel, fog_volumes) {
        (Some((i, k)), Some((vols, _))) => {
            let v = vols + 0x270 * i;
            let b = v + 0x30 + 0x90 * k;
            for (j, o) in out.iter_mut().enumerate() {
                *o = f(image, b + 4 * if j < 10 { j } else { j + 1 });
            }
            (out, f(image, v + 0x20))
        }
        _ => {
            for (j, o) in out.iter_mut().enumerate() {
                *o = f(image, world_header + 0x44 + 4 * j);
            }
            (out, f(image, world_header + 0x40))
        }
    }
}

/// Step 1: the bank in force at `eye` (settled, no transition).
pub fn bank(
    image: &[u8],
    world_header: usize,
    fog_volumes: Option<(usize, usize)>,
    fog_planes: Option<(usize, usize)>,
    lighting_state: usize,
    bank_mask: u32,
    eye: [f32; 3],
) -> [f32; 35] {
    bank_of(image, world_header, fog_volumes, select(image, fog_volumes, fog_planes, lighting_state, bank_mask, eye)).0
}

/// The fog modifier scales at `eye` (distance, `+0x1c`, r, g, b, alpha):
/// all ones outside every modifier volume.
pub fn modifier_scales(
    image: &[u8],
    modifiers: Option<(usize, usize)>,
    planes: Option<(usize, usize)>,
    bank_mask: u32,
    eye: [f32; 3],
) -> [f32; 6] {
    if bank_mask == 0 {
        return [1.0; 6];
    }
    if let (Some((mods, n)), Some((planes, _))) = (modifiers, planes) {
        for i in 0..n {
            let m = mods + 0x30 * i;
            let word = u(image, m);
            if bank_mask & word & 0xf == 0 {
                continue;
            }
            let (start, count) = ((word >> 16) as usize, ((word >> 8) & 0xff) as usize + 1);
            if inside(image, planes, start, count, eye) {
                return [f(image, m + 0x18), f(image, m + 0x1c), f(image, m + 0x20), f(image, m + 0x24), f(image, m + 0x28), f(image, m + 0x2c)];
            }
        }
    }
    [1.0; 6]
}

/// The fog modifier volume the eye is in (its index), as `modifier_scales`
/// finds it.
pub fn modifier_select(image: &[u8], modifiers: Option<(usize, usize)>, planes: Option<(usize, usize)>, bank_mask: u32, eye: [f32; 3]) -> Option<usize> {
    let ((mods, n), (planes, _)) = (modifiers?, planes?);
    (0..n).find(|&i| {
        let word = u(image, mods + 0x30 * i);
        bank_mask & word & 0xf != 0 && inside(image, planes, (word >> 16) as usize, ((word >> 8) & 0xff) as usize + 1, eye)
    })
}

/// The exe's smoothstep of a transition `elapsed` milliseconds in at `rate`
/// per millisecond: 0 before, 1 after, `(3 - 2t) t^2` between.
pub fn smoothstep(elapsed: i32, rate: f32) -> f32 {
    let t = elapsed as f32 * rate;
    if t < 0.0 {
        0.0
    } else if t < 1.0 {
        (3.0 - (t + t)) * t * t
    } else {
        1.0
    }
}

/// The world map's fog and modifier inputs (spans as the walk keeps them).
#[derive(Clone, Copy, Debug)]
pub struct FogSources {
    pub world_header: usize,
    pub fog_volumes: Option<(usize, usize)>,
    pub fog_planes: Option<(usize, usize)>,
    pub modifiers: Option<(usize, usize)>,
    pub modifier_planes: Option<(usize, usize)>,
}

impl FogSources {
    /// bo2mp: the spans of `image` [`FogTransitions::update`] reads (the
    /// world header's default bank, the fog and modifier volumes, and every
    /// plane their records name: the planes are taken by the records'
    /// indices, not checked against the list), for a copy the zone can be
    /// let go for ([`crate::kept::KeptBytes`]).
    pub fn kept_spans(&self, image: &[u8]) -> Vec<(usize, usize)> {
        let planes = |list: Option<(usize, usize)>, used: Option<usize>| list.map(|(at, n)| (at, 16usize.saturating_mul(n.max(used.unwrap_or(0)))));
        let mut spans = vec![(self.world_header, 0x44 + 4 * 35)];
        if let Some((at, n)) = self.fog_volumes {
            spans.push((at, 0x270usize.saturating_mul(n)));
            let used = (0..n).map(|i| at + 0x270 * i).filter(|&v| u(image, v + 4) != 0).map(|v| (u(image, v) as usize).saturating_add(u(image, v + 4) as usize)).max();
            spans.extend(planes(self.fog_planes, used));
        }
        if let Some((at, n)) = self.modifiers {
            spans.push((at, 0x30usize.saturating_mul(n)));
            let used = (0..n).map(|i| u(image, at + 0x30 * i)).map(|w| (w >> 16) as usize + ((w >> 8) & 0xff) as usize + 1).max();
            spans.extend(planes(self.modifier_planes, used));
        }
        spans
    }

    /// The sources at their offsets in `kept` (made from [`Self::kept_spans`]).
    pub fn rebased(&self, kept: &crate::kept::KeptBytes) -> Self {
        Self {
            world_header: kept.rebase(self.world_header),
            fog_volumes: kept.span(self.fog_volumes),
            fog_planes: kept.span(self.fog_planes),
            modifiers: kept.span(self.modifiers),
            modifier_planes: kept.span(self.modifier_planes),
        }
    }
}

/// The client's fog transition state, kept frame to frame as the exe keeps
/// it (one per local client).
#[derive(Clone, Debug)]
pub struct FogTransitions {
    last_eye: Option<[f32; 3]>,
    /// Bank: the selection now and its blend start (ms), the bank the blend
    /// runs from and the blended bank of the last frame.
    sel: Option<(usize, usize)>,
    started: i32,
    from: [f32; 35],
    current: [f32; 35],
    /// Modifier: the volume, blend start, rate, and the scales it runs from.
    mod_sel: Option<usize>,
    mod_started: i32,
    mod_rate: f32,
    mod_from: [f32; 6],
    primed: bool,
}

impl Default for FogTransitions {
    fn default() -> Self {
        Self { last_eye: None, sel: None, started: 0, from: [0.0; 35], current: [0.0; 35], mod_sel: None, mod_started: 0, mod_rate: 1.0, mod_from: [1.0; 6], primed: false }
    }
}

impl FogTransitions {
    /// One frame at client time `now` (ms): the bank and the modifier scales
    /// to build the client fog from. `cut`: the client asked for a cut (a
    /// teleport, a new map); the first frame is one too.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, image: &[u8], src: &FogSources, lighting_state: usize, bank_mask: u32, eye: [f32; 3], now: i32, cut: bool) -> ([f32; 35], [f32; 6]) {
        let moved = self.last_eye.map_or(f32::MAX, |l| (0..3).map(|i| (eye[i] - l[i]) * (eye[i] - l[i])).sum::<f32>());
        self.last_eye = Some(eye);
        let cut = cut || !self.primed || moved > 1_048_576.0;
        self.primed = true;
        // Modifier volumes.
        let msel = modifier_select(image, src.modifiers, src.modifier_planes, bank_mask, eye);
        let vol_scales = |i: usize| -> [f32; 6] {
            let m = src.modifiers.map_or(0, |(at, _)| at) + 0x30 * i;
            std::array::from_fn(|k| f(image, m + 0x18 + 4 * k))
        };
        let vol_rate = |i: usize| f(image, src.modifiers.map_or(0, |(at, _)| at) + 0x30 * i + 0x14);
        let scales = if cut {
            self.mod_sel = msel;
            self.mod_started = now;
            match msel {
                None => {
                    self.mod_rate = 1.0;
                    self.mod_from = [1.0; 6];
                }
                Some(i) => {
                    self.mod_rate = vol_rate(i);
                    self.mod_from = vol_scales(i);
                }
            }
            self.mod_from
        } else {
            let t = smoothstep(now - self.mod_started, self.mod_rate);
            let target = self.mod_sel.map_or([1.0; 6], vol_scales);
            let cur: [f32; 6] = std::array::from_fn(|k| t * target[k] + (1.0 - t) * self.mod_from[k]);
            if msel != self.mod_sel {
                self.mod_started = now;
                self.mod_sel = msel;
                if let Some(i) = msel {
                    self.mod_rate = vol_rate(i);
                }
                self.mod_from = cur;
            }
            cur
        };
        // Fog banks.
        if src.fog_volumes.is_none_or(|(_, n)| n == 0) {
            // (The script-driven fog runs instead: `script_fog`.)
            return (bank_of(image, src.world_header, None, None).0, scales);
        }
        let sel = select(image, src.fog_volumes, src.fog_planes, lighting_state, bank_mask, eye);
        let (target, rate) = bank_of(image, src.world_header, src.fog_volumes, sel);
        if cut {
            self.started = now;
            self.from = target;
            self.current = target;
            self.sel = sel;
        } else if sel != self.sel {
            self.started = now;
            self.from = self.current;
            self.sel = sel;
        }
        if !cut && rate != 0.0 {
            let t = smoothstep(now - self.started, rate);
            self.current = std::array::from_fn(|k| (target[k] - self.from[k]) * t + self.from[k]);
        } else {
            self.current = target;
            self.from = target;
        }
        (self.current, scales)
    }
}

/// The script-driven fog of a world without fog volumes: a linear blend from
/// `from` to `to` between `start` and `end` (ms; the fraction capped at 1),
/// `to` itself once `now` reaches `end`.
pub fn script_fog(from: &[f32; 35], to: &[f32; 35], start: i32, end: i32, now: i32) -> [f32; 35] {
    if now >= end {
        return *to;
    }
    let t = ((now - start) as f32 / (end - start).max(1) as f32).min(1.0);
    std::array::from_fn(|k| (to[k] - from[k]) * t + from[k])
}

/// Step 2: the client's fog from a bank, the modifier scales and the
/// current lighting state's sun settings.
pub fn client_fog(b: &[f32; 35], scales: [f32; 6], sun: Option<&GlobalLightSettings>) -> [f32; 35] {
    let [dist, _, sr, sg, sb, sa] = scales;
    let mut o = [0.0f32; 35];
    o[4] = b[5];
    let d = dist * b[6];
    o[5] = if 1.0 <= d { 1.0 / d } else { 1.0 };
    o[6] = b[7];
    o[7] = if 1.0 <= b[8] { 1.0 / b[8] } else { 1.0 };
    let e = b[9] + b[8];
    o[8] = if 1.0 <= e { 1.0 / e } else { 1.0 };
    o[0] = b[4] * b[0] * sr;
    o[1] = b[4] * b[1] * sg;
    o[2] = b[4] * b[2] * sb;
    o[3] = b[3] * sa;
    o[10] = b[14] * b[10] * sr;
    o[11] = b[14] * b[11] * sg;
    o[12] = b[14] * b[12] * sb;
    o[13] = b[13] * sa;
    let (sp, sy) = sun.map_or((0.0, 0.0), |s| (s.f(0), s.f(1)));
    let a = (b[17] + sp) * DEG;
    let c = (b[18] + sy) * DEG;
    o[14] = c.cos() * a.cos();
    o[15] = c.sin() * a.cos();
    o[16] = -a.sin();
    for i in [15, 16] {
        o[i + 2] = b[i];
    }
    for i in 19..=34 {
        o[i] = b[i];
    }
    o[9] = 1.0;
    o
}

/// Step 3: the exe's fog evaluation at `eye` (56 floats).
pub fn evaluate(p: &[f32; 35], eye: [f32; 3]) -> [f32; 56] {
    let mut o = [0.0f32; 56];
    o[0..4].copy_from_slice(&p[0..4]);
    o[4..8].copy_from_slice(&p[10..14]);
    o[17] = p[14];
    o[18] = p[15];
    o[19] = p[16];
    let c1 = (p[18] * DEG).cos();
    let c2 = (p[17] * DEG).cos();
    let inv = if c2 - c1 != 0.0 { 1.0 / (c2 - c1) } else { 10_000_000.0 };
    o[21] = inv;
    o[20] = c1 * inv;
    o[39] = p[34];
    o[43] = 0.0;
    let fv = p[31];
    o[26] = fv;
    let dz = eye[2] - p[6];
    let f3 = p[5];
    let ln2 = std::f32::consts::LN_2;
    if fv <= 0.0 {
        if f3 == 0.0 {
            return [0.0; 56];
        }
        let t = if p[9] <= f3 * 100.0 { p[9] } else { f3 * 100.0 };
        let f5 = if f3 <= t { t } else { f3 };
        let l = (f3 / f5).ln();
        let a = l - dz * p[7] * ln2;
        let b = l - dz * p[8] * ln2;
        o[8] = a;
        o[9] = b;
        o[14] = if 0.0 <= a { a + 1.0 } else { a.exp() };
        o[16] = if 0.0 <= b { b + 1.0 } else { b.exp() };
        o[10] = -f5;
        o[11] = f3 * p[4];
        o[12] = -p[7] * ln2;
        o[13] = -p[8] * ln2;
        if 0.0 < p[7] {
            o[10] = -f5 * ln2;
        }
    } else {
        let pw = 2.0f32.powf(p[30]);
        let s = p[28] * p[32];
        o[31] = p[26];
        let r = 1.0 - s;
        o[35] = 1.0 / (p[27] + 1.0e-7);
        let g = p[22];
        o[28] = p[19] * g * r;
        o[29] = p[20] * g * r;
        o[30] = p[21] * g * r;
        o[32] = p[23] * g * s;
        o[33] = p[24] * g * s;
        o[34] = p[25] * g * s;
        let x = (o[32] + o[28]).max(1.0e-4);
        let y = (o[33] + o[29]).max(1.0e-4);
        let z = (o[34] + o[30]).max(1.0e-4);
        let m = x * 0.333 + y * 0.333 + z * 0.333;
        let k = p[33];
        o[36] = (x - m) * k + m;
        o[37] = m + (y - m) * k;
        o[38] = m + (z - m) * k;
        o[40] = pw / o[36];
        o[41] = pw / o[37];
        o[42] = pw / o[38];
        o[27] = pw;
        o[23] = 0.99999 - p[29] * 0.99999;
        o[22] = p[32];
        let mut nv = -f3;
        if 0.0 < p[7] {
            nv *= ln2;
        }
        o[48] = nv;
        o[49] = nv;
        o[46] = f3 * p[4];
        o[47] = f3 * p[4];
        let e = (-p[7] * dz * ln2).exp();
        o[52] = e;
        o[53] = e;
        let e2 = (-p[8] * dz * ln2).exp();
        o[24] = e2;
        o[25] = e2;
        o[50] = ln2 * p[7];
        o[51] = ln2 * p[7];
        o[44] = ln2 * p[8];
        o[45] = ln2 * p[8];
    }
    o
}

/// Step 4a: the draw state's fog config (byte offsets 0x170..0x23c, as
/// floats from 0x170), from the evaluation; `fog_on` false (the fog dvar off,
/// or a secondary view) zeroes 0x170..0x18c only, as the exe does.
pub fn config(o: &[f32; 56], eye: [f32; 3], fog_on: bool) -> [f32; 52] {
    let mut c = [0.0f32; 52];
    let mut put = |at: usize, v: f32| c[(at - 0x170) / 4] = v;
    if !fog_on {
        return c;
    }
    for (i, at) in (0x230..0x240).step_by(4).enumerate() {
        put(at, o[4 + i]);
    }
    put(0x220, o[17]);
    put(0x224, o[18]);
    put(0x228, o[19]);
    put(0x210, o[20]);
    put(0x214, o[21]);
    for (i, at) in (0x200..0x210).step_by(4).enumerate() {
        put(at, o[i]);
    }
    let rows: &[(usize, f32)] = &[
        (0x170, o[8]), (0x174, o[10]), (0x178, o[11]), (0x17c, o[12]), (0x180, o[14]),
        (0x184, eye[1]), (0x188, eye[2]), (0x18c, 0.0),
        (0x190, o[28]), (0x194, o[29]), (0x198, o[30]), (0x19c, o[23]),
        (0x1a0, o[32]), (0x1a4, o[33]), (0x1a8, o[34]), (0x1ac, o[26]),
        (0x1b0, o[36]), (0x1b4, o[37]), (0x1b8, o[38]), (0x1bc, o[27]),
        (0x1c0, o[40]), (0x1c4, o[41]), (0x1c8, o[42]), (0x1cc, o[46]),
        (0x1d0, o[48]), (0x1d4, o[50]), (0x1d8, o[52]), (0x1dc, o[22]),
        (0x1e0, o[39]), (0x1e4, o[9]), (0x1e8, o[13]), (0x1ec, o[44]),
        (0x1f0, o[24]), (0x1f4, o[16]), (0x1f8, o[31]), (0x1fc, o[35]),
        (0x218, 0.0), (0x21c, 0.0), (0x22c, 0.0),
    ];
    for &(at, v) in rows {
        put(at, v);
    }
    c
}

/// Step 4b: `LightingGlobals.fogConstants` (byte offsets 0x30..0x10f of the
/// buffer, as 56 floats from 0x30) from the config.
pub fn fog_constants(c: &[f32; 52]) -> [f32; 56] {
    let g = |at: usize| c[(at - 0x170) / 4];
    let mut k = [0.0f32; 56];
    let mut put = |at: usize, v: f32| k[(at - 0x30) / 4] = v;
    for i in 0..4 {
        put(0x30 + 4 * i, g(0x200 + 4 * i));
        put(0x40 + 4 * i, g(0x230 + 4 * i));
    }
    let rows: &[(usize, usize)] = &[
        (0x50, 0x170), (0x58, 0x174), (0x5c, 0x178), (0x60, 0x17c), (0x68, 0x180),
        (0x74, 0x220), (0x78, 0x224), (0x7c, 0x228), (0x80, 0x210), (0x84, 0x214),
        (0xa0, 0x190), (0xa4, 0x194), (0xa8, 0x198), (0x8c, 0x19c),
        (0xb0, 0x1a0), (0xb4, 0x1a4), (0xb8, 0x1a8), (0x98, 0x1ac),
        (0xc0, 0x1b0), (0xc4, 0x1b4), (0xc8, 0x1b8), (0x9c, 0x1bc),
        (0xd0, 0x1c0), (0xd4, 0x1c4), (0xd8, 0x1c8),
        (0x88, 0x1dc), (0xcc, 0x1e0), (0x54, 0x1e4), (0x64, 0x1e8),
        (0x70, 0x1f4), (0xac, 0x1f8), (0xbc, 0x1fc),
    ];
    for &(dst, src) in rows {
        put(dst, g(src));
    }
    for (i, src) in [0x1cc, 0x1d0, 0x1d4, 0x1d8].into_iter().enumerate() {
        put(0xe8 + 8 * i, g(src));
        put(0xec + 8 * i, g(src));
    }
    put(0xe0, g(0x1ec));
    put(0xe4, g(0x1ec));
    put(0x90, g(0x1f0));
    put(0x94, g(0x1f0));
    put(0xdc, 0.0);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_density_is_no_fog() {
        let p = [0.0f32; 35];
        assert_eq!(evaluate(&p, [0.0; 3]), [0.0; 56]);
    }

    #[test]
    fn height_fog_terms_follow_the_exe() {
        let mut p = [0.0f32; 35];
        p[5] = 0.001; // density
        p[6] = 100.0; // base height
        p[7] = 0.01;
        p[8] = 0.02;
        p[9] = 1.0;
        let o = evaluate(&p, [0.0, 0.0, 200.0]);
        let ln2 = std::f32::consts::LN_2;
        // f5 = max(f3, min(100 f3, p9)) = 0.1; l = ln(0.01)
        let l = (0.001f32 / 0.1).ln();
        assert!((o[8] - (l - 100.0 * 0.01 * ln2)).abs() < 1e-5);
        assert!((o[10] + 0.1 * ln2).abs() < 1e-6);
        assert!((o[14] - o[8].exp()).abs() < 1e-6);
        let c = config(&o, [0.0, 0.0, 200.0], true);
        let k = fog_constants(&c);
        assert_eq!(k[(0x50 - 0x30) / 4], o[8]);
        assert_eq!(k[(0x58 - 0x30) / 4], o[10]);
    }

    #[test]
    fn a_volume_bank_skips_its_word_ten() {
        let mut img = vec![0u8; 0x2000];
        // One fog volume at 0x400 with no planes, masks all set; bank 0 at +0x30.
        img[0x400 + 40..0x400 + 44].copy_from_slice(&0xffu32.to_le_bytes());
        img[0x400 + 44..0x400 + 48].copy_from_slice(&0xffu32.to_le_bytes());
        for j in 0..36 {
            img[0x430 + 4 * j..0x434 + 4 * j].copy_from_slice(&(j as f32).to_le_bytes());
        }
        let b = bank(&img, 0, Some((0x400, 1)), Some((0x1000, 0)), 0, 1, [0.0; 3]);
        assert_eq!(b[9], 9.0);
        assert_eq!(b[10], 11.0);
        assert_eq!(b[34], 35.0);
        // No volume: the world default at 0x44.
        let b = bank(&img, 0x800, None, None, 0, 1, [0.0; 3]);
        assert_eq!(b, [0.0; 35]);
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    #[test]
    fn smoothstep_like_the_exe() {
        assert_eq!(smoothstep(-5, 0.01), 0.0);
        assert_eq!(smoothstep(50, 0.01), 0.5);
        assert_eq!(smoothstep(200, 0.01), 1.0);
    }

    #[test]
    fn world_default_settles_and_cuts() {
        // A world header with fog float 0 = 2 and rate 0.001, no volumes.
        let mut img = vec![0u8; 0x200];
        img[0x44..0x48].copy_from_slice(&2.0f32.to_le_bytes());
        let src = FogSources { world_header: 0, fog_volumes: None, fog_planes: None, modifiers: None, modifier_planes: None };
        let mut t = FogTransitions::default();
        let (b, s) = t.update(&img, &src, 0, 1, [0.0; 3], 0, false);
        assert_eq!(b[0], 2.0);
        assert_eq!(s, [1.0; 6]);
    }
}

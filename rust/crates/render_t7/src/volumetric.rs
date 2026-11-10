//! Lit fog volumes and the volumetric light pass as BlackOps3.exe sets them
//! up (`docs/T7_NATIVE.md`, "Lit fog and volumetric light").
//!
//! * The map's lit fog volumes (`GfxWorld` +0x5a8 count, +0x5b0 records of
//!   0x168; planes at +0x5c0) each keep a per-client state (0xf4 bytes, 64
//!   volumes per client): the bank shown, the bank blended to, the blend's
//!   start and length, and three copies of a bank's 19 floats (from, to,
//!   current). At map load every copy is bank 0; a script selects another
//!   bank and the client blends to it each frame — [`BankStates`].
//! * Per view the volumes the view can see within 8000 units are packed,
//!   at most 64, into `gLitFogVolumes` records of 0xd0 bytes (two builder
//!   steps and a copy that drops one word) — [`pack`]; the list keeps the
//!   count and the OR of the records' flag words, which pick the passes.
//! * The pass (the volumetric driver, the march and the apply) runs when the
//!   volumetric dvars are on (both default on), the view has the volumetric
//!   flag (the dvar, copied by the client into the scene) and the list is
//!   not empty: a 4× depth downsample (`downsample_depth_4x_compute` into
//!   render target 0x26), the ray march into render target 0x28
//!   (quarter size, RGBA16F) with `volumetric_light_sun(_skip_samples)` when
//!   a record has a sun flag and `volumetric_light(_skip_samples)` when one
//!   has the local-light flag, a horizontal and a vertical blur
//!   (`volumetric_light_hblur/vblur_compute`, through render target 0x29)
//!   and `volumetric_light_apply` over the scene — [`plan`], with the
//!   constants of [`constants`].

/// A packed record's size (`gLitFogVolumes` `$Element`, 208 bytes).
pub const RECORD: usize = 0xd0;
/// The most records a view packs (the packer stops at 0x40).
pub const MAX: usize = 64;
/// A lit fog volume record in the map (`GfxLitFogVolume`).
pub const VOLUME: usize = 0x168;
/// Floats in a bank (`+0x38 + 0x4c·bank`, four banks).
pub const BANK_FLOATS: usize = 19;
/// How far outside a volume's planes the eye may be for it to be packed.
pub const NEAR: f32 = 8000.0;

/// The exe's dvars of the pass (names are hashed in the exe; defaults from
/// their registration).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dvars {
    /// `0x977bd730` (bool, 1): copied by the client into the scene each
    /// frame, the view's volumetric flag (view +0xb2674).
    pub view_flag: bool,
    /// `0x61444835` (bool, 1): the pass at all.
    pub enabled: bool,
    /// `0x2fbd3d93` (enum, 0): `2DRayMarch`, `3DVolume`, `2DRayMarchUnified`.
    pub mode: i32,
    /// `0x4fdd24df` (int, 40, 1..256): into `postFxControl4.w`'s low half.
    pub samples: u16,
    /// `0xa90e937b` (int, 8, 1..256): into `postFxControl4.w`'s high half.
    pub steps: u16,
    /// `0xfc7f08f7` (float, 10, 0..40), `0x46d615f1` (float, 0.01, 0..1),
    /// `0xa08b16df` (float, 2000, 0..10000): `postFxControlA.xyz`.
    pub blur: [f32; 3],
    /// `0xc61a70a7` (bool, 0): half-resolution apply plus upsample.
    pub upsample: bool,
    /// `0xe239ecd2` (bool, 1): the sun march's `_skip_samples` program.
    pub sun_skip_samples: bool,
    /// `0x1c7ac809` (bool, 1): the light march's `_skip_samples` program.
    pub skip_samples: bool,
}

impl Default for Dvars {
    fn default() -> Self {
        Self { view_flag: true, enabled: true, mode: 0, samples: 40, steps: 8, blur: [10.0, 0.01, 2000.0], upsample: false, sun_skip_samples: true, skip_samples: true }
    }
}

impl Dvars {
    /// The defaults, `T7_VOLUMETRIC=0` turning the view flag off (as the
    /// dvar would).
    pub fn current() -> Self {
        let mut d = Self::default();
        if std::env::var("T7_VOLUMETRIC").is_ok_and(|v| v.trim() == "0") {
            d.view_flag = false;
        }
        d
    }
}

/// Whether the GPU passes run: on by default, `T7_VOLUMETRIC_GPU=0` off.
/// (Off for a while: Outlaw drew white on the PC, which turned out to be
/// its grade LUT, not the fog: offline its image is the same with the
/// passes on and off.)
pub fn gpu_on() -> bool {
    gpu_setting() != Some(false)
}

/// `T7_VOLUMETRIC_GPU` as set: `Some(false)` for 0, `Some(true)` for any
/// other value, `None` unset.
fn gpu_setting() -> Option<bool> {
    static ON: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_VOLUMETRIC_GPU").ok().map(|v| v.trim() != "0"))
}

/// Whether the GPU passes run for a map: as [`gpu_on`], except that a
/// Steam Workshop map / usermap (`Scene::workshop`) has them only with
/// `T7_VOLUMETRIC_GPU=1`. (bo2mp stop-gap, 2026-10-28: Workshop Warhawk drew
/// the whole view in a milky white haze on the PC; its map-wide volume is
/// the first staged one lit by local lights over many volumetric lights,
/// with banks unlike any retail map's. Not proven to be the cause.)
pub fn gpu_on_for(workshop: bool) -> bool {
    match gpu_setting() {
        Some(on) => on,
        None => !workshop && crate::quality::get().volumetrics,
    }
}

/// `T7_VOLUMETRIC_TEST=1` (offline checks only): every lit fog volume also
/// has flags 0x40 and 0x80 (local lights, the 3-D volume) and every packed
/// light and probe counts as volumetric, so the local-light march and the
/// 3-D volume run on maps that have none (no staged map does).
pub fn test_mode() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_VOLUMETRIC_TEST").is_ok_and(|v| v.trim() == "1"))
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    b.get(o..o + 4).map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

/// The map's lit fog volumes: the zone image and the two arrays.
#[derive(Clone, Copy, Debug)]
pub struct Volumes<'a> {
    pub image: &'a [u8],
    /// (offset, count) of the 0x168-byte records.
    pub volumes: (usize, usize),
    /// Offset of the plane array (vec4s).
    pub planes: Option<(usize, usize)>,
}

impl<'a> Volumes<'a> {
    pub fn len(&self) -> usize {
        self.volumes.1
    }
    pub fn is_empty(&self) -> bool {
        self.volumes.1 == 0
    }
    fn record(&self, i: usize) -> &'a [u8] {
        let at = self.volumes.0 + VOLUME * i;
        self.image.get(at..at + VOLUME).unwrap_or(&[])
    }
    fn bank(&self, i: usize, bank: usize) -> [f32; BANK_FLOATS] {
        let r = self.record(i);
        std::array::from_fn(|k| f32_at(r, 0x38 + 0x4c * bank.min(3) + 4 * k))
    }
    fn plane(&self, k: usize) -> Option<[f32; 4]> {
        let (at, n) = self.planes?;
        (k < n).then(|| std::array::from_fn(|j| f32_at(self.image, at + 16 * k + 4 * j)))
    }
}

/// One volume's client state (0xf4 bytes in the exe).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BankState {
    /// +0: the blend's start (ms); +4 its length (ms).
    pub start: i32,
    pub length: i32,
    /// +8: the bank shown; +0xc: the bank blended to.
    pub bank: u32,
    pub target: u32,
    /// +0x10, +0x5c, +0xa8.
    pub from: [f32; BANK_FLOATS],
    pub to: [f32; BANK_FLOATS],
    pub current: [f32; BANK_FLOATS],
}

/// Every volume's state for one client.
#[derive(Clone, Debug, Default)]
pub struct BankStates(pub Vec<BankState>);

/// The fields the blend eases; the others jump at its end.
const BLENDED: [usize; 9] = [0, 1, 2, 7, 8, 9, 10, 12, 13];

impl BankStates {
    /// At map load: bank 0 everywhere.
    pub fn new(v: &Volumes) -> Self {
        Self(
            (0..v.len())
                .map(|i| {
                    let b = v.bank(i, 0);
                    BankState { start: 0, length: 0, bank: 0, target: 0, from: b, to: b, current: b }
                })
                .collect(),
        )
    }

    /// A script's bank change for the volumes in `mask`:
    /// blend from where each is to bank `bank` (when the volume has it; else
    /// to where it is) over `length` ms, or the volume's own time (+0x34
    /// seconds) when `length` is negative.
    pub fn select(&mut self, v: &Volumes, mask: u64, bank: u32, now: i32, length: i32) {
        for i in 0..self.0.len().min(64) {
            if mask >> i & 1 == 0 {
                continue;
            }
            let r = v.record(i);
            let s = &mut self.0[i];
            s.start = now;
            s.length = if length < 0 { (f32_at(r, 0x34) * 1000.0) as i32 } else { length };
            s.target = bank;
            s.from = s.current;
            let flags = u32_at(r, 0x2c) & 0xffff;
            s.to = if bank < 32 && flags >> bank & 1 != 0 { v.bank(i, bank as usize) } else { s.current };
        }
    }

    /// The frame's blend, `now` in ms.
    pub fn update(&mut self, now: i32) {
        for s in &mut self.0 {
            if s.bank == s.target {
                continue;
            }
            let t = (now - s.start) as f32 / s.length as f32;
            if !(t > 0.0) {
                s.current = s.from;
            } else if t < 1.0 {
                let e = (3.0 - (t + t)) * t * t;
                for k in BLENDED {
                    s.current[k] = (s.to[k] - s.from[k]) * e + s.from[k];
                }
            } else {
                s.current = s.to;
                s.bank = s.target;
            }
        }
    }
}

/// What a view packs from: the eye, the frustum (inward planes `n·p + d`,
/// a box is out when its farthest corner along `n` gives ≤ 0), the sun
/// config (colour × intensity at +0, intensity +0xc, shadow mode +0x2c),
/// the lighting state.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub eye: [f32; 3],
    pub frustum: &'a [[f32; 4]],
    pub sun_colour: [f32; 3],
    pub sun_intensity: f32,
    pub sun_mode: i32,
    pub lighting_state: u32,
}

/// The view's list (one of the exe's six ring entries): records, count, flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Packed {
    pub records: Vec<u8>,
    pub count: u32,
    /// OR of the records' flag words (+0x90): bit 0 the volume's flag 0x20
    /// (sun), 1 its 0x40 (local lights), 2 its 0x80, 4 its 0x100 (lit
    /// without a sun).
    pub flags: u32,
    /// The packed volumes' indices (for the log).
    pub volumes: Vec<usize>,
}

fn culled(lo: [f32; 3], hi: [f32; 3], frustum: &[[f32; 4]]) -> bool {
    // (The exe's box-against-planes test.)
    frustum.iter().any(|p| {
        let c = |k: usize| if p[k] < 0.0 { lo[k] } else { hi[k] };
        p[0] * c(0) + p[1] * c(1) + p[2] * c(2) + p[3] <= 0.0
    })
}

/// The six planes of a volume (`n`, `d`: inside is `n·p ≤ d`): its own when
/// it has six whose opposite pairs cancel (each pair's components summing
/// to at most 0.001), else its box.
fn planes(v: &Volumes, r: &[u8], lo: [f32; 3], hi: [f32; 3]) -> [[f32; 4]; 6] {
    if u32_at(r, 4) == 6 {
        let first = u32_at(r, 0) as usize;
        let p: Option<Vec<[f32; 4]>> = (0..6).map(|k| v.plane(first + k)).collect();
        if let Some(p) = p {
            let s = |a: usize, b: usize| p[a][0] + p[a][1] + p[a][2] + p[b][0] + p[b][1] + p[b][2];
            if !(0.001 < s(0, 1)) && !(0.001 < s(2, 4)) && !(0.001 < s(3, 5)) {
                return [p[0], p[1], p[2], p[3], p[4], p[5]];
            }
        }
    }
    [[0.0, 0.0, -1.0, -lo[2]], [0.0, 0.0, 1.0, hi[2]], [0.0, 1.0, 0.0, hi[1]], [1.0, 0.0, 0.0, hi[0]], [0.0, -1.0, 0.0, -lo[1]], [-1.0, 0.0, 0.0, -lo[0]]]
}

fn clamp_half(x: f32) -> f32 {
    let x = if x < 65504.0 { x } else { 65504.0 };
    if x < 6.103_515_6e-5 { 6.103_515_6e-5 } else { x }
}

/// One record (the two builder steps, then the copy that drops word 0x24).
fn record(v: &Volumes, r: &[u8], c: &[f32; BANK_FLOATS], flags: u32, view: &View, planes: &[[f32; 4]; 6], lo: [f32; 3], hi: [f32; 3]) -> ([u8; RECORD], u32) {
    let eye = view.eye;
    let _ = v;
    // The first step.
    let o40 = if 1.0 <= c[8] { 1.0 / c[8] } else { 1.0 };
    let o44 = if 1.0 <= c[10] { 1.0 / c[10] } else { 1.0 };
    let o48 = { let x = if 65535.0 <= c[7] { 65535.0 } else { c[7] }; if 0.0 <= x { x } else { 0.0 } };
    let o4c = { let x = eye[2] - c[9]; let x = if 32767.0 <= x { 32767.0 } else { x }; if -32768.0 <= x { x } else { -32768.0 } };
    let o50 = {
        let k = if 0.99 <= c[12] { 0.99 } else { c[12] };
        let k = if -0.99 <= k { k } else { -0.99 };
        let s = k * 1.55 - k * 0.55 * k * k;
        let s = if 0.99 <= s { 0.99 } else { s };
        if -0.99 <= s { s } else { -0.99 }
    };
    let o54 = { let x = if 1.0 <= c[13] { 1.0 } else { c[13] }; if 0.0 <= x { x } else { 0.0 } };
    // The colour's direction, halved, as optical depth: −log2 per channel,
    // less the share taken by +0x54.
    let len = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    let inv = if len * len > 0.0 { 1.0 / len } else { 0.0 };
    let o18: [f32; 3] = std::array::from_fn(|k| {
        let x = c[k] * inv * 0.5;
        let x = if x < 1e-8 { 1e-8 } else { x };
        let x = if 1.0 <= x { 1.0 } else { x };
        let x = if 0.0 <= x { x } else { 0.0 };
        x.ln() * -1.442_695 * (1.0 - o54)
    });
    let o24: [f32; 3] = std::array::from_fn(|k| clamp_half(f32_at(r, 0x20 + 4 * k)));
    let bit = |b: u32| flags >> b & 1 != 0;
    let o58 = (if bit(6) { 2 } else { 0 }) | u32::from(bit(5)) | (if bit(7) { 4 } else { 0 }) | (if bit(8) { 0x10 } else { 0 });
    // The second step (then the copy without word 0x24).
    let mut w = [0u32; RECORD / 4];
    let mut put = |k: usize, x: f32| w[k] = x.to_bits();
    for k in 0..3 {
        put(k, lo[k] - eye[k]);
        put(3 + k, hi[k] - eye[k]);
    }
    for (slot, p) in [0usize, 1, 2, 4, 3, 5].into_iter().enumerate() {
        let p = planes[p];
        for k in 0..3 {
            let n = if p[k] < 1.0 { p[k] } else { 1.0 };
            put(6 + 4 * slot + k, if n < -1.0 { -1.0 } else { n });
        }
        let d = p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] - p[3];
        let d = if 65504.0 <= d { 65504.0 } else { d };
        put(6 + 4 * slot + 3, if -65504.0 <= d { d } else { -65504.0 });
    }
    put(0x1e, o40);
    put(0x1f, o44);
    put(0x20, o4c);
    put(0x21, o48);
    put(0x22, o50);
    put(0x23, o54);
    put(0x24, f32::from_bits(o58));
    for k in 0..3 {
        put(0x25 + k, o18[k]);
        put(0x28 + k, clamp_half(o24[k]));
        let s = view.sun_colour[k];
        put(0x2b + k, s + (c[3 + k] * view.sun_intensity - s) * c[6]);
    }
    for k in 0..5 {
        put(0x2e + k, c[14 + k]);
    }
    let mut b = [0u8; RECORD];
    for (k, x) in w.iter().enumerate() {
        b[4 * k..4 * k + 4].copy_from_slice(&x.to_le_bytes());
    }
    (b, o58)
}

/// The view's list.
pub fn pack(v: &Volumes, states: &BankStates, view: &View) -> Packed {
    let mut out = Packed::default();
    let no_sun = view.sun_mode == 0 || view.sun_intensity <= 0.0;
    let keep = if no_sun { 0xffff_ffdf } else { 0xffff_ffff };
    let state_bit = 0x1000u32 << (view.lighting_state & 31);
    let eye = view.eye;
    for i in 0..v.len() {
        let r = v.record(i);
        let Some(s) = states.0.get(i) else { break };
        let mut flags = u32_at(r, 0x2c) & 0xffff;
        if test_mode() {
            flags |= 0xc0;
        }
        if no_sun && flags & 0xa0 == 0x20 && 0.0 < f32_at(r, 0x20) + f32_at(r, 0x24) + f32_at(r, 0x28) {
            flags |= 0x100;
        }
        flags &= keep;
        if flags & state_bit == 0 || flags >> 9 & 1 != 0 || flags & 0x1e0 == 0 || flags >> (s.bank & 31) & 1 == 0 {
            continue;
        }
        let lo = [f32_at(r, 8), f32_at(r, 12), f32_at(r, 16)];
        let hi = [f32_at(r, 20), f32_at(r, 24), f32_at(r, 28)];
        if culled(lo, hi, view.frustum) {
            continue;
        }
        let p = planes(v, r, lo, hi);
        let inside = p.iter().filter(|p| p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] - (p[3] + NEAR) < 0.0).count();
        if inside != 6 {
            continue;
        }
        let (rec, f) = record(v, r, &s.current, flags, view, &p, lo, hi);
        out.records.extend_from_slice(&rec);
        out.flags |= f;
        out.count += 1;
        out.volumes.push(i);
        if out.count as usize == MAX {
            break;
        }
    }
    if out.flags == 0 {
        out.count = 0;
    }
    out
}

/// The passes the exe records for a list, in order; empty when the pass
/// does not run.
pub fn plan(d: &Dvars, p: &Packed) -> Vec<&'static str> {
    if !d.enabled || !d.view_flag || p.count == 0 || d.mode == 1 {
        return Vec::new();
    }
    let mut v = vec!["downsample_depth_4x_compute"];
    if d.mode == 2 {
        v.push("volumetric_light_unified");
    } else {
        if p.flags & 0x15 != 0 {
            v.push(if d.sun_skip_samples { "volumetric_light_sun_skip_samples" } else { "volumetric_light_sun" });
        }
        if p.flags & 2 != 0 {
            v.push(if d.skip_samples { "volumetric_light_skip_samples" } else { "volumetric_light" });
        }
    }
    if p.flags & 7 != 0 {
        v.extend(["volumetric_light_hblur_compute", "volumetric_light_vblur_compute"]);
    }
    if d.upsample {
        v.extend(["volumetric_light_upsample", "volumetric_light_half_res_apply"]);
    } else {
        v.push("volumetric_light_apply");
    }
    v
}

/// The pre-pass's programs for a list (before the opaque lists): the depth
/// downsample, then with flag 2 the volumetric light culling, with flag 4
/// the volumetric probe culling and the 3-D volume
/// (`volumetric_light_compute`). Empty when the pass does not run.
pub fn prepass(d: &Dvars, p: &Packed) -> Vec<&'static str> {
    if !d.enabled || !d.view_flag || p.count == 0 {
        return Vec::new();
    }
    let mut v = vec!["downsample_depth_with_uv_4x_compute"];
    if p.flags & 2 != 0 {
        v.push("volumetric_light_cull_compute");
    }
    if p.flags & 4 != 0 {
        v.extend(["volumetric_probe_cull_compute", "volumetric_light_compute"]);
    }
    v
}

/// The 3-D volume's size for a `w` × `h` view: one texel per 4x4 texels of
/// the quarter depth (16x16 pixels; the program reads that block and
/// maps a texel to `16 / postFxControl7.x` quarter texels), 64 slices.
pub fn volume_size(w: u32, h: u32) -> [u32; 3] {
    let (qw, qh) = quarter(w, h);
    [qw.div_ceil(4), qh.div_ceil(4), 64]
}

/// The quarter size of a `w` × `h` view (render targets 0x25..0x29: the
/// exe's size classes 2 and 9, `(w + 3) >> 2`).
pub fn quarter(w: u32, h: u32) -> (u32, u32) {
    ((w + 3) >> 2, (h + 3) >> 2)
}

/// `postFxControl0..7` and `postFxControlA` (registers 0..7 and 10 of the
/// result) for the march: the inverse of the view matrix
/// (`view_matrix`, the `viewMatrix` registers), `1/P00`, `1/P11` of the
/// projection, the list's count and flags and the sample counts as 16-bit
/// halves, the eye and the scene-to-march width ratio, the view matrix's
/// third column, `(4, 0, 0, 0)`, and the blur dvars.
pub fn constants(view_matrix: &[[f32; 4]], projection: &[[f32; 4]], eye: [f32; 3], scene_width: u32, march_width: u32, p: &Packed, d: &Dvars) -> [[f32; 4]; 11] {
    let m = glam::Mat4::from_cols_array_2d(&std::array::from_fn(|i| view_matrix.get(i).copied().unwrap_or([0.0; 4])));
    // (Registers as the columns of `m`: its inverse's columns are the
    // inverse's registers, the same layout either way.)
    let inv = m.inverse();
    let mut c = [[0f32; 4]; 11];
    for i in 0..4 {
        c[i] = inv.col(i).to_array();
    }
    let p00 = projection.first().map_or(1.0, |r| r[0]);
    let p11 = projection.get(1).map_or(1.0, |r| r[1]);
    c[4] = [1.0 / p00, 1.0 / p11, f32::from_bits((p.count & 0xffff) | (p.flags & 0xffff) << 16), f32::from_bits(u32::from(d.samples) | u32::from(d.steps) << 16)];
    c[5] = [eye[0], eye[1], eye[2], scene_width as f32 / march_width.max(1) as f32];
    let col = |r: usize| view_matrix.get(r).map_or(0.0, |x| x[2]);
    c[6] = [col(0), col(1), col(2), col(3)];
    c[7] = [4.0, 0.0, 0.0, 0.0];
    c[10] = [d.blur[0], d.blur[1], d.blur[2], 0.0];
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(flags: u32, lo: [f32; 3], hi: [f32; 3], bank0: [f32; BANK_FLOATS]) -> Vec<u8> {
        let mut r = vec![0u8; VOLUME];
        let mut put = |o: usize, v: u32| r[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(4, 6);
        for k in 0..3 {
            put(8 + 4 * k, lo[k].to_bits());
            put(20 + 4 * k, hi[k].to_bits());
        }
        put(0x2c, flags);
        put(0x34, 1.0f32.to_bits());
        for (k, x) in bank0.iter().enumerate() {
            put(0x38 + 4 * k, x.to_bits());
        }
        r
    }

    fn bank() -> [f32; BANK_FLOATS] {
        let mut b = [1.0f32; BANK_FLOATS];
        b[7] = 1500.0;
        b[8] = 10.0;
        b[9] = 0.0;
        b[10] = 2500.0;
        b[12] = 0.5;
        b[13] = 0.25;
        b
    }

    fn view<'a>(eye: [f32; 3], frustum: &'a [[f32; 4]]) -> View<'a> {
        View { eye, frustum, sun_colour: [2.0, 2.0, 2.0], sun_intensity: 2.0, sun_mode: 3, lighting_state: 0 }
    }

    #[test]
    fn packs_a_box_volume_as_the_exe() {
        // mp_sector's first volume's flags: state 0, bank 0, sun (0x20).
        let img = volume(0x1021, [-100.0, -100.0, 0.0], [100.0, 100.0, 50.0], bank());
        let v = Volumes { image: &img, volumes: (0, 1), planes: None };
        let s = BankStates::new(&v);
        let p = pack(&v, &s, &view([0.0, 0.0, 10.0], &[]));
        assert_eq!((p.count, p.flags, p.records.len()), (1, 1, RECORD));
        let f = |k: usize| f32::from_le_bytes(p.records[4 * k..4 * k + 4].try_into().unwrap());
        assert_eq!([f(0), f(1), f(2)], [-100.0, -100.0, -10.0]);
        // Plane 0 (0, 0, -1, -lo.z): d' = n·eye − d = −10.
        assert_eq!([f(6), f(7), f(8), f(9)], [0.0, 0.0, -1.0, -10.0]);
        // Slot 3 is the box's plane 4 (−y).
        assert_eq!([f(18), f(19), f(20)], [0.0, -1.0, 0.0]);
        assert_eq!(f(0x1e), 0.1);
        assert_eq!(f(0x1f), 1.0 / 2500.0);
        assert_eq!(f(0x20), 10.0);
        assert_eq!(f(0x21), 1500.0);
        assert!((f(0x22) - (0.5 * 1.55 - 0.55 * 0.125)).abs() < 1e-6);
        assert_eq!(u32::from_le_bytes(p.records[0x90..0x94].try_into().unwrap()), 1);
        // −log2(1/(2√3)) × (1 − 0.25).
        let want = -(0.5f32 / 3f32.sqrt()).ln() / std::f32::consts::LN_2 * 0.75;
        assert!((f(0x25) - want).abs() < 1e-5);
        // lerp(sun colour, colour × intensity, weight 1).
        assert_eq!(f(0x2b), 2.0);
        assert_eq!(f(0x33), 0.0);
    }

    #[test]
    fn skips_what_the_exe_skips() {
        let far = volume(0x1021, [9000.0, 0.0, 0.0], [9100.0, 10.0, 10.0], bank());
        let v = Volumes { image: &far, volumes: (0, 1), planes: None };
        assert_eq!(pack(&v, &BankStates::new(&v), &view([0.0; 3], &[])).count, 0);
        // Nuketown's flags (0x21): no lighting-state bit.
        let nuk = volume(0x21, [-1e4; 3], [1e4; 3], bank());
        let v = Volumes { image: &nuk, volumes: (0, 1), planes: None };
        assert_eq!(pack(&v, &BankStates::new(&v), &view([0.0; 3], &[])).count, 0);
        // Out of the frustum.
        let near = volume(0x1021, [-10.0; 3], [10.0; 3], bank());
        let v = Volumes { image: &near, volumes: (0, 1), planes: None };
        assert_eq!(pack(&v, &BankStates::new(&v), &view([0.0; 3], &[[1.0, 0.0, 0.0, -20.0]])).count, 0);
        // No sun: the sun flag goes, and with no other flag nothing packs.
        let mut vw = view([0.0; 3], &[]);
        vw.sun_mode = 0;
        assert_eq!(pack(&v, &BankStates::new(&v), &vw).count, 0);
    }

    #[test]
    fn bank_blend() {
        let mut img = volume(0x1023, [-10.0; 3], [10.0; 3], bank());
        let mut b1 = bank();
        b1[8] = 30.0;
        b1[3] = 5.0;
        for (k, x) in b1.iter().enumerate() {
            img[0x38 + 0x4c + 4 * k..0x38 + 0x4c + 4 * k + 4].copy_from_slice(&x.to_bits().to_le_bytes());
        }
        let v = Volumes { image: &img, volumes: (0, 1), planes: None };
        let mut s = BankStates::new(&v);
        s.select(&v, 1, 1, 1000, -1);
        assert_eq!(s.0[0].length, 1000);
        s.update(1500);
        assert_eq!(s.0[0].current[8], 20.0);
        assert_eq!(s.0[0].current[3], 1.0);
        s.update(2000);
        assert_eq!((s.0[0].bank, s.0[0].current[3]), (1, 5.0));
    }

    #[test]
    fn plans() {
        let d = Dvars::default();
        let p = Packed { count: 1, flags: 1, ..Default::default() };
        assert_eq!(plan(&d, &p), vec!["downsample_depth_4x_compute", "volumetric_light_sun_skip_samples", "volumetric_light_hblur_compute", "volumetric_light_vblur_compute", "volumetric_light_apply"]);
        assert!(plan(&d, &Packed::default()).is_empty());
        let p6 = Packed { count: 2, flags: 7, ..Default::default() };
        assert_eq!(plan(&d, &p6)[1..3], ["volumetric_light_sun_skip_samples", "volumetric_light_skip_samples"]);
        assert_eq!(prepass(&d, &p), vec!["downsample_depth_with_uv_4x_compute"]);
        assert_eq!(prepass(&d, &p6), vec!["downsample_depth_with_uv_4x_compute", "volumetric_light_cull_compute", "volumetric_probe_cull_compute", "volumetric_light_compute"]);
        assert!(prepass(&d, &Packed::default()).is_empty());
        assert_eq!(volume_size(1920, 1080), [120, 68, 64]);
        let c = constants(&[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]], &[[2.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0]], [1.0, 2.0, 3.0], 1920, 480, &p, &d);
        assert_eq!(c[4][0], 0.5);
        assert_eq!(c[4][2].to_bits(), 0x0001_0001);
        assert_eq!(c[4][3].to_bits(), 40 | 8 << 16);
        assert_eq!(c[5], [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(c[10], [10.0, 0.01, 2000.0, 0.0]);
    }

    /// Offline check on a staged zone: `T7_VOLUMETRIC_ZONE=<map.ff> cargo
    /// test -p render_t7 --lib volumetric -- --ignored --nocapture`: each
    /// lit fog volume packed for an eye at its centre (no frustum), with
    /// the map's sun.
    #[test]
    #[ignore]
    fn staged_zone() {
        let Ok(path) = std::env::var("T7_VOLUMETRIC_ZONE") else { return };
        let z = fastfile_t7::Zone::open(std::path::Path::new(&path)).expect("zone");
        let w = fastfile_t7::walk::walk(&z);
        let g = w.capture.world.gfx_worlds.first().expect("gfxworld");
        let Some(vols) = g.lit_fog_volumes else {
            println!("t7 volumetrics: {path}: no lit fog volumes");
            return;
        };
        let sun = g.sun_volumes.first().and_then(|&s| fastfile_t7::native::global_light_settings(&z.image, s, 0)).map(|s| fastfile_t7::native::view_sun(&s)).unwrap_or_default();
        let v = Volumes { image: &z.image, volumes: vols, planes: g.lit_fog_planes };
        let st = BankStates::new(&v);
        for i in 0..v.len() {
            let r = v.record(i);
            let eye: [f32; 3] = std::array::from_fn(|k| (f32_at(r, 8 + 4 * k) + f32_at(r, 20 + 4 * k)) * 0.5);
            let p = pack(&v, &st, &View { eye, frustum: &[], sun_colour: sun.colour, sun_intensity: sun.intensity, sun_mode: sun.shadow_mode, lighting_state: 0 });
            println!("t7 volumetrics: {path}: volume {i} flags {:#x}, eye at its centre {eye:?}: packed {} (volumes {:?}, flags {:#x}), passes {:?}", u32_at(r, 0x2c), p.count, p.volumes, p.flags, plan(&Dvars::default(), &p));
            if let Some(k) = p.volumes.iter().position(|&x| x == i) {
                let rec = &p.records[RECORD * k..RECORD * (k + 1)];
                let f: Vec<String> = (0..RECORD / 4).map(|j| if j == 0x24 { format!("{:#x}", u32_at(rec, 0x90)) } else { format!("{:.4}", f32_at(rec, 4 * j)) }).collect();
                println!("  record: {}", f.join(" "));
            }
        }
    }
}

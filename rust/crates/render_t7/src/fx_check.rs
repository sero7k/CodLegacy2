//! Checks of the native effects without a GPU (`render_t7::fx`), so that
//! what the owner saw on Nuk3town (rotated glow strips standing up as cyan
//! walls, sprites drawn as large solid blocks) cannot come back unseen:
//!
//! * [`sim`]: every placed effect of a map played (with its runners' and
//!   death effects) and stepped through time; each sprite record decoded
//!   and held against the exe's rules for its element (`FUN_141573090`,
//!   `FUN_14157cc80`, `FUN_14156fce0`; see docs/T7_VISUAL_PARITY.md):
//!   finite words, half sizes and colours inside the def's visual graphs,
//!   the def's draw words (alpha mode, soft depth, view-angle fade, pivot),
//!   facing flags by element type, own axes unit and square to each other,
//!   rotated sprites' axes fixed over their life, atlas cells inside the
//!   sheet, and each sprite within reach of its effect's origin (spawn
//!   offsets, velocity graphs, gravity, children);
//! * [`overlay`]: the sprites seen from the map's spawn points, each quad
//!   built as the effect vertex program builds it and clipped to a small
//!   screen, giving the screen share each covers: no plain-alpha sprite
//!   may fill a large part of the view from a spawn, and no own-axes sprite
//!   may stand taller than its def lets a flat one be.

use std::collections::BTreeMap;

use crate::fx::{self, Elem, FxScene, Sprite, elem_type};

/// A sprite record's words.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub centre: [f32; 3],
    pub rgba: [u8; 4],
    pub atlas: u32,
    pub angle_fade: u32,
    pub emissive: f32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub pivot: [f32; 2],
    pub half: [f32; 2],
    pub alpha_mode: u16,
    pub soft: u16,
    pub flags: u16,
    pub rotation: u16,
}

fn half_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from(h >> 10 & 0x1f);
    let m = f32::from(h & 0x3ff);
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => {
            if m == 0.0 {
                s * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// A unorm 10:10:10 axis as the program unpacks it.
pub fn unpack_axis(v: u32) -> [f32; 3] {
    core::array::from_fn(|k| (v >> (10 * k) & 1023) as f32 / 1023.0 * 2.00196 - 1.00196)
}

pub fn decode(r: &[u8; fx::SPRITE_RECORD]) -> Record {
    let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let u = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let h = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
    Record {
        centre: [f(0), f(4), f(8)],
        rgba: [r[16], r[17], r[18], r[19]],
        atlas: u(0x14),
        angle_fade: u(0x18),
        emissive: half_to_f32(h(0x1c)),
        a: unpack_axis(u(0x20)),
        b: unpack_axis(u(0x24)),
        pivot: [f(0x28), f(0x2c)],
        half: [f(0x30), f(0x34)],
        alpha_mode: h(0x38),
        soft: h(0x3a),
        flags: h(0x3c),
        rotation: h(0x3e),
    }
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    b.get(o..o + 4).map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}

/// The range a visual channel (offset into IW4's 48-byte sample) takes over
/// the life: each sample's `base + r x amplitude` for r in 0 .. 1, and the
/// lerp between samples stays inside them.
fn channel_range(e: &Elem, off: usize) -> (f32, f32) {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for s in e.vis.chunks_exact(48) {
        let (b, a) = (f32_at(s, off), f32_at(s, off + 24));
        lo = lo.min(b.min(b + a));
        hi = hi.max(b.max(b + a));
    }
    (lo, hi)
}

fn colour_range(e: &Elem, c: usize) -> (u8, u8) {
    let (mut lo, mut hi) = (255u8, 0u8);
    for s in e.vis.chunks_exact(48) {
        lo = lo.min(s[c]).min(s[24 + c]);
        hi = hi.max(s[c]).max(s[24 + c]);
    }
    (lo, hi)
}

fn vmax(v: &[fx_iw4::FxElemVec3Range]) -> f32 {
    v.iter()
        .map(|r| (0..3).map(|a| r.base[a].abs().max((r.base[a] + r.amplitude[a]).abs()).powi(2)).sum::<f32>().sqrt())
        .fold(0.0, f32::max)
}

/// How far from its effect's origin any sprite of an element can be.
pub(crate) fn elem_reach(e: &Elem) -> f32 {
    let r = |x: [f32; 2]| x[0].abs().max((x[0] + x[1]).abs());
    let spawn = (0..3).map(|a| r(e.spawn_origin[a]).powi(2)).sum::<f32>().sqrt() + r(e.offset_radius) + r(e.offset_height);
    let life = (e.life.0 + e.life.1.max(0)).max(1) as f32;
    let mut travel = 0.0;
    for (on, v) in [(e.flags & 0x0100_0000 != 0, &e.vel_local), (e.flags & 0x0200_0000 != 0, &e.vel_world)] {
        if on && v.len() >= 2 {
            travel += vmax(v) * life * (v.len() - 1) as f32;
        }
    }
    let secs = life * 0.001;
    let g = r(e.gravity) * 800.0 * 0.5 * secs * secs;
    // (A tail's centre sits size1 behind its head.)
    let tail = if matches!(e.ty, elem_type::TAIL | elem_type::LINE) { channel_range(e, 16).1.abs().max(channel_range(e, 16).0.abs()) } else { 0.0 };
    spawn + travel + g + tail + e.camera_offset
}

fn def_reach(s: &FxScene, d: usize, depth: u8, memo: &mut BTreeMap<usize, f32>) -> f32 {
    if let Some(r) = memo.get(&d) {
        return *r;
    }
    let mut best = 0.0f32;
    for e in &s.defs[d].elems {
        let own = elem_reach(e);
        let mut child = 0.0f32;
        if depth < 4 {
            for c in e.runs.iter().flatten().chain(e.death.iter()) {
                child = child.max(def_reach(s, *c, depth + 1, memo));
            }
        }
        best = best.max(own + child);
    }
    memo.insert(d, best);
    best
}

/// What a check found.
#[derive(Default, Debug)]
pub struct Report {
    pub effects: usize,
    pub sprites: usize,
    pub by_type: BTreeMap<u8, usize>,
    /// (rule, effect: element) -> count, with one example.
    pub failures: BTreeMap<(String, String), (usize, String)>,
    pub max_half: f32,
    pub lines: Vec<String>,
}

impl Report {
    fn fail(&mut self, rule: &str, who: String, example: String) {
        let e = self.failures.entry((rule.to_owned(), who)).or_insert((0, example));
        e.0 += 1;
    }
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
    pub fn summary(&self) -> Vec<String> {
        let mut out = self.lines.clone();
        out.push(format!("fx check: {} placed effects, {} sprites checked (by element type {:?}), largest half size {:.1}, {} rule failures", self.effects, self.sprites, self.by_type, self.max_half, self.failures.values().map(|v| v.0).sum::<usize>()));
        for ((rule, who), (n, ex)) in self.failures.iter().take(40) {
            out.push(format!("  FAIL {rule}: {who}: {n} sprites, e.g. {ex}"));
        }
        out
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Plays every placed effect at `times` (ms on the frame clock), seen from
/// 300 units above its origin, and holds each sprite against its
/// element's rules.
pub fn sim(s: &FxScene, times: &[i32]) -> Report {
    let mut rep = Report { effects: s.placed.len(), ..Default::default() };
    let mut memo = BTreeMap::new();
    let mut out: Vec<Sprite> = Vec::new();
    // Rotated sprites' axes per (placement, def, elem, material, rounded
    // spawn place): they must not turn.
    for (pi, p) in s.placed.iter().enumerate() {
        let reach = def_reach(s, p.def, 0, &mut memo);
        let top = s.defs[p.def].name.clone();
        let eye = [p.origin[0] + 37.0, p.origin[1] - 21.0, p.origin[2] + 300.0];
        let mut rotated: BTreeMap<(u32, u16, usize), Vec<([f32; 3], [f32; 3])>> = BTreeMap::new();
        for &t in times {
            s.placed_sprites(pi, t, eye, &mut out);
            for sp in &out {
                rep.sprites += 1;
                let def = &s.defs[sp.def as usize];
                let e = &def.elems[sp.elem as usize];
                *rep.by_type.entry(e.ty).or_default() += 1;
                let who = if def.name == top { format!("{} elem {} (type {})", def.name, sp.elem, e.ty) } else { format!("{} elem {} (type {}) under {}", def.name, sp.elem, e.ty, top) };
                let r = decode(&sp.record);
                let ex = format!("at {t} ms: {r:?}");
                // Finite words.
                let words = [r.centre[0], r.centre[1], r.centre[2], r.pivot[0], r.pivot[1], r.half[0], r.half[1], r.emissive, sp.dist];
                if words.iter().any(|v| !v.is_finite()) {
                    rep.fail("non-finite word", who.clone(), ex.clone());
                    continue;
                }
                rep.max_half = rep.max_half.max(r.half[0].abs()).max(r.half[1].abs());
                // Sizes and colours inside the visual graphs.
                let tail = matches!(e.ty, elem_type::TAIL | elem_type::LINE);
                let (lo0, hi0) = channel_range(e, 12);
                let (lo1, hi1) = if e.flags & 0x1000_0000 != 0 || tail { channel_range(e, 16) } else { (lo0, hi0) };
                let slack = |v: f32| 1e-3 + v.abs() * 1e-4;
                if r.half[0] < lo0 - slack(lo0) || r.half[0] > hi0 + slack(hi0) || r.half[1] < lo1 - slack(lo1) || r.half[1] > hi1 + slack(hi1) {
                    rep.fail("size outside the def's size graphs", who.clone(), format!("{ex} graphs {lo0}..{hi0}, {lo1}..{hi1}"));
                }
                for c in 0..3 {
                    let (lo, hi) = colour_range(e, c);
                    if r.rgba[c] < lo || r.rgba[c] > hi {
                        rep.fail("colour outside the def's colour graph", who.clone(), format!("{ex} channel {c} {lo}..{hi}"));
                    }
                }
                if r.rgba[3] > colour_range(e, 3).1 {
                    rep.fail("alpha above the def's alpha graph", who.clone(), ex.clone());
                }
                if r.emissive < 0.0 {
                    rep.fail("negative emissive", who.clone(), ex.clone());
                }
                // The def's draw words (`FUN_141573090`).
                let alpha_mode = (256.0 / e.alpha_scale.max(1e-4)).min(65535.0) as u16;
                let soft = (e.soft * 16.0).clamp(0.0, 65535.0) as u16;
                if r.alpha_mode != alpha_mode || r.soft != soft || r.angle_fade != e.angle_fade {
                    rep.fail("draw words not the def's (alpha mode, soft depth, view-angle fade)", who.clone(), format!("{ex} want {alpha_mode} {soft} {:#x}", e.angle_fade));
                }
                if !tail && r.pivot != e.pivot {
                    rep.fail("pivot not the def's", who.clone(), ex.clone());
                }
                // Facing flags by type (the exe's draw table 0x142ec75b0).
                let want_flags = match e.ty {
                    elem_type::ORIENTED | elem_type::ROTATED | elem_type::TAIL | elem_type::LINE => 4,
                    _ => u16::from(e.flags & 0x10_0000 != 0) | if e.flags & 0x8_0000 != 0 { 2 } else { 0 },
                };
                if r.flags != want_flags {
                    rep.fail("facing flags not the element type's", who.clone(), format!("{ex} want {want_flags}"));
                }
                if r.flags & 4 != 0 {
                    // Own axes: unit (10-bit), square to each other.
                    if (len(r.a) - 1.0).abs() > 0.01 || (len(r.b) - 1.0).abs() > 0.01 || dot(r.a, r.b).abs() > 0.02 {
                        rep.fail("own axes not unit and square", who.clone(), ex.clone());
                    }
                    if e.ty == elem_type::ROTATED {
                        rotated.entry((sp.def, sp.elem, sp.material)).or_default().push((r.a, r.b));
                    }
                    if tail {
                        // The tail's width runs across the eye's line.
                        let to_eye = [eye[0] - r.centre[0], eye[1] - r.centre[1], eye[2] - r.centre[2]];
                        if len(to_eye) > 1.0 && (dot(r.b, to_eye) / len(to_eye)).abs() > 0.05 {
                            rep.fail("tail's width not across the view", who.clone(), ex.clone());
                        }
                    }
                } else if len(r.a) > 0.01 || len(r.b) > 0.01 {
                    // (Billboards pass no axes: the words are 0.)
                    rep.fail("billboard with axes", who.clone(), ex.clone());
                }
                // Atlas cells inside the sheet (`FUN_14156fce0`).
                if r.atlas != 0 {
                    let (col, row) = (r.atlas >> 24 & 0xf, r.atlas >> 28);
                    let cells = 1u32 << (col + row);
                    if e.atlas_t7[0] & 0x80 == 0 || col != u32::from(e.atlas_t7[4] & 0xf) || row != u32::from(e.atlas_t7[5]) || (r.atlas & 0xff) >= cells || (r.atlas >> 8 & 0xff) >= cells {
                        rep.fail("atlas word outside the def's sheet", who.clone(), ex.clone());
                    }
                } else if e.atlas_t7[0] & 0x80 != 0 && (e.atlas_t7[4] | e.atlas_t7[5]) != 0 && e.atlas_t7[0] & 0x20 == 0 {
                    // (Enabled with cells: cell 0 of a sheet still names the sheet.)
                    rep.fail("atlas enabled but the record draws the whole sheet", who.clone(), ex.clone());
                }
                // Within reach of the effect's origin.
                let off = [r.centre[0] - p.origin[0], r.centre[1] - p.origin[1], r.centre[2] - p.origin[2]];
                if len(off) > reach * 1.01 + 1.0 {
                    rep.fail("sprite beyond its effect's reach", who.clone(), format!("{ex} {:.1} > reach {reach:.1}", len(off)));
                }
            }
        }
        // Rotated sprites keep the spawn angles' axes: at most a few distinct
        // facings per element (one per particle), never one that changes
        // with time for the same particle. Checked as: every facing seen is
        // a unit quad normal, and an element whose spawn angles have no
        // random range shows one facing only.
        for ((d, ei, _), ax) in rotated {
            let e = &s.defs[d as usize].elems[ei as usize];
            let fixed = e.spawn_angles.iter().all(|a| a[1] == 0.0);
            if fixed {
                let n0 = cross(ax[0].0, ax[0].1);
                for (a, b) in &ax {
                    let n = cross(*a, *b);
                    if dot(n, n0) < 0.99 {
                        rep.fail("rotated sprite turns (its axes are its spawn angles')", format!("{} elem {ei}", s.defs[d as usize].name), format!("{n:?} vs {n0:?}"));
                        break;
                    }
                }
            }
        }
    }
    rep
}

/// A view: eye, yaw and pitch (degrees, IW: pitch down positive).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// The map's spawn points as views (`mp_*spawn*` entities, eye 60 up).
pub fn spawn_views(entities: &str) -> Vec<View> {
    let mut out = Vec::new();
    for block in entities.split('{') {
        let value = |key: &str| {
            let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
            let rest = &block[at..];
            Some(&rest[..rest.find('"')?])
        };
        let Some(c) = value("classname") else { continue };
        if !(c.starts_with("mp_") && c.contains("spawn")) {
            continue;
        }
        let v3 = |s: &str| -> Option<[f32; 3]> {
            let v: Vec<f32> = s.split_whitespace().filter_map(|w| w.parse().ok()).collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        };
        let Some(o) = value("origin").and_then(v3) else { continue };
        let ang = value("angles").and_then(v3).unwrap_or_default();
        out.push(View { eye: [o[0], o[1], o[2] + 60.0], yaw: ang[1], pitch: 0.0 });
    }
    out
}

/// One view's overlay: how much of a small screen the sprites cover.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    /// The largest share of the screen one plain-alpha sprite (alpha mode
    /// 256) covers, at its alpha, and which.
    pub max_plain: f32,
    pub max_plain_who: String,
    /// The sum over sprites of share x alpha (how much the effects lay over
    /// the view).
    pub alpha_cover: f32,
    pub sprites: usize,
    pub on_screen: usize,
    /// Own-axes sprites standing up (quad normal within 30 degrees of
    /// level) taller than 200 units.
    pub tall_upright: Vec<String>,
}

/// The quad's four corners as the effect vertex program builds them
/// (`fx_draw`'s `spriteExtra` builder): billboards from the view's up and
/// right (flag 1: facing the eye; 2: z-up), own axes (4) from the record.
pub fn quad(r: &Record, eye: glam::Vec3, right: glam::Vec3, up: glam::Vec3, fwd: glam::Vec3) -> [glam::Vec3; 4] {
    let centre = glam::Vec3::from(r.centre);
    let d = (eye - centre).normalize_or_zero();
    let side = if r.flags & 1 != 0 { up.cross(d).normalize_or_zero() } else { right };
    let flat = if r.flags & 1 != 0 { glam::Vec2::new(d.x, d.y) } else { -glam::Vec2::new(fwd.x, fwd.y) }.normalize_or_zero();
    let (mut a, mut b) = if r.flags & 2 != 0 { (glam::Vec3::Z, glam::Vec3::new(-flat.y, flat.x, 0.0)) } else { (up, side) };
    if r.flags & 4 != 0 {
        a = glam::Vec3::from(r.a);
        b = glam::Vec3::from(r.b);
    }
    let rot = f32::from(r.rotation) * (std::f32::consts::TAU / 65536.0);
    let (sn, cs) = rot.sin_cos();
    let uh = (a * sn + b * cs) * r.half[0];
    let vh = (b * sn - a * cs) * r.half[1];
    let c = centre + uh * r.pivot[0] + vh * r.pivot[1];
    [c - uh - vh, c + uh - vh, c + uh + vh, c - uh + vh]
}

fn clip(poly: Vec<glam::Vec3>, keep: impl Fn(glam::Vec3) -> f32) -> Vec<glam::Vec3> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (dp, dq) = (keep(p), keep(q));
        if dp >= 0.0 {
            out.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            out.push(p + (q - p) * (dp / (dp - dq)));
        }
    }
    out
}

/// The share (0 .. 1) of a 16:9 screen with a 65 degree horizontal field
/// of view a world quad covers.
pub fn screen_share(corners: &[glam::Vec3; 4], eye: glam::Vec3, right: glam::Vec3, up: glam::Vec3, fwd: glam::Vec3) -> f32 {
    let tan_x = (65f32.to_radians() * 0.5).tan();
    let tan_y = tan_x * 9.0 / 16.0;
    // View space: x right, y up, z forward.
    let v: Vec<glam::Vec3> = corners.iter().map(|c| {
        let d = *c - eye;
        glam::Vec3::new(d.dot(right), d.dot(up), d.dot(fwd))
    }).collect();
    let mut poly = clip(v, |p| p.z - 1.0);
    for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        if poly.len() < 3 {
            return 0.0;
        }
        // (Inside: x <= z tan_x etc.)
        poly = clip(poly, |p| if sx != 0.0 { p.z * tan_x - sx * p.x } else { p.z * tan_y - sy * p.y });
    }
    if poly.len() < 3 {
        return 0.0;
    }
    let pts: Vec<glam::Vec2> = poly.iter().map(|p| glam::Vec2::new(p.x / (p.z * tan_x), p.y / (p.z * tan_y))).collect();
    let mut area = 0.0;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        area += a.x * b.y - b.x * a.y;
    }
    (area.abs() * 0.5 / 4.0).min(1.0)
}

/// The effects seen from `view` at `now_ms`.
pub fn overlay(s: &FxScene, view: View, now_ms: i32) -> Overlay {
    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let fwd = glam::Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), -pitch.sin());
    let right = fwd.cross(glam::Vec3::Z).normalize();
    let up = right.cross(fwd);
    let eye = glam::Vec3::from(view.eye);
    let mut sprites = Vec::new();
    s.sprites(now_ms, view.eye, &mut sprites);
    let mut o = Overlay { sprites: sprites.len(), ..Default::default() };
    for sp in &sprites {
        let r = decode(&sp.record);
        let q = quad(&r, eye, right, up, fwd);
        let share = screen_share(&q, eye, right, up, fwd);
        let e = &s.defs[sp.def as usize].elems[sp.elem as usize];
        if r.flags & 4 != 0 {
            let n = (q[1] - q[0]).cross(q[3] - q[0]).normalize_or_zero();
            let tall = q.iter().map(|c| c.z).fold(f32::NEG_INFINITY, f32::max) - q.iter().map(|c| c.z).fold(f32::INFINITY, f32::min);
            if n.z.abs() < 0.5 && tall > 200.0 && e.ty != elem_type::TAIL && e.ty != elem_type::LINE {
                o.tall_upright.push(format!("{} elem {} (type {}) {tall:.0} tall at {:?}", s.defs[sp.def as usize].name, sp.elem, e.ty, r.centre));
            }
        }
        if share <= 0.0 {
            continue;
        }
        o.on_screen += 1;
        let alpha = f32::from(r.rgba[3]) / 255.0;
        o.alpha_cover += share * alpha;
        if r.alpha_mode == 256 && share * alpha > o.max_plain {
            o.max_plain = share * alpha;
            o.max_plain_who = format!("{} elem {} (type {}, half {:?}, {:.0} away)", s.defs[sp.def as usize].name, sp.elem, e.ty, r.half, sp.dist);
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(name: &str) -> Option<FxScene> {
        let zone = std::path::PathBuf::from(format!("/mnt/user-data/uploads/bo3_game_files/zone/{name}.ff"));
        let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).ok()?;
        let mut names: Vec<String> = Vec::new();
        let mut s = FxScene::build(&map, &mut |n| {
            Some(names.iter().position(|m| m == n).unwrap_or_else(|| {
                names.push(n.to_owned());
                names.len() - 1
            }))
        });
        s.report.push(format!("spawn views: {}", spawn_views(&map.entities).len()));
        Some(s)
    }

    /// Every staged map's placed effects stepped and checked:
    /// `cargo test -p render_t7 --lib fx_check -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn staged_maps_effects_follow_the_exes_rules() {
        let mut ran = 0;
        let mut bad = Vec::new();
        for name in ["mp_nuketown_x", "mp_apartments", "mp_sector", "mp_skyjacked", "mp_metro", "mp_western"] {
            let Some(s) = staged(name) else {
                println!("{name}: not staged");
                continue;
            };
            ran += 1;
            let times: Vec<i32> = (0..24).map(|k| k * 1250 + 17).collect();
            let rep = sim(&s, &times);
            println!("== {name}");
            for l in s.report.iter().filter(|l| l.contains("placed effects (")) {
                println!("{l}");
            }
            for l in rep.summary() {
                println!("{l}");
            }
            if !rep.ok() {
                bad.push(name);
            }
        }
        assert!(ran > 0, "no staged map");
        assert!(bad.is_empty(), "rule failures on {bad:?}");
    }

    /// Nuk3town from its spawn points (the owner's regressions: upright
    /// cyan pod strips, large solid blocks): no own-axes sprite stands up
    /// tall, no plain sprite fills much of a view.
    #[test]
    #[ignore]
    fn nuketown_overlay_from_its_spawns() {
        let Some(s) = staged("mp_nuketown_x") else { return };
        let zone = std::path::PathBuf::from("/mnt/user-data/uploads/bo3_game_files/zone/mp_nuketown_x.ff");
        let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).unwrap();
        let views = spawn_views(&map.entities);
        assert!(!views.is_empty());
        let (mut worst, mut worst_who, mut cover_max, mut upright) = (0.0f32, String::new(), 0.0f32, Vec::new());
        let mut cover_sum = 0.0;
        for (k, v) in views.iter().enumerate() {
            for t in [3_000, 11_000, 27_000] {
                let o = overlay(&s, *v, t + k as i32 * 7);
                if o.max_plain > worst {
                    worst = o.max_plain;
                    worst_who = o.max_plain_who.clone();
                }
                cover_max = cover_max.max(o.alpha_cover);
                cover_sum += o.alpha_cover;
                upright.extend(o.tall_upright);
            }
        }
        upright.sort();
        upright.dedup();
        println!("nuketown overlay: {} spawn views x 3 times: largest plain sprite {:.3} of the screen ({worst_who}); alpha cover max {cover_max:.3}, mean {:.3}; {} tall upright own-axes sprites", views.len(), worst, cover_sum / (views.len() * 3) as f32, upright.len());
        for u in upright.iter().take(10) {
            println!("  upright: {u}");
        }
        assert!(upright.is_empty(), "own-axes sprites standing up");
        // (A plain-alpha sprite may well be large near a spawn, as the game's
        // own are; one filling half the view from a spawn is the block
        // artefact: a size, atlas or facing rule broken.)
        assert!(worst < 0.5, "a plain sprite covers {worst} of a spawn view: {worst_who}");
    }

    #[test]
    fn a_flat_rotated_strip_reads_flat_and_a_quad_covers_its_share() {
        // A record lying flat: axes x and y, 100 x 10 half sizes.
        let mut r = Record { centre: [0.0, 0.0, 0.0], rgba: [255; 4], atlas: 0, angle_fade: 0, emissive: 0.0, a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], pivot: [0.0; 2], half: [100.0, 10.0], alpha_mode: 256, soft: 0, flags: 4, rotation: 0 };
        let (eye, fwd) = (glam::Vec3::new(-500.0, 0.0, 0.0), glam::Vec3::X);
        let (right, up) = (fwd.cross(glam::Vec3::Z).normalize(), glam::Vec3::Z);
        let q = quad(&r, eye, right, up, fwd);
        assert!(q.iter().all(|c| c.z.abs() < 1e-4));
        // Seen edge-on: no screen share.
        assert!(screen_share(&q, eye, right, up, fwd) < 1e-4);
        // A billboard filling the view's height at 10 units covers it.
        r.flags = 0;
        r.half = [1000.0, 1000.0];
        r.centre = [-490.0, 0.0, 0.0];
        let q = quad(&r, eye, right, up, fwd);
        assert!((screen_share(&q, eye, right, up, fwd) - 1.0).abs() < 1e-3);
        assert!((half_to_f32(0x3c00) - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// `T7_FX_DUMP=<map>:<effect substring>`: the matching defs' elements.
    #[test]
    #[ignore]
    fn dump_effect_defs() {
        let Ok(arg) = std::env::var("T7_FX_DUMP") else { return };
        let (map, want) = arg.split_once(':').unwrap();
        let zone = std::path::PathBuf::from(format!("/mnt/user-data/uploads/bo3_game_files/zone/{map}.ff"));
        let m = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).unwrap();
        let mut names: Vec<String> = Vec::new();
        let s = FxScene::build(&m, &mut |n| {
            Some(names.iter().position(|x| x == n).unwrap_or_else(|| {
                names.push(n.to_owned());
                names.len() - 1
            }))
        });
        for (di, d) in s.defs.iter().enumerate().filter(|(_, d)| d.name.contains(want)) {
            let placed = s.placed.iter().filter(|p| p.def == di).count();
            println!("{} (placed {placed}, looping {}, one-shot {}, looping life {}, duration {})", d.name, d.looping, d.one_shot, d.msec_looping_life, d.duration);
            for (ei, e) in d.elems.iter().enumerate() {
                println!(
                    "  elem {ei}: type {} flags {:#x} spawn {:?} life {:?} delay {:?} range {:?} fade out {:?} in {:?} sizes {:?} {:?} alpha {:?} alpha scale {} soft {} pivot {:?} angle fade {:#x} atlas {:02x?} cam {} cyl {} intervals {} mats {:?} angles {:?} vel n {}",
                    e.ty, e.flags, e.spawn, e.life, e.delay, e.spawn_range, e.fade_out, e.fade_in, channel_range(e, 12), channel_range(e, 16), colour_range(e, 3), e.alpha_scale, e.soft, e.pivot, e.angle_fade, e.atlas_t7, e.camera_offset, e.cylinder_axis, e.vis_intervals,
                    e.materials.iter().map(|m| m.and_then(|m| names.get(m).cloned())).collect::<Vec<_>>(), e.spawn_angles, e.vel_world.len()
                );
            }
        }
    }
}

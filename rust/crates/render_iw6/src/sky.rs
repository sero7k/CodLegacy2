//! The map's sky: which picture its sky material samples, whether that
//! picture can be read here, and what is drawn when it cannot.
//!
//! Ghosts streams a map's sky cube from an `imagefile<N>.pak` (mp_frag:
//! `sky_mp_frag_hdr`, BC6H 1024, pak 60). Where the pak is not beside the map
//! the sky is a stand-in built from the map's own look: the fog colour at the
//! horizon (what far geometry fogs to, so the two meet), a little darker
//! overhead, and a halo round the sun when the map's sun is bright. The old
//! stand-in, the map's global reflection probe 0, is a flat 1.0 (white), which
//! is wrong on any map that is not an overcast white day.
//!
//! Everything here is pure (no files, no GPU) and unit-tested.

use fastfile_h1::image::ImagePixels;

/// A sky picture as the zone names it and as the staged paks allow reading it.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyImage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub dxgi: u32,
    /// Every pak the picture's parts live in.
    pub needed_paks: Vec<u32>,
    /// Those of them that are not there.
    pub missing_paks: Vec<u32>,
    /// A part that fits the size cap can be read (`scene::streamed_cube` takes
    /// the largest such part), or the picture is inline.
    pub readable: bool,
}

impl SkyImage {
    pub fn from_rec(rec: &fastfile_iw6::walk::ImageRec, has_pak: impl Fn(u32) -> bool, cap: u32) -> Self {
        let mut needed = Vec::new();
        // (Unused part slots hold a zero size and file index 0.)
        for (k, f) in rec.stream_files.iter().enumerate() {
            let Some(f) = f else { continue };
            if rec.streams[k].0 == 0 {
                continue;
            }
            let n = u32::from(f.file_index);
            if !needed.contains(&n) {
                needed.push(n);
            }
        }
        let missing = needed.iter().copied().filter(|&n| !has_pak(n)).collect();
        let readable = !rec.streamed
            || (0..4).any(|k| {
                let (w, h, _, _) = rec.streams[k];
                rec.stream_files[k].is_some_and(|f| has_pak(u32::from(f.file_index)))
                    && w > 0
                    && u32::from(w.max(h)) <= cap
            });
        let top = rec.streams.iter().max_by_key(|s| s.0.max(s.1)).copied().unwrap_or_default();
        let (width, height) = if rec.streamed { (u32::from(top.0), u32::from(top.1)) } else { (u32::from(rec.width), u32::from(rec.height)) };
        Self { name: rec.name.clone(), width, height, dxgi: rec.format, needed_paks: needed, missing_paks: missing, readable }
    }

    /// Every part is there.
    pub fn complete(&self) -> bool {
        self.missing_paks.is_empty()
    }
}

/// What the map's sky comes to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkyPlan {
    /// The pictures the sky draws with: those the sky materials sample, else
    /// the `GfxSky` pictures; no repeats.
    pub drawn: Vec<SkyImage>,
    /// Names whose picture cannot be read: they get the stand-in.
    pub stand_in_for: Vec<String>,
    pub notes: Vec<String>,
}

impl SkyPlan {
    /// The first sky picture is not readable (the stand-in is what shows).
    pub fn needs_stand_in(&self) -> bool {
        !self.stand_in_for.is_empty()
    }
}

/// `gfx`: the `GfxSky` pictures; `material`: the cube pictures the sky surfaces'
/// materials sample (what the shader reads: it is the material's picture, not
/// the `GfxSky` one, that is bound).
pub fn plan(map: &str, gfx: &[SkyImage], material: &[SkyImage]) -> SkyPlan {
    let mut out = SkyPlan::default();
    let push = |v: &mut Vec<SkyImage>, i: &SkyImage| {
        if !v.iter().any(|d| d.name.eq_ignore_ascii_case(&i.name)) {
            v.push(i.clone());
        }
    };
    let mut drawn = Vec::new();
    for i in if material.is_empty() { gfx } else { material } {
        push(&mut drawn, i);
    }
    if drawn.is_empty() {
        out.notes.push(format!("iw6 sky: map {map} has no sky picture (no sky surface, no GfxSky image): nothing to draw"));
        return out;
    }
    if !material.is_empty() {
        for g in gfx {
            if !material.iter().any(|m| m.name.eq_ignore_ascii_case(&g.name)) {
                out.notes.push(format!(
                    "iw6 sky: the map's sky list names {} but its sky material samples {}: the material's picture is drawn",
                    g.name,
                    material.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    out.drawn = drawn.clone();
    for i in &drawn {
        if i.readable && i.complete() {
            out.notes.push(format!("iw6 sky: {} ({}x{}, DXGI {}) from its pak", i.name, i.width, i.height, i.dxgi));
        } else if i.readable {
            out.notes.push(format!(
                "iw6 sky: {} ({}x{}, DXGI {}) from its pak at a reduced size (pak {} not here)",
                i.name,
                i.width,
                i.height,
                i.dxgi,
                paks(&i.missing_paks)
            ));
        } else {
            out.notes.push(format!("iw6 sky: {} NOT RESOLVED (map {map} needs pak {})", i.name, paks(&i.missing_paks)));
            out.stand_in_for.push(i.name.clone());
        }
    }
    out
}

fn paks(v: &[u32]) -> String {
    v.iter().map(|n| format!("imagefile{n}.pak")).collect::<Vec<_>>().join(", ")
}

/// Whether a material is a sky material: its technique set names the sky
/// (`wc_sky_bl`) or the material is called `sky_...`.
pub fn is_sky_material(material: &str, techset: &str) -> bool {
    let base = material.rsplit('/').next().unwrap_or(material).to_ascii_lowercase();
    base.starts_with("sky_")
        || techset.to_ascii_lowercase().trim_start_matches(',').split('_').any(|p| p == "sky")
}

/// What the stand-in sky is made of (all linear, in the units the scene is lit in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandIn {
    /// The colour at the horizon: the fog colour. `None`: the map has no fog
    /// block; a neutral dim sky is used instead.
    pub horizon: Option<[f32; 3]>,
    /// Toward the sun.
    pub sun_dir: [f32; 3],
    pub sun_color: [f32; 3],
}

/// Without a fog block there is no colour to take; a dim neutral blue-grey
/// (INFERRED, only ever the last resort).
pub const NEUTRAL_HORIZON: [f32; 3] = [0.18, 0.21, 0.27];
/// How much darker the zenith is than the horizon (INFERRED).
pub const ZENITH_SCALE: f32 = 0.7;
/// Below the horizon (mostly hidden by the world).
pub const GROUND_SCALE: f32 = 0.8;
/// A sun this bright (largest colour component) has the full halo.
pub const FULL_HALO_AT: f32 = 4.0;

impl StandIn {
    pub fn horizon_color(&self) -> [f32; 3] {
        self.horizon.unwrap_or(NEUTRAL_HORIZON)
    }

    /// The sky's colour looking along `d` (unit, world, z up).
    pub fn color(&self, d: [f32; 3]) -> [f32; 3] {
        let h = self.horizon_color();
        let e = d[2];
        let scale = if e >= 0.0 {
            1.0 + (ZENITH_SCALE - 1.0) * e.sqrt()
        } else {
            1.0 + (GROUND_SCALE - 1.0) * (-e / 0.3).min(1.0)
        };
        let mut c = [h[0] * scale, h[1] * scale, h[2] * scale];
        let m = self.sun_color.iter().cloned().fold(0.0, f32::max);
        if m > 0.0 {
            let strength = (m / FULL_HALO_AT).clamp(0.0, 1.0);
            let lum = 0.2126 * h[0] + 0.7152 * h[1] + 0.0722 * h[2];
            let cs = (d[0] * self.sun_dir[0] + d[1] * self.sun_dir[1] + d[2] * self.sun_dir[2]).max(0.0);
            let halo = strength * lum * (0.6 * cs.powi(16) + 4.0 * cs.powi(600));
            for k in 0..3 {
                c[k] += self.sun_color[k] / m * halo;
            }
        }
        c
    }
}

/// D3D's cube face texel -> direction (face order +X -X +Y -Y +Z -Z, `u`
/// right and `v` down in -1..1), with the world axes as they are (z up), the
/// way the probes are sampled (`docs/IW6.md`, "Reflection probes and the sky").
pub fn face_dir(face: usize, u: f32, v: f32) -> [f32; 3] {
    let d = match face {
        0 => [1.0, -v, -u],
        1 => [-1.0, -v, u],
        2 => [u, 1.0, v],
        3 => [u, -1.0, -v],
        4 => [u, -v, 1.0],
        _ => [-u, -v, -1.0],
    };
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    [d[0] / n, d[1] / n, d[2] / n]
}

/// `f32` to IEEE half bits (round to nearest, overflow to infinity, tiny to 0).
pub fn f16_bits(f: f32) -> u16 {
    let b = f.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x007f_ffff;
    if ((b >> 23) & 0xff) == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x0080_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let r = ((exp as u32) << 10 | man >> 13) + ((man >> 12) & 1);
    sign | r as u16
}

/// The stand-in cube: `size` x `size` per face, RGBA16F, one level, six faces
/// one after another.
pub fn stand_in_cube(s: &StandIn, size: u32) -> ImagePixels {
    let mut data = Vec::with_capacity(6 * (size * size * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let c = s.color(face_dir(face, u, v));
                for h in [c[0], c[1], c[2], 1.0] {
                    data.extend_from_slice(&f16_bits(h).to_le_bytes());
                }
            }
        }
    }
    ImagePixels { width: size, height: size, levels: 1, dxgi_format: 10, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(name: &str, needed: &[u32], missing: &[u32], readable: bool) -> SkyImage {
        SkyImage { name: name.into(), width: 1024, height: 1024, dxgi: 95, needed_paks: needed.into(), missing_paks: missing.into(), readable }
    }

    #[test]
    fn a_missing_pak_is_said_and_gets_the_stand_in() {
        let p = plan("mp_frag", &[img("sky_mp_frag_hdr", &[60], &[60], false)], &[img("sky_mp_frag_hdr", &[60], &[60], false)]);
        assert_eq!(p.stand_in_for, ["sky_mp_frag_hdr"]);
        assert_eq!(p.notes, ["iw6 sky: sky_mp_frag_hdr NOT RESOLVED (map mp_frag needs pak imagefile60.pak)"]);
        assert!(p.needs_stand_in());
    }

    #[test]
    fn unused_part_slots_name_no_pak() {
        let mut rec = fastfile_iw6::walk::ImageRec { name: "sky_t".into(), format: 95, map_type: 5, streamed: true, ..Default::default() };
        let sf = |n| Some(fastfile_iw6::container::StreamFile { word0: 0, file_index: n, flags: 0, offset: 0, offset_end: 0 });
        rec.streams[0] = (1024, 1024, 6 << 20, 1);
        rec.stream_files = [sf(60), sf(0), sf(0), sf(0)];
        let i = SkyImage::from_rec(&rec, |n| n != 60, 2048);
        assert_eq!((i.needed_paks.clone(), i.missing_paks.clone(), i.readable), (vec![60], vec![60], false));
        assert_eq!((i.width, i.height), (1024, 1024));
        assert!(SkyImage::from_rec(&rec, |_| true, 2048).readable);
        // Larger than the size cap: not read.
        assert!(!SkyImage::from_rec(&rec, |_| true, 512).readable);
    }

    #[test]
    fn a_staged_pak_resolves() {
        let a = img("sky_x_hdr", &[60], &[], true);
        let p = plan("mp_x", &[a.clone()], &[a]);
        assert!(!p.needs_stand_in());
        assert_eq!(p.notes, ["iw6 sky: sky_x_hdr (1024x1024, DXGI 95) from its pak"]);
    }

    #[test]
    fn a_part_that_reads_while_another_is_missing_is_reduced() {
        let a = img("sky_x_hdr", &[60, 61], &[61], true);
        let p = plan("mp_x", &[], &[a]);
        assert!(!p.needs_stand_in());
        assert!(p.notes[0].contains("reduced size (pak imagefile61.pak not here)"), "{:?}", p.notes);
    }

    #[test]
    fn the_material_picture_wins_over_the_sky_list() {
        let list = img("sky_old", &[1], &[], true);
        let mat = img("sky_mp_x_hdr", &[60], &[60], false);
        let p = plan("mp_x", &[list], &[mat]);
        assert_eq!(p.drawn.len(), 1);
        assert_eq!(p.drawn[0].name, "sky_mp_x_hdr");
        assert_eq!(p.stand_in_for, ["sky_mp_x_hdr"]);
        assert!(p.notes[0].contains("names sky_old but its sky material samples sky_mp_x_hdr"));
    }

    #[test]
    fn the_sky_list_is_used_when_no_material_cube_is_found() {
        let p = plan("mp_x", &[img("sky_a", &[2], &[], true), img("SKY_A", &[2], &[], true)], &[]);
        assert_eq!(p.drawn.len(), 1);
        assert!(plan("mp_x", &[], &[]).notes[0].contains("no sky picture"));
    }

    #[test]
    fn sky_materials_are_found_by_technique_set_or_name() {
        assert!(is_sky_material("wc/sky_mp_frag", ",wc_sky_bl"));
        assert!(is_sky_material("wc/sky_mp_frag", ""));
        assert!(is_sky_material("wc/anything", ",wc_sky_bl"));
        assert!(!is_sky_material("wc/ag_cargocontainer_door_orange", ",wc_l_sm_r0c0n0s0l0_nfwpf"));
        assert!(!is_sky_material("wc/skylight_glass", ",wc_l_sm"));
    }

    #[test]
    fn faces_follow_the_probe_convention() {
        let up = face_dir(4, 0.0, 0.0);
        assert!((up[2] - 1.0).abs() < 1e-6);
        // The -Y face's top row looks up, the +Y face's down.
        assert!(face_dir(3, 0.0, -0.9)[2] > 0.0);
        assert!(face_dir(2, 0.0, -0.9)[2] < 0.0);
        // Face centres are the six axes.
        let want = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        for (f, w) in want.iter().enumerate() {
            let d = face_dir(f, 0.0, 0.0);
            assert!((0..3).all(|k| (d[k] - w[k]).abs() < 1e-6), "face {f}: {d:?}");
        }
    }

    #[test]
    fn half_floats_round_trip() {
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.0), 0);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        assert_eq!(f16_bits(1e9), 0x7c00);
        assert_eq!(f16_bits(1e-9), 0);
        assert_eq!(f16_bits(0.5), 0x3800);
    }

    #[test]
    fn the_stand_in_meets_the_fog_at_the_horizon_and_is_darker_overhead() {
        let s = StandIn { horizon: Some([0.4, 0.5, 0.7]), sun_dir: [0.0, 0.0, 1.0], sun_color: [0.0; 3] };
        assert_eq!(s.color([1.0, 0.0, 0.0]), [0.4, 0.5, 0.7]);
        let top = s.color([0.0, 0.0, 1.0]);
        assert!((top[2] - 0.7 * ZENITH_SCALE).abs() < 1e-6);
        assert!(s.color([0.0, 0.0, -1.0])[0] < 0.4);
    }

    #[test]
    fn a_dim_night_sky_stays_dark_and_a_bright_sun_adds_a_halo() {
        let night = StandIn { horizon: Some([0.01, 0.015, 0.03]), sun_dir: [1.0, 0.0, 0.2], sun_color: [0.0; 3] };
        assert!(night.color([0.0, 1.0, 0.5])[2] < 0.05);
        let day = StandIn { horizon: Some([0.4, 0.5, 0.7]), sun_dir: [0.0, 1.0, 0.0], sun_color: [8.0, 7.9, 7.5] };
        let near = day.color([0.0, 1.0, 0.0]);
        let far = day.color([1.0, 0.0, 0.0]);
        assert!(near[0] > 2.0 * far[0]);
        // No fog block: the neutral sky, not white.
        let none = StandIn { horizon: None, sun_dir: [0.0, 0.0, 1.0], sun_color: [0.0; 3] };
        assert_eq!(none.color([1.0, 0.0, 0.0]), NEUTRAL_HORIZON);
    }

    #[test]
    fn the_cube_has_six_faces_of_half_floats() {
        let s = StandIn { horizon: Some([0.4, 0.5, 0.7]), sun_dir: [0.0, 0.0, 1.0], sun_color: [0.0; 3] };
        let px = stand_in_cube(&s, 4);
        assert_eq!((px.width, px.height, px.levels, px.dxgi_format), (4, 4, 1, 10));
        assert_eq!(px.data.len(), 6 * 4 * 4 * 8);
        // Alpha of the first texel is 1.0.
        assert_eq!(u16::from_le_bytes([px.data[6], px.data[7]]), 0x3c00);
    }
}

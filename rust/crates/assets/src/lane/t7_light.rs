//! bo2mp: Black Ops III's own light for its maps (`fastfile_t7::light`):
//! the sky picture as the sky, the reflection probes (their cubes, in the
//! map-wide probe's unit) for the ambient light and the reflections, the
//! sun volume's sun (direction from its angles, brightness from its
//! intensity) with shadows from a sun map baked at load over the map's own
//! triangles, and a filmic tone curve in place of Black Ops II's
//! square-root output. The probe volume and the sun shadow tree are not
//! read: the ambient light is the probes' ambient cubes times an occlusion
//! traced at load (`fastfile_t7::bake`).
//!
//! What the renderer gets: the world's vertex colours hold each vertex's
//! ambient light, every lit world surface reads a blank lightmap page (so
//! it draws in the lightmapped pipelines, with its normal and specular maps
//! and its probe), the placed models' light rows their ambient light, the
//! colour-table slot the sun map, and light rows 254 and 255 the numbers
//! (`asset_world::T7_LIGHT_ROW`); the fallback pass's T7 branch draws them.
//!
//! A self-contained path: `BO2MP_T7_LIGHT=0` turns it off (the old flat
//! light). Other games' maps never come here; the branch could take any
//! map whose light reads into these same pieces.

use std::sync::Arc;

use asset_core::{AssetEdge, AssetNamespace, AssetRef, T6Draw, ZoneOwner};
use asset_material::{AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use fastfile_t7::bake::Tracer;
use fastfile_t7::light::{Bake, Blocks, SunMap};
use fastfile_t7::map::T7Map;

/// Whether Black Ops III maps get their own light (`BO2MP_T7_LIGHT=0`: the
/// old flat light).
pub(super) fn on() -> bool {
    std::env::var("BO2MP_T7_LIGHT").map_or(true, |v| v.trim() != "0")
}

/// bo2mp (load speed): whether the occlusion tracer is built for a map the
/// native renderer draws. Only the converted draw reads it (the world's
/// vertex colours and the placed models' light, both made only without the
/// native renderer), and on Skyjacked it is a tree over 6.4 million
/// triangles in the light stage the load waits on. `BO2MP_T7_NATIVE_TRACER=1`:
/// built anyway (the old way).
pub(super) fn native_tracer() -> bool {
    std::env::var("BO2MP_T7_NATIVE_TRACER").is_ok_and(|v| v.trim() == "1")
}

/// The renderer's probe table holds this many (probe 0 the map-wide one).
const MAX_PROBES: usize = 32;
/// The sky cube's face size (the 2048-wide picture's quarter).
const SKY_FACE: u32 = 512;
/// What Black Ops III's tone curve's input is: light in the probes' unit
/// times this (`BO2MP_T7_EXPOSURE`). BO3's own is its auto-exposure's
/// (histograms and exposure volumes: not drawn); this is chosen against
/// pictures of the game.
const EXPOSURE: f32 = 1.6;
/// The sun's light over the probes' unit is its intensity over probe 0's
/// scale (a guess at the unit); against pictures of the game (Redwood's
/// sunlit dirt and shade, with the probe volume's ambient) it reads this
/// much weak (`BO2MP_T7_SUN` sets it).
const SUN_SCALE: f32 = 1.5;
/// How far, round the spawn points' square, the sun map and the occlusion
/// reach.
const MARGIN: f32 = 3000.0;

/// A test view (`BO2MP_T7_VIEW`): 1 the sun's visibility, 2 the ambient
/// light, 3 the albedo, 4 the light (no albedo); 0 the picture.
pub(super) fn view() -> f32 {
    match std::env::var("BO2MP_T7_VIEW").as_deref().map(str::trim) {
        Ok("sun" | "1") => 1.0,
        Ok("ambient" | "2") => 2.0,
        Ok("albedo" | "3") => 3.0,
        Ok("light" | "4") => 4.0,
        _ => 0.0,
    }
}

/// A map's look against pictures of the game: exposure, the sun's factor
/// over its intensity, the sky light's gain in the draw (ambient and
/// reflections, against the sun), the grade's saturation and per-channel
/// gain, and the sky's exposure over the world's. Each has its test
/// override (`BO2MP_T7_EXPOSURE`, `_SUN`, `_SKY_GAIN`, `_SATURATION`,
/// `_TINT=r,g,b`, `_SKY_EXPOSURE`); and the probes' reflections' own gain
/// (`_REFLECT`; 0: the sky light's).
#[derive(Clone, Copy, Debug)]
pub(super) struct Tune {
    pub exposure: f32,
    pub sun: f32,
    pub sky_gain: f32,
    pub saturation: f32,
    pub tint: [f32; 3],
    pub sky_exposure: f32,
    pub reflect: f32,
    /// The bloom's strength (`BO2MP_T7_BLOOM` scales it).
    pub bloom: f32,
}

/// The tune Redwood was matched with (every map without its own).
const DEFAULT_TUNE: Tune = Tune { exposure: EXPOSURE, sun: SUN_SCALE, sky_gain: 1.0, saturation: 1.0, tint: [1.0; 3], sky_exposure: 1.0, reflect: 0.0, bloom: 1.0 };

/// Nuk3town against the owner's four 1080p pictures of the retail game
/// (`docs/screenshots/nuk3town_ref`): the default tune drew it a stop too
/// bright, low in contrast (the shoulder of the tone curve) and cool (sky
/// light over the sun); sunlit grass (156, 174, 91) and paths (195, 196,
/// 190) measured against them.
const NUKETOWN_TUNE: Tune = Tune { exposure: 0.56, sun: 3.0, sky_gain: 0.4, saturation: 1.1, tint: [1.02, 1.0, 1.0], sky_exposure: 2.75, reflect: 1.0, bloom: 1.0 };

/// The tune for a map, with the test overrides.
pub(super) fn tune(map: &str) -> Tune {
    let base = if map.starts_with("mp_nuketown_x") { NUKETOWN_TUNE } else { DEFAULT_TUNE };
    let tint = std::env::var("BO2MP_T7_TINT")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect::<Vec<f32>>())
        .filter(|t| t.len() == 3 && t.iter().all(|x| x.is_finite() && *x >= 0.0))
        .map_or(base.tint, |t| [t[0], t[1], t[2]]);
    Tune {
        exposure: std::env::var("BO2MP_T7_EXPOSURE").ok().and_then(|v| v.trim().parse().ok()).filter(|v: &f32| *v > 0.0 && *v < 100.0).unwrap_or(base.exposure),
        sun: env_scale("BO2MP_T7_SUN", base.sun),
        sky_gain: env_scale("BO2MP_T7_SKY_GAIN", base.sky_gain),
        saturation: env_scale("BO2MP_T7_SATURATION", base.saturation),
        tint,
        sky_exposure: env_scale("BO2MP_T7_SKY_EXPOSURE", base.sky_exposure),
        bloom: base.bloom,
        reflect: std::env::var("BO2MP_T7_REFLECT").ok().and_then(|v| v.trim().parse().ok()).filter(|v: &f32| *v >= 0.0 && *v < 100.0).unwrap_or(base.reflect),
    }
}

/// The map's light, baked.
pub(super) struct Lit {
    pub bake: Bake,
    /// The probes drawn (`keep[0]` is 0, the map-wide one).
    pub keep: Vec<usize>,
    pub sun_map: SunMap,
    tracer: Tracer,
    /// Toward the sun.
    pub sun: [f32; 3],
    /// The sun's brightness in the probes' unit (its colour's peak one).
    pub sun_light: f32,
    pub exposure: f32,
    /// The map's tune (`tune`).
    pub tune: Tune,
    /// The fog's rows (`T7_FOG_ROW`), when the map has fog.
    pub fog: Option<[[f32; 4]; 8]>,
    pub report: Vec<String>,
}

/// bo2mp: the first of the two light rows the fog's eight vec4s sit in
/// (`T7_FOG` in the shader).
pub(super) const T7_FOG_ROW: usize = 252;

/// How much a sky dome's glow lifts the sky under it (over the sky's own
/// light there): Nuk3town's hex grid, lines about sRGB 41 in its picture,
/// faint cyan over the blue in the game.
const DOME_LIFT: f32 = 48.0;

/// bo2mp: a glowing sky dome (a placed model whose material is an emissive
/// `_sky` technique: Nuk3town's hex grid, `p7_ntx_vista_sky1`) painted into
/// the sky picture: the sky pass draws over a dome drawn as a model. Each
/// dome triangle is laid over the picture by direction from the dome's
/// origin, its emissive picture read at the texel's place on it.
fn paint_sky_domes(map: &T7Map, sky: &mut fastfile_t7::light::SkyPicture) -> Option<String> {
    let mut painted = 0usize;
    let mut names = Vec::new();
    for sm in &map.static_models {
        let model = &map.models[sm.model];
        for surf in &model.surfaces {
            let Some(mi) = surf.material else { continue };
            let mat = &map.materials[mi];
            if !mat.technique_set.contains("_sky") || !mat.technique_set.contains("emissive") {
                continue;
            }
            let Some(name) = mat.texture("emissiveMap") else { continue };
            let Ok(levels) = map.image(name, 2048) else { continue };
            let Some((iw, ih, px)) = fastfile_t7::texture::level_rgba(&levels, 0) else { continue };
            names.push(model.name.clone());
            let (w, h) = (sky.width as usize, sky.height as usize);
            // Per sky texel: the direction, and which triangle covers it.
            let dir_of = |x: usize, y: usize| -> [f32; 3] {
                let u = (x as f32 + 0.5) / w as f32;
                let v = (y as f32 + 0.5) / h as f32;
                let phi = (0.5 - u) * std::f32::consts::TAU;
                let th = v * std::f32::consts::PI;
                [th.sin() * phi.sin(), th.sin() * phi.cos(), th.cos()]
            };
            let place = |p: [f32; 3]| -> [f32; 3] {
                let q: [f32; 3] = std::array::from_fn(|k| (p[0] * sm.axis[0][k] + p[1] * sm.axis[1][k] + p[2] * sm.axis[2][k]) * sm.scale);
                let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt().max(1e-6);
                q.map(|c| c / n)
            };
            for t in &surf.tris {
                let p: [[f32; 3]; 3] = std::array::from_fn(|k| place(surf.positions[usize::from(t[k])]));
                let uv: [[f32; 2]; 3] = std::array::from_fn(|k| surf.uvs[usize::from(t[k])]);
                // The triangle's texel box on the picture.
                let lat = |d: [f32; 3]| (d[2].clamp(-1.0, 1.0).acos() / std::f32::consts::PI * h as f32) as i64;
                let lon = |d: [f32; 3]| ((0.5 - d[0].atan2(d[1]) / std::f32::consts::TAU) * w as f32) as i64;
                let (y0, y1) = (p.iter().map(|d| lat(*d)).min().unwrap_or(0) - 1, p.iter().map(|d| lat(*d)).max().unwrap_or(0) + 1);
                let xs: Vec<i64> = p.iter().map(|d| lon(*d)).collect();
                let (mut x0, mut x1) = (*xs.iter().min().unwrap_or(&0) - 1, *xs.iter().max().unwrap_or(&0) + 1);
                if x1 - x0 > w as i64 / 2 {
                    // (Across the seam, or round a pole: every column.)
                    x0 = 0;
                    x1 = w as i64 - 1;
                }
                let n = {
                    let (a, b) = (sub(p[1], p[0]), sub(p[2], p[0]));
                    cross(a, b)
                };
                let area = dot(n, n);
                if area < 1e-12 {
                    continue;
                }
                for y in y0.max(0)..=y1.min(h as i64 - 1) {
                    for x in x0..=x1 {
                        let xi = x.rem_euclid(w as i64) as usize;
                        let d = dir_of(xi, y as usize);
                        // The ray from the centre through the triangle's plane.
                        let dn = dot(d, n);
                        if dn.abs() < 1e-9 {
                            continue;
                        }
                        let tt = dot(p[0], n) / dn;
                        if tt <= 0.0 {
                            continue;
                        }
                        let q = d.map(|c| c * tt);
                        let b0 = dot(cross(sub(p[1], q), sub(p[2], q)), n) / area;
                        let b1 = dot(cross(sub(p[2], q), sub(p[0], q)), n) / area;
                        let b2 = 1.0 - b0 - b1;
                        if b0 < -1e-4 || b1 < -1e-4 || b2 < -1e-4 {
                            continue;
                        }
                        let u = b0 * uv[0][0] + b1 * uv[1][0] + b2 * uv[2][0];
                        let v = b0 * uv[0][1] + b1 * uv[1][1] + b2 * uv[2][1];
                        let tx = ((u.rem_euclid(1.0)) * iw as f32) as usize % iw as usize;
                        let ty = ((v.rem_euclid(1.0)) * ih as f32) as usize % ih as usize;
                        let o = (ty * iw as usize + tx) * 4;
                        let lin = |b: u8| fastfile_t7::look::to_linear(b);
                        let g = [lin(px[o]), lin(px[o + 1]), lin(px[o + 2])];
                        if g.iter().all(|c| *c <= 0.0) {
                            continue;
                        }
                        let cell = &mut sky.rgb[y as usize * w + xi];
                        let l = 0.2126 * cell[0] + 0.7152 * cell[1] + 0.0722 * cell[2];
                        for k in 0..3 {
                            cell[k] += g[k] * DOME_LIFT * l;
                        }
                        painted += 1;
                    }
                }
            }
        }
    }
    // `BO2MP_T7_SKY_DUMP=<file.png>`: the sky as painted (a check).
    if let Ok(path) = std::env::var("BO2MP_T7_SKY_DUMP") {
        let peak = sky.rgb.iter().map(|c| c[0].max(c[1]).max(c[2])).fold(0.0f32, f32::max).max(1e-6);
        let px: Vec<u8> = sky.rgb.iter().flat_map(|c| c.map(|v| ((v / peak).sqrt() * 255.0).round().clamp(0.0, 255.0) as u8)).collect();
        if let Ok(f) = std::fs::File::create(&path) {
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), sky.width, sky.height);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            if let Ok(mut w) = e.write_header() {
                let _ = w.write_image_data(&px);
            }
        }
    }
    (!names.is_empty()).then(|| format!("sky domes painted into the sky: {names:?} ({painted} texels)"))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// bo2mp (load speed, lane B): the light read, its bake still to come, so the
/// bake (a Skyjacked's 3.4 s: its casters, sun map and tracer) runs beside the
/// world's materials, which only ask whether there is light.
pub(super) struct LitRead {
    bake: Bake,
    started: std::time::Instant,
}

impl LitRead {
    /// Bakes the sun map and the occluders round the spawn points
    /// (`spawns`); `blocks` says what a material does to light. `trace`:
    /// build the occlusion tracer (else an empty one: nothing reads it).
    pub fn finish(self, zone: &std::path::Path, map: &T7Map, spawns: &[[f32; 3]], blocks: &dyn Fn(usize) -> Blocks, trace: bool) -> Lit {
        let LitRead { bake, started } = self;
        let mut report: Vec<String> = bake.report.iter().map(|l| format!("t7 light: {l}")).collect();
        // bo2mp (load speed): the sun map kept on disk (`t7_light_cache`),
        // only when the tracer is not wanted (it needs the blockers, which
        // the cache skips).
        let slot = (!trace && super::t7_light_cache::on()).then(|| super::t7_light_cache::Slot::new(zone, map, spawns, blocks)).flatten();
        // The spawn points' square (else the probes' boxes).
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in spawns {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        if lo[0] > hi[0] {
            lo = [bake.bounds[0][0], bake.bounds[0][1]];
            hi = [bake.bounds[1][0], bake.bounds[1][1]];
        }
        let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let span = ((hi[0] - lo[0]).max(hi[1] - lo[1]) + MARGIN).clamp(2048.0, 16000.0);
        let area = [[centre[0] - span * 0.5 - MARGIN, centre[1] - span * 0.5 - MARGIN], [centre[0] + span * 0.5 + MARGIN, centre[1] + span * 0.5 + MARGIN]];
        let sun = map.sun.as_ref().map_or([0.4, 0.3, 0.866], fastfile_t7::light::sun_toward);
        let t = std::time::Instant::now();
        let read_back = slot.as_ref().and_then(|s| s.read());
        let (sun_map, casters_n, partial_n, tracer) = if let Some(baked) = read_back {
            report.push(format!(
                "t7 light: sun map read back from {} in {} ms (BO2MP_T7_LIGHT_CACHE=0: baked every load)",
                slot.as_ref().map_or(String::new(), |s| s.path().display().to_string()),
                t.elapsed().as_millis()
            ));
            (baked.sun_map, baked.casters, baked.partial, Tracer::new(&[]))
        } else {
            let casters = fastfile_t7::light::casters(map, blocks, 8.0, area);
            let casters_ms = t.elapsed().as_millis();
            let z = spawns.iter().map(|p| p[2]).sum::<f32>() / spawns.len().max(1) as f32;
            let t = std::time::Instant::now();
            let sun_map = SunMap::render(&casters, sun, [centre[0], centre[1], z], span * 1.15, 4096);
            let sun_map_ms = t.elapsed().as_millis();
            let t = std::time::Instant::now();
            let tracer = if trace {
                let solid: Vec<[[f32; 3]; 3]> = casters.iter().filter(|c| !c.partial).map(|c| c.p).collect();
                Tracer::new(&solid)
            } else {
                Tracer::new(&[])
            };
            let tracer_ms = t.elapsed().as_millis();
            report.push(format!(
                "t7 light: bake pieces: blockers {casters_ms} ms, sun map {sun_map_ms} ms, occlusion tracer {} ({tracer_ms} ms)",
                if trace { "built" } else { "skipped: the native renderer draws the map (BO2MP_T7_NATIVE_TRACER=1: built)" }
            ));
            let partial = casters.iter().filter(|c| c.partial).count();
            let baked = super::t7_light_cache::Baked { sun_map, casters: casters.len(), partial };
            if let Some(s) = slot.as_ref() {
                let t = std::time::Instant::now();
                match s.write(&baked) {
                    Ok(n) => report.push(format!("t7 light: sun map written to {} ({:.1} MiB, {} ms): later loads read it back", s.path().display(), n as f64 / 1048576.0, t.elapsed().as_millis())),
                    Err(e) => report.push(format!("t7 light: sun map not written to {}: {e}", s.path().display())),
                }
            }
            (baked.sun_map, baked.casters, baked.partial, tracer)
        };
        let units = bake.units();
        let sun_light = map.sun.filter(|s| s.intensity.is_finite() && s.intensity > 0.0).map_or(5.0, |s| s.intensity / units);
        // The probes drawn: those the most surfaces and models take light
        // from round the spawn points, the map-wide one first.
        let mut uses = vec![0usize; bake.probes.len()];
        let inside = |p: [f32; 3]| p[0] >= area[0][0] && p[0] <= area[1][0] && p[1] >= area[0][1] && p[1] <= area[1][1];
        for s in &map.surfaces {
            let c = std::array::from_fn(|k| (s.mins[k] + s.maxs[k]) * 0.5);
            if inside(c)
                && let Some((_, i)) = bake.box_at(c)
            {
                uses[i] += 1;
            }
        }
        for sm in &map.static_models {
            if inside(sm.origin)
                && let Some((_, i)) = bake.box_at(sm.origin)
            {
                uses[i] += 1;
            }
        }
        let mut order: Vec<usize> = (1..bake.probes.len()).filter(|&i| uses[i] > 0).collect();
        order.sort_by(|&a, &b| uses[b].cmp(&uses[a]));
        let mut keep = vec![0usize];
        keep.extend(order.into_iter().take(MAX_PROBES - 1));
        report.push(format!(
            "t7 light: {} of {} probes drawn (unit {units:.1}), sun toward {sun:.3?} at {sun_light:.2}, sun map {}px over {:.0} units ({:.2} a texel), {} blockers ({} partial), {:.1}s",
            keep.len(),
            bake.probes.len(),
            sun_map.depth.size,
            span * 1.15,
            sun_map.depth.texel,
            casters_n,
            partial_n,
            started.elapsed().as_secs_f32()
        ));
        let fog = Fog::read(map, &bake, sun, sun_light, spawns);
        match fog.as_ref() {
            Some(f) => report.push(f.report.clone()),
            None => report.push("t7 fog: none (no fog volume with atmospheric settings, or BO2MP_T7_FOG=0)".to_owned()),
        }
        let tune = tune(&map.name);
        Lit { bake, keep, sun_map, tracer, sun, sun_light, exposure: tune.exposure, tune, fog: fog.map(|f| f.rows), report }
    }
}

impl Lit {
    /// Reads the light, for `LitRead::finish` to bake (every way there is
    /// no light is here).
    pub fn read(map: &T7Map) -> Result<LitRead, String> {
        let started = std::time::Instant::now();
        let mut bake = Bake::read(map)?;
        if let Some(sky) = bake.sky.as_mut()
            && let Some(line) = paint_sky_domes(map, sky)
        {
            bake.report.push(line);
        }
        if bake.probes.is_empty() {
            return Err("no reflection probes".to_owned());
        }
        Ok(LitRead { bake, started })
    }


    /// The drawn probe a point reflects (an index into `keep`): the
    /// smallest kept box holding it, else the nearest kept box within its
    /// blend, else the map-wide one.
    pub fn probe_for(&self, p: [f32; 3]) -> u8 {
        let mut best: Option<(f32, f32, usize)> = None;
        for (k, &i) in self.keep.iter().enumerate().skip(1) {
            let pr = &self.bake.probes[i];
            let o = pr.outside(p);
            if o > 96.0 {
                continue;
            }
            let key = (o, pr.volume());
            if best.is_none_or(|(bo, bv, _)| key < (bo, bv)) {
                best = Some((key.0, key.1, k));
            }
        }
        best.map_or(0, |b| b.2 as u8)
    }

    /// How open a point is (0..1): short rays over the hemisphere round
    /// `n`, softened (the probes already hold the room's light).
    /// (`BO2MP_T7_AMBIENT` scales the ambient light: a test aid.)
    fn occlusion(&self, p: [f32; 3], n: [f32; 3], seed: u32, reach: f32) -> f32 {
        static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
        (0.35 + 0.65 * self.tracer.openness(p, n, reach, 12, seed, 1.0)) * *SCALE.get_or_init(|| env_scale("BO2MP_T7_AMBIENT", 1.0))
    }

    /// Each world vertex's ambient light (probes' unit, occlusion in), as
    /// the vertex colour, its alpha kept from `colours`; on a reveal decal
    /// (`reveals`: map material, its soft edge and ramp) the alpha carries
    /// the soft edge too (`reveal_alpha`).
    pub fn world_colours(&self, map: &T7Map, colours: &[[f32; 4]], reveals: &std::collections::HashMap<usize, [f32; 2]>) -> Vec<[f32; 4]> {
        let mut soft = vec![0f32; map.vertices.len()];
        for s in &map.surfaces {
            if let Some(r) = s.material.and_then(|m| reveals.get(&m)) {
                let a = s.first_vertex as usize;
                let b = (a + s.vertex_count as usize).min(soft.len());
                if a < b {
                    soft[a..b].fill(r[0]);
                }
            }
        }
        fastfile_t7::bake::parallel_map(map.vertices.len(), |i| {
            let v = &map.vertices[i];
            let n = v.normal;
            let a = self.ambient_at(v.xyz, n, i as u32, 128.0);
            let alpha = colours.get(i).map_or(1.0, |c| c[3]);
            [a[0], a[1], a[2], if soft[i] > 0.0 { reveal_alpha(alpha, soft[i]) } else { alpha }]
        })
    }

    /// The ambient light at `p` facing `n`: the probes' ambient cube times,
    /// where the probe volume is read (`Bake::volume_term`), its scale and
    /// a light contact occlusion (short rays: the volume holds the room's
    /// shadowing at its texels, about 4 to 8 units), else the occlusion
    /// alone. `BO2MP_T7_VOLUME=0` leaves the volume out.
    fn ambient_at(&self, p: [f32; 3], n: [f32; 3], seed: u32, reach: f32) -> [f32; 3] {
        static VOLUME: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let a = self.bake.ambient_lit(p, n);
        let term = if *VOLUME.get_or_init(|| env_scale("BO2MP_T7_VOLUME", 1.0) > 0.0) { self.bake.volume_term(p, n) } else { None };
        match term {
            Some(t) => {
                static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
                let o = (0.7 + 0.3 * self.tracer.openness(p, n, reach * 0.5, 8, seed, 1.0)) * *SCALE.get_or_init(|| env_scale("BO2MP_T7_AMBIENT", 1.0));
                [a[0] * t[0] * o, a[1] * t[1] * o, a[2] * t[2] * o]
            }
            None => {
                let o = self.occlusion(p, n, seed, reach);
                [a[0] * o, a[1] * o, a[2] * o]
            }
        }
    }

    /// A placed model's light (`packed_lighting`: sqrt(light / 32)): the
    /// mean ambient light at its origin, its occlusion seen from just
    /// above it.
    pub fn prop_light(&self, origin: [f32; 3], seed: u32) -> [u8; 4] {
        Self::pack(self.mean_ambient([origin[0], origin[1], origin[2] + 8.0], seed))
    }

    /// The mean ambient light over the six axes at `p`.
    fn mean_ambient(&self, p: [f32; 3], seed: u32) -> [f32; 3] {
        let mut a = [0f32; 3];
        for d in [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0f32]] {
            let c = self.ambient_at(p, d, seed, 256.0);
            for k in 0..3 {
                a[k] += c[k] / 6.0;
            }
        }
        a
    }

    fn pack(a: [f32; 3]) -> [u8; 4] {
        let p = |v: f32| ((v.max(0.0) / 32.0).sqrt() * 255.0).round().clamp(1.0, 255.0) as u8;
        [p(a[0]), p(a[1]), p(a[2]), 0]
    }

    /// `prop_light` for a placed model with its bounds (model space): a
    /// big model's origin is often inside it (Nuk3town's hover bus, on the
    /// ground under its hull), where the occlusion rays see only its
    /// insides and its whole shaded side drew black. Such a model (over
    /// 48 units) takes the light just outside its box instead: above it
    /// and off its four sides at half height, the mean of the three
    /// brightest (the side against a wall or the ground left out).
    pub fn prop_light_box(&self, origin: [f32; 3], axis: [[f32; 3]; 3], scale: f32, lo: [f32; 3], hi: [f32; 3], seed: u32) -> [u8; 4] {
        let half: [f32; 3] = std::array::from_fn(|k| (hi[k] - lo[k]) * 0.5 * scale);
        if half.iter().all(|h| *h * 2.0 <= 48.0) || lo[0] > hi[0] {
            return self.prop_light(origin, seed);
        }
        let mid: [f32; 3] = std::array::from_fn(|k| (hi[k] + lo[k]) * 0.5 * scale);
        let at = |l: [f32; 3]| -> [f32; 3] { std::array::from_fn(|k| origin[k] + l[0] * axis[0][k] + l[1] * axis[1][k] + l[2] * axis[2][k]) };
        let points = [
            at([mid[0], mid[1], mid[2] + half[2] + 8.0]),
            at([mid[0] + half[0] + 16.0, mid[1], mid[2]]),
            at([mid[0] - half[0] - 16.0, mid[1], mid[2]]),
            at([mid[0], mid[1] + half[1] + 16.0, mid[2]]),
            at([mid[0], mid[1] - half[1] - 16.0, mid[2]]),
        ];
        let mut lit: Vec<[f32; 3]> = points.iter().map(|p| self.mean_ambient(*p, seed)).collect();
        lit.sort_by(|a, b| (b[0] + b[1] + b[2]).total_cmp(&(a[0] + a[1] + a[2])));
        let mut a = [0f32; 3];
        for l in &lit[..3] {
            for k in 0..3 {
                a[k] += l[k] / 3.0;
            }
        }
        Self::pack(a)
    }

    /// The light rows (`asset_world::T7_LIGHT_ROW`): exposure, the sun's
    /// light over the sun colour drawn (`drawn_sun`), the sky light's gain,
    /// kind 7; then the sun
    /// map's rows.
    pub fn light_rows(&self, drawn_sun: f32) -> [[f32; 16]; 2] {
        let m = self.sun_map.rows();
        let mut a = [0f32; 16];
        a[0] = self.exposure;
        a[1] = self.sun_light * self.tune.sun / drawn_sun.max(1e-3);
        a[2] = self.tune.sky_gain;
        a[3] = 7.0;
        for k in 0..3 {
            a[4 + 4 * k..8 + 4 * k].copy_from_slice(&m[k]);
        }
        let mut b = [0f32; 16];
        b[..4].copy_from_slice(&m[3]);
        b[4] = view();
        b[5] = self.tune.sky_exposure;
        b[7] = 7.0;
        b[8] = self.tune.reflect;
        [a, b]
    }

    /// The probe table rows: each drawn probe's gain (its light over the
    /// map-wide probe's) and where it was taken; its parallax box's centre
    /// and its turn about z (cosine), its half size and the turn's sine
    /// (half size 0: no parallax, the map-wide probe). `BO2MP_T7_PARALLAX=0`
    /// leaves the parallax out.
    pub fn probe_rows(&self) -> Vec<[[f32; 4]; 3]> {
        let parallax = std::env::var("BO2MP_T7_PARALLAX").map_or(true, |v| v.trim() != "0");
        self.keep
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let g = self.bake.gain(i);
                let p = &self.bake.probes[i];
                let (c, s) = (p.axes[0][0], p.axes[0][1]);
                let flat = (p.axes[2][2] - 1.0).abs() < 0.01 && (c * c + s * s - 1.0).abs() < 0.02;
                let on = parallax && k > 0 && flat && p.box_half.iter().all(|h| *h > 1.0 && *h < 20000.0) && p.box_centre.iter().all(|v| v.is_finite());
                let h = if on { p.box_half } else { [0.0; 3] };
                [[g, p.origin[0], p.origin[1], p.origin[2]], [p.box_centre[0], p.box_centre[1], p.box_centre[2], c], [h[0], h[1], h[2], s]]
            })
            .collect()
    }

    /// The sun map as the renderer's colour-table slot.
    pub fn sun_map_lut(&self) -> asset_world::T6Lut {
        let s = self.sun_map.depth.size;
        asset_world::T6Lut { width: s, height: s, rgba: self.sun_map.texels() }
    }

    /// Links the sky (a cube from the picture, in the probes' unit) and the
    /// drawn probes' cubes into the catalog.
    pub fn link(&self, map: &T7Map, name: &str, catalog: &mut MaterialCatalog, owner: ZoneOwner) -> Vec<String> {
        let mut report = Vec::new();
        match self.bake.sky.as_ref() {
            Some(sky) => {
                let gain = self.bake.sky_gain();
                let cube = fastfile_t7::light::sky_cube(sky, SKY_FACE);
                let mut data = Vec::with_capacity(cube.len() * 8);
                for c in &cube {
                    for k in 0..4 {
                        let v = if k == 3 { 1.0 } else { c[k] * gain };
                        data.extend_from_slice(&super::t7_map::f16_bits(v).to_le_bytes());
                    }
                }
                let mut image = Image::new_uninit(
                    Extent3d { width: SKY_FACE, height: SKY_FACE, depth_or_array_layers: 6 },
                    TextureDimension::D2,
                    TextureFormat::Rgba16Float,
                    RenderAssetUsages::RENDER_WORLD,
                );
                image.data = Some(data);
                image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..Default::default() });
                image.sampler = clamped();
                let slot = link_image(catalog, format!("t7:{name}#sky"), SKY_FACE, 1, TS_COLOR_MAP, image);
                catalog.link_material(material(format!("t7:{name}#sky"), TS_COLOR_MAP, slot, owner, Some(T6Draw::from_technique_set("wc_sky", 0, None))));
                report.push(format!("t7 light: sky {SKY_FACE}px cube from the picture, times {gain:.5}"));
            }
            None => report.push("t7 light: no sky picture; the sky is not drawn".to_owned()),
        }
        match fastfile_t7::light::probe_array(map, &self.keep) {
            Ok(arr) if arr.format == 95 || arr.format == 96 => {
                let mut image = Image::new_uninit(
                    Extent3d { width: arr.size, height: arr.size, depth_or_array_layers: 6 * self.keep.len() as u32 },
                    TextureDimension::D2,
                    if arr.format == 95 { TextureFormat::Bc6hRgbUfloat } else { TextureFormat::Bc6hRgbFloat },
                    RenderAssetUsages::RENDER_WORLD,
                );
                image.texture_descriptor.mip_level_count = arr.levels;
                image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::CubeArray), ..Default::default() });
                image.data = Some(arr.data);
                image.sampler = clamped();
                let semantic = super::t6_materials::T6_PROBES_SEMANTIC;
                let slot = link_image(catalog, format!("t7:{name}#probes"), arr.size, arr.levels, semantic, image);
                catalog.link_material(material(super::t6_materials::T6_PROBES_MATERIAL.to_owned(), semantic, slot, owner, None));
                report.push(format!("t7 light: {} probe cubes ({}px, {} levels)", self.keep.len(), arr.size, arr.levels));
            }
            Ok(arr) => report.push(format!("t7 light: probes: format {} not drawn", arr.format)),
            Err(e) => report.push(format!("t7 light: probes: {e}")),
        }
        report
    }
}

/// bo2mp: Black Ops III's atmospheric fog for the shader (`t7_fog_core` in
/// `geometry_diagnostic.wgsl`), from the map's fog volume: the one holding
/// most spawn points (else the map-wide one), lighting state 0. Its bank's
/// words as we read them (BO3 turns them into its shader constants in code
/// we cannot read; names ours, checked against how the maps look): 1 start
/// distance, 2 half distance (the transmittance halves every this many
/// units), 3 base height, 4 half height (the density halves every this many
/// units up), 16..18 the fog's (Rayleigh) colour, 19 extinction strength,
/// 20..22 the haze's (Mie) colour, 23 haze start, 24 haze spread, 25 the
/// haze's Schlick k (forward scattering), 26 its density, 28 in-scatter
/// strength, 30 sun strength (the Rayleigh phase's share), 31 the sky's
/// distance (27, 13.5 on most maps, 19.75 on the snow one: not used).
///
/// The fog's in-scatter colour is the sky's horizon colour (a far wall
/// fogs into the sky behind it); the haze's is its colour at the sun's
/// light times its density. `BO2MP_T7_FOG` scales the
/// density (0: no fog), `BO2MP_T7_HAZE` the haze, `BO2MP_T7_SHAFTS` the sun
/// shafts (0: none).
pub(super) struct Fog {
    pub rows: [[f32; 4]; 8],
    pub report: String,
}

fn env_scale(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

impl Fog {
    pub fn read(map: &T7Map, bake: &Bake, toward_sun: [f32; 3], sun_light: f32, spawns: &[[f32; 3]]) -> Option<Fog> {
        let density = env_scale("BO2MP_T7_FOG", 1.0);
        if density <= 0.0 {
            return None;
        }
        let vols = fastfile_t7::light::fog(map);
        // The volume most spawn points are in (ties: the higher priority).
        let mut best: Option<(usize, f32, usize)> = None;
        for (i, v) in vols.iter().enumerate() {
            if v.bank(0).is_none() {
                continue;
            }
            let n = v.holds(spawns);
            if best.is_none_or(|(bn, bp, _)| (n, v.priority) > (bn, bp)) {
                best = Some((n, v.priority, i));
            }
        }
        let (inside, _, vi) = best?;
        let b = vols[vi].bank(0)?;
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let norm = |c: [f32; 3]| {
            let l = lum(c).max(1e-4);
            c.map(|x| x.max(0.0) / l)
        };
        // The sky's horizon, in the probes' unit.
        // The sky's horizon (its colour round the map a few degrees up, the
        // brightest quarter, the sun's side, left out), in the probes' unit.
        let horizon = match bake.sky.as_ref() {
            Some(sky) => {
                let g = bake.sky_gain();
                let mut ring: Vec<[f32; 3]> = (0..64)
                    .map(|k| {
                        let a = k as f32 / 64.0 * std::f32::consts::TAU;
                        sky.at([a.cos(), a.sin(), 0.08]).map(|c| c * g)
                    })
                    .collect();
                ring.sort_by(|x, y| lum(*x).total_cmp(&lum(*y)));
                let keep = &ring[..48];
                let mut sum = [0f32; 3];
                for c in keep {
                    for j in 0..3 {
                        sum[j] += c[j] / keep.len() as f32;
                    }
                }
                sum
            }
            None => [1.0; 3],
        };
        // The fog takes on the horizon's colour (a far wall fogs into the
        // sky behind it; the bank's own colour, words 16..18, read as a
        // Rayleigh tint, was far bluer than the game's fog on mp_waterpark).
        let fog_c = horizon;
        // The haze scatters the sun: its colour at the sun's light times its
        // density (times 2 pi: the phase's normalisation, half of it).
        let haze_c = norm([b[20], b[21], b[22]]).map(|x| x * sun_light * std::f32::consts::TAU * b[26].max(0.0) * env_scale("BO2MP_T7_HAZE", 1.0));
        let half = (b[2] / density).max(1.0);
        let ext = if b[19] > 0.0 { b[19].min(2.0) } else { 1.0 };
        let k = b[25].clamp(0.0, 0.95);
        let half_height = if b[4] > 1.0 { 1.0 / b[4] } else { 0.0 };
        let shafts = env_scale("BO2MP_T7_SHAFTS", 1.0);
        let rows = [
            [fog_c[0], fog_c[1], fog_c[2], b[23].max(0.0)],
            [haze_c[0], haze_c[1], haze_c[2], 1.0 / b[24].max(1.0)],
            [-1.0 / half, -1.0 / half, -1.0 / half, b[31].clamp(2000.0, 200000.0)],
            [toward_sun[0], toward_sun[1], toward_sun[2], k],
            [ext, b[28].max(0.0), b[30].clamp(0.0, 1.0), 1.0],
            [b[1].max(0.0), b[3], half_height, 0.0],
            [b[1].max(0.0), b[3], half_height, 0.0],
            [1.0 / 2000.0, 2500.0, if shafts > 0.0 { 16.0 } else { 0.0 }, shafts],
        ];
        let report = format!(
            "t7 fog: volume {vi} of {} ({inside} of {} spawns in it): start {:.0}, half {:.0}, base {:.0}, half height {:.0}, fog {:.3?} (the horizon's), haze {:.3?} from {:.0} over {:.0}, k {k:.2}, extinction {ext:.2}, in-scatter {:.2}, sun strength {:.2}, sky at {:.0}",
            vols.len(),
            spawns.len(),
            b[1],
            half,
            b[3],
            b[4],
            fog_c,
            haze_c,
            b[23],
            b[24],
            b[28],
            b[30],
            b[31]
        );
        Some(Fog { rows, report })
    }
}

/// bo2mp: the final pass's grade for a Black Ops III map lit by its own
/// light: its colour table (the `GfxWorld`'s LUT material's picture, a
/// `create_lut2dv` source like Black Ops II's: 32 tables of 32 x 32 side by
/// side per 32 rows, R10G10B10A2; the first band, which the map starts in)
/// and a bloom for its highlights (BO3's bloom settings are not read: its
/// post shaders are not in the zones read; the ramp and tints are ours).
/// `BO2MP_T7_GRADE=0`: no table; `BO2MP_T7_BLOOM` scales the bloom (0 none).
pub(super) fn grade(map: &T7Map, report: &mut Vec<String>) -> Option<asset_world::T6Grade> {
    let mut vision = asset_world::T6Vision::default();
    // (Nuk3town's pictures glow round every bright edge: its tune's bloom.)
    let bloom = env_scale("BO2MP_T7_BLOOM", 1.0) * tune(&map.name).bloom;
    if bloom > 0.0 {
        vision.bloom_rgb_hi = [0.35 * bloom, 0.35 * bloom, 0.35 * bloom, 0.7];
        vision.bloom_rgb_lo = [0.45 * bloom, 0.45 * bloom, 0.45 * bloom, 3.0];
    }
    // The grade's saturation and per-channel gain (`tune`).
    let t = tune(&map.name);
    vision.final_saturation[3] = t.saturation;
    for rows in [&mut vision.shadow, &mut vision.midtone, &mut vision.highlight] {
        for ch in 0..3 {
            rows[ch][ch] *= t.tint[ch];
        }
    }
    let mut base: Option<Vec<u8>> = None;
    if env_scale("BO2MP_T7_GRADE", 1.0) > 0.0
        && let Some(name) = fastfile_t7::light::lut_image(map)
    {
        match map.image(&name, 1024) {
            Ok(l) if l.format == 24 && l.width == 1024 && l.height >= 32 && l.data.len() >= 1024 * 32 * 4 => {
                let px: Vec<u8> = l.data[..1024 * 32 * 4]
                    .chunks_exact(4)
                    .flat_map(|w| {
                        let v = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
                        let c = |k: u32| ((((v >> (10 * k)) & 1023) as f32) * 255.0 / 1023.0).round() as u8;
                        [c(0), c(1), c(2), 255]
                    })
                    .collect();
                report.push(format!("t7 grade: {name} (band 0 of {} rows)", l.height));
                base = Some(px);
            }
            Ok(l) => report.push(format!("t7 grade: {name}: format {} {}x{} not read", l.format, l.width, l.height)),
            Err(e) => report.push(format!("t7 grade: {e}")),
        }
    }
    report.push(format!("t7 bloom: x{bloom}"));
    Some(asset_world::T6Grade { lut: asset_world::t6_grade_lut(&vision, base.as_deref()), vision })
}

/// A reveal decal vertex's alpha with its material's soft edge, for the
/// shader (`t7_reveal`): -(2 + 2 q + 0.999 a), q the soft edge in 64ths.
fn reveal_alpha(a: f32, soft: f32) -> f32 {
    let q = (soft * 64.0).round().clamp(1.0, 64.0);
    -(2.0 + 2.0 * q + 0.999 * a.clamp(0.0, 1.0))
}

pub(super) fn clamped() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    })
}

pub(super) fn link_image(catalog: &mut MaterialCatalog, name: String, size: u32, levels: u32, semantic: u8, image: Image) -> usize {
    catalog.link_image(AuthoredImage {
        namespace: AssetNamespace::T6,
        name: AssetRef::Real(name),
        map_type: 5,
        semantic,
        category: 0,
        use_srgb_reads: false,
        width: size.min(u32::from(u16::MAX)) as u16,
        height: size.min(u32::from(u16::MAX)) as u16,
        depth: 1,
        level_count: levels.min(255) as u8,
        format: 0,
        payload: Arc::new(Vec::new()),
        decoded: Some(Arc::new(image)),
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    })
}

pub(super) fn material(name: String, semantic: u8, slot: usize, owner: ZoneOwner, t6_draw: Option<T6Draw>) -> AuthoredMaterial {
    AuthoredMaterial {
        name: AssetRef::Real(name),
        namespace: AssetNamespace::T6,
        technique_set: AssetRef::default(),
        technique_set_edge: AssetEdge::Absent,
        draw_surf: 0,
        sort_key: 0,
        info_game_flags: 0,
        texture_atlas: None,
        surface_type_bits: None,
        t5_layered_surface_types: None,
        state_flags: 0,
        camera_region: 0,
        state_bits: Vec::new(),
        state_bits_entry: None,
        t5_state_bits_entry: None,
        iw5_state_bits_entry: None,
        technique_table: None,
        route: None,
        textures: vec![MaterialTextureBinding { name_hash: 0, name_start: 0, name_end: 0, sampler_state: 0x13, semantic, image: Some(slot) }],
        constants: Vec::new(),
        zone: owner,
        t6_draw,
    }
}

/// A blank lightmap page (white, the lightmapped pipelines' input), so the
/// world's lit surfaces draw with their normal and specular maps and probe.
pub(super) fn blank_page() -> asset_world::WorldLightmap {
    use bevy::prelude::UVec2;
    let one = || Image::new(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, vec![255; 4], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    let page = Image::new(Extent3d { width: 1, height: 3, depth_or_array_layers: 1 }, TextureDimension::D2, vec![255; 12], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    asset_world::WorldLightmap {
        primary_image: None,
        secondary_image: Some(page),
        secondary_b_image: None,
        ambient_image: one(),
        directional_image: one(),
        sun_mask_image: one(),
        ambient_source_name: "t7 light".to_owned(),
        sun_mask_source_name: "t7 light".to_owned(),
        ambient_size: UVec2::new(1, 3),
        sun_mask_size: UVec2::new(1, 1),
    }
}

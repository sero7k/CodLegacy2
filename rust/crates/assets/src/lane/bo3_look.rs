//! bo2mp: Black Ops III's look on a Black Ops II map (`BO2MP_BO3_LOOK=1`;
//! off by default, so Black Ops II plays as before). The world is drawn by
//! the fallback pass's Black Ops III branch (`t7_shade` in
//! `geometry_diagnostic.wgsl`, the one Black Ops III's own maps use,
//! `super::t7_light`): GGX sun highlights, the probe's reflection by
//! gloss, the sun shadowed by a sun map baked at load over the map's own
//! triangles (`fastfile_t7::light::SunMap`, the same bake), the sun's
//! shafts through it, and the filmic tone curve.
//!
//! What it takes from Black Ops II, as Black Ops II has them: the textures
//! (colour, normal, specular with gloss in its alpha: the forms the branch
//! reads), the sun (direction and colour), the sky cube, the reflection
//! probes (`rgb / a`), the fog, and the light without the sun: the
//! lightmap's ambient and directional pages for the world (a vertex colour
//! here is a tint or layer weights, not free for a baked light) and the
//! light grid for models. `t7_bo2` in the shader says which.
//!
//! `BO2MP_BO3_EXPOSURE` scales the exposure (default 1.2: Black Ops II's own
//! into the tone curve), `BO2MP_T7_HAZE` and `BO2MP_T7_SHAFTS` the shafts.

use asset_core::{T6Blend, T6Draw};
use asset_material::MaterialCatalog;
use asset_world::{ModelMesh, StaticModelPlacement, WorldDraw};
use fastfile_t7::bake::{Caster, Tracer};
use fastfile_t7::light::SunMap;

/// Whether Black Ops II maps take Black Ops III's look (also under the
/// shared look, `BO2MP_BO2_SHARED_LOOK=1`, which then keeps Black Ops II's
/// own sun term: [`SHARED_FLAG`]).
pub(super) fn on() -> bool {
    std::env::var("BO2MP_BO3_LOOK").is_ok_and(|v| v.trim() == "1") || asset_core::bo2_shared_look()
}

/// The shared look's flag in the branch's second row (light row 255,
/// element 14; free in every lane: `t7_shared` in the shader): the sun's
/// visibility is Black Ops II's own baked one (lightmap page 2 alpha for
/// the world, the light grid's for models) times the live sun cascades,
/// and only where Black Ops II's primary light is the sun, so the
/// real-time sun never lands on top of a baked one or where Black Ops II
/// draws none.
pub(super) const SHARED_FLAG: usize = 14;

/// Sets [`SHARED_FLAG`] on a Black Ops II map the branch drew (`t7_light`).
pub(super) fn mark_shared(draw: &mut WorldDraw, report: &mut Vec<String>) {
    if !draw.t7_light || draw.t6_lights.len() < asset_world::T7_LIGHT_ROW + 2 {
        report.push("bo2 shared look: off (the Black Ops III branch is not on for this map)".to_owned());
        return;
    }
    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][SHARED_FLAG] = 1.0;
    report.push(
        "bo2 shared look: on (BO2MP_BO2_SHARED_LOOK=1): Black Ops III's branch with Black Ops II's own sun visibility x the live sun cascades (render_t7::sun_shadow's fit), placed models cast; SMAA and motion blur from Black Ops III's core_patch over the frame"
            .to_owned(),
    );
}

/// Whether another game's maps take the look through their own switch
/// (`BO2MP_BO3_LOOK_IW6` Ghosts, `_JUP` Modern Warfare III (2023), `_H1`
/// Modern Warfare Remastered): `1`/`on`/`true` or `0`/`off`/`false`;
/// `default` when unset or unreadable. (`BO2MP_BO3_LOOK` is Black Ops II's.)
pub(super) fn game_on(var: &str, default: bool) -> bool {
    match codlegacy_env::var(var)
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("1" | "on" | "true" | "yes") => true,
        Some("0" | "off" | "false" | "no") => false,
        _ => default,
    }
}

/// Black Ops III's tone curve (`t7_tone` in the shader), for the load-time
/// look check: the light times the exposure to a display value 0..1.
pub(super) fn tone(c: f32, exposure: f32) -> f32 {
    let x = (c * exposure).max(0.0) + 0.00873;
    let t = (x.log2() * 0.072703 + 0.598206).clamp(0.0, 1.0);
    let p = (((7.712947 * t - 19.311527) * t + 14.275167) * t - 2.490045) * t + 0.878083;
    (p * t - 0.06691).clamp(0.0, 1.0)
}

/// What a map's light comes to on screen, measured from the numbers the
/// look was built from (no picture needed): the sun's and the traced
/// ambient's luminance, and where a mid-grey (albedo 0.35) and a white
/// surface (0.8) land in the display values, sunlit and in shade.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LookCheck {
    pub sun: f32,
    pub amb_up: f32,
    pub amb_side: f32,
    pub sun_over_shade: f32,
    pub grey_lit: f32,
    pub grey_shade: f32,
    pub white_lit: f32,
    pub warnings: Vec<String>,
}

/// The check: `sun_lum` the sun's light (luminance), `amb_up` / `amb_side`
/// the traced ambient on up and side faces, `exposure` the tone curve's.
pub(super) fn look_check(sun_lum: f32, amb_up: f32, amb_side: f32, exposure: f32) -> LookCheck {
    const GREY: f32 = 0.35;
    const WHITE: f32 = 0.8;
    let grey_lit = tone(GREY * (sun_lum + amb_up), exposure);
    let grey_shade = tone(GREY * amb_side, exposure);
    let white_lit = tone(WHITE * (sun_lum + amb_up), exposure);
    let sun_over_shade = (sun_lum + amb_up) / amb_side.max(1e-4);
    let mut warnings = Vec::new();
    // Ranges from Black Ops III's own maps' numbers (Skyjacked, Carrier):
    // outside them the picture is flat, crushed or blown, whatever the map.
    if !(0.5..=0.85).contains(&grey_lit) {
        warnings.push(format!("a mid-grey in the sun draws {grey_lit:.2} (0.5..0.85 expected): the exposure or the sun's level is off"));
    }
    if white_lit > 0.95 {
        warnings.push(format!("a sunlit white draws {white_lit:.2}: sunlit whites clip"));
    }
    if !(0.12..=0.6).contains(&grey_shade) {
        warnings.push(format!(
            "a mid-grey in shade draws {grey_shade:.2} (0.12..0.6 expected): shade is {}",
            if grey_shade < 0.12 { "crushed" } else { "washed" }
        ));
    }
    if !(1.8..=8.0).contains(&sun_over_shade) {
        warnings.push(format!(
            "sun over shade {sun_over_shade:.1} (1.8..8 expected): {}",
            if sun_over_shade < 1.8 { "flat, no readable shadows" } else { "harsh, black shadows" }
        ));
    }
    LookCheck { sun: sun_lum, amb_up, amb_side, sun_over_shade, grey_lit, grey_shade, white_lit, warnings }
}

/// The exposure at which a mid-grey (albedo 0.35) in the sun draws `target`
/// (0..1) on Black Ops III's tone curve, for a sun and an up-face ambient
/// of the given luminance: the map's own light picks the exposure instead
/// of a fixed constant. The curve is monotonic, so it is found by bisection
/// (`None` when no light is there to expose).
pub(super) fn solve_exposure(sun_lum: f32, amb_up: f32, target: f32) -> Option<f32> {
    let lit = 0.35 * (sun_lum + amb_up);
    if !(lit.is_finite() && lit > 1e-4) {
        return None;
    }
    let (mut lo, mut hi) = (0.02f32, 64.0f32);
    if tone(lit, hi) < target || tone(lit, lo) > target {
        return None;
    }
    for _ in 0..40 {
        let mid = (lo * hi).sqrt();
        if tone(lit, mid) < target { lo = mid } else { hi = mid }
    }
    Some((lo * hi).sqrt())
}

/// How far over its shade a map's sun is drawn (the sun's light over the
/// shaded baked light's level, both in the baked light's linear units), from
/// the map's own numbers: the sun's HDR peak (`sun_peak`, the ComWorld's)
/// over the baked light's whole shaded level before it is scaled
/// (`shade_raw`: the upper page plus 0.8 of the directional one,
/// `fastfile_iw6::map::baked_light`). The sun is compressed by Ghosts' tone
/// mapping against the baked light, so the ratio grows with the 0.35 power of
/// that quotient. INFERRED: the exponent is a choice and the anchor is the
/// old fixed 2.6 over a shaded level of 1.126 (2.31), which was set by eye on
/// Prison Break (sun 9 over a shaded level of 0.253: a quotient of 35.6);
/// the result is clamped to 1.07..3.55 (the old 3..10 times 0.4 / 1.126), and
/// the look check below it says whether the picture came out in range.
pub(super) fn map_sun_ratio(sun_peak: f32, shade_raw: f32) -> f32 {
    const ANCHOR_RATIO: f32 = 2.31;
    const ANCHOR_QUOTIENT: f32 = 35.6;
    let q = (sun_peak.max(1e-3) / shade_raw.max(1e-3)) / ANCHOR_QUOTIENT;
    (ANCHOR_RATIO * q.powf(0.35)).clamp(1.07, 3.55)
}

/// The traced ambient's mean luminance over the cells that hold light:
/// (up face, mean of the four side faces).
pub(super) fn grid_levels(g: &super::rt_ambient::Grid) -> Option<(f32, f32)> {
    let lum = |c: &[f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (mut up, mut side, mut n) = (0.0f64, 0.0f64, 0usize);
    for cube in &g.cubes {
        let total: f32 = cube.iter().map(lum).sum();
        if total <= 1e-5 {
            continue;
        }
        up += f64::from(lum(&cube[4]));
        side += f64::from(cube[..4].iter().map(lum).sum::<f32>() / 4.0);
        n += 1;
    }
    (n > 0).then(|| ((up / n as f64) as f32, (side / n as f64) as f32))
}

/// How far, round the spawn points' square, the sun map reaches.
const MARGIN: f32 = 3000.0;

fn env_scale(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

/// What a material does to the sun: none, partly (alpha tested), fully.
fn blocks(d: Option<&T6Draw>) -> Option<bool> {
    let d = d?;
    if d.unlit || d.effect || d.cloud {
        return None;
    }
    match d.blend {
        T6Blend::Opaque => Some(d.alpha_test),
        T6Blend::ShadowOnly => Some(false),
        _ => None,
    }
}

/// Turns Black Ops III's look on for a Black Ops II world: bakes the sun
/// map over the spawn points' square from the world's and placed models'
/// triangles and writes the branch's rows into the light table. `sun`:
/// toward the sun; `sun_colour`: its light. (The exposure is the map's as
/// it moves between its exposure volumes: the shader takes Black Ops II's.)
pub(super) fn apply(
    draw: &mut WorldDraw,
    catalog: &MaterialCatalog,
    meshes: &[ModelMesh],
    placements: &[Option<StaticModelPlacement>],
    spawns: &[[f32; 3]],
    sun: [f32; 3],
    sun_colour: [f32; 3],
    exposure: Option<f32>,
    rt: &super::rt_ambient::Options,
    report: &mut Vec<String>,
) {
    apply_except(draw, catalog, meshes, placements, spawns, sun, sun_colour, exposure, None, rt, &std::collections::HashSet::new(), report);
}

/// `apply`, with materials (catalog indices) that cast no shadow: Ghosts'
/// `nocast` techniques (a tree's leaf cards, grass), which the sun map and
/// the probe trace leave out. `auto_grey`: the display value a mid-grey in
/// the sun is to draw; the exposure is then solved from the sun and the
/// traced ambient (`solve_exposure`) instead of taken from `exposure`.
pub(super) fn apply_except(
    draw: &mut WorldDraw,
    catalog: &MaterialCatalog,
    meshes: &[ModelMesh],
    placements: &[Option<StaticModelPlacement>],
    spawns: &[[f32; 3]],
    sun: [f32; 3],
    sun_colour: [f32; 3],
    exposure: Option<f32>,
    auto_grey: Option<f32>,
    rt: &super::rt_ambient::Options,
    no_cast: &std::collections::HashSet<usize>,
    report: &mut Vec<String>,
) {
    let started = std::time::Instant::now();
    // The rows must be free (no primary light there).
    const ROWS: [usize; 4] = [super::t7_light::T7_FOG_ROW, super::t7_light::T7_FOG_ROW + 1, asset_world::T7_LIGHT_ROW, asset_world::T7_LIGHT_ROW + 1];
    if draw.t6_lights.len() < asset_world::T7_LIGHT_ROW + 2 {
        draw.t6_lights.resize(asset_world::T7_LIGHT_ROW + 2, [0.0; 16]);
    }
    if ROWS.iter().any(|&r| draw.t6_lights[r].iter().any(|v| *v != 0.0)) {
        report.push("bo3 look: off (the light table's rows 252..255 hold primary lights)".to_owned());
        return;
    }
    // The spawn points' square (else the world's bounds).
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in spawns {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if lo[0] > hi[0] {
        lo = [draw.stats.min[0], draw.stats.min[1]];
        hi = [draw.stats.max[0], draw.stats.max[1]];
    }
    let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
    let span = ((hi[0] - lo[0]).max(hi[1] - lo[1]) + MARGIN).clamp(2048.0, 16000.0);
    let half = span * 0.5 + MARGIN;
    let inside = |p: [f32; 3]| (p[0] - centre[0]).abs() <= half && (p[1] - centre[1]).abs() <= half;
    let material_draw = |m: Option<usize>| m.and_then(|m| catalog.materials.get(m)).and_then(|m| m.t6_draw.as_ref());
    let casts = |m: Option<usize>| if m.is_some_and(|m| no_cast.contains(&m)) { None } else { blocks(material_draw(m)) };
    let rt_free = super::rt_ambient::row_free(draw);
    let rt_wanted = rt_free && super::rt_ambient::wanted(rt) && !spawns.is_empty();
    let rt_local = super::rt_ambient::local_wanted(rt);
    let report_from = report.len();
    // Kept from an earlier load of the same map, sun, sky and settings?
    let cache_key = super::rt_cache::on().then(|| {
        let class = |m: Option<usize>| match casts(m) {
            None => 0u8,
            Some(false) => 1,
            Some(true) => 2,
        };
        let inputs = super::rt_cache::Inputs { spawns, sun, sun_colour, exposure, auto_grey, sky: rt.sky, rt_free, rt_wanted, rt_local, material_class: &class };
        super::rt_cache::key(draw, meshes, placements, &inputs)
    });
    if let Some(key) = &cache_key
        && let Some(hit) = super::rt_cache::load(key)
    {
        draw.t6_lut = Some(hit.lut);
        for (row, v) in hit.rows {
            if draw.t6_lights.len() <= row {
                draw.t6_lights.resize(row + 1, [0.0; 16]);
            }
            draw.t6_lights[row] = v;
        }
        draw.t7_light = true;
        report.push(format!("bo3 look: read from the cache ({key}) in {:.2}s; the lines below are from the load that baked it", started.elapsed().as_secs_f32()));
        report.extend(hit.report);
        return;
    }
    // The tree takes the solid triangles over a small area: a props' detail
    // (millions of triangles on Carrier and Ghosts maps) lights nothing the
    // probes' spacing could show, and the tree's build is the cost.
    let min_area = env_scale("BO2MP_RT_MIN_AREA", 40.0);
    let area = |p: &[[f32; 3]; 3]| {
        let (a, b) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
        let c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    };
    // The triangles the tree takes, gathered with the casters.
    let mut solid: Vec<[[f32; 3]; 3]> = Vec::new();
    // The world's triangles.
    let mut casters: Vec<Caster> = Vec::new();
    for (s, &(start, count)) in draw.surface_index_ranges.iter().enumerate() {
        let Some(partial) = casts(draw.surface_materials.get(s).copied().flatten()) else { continue };
        let idx = &draw.packed_indices[start as usize..(start + count) as usize];
        for t in idx.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| draw.positions.get(t[k] as usize).copied().unwrap_or_default());
            if p.iter().any(|v| inside(*v)) {
                if rt_wanted && !partial && area(&p) >= min_area {
                    solid.push(p);
                }
                casters.push(Caster { p, partial });
            }
        }
    }
    let world_tris = casters.len();
    // The placed models' (their first LOD), placed. Each mesh is read once
    // (its surfaces' positions and indices); the placements, thousands of
    // them over a few hundred meshes, are transformed on every core.
    struct Local<'a> {
        partial: bool,
        pos: &'a [[f32; 3]],
        ix: Vec<u32>,
    }
    let placed: Vec<&StaticModelPlacement> = placements.iter().flatten().filter(|pl| inside(pl.origin) && pl.mesh < meshes.len()).collect();
    let mut used = vec![false; meshes.len()];
    for pl in &placed {
        used[pl.mesh] = true;
    }
    let locals: Vec<Vec<Local>> = meshes
        .iter()
        .zip(&used)
        .map(|(mesh, &used)| {
            let mut out = Vec::new();
            if !used {
                return out;
            }
            for surf in mesh.surfaces() {
                let Some(partial) = casts(surf.material) else { continue };
                let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) = surf.mesh.attribute(bevy::prelude::Mesh::ATTRIBUTE_POSITION) else { continue };
                let Some(indices) = surf.mesh.indices() else { continue };
                out.push(Local { partial, pos: pos.as_slice(), ix: indices.iter().map(|i| i as u32).collect() });
            }
            out
        })
        .collect();
    let parts = fastfile_t7::bake::parallel_chunks(placed.len(), 128, |range| {
        let (mut c, mut s, mut world): (Vec<Caster>, Vec<[[f32; 3]; 3]>, Vec<[f32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
        for pl in &placed[range] {
            for l in &locals[pl.mesh] {
                world.clear();
                world.extend(l.pos.iter().map(|v| pl.transform.transform_point(bevy::prelude::Vec3::from_array(*v)).to_array()));
                for t in l.ix.chunks_exact(3) {
                    if let (Some(a), Some(b), Some(d)) = (world.get(t[0] as usize), world.get(t[1] as usize), world.get(t[2] as usize)) {
                        let p = [*a, *b, *d];
                        if rt_wanted && !l.partial && area(&p) >= min_area {
                            s.push(p);
                        }
                        c.push(Caster { p, partial: l.partial });
                    }
                }
            }
        }
        (c, s)
    });
    casters.reserve(parts.iter().map(|p| p.0.len()).sum());
    solid.reserve(parts.iter().map(|p| p.1.len()).sum());
    for (c, s) in parts {
        casters.extend(c);
        solid.extend(s);
    }
    let casters_time = started.elapsed().as_secs_f32();
    let z = spawns.iter().map(|p| p[2]).sum::<f32>() / spawns.len().max(1) as f32;
    let sun_started = std::time::Instant::now();
    let map = SunMap::render(&casters, sun, [centre[0], centre[1], z], span * 1.15, 4096);
    let s = map.depth.size;
    let mut lut = map.texels();
    let sun_time = sun_started.elapsed().as_secs_f32();
    // The ambient rebake (`rt_ambient`): our own sun and sky traced at load
    // over the play area, its grid in rows of the sun map's picture below it.
    let mut lut_height = s;
    let mut rt_grid = None;
    if rt_free {
        if rt_wanted
            && let Some(params) = super::rt_ambient::params(rt, sun, sun_colour)
        {
            let t = std::time::Instant::now();
            let tracer = Tracer::new(&solid);
            let tree_time = t.elapsed().as_secs_f32();
            report.push(format!("rt ambient: tree over {} solid triangles of {}, {tree_time:.1}s", solid.len(), casters.len()));
            const GRID_MARGIN: f32 = 384.0;
            let zmin = spawns.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
            let zmax = spawns.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
            let lo = [lo[0] - GRID_MARGIN, lo[1] - GRID_MARGIN, zmin - 320.0];
            let hi = [hi[0] + GRID_MARGIN, hi[1] + GRID_MARGIN, zmax + 768.0];
            let grid = super::rt_ambient::bake(&tracer, lo, hi, &params);
            super::rt_ambient::dump(&grid);
            match grid.pack(s as usize) {
                Some((px, rows, per_row, scale)) => {
                    let y0 = lut_height as usize;
                    lut.extend_from_slice(&px);
                    lut_height += rows as u32;
                    report.push(format!(
                        "rt ambient: on: {}x{}x{} probes of {:.0} units ({} buried, filled), {} rays each, sky {:.2?} / {:.2?}, albedo {}, picture rows {y0}..{}, {tree_time:.1}s tree + {:.1}s traced",
                        grid.dims[0],
                        grid.dims[1],
                        grid.dims[2],
                        grid.cell,
                        grid.filled,
                        params.rays,
                        params.sky.zenith,
                        params.sky.horizon,
                        params.albedo,
                        lut_height,
                        t.elapsed().as_secs_f32() - tree_time
                    ));
                    rt_grid = Some((grid, y0, per_row, scale));
                }
                None => report.push("rt ambient: off (the grid is wider than the sun map's picture)".to_owned()),
            }
        }
        let mix = env_scale("BO2MP_RT_AMBIENT_MIX", 0.85).min(1.0);
        super::rt_ambient::write_rows(draw, rt_grid.as_ref().map(|(g, y0, per_row, scale)| (g, *y0, *per_row, *scale, s as usize)), mix, rt_local);
        if rt_local {
            report.push("rt lights: the map's local lights and effect lights light the branch".to_owned());
        }
    } else if super::rt_ambient::wanted(rt) || rt_local {
        report.push(format!("rt ambient: off (the light table's row {} holds a primary light)", super::rt_ambient::ROW));
    }
    draw.t6_lut = Some(std::sync::Arc::new(asset_world::T6Lut { width: s, height: lut_height, rgba: lut }));
    // The branch's rows (`t7_light::Lit::light_rows`): exposure, the sun's
    // light over the sun colour drawn (1: Black Ops II's own), the sun
    // map's axes and packing, the test view, and the Black Ops II flag.
    let m = map.rows();
    let mut user = exposure.unwrap_or_else(|| env_scale("BO2MP_BO3_EXPOSURE", 1.2)).max(0.01);
    if let Some(target) = auto_grey {
        let sun_lum = 0.2126 * sun_colour[0] + 0.7152 * sun_colour[1] + 0.0722 * sun_colour[2];
        match rt_grid.as_ref().and_then(|(g, ..)| grid_levels(g)) {
            Some((up, side)) => match solve_exposure(sun_lum, up, target) {
                Some(e) => {
                    report.push(format!(
                        "exposure: x{e:.3} solved from the map's light (sun {sun_lum:.2}, traced ambient up {up:.2} / side {side:.2}) so a mid-grey in the sun draws {target:.2} (was x{user:.2}; BO2MP_*_EXPOSURE fixes it)"
                    ));
                    user = e.clamp(0.05, 16.0);
                }
                None => report.push(format!("exposure: x{user:.2} kept (no light to solve from: sun {sun_lum:.3}, ambient up {up:.3})")),
            },
            None => report.push(format!("exposure: x{user:.2} kept (the traced ambient is off, so the map's light cannot be measured)")),
        }
    }
    let mut a = [0f32; 16];
    a[0] = user;
    a[1] = 1.0;
    a[3] = 7.0;
    for k in 0..3 {
        a[4 + 4 * k..8 + 4 * k].copy_from_slice(&m[k]);
    }
    let mut b = [0f32; 16];
    b[..4].copy_from_slice(&m[3]);
    b[4] = super::t7_light::view();
    b[6] = 1.0;
    b[7] = 7.0;
    draw.t6_lights[asset_world::T7_LIGHT_ROW] = a;
    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1] = b;
    // The shafts (`t7_fog_core` with its atmosphere off: Black Ops II's
    // own fog stays): the haze toward the sun, the sun's light times a
    // small density, shadowed by the sun map along the ray.
    let shafts = env_scale("BO2MP_T7_SHAFTS", 1.0);
    let haze = env_scale("BO2MP_T7_HAZE", 1.0) * 0.06;
    let hc = sun_colour.map(|c| c * haze);
    let fog: [[f32; 4]; 8] = [
        [0.0, 0.0, 0.0, 0.0],
        [hc[0], hc[1], hc[2], 1.0],
        [0.0, 0.0, 0.0, 65000.0],
        [sun[0], sun[1], sun[2], 0.6],
        [1.0, 1.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [1.0 / 2000.0, 2500.0, if shafts > 0.0 { 16.0 } else { 0.0 }, shafts],
    ];
    for (i, r) in fog.iter().enumerate() {
        let row = super::t7_light::T7_FOG_ROW + i / 4;
        draw.t6_lights[row][4 * (i % 4)..4 * (i % 4) + 4].copy_from_slice(r);
    }
    draw.t7_light = true;
    // The automatic check (`BO2MP_LOOK_CHECK=0` leaves it out): where the
    // map's numbers put a grey and a white on screen, warned when outside
    // what Black Ops III's own maps reach.
    if let Some((g, ..)) = &rt_grid
        && std::env::var("BO2MP_LOOK_CHECK").map_or(true, |v| v.trim() != "0")
        && let Some((up, side)) = grid_levels(g)
    {
        let sun_lum = (0.2126 * sun_colour[0] + 0.7152 * sun_colour[1] + 0.0722 * sun_colour[2]) * a[1].max(0.0);
        let c = look_check(sun_lum, up, side, a[0]);
        report.push(format!(
            "look check: sun {:.2}, traced ambient up {:.2} / side {:.2}, sun over shade {:.1}; mid-grey 0.35 draws {:.2} in sun, {:.2} in shade, white 0.8 {:.2} in sun (exposure x{:.2})",
            c.sun, c.amb_up, c.amb_side, c.sun_over_shade, c.grey_lit, c.grey_shade, c.white_lit, a[0]
        ));
        for w in &c.warnings {
            report.push(format!("look check WARNING: {w}"));
        }
    }
    report.push(format!(
        "bo3 look: on (BO2MP_BO3_LOOK=1): sun toward {sun:.3?} colour {sun_colour:.2?}, exposure x{:.2}, sun map {s}px over {:.0} units from {} triangles ({world_tris} world, {} placed; {} partial), {:.1}s (casters {casters_time:.1}s, sun map {sun_time:.1}s)",
        a[0],
        span * 1.15,
        casters.len(),
        casters.len() - world_tris,
        casters.iter().filter(|c| c.partial).count(),
        started.elapsed().as_secs_f32()
    ));
    // Kept for the next load (written off this thread).
    if let (Some(key), Some(lut)) = (cache_key, draw.t6_lut.clone()) {
        let mut rows: Vec<usize> = (super::t7_light::T7_FOG_ROW..super::t7_light::T7_FOG_ROW + 2).chain([asset_world::T7_LIGHT_ROW, asset_world::T7_LIGHT_ROW + 1]).collect();
        if rt_free {
            rows.push(super::rt_ambient::ROW);
        }
        let rows = rows.into_iter().map(|r| (r, draw.t6_lights[r])).collect();
        super::rt_cache::store(key, super::rt_cache::Baked { lut, rows, report: report[report_from..].to_vec() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_is_monotonic_and_bounded() {
        let mut last = -1.0;
        for i in 0..200 {
            let v = tone(i as f32 * 0.05, 1.0);
            assert!((0.0..=1.0).contains(&v) && v >= last - 1e-6);
            last = v;
        }
    }

    #[test]
    fn the_solved_exposure_puts_a_grey_in_the_sun_where_asked() {
        for (sun, up) in [(1.1f32, 0.35f32), (2.6, 0.8), (0.4, 0.2), (6.0, 1.0)] {
            let e = solve_exposure(sun, up, 0.65).expect("light to solve from");
            let got = tone(0.35 * (sun + up), e);
            assert!((got - 0.65).abs() < 0.01, "sun {sun} up {up}: exposure {e} draws {got}");
        }
        // A brighter sun needs less exposure; none at all has nothing to solve.
        assert!(solve_exposure(6.0, 1.0, 0.65) < solve_exposure(1.1, 0.35, 0.65));
        assert_eq!(solve_exposure(0.0, 0.0, 0.65), None);
    }

    #[test]
    fn the_sun_ratio_follows_the_map_and_stays_in_range() {
        // The anchor (Prison Break: sun 9 over a shaded level of 0.253) is
        // the old 2.6 over 1.126.
        assert!((map_sun_ratio(9.0, 0.253) - 2.31).abs() < 0.02);
        // mp_frag (sun 8 over 0.432, an overcast snow field) is flatter.
        let frag = map_sun_ratio(8.0, 0.432);
        assert!(frag < 2.31 && frag > 1.5, "{frag}");
        // A dim sun over a bright shade (an overcast or interior map) is flatter,
        // a strong sun over a dark shade harsher, within 1.07..3.55.
        assert!(map_sun_ratio(0.6, 0.03) < 2.31 && map_sun_ratio(0.6, 0.03) >= 1.07);
        assert!(map_sun_ratio(10.0, 0.01) > 2.31 && map_sun_ratio(10.0, 0.01) <= 3.55);
        assert_eq!(map_sun_ratio(0.01, 5.0), 1.07);
    }

    #[test]
    fn a_black_ops_iii_like_map_passes_and_bad_ones_warn() {
        // Skyjacked-like numbers: sun luminance ~1.1 at x1.6, sky a third.
        let ok = look_check(1.1, 0.35, 0.3, 1.6);
        assert!(ok.warnings.is_empty(), "{:?}", ok.warnings);
        // Hazy overcast: the sun hardly over the shade.
        let flat = look_check(0.3, 0.45, 0.45, 1.0);
        assert!(flat.warnings.iter().any(|w| w.contains("flat")), "{:?}", flat.warnings);
        // Sun far over the shade.
        let harsh = look_check(3.0, 0.05, 0.03, 1.0);
        assert!(harsh.warnings.iter().any(|w| w.contains("harsh") || w.contains("crushed")), "{:?}", harsh.warnings);
    }
}

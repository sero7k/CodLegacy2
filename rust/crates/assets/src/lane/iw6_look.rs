//! bo2mp: what a Ghosts map needs to be drawn by Black Ops III's lit branch
//! beyond its sun and ambient (`super::iw6_map`, `super::bo3_look`).
//!
//! **A reflection probe.** The branch's normal and specular maps (and a
//! prop's) are only bound when the map has a probe cube array: the draw
//! pass builds the "shine" bind groups from it, and a surface without one is
//! drawn with its colour map alone. Ghosts' own probes (`GfxWorld`'s
//! reflection probes) are not read, so every Ghosts map was drawn flat: no
//! bumps, no highlights, no reflections. This builds a probe from the sky the
//! map is lit by (zenith, horizon, a dark ground) as Infinite Warfare's
//! probes are given (`iw7::link_probes`: each normalised to a mean of one,
//! because the shader scales a reflection by the light where it is seen,
//! `so` in `t7_shade`). `BO2MP_IW6_PROBE=0` leaves it out.

use asset_material::MaterialCatalog;

/// The probe's size and levels (a smooth sky needs little).
pub(super) const PROBE_SIZE: u32 = 32;
pub(super) const PROBE_LEVELS: u32 = 6;

/// Whether Ghosts maps get the sky probe (on by default).
pub(super) fn probe_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_PROBE", true)
}

/// The sky the map is lit by: (zenith, horizon), linear. The horizon is the
/// fog's colour squared (the fog's colours are gamma values) or a neutral
/// blue-grey; the zenith a touch bluer. The real-time ambient traces the
/// same sky (`bo3_look::apply_except`'s `rt_ambient::Options::sky`).
pub(super) fn sky_colours(fog_colour: Option<[f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let horizon = fog_colour.map(|c| c.map(|v| v * v)).unwrap_or([0.6, 0.65, 0.7]);
    ([horizon[0] * 0.85, horizon[1] * 0.95, horizon[2] * 1.15], horizon)
}

/// The sky's radiance toward a direction (z up): the horizon colour at the
/// horizon rising to the zenith's overhead, falling to a dark ground (a
/// fifth of the horizon, a little warmer: light bounced off the floor).
fn sky_toward(d: [f32; 3], zenith: [f32; 3], horizon: [f32; 3]) -> [f32; 3] {
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
    let z = d[2] / len;
    if z >= 0.0 {
        let t = z.powf(0.6);
        std::array::from_fn(|k| horizon[k] + (zenith[k] - horizon[k]) * t)
    } else {
        let ground = [horizon[0] * 0.22, horizon[1] * 0.2, horizon[2] * 0.18];
        let t = (-z * 2.5).min(1.0);
        std::array::from_fn(|k| horizon[k] + (ground[k] - horizon[k]) * t)
    }
}

/// The direction of a cube face's texel (`sc`, `tc` in -1..1), D3D / Vulkan
/// order: +X, -X, +Y, -Y, +Z, -Z.
fn face_direction(face: usize, sc: f32, tc: f32) -> [f32; 3] {
    match face {
        0 => [1.0, -tc, -sc],
        1 => [-1.0, -tc, sc],
        2 => [sc, 1.0, tc],
        3 => [sc, -1.0, -tc],
        4 => [sc, -tc, 1.0],
        _ => [-sc, -tc, -1.0],
    }
}

/// The probe's texels: six faces, each its `PROBE_LEVELS` levels one after
/// another, RGBA8 as Infinite Warfare's probes (`rgb / m`, alpha `1 / m`, `m`
/// the brightest channel over one), the whole scaled to a mean luminance of
/// one over the sphere. Returns the data and the scale applied (the sky's
/// mean luminance before it).
pub(super) fn sky_probe_data(zenith: [f32; 3], horizon: [f32; 3]) -> (Vec<u8>, f32) {
    let lum = |c: &[f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    // The mean over the sphere (a solid-angle weight per texel of the top level).
    let n = PROBE_SIZE as usize;
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let (sc, tc) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
                let w = f64::from((1.0 + sc * sc + tc * tc).powf(-1.5));
                sum += w * f64::from(lum(&sky_toward(face_direction(face, sc, tc), zenith, horizon)));
                weight += w;
            }
        }
    }
    let mean = (sum / weight.max(1e-9)) as f32;
    let k = if mean > 1e-5 { 1.0 / mean } else { 1.0 };
    let mut data = Vec::new();
    for face in 0..6 {
        for level in 0..PROBE_LEVELS {
            let d = (PROBE_SIZE >> level).max(1) as usize;
            for y in 0..d {
                for x in 0..d {
                    let (sc, tc) = ((x as f32 + 0.5) / d as f32 * 2.0 - 1.0, (y as f32 + 0.5) / d as f32 * 2.0 - 1.0);
                    let c = sky_toward(face_direction(face, sc, tc), zenith, horizon).map(|v| v * k);
                    let m = c[0].max(c[1]).max(c[2]).clamp(1.0, 255.0);
                    data.extend_from_slice(&[
                        ((c[0] / m).min(1.0) * 255.0).round() as u8,
                        ((c[1] / m).min(1.0) * 255.0).round() as u8,
                        ((c[2] / m).min(1.0) * 255.0).round() as u8,
                        (255.0 / m).round().max(1.0) as u8,
                    ]);
                }
            }
        }
    }
    (data, mean)
}

/// Links the sky probe into the catalog as the map's probe cube array and
/// returns the probes' lighting table (a flat light of one each, as
/// Infinite Warfare's) and the sky's mean luminance before normalising.
pub(super) fn link_sky_probe(
    catalog: &mut MaterialCatalog,
    owner: asset_core::ZoneOwner,
    name: &str,
    zenith: [f32; 3],
    horizon: [f32; 3],
) -> (Vec<[[f32; 4]; 3]>, f32) {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::Image;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    let (data, mean) = sky_probe_data(zenith, horizon);
    let mut cube = Image::new_uninit(
        Extent3d { width: PROBE_SIZE, height: PROBE_SIZE, depth_or_array_layers: 6 },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    cube.texture_descriptor.mip_level_count = PROBE_LEVELS;
    cube.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::CubeArray), ..Default::default() });
    cube.data = Some(data);
    cube.sampler = super::t7_light::clamped();
    let semantic = super::t6_materials::T6_PROBES_SEMANTIC;
    let slot = super::t7_light::link_image(catalog, format!("iw6:{name}#sky_probe"), PROBE_SIZE, PROBE_LEVELS, semantic, cube);
    catalog.link_material(super::t7_light::material(super::t6_materials::T6_PROBES_MATERIAL.to_owned(), semantic, slot, owner, None));
    (vec![[[1.0, 1.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0; 4]]], mean)
}

/// Whether Ghosts maps get their own reflection probes in place of the sky
/// probe (`BO2MP_IW6_OWN_PROBES=1`; off by default until seen on screen).
pub(super) fn own_probes_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_OWN_PROBES", false)
}

/// The most probes the draw pass's cube array and table hold.
pub(super) const MAX_PROBES: usize = 32;

/// A map's own probes as linked: the table rows, each zone probe's slot in
/// the cube array (`None`: not kept), and the one report line.
#[derive(Clone)]
pub(super) struct MapProbes {
    pub table: Vec<[[f32; 4]; 3]>,
    pub slot_of: Vec<Option<u8>>,
    pub origins: Vec<[f32; 3]>,
    pub line: String,
}

impl MapProbes {
    /// The slot for a zone probe index, else the kept probe nearest `at`
    /// (never slot 0, the neutral default, while another is kept).
    pub fn slot(&self, index: usize, set: &fastfile_iw6::probe::ProbeSet, at: [f32; 3]) -> u8 {
        if let Some(Some(s)) = self.slot_of.get(index) {
            return *s;
        }
        if let Some((i, _)) = set.pick(at)
            && let Some(Some(s)) = self.slot_of.get(i)
        {
            return *s;
        }
        let skip = usize::from(self.origins.len() > 1);
        self.origins
            .iter()
            .enumerate()
            .skip(skip)
            .min_by(|a, b| {
                let d = |o: &[f32; 3]| (0..3).map(|k| (o[k] - at[k]).powi(2)).sum::<f32>();
                d(a.1).total_cmp(&d(b.1))
            })
            .map_or(0, |(i, _)| i as u8)
    }
}

/// The texels of one kept probe at the array's `size` and `levels`: each
/// face's levels from the one `size` wide (a larger probe drops its top
/// levels), scaled to a mean luminance of one, as half floats with alpha 1.
/// `None` when the probe is smaller than the array or its chain is short.
pub(super) fn probe_layer(cube: &fastfile_iw6::probe::ProbeCube, size: u32, levels: u32) -> Option<(Vec<u8>, f32)> {
    let skip = (cube.size / size.max(1)).checked_ilog2()?;
    if cube.size != size << skip || cube.levels < skip + levels {
        return None;
    }
    // The mean over the sphere at the array's top level.
    let n = size as usize;
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for face in 0..6 {
        let l = cube.level(face, skip);
        for y in 0..n {
            for x in 0..n {
                let (sc, tc) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
                let w = f64::from((1.0 + sc * sc + tc * tc).powf(-1.5));
                let c = l[y * n + x];
                sum += w * f64::from(0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]);
                weight += w;
            }
        }
    }
    let mean = (sum / weight.max(1e-9)) as f32;
    let k = if mean > 1e-5 { 1.0 / mean } else { 1.0 };
    let mut data = Vec::new();
    for face in 0..6 {
        for level in skip..skip + levels {
            for c in cube.level(face, level) {
                for v in c {
                    data.extend_from_slice(&super::t7_map::f16_bits((v * k).clamp(0.0, 60000.0)).to_le_bytes());
                }
                data.extend_from_slice(&super::t7_map::f16_bits(1.0).to_le_bytes());
            }
        }
    }
    Some((data, mean))
}

/// Links the map's own reflection probes (`GfxWorld`'s, `fastfile_iw6::probe`)
/// as the map's probe cube array: the first `MAX_PROBES` of the size most of
/// them have (a larger one at that size from its smaller levels), each
/// normalised to a mean of one as the sky probe is (the shader re-lights a
/// reflection by the light where it is seen). `Err` with the reason when no
/// probe decodes.
pub(super) fn link_map_probes(
    cap: &fastfile_iw6::walk::Capture,
    zone_image: &[u8],
    set: &fastfile_iw6::probe::ProbeSet,
    catalog: &mut MaterialCatalog,
    owner: asset_core::ZoneOwner,
    name: &str,
) -> Result<MapProbes, String> {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::Image;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    let mut cubes = Vec::with_capacity(set.probes.len());
    let mut failed = Vec::new();
    for (i, p) in set.probes.iter().enumerate() {
        let cube = p
            .image
            .and_then(|id| cap.images.get(id.index as usize))
            .ok_or_else(|| format!("probe {i}: no image"))
            .and_then(|rec| fastfile_iw6::probe::probe_cube(rec, zone_image));
        match cube {
            Ok(c) => cubes.push(Some(c)),
            Err(e) => {
                failed.push(e);
                cubes.push(None);
            }
        }
    }
    // The array's size: the commonest (ties: the smaller), its levels the
    // fewest of the probes at that size.
    let mut sizes: Vec<(u32, usize)> = Vec::new();
    for c in cubes.iter().flatten() {
        match sizes.iter_mut().find(|s| s.0 == c.size) {
            Some(s) => s.1 += 1,
            None => sizes.push((c.size, 1)),
        }
    }
    let size = sizes.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|s| s.0).ok_or_else(|| {
        format!("no probe decoded ({} probes; {})", set.probes.len(), failed.first().cloned().unwrap_or_default())
    })?;
    let levels = cubes.iter().flatten().filter(|c| c.size == size).map(|c| c.levels).min().unwrap_or(1);
    let mut data = Vec::new();
    let mut slot_of = vec![None; set.probes.len()];
    let mut table = Vec::new();
    let mut origins = Vec::new();
    let mut means = Vec::new();
    let mut resized = 0usize;
    for (i, c) in cubes.iter().enumerate() {
        if table.len() >= MAX_PROBES {
            break;
        }
        let Some(c) = c else { continue };
        let Some((layer, mean)) = probe_layer(c, size, levels) else { continue };
        resized += usize::from(c.size != size);
        data.extend_from_slice(&layer);
        slot_of[i] = Some(table.len() as u8);
        let o = set.probes[i].origin;
        // (Gain one; the origin; no parallax box: the volumes' planes bound
        // where a probe serves, not the room it saw.)
        table.push([[1.0, o[0], o[1], o[2]], [0.0, 0.0, 0.0, 1.0], [0.0; 4]]);
        origins.push(o);
        means.push(mean);
    }
    if table.is_empty() {
        return Err(format!("no probe at {size}px kept"));
    }
    let mut cube = Image::new_uninit(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 * table.len() as u32 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    cube.texture_descriptor.mip_level_count = levels;
    cube.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::CubeArray), ..Default::default() });
    cube.data = Some(data);
    cube.sampler = super::t7_light::clamped();
    let semantic = super::t6_materials::T6_PROBES_SEMANTIC;
    let slot = super::t7_light::link_image(catalog, format!("iw6:{name}#probes"), size, levels, semantic, cube);
    catalog.link_material(super::t7_light::material(super::t6_materials::T6_PROBES_MATERIAL.to_owned(), semantic, slot, owner, None));
    let volumes: usize = set.probes.iter().map(|p| p.volumes.len()).sum();
    let (lo, hi) = means.iter().skip(usize::from(means.len() > 1)).fold((f32::INFINITY, 0f32), |a, &m| (a.0.min(m), a.1.max(m)));
    let line = format!(
        "iw6 probe: the map's own {} of {} reflection probes ({size}px, {levels} levels, {resized} from a larger cube; each to a mean of 1, the captured probes' own means {lo:.3}..{hi:.3}), {volumes} volumes, {} references (not used); surfaces and placed models take their stored probe, the rest the stored probe of the nearest placed model or surface (BO2MP_IW6_OWN_PROBES=1){}",
        table.len(),
        set.probes.len(),
        set.references.len(),
        if failed.is_empty() { String::new() } else { format!("; {} not decoded (first: {})", failed.len(), failed[0]) }
    );
    Ok(MapProbes { table, slot_of, origins, line })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texel (face, x, y) of the top level, decoded as the shader does
    /// (`rgb / a`), as a luminance.
    fn decoded(data: &[u8], face: usize, x: usize, y: usize) -> f32 {
        let per_face: usize = (0..PROBE_LEVELS).map(|l| ((PROBE_SIZE >> l).max(1) as usize).pow(2) * 4).sum();
        let at = face * per_face + (y * PROBE_SIZE as usize + x) * 4;
        let t = &data[at..at + 4];
        let a = f32::from(t[3]) / 255.0;
        let c = [0, 1, 2].map(|k| f32::from(t[k]) / 255.0 / a);
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    #[test]
    fn the_probe_has_a_mean_of_one_and_a_sky_above_a_ground() {
        let (zenith, horizon) = sky_colours(Some([0.78, 0.8, 0.86]));
        let (data, mean) = sky_probe_data(zenith, horizon);
        let per_face: usize = (0..PROBE_LEVELS).map(|l| ((PROBE_SIZE >> l).max(1) as usize).pow(2) * 4).sum();
        assert_eq!(data.len(), 6 * per_face);
        assert!(mean > 0.3 && mean < 1.0, "{mean}");
        // Mean luminance over the sphere (solid-angle weighted): one, within
        // the 8-bit steps.
        let n = PROBE_SIZE as usize;
        let (mut sum, mut weight) = (0.0f64, 0.0f64);
        for face in 0..6 {
            for y in 0..n {
                for x in 0..n {
                    let (sc, tc) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
                    let w = f64::from((1.0 + sc * sc + tc * tc).powf(-1.5));
                    sum += w * f64::from(decoded(&data, face, x, y));
                    weight += w;
                }
            }
        }
        let got = sum / weight;
        assert!((got - 1.0).abs() < 0.04, "mean {got}");
        // +Z (up) is the sky, -Z (down) the dark ground: z is the world's up.
        let up = decoded(&data, 4, n / 2, n / 2);
        let down = decoded(&data, 5, n / 2, n / 2);
        assert!(up > 1.0 && down < 0.5 * up, "up {up} down {down}");
        // On a side face the top rows look up and the bottom rows down (+X:
        // dir.z = -sc is positive on the left half; the rows are the
        // horizon's whatever).
        let left = decoded(&data, 0, 1, n / 2);
        let right = decoded(&data, 0, n - 2, n / 2);
        assert!(left > right, "left {left} right {right}");
    }

    #[test]
    fn a_probe_layer_is_normalised_and_a_larger_cube_drops_its_top_levels() {
        // A 4px cube of 3 levels, every texel 0.25 (a mean of 0.25).
        let face = 16 + 4 + 1;
        let big = fastfile_iw6::probe::ProbeCube { size: 4, levels: 3, dxgi_format: 10, texels: vec![[0.25; 3]; 6 * face] };
        let (data, mean) = probe_layer(&big, 4, 3).unwrap();
        assert!((mean - 0.25).abs() < 1e-4);
        assert_eq!(data.len(), 6 * face * 8);
        // Every texel scaled to one (half 1.0 = 0x3c00), alpha one.
        assert!(data.chunks_exact(2).all(|h| u16::from_le_bytes([h[0], h[1]]) == 0x3c00));
        // At 2px: its levels 1 and 2 only.
        let (small, _) = probe_layer(&big, 2, 2).unwrap();
        assert_eq!(small.len(), 6 * (4 + 1) * 8);
        // Smaller than the array, or too few levels: not kept.
        assert!(probe_layer(&big, 8, 1).is_none());
        assert!(probe_layer(&big, 2, 3).is_none());
    }

    #[test]
    fn the_sky_comes_from_the_fog_colour() {
        let (z, h) = sky_colours(Some([0.5, 0.5, 0.5]));
        assert!((h[0] - 0.25).abs() < 1e-6 && z[2] > h[2]);
        let (_, d) = sky_colours(None);
        assert_eq!(d, [0.6, 0.65, 0.7]);
    }
}

//! bo2zm: Black Ops II lighting a moving thing samples at run time (the
//! first-person gun and arms, effects): the map's light grid, the sky grid
//! volumes outside it, and the scale the props measured (the grid's
//! coefficient scale is not stored with it). Installed once per map load.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// What lights a point: the grid's light there (linear, before exposure,
/// times the measured scale), the primary light most of its corners carry
/// and that light's visibility (0..1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct T6PointLight {
    pub light: [f32; 3],
    pub primary: u8,
    pub visibility: f32,
}

pub struct T6LightSampler {
    pub grid: Option<asset_model::OwnedLightGrid>,
    /// Grid bounds in cells (x, y at 32 units, z at 64), for the in-grid
    /// test the props use.
    pub mins: [u16; 3],
    pub maxs: [u16; 3],
    /// Sky volumes: bounds, base light, primary light and visibility.
    pub sky: Vec<([f32; 3], [f32; 3], [f32; 3], u8, u8)>,
    pub scale: f32,
    pub sun_primary: u8,
    /// bo2zm M3 fix list 1: the map's collision, for the grid's sight
    /// checks: a grid point marked "needs trace" counts only when it can be
    /// seen from the lit point (BO2's R_LightGridLookup); without them a
    /// zombie near the ground took the light of the points under it (black).
    pub clip: Option<Arc<crate::ClipCollision>>,
    /// Sight-checked lights by 8-unit cell (the traces cost; a zombie's
    /// body and head, the gun and effects read the same cells frame after
    /// frame).
    pub cache: Mutex<HashMap<[i32; 3], T6PointLight>>,
    /// bo2mp: a map from another game under Black Ops II's rules (MW2, MW3,
    /// Black Ops, CoD4): its own grid read as Black Ops II light; `None` on
    /// a Black Ops II map. Before, such a map installed nothing: the gun and
    /// players read the last Black Ops II map's grid at this map's points
    /// (or no grid: black).
    pub foreign: Option<ForeignGridLight>,
    /// bo2mp: a foreign map without a light grid (Ghosts, Modern Warfare
    /// Remastered, Infinite Warfare, Black Ops III, Modern Warfare III): the
    /// light its placed models were given, read where the grid would be.
    pub prop_light: Option<Arc<PropLight>>,
}

/// bo2mp: the light a map's placed models carry (their origins and light,
/// in the light grid's unit), as the ambient light of what moves near them
/// on a map that has no light grid: a point takes the nearest ones it can
/// see, nearer ones more.
pub struct PropLight {
    cells: HashMap<[i32; 2], Vec<([f32; 3], [f32; 3])>>,
}

impl PropLight {
    /// Cell size (units, x and y).
    const CELL: f32 = 256.0;
    /// How far a model's light reaches.
    const REACH: f32 = 768.0;
    /// The most models read for one point.
    const NEAREST: usize = 6;

    fn cell(p: [f32; 3]) -> [i32; 2] {
        [(p[0] / Self::CELL).floor() as i32, (p[1] / Self::CELL).floor() as i32]
    }

    /// From the placed models' light (`origin`, packed sqrt(light / 32)
    /// rgb, alpha 255: lit by its vertices instead, left out). `None` when
    /// none carries light.
    pub fn from_packed(models: impl Iterator<Item = ([f32; 3], [u8; 4])>) -> Option<Self> {
        let mut cells: HashMap<[i32; 2], Vec<([f32; 3], [f32; 3])>> = HashMap::new();
        for (origin, packed) in models {
            if packed[3] == 255 {
                continue;
            }
            let light = [0, 1, 2].map(|c| {
                let v = f32::from(packed[c]) / 255.0;
                v * v * 32.0
            });
            // (Lifted off the ground it stands on, for the sight checks.)
            let at = [origin[0], origin[1], origin[2] + 16.0];
            cells.entry(Self::cell(at)).or_default().push((at, light));
        }
        (!cells.is_empty()).then_some(Self { cells })
    }

    /// The light at `pos`: the nearest models' (in sight when `clip` says,
    /// else the nearest), weighted by nearness; `None` with none in reach.
    fn at(&self, pos: [f32; 3], clip: Option<&crate::ClipCollision>) -> Option<[f32; 3]> {
        let c = Self::cell(pos);
        let span = (Self::REACH / Self::CELL).ceil() as i32;
        let mut near: Vec<(f32, [f32; 3], [f32; 3])> = Vec::new();
        for dx in -span..=span {
            for dy in -span..=span {
                for &(at, light) in self.cells.get(&[c[0] + dx, c[1] + dy]).into_iter().flatten() {
                    let d2: f32 = (0..3).map(|k| (at[k] - pos[k]).powi(2)).sum();
                    if d2 <= Self::REACH * Self::REACH {
                        near.push((d2, at, light));
                    }
                }
            }
        }
        if near.is_empty() {
            return None;
        }
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let seen: Vec<&(f32, [f32; 3], [f32; 3])> = match clip {
            Some(clip) => near
                .iter()
                .take(Self::NEAREST * 3)
                .filter(|(_, at, _)| clip.box_sight_clear(pos, *at, lighting_iw4::LIGHT_GRID_SIGHT_CONTENT_MASK))
                .take(Self::NEAREST)
                .collect(),
            None => Vec::new(),
        };
        let used: Vec<&(f32, [f32; 3], [f32; 3])> =
            if seen.is_empty() { near.iter().take(Self::NEAREST).collect() } else { seen };
        let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
        for (d2, _, light) in used {
            let w = 1.0 / (d2 + 48.0 * 48.0);
            for k in 0..3 {
                sum[k] += light[k] * w;
            }
            weight += w;
        }
        Some(sum.map(|v| v / weight.max(1e-12)))
    }
}

/// bo2mp: how a foreign map's light grid reads as Black Ops II light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForeignGridLight {
    /// The grid's mean colour (0..1), squared, times this: Black Ops' grids
    /// keep sqrt(light * 8 / 255) (~32); Infinity Ward's lit models read
    /// (2 * colour)^2 (4).
    pub scale: f32,
    /// The light where the map has no grid (outside it, a sample the grid
    /// cannot answer, or a map without one): an ambient light, and the sun
    /// at full visibility.
    pub outside: T6PointLight,
}

fn slot() -> &'static RwLock<Option<Arc<T6LightSampler>>> {
    static SLOT: RwLock<Option<Arc<T6LightSampler>>> = RwLock::new(None);
    &SLOT
}

pub fn install_t6_light_sampler(sampler: Option<T6LightSampler>) {
    if let Ok(mut s) = slot().write() {
        *s = sampler.map(Arc::new);
    }
}

pub fn t6_light_sampler() -> Option<Arc<T6LightSampler>> {
    slot().read().ok()?.clone()
}

impl T6LightSampler {
    fn in_grid(&self, p: [f32; 3]) -> bool {
        let cell = [
            (p[0].floor() as i32).wrapping_add(0x20000) >> 5,
            (p[1].floor() as i32).wrapping_add(0x20000) >> 5,
            (p[2].floor() as i32).wrapping_add(0x20000) >> 6,
        ];
        (0..3).all(|a| cell[a] >= i32::from(self.mins[a]) && cell[a] <= i32::from(self.maxs[a]))
    }

    /// The light at `pos` (with sight checks: once per 8-unit cell, read at
    /// its centre).
    pub fn at(&self, pos: [f32; 3]) -> T6PointLight {
        if self.clip.is_none() {
            return self.sample(pos);
        }
        let key = pos.map(|v| (v / 8.0).floor() as i32);
        if let Ok(cache) = self.cache.lock()
            && let Some(hit) = cache.get(&key)
        {
            return *hit;
        }
        let centre = key.map(|k| k as f32 * 8.0 + 4.0);
        let lit = self.sample(centre);
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() > 16_384 {
                cache.clear();
            }
            cache.insert(key, lit);
        }
        lit
    }

    fn sample(&self, pos: [f32; 3]) -> T6PointLight {
        if let Some(foreign) = self.foreign {
            return self.sample_foreign(pos, foreign);
        }
        if !self.in_grid(pos) {
            // Inside a sky volume: its light.
            let gap = |v: &([f32; 3], [f32; 3], [f32; 3], u8, u8)| {
                (0..3)
                    .map(|a| (v.0[a] - pos[a]).max(pos[a] - v.1[a]).max(0.0).powi(2))
                    .sum::<f32>()
            };
            let volume = |v: &([f32; 3], [f32; 3], [f32; 3], u8, u8)| T6PointLight {
                light: v.2,
                primary: v.3,
                visibility: f32::from(v.4) / 255.0,
            };
            if let Some(v) = self.sky.iter().find(|v| gap(v) == 0.0) {
                return volume(v);
            }
            // Past the grid's edge (Takeoff's shuttle and dish, out over the
            // vista): the grid's nearest edge. The nearest sky volume came
            // first before, and Takeoff's are small rooms inside the lab:
            // their dark, sunless light drew the shuttle black.
            if self.grid.is_some() {
                let lo = self.mins.map(f32::from);
                let hi = self.maxs.map(f32::from);
                let size = [32.0, 32.0, 64.0];
                let inside = [0, 1, 2].map(|a| {
                    pos[a].clamp(lo[a] * size[a] - 131_072.0, (hi[a] + 1.0) * size[a] - 131_073.0)
                });
                if self.in_grid(inside) {
                    return self.sample(inside);
                }
            }
            // No grid: the nearest volume's light, not zero (a first-person
            // gun at a spawn past the grid's edge, mp_village, drew black).
            if let Some(v) = self.sky.iter().min_by(|a, b| gap(a).total_cmp(&gap(b))) {
                return volume(v);
            }
            return T6PointLight::default();
        }
        let Some(grid) = self.grid.as_ref() else {
            return T6PointLight::default();
        };
        let sampled = match self.clip.as_deref() {
            Some(clip) => {
                let clear = |a: [f32; 3], b: [f32; 3]| {
                    clip.box_sight_clear(a, b, lighting_iw4::LIGHT_GRID_SIGHT_CONTENT_MASK)
                };
                asset_model::sample_light_grid_with_sight(&grid.view(), pos, Some(&clear))
            }
            None => asset_model::sample_light_grid(&grid.view(), pos),
        }
        .ok();
        let light = sampled
            .as_ref()
            .map(|s| crate::t6_sampled_light(s).map(|c| c * self.scale))
            .unwrap_or_default();
        // The primary light most of the surrounding entries carry.
        let primary = sampled
            .as_ref()
            .and_then(|s| {
                let mut best: Option<(u8, f32)> = None;
                let mut totals: Vec<(u8, f32)> = Vec::new();
                for i in 0..8 {
                    if let Some(p) = s.corner_primaries[i] {
                        match totals.iter_mut().find(|t| t.0 == p) {
                            Some(t) => t.1 += s.weights[i],
                            None => totals.push((p, s.weights[i])),
                        }
                    }
                }
                for t in totals {
                    if best.is_none_or(|b| t.1 > b.1) {
                        best = Some(t);
                    }
                }
                best.map(|b| b.0)
            })
            .unwrap_or(self.sun_primary);
        let visibility = sampled
            .as_ref()
            .and_then(|s| crate::t6_sampled_visibility(s, primary))
            .map_or(0.0, |v| f32::from(v) / 255.0);
        T6PointLight {
            light,
            primary,
            visibility,
        }
    }
}

/// bo2mp: whether Black Ops II's models on another game's map take the
/// primary light's term on top of the grid's light
/// (`lighting_iw4::guest_sun_on`).
static FOREIGN_SUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_foreign_sun(on: bool) {
    FOREIGN_SUN.store(on, std::sync::atomic::Ordering::Relaxed);
}

impl T6LightSampler {
    /// bo2mp: a foreign map's grid at `pos`: its light (the mean of the
    /// lookup's colours, decoded as `foreign.scale` says), the primary light
    /// the lookup picked and, for that light's visibility, the share of the
    /// corners' weight that names it (Infinity Ward's grids carry no baked
    /// visibility; a corner of another light is in its shadow).
    fn sample_foreign(&self, pos: [f32; 3], foreign: ForeignGridLight) -> T6PointLight {
        let Some(grid) = self.grid.as_ref().filter(|_| self.in_grid(pos)) else {
            // bo2mp: no grid here: the placed models' light round the point,
            // when the map gave them light (else the even light).
            if let Some(light) = self.prop_light.as_ref().and_then(|p| p.at(pos, self.clip.as_deref())) {
                return T6PointLight { light, ..foreign.outside };
            }
            return foreign.outside;
        };
        let sampled = match self.clip.as_deref() {
            Some(clip) => {
                let clear = |a: [f32; 3], b: [f32; 3]| {
                    clip.box_sight_clear(a, b, lighting_iw4::LIGHT_GRID_SIGHT_CONTENT_MASK)
                };
                asset_model::sample_light_grid_with_sight(&grid.view(), pos, Some(&clear))
            }
            None => asset_model::sample_light_grid(&grid.view(), pos),
        };
        let Ok(s) = sampled else {
            return foreign.outside;
        };
        let mean = asset_model::mean_tile_rgba01(&s.tile);
        let light = mean.map(|c| c * c * foreign.scale);
        let primary = s.picked_primary;
        // bo2mp: corners name the sun in the grid's sun band (255 and up
        // to it), the picked light by its table index: compare the same
        // way, or the sun's corners counted as another light's.
        let sun_first = grid.sun_primary_light_index;
        let sun_band = lighting_iw4::light_grid_primary_light_sun_band(sun_first);
        let remap = |p: u8| if u32::from(p) >= sun_band { p.wrapping_sub(0xff).wrapping_add(sun_first as u8) } else { p };
        let (mut named, mut all) = (0.0f32, 0.0f32);
        for i in 0..8 {
            if let Some(p) = s.corner_primaries[i] {
                all += s.weights[i];
                if p == primary || remap(p) == primary {
                    named += s.weights[i];
                }
            }
        }
        // (No corner naming any light: in no light's view, not in full sun.)
        let visibility = if primary == 0 || all <= 1e-6 { 0.0 } else { (named / all).clamp(0.0, 1.0) };
        // bo2mp: the primary light's term only where the map's own models
        // add it (`set_foreign_sun`).
        let visibility = if FOREIGN_SUN.load(std::sync::atomic::Ordering::Relaxed) { visibility } else { 0.0 };
        T6PointLight {
            light,
            primary,
            visibility,
        }
    }
}

/// bo2zm fix list 2: the map's lightmaps on the CPU, for what is lit like
/// the world but drawn later: a decal on a wall (BO2's `wc_` mark
/// materials) takes the wall's own lightmap, not the light grid in front of
/// it. Each image holds three pages stacked (base light, directional light,
/// direction + the primary light's baked visibility), as the world shader
/// reads them.
pub struct T6LightmapPage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

fn lightmap_slot() -> &'static RwLock<Vec<Option<Arc<T6LightmapPage>>>> {
    static SLOT: RwLock<Vec<Option<Arc<T6LightmapPage>>>> = RwLock::new(Vec::new());
    &SLOT
}

static LIGHTMAP_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn install_t6_lightmaps(pages: Vec<Option<T6LightmapPage>>) {
    if let Ok(mut s) = lightmap_slot().write() {
        *s = pages.into_iter().map(|p| p.map(Arc::new)).collect();
    }
    LIGHTMAP_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// bo2mp: bumped whenever the lightmaps are replaced, so a cache of their
/// light knows to start over.
pub fn t6_lightmap_generation() -> u64 {
    LIGHTMAP_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

impl T6LightmapPage {
    /// One texel channel set, filtered as the GPU's linear sampler with
    /// clamped edges does.
    fn bilinear(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width as i64, self.height as i64);
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let texel = |xi: i64, yi: i64| -> [f32; 4] {
            let xi = xi.clamp(0, w - 1) as usize;
            let yi = yi.clamp(0, h - 1) as usize;
            let at = (yi * self.width as usize + xi) * 4;
            self.rgba
                .get(at..at + 4)
                .map_or([0.0; 4], |p| [0, 1, 2, 3].map(|c| f32::from(p[c]) / 255.0))
        };
        let (xi, yi) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (texel(xi, yi), texel(xi + 1, yi), texel(xi, yi + 1), texel(xi + 1, yi + 1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }
}

/// The world shader's lightmap light at lightmap `index`, coordinate `uv`
/// (one page's u, v) for normal `n`: base + directional * saturate(N.dir),
/// and the baked visibility of the surface's primary light. `None` when the
/// map has no such lightmap.
pub fn t6_lightmap_light(index: u8, uv: [f32; 2], n: [f32; 3]) -> Option<([f32; 3], f32)> {
    let (base, directional, ndl, vis) = t6_lightmap_parts(index, uv, n)?;
    let light = [0, 1, 2].map(|c| base[c] + directional[c] * ndl);
    Some((light, vis))
}

/// The lightmap's parts at a point: base light, directional light, the
/// directional term's N.dir, and the primary light's baked visibility.
pub fn t6_lightmap_parts(
    index: u8,
    uv: [f32; 2],
    n: [f32; 3],
) -> Option<([f32; 3], [f32; 3], f32, f32)> {
    let pages = lightmap_slot().read().ok()?;
    let page = pages.get(usize::from(index))?.as_ref()?;
    let v = uv[1] / 3.0;
    let p0 = page.bilinear(uv[0], v);
    let p1 = page.bilinear(uv[0], v + 1.0 / 3.0);
    let p2 = page.bilinear(uv[0], v + 2.0 / 3.0);
    let dir = [p2[0] * 2.0 - 1.0, p2[1] * 2.0 - 1.0, p2[2] * 2.0 - 1.0];
    let ndl = (dir[0] * n[0] + dir[1] * n[1] + dir[2] * n[2]).clamp(0.0, 1.0);
    let base = [0, 1, 2].map(|c| p0[c] / (p0[3] + 0.000001));
    let directional = [0, 1, 2].map(|c| p1[c] / (p1[3] + 0.000001));
    Some((base, directional, ndl, p2[3]))
}

/// bo2zm: the world's reflection probe origins, for what moves (the
/// first-person gun reflects the probe nearest the eye).
fn probe_slot() -> &'static std::sync::RwLock<Vec<[f32; 3]>> {
    static SLOT: std::sync::OnceLock<std::sync::RwLock<Vec<[f32; 3]>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(Default::default)
}

pub fn install_t6_probe_origins(origins: Vec<[f32; 3]>) {
    if let Ok(mut s) = probe_slot().write() {
        *s = origins;
    }
    install_t6_probe_picker(None);
}

/// bo2mp: a map's own rule for the probe of a point (Ghosts' maps: the
/// stored probe of the nearest placed model or surface), in place of the
/// nearest origin. Cleared by `install_t6_probe_origins`; install it after.
pub type T6ProbePicker = std::sync::Arc<dyn Fn([f32; 3]) -> Option<u32> + Send + Sync>;

fn probe_picker() -> &'static std::sync::RwLock<Option<T6ProbePicker>> {
    static SLOT: std::sync::OnceLock<std::sync::RwLock<Option<T6ProbePicker>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(Default::default)
}

pub fn install_t6_probe_picker(picker: Option<T6ProbePicker>) {
    if let Ok(mut s) = probe_picker().write() {
        *s = picker;
    }
}

/// bo2mp: Black Ops III's `zone` folder for the shared look's post passes
/// (`render_t7::bo2_shared`: SMAA and motion blur are Black Ops III's own
/// programs, read from its `core_patch.ff`), set as a Black Ops II map loads
/// with the shared look on (`asset_core::bo2_shared_look`).
fn shared_look_zone_slot() -> &'static std::sync::RwLock<Option<std::path::PathBuf>> {
    static SLOT: std::sync::OnceLock<std::sync::RwLock<Option<std::path::PathBuf>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(Default::default)
}

pub fn install_shared_look_zone_dir(dir: Option<std::path::PathBuf>) {
    if let Ok(mut s) = shared_look_zone_slot().write() {
        *s = dir;
    }
}

pub fn shared_look_zone_dir() -> Option<std::path::PathBuf> {
    shared_look_zone_slot().read().ok().and_then(|s| s.clone())
}

/// The reflection probe nearest `p` (probe 0 is a stand-in and never picked
/// while another exists), or the map's own rule where it set one.
pub fn t6_nearest_probe(p: [f32; 3]) -> Option<u32> {
    let picker = probe_picker().read().ok().and_then(|s| s.clone());
    if let Some(pick) = picker {
        return pick(p);
    }
    let origins = probe_slot().read().ok()?;
    origins
        .iter()
        .enumerate()
        .skip(usize::from(origins.len() > 1))
        .min_by(|(_, a), (_, b)| {
            let d = |o: &[f32; 3]| (0..3).map(|i| (o[i] - p[i]) * (o[i] - p[i])).sum::<f32>();
            d(a).total_cmp(&d(b))
        })
        .map(|(i, _)| i as u32)
}

/// bo2mp: the zones of a Modern Warfare Remastered map drawn by MWR's own
/// renderer (`render_h1`): the map's zone first, then those it draws on.
/// Set as the map loads, cleared when any map loads; read by the renderer.
static H1_NATIVE_ZONES: std::sync::RwLock<Option<std::sync::Arc<[std::path::PathBuf]>>> =
    std::sync::RwLock::new(None);

pub fn set_h1_native_zones(zones: Option<Vec<std::path::PathBuf>>) {
    if let Ok(mut z) = H1_NATIVE_ZONES.write() {
        *z = zones.filter(|z| !z.is_empty()).map(Into::into);
    }
}

pub fn h1_native_zones() -> Option<std::sync::Arc<[std::path::PathBuf]>> {
    H1_NATIVE_ZONES.read().ok().and_then(|z| z.clone())
}

/// bo2mp: the zones of a Ghosts map drawn by Ghosts' own renderer
/// (`render_iw6`): the map's zone first, then those it draws on. Set as the
/// map loads (unless `BO2MP_IW6_NATIVE=0`), cleared when any map loads;
/// read by the renderer.
static IW6_NATIVE_ZONES: std::sync::RwLock<Option<std::sync::Arc<[std::path::PathBuf]>>> =
    std::sync::RwLock::new(None);

pub fn set_iw6_native_zones(zones: Option<Vec<std::path::PathBuf>>) {
    if let Ok(mut z) = IW6_NATIVE_ZONES.write() {
        *z = zones.filter(|z| !z.is_empty()).map(Into::into);
    }
}

pub fn iw6_native_zones() -> Option<std::sync::Arc<[std::path::PathBuf]>> {
    IW6_NATIVE_ZONES.read().ok().and_then(|z| z.clone())
}

/// bo2mp: one thing a Ghosts door switch moves (`fastfile_iw6::maplogic`):
/// its brush model or its model, where it stands and where it goes, and
/// its `script_parameters` sounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6DoorPiece {
    pub brush: Option<u32>,
    pub model: Option<String>,
    pub start: [f32; 3],
    pub start_angles: [f32; 3],
    pub end: [f32; 3],
    pub end_angles: [f32; 3],
    pub start_sound: Option<String>,
    pub stop_sound: Option<String>,
    pub loop_sound: Option<String>,
}

/// bo2mp: the pieces one name of Ghosts door switches moves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6DoorGroup {
    pub name: String,
    /// The switches' (`trigger_use_touch`) origins.
    pub switches: Vec<[f32; 3]>,
    pub button_sound: Option<String>,
    pub pieces: Vec<Iw6DoorPiece>,
}

/// bo2mp: the Ghosts map's door switches, set as the map loads (empty
/// otherwise); read by the sim (`t6::iw6_doors`).
static IW6_DOORS: std::sync::RwLock<Option<std::sync::Arc<[Iw6DoorGroup]>>> = std::sync::RwLock::new(None);

pub fn set_iw6_doors(doors: Vec<Iw6DoorGroup>) {
    if let Ok(mut d) = IW6_DOORS.write() {
        *d = (!doors.is_empty()).then(|| doors.into());
    }
}

pub fn iw6_doors() -> Option<std::sync::Arc<[Iw6DoorGroup]>> {
    IW6_DOORS.read().ok().and_then(|d| d.clone())
}

/// bo2mp: one createfx placement that waits for its exploder (`exploder(n)` in the map's
/// scripts ends in `activateclientexploder(n)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6Exploder {
    /// The exploder id as the createfx source gives it (a string: `1`).
    pub id: String,
    /// The level's effect id (`vfx_sprinkler_system_frag`).
    pub fxid: String,
    /// The name of the effect as the match's effect catalog has it (empty: not converted).
    pub effect: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    pub delay: f32,
    pub soundalias: Option<String>,
    pub loopsound: Option<String>,
}

/// bo2mp: what the sim's script VM needs from a Ghosts map load: the compiled scripts of the
/// map and the common zones (in load order; a later script replaces an earlier one of the same
/// name), the string tables, the map's entity blocks with their raw numeric keys, the exploder
/// placements, and the zones they came from.
#[derive(Clone, Debug, Default)]
pub struct Iw6ScriptPack {
    pub map: String,
    /// (name, string stream, bytecode).
    pub scripts: Vec<(String, Vec<u8>, Vec<u8>)>,
    /// (name, columns, rows, cells).
    pub tables: Vec<(String, usize, usize, Vec<String>)>,
    pub entities: Vec<Vec<(u32, String)>>,
    pub exploders: Vec<Iw6Exploder>,
    /// The zones the scripts came from, in order (for the log).
    pub zones: Vec<String>,
    /// Every model of the map zone with its bounds and tags (the sim's damage boxes and
    /// `gettagorigin` for models the scripts spawn or swap in).
    pub models: Vec<Iw6Model>,
    /// The effects the scripts name (`"fx/..."` literals in the scripts reachable from the map's
    /// main), converted at load: (path, lower case; name in the effect catalog).
    pub script_fx: Vec<(String, String)>,
    /// The models of the map zone the scripts may spawn or swap in (their names appear in the
    /// reachable scripts): the renderer keeps a few hidden instances of each.
    pub pool_models: Vec<String>,
    /// The map zone's animations, decoded and sampled (`fastfile_iw6::anim`): what
    /// `scriptmodelplayanim` / `useanimtree` play.
    pub clips: Vec<iw6_pose::Clip>,
    /// The skeletons of the map zone's multi-bone models (tags that animate, rigid parts).
    pub skeletons: Vec<iw6_pose::Skeleton>,
    /// The bounds of the map's brush models (`"model" "*N"`), by N, about the model's origin.
    pub brush_bounds: Vec<(u32, [f32; 3], [f32; 3])>,
    /// The English localized strings of the map's zones (reference name, text): door hints.
    pub localized: Vec<(String, String)>,
}

/// bo2mp: a model of the map zone as the script host needs it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6Model {
    pub name: String,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// (tag name, lower case; origin in model space).
    pub tags: Vec<(String, [f32; 3])>,
}

static IW6_SCRIPTS: std::sync::RwLock<Option<std::sync::Arc<Iw6ScriptPack>>> = std::sync::RwLock::new(None);

pub fn set_iw6_scripts(pack: Option<Iw6ScriptPack>) {
    if let Ok(mut p) = IW6_SCRIPTS.write() {
        *p = pack.filter(|p| !p.scripts.is_empty()).map(Into::into);
    }
}

pub fn iw6_scripts() -> Option<std::sync::Arc<Iw6ScriptPack>> {
    IW6_SCRIPTS.read().ok().and_then(|p| p.clone())
}

/// bo2mp: a Ghosts map's path node (`fastfile_iw6::pathnode`): what the bots route over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6PathNode {
    /// The engine's node type index (`Path` is 1, `Begin` 17, `End` 18).
    pub ty: u8,
    pub origin: [f32; 3],
    pub yaw: f32,
    /// For a `Begin` node: the `End` node of the traversal (index into the list) and its animation.
    pub negotiation_end: Option<u32>,
    pub animscript: String,
    /// The links the map's own path data (`<map>_path.ff`) gives the node; empty when that zone is
    /// not installed (the bots then rebuild walk links from the collision).
    pub links: Vec<Iw6PathLink>,
}

/// One link of a node in the map's path data (`pathlink_s`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6PathLink {
    pub to: u32,
    /// The engine's cost of the link (a distance).
    pub cost: f32,
    /// A traversal (jump, climb, window) rather than a walk.
    pub negotiation: bool,
}

static IW6_PATH_NODES: std::sync::RwLock<Option<std::sync::Arc<[Iw6PathNode]>>> = std::sync::RwLock::new(None);

pub fn set_iw6_path_nodes(nodes: Option<Vec<Iw6PathNode>>) {
    if let Ok(mut p) = IW6_PATH_NODES.write() {
        *p = nodes.filter(|n| !n.is_empty()).map(Into::into);
    }
}

pub fn iw6_path_nodes() -> Option<std::sync::Arc<[Iw6PathNode]>> {
    IW6_PATH_NODES.read().ok().and_then(|p| p.clone())
}

/// bo2mp: where a Ghosts map's moved entity stands now: a brush model by
/// its number, a model by its name and where the map put it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6MoverPose {
    pub brush: Option<u32>,
    pub model: Option<String>,
    pub spawn_origin: [f32; 3],
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// The scripts hid or deleted it (`hide`, `delete`): not drawn.
    pub hidden: bool,
    /// A model a script spawned or swapped in: the hidden instance of `model` (0..) that draws
    /// it (`spawn_origin` is not used).
    pub slot: Option<u32>,
    /// A model that plays an animation: its skinning pose per bone (model space, `pose * bind^-1`),
    /// in the skeleton's bone order. None: the bind pose (the whole model at the entity's place).
    pub skin: Option<Vec<iw6_pose::Bone>>,
}

/// bo2mp: the poses the sim last gave the Ghosts map's movers (a count
/// that changes with each new list, and the list); read by Ghosts' own
/// renderer (`iw6_native`). The process that runs the sim only.
static IW6_MOVER_POSES: std::sync::RwLock<(u64, Option<std::sync::Arc<[Iw6MoverPose]>>)> =
    std::sync::RwLock::new((0, None));

pub fn set_iw6_mover_poses(poses: Vec<Iw6MoverPose>) {
    if let Ok(mut p) = IW6_MOVER_POSES.write() {
        p.0 += 1;
        p.1 = Some(poses.into());
    }
}

pub fn iw6_mover_poses() -> (u64, Option<std::sync::Arc<[Iw6MoverPose]>>) {
    IW6_MOVER_POSES.read().map_or((0, None), |p| (p.0, p.1.clone()))
}

/// bo2mp: a mover whose volume the scripts opened or closed to walkers (`connectpaths` /
/// `disconnectpaths` on a brush entity): the key of the entity, its world bounds now, and whether
/// the paths through it are cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Iw6PathGate {
    pub key: u32,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub blocked: bool,
}

/// bo2mp: the gates the scripts last set (a count that changes with each new list, and the
/// list); read by the bots' navigation. The process that runs the sim only.
static IW6_PATH_GATES: std::sync::RwLock<(u64, Option<std::sync::Arc<[Iw6PathGate]>>)> =
    std::sync::RwLock::new((0, None));

pub fn set_iw6_path_gates(gates: Vec<Iw6PathGate>) {
    if let Ok(mut p) = IW6_PATH_GATES.write() {
        p.0 += 1;
        p.1 = Some(gates.into());
    }
}

pub fn iw6_path_gates() -> (u64, std::sync::Arc<[Iw6PathGate]>) {
    IW6_PATH_GATES.read().map_or((0, std::sync::Arc::from([])), |p| (p.0, p.1.clone().unwrap_or_else(|| std::sync::Arc::from([]))))
}

/// bo2mp: the grade of the Ghosts map drawn natively this frame (its iris
/// exposure, tone curve and film), for effects drawn after its final pass
/// (`render_fx::iw6_rules`); `None` off a native Ghosts map.
static IW6_FX_GRADE: std::sync::RwLock<Option<fx_iw4::iw6::grade::Grade>> = std::sync::RwLock::new(None);

pub fn set_iw6_fx_grade(grade: Option<fx_iw4::iw6::grade::Grade>) {
    if let Ok(mut g) = IW6_FX_GRADE.write() {
        *g = grade;
    }
}

pub fn iw6_fx_grade() -> Option<fx_iw4::iw6::grade::Grade> {
    IW6_FX_GRADE.read().ok().and_then(|g| *g)
}

/// bo2mp: the most effect lights a Ghosts frame lights the world with (the
/// quality preset takes fewer: `native_quality::Quality::fx_lights`).
pub const IW6_FX_LIGHTS_MAX: usize = 16;

/// One effect light (the omni and spot light elements of Ghosts' effects) as
/// the native renderer takes it: world position, radius, linear colour, and
/// for a spot the direction it shines and the cone's cosine at its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6FxLight {
    pub origin: [f32; 3],
    pub radius: f32,
    pub color: [f32; 3],
    pub dir: [f32; 3],
    /// `1.0` omni (no cone); a spot's cosine of its half angle.
    pub cos_cone: f32,
}

/// The frame's effect lights, fixed in size so that handing them over
/// allocates nothing.
#[derive(Clone, Copy, Debug)]
pub struct Iw6FxLights {
    pub len: usize,
    pub lights: [Iw6FxLight; IW6_FX_LIGHTS_MAX],
}

impl Default for Iw6FxLights {
    fn default() -> Self {
        Self { len: 0, lights: [Iw6FxLight::default(); IW6_FX_LIGHTS_MAX] }
    }
}

impl Iw6FxLights {
    pub fn as_slice(&self) -> &[Iw6FxLight] {
        &self.lights[..self.len.min(IW6_FX_LIGHTS_MAX)]
    }
}

static IW6_FX_LIGHTS: std::sync::Mutex<Iw6FxLights> =
    std::sync::Mutex::new(Iw6FxLights { len: 0, lights: [Iw6FxLight { origin: [0.0; 3], radius: 0.0, color: [0.0; 3], dir: [0.0; 3], cos_cone: 1.0 }; IW6_FX_LIGHTS_MAX] });

/// Fills the frame's effect lights in place (`fill` gets the emptied list).
pub fn set_iw6_fx_lights_with(fill: impl FnOnce(&mut Iw6FxLights)) {
    if let Ok(mut g) = IW6_FX_LIGHTS.lock() {
        g.len = 0;
        fill(&mut g);
    }
}

/// Copies the frame's effect lights out (no allocation: `out` is reused).
pub fn iw6_fx_lights_into(out: &mut Iw6FxLights) {
    match IW6_FX_LIGHTS.lock() {
        Ok(g) => *out = *g,
        Err(_) => out.len = 0,
    }
}

/// bo2mp: the camera of the frame the effects are built for: its position,
/// right / up / forward directions and the tangents of its half fields of
/// view, for the screen-space flare (`render_fx::iw6_flare`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iw6FxView {
    pub eye: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    pub tan_x: f32,
    pub tan_y: f32,
}

static IW6_FX_VIEW: std::sync::RwLock<Option<Iw6FxView>> = std::sync::RwLock::new(None);

pub fn set_iw6_fx_view(view: Option<Iw6FxView>) {
    if let Ok(mut v) = IW6_FX_VIEW.write() {
        *v = view;
    }
}

pub fn iw6_fx_view() -> Option<Iw6FxView> {
    IW6_FX_VIEW.read().ok().and_then(|v| *v)
}

/// bo2mp: what the frame's effect sprites cost the fill (opt-in profile,
/// `BO2MP_IW6_PROFILE=1`): sprites built, those in front of the eye and on
/// screen, the sum of their screen coverage in screens (an estimate from
/// the quads' size and distance, each capped at one screen: 2.0 means the
/// sprites cover the screen twice over) and the biggest single quad's share
/// of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6FxStats {
    pub sprites: u32,
    pub visible: u32,
    pub overdraw: f32,
    pub biggest: f32,
    /// Quads covering more than a quarter of the screen.
    pub big: u32,
}

static IW6_FX_STATS: std::sync::RwLock<Iw6FxStats> = std::sync::RwLock::new(Iw6FxStats { sprites: 0, visible: 0, overdraw: 0.0, biggest: 0.0, big: 0 });

pub fn set_iw6_fx_stats(s: Iw6FxStats) {
    if let Ok(mut v) = IW6_FX_STATS.write() {
        *v = s;
    }
}

pub fn iw6_fx_stats() -> Iw6FxStats {
    IW6_FX_STATS.read().map(|v| *v).unwrap_or_default()
}

/// The stats of `sprites` seen from `view` (pure; the quads' screen share
/// is `size0 * height / (z^2 * tan_x * tan_y)`, a quad one screen wide and
/// high being 1).
pub fn fx_stats_of(sprites: impl Iterator<Item = ([f32; 3], f32, f32)>, view: &Iw6FxView) -> Iw6FxStats {
    let mut s = Iw6FxStats::default();
    for (o, size0, size1) in sprites {
        s.sprites += 1;
        let d = [o[0] - view.eye[0], o[1] - view.eye[1], o[2] - view.eye[2]];
        let dot = |a: [f32; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        let z = dot(view.forward);
        if z < 0.1 {
            continue;
        }
        let (x, y) = (dot(view.right), dot(view.up));
        let height = if size1 > 0.0 { size1 } else { size0 };
        // (Centre within the frustum widened by the quad's half size.)
        if x.abs() > z * view.tan_x + size0 || y.abs() > z * view.tan_y + height {
            continue;
        }
        let share = (size0 * height / (z * z * view.tan_x * view.tan_y)).min(1.0);
        s.visible += 1;
        s.overdraw += share;
        s.biggest = s.biggest.max(share);
        if share > 0.25 {
            s.big += 1;
        }
    }
    s
}

/// `BO2MP_IW6_PROFILE=1` (or `2`, or the shared `BO2MP_GPU_TIMING=1`): the
/// effect stats are worked out.
pub fn iw6_fx_stats_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("BO2MP_IW6_PROFILE").is_ok_and(|v| matches!(v.trim(), "1" | "2"))
            || std::env::var("BO2MP_GPU_TIMING").is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0")
    })
}

#[cfg(test)]
mod fx_stats_tests {
    use super::*;

    #[test]
    fn a_quad_ahead_covers_its_share_and_one_behind_none() {
        let v = Iw6FxView { eye: [0.0; 3], right: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], forward: [0.0, 1.0, 0.0], tan_x: 1.0, tan_y: 0.5 };
        // 100 ahead, 10 x 10 half sizes: 100 / (10000 * 0.5) = 0.02 of the screen.
        let s = fx_stats_of([([0.0, 100.0, 0.0], 10.0, 0.0), ([0.0, -100.0, 0.0], 10.0, 0.0), ([5000.0, 100.0, 0.0], 10.0, 0.0)].into_iter(), &v);
        assert_eq!((s.sprites, s.visible), (3, 1));
        assert!((s.overdraw - 0.02).abs() < 1e-6 && s.big == 0);
    }
}

/// bo2mp: Ghosts' own bullet penetration depths (`info/bullet_penetration_mp`
/// of the install) in Black Ops II's surface list, and the zone they came
/// from; set once by the Ghosts gun lane, read as a match starts.
static IW6_PEN_TABLE: std::sync::RwLock<Option<(weapon_iw4::PenetrationDepthTable, String)>> =
    std::sync::RwLock::new(None);

pub fn set_iw6_pen_table(table: Option<(weapon_iw4::PenetrationDepthTable, String)>) {
    if let Ok(mut t) = IW6_PEN_TABLE.write() {
        *t = table;
    }
}

pub fn iw6_pen_table() -> Option<(weapon_iw4::PenetrationDepthTable, String)> {
    IW6_PEN_TABLE.read().ok().and_then(|t| t.clone())
}

/// bo2mp: Ghosts' depths (by its 31 surface types, `metal_grate` at 14)
/// in Black Ops II's list: each BO2 type takes the Ghosts type that maps
/// to it (`iw6_surface_flags_to_t6`; metal over metal grate), BO2's
/// `cushion` Ghosts' `cloth` (Ghosts reads `cushion` as cloth), and the
/// two types Ghosts has no column for, `player` and `tallgrass`, `flesh`
/// and `grass` (INFERRED).
pub fn iw6_pen_depths_to_t6(depths: &[[f32; 31]; 4]) -> weapon_iw4::PenetrationDepthTable {
    use crate::surface_types::iw6_surface_flags_to_t6;
    let mut t = weapon_iw4::PenetrationDepthTable::empty();
    let mut set = [false; weapon_iw4::PEN_TABLE_SURF_COUNT];
    for g in 0..31u32 {
        let to = ((iw6_surface_flags_to_t6(g << 20) >> 20) & 0x1f) as usize;
        if g != 0 && to == 0 || to >= set.len() || set[to] {
            continue;
        }
        set[to] = true;
        for (p, row) in depths.iter().enumerate() {
            t.set_depth(p, to, row[g as usize]);
        }
    }
    let name = |n: &str| weapon_iw4::T6_SURFACE_TYPE_NAMES.iter().position(|x| *x == n);
    for (bo2, ghosts) in [("cushion", 4usize), ("player", 7), ("tallgrass", 10)] {
        if let Some(i) = name(bo2) {
            for (p, row) in depths.iter().enumerate() {
                t.set_depth(p, i, row[ghosts]);
            }
        }
    }
    t
}

#[cfg(test)]
mod iw6_pen_tests {
    #[test]
    fn ghosts_depths_land_on_bo2_surfaces() {
        let mut d = [[0.0f32; 31]; 4];
        for (g, v) in d[1].iter_mut().enumerate() {
            *v = g as f32 + 0.5;
        }
        let t = super::iw6_pen_depths_to_t6(&d);
        let idx = |n: &str| weapon_iw4::T6_SURFACE_TYPE_NAMES.iter().position(|x| *x == n).unwrap() as u32;
        assert_eq!(t.depth(1, idx("metal")), 13.5, "metal, not metal grate");
        assert_eq!(t.depth(1, idx("mud")), 15.5);
        assert_eq!(t.depth(1, idx("wood")), 22.5);
        assert_eq!(t.depth(1, idx("plastic")), 25.5);
        assert_eq!(t.depth(1, idx("rubber")), 26.5);
        assert_eq!(t.depth(1, idx("fruit")), 27.5);
        assert_eq!(t.depth(1, idx("paintedmetal")), 28.5);
        assert_eq!(t.depth(1, idx("riotshield")), 29.5);
        assert_eq!(t.depth(1, idx("snow")), 20.5, "snow, not slush");
        assert_eq!(t.depth(1, idx("cushion")), 4.5);
        assert_eq!(t.depth(1, idx("player")), 7.5);
        assert_eq!(t.depth(1, idx("tallgrass")), 10.5);
        assert_eq!(t.depth(2, idx("wood")), 0.0);
    }
}

/// bo2mp: the zone of a Black Ops III map drawn the way BlackOps3.exe
/// draws it (`render_t7`; the zones beside it are read too). Set as the
/// map loads unless `BO2MP_T7_NATIVE=0`, cleared when any map loads.
static T7_NATIVE_ZONE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

pub fn set_t7_native_zone(zone: Option<std::path::PathBuf>) {
    if let Ok(mut z) = T7_NATIVE_ZONE.write() {
        *z = zone;
        // (A new request even for the same zone again: the renderer reads
        // the generation, not only the path.)
        T7_NATIVE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

static T7_NATIVE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn t7_native_zone() -> Option<std::path::PathBuf> {
    T7_NATIVE_ZONE.read().ok().and_then(|z| z.clone())
}

/// The native zone and the number of the request that named it (it changes
/// with every `set_t7_native_zone`).
pub fn t7_native_request() -> (Option<std::path::PathBuf>, u64) {
    match T7_NATIVE_ZONE.read() {
        Ok(z) => (z.clone(), T7_NATIVE_GENERATION.load(std::sync::atomic::Ordering::Relaxed)),
        Err(_) => (None, 0),
    }
}

/// bo2mp: the zone of a Modern Warfare III / II map drawn by `render_jup`
/// (the game's own look). Set as the map loads with `BO2MP_JUP_NATIVE=1` (opt-in since 2026-10-28),
/// cleared when any map loads. The loading hold is `t7_native_load`'s.
static JUP_NATIVE_ZONE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);
static JUP_NATIVE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_jup_native_zone(zone: Option<std::path::PathBuf>) {
    if let Ok(mut z) = JUP_NATIVE_ZONE.write() {
        *z = zone;
        JUP_NATIVE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn jup_native_zone() -> Option<std::path::PathBuf> {
    JUP_NATIVE_ZONE.read().ok().and_then(|z| z.clone())
}

/// The native zone and the number of the request that named it.
pub fn jup_native_request() -> (Option<std::path::PathBuf>, u64) {
    match JUP_NATIVE_ZONE.read() {
        Ok(z) => (z.clone(), JUP_NATIVE_GENERATION.load(std::sync::atomic::Ordering::Relaxed)),
        Err(_) => (None, 0),
    }
}

/// bo2mp: the zone of an Infinite Warfare map drawn by `render_iw7` (its
/// own lit techniques). Set as the map loads unless `BO2MP_IW7_NATIVE=0`
/// (the converted world then), cleared when any map
/// loads. The loading hold is `t7_native_load`'s; the converted world is
/// not built.
static IW7_NATIVE_ZONE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);
static IW7_NATIVE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_iw7_native_zone(zone: Option<std::path::PathBuf>) {
    if let Ok(mut z) = IW7_NATIVE_ZONE.write() {
        *z = zone;
        IW7_NATIVE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn iw7_native_zone() -> Option<std::path::PathBuf> {
    IW7_NATIVE_ZONE.read().ok().and_then(|z| z.clone())
}

/// The native zone and the number of the request that named it.
pub fn iw7_native_request() -> (Option<std::path::PathBuf>, u64) {
    match IW7_NATIVE_ZONE.read() {
        Ok(z) => (z.clone(), IW7_NATIVE_GENERATION.load(std::sync::atomic::Ordering::Relaxed)),
        Err(_) => (None, 0),
    }
}

/// bo2mp: Modern Warfare Remastered's automatic exposure (its
/// `r_tonemapAuto`): a map drawn under MWR's own shaders sets the key the
/// scene's average is held at; the renderer measures the frame and sets
/// the factor its exposure is multiplied by. `None`: off (every other map).
static MWR_AUTO_KEY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static MWR_AUTO_FACTOR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

pub fn set_mwr_auto_exposure(key: Option<f32>) {
    use std::sync::atomic::Ordering::Relaxed;
    MWR_AUTO_KEY.store(key.filter(|k| *k > 0.0).map_or(0, f32::to_bits), Relaxed);
    MWR_AUTO_FACTOR.store(1.0f32.to_bits(), Relaxed);
}

/// The key, while a map asks for automatic exposure.
pub fn mwr_auto_exposure_key() -> Option<f32> {
    let bits = MWR_AUTO_KEY.load(std::sync::atomic::Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits))
}

/// The factor the exposure is multiplied by now (1 while off).
pub fn mwr_auto_exposure_factor() -> f32 {
    if mwr_auto_exposure_key().is_none() {
        return 1.0;
    }
    f32::from_bits(MWR_AUTO_FACTOR.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_mwr_auto_exposure_factor(f: f32) {
    if f.is_finite() && f > 0.0 {
        MWR_AUTO_FACTOR.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// Modern Warfare Remastered maps the converted scene reads no light grid
/// for: the light the first-person gun and arms take, by position (each
/// reflection probe's origin and its packed ambient, as the placed models
/// take it). Empty on every other map.
static H1_EYE_LIGHT: std::sync::Mutex<Vec<([f32; 3], [u8; 4])>> = std::sync::Mutex::new(Vec::new());

pub fn set_h1_eye_lighting(points: Vec<([f32; 3], [u8; 4])>) {
    if let Ok(mut g) = H1_EYE_LIGHT.lock() {
        *g = points;
    }
}

/// The packed ambient of the probe nearest `origin`, if the map has any.
pub fn h1_eye_lighting_at(origin: [f32; 3]) -> Option<[u8; 4]> {
    let g = H1_EYE_LIGHT.lock().ok()?;
    g.iter()
        .min_by(|a, b| {
            let d = |o: [f32; 3]| (0..3).map(|k| (o[k] - origin[k]).powi(2)).sum::<f32>();
            d(a.0).total_cmp(&d(b.0))
        })
        .map(|(_, packed)| *packed)
}

/// bo2mp: the most Ghosts decal spawns handed to the native renderer in one
/// frame (older ones are dropped when more arrive).
pub const IW6_DECALS_MAX: usize = 32;

/// One Ghosts decal element (type 15) spawn for the native renderer
/// (`render_iw6::decal`): the mark's centre, its axes (the surface normal
/// first, then the texture axes, as `fx_iw4::impact_mark_axis` gives them),
/// its radius, native (BGRA) colour, life in milliseconds (0: until the
/// ring reuses its slot) and the material (`iw6_decal_material`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6DecalSpawn {
    pub origin: [f32; 3],
    pub axis: [[f32; 3]; 3],
    pub radius: f32,
    pub color: u32,
    pub life_ms: u32,
    pub material: u16,
}

/// The frame's decal spawns, fixed in size so that handing them over
/// allocates nothing.
#[derive(Clone, Copy, Debug)]
pub struct Iw6Decals {
    pub len: usize,
    pub spawns: [Iw6DecalSpawn; IW6_DECALS_MAX],
}

impl Default for Iw6Decals {
    fn default() -> Self {
        Self { len: 0, spawns: [Iw6DecalSpawn::default(); IW6_DECALS_MAX] }
    }
}

impl Iw6Decals {
    pub fn as_slice(&self) -> &[Iw6DecalSpawn] {
        &self.spawns[..self.len.min(IW6_DECALS_MAX)]
    }
}

static IW6_DECALS: std::sync::Mutex<Iw6Decals> = std::sync::Mutex::new(Iw6Decals {
    len: 0,
    spawns: [Iw6DecalSpawn { origin: [0.0; 3], axis: [[0.0; 3]; 3], radius: 0.0, color: 0, life_ms: 0, material: 0 }; IW6_DECALS_MAX],
});
static IW6_DECALS_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static IW6_DECAL_MATERIALS: std::sync::Mutex<Vec<std::sync::Arc<str>>> = std::sync::Mutex::new(Vec::new());

/// Whether a native Ghosts frame takes decal spawns (set by the native
/// draw; false off a Ghosts map and under `BO2MP_IW6_DECALS=0`).
pub fn set_iw6_decals_active(on: bool) {
    IW6_DECALS_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn iw6_decals_active() -> bool {
    IW6_DECALS_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The id of a decal material name (made on first sight, so a spawn after
/// that allocates nothing).
pub fn iw6_decal_material_id(name: &str) -> Option<u16> {
    let mut names = IW6_DECAL_MATERIALS.lock().ok()?;
    if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        return u16::try_from(i).ok();
    }
    let id = u16::try_from(names.len()).ok()?;
    names.push(std::sync::Arc::from(name));
    Some(id)
}

pub fn iw6_decal_material(id: u16) -> Option<std::sync::Arc<str>> {
    IW6_DECAL_MATERIALS.lock().ok()?.get(usize::from(id)).cloned()
}

/// Queues a spawn (the oldest is dropped when the frame's list is full).
pub fn push_iw6_decal(spawn: Iw6DecalSpawn) {
    if let Ok(mut g) = IW6_DECALS.lock() {
        if g.len >= IW6_DECALS_MAX {
            g.spawns.copy_within(1.., 0);
            g.len = IW6_DECALS_MAX - 1;
        }
        let at = g.len;
        g.spawns[at] = spawn;
        g.len += 1;
    }
}

/// Moves the queued spawns out (no allocation: `out` is reused).
pub fn take_iw6_decals(out: &mut Iw6Decals) {
    match IW6_DECALS.lock() {
        Ok(mut g) => {
            *out = *g;
            g.len = 0;
        }
        Err(_) => out.len = 0,
    }
}

/// bo2mp: the most distortion quads (Ghosts' heat haze and shockwave
/// elements) one frame hands the native renderer.
pub const IW6_DISTORTION_MAX: usize = 32;

/// One distortion sprite: its centre, the half-extent vectors of its two
/// sides (world units), native (BGRA) colour and its material
/// (`iw6_decal_material`'s name table).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6DistortionQuad {
    pub center: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub color: u32,
    pub material: u16,
}

/// The frame's distortion quads, fixed in size so that handing them over
/// allocates nothing.
#[derive(Clone, Copy, Debug)]
pub struct Iw6DistortionQuads {
    pub len: usize,
    pub quads: [Iw6DistortionQuad; IW6_DISTORTION_MAX],
}

impl Default for Iw6DistortionQuads {
    fn default() -> Self {
        Self { len: 0, quads: [Iw6DistortionQuad::default(); IW6_DISTORTION_MAX] }
    }
}

impl Iw6DistortionQuads {
    pub fn as_slice(&self) -> &[Iw6DistortionQuad] {
        &self.quads[..self.len.min(IW6_DISTORTION_MAX)]
    }
}

static IW6_DISTORTION: std::sync::Mutex<Iw6DistortionQuads> = std::sync::Mutex::new(Iw6DistortionQuads {
    len: 0,
    quads: [Iw6DistortionQuad { center: [0.0; 3], right: [0.0; 3], up: [0.0; 3], color: 0, material: 0 }; IW6_DISTORTION_MAX],
});
static IW6_DISTORTION_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a native Ghosts frame takes distortion quads (set by the native
/// draw).
pub fn set_iw6_distortion_active(on: bool) {
    IW6_DISTORTION_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn iw6_distortion_active() -> bool {
    IW6_DISTORTION_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Starts a frame's list (the effect system builds a frame's sprites after).
pub fn clear_iw6_distortion() {
    if let Ok(mut g) = IW6_DISTORTION.lock() {
        g.len = 0;
    }
}

/// Adds a quad (dropped when the frame's list is full).
pub fn push_iw6_distortion(quad: Iw6DistortionQuad) {
    if let Ok(mut g) = IW6_DISTORTION.lock()
        && g.len < IW6_DISTORTION_MAX
    {
        let at = g.len;
        g.quads[at] = quad;
        g.len += 1;
    }
}

/// Copies the frame's quads out (no allocation: `out` is reused).
pub fn iw6_distortion_into(out: &mut Iw6DistortionQuads) {
    match IW6_DISTORTION.lock() {
        Ok(g) => *out = *g,
        Err(_) => out.len = 0,
    }
}

/// bo2mp: the most players and guns one frame hands the Ghosts native
/// renderer as sun shadow proxies.
pub const IW6_CASTER_MAX: usize = 16;

/// One proxy capsule (a segment and a radius, world units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6SunCaster {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

/// The frame's proxies, fixed in size so that handing them over allocates
/// nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Iw6SunCasters {
    pub len: usize,
    pub list: [Iw6SunCaster; IW6_CASTER_MAX],
}

impl Iw6SunCasters {
    pub fn as_slice(&self) -> &[Iw6SunCaster] {
        &self.list[..self.len.min(IW6_CASTER_MAX)]
    }
}

static IW6_CASTERS: std::sync::Mutex<Iw6SunCasters> = std::sync::Mutex::new(Iw6SunCasters {
    len: 0,
    list: [Iw6SunCaster { a: [0.0; 3], b: [0.0; 3], radius: 0.0 }; IW6_CASTER_MAX],
});
static IW6_CASTERS_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a native Ghosts frame takes sun shadow proxies (set by the native
/// draw).
pub fn set_iw6_casters_active(on: bool) {
    IW6_CASTERS_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn iw6_casters_active() -> bool {
    IW6_CASTERS_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Starts a frame's list (the draw plan builder fills it).
pub fn clear_iw6_casters() {
    if let Ok(mut g) = IW6_CASTERS.lock() {
        g.len = 0;
    }
}

/// Adds a proxy unless one with the same segment is in the list or the list
/// is full.
pub fn push_iw6_caster(c: Iw6SunCaster) {
    if let Ok(mut g) = IW6_CASTERS.lock() {
        let n = g.len.min(IW6_CASTER_MAX);
        if g.list[..n].contains(&c) || n >= IW6_CASTER_MAX {
            return;
        }
        g.list[n] = c;
        g.len = n + 1;
    }
}

/// Copies the frame's proxies out (no allocation: `out` is reused).
pub fn iw6_casters_into(out: &mut Iw6SunCasters) {
    match IW6_CASTERS.lock() {
        Ok(g) => *out = *g,
        Err(_) => out.len = 0,
    }
}

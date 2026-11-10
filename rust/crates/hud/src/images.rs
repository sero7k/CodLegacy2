use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_core::AssetNamespace;
use asset_game::{HUD_CHROME_MENUS, MenuCatalog};
use asset_material::TS_COLOR_MAP;
use assets::{NamespaceTrees, SessionCompass};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gamemode_iw4::DAMAGE_FEEDBACK_SHADER;
use hud_iw4::{COMPASS_ENEMY_FIRING_PING_IMAGE, COMPASS_RADAR_LINE_IMAGE};

use crate::gaps::ImageMiss;

pub const HUD_CHROME_NAMESPACE: AssetNamespace = AssetNamespace::Iw4;

const DATA_SAMPLED_IMAGES: &[&str] = &[BLOOD_OVERLAY_MASK, BLOOD_OVERLAY_COLOR];

pub(crate) const BLOOD_OVERLAY_MASK: &str = "blood_defocus_mask";
pub(crate) const BLOOD_OVERLAY_COLOR: &str = "blood_defocus_color";

pub(crate) fn hud_sampling_for(name: &str) -> HudSampling {
    let key = cache_key(name);
    if DATA_SAMPLED_IMAGES.iter().any(|mask| *mask == key) {
        HudSampling::Data
    } else {
        HudSampling::Color
    }
}

fn cache_key(name: &str) -> String {
    asset_core::AssetRef::bare_name(name).to_ascii_lowercase()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HudSampling {
    #[default]
    Color,

    Data,
}

impl HudSampling {
    fn texture_format(self) -> TextureFormat {
        match self {
            Self::Color => TextureFormat::Rgba8UnormSrgb,
            Self::Data => TextureFormat::Rgba8Unorm,
        }
    }
}

type IwdKey = (AssetNamespace, String, HudSampling, Option<u8>);

type ZoneKey = (String, HudSampling, Option<u8>);

type CachedRgba = Option<(u32, u32, Vec<u8>)>;

fn iwd_key(ns: AssetNamespace, name: &str, sampling: HudSampling, sampler: Option<u8>) -> IwdKey {
    (ns, cache_key(name), sampling, sampler)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BloodMaterialBinding {
    pub state: [u32; 2],
    pub color_sampler: u8,
    pub mask_sampler: u8,
}

fn blood_material_binding(catalog: &MenuCatalog) -> Result<BloodMaterialBinding, String> {
    let pass = "splatter_alt/unlit/pass 0";
    let plan = catalog
        .material_2d_plans
        .get("splatter_alt")
        .ok_or_else(|| format!("{pass}: material plan missing"))?;
    if plan.unlit_pass_count != Some(1) || plan.unlit_pass_states.len() != 1 {
        return Err(format!(
            "{pass}: exactly one scanned pass and state row required"
        ));
    }
    if plan.textures.len() != 2 {
        return Err(format!("{pass}: expected two texture bindings"));
    }
    let [color, mask] = &plan.textures[..] else {
        unreachable!();
    };
    if color.semantic != TS_COLOR_MAP
        || mask.semantic != TS_COLOR_MAP
        || color.image.as_deref() != Some(BLOOD_OVERLAY_COLOR)
        || mask.image.as_deref() != Some(BLOOD_OVERLAY_MASK)
    {
        return Err(format!(
            "{pass}: color/mask color-map bindings missing or out of order"
        ));
    }
    Ok(BloodMaterialBinding {
        state: plan.unlit_pass_states[0],
        color_sampler: color.sampler_state,
        mask_sampler: mask.sampler_state,
    })
}

fn make_image(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    sampling: HudSampling,
    sampler: Option<u8>,
) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        sampling.texture_format(),
        RenderAssetUsages::default(),
    );
    if let Some(state) = sampler {
        image.sampler = ImageSampler::Descriptor(asset_material::sampler_from_iw4(state, 1, false));
    }
    image
}

#[derive(Resource, Default)]
pub struct HudImages {
    games_root: PathBuf,

    trees: NamespaceTrees,
    adopted_zone: Option<PathBuf>,

    map_namespace: AssetNamespace,
    by_name: HashMap<IwdKey, Option<Handle<Image>>>,
    rgba_by_name: HashMap<(AssetNamespace, String), CachedRgba>,
    zone_rgba: HashMap<String, (u32, u32, Arc<Vec<u8>>)>,
    zone_handles: HashMap<ZoneKey, Handle<Image>>,
    zone_image_name: HashMap<String, String>,
    material_images: HashMap<String, String>,
    zone_states: HashMap<String, Option<[u32; 2]>>,
    zone_srgb_reads: HashMap<String, bool>,
    blood_plan: Option<Result<BloodMaterialBinding, String>>,
    zone_installed: bool,
    zone_uploaded: bool,
    iwd_warmed: bool,
}

impl HudImages {
    pub fn set_games_root(&mut self, root: &Path) {
        if self.games_root == root {
            return;
        }
        self.games_root = root.to_path_buf();
        self.trees = NamespaceTrees::discover(&asset_transport::GamesRoot(self.games_root.clone()));
        self.adopted_zone = None;
        self.by_name.clear();
        self.rgba_by_name.clear();
        self.zone_uploaded = false;
        self.iwd_warmed = false;
        self.log_trees();
        prewarm_hmw_icons();
    }

    pub fn adopt_map_zone(&mut self, zone_ff: &Path) {
        if self.adopted_zone.as_deref() == Some(zone_ff) {
            return;
        }
        self.adopted_zone = Some(zone_ff.to_path_buf());
        let namespace = asset_transport::zone_game_for_path(zone_ff)
            .map_or(AssetNamespace::Iw4, AssetNamespace::from_zone_game);
        let mut trees = self.trees.clone();
        trees.adopt_zone(zone_ff);
        if trees == self.trees && namespace == self.map_namespace {
            return;
        }
        self.trees = trees;
        self.map_namespace = namespace;
        self.by_name.clear();
        self.rgba_by_name.clear();
        self.iwd_warmed = false;
        self.log_trees();
    }

    pub fn map_namespace(&self) -> AssetNamespace {
        self.map_namespace
    }

    fn log_trees(&self) {
        for line in self.trees.report_lines() {
            diag::info!(Ui, "hud: {line}");
        }
    }

    pub fn has_games_root(&self) -> bool {
        !self.games_root.as_os_str().is_empty()
    }

    pub fn install_zone_catalog(&mut self, catalog: &MenuCatalog) {
        if self.zone_installed {
            return;
        }
        self.zone_installed = true;
        self.zone_uploaded = false;
        self.blood_plan = Some(blood_material_binding(catalog));
        self.zone_srgb_reads.extend(
            catalog
                .material_srgb_reads
                .iter()
                .map(|(name, &srgb)| (cache_key(name), srgb)),
        );
        self.material_images.extend(
            catalog
                .material_images
                .iter()
                .map(|(material, image)| (cache_key(material), image.clone())),
        );
        for (name, state) in &catalog.material_state_bits {
            self.zone_states.insert(name.clone(), state.agreed());
            if catalog.zone_images.contains_key(name) && state.agreed().is_none() {
                diag::warn!(Ui, "hud material state gap: {name}: {state:?}");
            }
        }
        for (key, atlas) in &catalog.zone_images {
            self.zone_rgba
                .insert(key.clone(), (atlas.width, atlas.height, atlas.rgba.clone()));
            self.zone_image_name
                .insert(key.clone(), atlas.image.clone());
            self.zone_handles.retain(|(name, _, _), _| name != key);
            self.by_name.retain(|(_, name, _, _), _| name != key);
            self.rgba_by_name.retain(|(_, name), _| name != key);
        }
    }

    pub fn material_state_bits(&self, ns: AssetNamespace, name: &str) -> Option<[u32; 2]> {
        (ns == HUD_CHROME_NAMESPACE)
            .then(|| self.zone_states.get(&cache_key(name)).copied().flatten())
            .flatten()
    }

    pub(crate) fn blood_material_binding(&self) -> Result<BloodMaterialBinding, &str> {
        self.blood_plan
            .as_ref()
            .ok_or("splatter_alt/unlit/pass 0: menu catalog missing")?
            .as_ref()
            .map(|binding| *binding)
            .map_err(String::as_str)
    }

    pub fn zone_installed(&self) -> bool {
        self.zone_installed
    }

    pub fn zone_image_name(&self, material: &str) -> Option<&str> {
        let key = cache_key(material);
        self.zone_image_name.get(&key).map(String::as_str)
    }

    pub fn miss_reason(&self) -> ImageMiss {
        if self.has_games_root() || self.zone_installed || !self.trees.is_empty() {
            ImageMiss::NotDecoded
        } else {
            ImageMiss::NoGamesRoot
        }
    }

    pub fn get(
        &mut self,
        ns: AssetNamespace,
        name: &str,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        self.get_sampled(ns, name, HudSampling::Color, images)
    }

    pub(crate) fn get_native(
        &mut self,
        ns: AssetNamespace,
        name: &str,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let srgb = ns == HUD_CHROME_NAMESPACE
            && self
                .zone_srgb_reads
                .get(&cache_key(name))
                .copied()
                .unwrap_or(false);
        self.get_sampled(
            ns,
            name,
            if srgb {
                HudSampling::Color
            } else {
                HudSampling::Data
            },
            images,
        )
    }

    pub fn get_sampled(
        &mut self,
        ns: AssetNamespace,
        name: &str,
        sampling: HudSampling,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        self.get_sampled_with_sampler(ns, name, sampling, None, images)
    }

    pub(crate) fn get_sampled_with_sampler(
        &mut self,
        ns: AssetNamespace,
        name: &str,
        sampling: HudSampling,
        sampler: Option<u8>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let key = iwd_key(ns, name, sampling, sampler);
        if ns == HUD_CHROME_NAMESPACE && self.zone_states.get(&key.1) == Some(&None) {
            return None;
        }
        if let Some(cached) = self.by_name.get(&key) {
            return cached.clone();
        }
        if ns == HUD_CHROME_NAMESPACE {
            if let Some(handle) = self.upload_zone(name, sampling, sampler, images) {
                return Some(handle);
            }
        }
        let started = std::time::Instant::now();
        let decoded = self.decode_iwd(ns, name, sampling, sampler, images);
        slow_first_read("picture", name, started);
        self.by_name.insert(key, decoded.clone());
        decoded
    }

    pub fn ensure_rgba(&mut self, ns: AssetNamespace, name: &str) {
        let key = (ns, cache_key(name));
        if self.rgba_by_name.contains_key(&key) {
            return;
        }
        let started = std::time::Instant::now();
        self.ensure_rgba_uncached(key, ns, name);
        slow_first_read("pixels", name, started);
    }

    fn ensure_rgba_uncached(&mut self, key: (AssetNamespace, String), ns: AssetNamespace, name: &str) {
        if ns == HUD_CHROME_NAMESPACE {
            if let Some((width, height, rgba)) = self.zone_lookup(name) {
                self.rgba_by_name
                    .insert(key, Some((width, height, rgba.to_vec())));
                return;
            }
        }
        // bo2mp: Ghosts' pictures (its guns' kill icons) are in its zones.
        if ns == AssetNamespace::Iw6 {
            let bare = name.trim_start_matches(',');
            let found = if bare.to_ascii_lowercase().starts_with("compass_map_") {
                ghosts_minimap_rgba(bare)
            } else {
                ghosts_ui_rgba(name)
            };
            self.rgba_by_name.insert(key, found);
            return;
        }
        // bo2mp: Horizon MW's (its guns' kill icons) are in its weapon zones.
        if ns == AssetNamespace::H1 {
            let found = asset_audio::hmw_zone_dir()
                .and_then(|dir| fastfile_h1::ui_image::read_hmw_icon(&dir, &[name.trim_start_matches(',').to_owned()]));
            self.rgba_by_name.insert(key, found);
            return;
        }
        // bo2mp: Infinite Warfare's (its guns' kill icons) are in its
        // global_core_mp.
        #[cfg(feature = "iw7")]
        if ns == AssetNamespace::Iw7 {
            let found = asset_audio::iw7_zone_dir()
                .and_then(|dir| fastfile_iw7::ui_image::read_gun_icon(&dir, &[name.trim_start_matches(',').to_owned()]));
            self.rgba_by_name.insert(key, found);
            return;
        }
        let decoded = self.decode_iwd_rgba(ns, name);
        self.rgba_by_name.insert(key, decoded);
    }

    /// bo2mp: `ensure_rgba` for many pictures, the IWD ones decoded on
    /// every core at once.
    pub fn ensure_rgba_many(&mut self, wanted: &[(AssetNamespace, String)]) {
        let mut todo: Vec<(AssetNamespace, &str)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (ns, name) in wanted {
            let key = (*ns, cache_key(name));
            if self.rgba_by_name.contains_key(&key) || !seen.insert(key) {
                continue;
            }
            if *ns == HUD_CHROME_NAMESPACE || *ns == AssetNamespace::Iw6 || *ns == AssetNamespace::H1 || *ns == AssetNamespace::Iw7 {
                self.ensure_rgba(*ns, name);
            } else {
                todo.push((*ns, name.as_str()));
            }
        }
        if todo.is_empty() {
            return;
        }
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
        let chunk = todo.len().div_ceil(threads);
        let this = &*self;
        let decoded: Vec<((AssetNamespace, String), CachedRgba)> = std::thread::scope(|scope| {
            let jobs: Vec<_> = todo
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|&(ns, name)| ((ns, cache_key(name)), this.decode_iwd_rgba(ns, name)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
        });
        self.rgba_by_name.extend(decoded);
    }

    pub fn rgba(&self, ns: AssetNamespace, name: &str) -> Option<&(u32, u32, Vec<u8>)> {
        self.rgba_by_name
            .get(&(ns, cache_key(name)))
            .and_then(Option::as_ref)
    }

    pub fn warm_present_stems(
        &mut self,
        images: &mut Assets<Image>,
        catalog: Option<&MenuCatalog>,
        compass: Option<&SessionCompass>,
    ) {
        self.upload_pending_zone(images);
        if self.iwd_warmed {
            return;
        }
        if self.trees.is_empty() {
            return;
        }
        // bo2mp: the warm waits (frame by frame) for the archive indexes it
        // reads, built on their own thread rather than inside this frame.
        let mut ready = true;
        for ns in [HUD_CHROME_NAMESPACE, self.map_namespace] {
            if let Some(main) = self.trees.main_for(ns) {
                ready &= iwd_index_ready(main);
            }
        }
        if !ready {
            return;
        }

        let blood = self.blood_material_binding().ok();
        for (name, sampling, sampler) in [
            (
                BLOOD_OVERLAY_COLOR,
                HudSampling::Color,
                blood.map(|b| b.color_sampler),
            ),
            (
                BLOOD_OVERLAY_MASK,
                HudSampling::Data,
                blood.map(|b| b.mask_sampler),
            ),
            (DAMAGE_FEEDBACK_SHADER, HudSampling::Color, None),
            (COMPASS_ENEMY_FIRING_PING_IMAGE, HudSampling::Color, None),
            (COMPASS_RADAR_LINE_IMAGE, HudSampling::Color, None),
        ] {
            let _ = self.get_sampled_with_sampler(
                HUD_CHROME_NAMESPACE,
                name,
                sampling,
                sampler,
                images,
            );
        }
        if let Some(compass) = compass {
            if let Some(name) = compass.declaration.image.as_deref() {
                let ns = self.map_namespace;
                let _ = self.get(ns, name, images);
                self.ensure_rgba(ns, name);
            }
        }
        if let Some(catalog) = catalog {
            for font in catalog.fonts.values() {
                if !font.material.is_empty() {
                    let _ = self.get(HUD_CHROME_NAMESPACE, &font.material, images);
                }
            }
            for menu_name in HUD_CHROME_MENUS {
                let Some(menu) = catalog.get(menu_name) else {
                    continue;
                };
                if !menu.window_background.is_empty() {
                    let _ = self.get(HUD_CHROME_NAMESPACE, &menu.window_background, images);
                }
                for item in &menu.items {
                    if !item.background.is_empty() {
                        let _ = self.get(HUD_CHROME_NAMESPACE, &item.background, images);
                    }
                }
            }
        }
        self.iwd_warmed = true;
    }

    fn upload_pending_zone(&mut self, images: &mut Assets<Image>) {
        if self.zone_uploaded || !self.zone_installed {
            return;
        }
        let keys: Vec<String> = self.zone_rgba.keys().cloned().collect();
        let blood = self.blood_material_binding().ok();
        for key in keys {
            let sampling = hud_sampling_for(&key);
            let sampler = match key.as_str() {
                BLOOD_OVERLAY_COLOR => blood.map(|binding| binding.color_sampler),
                BLOOD_OVERLAY_MASK => blood.map(|binding| binding.mask_sampler),
                _ => None,
            };
            let _ = self.upload_zone(&key, sampling, sampler, images);
        }
        self.zone_uploaded = true;
    }

    fn decode_iwd(
        &self,
        ns: AssetNamespace,
        name: &str,
        sampling: HudSampling,
        sampler: Option<u8>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        match self.decode_iwd_rgba(ns, name) {
            Some((width, height, rgba)) => {
                Some(images.add(make_image(width, height, rgba, sampling, sampler)))
            }
            None => None,
        }
    }

    fn decode_iwd_rgba(&self, ns: AssetNamespace, name: &str) -> CachedRgba {
        if cache_key(name) == "white" {
            return Some((1, 1, vec![255; 4]));
        }
        // (bo2mp: World at War's and CoD4's guest guns: their `main` is
        // beside their common zone; they have no namespace tree.)
        let legacy_main;
        let main = match self.trees.main_for(ns) {
            Some(main) => main,
            None => {
                legacy_main = legacy_main_for(ns)?;
                legacy_main.as_path()
            }
        };
        let mapped = (ns == HUD_CHROME_NAMESPACE)
            .then(|| {
                self.material_images
                    .get(&cache_key(name))
                    .map(String::as_str)
            })
            .flatten();
        for image_name in mapped
            .filter(|image| *image != name)
            .into_iter()
            .chain(std::iter::once(name))
        {
            match asset_material::decode_ui_image_from_main(main, image_name) {
                Ok(Some(image)) => return Some(image),
                Ok(None) => {}
                Err(error) => {
                    diag::warn!(
                        Ui,
                        "hud: decode `{}:{image_name}` from {}: {error}",
                        ns.as_str(),
                        main.display()
                    );
                }
            }
        }
        None
    }

    fn zone_lookup(&self, name: &str) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        self.zone_rgba.get(&cache_key(name)).cloned()
    }

    fn upload_zone(
        &mut self,
        name: &str,
        sampling: HudSampling,
        sampler: Option<u8>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let key = (cache_key(name), sampling, sampler);
        if let Some(handle) = self.zone_handles.get(&key) {
            return Some(handle.clone());
        }
        let (width, height, rgba) = self.zone_lookup(name)?;
        let handle = images.add(make_image(
            width,
            height,
            rgba.as_ref().clone(),
            sampling,
            sampler,
        ));
        self.zone_handles.insert(key, handle.clone());
        Some(handle)
    }
}

/// bo2mp: true once `main`'s archive index is built (or its build gave up).
/// The first ask starts the build on its own thread: on the window thread it
/// held the first frame for a minute (MW2's main and iw4x DLC archives,
/// thousands of pictures, listed cold off his hard drive).
fn iwd_index_ready(main: &Path) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUILDS: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, Arc<AtomicBool>>>> =
        std::sync::OnceLock::new();
    if asset_transport::IwdIndex::is_cached(main) {
        return true;
    }
    let Ok(mut builds) = BUILDS.get_or_init(Default::default).lock() else {
        return true;
    };
    let done = builds.entry(main.to_path_buf()).or_insert_with(|| {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let main = main.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("hud iwd index".into())
            .spawn(move || {
                let _ = asset_transport::IwdIndex::open(&main);
                flag.store(true, Ordering::Release);
            });
        if spawned.is_err() {
            done.store(true, Ordering::Release);
        }
        done
    });
    done.load(Ordering::Acquire)
}

/// bo2mp: World at War's or CoD4's `main` (beside its multiplayer common
/// zone), for its guest guns' pictures.
fn legacy_main_for(ns: AssetNamespace) -> Option<std::path::PathBuf> {
    // bo2mp: found once per game. (Each lookup walks the games folders and
    // reads every common_mp's version: done per kill icon it held the main
    // thread for over half a minute while a match loaded.)
    static FOUND: std::sync::OnceLock<std::sync::Mutex<HashMap<AssetNamespace, Option<std::path::PathBuf>>>> =
        std::sync::OnceLock::new();
    let cache = FOUND.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&ns).cloned()) {
        return hit;
    }
    let found = find_legacy_main(ns);
    if let Ok(mut c) = cache.lock() {
        c.insert(ns, found.clone());
    }
    found
}

fn find_legacy_main(ns: AssetNamespace) -> Option<std::path::PathBuf> {
    let version = asset_transport::legacy_version(ns.as_str())?;
    let root = asset_transport::games_root_from_env().ok()?;
    let common = asset_transport::find_common_mp_for_envelope(&root, version).ok()?;
    Some(asset_transport::game_root_for_zone(&common.path).ok()?.join("main"))
}

/// bo2mp: a Ghosts map's minimap, read from the map's own zone.
fn ghosts_minimap_rgba(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let root = asset_transport::games_root_from_env().ok();
    let extra: Vec<std::path::PathBuf> = asset_transport::game_paths::get("ghosts").into_iter().collect();
    let install = fastfile_iw6::install::find_install(root.as_ref().map(|r| r.0.as_path()), &extra)?;
    match fastfile_iw6::ui_image::read_minimap(&install, name) {
        Ok(p) => Some(p),
        Err(e) => {
            diag::warn!(World, "bo2mp iw6: minimap `{name}`: {e}");
            None
        }
    }
}

/// bo2mp: a Ghosts 2-D picture by material name (`hud_icon_ak12`), read
/// from its ui_mp / common_core_mp zones when Ghosts is installed.
fn ghosts_ui_rgba(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    static INSTALL: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let install = INSTALL.get_or_init(|| {
        let root = asset_transport::games_root_from_env().ok();
        let extra: Vec<std::path::PathBuf> = asset_transport::game_paths::get("ghosts").into_iter().collect();
        fastfile_iw6::install::find_install(root.as_ref().map(|r| r.0.as_path()), &extra)
    });
    fastfile_iw6::ui_image::read_ui_image(install.as_ref()?, &[name.trim_start_matches(',').to_owned()])
}

/// bo2mp: Horizon MW's weapon icons come from walking all of its weapon
/// zones, once a run, the first time one is asked for (6.5 s on the
/// reference PC, inside the match start's kill icons). Done beside the menu
/// instead, so a match with its guns starts at once.
fn prewarm_hmw_icons() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let Some(dir) = asset_audio::hmw_zone_dir() else {
            return;
        };
        let _ = std::thread::Builder::new().name("hmw-icons".into()).spawn(move || {
            let _ = fastfile_h1::ui_image::read_hmw_icon(&dir, &[]);
        });
    });
}

/// A HUD picture read on first use that held the frame (finding in-match
/// stalls: these reads run on the main schedule).
fn slow_first_read(what: &str, name: &str, started: std::time::Instant) {
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    if ms > 15.0 {
        diag::warn!(Ui, "bo2mp slow: hud {what} {name} read on first use in {ms:.0} ms");
    }
}

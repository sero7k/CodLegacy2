use std::collections::HashSet;
use std::sync::Arc;

use bevy::prelude::*;

use crate::assemble::drawsurf::RuntimeLightmapHandles;

use super::world::WorldScene;

pub(crate) const IMAGE_BYTES_PER_OVERLAY_FRAME: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES_PER_SLICE: u64 = 128 * 1024 * 1024;
pub(crate) const IMAGES_PER_OVERLAY_FRAME: u32 = 16;

pub(crate) fn overlay_count_byte_capped(
    count: u32,
    bytes: u64,
    max_count: u32,
    byte_budget: u64,
) -> bool {
    count >= max_count && bytes >= byte_budget
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeDebug {
    Flat([u8; 4]),

    Faces,

    Mips { shift: usize },
}

const PROBE_FACE_COLOURS: [[u8; 4]; 6] = [
    [255, 0, 0, 255],
    [0, 255, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 255, 255],
    [0, 0, 255, 255],
    [255, 255, 0, 255],
];

const PROBE_MIP_COLOURS: [[u8; 4]; 8] = [
    [255, 0, 0, 255],
    [255, 128, 0, 255],
    [255, 255, 0, 255],
    [0, 255, 0, 255],
    [0, 255, 255, 255],
    [0, 0, 255, 255],
    [255, 0, 255, 255],
    [255, 255, 255, 255],
];

fn probe_debug_mode() -> Option<ProbeDebug> {
    use std::sync::OnceLock;
    static MODE: OnceLock<Option<ProbeDebug>> = OnceLock::new();
    *MODE.get_or_init(|| {
        let raw = codlegacy_env::var("CODLEGACY_PROBE_DEBUG").ok()?;
        match raw.trim() {
            "faces" => {
                diag::warn!(World, "reflection probe A/B: every cube layer flat, +X red -X cyan +Y green -Y magenta +Z blue -Z yellow");
                return Some(ProbeDebug::Faces);
            }
            "mips" => {
                diag::warn!(World, "reflection probe A/B: every mip flat, 0 red 1 orange 2 yellow 3 green 4 cyan 5 blue 6 magenta 7 white");
                return Some(ProbeDebug::Mips { shift: 0 });
            }
            "mips-shift" => {
                diag::warn!(World, "reflection probe A/B: every mip flat, palette rotated by one level (0 white 1 red 2 orange 3 yellow 4 green 5 cyan 6 blue 7 magenta)");
                return Some(ProbeDebug::Mips { shift: 1 });
            }
            _ => {}
        }
        let mut parts = raw.split(',').map(|part| part.trim().parse::<u8>());
        let (Some(Ok(r)), Some(Ok(g)), Some(Ok(b))) = (parts.next(), parts.next(), parts.next())
        else {
            diag::warn!(World, "unparseable CODLEGACY_PROBE_DEBUG={raw:?}; probes unchanged");
            return None;
        };
        diag::warn!(World, "reflection probe A/B: every probe forced to {r},{g},{b},255");
        Some(ProbeDebug::Flat([r, g, b, 255]))
    })
}

fn paint_probe_debug(image: &mut Image, mode: ProbeDebug) {
    let size = image.texture_descriptor.size.width;
    let levels = image.texture_descriptor.mip_level_count;
    let Some(data) = image.data.as_mut() else {
        diag::warn!(
            World,
            "CODLEGACY_PROBE_DEBUG: probe cube has no CPU data; left unpainted"
        );
        return;
    };
    let expected: usize = (0..levels)
        .map(|level| {
            let side = (size >> level).max(1) as usize;
            side * side * 4 * 6
        })
        .sum();
    if data.len() != expected {
        diag::warn!(
            World,
            "CODLEGACY_PROBE_DEBUG: probe cube is {} bytes, mip walk of {size}px x{levels} needs {expected}; left unpainted",
            data.len()
        );
        return;
    }
    let mut offset = 0usize;
    for level in 0..levels {
        let side = (size >> level).max(1) as usize;
        let level_bytes = side * side * 4;
        for face in 0..6usize {
            let colour = match mode {
                ProbeDebug::Flat(colour) => colour,
                ProbeDebug::Faces => PROBE_FACE_COLOURS[face],

                ProbeDebug::Mips { shift } if (level as usize) < PROBE_MIP_COLOURS.len() => {
                    let len = PROBE_MIP_COLOURS.len();
                    PROBE_MIP_COLOURS[(level as usize + len - shift % len) % len]
                }
                ProbeDebug::Mips { .. } => [128, 128, 128, 255],
            };
            for texel in data[offset..offset + level_bytes].chunks_exact_mut(4) {
                texel.copy_from_slice(&colour);
            }
            offset += level_bytes;
        }
    }
}

fn image_bytes(image: &Image) -> u64 {
    image
        .data
        .as_ref()
        .map(|data| data.len() as u64)
        .unwrap_or(0)
}

/// Every image one lightmap page hands over, summed. A page is six `add`s, and
/// `handed_bytes`, `bytes_total` and the budget that paces the overlay frames
/// are all owed all six of them.
fn lightmap_page_bytes(page: &super::world::WorldLightmap) -> u64 {
    page.primary_image.as_ref().map_or(0, image_bytes)
        + page.secondary_image.as_ref().map_or(0, image_bytes)
        + page.secondary_b_image.as_ref().map_or(0, image_bytes)
        + image_bytes(&page.ambient_image)
        + image_bytes(&page.directional_image)
        + image_bytes(&page.sun_mask_image)
}

#[derive(Resource, Default)]
pub struct ResidentGpuImages {
    profile_id: u64,
    by_variant: std::collections::HashMap<asset_material::ImageVariantId, Handle<Image>>,
    map: ResidentMapImages,
}

#[derive(Default)]
struct ResidentMapImages {
    products_id: u64,
    exact: Vec<Option<Handle<Image>>>,
    probes: Vec<Option<Handle<Image>>>,
    lightmaps: Vec<Option<RuntimeLightmapHandles>>,
}

impl ResidentMapImages {
    fn all_live(&self, images: &Assets<Image>) -> bool {
        let live = |handle: &Handle<Image>| images.get(handle.id()).is_some();
        self.exact.iter().chain(&self.probes).flatten().all(live)
            && self.lightmaps.iter().flatten().all(|page| {
                page.primary.iter().all(live)
                    && page.secondary.iter().all(live)
                    && page.secondary_b.iter().all(live)
                    && live(&page.ambient_diagnostic)
                    && live(&page.directional_diagnostic)
                    && live(&page.sun_mask_diagnostic)
            })
    }
}

impl ResidentGpuImages {
    /// Lets go of the map kept for a same-map load once the walk it came
    /// from is no longer the one meant to stay (`gpu_resident`). Until the
    /// next map's own upload replaced it, its textures, probes and lightmaps
    /// stayed on the card and in the asset server through the whole walk.
    fn forget_dropped_map(&mut self, gpu_resident: u64) -> Option<u64> {
        let products_id = self.map.products_id;
        if products_id == 0 || products_id == gpu_resident {
            return None;
        }
        self.map = ResidentMapImages::default();
        Some(products_id)
    }

    fn use_profile(&mut self, profile_id: u64) {
        if self.profile_id != profile_id {
            self.by_variant.clear();
            self.profile_id = profile_id;
        }
    }

    fn is_live(&self, variant: asset_material::ImageVariantId, images: &Assets<Image>) -> bool {
        self.by_variant.get(&variant).is_some_and(|handle| images.get(handle.id()).is_some())
    }

    fn get_live(
        &mut self,
        variant: asset_material::ImageVariantId,
        images: &Assets<Image>,
    ) -> Option<Handle<Image>> {
        let handle = self.by_variant.get(&variant)?;
        if images.get(handle.id()).is_some() {
            Some(handle.clone())
        } else {
            self.by_variant.remove(&variant);
            None
        }
    }
}

#[derive(Default)]
pub struct WorldImageUpload {
    pub done: u32,
    pub total: u32,
    /// Bytes handed to `Assets<Image>`. **Not** bytes on the GPU: the asset
    /// server takes the image here, the render world turns it into a texture
    /// some frames later, and the driver copies it later still. Calling this
    /// "uploaded" is how a handoff rate gets reported as a transfer rate.
    ///
    /// A slot that takes another slot's handle hands nothing over and is not
    /// in here; its bytes are `reused_handle_bytes`.
    pub handed_bytes: u64,
    pub bytes_total: u64,
    /// The longest single handoff, and how big it was. A frame budget can only
    /// be kept to the granularity of the largest thing that cannot be split —
    /// so when a 40 ms slice runs to 227 ms, this is the number that says
    /// whether the budget was ignored or was never keepable.
    pub largest_step_ns: u64,
    pub largest_step_bytes: u64,
    pub handoff_ns: u64,
    pub steps: u64,

    pub skipped: u32,

    pub pipeline_world_materials: Arc<std::collections::HashSet<u16>>,

    pub pipeline_smodel_materials: Arc<std::collections::HashSet<u16>>,
    exact_images: Vec<Option<Arc<Image>>>,
    exact_variants: Vec<Option<asset_material::ImageVariantId>>,
    /// One asset per prepared variant. Two catalog slots that hold the same
    /// variant hold byte-identical images with the same sampler and colour
    /// space; giving each its own `Assets<Image>` entry uploads the same
    /// texture twice and keeps two copies of it resident, so the second slot
    /// takes a clone of the first one's handle and drops its own copy.
    exact_by_variant: std::collections::HashMap<asset_material::ImageVariantId, Handle<Image>>,
    /// Slots that took another slot's handle, and the bytes that saved.
    pub reused_handles: u32,
    pub reused_handle_bytes: u64,
    pub reused_common_handles: u32,
    pub reused_common_bytes: u64,
    exact_handles: Vec<Option<Handle<Image>>>,
    exact_common: Vec<bool>,
    common_profile_id: u64,
    products_id: u64,
    pub resident: bool,
    exact_at: usize,
    probes: Vec<Option<Image>>,
    probe_handles: Vec<Option<Handle<Image>>>,
    probe_at: usize,
    lightmaps: Vec<Option<super::world::WorldLightmap>>,
    lightmap_handles: Vec<Option<RuntimeLightmapHandles>>,
    lightmap_at: usize,
    upload_stage: Option<asset_transport::StageHandle>,
    reachable_exact: HashSet<u32>,
    /// The largest picture side handed over this load (`u32::MAX`: as
    /// stored): the material pictures that are reachable, held to a share
    /// of the card's memory (`picture_budget`).
    pub picture_cap: u32,
    /// Pictures that lost top levels to the cap, and the bytes they lost.
    pub capped_images: u32,
    pub capped_bytes: u64,
}

impl WorldImageUpload {
    pub fn unfinished(&self) -> bool {
        self.done < self.total
    }

    pub fn tess_handles(
        &self,
    ) -> (
        &[Option<Handle<Image>>],
        &[Option<RuntimeLightmapHandles>],
        &[Option<Handle<Image>>],
    ) {
        (
            &self.exact_handles,
            &self.lightmap_handles,
            &self.probe_handles,
        )
    }

    pub fn exact_handles(&self) -> &[Option<Handle<Image>>] {
        &self.exact_handles
    }

    pub fn probe_handles(&self) -> &[Option<Handle<Image>>] {
        &self.probe_handles
    }

    pub fn lightmap_handles(&self) -> &[Option<RuntimeLightmapHandles>] {
        &self.lightmap_handles
    }

    pub fn take_stage(&mut self) -> Option<asset_transport::StageHandle> {
        self.upload_stage.take()
    }

    pub fn arm(&mut self, scene: &mut WorldScene, progress: Option<&asset_transport::LoadProgress>) {
        self.pipeline_world_materials = Arc::new(
            scene
                .batches
                .iter()
                .filter_map(|batch| batch.material)
                .filter_map(|id| {
                    scene
                        .runtime_material_catalog
                        .derived(id)
                        // bo2zm: a material with no technique set (Black Ops
                        // II's) never gets an exact pipeline; it draws through
                        // the fallback pass, so the spawn must not wait on one.
                        .filter(|material| !material.technique_set.is_empty())
                        .map(|material| material.asset_id.0)
                })
                .collect(),
        );
        self.pipeline_smodel_materials = Arc::new(
            scene
                .static_model_meshes
                .iter()
                .chain(scene.sky_model.iter())
                .flat_map(|mesh| mesh.surface_materials())
                .flatten()
                .filter_map(|id| {
                    scene
                        .runtime_material_catalog
                        .derived(id)
                        // bo2zm: a material with no technique set (Black Ops
                        // II's) never gets an exact pipeline; it draws through
                        // the fallback pass, so the spawn must not wait on one.
                        .filter(|material| !material.technique_set.is_empty())
                        .map(|material| material.asset_id.0)
                })
                .collect(),
        );
        self.exact_images = std::mem::take(&mut scene.exact_material_images);
        self.exact_common = std::mem::take(&mut scene.exact_material_common);
        self.exact_common.resize(self.exact_images.len(), false);
        self.common_profile_id = scene.common_profile_id;
        self.products_id = scene.products_id;
        self.resident = false;
        self.exact_variants = std::mem::take(&mut scene.exact_material_variants);
        self.exact_variants.resize(self.exact_images.len(), None);
        self.exact_by_variant.clear();
        self.reused_handles = 0;
        self.reused_handle_bytes = 0;
        self.exact_handles = vec![None; self.exact_images.len()];
        self.reachable_exact = reachable_exact_image_slots(
            &scene.runtime_material_catalog,
            &scene.exact_material_names,
        );

        self.reachable_exact.extend(
            scene
                .primary_light_attenuation
                .iter()
                .filter_map(|light| light.image),
        );
        self.reachable_exact.extend(
            scene
                .dynamic_light
                .and_then(|light| light.attenuation.image),
        );
        self.reachable_exact.extend(scene.outdoor_image);
        // bo2mp: the pattern camos' pictures (Black Ops III's, Black Ops',
        // Black Ops II's own patterns): no material binds them, the gun
        // shader lays them from the camo table, so without this they were
        // never handed to the GPU; a gun wearing one lost its camo and, the
        // specular slot carrying the pattern, drew without its shine maps
        // (log: `bo2mp shine maps not ready: ... specular [not uploaded]`).
        // `BO2MP_CAMO_PICTURES=0` leaves them out, as before.
        if !self.reachable_exact.is_empty() && camo_pictures_uploaded() {
            let camos = camo_pattern_slots(self.exact_images.len());
            if !camos.is_empty() {
                diag::info!(World, "bo2mp camo pictures: {} pattern camo pictures reach the GPU with the map's", camos.len());
            }
            self.reachable_exact.extend(camos);
        }
        if let Some(sun) = scene.sun_effects {
            self.reachable_exact.extend(sun.sprite_image);
            self.reachable_exact.extend(sun.flare_image);
            self.pipeline_world_materials = Arc::new({
                let mut set = (*self.pipeline_world_materials).clone();
                // (A Black Ops II-drawn material, a Black Ops III map's own
                // sun sprite, has no world pipeline of its own to warm: the
                // sun pass draws its picture; demanding one left the
                // world's pipeline wait never done.)
                for material in [sun.sprite_material, sun.flare_material]
                    .into_iter()
                    .flatten()
                    .filter(|&m| {
                        scene
                            .runtime_material_catalog
                            .derived(assets::MaterialIndex::from_order(m as usize))
                            .is_none_or(|d| d.t6_draw.is_none())
                    })
                {
                    if let Ok(id) = u16::try_from(material) {
                        set.insert(id);
                    }
                }
                set
            });
        }
        self.probes = std::mem::take(&mut scene.reflection_probes);
        self.probe_handles = vec![None; self.probes.len()];
        self.lightmaps = std::mem::take(&mut scene.lightmaps);
        self.lightmap_handles = vec![None; self.lightmaps.len()];
        self.total = (self.exact_images.len()
            + self.probes.len()
            + self.lightmaps.iter().filter(|page| page.is_some()).count())
            as u32;
        self.done = 0;
        self.skipped = 0;
        self.reused_common_handles = 0;
        self.reused_common_bytes = 0;
        self.handed_bytes = 0;
        self.bytes_total = self
            .exact_images
            .iter()
            .flatten()
            .map(|image| image_bytes(image))
            .sum::<u64>()
            + self.probes.iter().flatten().map(image_bytes).sum::<u64>()
            + self
                .lightmaps
                .iter()
                .flatten()
                .map(lightmap_page_bytes)
                .sum::<u64>();
        self.exact_at = 0;
        self.probe_at = 0;
        self.lightmap_at = 0;
        self.capped_images = 0;
        self.capped_bytes = 0;
        self.picture_cap = self.cap_for_card();
        // A rearm after a load that never got here leaves the old handle
        // behind; it was interrupted, not finished.
        if let Some(stage) = self.upload_stage.take() {
            stage.cancel();
        }
        if let Some(progress) = progress {
            self.upload_stage =
                Some(progress.begin(asset_transport::StageId::WorldImages, Some(u64::from(self.total))));
        }
    }

    /// The picture cap this load's reachable material pictures need to fit
    /// the card's picture budget (`u32::MAX`: no budget, or they fit).
    fn cap_for_card(&self) -> u32 {
        let Some(budget) = super::picture_budget::current_budget() else {
            return u32::MAX;
        };
        let sizes: Vec<super::picture_budget::PictureSize> = self
            .exact_images
            .iter()
            .enumerate()
            .filter(|(slot, _)| self.reachable_exact.is_empty() || self.reachable_exact.contains(&(*slot as u32)))
            .filter_map(|(_, image)| image.as_deref().map(super::picture_budget::size_of))
            .collect();
        let total: u64 = sizes.iter().map(|p| p.bytes).sum();
        let cap = super::picture_budget::cap_for_budget(&sizes, budget);
        if cap != u32::MAX {
            diag::info!(
                World,
                "world images: {} reachable material pictures hold {:.0} MB, over the card's picture budget of {:.0} MB: pictures over {cap} a side lose their top levels (BO2MP_PICTURE_BUDGET_MB sets the budget, 0 none)",
                sizes.len(),
                total as f64 / (1024.0 * 1024.0),
                budget as f64 / (1024.0 * 1024.0),
            );
        }
        cap
    }

}

/// The picture as handed over: its top levels dropped to `cap`, counted
/// in `capped_images` and `capped_bytes` (the bytes it lost).
/// The picture as it goes to the asset server: capped, and taken whole
/// (not copied) when the cap leaves it as it is.
fn handed_image(cap: u32, image: Image, capped_images: &mut u32, capped_bytes: &mut u64) -> Image {
    let mut handed = match super::picture_budget::capped(&image, cap) {
        Some(capped) => {
            *capped_images = capped_images.saturating_add(1);
            *capped_bytes = capped_bytes.saturating_add(image_bytes(&image).saturating_sub(image_bytes(&capped)));
            capped
        }
        None => image,
    };
    if render_world_only() {
        handed.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    }
    handed
}

/// Match memory: a map's pictures were added for both worlds, so the main
/// world kept every one's pixels after the card had them (380-475 MiB in a
/// match, growing map to map) and each was copied once more to go to the
/// card. Added for the render world only, the pixels move to the card and
/// the main world keeps the picture's size and format alone; nothing reads
/// a map picture's pixels back from the asset server. `BO2MP_MAP_IMAGES_RENDER_ONLY=1`
/// turns it on (off by default until measured).
/// The decoded picture, taken whole when nothing else holds it (not a
/// copy); `BO2MP_MAP_IMAGES_COPY=1` copies it as before.
fn handover(image: Arc<Image>) -> Image {
    static COPY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *COPY.get_or_init(|| codlegacy_env::var("BO2MP_MAP_IMAGES_COPY").is_ok_and(|v| v.trim() == "1")) {
        (*image).clone()
    } else {
        Arc::unwrap_or_clone(image)
    }
}

fn render_world_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("BO2MP_MAP_IMAGES_RENDER_ONLY").is_ok_and(|v| v.trim() == "1"))
}

impl WorldImageUpload {
    /// One handoff that could not be interrupted. Kept as a maximum rather
    /// than a histogram: the question is what the smallest keepable budget is,
    /// and that is the largest step, not its distribution.
    fn note_step(&mut self, took: std::time::Duration, bytes: u64) {
        let ns = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
        self.steps = self.steps.saturating_add(1);
        self.handoff_ns = self.handoff_ns.saturating_add(ns);
        if ns > self.largest_step_ns {
            self.largest_step_ns = ns;
            self.largest_step_bytes = bytes;
        }
    }

    pub fn until(
        &mut self,
        images: &mut Assets<Image>,
        common: &mut ResidentGpuImages,
        deadline: Option<std::time::Instant>,
        max_this_frame: u32,
        paced_byte_budget: u64,
    ) -> bool {
        common.use_profile(self.common_profile_id);
        if self.done == 0 && self.take_resident(common, images) {
            return true;
        }
        let start_done = self.done;
        let start_bytes = self.handed_bytes;
        let byte_budget = if max_this_frame == u32::MAX {
            u64::MAX
        } else {
            paced_byte_budget
        };
        let capped = |done: u32, bytes: u64| {
            deadline.is_some_and(|end| std::time::Instant::now() >= end)
                || overlay_count_byte_capped(
                    done.saturating_sub(start_done),
                    bytes.saturating_sub(start_bytes),
                    max_this_frame,
                    byte_budget,
                )
        };
        let mut stepped = false;
        while self.exact_at < self.exact_images.len() {
            if stepped && capped(self.done, self.handed_bytes) {
                return false;
            }
            let image = self.exact_images[self.exact_at].take();
            let slot = self.exact_at as u32;
            if !self.reachable_exact.is_empty() && !self.reachable_exact.contains(&slot) {
                self.exact_at += 1;
                self.done = self.done.saturating_add(1);
                self.skipped = self.skipped.saturating_add(1);
                stepped = true;
                self.sync_stage();
                continue;
            }
            let bytes = image.as_ref().map(|image| image_bytes(image)).unwrap_or(0);
            let step = std::time::Instant::now();
            let mut handed = bytes;
            let variant = self.exact_variants[self.exact_at];
            let common_owned = self.exact_common[self.exact_at];
            let cap = self.picture_cap;
            self.exact_handles[self.exact_at] = image.and_then(|image| {
                // A slot whose variant is already an asset takes that handle
                // and lets its own copy go: the two are the same texels under
                // the same sampler, and a second `add` is a second texture.
                let Some(variant) = variant else {
                    let handed_over = handed_image(cap, handover(image), &mut self.capped_images, &mut self.capped_bytes);
                    handed = image_bytes(&handed_over);
                    return Some(images.add(handed_over));
                };
                if let Some(handle) = self.exact_by_variant.get(&variant) {
                    if common_owned && self.common_profile_id != 0 {
                        common.by_variant.insert(variant, handle.clone());
                    }
                    self.reused_handles = self.reused_handles.saturating_add(1);
                    self.reused_handle_bytes = self.reused_handle_bytes.saturating_add(bytes);
                    // Nothing was handed to the asset server: the slot took a
                    // handle that already existed.
                    handed = 0;
                    return Some(handle.clone());
                }
                if common_owned
                    && self.common_profile_id != 0
                    && let Some(handle) = common.get_live(variant, images)
                {
                    self.reused_handles = self.reused_handles.saturating_add(1);
                    self.reused_handle_bytes = self.reused_handle_bytes.saturating_add(bytes);
                    self.reused_common_handles = self.reused_common_handles.saturating_add(1);
                    self.reused_common_bytes = self.reused_common_bytes.saturating_add(bytes);
                    handed = 0;
                    self.exact_by_variant.insert(variant, handle.clone());
                    return Some(handle);
                }
                // (A common picture whose pixels were let go once the card
                // had it, and whose card texture is gone too: an `add` of it
                // would be a blank texture. It is built again from the common
                // set's kept payload; with none kept the slot stays empty, as
                // a picture that never decoded, and the common set is
                // prepared again next load.)
                let source: Image = if common_owned && image.data.is_none() {
                    match assets::rebuild_common_image(self.common_profile_id, variant) {
                        Some(again) => again,
                        None => {
                            assets::forget_stripped_common(self.common_profile_id);
                            handed = 0;
                            return None;
                        }
                    }
                } else {
                    handover(image)
                };
                let handed_over = handed_image(cap, source, &mut self.capped_images, &mut self.capped_bytes);
                handed = image_bytes(&handed_over);
                let handle = images.add(handed_over);
                if common_owned && self.common_profile_id != 0 {
                    common.by_variant.insert(variant, handle.clone());
                }
                self.exact_by_variant.insert(variant, handle.clone());
                Some(handle)
            });
            self.handed_bytes += handed;
            self.note_step(step.elapsed(), handed);
            self.exact_at += 1;
            self.done = self.done.saturating_add(1);
            stepped = true;
            self.sync_stage();
        }
        while self.probe_at < self.probes.len() {
            if stepped && capped(self.done, self.handed_bytes) {
                return false;
            }
            let image = self.probes[self.probe_at].take();
            let bytes = image.as_ref().map(image_bytes).unwrap_or(0);
            self.handed_bytes += bytes;
            let step = std::time::Instant::now();
            self.probe_handles[self.probe_at] = image.map(|mut image| {
                if let Some(mode) = probe_debug_mode() {
                    paint_probe_debug(&mut image, mode);
                }
                if render_world_only() {
                    image.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
                }
                images.add(image)
            });
            self.note_step(step.elapsed(), bytes);
            self.probe_at += 1;
            self.done = self.done.saturating_add(1);
            stepped = true;
            self.sync_stage();
        }
        while self.lightmap_at < self.lightmaps.len() {
            if stepped && capped(self.done, self.handed_bytes) {
                return false;
            }
            let page = self.lightmaps[self.lightmap_at].take();
            let bytes = page.as_ref().map_or(0, lightmap_page_bytes);
            let step = std::time::Instant::now();
            self.lightmap_handles[self.lightmap_at] = page.map(|lightmap| {
                diag::info!(
                    World,
                    "world lightmap[{}]: secondary={} 2x{}x{} mask={} {}x{}",
                    self.lightmap_at,
                    lightmap.ambient_source_name,
                    lightmap.ambient_size.x,
                    lightmap.ambient_size.y,
                    lightmap.sun_mask_source_name,
                    lightmap.sun_mask_size.x,
                    lightmap.sun_mask_size.y
                );
                RuntimeLightmapHandles {
                    primary: lightmap.primary_image.map(|image| images.add(image)),
                    secondary: lightmap.secondary_image.map(|image| images.add(image)),
                    secondary_b: lightmap.secondary_b_image.map(|image| images.add(image)),
                    ambient_diagnostic: images.add(lightmap.ambient_image),
                    directional_diagnostic: images.add(lightmap.directional_image),
                    sun_mask_diagnostic: images.add(lightmap.sun_mask_image),
                }
            });
            // One lightmap page is six adds and they cannot be split, so the
            // page is the indivisible unit here, not the image — and its bytes
            // are the six summed, which is what the budget has to be told
            // about even though it cannot stop in the middle of them.
            self.handed_bytes += bytes;
            self.note_step(step.elapsed(), bytes);
            if self.lightmap_handles[self.lightmap_at].is_some() {
                self.done = self.done.saturating_add(1);
            }
            self.lightmap_at += 1;
            stepped = true;
            self.sync_stage();
        }
        // Every slot has been handed over: this is the boundary the owner
        // meant, not merely `done == total`.
        if self.products_id != 0 {
            common.map = ResidentMapImages {
                products_id: self.products_id,
                exact: self.exact_handles.clone(),
                probes: self.probe_handles.clone(),
                lightmaps: self.lightmap_handles.clone(),
            };
            // The asset server has its own copy of every texture now: the
            // map kept for a same-map load lets its decoded ones go.
            assets::note_gpu_resident(self.products_id);
            if let Some((count, freed)) = assets::release_resident_cpu_images(self.products_id) {
                // Handed back to Windows now, not at the end of the load: the
                // textures' upload and the pipelines still to come would
                // otherwise sit on top of the freed images.
                let trim_ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
                diag::info!(
                    World,
                    "resident map: walk #{} let go of {count} decoded images ({:.0} MB freed) once the card had them (trim {trim_ms:.1}ms)",
                    self.products_id,
                    freed as f64 / (1024.0 * 1024.0)
                );
            }
            // (So does the common set, for its pictures the card holds:
            // `BO2MP_COMMON_CPU_STRIP=1`.)
            if let Some((count, freed)) =
                assets::release_common_cpu_images(self.common_profile_id, &|variant| common.is_live(variant, images))
            {
                let trim_ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
                diag::info!(
                    World,
                    "common set: let go of {count} decoded pictures' pixels ({:.0} MB freed) once the card had them (trim {trim_ms:.1}ms)",
                    freed as f64 / (1024.0 * 1024.0)
                );
            }
            // (The shared Black Ops II pictures the card holds stay for the
            // next map: `CODLEGACY_T6_CARD_KEEP=0` the old way.)
            if let Some(held) = assets::note_t6_card_held(&|variant| common.is_live(variant, images))
                && held > 0
            {
                diag::info!(World, "t6 card keep: {held} shared pictures stay on the card for the next map");
            }
        }
        if let Some(stage) = self.upload_stage.take() {
            stage.set_completed(u64::from(self.done));
            stage.set_bytes(self.handed_bytes);
            stage.done();
        }
        true
    }

    fn take_resident(&mut self, common: &mut ResidentGpuImages, images: &Assets<Image>) -> bool {
        let map = &common.map;
        let same = self.products_id != 0
            && map.products_id == self.products_id
            && map.exact.len() == self.exact_handles.len()
            && map.probes.len() == self.probe_handles.len()
            && map.lightmaps.len() == self.lightmap_handles.len();
        if !same || !map.all_live(images) {
            common.map = ResidentMapImages::default();
            assets::note_gpu_resident(0);
            return false;
        }
        self.exact_handles.clone_from(&map.exact);
        self.probe_handles.clone_from(&map.probes);
        self.lightmap_handles.clone_from(&map.lightmaps);
        self.exact_images.clear();
        self.probes.clear();
        self.lightmaps.clear();
        self.exact_at = 0;
        self.probe_at = 0;
        self.lightmap_at = 0;
        self.done = self.total;
        self.resident = true;
        if let Some(stage) = self.upload_stage.take() {
            stage.set_completed(u64::from(self.done));
            stage.reuse();
        }
        true
    }

    fn sync_stage(&self) {
        if let Some(stage) = &self.upload_stage {
            stage.set_completed(u64::from(self.done));
            stage.set_bytes(self.handed_bytes);
        }
    }
}

fn reachable_exact_image_slots(
    catalog: &crate::assemble::drawsurf::RuntimeMaterialCatalog,
    names: &[String],
) -> HashSet<u32> {
    let mut ids = HashSet::new();
    for (index, name) in names.iter().enumerate() {
        if name.starts_with('$') {
            ids.insert(index as u32);
        }
    }
    for material in &catalog.materials {
        for (_, texture) in &material.textures {
            if let Some(binding) = texture {
                ids.insert(binding.image.0);
            }
        }
    }
    if ids.is_empty() {
        ids.extend(0..names.len() as u32);
    }
    ids
}

/// bo2mp: whether the pattern camos' pictures are uploaded with the map's
/// (`BO2MP_CAMO_PICTURES=0`: not, as before).
fn camo_pictures_uploaded() -> bool {
    std::env::var("BO2MP_CAMO_PICTURES").map_or(true, |v| v.trim() != "0")
}

/// bo2mp: the pattern camos' picture slots within the catalog's `slots`
/// (a stale table's slot past the end is left out).
fn camo_pattern_slots(slots: usize) -> Vec<u32> {
    asset_core::weapon_camos::pattern_images()
        .into_iter()
        .filter(|&slot| (slot as usize) < slots)
        .collect()
}

/// The old map's images go when its resident copy is dropped for another
/// map (before that map's walk), not when the new map's upload begins.
pub(crate) fn release_dropped_map_images(mut resident: ResMut<ResidentGpuImages>) {
    if resident.map.products_id == 0 {
        return;
    }
    if let Some(products_id) = resident.forget_dropped_map(assets::gpu_resident()) {
        diag::info!(
            World,
            "resident map: walk #{products_id} let go of its textures, probes and lightmaps before the next walk"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(products_id: u64) -> ResidentGpuImages {
        ResidentGpuImages {
            map: ResidentMapImages {
                products_id,
                exact: vec![Some(Handle::default()), None],
                probes: vec![Some(Handle::default())],
                lightmaps: Vec::new(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn camo_pattern_pictures_within_the_catalog_are_reachable() {
        use asset_core::weapon_camos::{PatternCamo, T7_FIRST, install_t7_patterns};
        install_t7_patterns(
            [(T7_FIRST + 1, PatternCamo::still(4, 1.0, true)), (T7_FIRST + 2, PatternCamo::still(999, 1.0, true))]
                .into_iter()
                .collect(),
        );
        assert_eq!(camo_pattern_slots(10), vec![4]);
        assert_eq!(camo_pattern_slots(1000), vec![4, 999]);
        install_t7_patterns(Default::default());
        assert!(camo_pattern_slots(1000).is_empty());
    }

    #[test]
    fn the_map_still_resident_is_kept() {
        let mut resident = kept(7);
        assert_eq!(resident.forget_dropped_map(7), None);
        assert_eq!(resident.map.products_id, 7);
        assert_eq!(resident.map.exact.len(), 2);
    }

    #[test]
    fn a_dropped_map_lets_go_of_its_handles() {
        let mut resident = kept(7);
        assert_eq!(resident.forget_dropped_map(0), Some(7));
        assert_eq!(resident.map.products_id, 0);
        assert!(resident.map.exact.is_empty() && resident.map.probes.is_empty());
        // Nothing left to drop the next frame.
        assert_eq!(resident.forget_dropped_map(0), None);
    }

    #[test]
    fn no_map_kept_is_left_alone() {
        let mut resident = ResidentGpuImages::default();
        assert_eq!(resident.forget_dropped_map(0), None);
        assert_eq!(resident.forget_dropped_map(3), None);
    }
}

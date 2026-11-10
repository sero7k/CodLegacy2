//! bo2mp: Black Ops III's surface look for its maps' world surfaces and
//! placed models (`fastfile_t7::look`): each lit material's colour map
//! tinted (its `colorTint` by its tint mask), its metal's diffuse and its
//! ambient occlusion taken out, its normal map (with its detail normal
//! map) and its specular map (reflectance from its metal mask and
//! `specColorTint`, gloss from its gloss map over its `glossRange` less its
//! normal map's variance), in the form Black Ops II's lit draw reads. The
//! reflections the specular maps show come from the lighting side's probes
//! (`T6_PROBES_MATERIAL`); without them the renderer draws the colour maps
//! alone, as before.
//!
//! A self-contained path: `BO2MP_T7_MATERIALS=0` turns it off (colour maps
//! only, as before). Other games' maps never come here.

use std::collections::HashMap;
use std::sync::Arc;

use asset_core::{AssetNamespace, AssetRef, T6Blend, T6Draw};
use asset_material::{AuthoredImage, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP, TS_NORMAL_MAP, TS_SPECULAR_MAP};
use fastfile_t7::look::{Cover, Look};
use fastfile_t7::map::T7Map;
use fastfile_t7::texture::ImageLevels;

/// `R_HashString` of Black Ops II's normal and specular map slots.
const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;

/// The largest specular map kept (gloss and reflectance vary slowly).
const SPECULAR_MAX: u32 = 256;

/// Whether Black Ops III maps get their own look (`BO2MP_T7_MATERIALS=0`:
/// colour maps only).
pub(super) fn on() -> bool {
    std::env::var("BO2MP_T7_MATERIALS").map_or(true, |v| v.trim() != "0")
}

/// A material's look, linked: (colour, normal, specular) catalog images.
pub(super) type Linked = [Option<usize>; 3];

/// What `Looks` did, for the report.
#[derive(Default)]
pub(super) struct Stats {
    pub materials: usize,
    pub normal: usize,
    pub specular: usize,
    /// Tinted, metal, gloss map, occlusion, detail normals.
    pub used: [usize; 5],
    /// Emissive surfaces drawn from their emissive maps.
    pub emissive: usize,
    pub failures: Vec<String>,
}

/// The looks of a map's materials, each made once.
pub(super) struct Looks {
    max: u32,
    done: HashMap<usize, Linked>,
    pub stats: Stats,
    /// bo2mp: reveal decals keep their raw reveal map (for Black Ops III's
    /// own light, whose draw covers them by the vertex alpha,
    /// `super::t7_light`); those made so, with their soft edge and ramp.
    raw_reveal: bool,
    pub reveals: HashMap<usize, [f32; 2]>,
    /// bo2mp (load speed): the looks `link` will want, in the order it
    /// will want them ([`Self::ahead`]); each made a batch at a time on
    /// worker threads, then linked in `link`'s own order (the catalog's
    /// slots come out as before).
    wanted: Vec<(usize, Cover)>,
    at: HashMap<usize, usize>,
    next: usize,
    made: HashMap<usize, (Cover, Result<Look, String>)>,
}

/// Looks made side by side, a batch ahead of `link`: 1314 materials of
/// mp_skyjacked decoded and compressed one after another were most of its
/// materials step.
const AHEAD_BATCH: usize = 48;

impl Looks {
    pub fn new(max: u32, raw_reveal: bool) -> Self {
        Self {
            max,
            done: HashMap::new(),
            stats: Stats::default(),
            raw_reveal,
            reveals: HashMap::new(),
            wanted: Vec::new(),
            at: HashMap::new(),
            next: 0,
            made: HashMap::new(),
        }
    }

    /// The looks `link` will be asked for, in order (a material's first
    /// cover only); made a batch ahead on worker threads as `link` reaches
    /// them. One asked for out of this list is made where it is asked.
    pub fn ahead(&mut self, wanted: Vec<(usize, Cover)>) {
        self.at = wanted.iter().enumerate().map(|(k, &(i, _))| (i, k)).collect();
        self.wanted = wanted;
        self.next = 0;
    }

    fn make(&self, map: &T7Map, index: usize, cover: Cover) -> Result<Look, String> {
        let m = &map.materials[index];
        if self.raw_reveal {
            map.look_raw_reveal(m, self.max, SPECULAR_MAX.min(self.max), cover)
        } else {
            map.look(m, self.max, SPECULAR_MAX.min(self.max), cover)
        }
    }

    /// The next batch of `wanted`, made on worker threads.
    fn make_batch(&mut self, map: &T7Map) {
        let end = (self.next + AHEAD_BATCH).min(self.wanted.len());
        let jobs = &self.wanted[self.next..end];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8).min(jobs.len().max(1));
        let take = std::sync::atomic::AtomicUsize::new(0);
        let me = &*self;
        let made: Vec<(usize, (Cover, Result<Look, String>))> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut out = Vec::new();
                        loop {
                            let k = take.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&(index, cover)) = jobs.get(k) else { break };
                            out.push((index, (cover, me.make(map, index, cover))));
                        }
                        out
                    })
                })
                .collect();
            workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
        });
        self.made.extend(made);
        self.next = end;
    }

    /// Whether a material drawn as `draw` gets a look, and how it covers.
    pub fn cover(draw: &T6Draw) -> Option<Cover> {
        if draw.unlit || draw.effect || draw.water {
            return None;
        }
        match draw.blend {
            T6Blend::Opaque if !draw.alpha_test => Some(Cover::Opaque),
            T6Blend::Opaque | T6Blend::Blend => Some(Cover::Alpha),
            _ => None,
        }
    }

    /// Material `index`'s look, linked into `catalog`.
    pub fn link(&mut self, map: &T7Map, catalog: &mut MaterialCatalog, index: usize, cover: Cover) -> Linked {
        if let Some(l) = self.done.get(&index) {
            return *l;
        }
        while !self.made.contains_key(&index) && self.at.get(&index).is_some_and(|&k| k >= self.next) {
            self.make_batch(map);
        }
        let made = match self.made.remove(&index) {
            Some((c, made)) if c == cover => made,
            _ => self.make(map, index, cover),
        };
        let m = &map.materials[index];
        let linked = match made {
            Ok(look) => {
                if let Some(r) = look.reveal {
                    self.reveals.insert(index, r);
                }
                self.stats.materials += 1;
                for (k, u) in look.used.iter().enumerate() {
                    self.stats.used[k] += usize::from(*u);
                }
                let key = format!("t7look:{}#{}", m.name, self.max);
                let mut link = |levels: Option<ImageLevels>, semantic: u8, what: &str| {
                    levels.and_then(|l| link_levels(catalog, &mut self.stats.failures, format!("{key}#{what}"), l, semantic))
                };
                let c = link(look.colour, TS_COLOR_MAP, "colour");
                let n = link(look.normal, TS_NORMAL_MAP, "normal");
                let s = link(look.specular, TS_SPECULAR_MAP, "specular");
                self.stats.normal += usize::from(n.is_some());
                self.stats.specular += usize::from(s.is_some());
                [c, n, s]
            }
            Err(e) => {
                if self.stats.failures.len() < 32 {
                    self.stats.failures.push(format!("{}: {e}", m.name));
                }
                [None; 3]
            }
        };
        self.done.insert(index, linked);
        linked
    }

    /// The normal and specular bindings of a look.
    pub fn bindings(l: &Linked) -> Vec<MaterialTextureBinding> {
        [(NORMAL_MAP_HASH, TS_NORMAL_MAP, l[1]), (SPECULAR_MAP_HASH, TS_SPECULAR_MAP, l[2])]
            .into_iter()
            .filter_map(|(name_hash, semantic, image)| {
                image.map(|image| MaterialTextureBinding { name_hash, name_start: 0, name_end: 0, sampler_state: 0x13, semantic, image: Some(image) })
            })
            .collect()
    }

    pub fn report(&self) -> String {
        let s = &self.stats;
        format!(
            "t7 look: {} materials with Black Ops III's look ({} normal maps, {} specular maps; {} tinted, {} with metal, {} with gloss maps, {} with occlusion, {} with detail normals), {} emissive, {} failed{}",
            s.materials,
            s.normal,
            s.specular,
            s.used[0],
            s.used[1],
            s.used[2],
            s.used[3],
            s.used[4],
            s.emissive,
            s.failures.len(),
            s.failures.first().map_or(String::new(), |f| format!(" (first: {f})"))
        )
    }
}

/// An emissive material (`lit_emissive*`: screens, signs, lit panels; its
/// diffuse mostly Black Ops III's black placeholder): its emissive map,
/// and Black Ops II's unlit colour scale for its `hdrScale` (log2, at most
/// 1: Black Ops III's HDR units run brighter than Black Ops II's display
/// ones). The emissive programs add `emissiveMap * emissiveTint1 *
/// hdrScale` to the lit surface; drawn unlit here, the glow alone.
pub(super) fn emissive(map: &T7Map, index: usize) -> Option<(String, u8)> {
    let m = &map.materials[index];
    if !m.technique_set.split(['_', '/', '#']).any(|w| w == "emissive") {
        return None;
    }
    let name = m.texture("emissiveMap")?.to_owned();
    let hdr = m.constant::<1>("hdrScale").map_or(1.0, |v| v[0]).max(1.0);
    // (At most twice; a point light (`hdrScale` 8 and up: Nuk3town's light
    // dots, 16) once: its picture is white already, and the bloom glows
    // round it; brighter, the dots glared as blown-out discs.)
    let exp = if hdr >= 8.0 { 0 } else { hdr.log2().round().clamp(0.0, 1.0) as u8 };
    Some((name, exp))
}

/// Block-compressed levels into the catalog.
fn link_levels(catalog: &mut MaterialCatalog, failures: &mut Vec<String>, key: String, l: ImageLevels, semantic: u8) -> Option<usize> {
    let px = fastfile_iw6::image::ImagePixels { width: l.width, height: l.height, levels: l.levels, dxgi_format: l.format, data: l.data };
    let srgb = semantic != TS_NORMAL_MAP;
    match super::iw6::gpu_image(&px, srgb) {
        Ok(gpu) => Some(catalog.link_image(AuthoredImage {
            namespace: AssetNamespace::T6,
            name: AssetRef::Real(key),
            map_type: 0,
            semantic,
            category: 0,
            use_srgb_reads: srgb,
            width: px.width as u16,
            height: px.height as u16,
            depth: 1,
            level_count: px.levels as u8,
            format: 0,
            payload: Arc::new(Vec::new()),
            decoded: Some(Arc::new(gpu)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        })),
        Err(e) => {
            if failures.len() < 32 {
                failures.push(format!("{key}: {e}"));
            }
            None
        }
    }
}

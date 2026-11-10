//! bo2mp: Ghosts (and other foreign) world materials swapped for Black Ops
//! III's own where it has a good equivalent (`BO2MP_BO3_MAT_SUBST=0` turns
//! the pass off).
//!
//! No table is written by hand: at map load the pass reads the Black Ops III
//! install's shared zones through the existing T7 loaders
//! (`t7::bo3_pool_run`), classifies every material of both games into a
//! surface class (concrete, snow, dirt, rock, metal, wood, brick, asphalt,
//! grass, foliage) by the artists' words in its name and its colour map's
//! name, measures each colour map (mean colour and contrast of a small mip
//! level), and gives each foreign material the Black Ops III material of the
//! same class whose colour is nearest, when the pair clears a confidence
//! (`BO2MP_BO3_MAT_CONF`, 0.6) and a colour distance (`BO2MP_BO3_MAT_DIST`,
//! 0.12). The game's own stays where it is unique (signs, decals, flags,
//! logos, glass, water), where it cannot take another texture (layered,
//! unlit, cut-out, blended materials: a leaf card's UVs belong to its own
//! atlas) and where no Black Ops III texture fits its colour. With no
//! Black Ops III install nothing changes. The load report counts what was
//! swapped, kept and unmatched, with examples.

use asset_material::{MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP};
use fastfile_t7::texture::ImageLevels;

/// A surface class with a Black Ops III equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Class {
    Concrete,
    Snow,
    Dirt,
    Rock,
    Metal,
    Wood,
    Brick,
    Asphalt,
    Grass,
    Foliage,
}

/// What the pass decides for one material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Verdict {
    /// A class above the confidence threshold.
    Substitute(Class, f32),
    /// Unique to its game (signs, decals, flags, logos, glass, props with a
    /// name of their own): the game's own texture stays.
    Keep,
    /// No class or too little confidence.
    Unmatched,
}

pub(super) fn enabled() -> bool {
    super::bo3_look::game_on("BO2MP_BO3_MAT_SUBST", true)
}

/// The confidence a class needs (`BO2MP_BO3_MAT_CONF`, default 0.6).
pub(super) fn threshold() -> f32 {
    std::env::var("BO2MP_BO3_MAT_CONF").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0.6)
}

/// Words that mark a material as the game's own whatever else it says.
const KEEP: &[&str] = &[
    "decal", "sign", "poster", "flag", "logo", "graffiti", "label", "glass", "window", "sticker", "text", "banner", "map", "screen", "light", "lamp", "fx", "particle", "sky", "water", "cloth", "fabric", "paper",
    "billboard", "emblem", "stencil", "mural", "neon", "glow", "emissive", "monitor", "tv", "clock", "chalk", "scorch", "stain", "crack", "bullet", "blood", "tag", "ad", "ads", "insignia", "crest", "hud", "ui",
];

/// Words that mark the game's own wherever they sit in a compound
/// (`graffitiwall`, `decalconcrete`).
const KEEP_INSIDE: &[&str] = &["decal", "graffiti", "poster", "banner", "sticker", "billboard", "emblem", "stencil", "signage", "logo", "flag"];

const WORDS: &[(&str, Class, f32)] = &[
    ("concrete", Class::Concrete, 0.9),
    ("cement", Class::Concrete, 0.8),
    ("plaster", Class::Concrete, 0.6),
    ("stucco", Class::Concrete, 0.6),
    ("snow", Class::Snow, 0.9),
    ("ice", Class::Snow, 0.6),
    ("dirt", Class::Dirt, 0.9),
    ("mud", Class::Dirt, 0.8),
    ("sand", Class::Dirt, 0.7),
    ("gravel", Class::Dirt, 0.7),
    ("ground", Class::Dirt, 0.5),
    ("rock", Class::Rock, 0.9),
    ("stone", Class::Rock, 0.8),
    ("cliff", Class::Rock, 0.8),
    ("metal", Class::Metal, 0.9),
    ("steel", Class::Metal, 0.9),
    ("iron", Class::Metal, 0.8),
    ("container", Class::Metal, 0.7),
    ("corrugated", Class::Metal, 0.8),
    ("rust", Class::Metal, 0.7),
    ("wood", Class::Wood, 0.9),
    ("plank", Class::Wood, 0.9),
    ("board", Class::Wood, 0.5),
    ("brick", Class::Brick, 0.9),
    ("asphalt", Class::Asphalt, 0.9),
    ("road", Class::Asphalt, 0.7),
    ("tarmac", Class::Asphalt, 0.9),
    ("grass", Class::Grass, 0.9),
    ("lawn", Class::Grass, 0.8),
    ("leaf", Class::Foliage, 0.8),
    ("leaves", Class::Foliage, 0.8),
    ("foliage", Class::Foliage, 0.9),
    ("branch", Class::Foliage, 0.7),
    ("bush", Class::Foliage, 0.8),
    ("shrub", Class::Foliage, 0.8),
    ("fern", Class::Foliage, 0.7),
    ("palm", Class::Foliage, 0.7),
    ("tree", Class::Foliage, 0.6),
    ("bark", Class::Wood, 0.6),
    ("log", Class::Wood, 0.6),
    ("pavement", Class::Asphalt, 0.8),
    ("sidewalk", Class::Concrete, 0.8),
    ("curb", Class::Concrete, 0.7),
    ("cinder", Class::Concrete, 0.7),
    ("cmu", Class::Concrete, 0.7),
    ("masonry", Class::Brick, 0.7),
    ("tin", Class::Metal, 0.5),
    ("alum", Class::Metal, 0.8),
    ("aluminum", Class::Metal, 0.8),
    ("pipe", Class::Metal, 0.6),
    ("girder", Class::Metal, 0.8),
    ("sheet", Class::Metal, 0.4),
    ("pebble", Class::Dirt, 0.7),
    ("soil", Class::Dirt, 0.9),
    ("earth", Class::Dirt, 0.7),
    ("clay", Class::Dirt, 0.7),
    ("boulder", Class::Rock, 0.9),
    ("granite", Class::Rock, 0.9),
    ("marble", Class::Rock, 0.8),
    ("slate", Class::Rock, 0.8),
    ("limestone", Class::Rock, 0.9),
    ("sandstone", Class::Rock, 0.8),
    ("tile", Class::Concrete, 0.4),
    ("parquet", Class::Wood, 0.8),
    ("lumber", Class::Wood, 0.9),
    ("pallet", Class::Wood, 0.8),
    ("crate", Class::Wood, 0.5),
    ("frost", Class::Snow, 0.7),
    ("drift", Class::Snow, 0.7),
];

/// Classifies a material by its name (`mp_x_concrete_wall_02`, `ch_...`):
/// the underscore-separated words; the best class's confidence is its word's,
/// dropped by a tenth for every other class named (ambiguity), and a game's
/// own word (`KEEP`) wins over any class.
pub(super) fn classify(name: &str, threshold: f32) -> Verdict {
    let lower = name.trim_start_matches(',').to_ascii_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| KEEP.contains(w) || KEEP_INSIDE.iter().any(|k| w.contains(k))) {
        return Verdict::Keep;
    }
    let mut best: Option<(Class, f32)> = None;
    let mut classes = std::collections::HashSet::new();
    for w in &words {
        for &(key, class, conf) in WORDS {
            // (A word or its plural / numbered form: `bricks`, `rock2`.)
            if w.starts_with(key) && w[key.len()..].chars().all(|c| c.is_ascii_digit() || c == 's') {
                classes.insert(class);
                if best.is_none_or(|b| conf > b.1) {
                    best = Some((class, conf));
                }
            }
        }
    }
    match best {
        Some((class, conf)) => {
            let conf = conf - 0.1 * (classes.len() - 1) as f32;
            if conf >= threshold {
                Verdict::Substitute(class, conf)
            } else {
                Verdict::Unmatched
            }
        }
        None => Verdict::Unmatched,
    }
}

/// The class of a Black Ops III surface type (the clip map's material
/// flags, bits 20..25; the list is Black Ops II's, Black Ops III's extra
/// metals 32..35 are metal): what its collision says the surface is. Types
/// with no class here (glass, flesh, cloth, paper, carpet, water...) give
/// `None`.
pub(super) fn surface_class(ty: u32) -> Option<Class> {
    Some(match ty {
        1 | 21 => Class::Wood,
        2 => Class::Brick,
        5 | 16 => Class::Concrete,
        6 | 11 | 14 | 18 => Class::Dirt,
        8 => Class::Foliage,
        10 | 30 => Class::Grass,
        12 | 19 => Class::Snow,
        13 | 28 | 32..=35 => Class::Metal,
        17 => Class::Rock,
        22 => Class::Asphalt,
        _ => return None,
    })
}

/// The classes whose textures tile the same way on any surface: ground,
/// snow, rock, asphalt and bare concrete. Metal (containers, vehicles,
/// panels), wood (crates, boards), brick (painted or patterned walls) and
/// foliage (cards with their own atlas) carry identifiable detail laid out
/// for their own mesh, and another game's picture on them loses the
/// original's look: they keep the game's own.
pub(super) fn swappable(class: Class) -> bool {
    matches!(class, Class::Concrete | Class::Snow | Class::Dirt | Class::Rock | Class::Asphalt)
}

/// Words of a Black Ops III material that tie it to one prop, one mesh's UV
/// layout or one painted pattern (a tile grid, a trim, an edge strip): never
/// a pool candidate.
const POOL_DENY: &[&str] = &[
    "mtl", "p7", "zm", "sin", "vista", "building", "buildings", "crate", "ammo", "conduit", "pipe", "pipes", "trim", "edge", "edges", "tile", "tiles", "panel", "panels", "beam", "door", "doors", "window", "windows", "vent", "grate",
    "stripe", "stripes", "sign", "signs", "decal", "decals", "grime", "stain", "stains", "crack", "cracks", "graffiti", "poster", "wire", "fence", "rail", "railing", "stairs", "step", "steps", "column", "pillar", "arch", "frame", "weapon",
    "wpn", "veh", "vehicle", "car", "truck", "barrel", "container", "shelf", "locker", "cabinet", "pallet", "board", "plank", "planks", "sheet", "corrugated", "rust", "rusty", "brick", "bricks", "foliage", "leaf", "leaves", "grass",
    "bush", "tree", "trunk", "bark", "cloth", "fabric", "carpet", "paper", "glass", "water", "logo", "text", "number", "numbers", "arrow", "hazard", "warning", "blend", "trans", "transition", "overlay", "detail", "lines", "line",
];

/// Whether a Black Ops III material is generic tileable world texture: a
/// world material (`wc/`, not a model's `mc/`), a class word in its own name
/// or its colour map's, and no word of `POOL_DENY`.
pub(super) fn generic_world(name: &str, colour: &str) -> bool {
    let lower = name.trim_start_matches(',').to_ascii_lowercase();
    if !lower.starts_with("wc/") {
        return false;
    }
    let words = |s: &str| -> Vec<String> { s.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect() };
    let (a, b) = (words(&lower[3..]), words(colour));
    if a.iter().chain(&b).any(|w| POOL_DENY.contains(&w.as_str())) {
        return false;
    }
    // (Dimensions of a pattern: `4x4`, `2x2`.)
    if a.iter().chain(&b).any(|w| w.len() >= 3 && w.contains('x') && w.chars().all(|c| c.is_ascii_digit() || c == 'x')) {
        return false;
    }
    a.iter().chain(&b).any(|w| ["concrete", "cement", "snow", "ice", "frost", "dirt", "mud", "sand", "gravel", "ground", "soil", "earth", "clay", "rock", "stone", "cliff", "boulder", "granite", "asphalt", "tarmac", "road", "pavement", "terrain"].iter().any(|k| w.starts_with(k)))
}

/// The largest difference between two colour maps' chromaticities (each
/// channel's share of the sum of the three, linear): a pick whose hue is off
/// (a mauve ground for a grey-brown one) reads as a patch of another surface
/// whatever its brightness; the tint only darkens, it cannot turn a hue.
pub(super) fn chroma_gap(a: &TexStats, b: &TexStats) -> f32 {
    let share = |s: &TexStats| {
        let t = (s.lin[0] + s.lin[1] + s.lin[2]).max(1e-4);
        s.lin.map(|c| c / t)
    };
    let (x, y) = (share(a), share(b));
    (0..3).map(|k| (x[k] - y[k]).abs()).fold(0.0, f32::max)
}

/// The most two chromaticities may differ (`chroma_gap`).
pub(super) const MAX_CHROMA_GAP: f32 = 0.05;

/// The most a pair's contrast may differ (the Ghosts material's painted
/// detail against the pick's): past it the textures are different kinds of
/// surface even where the mean colours agree.
pub(super) const MAX_SPREAD_GAP: f32 = 0.08;

/// The class and confidence of a Black Ops III material from its surface
/// type and its names: the surface type (0.85) and a name that says the
/// same (0.95) beat a name alone; they disagree, the surface type wins.
pub(super) fn class_of_bo3(surface: Option<u32>, names: &[&str], threshold: f32) -> Option<(Class, f32)> {
    let by_name = match classify_all(names, threshold) {
        Verdict::Substitute(c, conf) => Some((c, conf)),
        Verdict::Keep => return None,
        Verdict::Unmatched => None,
    };
    match (surface.and_then(surface_class), by_name) {
        (Some(s), Some((n, c))) if s == n => Some((s, (c + 0.1).min(0.95))),
        (Some(s), _) => Some((s, 0.85)),
        (None, n) => n,
    }
}

/// The class of a foreign material: its names must say it (a surface type
/// alone names a class for every painted wall, container and prop the
/// collision calls concrete or metal), only a swappable class counts, and a
/// surface type that says something else vetoes it.
pub(super) fn ghost_class(surface: Option<u32>, names: &[&str], threshold: f32) -> Option<(Class, f32)> {
    let Verdict::Substitute(class, conf) = classify_all(names, threshold) else { return None };
    if !swappable(class) {
        return None;
    }
    match surface.and_then(surface_class) {
        Some(s) if s != class => None,
        _ => Some((class, conf)),
    }
}

/// The best verdict over several names of one material (its own and its
/// colour map's): a game's own word in any wins; else the surest class.
pub(super) fn classify_all(names: &[&str], threshold: f32) -> Verdict {
    let mut best = Verdict::Unmatched;
    for n in names {
        match classify(n, threshold) {
            Verdict::Keep => return Verdict::Keep,
            Verdict::Substitute(c, conf) => {
                if !matches!(best, Verdict::Substitute(_, b) if b >= conf) {
                    best = Verdict::Substitute(c, conf);
                }
            }
            Verdict::Unmatched => {}
        }
    }
    best
}

/// What a colour map looks like, small: its mean colour (linear) and the
/// spread of its brightness (sRGB, 0..1: a flat paint is near 0, a rock or
/// foliage 0.1..0.25).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TexStats {
    pub lin: [f32; 3],
    pub spread: f32,
}

fn to_linear(b: u8) -> f32 {
    let c = f32::from(b) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Stats of RGBA8 sRGB texels (`None` when there are none).
pub(super) fn stats_of_rgba(px: &[u8]) -> Option<TexStats> {
    let n = px.len() / 4;
    if n == 0 {
        return None;
    }
    let (mut lin, mut sum, mut sum2) = ([0.0f64; 3], 0.0f64, 0.0f64);
    for t in px.chunks_exact(4) {
        for k in 0..3 {
            lin[k] += f64::from(to_linear(t[k]));
        }
        let l = (0.2126 * f64::from(t[0]) + 0.7152 * f64::from(t[1]) + 0.0722 * f64::from(t[2])) / 255.0;
        sum += l;
        sum2 += l * l;
    }
    let nf = n as f64;
    let mean = sum / nf;
    let var = (sum2 / nf - mean * mean).max(0.0);
    Some(TexStats { lin: lin.map(|v| (v / nf) as f32), spread: var.sqrt() as f32 })
}

/// Stats of a picture's mip chain (`data`: biggest level first) read at the
/// first level that is 32 texels or less on a side. `None` for a format the
/// reader does not decode (BC6H, cube maps).
pub(super) fn stats_of_chain(format: u32, width: u32, height: u32, levels: u32, data: &[u8]) -> Option<TexStats> {
    let levels = levels.max(1);
    let mut mip = 0;
    while mip + 1 < levels && (width >> mip).max(height >> mip) > 32 {
        mip += 1;
    }
    let start: usize = (0..mip).map(|m| fastfile_t7::texture::level_size(format, (width >> m).max(1), (height >> m).max(1))).sum::<Option<usize>>()?;
    let (w, h) = ((width >> mip).max(1), (height >> mip).max(1));
    let size = fastfile_t7::texture::level_size(format, w, h)?;
    let tail = ImageLevels { format, width: w, height: h, levels: 1, data: data.get(start..start + size)?.to_vec(), part: None };
    let (_, _, px) = fastfile_t7::texture::level_rgba(&tail, 0)?;
    stats_of_rgba(&px)
}

/// Whether a colour map is what its class would look like: a name that says
/// snow on a dark brown picture, or foliage on a blue one, is a name of
/// something else (a snow-covered sign's atlas, a leaf-pattern wallpaper).
pub(super) fn plausible(class: Class, s: &TexStats) -> bool {
    let lum = 0.2126 * s.lin[0] + 0.7152 * s.lin[1] + 0.0722 * s.lin[2];
    match class {
        Class::Snow => lum >= 0.15 && s.lin[2] >= s.lin[0] * 0.8,
        Class::Asphalt => lum <= 0.3,
        Class::Foliage | Class::Grass => s.lin[1] >= s.lin[2] * 0.9 && s.lin[1] >= s.lin[0] * 0.7,
        Class::Brick => s.lin[0] >= s.lin[2] * 0.85,
        _ => true,
    }
}

/// The darkening-only vertex colour that takes a Black Ops III colour map
/// toward the foreign material's own colour: a vertex colour only scales the
/// texel (`base = (texel * colour)^2`), so a brighter pick is toned down and a
/// darker one stays. Per channel, kept above `TINT_FLOOR`.
pub(super) const TINT_FLOOR: f32 = 0.55;
pub(super) fn tint_toward(ghost: &TexStats, pick: &TexStats) -> [f32; 3] {
    std::array::from_fn(|k| (ghost.lin[k].max(1e-3) / pick.lin[k].max(1e-3)).sqrt().clamp(TINT_FLOOR, 1.0))
}

/// The pick's stats as the tint leaves them (a gamma-space colour factor
/// squares in linear light).
pub(super) fn tinted(pick: &TexStats, tint: [f32; 3]) -> TexStats {
    TexStats { lin: std::array::from_fn(|k| pick.lin[k] * tint[k] * tint[k]), spread: pick.spread }
}

/// How far apart two colour maps look: the root-mean-square difference of
/// their mean colours in gamma space (0 same, about 1 black and white) plus
/// half their difference in contrast.
pub(super) fn distance(a: &TexStats, b: &TexStats) -> f32 {
    let g = |c: f32| c.max(0.0).sqrt();
    let sq: f32 = (0..3).map(|k| (g(a.lin[k]) - g(b.lin[k])).powi(2)).sum();
    (sq / 3.0).sqrt() + 0.5 * (a.spread - b.spread).abs()
}

/// A Black Ops III colour distance past which a pair does not fit
/// (`BO2MP_BO3_MAT_DIST`).
pub(super) fn max_distance() -> f32 {
    std::env::var("BO2MP_BO3_MAT_DIST").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0.12)
}

/// A Black Ops III material the pool offers.
#[derive(Debug, Clone)]
pub(super) struct PoolItem {
    pub name: String,
    pub class: Class,
    pub name_conf: f32,
    pub stats: TexStats,
}

/// The Black Ops III materials, as `t7::bo3_pool_run` gives them: the
/// candidates, and the textures of one linked into the catalog.
pub(super) trait Pool {
    fn items(&self) -> &[PoolItem];
    /// The candidate's colour, normal and specular bindings (linked into
    /// the catalog), `None` when its pictures cannot be made.
    fn link(&mut self, i: usize) -> Option<Vec<MaterialTextureBinding>>;
}

/// A foreign material the pass looks at.
#[derive(Debug, Clone)]
pub(super) struct Ghost {
    pub name: String,
    pub class: Class,
    pub name_conf: f32,
    pub stats: TexStats,
}

/// Why a material is not swapped.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Why {
    /// A class, but no Black Ops III material of it (or none that fits).
    NoFit(Option<f32>),
    /// The best pair's pictures could not be made.
    LinkFailed,
}

/// The decision for one material.
#[derive(Debug, Clone)]
pub(super) enum Decision {
    Swap { item: usize, confidence: f32, dist: f32, tint: [f32; 3], textures: Vec<MaterialTextureBinding> },
    Keep(Why),
}

/// Gives each ghost the pool's nearest material of its class that clears
/// the confidence and the distance, linking each chosen one once.
pub(super) fn decide(ghosts: &[Ghost], pool: &mut dyn Pool, threshold: f32, max_dist: f32) -> Vec<Decision> {
    let mut linked: std::collections::HashMap<usize, Option<Vec<MaterialTextureBinding>>> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(ghosts.len());
    for g in ghosts {
        let best = pool
            .items()
            .iter()
            .enumerate()
            .filter(|(_, it)| it.class == g.class && swappable(it.class) && (it.stats.spread - g.stats.spread).abs() <= MAX_SPREAD_GAP && chroma_gap(&g.stats, &it.stats) <= MAX_CHROMA_GAP)
            .map(|(i, it)| (i, it.name_conf, distance(&g.stats, &tinted(&it.stats, tint_toward(&g.stats, &it.stats)))))
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let Some((i, item_conf, dist)) = best else {
            out.push(Decision::Keep(Why::NoFit(None)));
            continue;
        };
        // (The confidence: how sure both names are of the class, times how
        // well the colours fit; the fit is full at 0 and nothing at twice
        // the distance allowed.)
        let fit = (1.0 - dist / (2.0 * max_dist)).clamp(0.0, 1.0);
        let confidence = (g.name_conf * item_conf).sqrt() * fit;
        if dist > max_dist || confidence < threshold {
            out.push(Decision::Keep(Why::NoFit(Some(dist))));
            continue;
        }
        let textures = linked.entry(i).or_insert_with(|| pool.link(i)).clone();
        out.push(match textures {
            Some(textures) if textures.iter().any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some()) => {
                let tint = tint_toward(&g.stats, &pool.items()[i].stats);
                Decision::Swap { item: i, confidence, dist, tint, textures }
            }
            _ => Decision::Keep(Why::LinkFailed),
        });
    }
    out
}

/// The load report's counts: swapped by class, kept by reason, with
/// examples.
#[derive(Debug, Default)]
pub(super) struct Tally {
    pub total: usize,
    pub swapped: std::collections::BTreeMap<Class, usize>,
    pub own: usize,
    pub ineligible: std::collections::BTreeMap<&'static str, usize>,
    pub unmatched: usize,
    pub no_fit: usize,
    pub link_failed: usize,
    pub examples: Vec<String>,
    pub misses: Vec<String>,
    /// Per class: (swapped, kept for want of a fit, link failed).
    pub classes: std::collections::BTreeMap<Class, [usize; 3]>,
    pub own_names: Vec<String>,
    pub unmatched_names: Vec<String>,
    pub ineligible_names: Vec<String>,
}

impl Tally {
    pub(super) fn line(&self, tag: &str, source: &str, pool: usize, thr: f32, max_dist: f32) -> Vec<String> {
        let swapped: usize = self.swapped.values().sum();
        let mut out = vec![format!(
            "{tag} material substitution: {} materials: {swapped} swapped for Black Ops III's {:?} (pool {pool} materials from {source}), {} kept (game's own: signs, decals, glass, water..), {} kept (cannot take another texture {:?}), {} unmatched (no surface class), {} kept (class but no Black Ops III colour within {max_dist:.2}), {} link failed (confidence {thr:.2})",
            self.total,
            self.swapped,
            self.own,
            self.ineligible.values().sum::<usize>(),
            self.ineligible,
            self.unmatched,
            self.no_fit,
            self.link_failed
        )];
        if !self.classes.is_empty() {
            let per: Vec<String> = self.classes.iter().map(|(c, [s, k, l])| format!("{c:?} {s} swapped / {k} kept / {l} link failed")).collect();
            out.push(format!("{tag} material substitution by class: {}", per.join("; ")));
        }
        if !self.own_names.is_empty() {
            out.push(format!("{tag} material substitution: the game's own stays, e.g. {}", self.own_names.join("; ")));
        }
        if !self.ineligible_names.is_empty() {
            out.push(format!("{tag} material substitution: cannot take another texture, e.g. {}", self.ineligible_names.join("; ")));
        }
        if !self.unmatched_names.is_empty() {
            out.push(format!("{tag} material substitution: no surface class, e.g. {}", self.unmatched_names.join("; ")));
        }
        if !self.examples.is_empty() {
            out.push(format!("{tag} material substitution: swapped, e.g. {}", self.examples.join("; ")));
        }
        if !self.misses.is_empty() {
            out.push(format!("{tag} material substitution: kept for want of a fit, e.g. {}", self.misses.join("; ")));
        }
        out
    }
}

/// One material the pass was asked about.
pub(super) enum Subject {
    Own,
    Ineligible(&'static str),
    Unmatched,
    Candidate(Ghost),
}

/// Folds decisions into a tally (`subjects` in the order asked; the decisions
/// are for its candidates, in order).
pub(super) fn tally(subjects: &[(String, Subject)], decisions: &[Decision], pool: &[PoolItem]) -> Tally {
    let mut t = Tally { total: subjects.len(), ..Default::default() };
    let mut d = decisions.iter();
    for (name, s) in subjects {
        match s {
            Subject::Own => {
                t.own += 1;
                if t.own_names.len() < 8 {
                    t.own_names.push(name.clone());
                }
            }
            Subject::Ineligible(r) => {
                *t.ineligible.entry(r).or_default() += 1;
                if t.ineligible_names.len() < 6 {
                    t.ineligible_names.push(format!("{name} ({r})"));
                }
            }
            Subject::Unmatched => {
                t.unmatched += 1;
                if t.unmatched_names.len() < 8 {
                    t.unmatched_names.push(name.clone());
                }
            }
            Subject::Candidate(g) => match d.next() {
                Some(Decision::Swap { item, confidence, dist, tint, .. }) => {
                    *t.swapped.entry(g.class).or_default() += 1;
                    t.classes.entry(g.class).or_default()[0] += 1;
                    if t.examples.len() < 6 {
                        t.examples.push(format!("{name} -> {} ({:?}, confidence {confidence:.2}, colour distance {dist:.2}, tint {:.2}/{:.2}/{:.2})", pool[*item].name, g.class, tint[0], tint[1], tint[2]));
                    }
                }
                Some(Decision::Keep(Why::NoFit(dist))) => {
                    t.no_fit += 1;
                    t.classes.entry(g.class).or_default()[1] += 1;
                    if t.misses.len() < 4 {
                        t.misses.push(format!("{name} ({:?}, nearest {})", g.class, dist.map_or("none in the pool".to_owned(), |d| format!("{d:.2}"))));
                    }
                }
                Some(Decision::Keep(Why::LinkFailed)) => {
                    t.link_failed += 1;
                    t.classes.entry(g.class).or_default()[2] += 1;
                }
                None => t.unmatched += 1,
            },
        }
    }
    t
}

/// A bevy picture's DXGI format (the ones the readers decode), else `None`.
fn dxgi_of(f: bevy::render::render_resource::TextureFormat) -> Option<u32> {
    use bevy::render::render_resource::TextureFormat as F;
    Some(match f {
        F::Bc1RgbaUnorm | F::Bc1RgbaUnormSrgb => 71,
        F::Bc2RgbaUnorm | F::Bc2RgbaUnormSrgb => 74,
        F::Bc3RgbaUnorm | F::Bc3RgbaUnormSrgb => 77,
        F::Bc7RgbaUnorm | F::Bc7RgbaUnormSrgb => 98,
        F::Rgba8Unorm | F::Rgba8UnormSrgb => 28,
        F::Bgra8Unorm | F::Bgra8UnormSrgb => 87,
        _ => return None,
    })
}

/// A catalog picture's stats (its decoded GPU form).
pub(super) fn image_stats(catalog: &MaterialCatalog, image: usize) -> Option<TexStats> {
    let im = catalog.images.get(image)?.decoded.as_ref()?;
    let data = im.data.as_ref()?;
    let size = im.texture_descriptor.size;
    stats_of_chain(dxgi_of(im.texture_descriptor.format)?, size.width, size.height, im.texture_descriptor.mip_level_count, data)
}

/// Why a catalog material cannot take another game's texture, if so.
fn ineligible(m: &asset_material::AuthoredMaterial) -> Option<&'static str> {
    use asset_core::T6Blend;
    let d = m.t6_draw.as_ref()?;
    Some(if d.unlit {
        "unlit"
    } else if d.layered || d.tile || d.tile_blend {
        "layered"
    } else if d.effect || d.cloud {
        "effect"
    } else if d.water || d.flow {
        "water"
    } else if d.alpha_test {
        "cut-out"
    } else if d.blend != T6Blend::Opaque {
        "blended"
    } else if d.projected_decal || d.polygon_offset > 0 {
        "decal"
    } else {
        return None;
    })
}

/// Puts a pick's textures on a catalog material, keeping the binding hashes
/// the material had for each slot (so its draw finds them).
pub(super) fn apply_textures(m: &mut asset_material::AuthoredMaterial, textures: &[MaterialTextureBinding]) {
    let old = std::mem::take(&mut m.textures);
    m.textures = textures
        .iter()
        .map(|t| {
            let mut t = t.clone();
            if let Some(o) = old.iter().find(|o| o.semantic == t.semantic) {
                t.name_hash = o.name_hash;
            }
            t
        })
        .collect();
}

/// The pass over a catalog's materials: `mats` are (the game's name for the
/// material, its catalog slot). With the Black Ops III install's pool the
/// swappable ones take its textures (colour, normal, specular); the report
/// says what happened. `tag` is the game's (`iw6`).
pub(super) fn run(
    tag: &str,
    catalog: &mut MaterialCatalog,
    mats: &[(String, usize)],
    surfaces: &std::collections::HashMap<String, u32>,
    max: u32,
    report: &mut Vec<String>,
) -> (Vec<(usize, [f32; 3])>, String) {
    if !enabled() {
        report.push(format!("{tag} material substitution: off (BO2MP_BO3_MAT_SUBST=0)"));
        return (Vec::new(), "swaps off".to_owned());
    }
    let (thr, max_dist) = (threshold(), max_distance());
    let mut subjects: Vec<(String, Subject)> = Vec::with_capacity(mats.len());
    let mut slots: Vec<usize> = Vec::new();
    let mut ghosts: Vec<Ghost> = Vec::new();
    let mut with_surface = 0usize;
    for (name, slot) in mats {
        let Some(m) = catalog.materials.get(*slot) else { continue };
        let colour = m.textures.iter().find(|t| t.semantic == TS_COLOR_MAP).and_then(|t| t.image);
        let image_name = colour.and_then(|i| catalog.images.get(i)).map(|im| im.name.as_str().to_owned()).unwrap_or_default();
        let surface = if surface_evidence() { surfaces.get(&surface_key(name)).copied() } else { None };
        if surface.is_some() {
            with_surface += 1;
        }
        let evidence = match classify_all(&[name, &image_name], 0.0) {
            Verdict::Keep => None,
            _ => Some(ghost_class(surface, &[name, &image_name], thr)),
        };
        let subject = match evidence {
            None => Subject::Own,
            Some(None) => Subject::Unmatched,
            Some(Some((class, name_conf))) => match (ineligible(m), colour.and_then(|i| image_stats(catalog, i))) {
                (Some(why), _) => Subject::Ineligible(why),
                (None, None) => Subject::Ineligible("no readable colour map"),
                (None, Some(stats)) if !plausible(class, &stats) => Subject::Ineligible("colours unlike their class"),
                (None, Some(stats)) => {
                    let g = Ghost { name: name.clone(), class, name_conf, stats };
                    ghosts.push(g.clone());
                    slots.push(*slot);
                    Subject::Candidate(g)
                }
            },
        };
        subjects.push((name.clone(), subject));
    }
    report.push(format!(
        "{tag} material substitution: surface type known (from the collision's materials) for {with_surface} of {} materials (BO2MP_BO3_MAT_SURFACE=0 reads names alone)",
        subjects.len()
    ));
    if ghosts.is_empty() {
        report.extend(tally(&subjects, &[], &[]).line(tag, "not read: nothing to swap", 0, thr, max_dist));
        return (Vec::new(), format!("0 of {} materials swapped (none generic)", subjects.len()));
    }
    let taken = std::mem::take(catalog);
    let (back, result) = super::t7::bo3_pool_run(taken, max, |pool| {
        let decisions = decide(&ghosts, pool, thr, max_dist);
        (decisions, pool.items().to_vec())
    });
    *catalog = back;
    match result {
        Ok(((decisions, items), source)) => {
            let mut tints = Vec::new();
            for (d, slot) in decisions.iter().zip(&slots) {
                if let Decision::Swap { textures, tint, .. } = d
                    && let Some(m) = catalog.materials.get_mut(*slot)
                {
                    apply_textures(m, textures);
                    tints.push((*slot, *tint));
                }
            }
            let t = tally(&subjects, &decisions, &items);
            report.extend(t.line(tag, &source, items.len(), thr, max_dist));
            let n: usize = t.swapped.values().sum();
            (tints, format!("{n} of {} materials swapped (pool {} generic world materials, classes {:?})", t.total, items.len(), t.swapped))
        }
        Err(why) => {
            let mut by_class = std::collections::BTreeMap::<Class, usize>::new();
            for g in &ghosts {
                *by_class.entry(g.class).or_default() += 1;
            }
            report.push(format!(
                "{tag} material substitution: nothing swapped, the Black Ops III pool is unavailable ({why}); the game's own textures stay: {} materials, {} with a surface class {by_class:?}",
                subjects.len(),
                ghosts.len()
            ));
            (Vec::new(), format!("0 of {} materials swapped (no Black Ops III pool)", subjects.len()))
        }
    }
}

/// Whether the collision's surface types count as evidence of a class
/// (`BO2MP_BO3_MAT_SURFACE=0` reads names alone).
pub(super) fn surface_evidence() -> bool {
    super::bo3_look::game_on("BO2MP_BO3_MAT_SURFACE", true)
}

/// A material's key in the collision's table: the lower-case name past any
/// path and leading comma.
pub(super) fn surface_key(name: &str) -> String {
    name.trim_start_matches(',').rsplit('/').next().unwrap_or(name).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_name() {
        assert_eq!(classify("mp_prison_concrete_wall_01", 0.6), Verdict::Substitute(Class::Concrete, 0.9));
        assert!(matches!(classify("ch_snow_ground_02", 0.6), Verdict::Substitute(Class::Snow, c) if (c - 0.8).abs() < 1e-4));
        assert!(matches!(classify("metal_container_blue", 0.6), Verdict::Substitute(Class::Metal, _)));
    }

    #[test]
    fn the_games_own_stay() {
        assert_eq!(classify("mp_x_sign_concrete", 0.6), Verdict::Keep);
        assert_eq!(classify("decal_snow_01", 0.6), Verdict::Keep);
        assert_eq!(classify("mp_x_logo", 0.6), Verdict::Keep);
    }

    #[test]
    fn weak_or_unknown_is_unmatched() {
        assert_eq!(classify("mp_x_thing_07", 0.6), Verdict::Unmatched);
        assert_eq!(classify("ground_a", 0.6), Verdict::Unmatched);
    }

    fn stats(l: f32, spread: f32) -> TexStats {
        TexStats { lin: [l; 3], spread }
    }

    struct Fake(Vec<PoolItem>, usize);
    impl Pool for Fake {
        fn items(&self) -> &[PoolItem] {
            &self.0
        }
        fn link(&mut self, _: usize) -> Option<Vec<MaterialTextureBinding>> {
            self.1 += 1;
            Some(vec![MaterialTextureBinding { name_hash: 1, name_start: 0, name_end: 0, sampler_state: 0, semantic: TS_COLOR_MAP, image: Some(7) }])
        }
    }

    fn item(name: &str, class: Class, l: f32) -> PoolItem {
        PoolItem { name: name.into(), class, name_conf: 0.9, stats: stats(l, 0.1) }
    }

    fn ghost(name: &str, class: Class, l: f32) -> Ghost {
        Ghost { name: name.into(), class, name_conf: 0.9, stats: stats(l, 0.1) }
    }

    #[test]
    fn the_nearest_colour_of_the_class_is_chosen_and_linked_once() {
        let mut pool = Fake(vec![item("dark_concrete", Class::Concrete, 0.05), item("pale_concrete", Class::Concrete, 0.3), item("snow", Class::Snow, 0.8)], 0);
        let g = [ghost("a", Class::Concrete, 0.28), ghost("b", Class::Concrete, 0.32), ghost("c", Class::Snow, 0.75)];
        let d = decide(&g, &mut pool, 0.6, 0.2);
        assert!(matches!(&d[0], Decision::Swap { item: 1, .. }), "{:?}", d[0]);
        assert!(matches!(&d[1], Decision::Swap { item: 1, .. }));
        assert!(matches!(&d[2], Decision::Swap { item: 2, .. }));
        assert_eq!(pool.1, 2, "the pale concrete linked once for two materials");
    }

    #[test]
    fn a_colour_that_does_not_fit_stays_the_games_own() {
        // A near-black ghost rock against a pale-only pool.
        let mut pool = Fake(vec![item("pale_rock", Class::Rock, 0.5)], 0);
        let d = decide(&[ghost("dark_rock", Class::Rock, 0.01), ghost("wood", Class::Wood, 0.2)], &mut pool, 0.6, 0.2);
        assert!(matches!(d[0], Decision::Keep(Why::NoFit(Some(_)))), "{:?}", d[0]);
        assert!(matches!(d[1], Decision::Keep(Why::NoFit(None))), "no wood in the pool");
        assert_eq!(pool.1, 0, "nothing was linked for a miss");
    }

    #[test]
    fn a_sure_name_and_a_close_colour_pass_a_weak_pair_does_not() {
        let mut pool = Fake(vec![item("c", Class::Concrete, 0.3)], 0);
        let mut weak = ghost("w", Class::Concrete, 0.3);
        weak.name_conf = 0.3;
        let d = decide(&[ghost("s", Class::Concrete, 0.3), weak], &mut pool, 0.6, 0.2);
        assert!(matches!(d[0], Decision::Swap { .. }));
        assert!(matches!(d[1], Decision::Keep(Why::NoFit(_))), "{:?}", d[1]);
    }

    #[test]
    fn stats_read_a_flat_and_a_noisy_picture() {
        let flat: Vec<u8> = [128u8, 128, 128, 255].repeat(16);
        let s = stats_of_rgba(&flat).unwrap();
        assert!(s.spread < 1e-3 && (s.lin[0] - 0.2158).abs() < 0.01, "{s:?}");
        let noisy: Vec<u8> = (0..16).flat_map(|i| if i % 2 == 0 { [20u8, 20, 20, 255] } else { [220, 220, 220, 255] }).collect();
        assert!(stats_of_rgba(&noisy).unwrap().spread > 0.3);
    }

    #[test]
    fn a_chain_is_read_at_a_small_level() {
        // 64x64 RGBA8 (format 28), three levels: level 1 (32x32) is read.
        let mut data = vec![0u8; 64 * 64 * 4];
        data.extend([200u8, 100, 50, 255].repeat(32 * 32));
        data.extend([0u8, 0, 0, 255].repeat(16 * 16));
        let s = stats_of_chain(28, 64, 64, 3, &data).unwrap();
        assert!(s.lin[0] > s.lin[2] && s.lin[0] > 0.4, "{s:?}");
    }

    #[test]
    fn a_surface_type_gives_a_class_and_agrees_with_a_name() {
        assert_eq!(surface_class(5), Some(Class::Concrete));
        assert_eq!(surface_class(34), Some(Class::Metal));
        assert_eq!(surface_class(9), None, "glass is not swapped");
        // Concrete by type, nothing in the name.
        assert_eq!(class_of_bo3(Some(5), &["mtl_p7_wall_a"], 0.5), Some((Class::Concrete, 0.85)));
        // Both agree: surer. Disagree: the type wins. A game's own word: none.
        assert!(matches!(class_of_bo3(Some(17), &["mtl_rock_wall"], 0.5), Some((Class::Rock, c)) if c > 0.9));
        assert_eq!(class_of_bo3(Some(13), &["mtl_wood_crate"], 0.5).map(|c| c.0), Some(Class::Metal));
        assert_eq!(class_of_bo3(Some(5), &["mtl_concrete_decal"], 0.5), None);
        assert_eq!(class_of_bo3(None, &["mtl_p7_thing"], 0.5), None);
    }

    #[test]
    fn compounds_and_more_words_are_read() {
        assert_eq!(classify("wc/graffitiwall_01", 0.6), Verdict::Keep);
        assert_eq!(classify("me_billboard_a", 0.6), Verdict::Keep);
        assert!(matches!(classify("wc/ground_soil_03", 0.6), Verdict::Substitute(Class::Dirt, _)));
        assert!(matches!(classify("wc/me_granite_block", 0.6), Verdict::Substitute(Class::Rock, _)));
        assert!(matches!(classify("sidewalk_a", 0.6), Verdict::Substitute(Class::Concrete, _)));
        // "logo" is a flag word but "log" alone is wood and "design" is not "sign".
        assert!(matches!(classify("log_cabin_wall", 0.6), Verdict::Substitute(Class::Wood, _)));
        assert_eq!(classify("design_wall", 0.6), Verdict::Unmatched);
    }

    #[test]
    fn the_surface_type_names_a_material_the_name_does_not() {
        // Nothing in the name; the collision says brick (2) -> Brick at 0.85.
        assert_eq!(class_of_bo3(Some(2), &["wc/ab_wall_07"], 0.6), Some((Class::Brick, 0.85)));
        assert_eq!(surface_key(",wc/Ab_Wall_07"), "ab_wall_07");
    }

    #[test]
    fn colours_that_do_not_suit_a_class_are_rejected() {
        let snow = TexStats { lin: [0.6, 0.6, 0.65], spread: 0.05 };
        let mud = TexStats { lin: [0.2, 0.12, 0.05], spread: 0.1 };
        assert!(plausible(Class::Snow, &snow) && !plausible(Class::Snow, &mud));
        assert!(plausible(Class::Foliage, &TexStats { lin: [0.05, 0.15, 0.04], spread: 0.2 }));
        assert!(!plausible(Class::Foliage, &TexStats { lin: [0.05, 0.05, 0.3], spread: 0.2 }));
        assert!(plausible(Class::Concrete, &mud));
    }

    #[test]
    fn the_tint_only_darkens_and_has_a_floor() {
        let pale = TexStats { lin: [0.5, 0.5, 0.5], spread: 0.1 };
        let dark = TexStats { lin: [0.1, 0.1, 0.1], spread: 0.1 };
        let t = tint_toward(&dark, &pale);
        assert!(t.iter().all(|&c| (TINT_FLOOR..=1.0).contains(&c)) && t[0] < 1.0, "{t:?}");
        // A darker pick than the ghost is left alone.
        assert_eq!(tint_toward(&pale, &dark), [1.0; 3]);
        // The tinted pick is nearer the ghost than the pick was.
        assert!(distance(&dark, &tinted(&pale, t)) < distance(&dark, &pale));
    }

    #[test]
    fn the_names_of_a_material_and_its_picture_both_count() {
        assert!(matches!(classify_all(&["mp_x_wall_07", "~mp_x_concrete_col"], 0.6), Verdict::Substitute(Class::Concrete, _)));
        assert_eq!(classify_all(&["mp_x_concrete_01", "decal_sign_col"], 0.6), Verdict::Keep);
        assert_eq!(classify_all(&["a_b", "c_d"], 0.6), Verdict::Unmatched);
    }

    #[test]
    fn a_swap_keeps_the_binding_hashes_of_the_material() {
        let b = |hash, semantic, image| MaterialTextureBinding { name_hash: hash, name_start: 0, name_end: 0, sampler_state: 0, semantic, image: Some(image) };
        let mut m = test_material(vec![b(0xaa, TS_COLOR_MAP, 1), b(0xbb, asset_material::TS_NORMAL_MAP, 2)]);
        apply_textures(&mut m, &[b(0x11, TS_COLOR_MAP, 9)]);
        assert_eq!(m.textures.len(), 1, "a normal map of the other texture is dropped");
        assert_eq!((m.textures[0].name_hash, m.textures[0].image), (0xaa, Some(9)));
    }

    fn test_material(textures: Vec<MaterialTextureBinding>) -> asset_material::AuthoredMaterial {
        use asset_core::{AssetEdge, AssetNamespace, AssetRef};
        asset_material::AuthoredMaterial {
            name: AssetRef::Real("m".into()),
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
            textures,
            constants: Vec::new(),
            zone: Default::default(),
            t6_draw: None,
        }
    }

    /// Reads the real Black Ops III pool and matches synthetic Ghosts materials
    /// against it, printing what the load report would say (needs the install:
    /// `CODLEGACY_GAMES=<Black Ops III folder> cargo test -p assets --lib
    /// bo3_pool_exercise -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn bo3_pool_exercise() {
        let mut cat = MaterialCatalog::default();
        let before = cat.materials.len();
        let (cat2, res) = super::super::t7::bo3_pool_run(std::mem::take(&mut cat), 256, |pool| {
            let mut by_class = std::collections::BTreeMap::<Class, usize>::new();
            for it in pool.items() {
                *by_class.entry(it.class).or_default() += 1;
            }
            println!("pool {} items by class {by_class:?}", pool.items().len());
            for it in pool.items().iter().take(40) {
                println!("  {:?} {:.2} {} lin {:.3?} spread {:.2}", it.class, it.name_conf, it.name, it.stats.lin, it.stats.spread);
            }
            // Ghosts-like materials of every class at a few typical colours.
            let mut ghosts = Vec::new();
            for class in [Class::Concrete, Class::Snow, Class::Dirt, Class::Rock, Class::Metal, Class::Wood, Class::Brick, Class::Asphalt, Class::Grass, Class::Foliage] {
                for l in [0.05f32, 0.15, 0.35, 0.7] {
                    ghosts.push(Ghost { name: format!("ghost_{class:?}_{l}"), class, name_conf: 0.9, stats: TexStats { lin: [l, l * 0.95, l * 0.85], spread: 0.12 } });
                }
            }
            let d = decide(&ghosts, pool, 0.6, 0.2);
            for (g, d) in ghosts.iter().zip(&d) {
                match d {
                    Decision::Swap { item, confidence, dist, textures, .. } => println!("{} -> {} conf {confidence:.2} dist {dist:.2}, {} bindings", g.name, pool.items()[*item].name, textures.len()),
                    Decision::Keep(why) => println!("{} kept {why:?}", g.name),
                }
            }
            d.iter().filter(|d| matches!(d, Decision::Swap { .. })).count()
        });
        println!("result {:?}; catalog materials {} -> {}, images {}", res.as_ref().map(|(n, s)| (n, s.clone())), before, cat2.materials.len(), cat2.images.len());
    }

    /// What the Black Ops III zones' material names look like (the words that
    /// would give a surface class, with and without the class threshold).
    #[test]
    #[ignore]
    fn bo3_names_dump() {
        let dir = super::super::t7::bo3_zone_dir().expect("a Black Ops III install");
        for zone in ["mp_common", "core_common", "mp_patch", "core_patch"] {
            let Ok(z) = fastfile_t7::Zone::open(&dir.join(format!("{zone}.ff"))) else { continue };
            let w = fastfile_t7::walk::walk(&z);
            let mats = &w.capture.materials;
            let mut by_word = std::collections::BTreeMap::<&str, Vec<&str>>::new();
            for m in mats {
                let l = m.name.to_ascii_lowercase();
                for word in ["concrete", "rock", "stone", "metal", "steel", "wood", "brick", "dirt", "asphalt", "grass", "snow", "sand", "leaf", "foliage", "terrain", "ground", "wall", "floor", "road", "container", "rust"] {
                    if l.contains(word) {
                        by_word.entry(word).or_default().push(&m.name);
                    }
                }
            }
            println!("== {zone}: {} materials, {} images", mats.len(), w.capture.images.len());
            for (word, v) in &by_word {
                println!("  [{word}] {}: {:?}", v.len(), v.iter().take(6).collect::<Vec<_>>());
            }
            let step = (mats.len() / 30).max(1);
            println!("  sample: {:?}", mats.iter().step_by(step).map(|m| m.name.as_str()).collect::<Vec<_>>());
        }
    }

    #[test]
    fn only_generic_tileable_materials_swap() {
        // Prop and pattern specific Black Ops III materials are not candidates.
        for (n, c) in [
            ("mc/mtl_p7_crate_ammo_metal_small", "mtl_p7_crate_ammo_metal_small_c"),
            ("mc/concrete_edges01_warm", "concrete_edges01_c"),
            ("wc/zm_trim_metal_blk", "zm_trim_metal_blk_c"),
            ("wc/t7_concrete_tiles_4x4_dirty_01_grey", "t7_concrete_tiles_c"),
            ("mc/mtl_p7_sin_vista_building_complex_a_concrete_blue", "x_c"),
            ("wc/t7_brick_worn_dirty_tan", "t7_brick_c"),
            ("wc/t7_concrete_decal_grime", "t7_concrete_c"),
        ] {
            assert!(!generic_world(n, c), "{n}");
        }
        for (n, c) in [("wc/t7_mud_ground_dirt_wet_01", "t7_mud_c"), ("wc/t7_sand_rocky_01", "t7_sand_rocky_01_c"), ("wc/t7_asphalt_road_02", "x_c"), ("wc/t7_snow_01", "t7_snow_01_c")] {
            assert!(generic_world(n, c), "{n}");
        }
    }

    #[test]
    fn a_foreign_material_is_swapped_by_name_in_a_generic_class_only() {
        assert!(!swappable(Class::Metal) && !swappable(Class::Wood) && !swappable(Class::Brick) && !swappable(Class::Foliage) && swappable(Class::Dirt));
        // A container named by the artists, or a wall the collision calls concrete with a nondescript name: own.
        assert_eq!(ghost_class(Some(13), &["mp_x_container_a", "x_col"], 0.6), None);
        assert_eq!(ghost_class(Some(5), &["*a101n_155z", "~x_col"], 0.6), None, "a surface type alone is not enough");
        assert!(matches!(ghost_class(Some(5), &["mp_x_concrete_wall", "x_col"], 0.6), Some((Class::Concrete, _))));
        assert_eq!(ghost_class(Some(13), &["mp_x_concrete_wall", "x_col"], 0.6), None, "the collision says metal");
    }

    #[test]
    fn a_pick_of_another_contrast_is_not_chosen() {
        let mut pool = Fake(vec![PoolItem { name: "busy".into(), class: Class::Concrete, name_conf: 0.9, stats: stats(0.3, 0.3) }], 0);
        let d = decide(&[ghost("flat", Class::Concrete, 0.3)], &mut pool, 0.6, 0.2);
        assert!(matches!(d[0], Decision::Keep(Why::NoFit(None))), "{:?}", d[0]);
    }

    #[test]
    fn a_pick_of_another_hue_is_not_chosen() {
        let mut pool = Fake(vec![PoolItem { name: "mauve".into(), class: Class::Dirt, name_conf: 0.9, stats: TexStats { lin: [0.3, 0.18, 0.3], spread: 0.1 } }], 0);
        let brown = Ghost { name: "brown".into(), class: Class::Dirt, name_conf: 0.9, stats: TexStats { lin: [0.3, 0.22, 0.14], spread: 0.1 } };
        let d = decide(&[brown], &mut pool, 0.6, 0.2);
        assert!(matches!(d[0], Decision::Keep(Why::NoFit(None))), "{:?}", d[0]);
    }

    #[test]
    fn the_tally_counts_every_material() {
        let g = ghost("a_concrete", Class::Concrete, 0.3);
        let subjects = vec![
            ("a_concrete".to_owned(), Subject::Candidate(g)),
            ("b_sign".to_owned(), Subject::Own),
            ("c_zzz".to_owned(), Subject::Unmatched),
            ("d_metal".to_owned(), Subject::Ineligible("layered")),
        ];
        let items = vec![item("bo3_concrete", Class::Concrete, 0.3)];
        let mut pool = Fake(items.clone(), 0);
        let d = decide(&[ghost("a_concrete", Class::Concrete, 0.3)], &mut pool, 0.6, 0.2);
        let lines = tally(&subjects, &d, &items).line("iw6", "test", 1, 0.6, 0.2);
        assert!(lines[0].starts_with("iw6 material substitution: 4 materials: 1 swapped"), "{}", lines[0]);
        assert!(lines[0].contains("1 kept (game's own") && lines[0].contains("1 unmatched") && lines[0].contains("layered"), "{}", lines[0]);
        assert!(lines.iter().any(|l| l.contains("a_concrete -> bo3_concrete")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("by class: Concrete 1 swapped")), "{lines:?}");
    }
}

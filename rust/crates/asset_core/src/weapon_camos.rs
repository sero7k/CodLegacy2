//! bo2mp: Black Ops II weapon camos, shared by the match load (which reads
//! them), the client (which knows who wears which) and the draw.
//!
//! A camo is a weapon option (`mp/attachmenttable.csv` "camo" rows, the
//! number Create-a-Class saves as `primarycamo` / `secondarycamo`).
//! `mp/weaponoptions.csv` says how it looks: a material camo (gold,
//! diamond, carbon fibre, the DLC camos) swaps each of a gun's camo-ready
//! materials for the camo's material, its pictures tiled across the gun
//! (the gun's `WeaponCamo` material sets); a pattern camo lays a pattern
//! picture under the gun's colour by its colour map's alpha. The other
//! games' camos (numbers below) are patterns over whole materials of every
//! gun, under that mask on Black Ops II's own.
//!
//! Who wears what rides on each player's script values (`bo2mp_camos`:
//! `<weapon>:<option>,...`, written by the server's `giveweapon`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A camo's material for one of a gun's materials: the catalog material
/// drawn instead and how many times its pictures repeat across the gun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialCamo {
    pub material: u32,
    pub tile: f32,
}

/// A pattern camo on one of a gun's materials: the pattern picture (a
/// texture slot) and how many times it repeats across the gun. `full`: it
/// covers the whole material (another game's gun, whose colour map's alpha
/// is no camo mask).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternCamo {
    pub image: u32,
    pub scale: f32,
    pub full: bool,
    /// How the gun's shader moves it (0: still; else `EffectCamo::code`:
    /// a `STYLE_*` and a speed, riding in instance slot 22 above 10000).
    pub style: u16,
}

impl PatternCamo {
    /// A still pattern (the plain camos').
    #[must_use]
    pub const fn still(image: u32, scale: f32, full: bool) -> Self {
        Self { image, scale, full, style: 0 }
    }

    /// Whether this is the Ghost look: white and see-through, the gun
    /// drawn in the blended pass.
    #[must_use]
    pub fn is_ghost(&self) -> bool {
        self.style % STYLE_RADIX == STYLE_GHOST
    }
}

impl PatternCamo {
    /// Instance slot 22's value for its surfaces: 3000 + its repeat laid
    /// under the gun's colour by the colour map's alpha, as Black Ops II's
    /// gun shaders do; 5000 + its repeat over the whole surface.
    #[must_use]
    pub fn slot(&self) -> f32 {
        let repeat = if self.scale > 0.0 { self.scale.min(900.0) } else { 1.0 };
        let base = if self.full { 5000.0 + repeat } else { 3000.0 + repeat };
        // (The shader reads the style as `floor(slot / 10000)`.)
        base + 10000.0 * f32::from(self.style)
    }
}

/// The camo numbers of the other games' camos (Black Ops II's are its own
/// weapon options, below 100): Modern Warfare 2's slot `n` is
/// `MW2_FIRST + n`, Modern Warfare 3's `MW3_FIRST + n`, Black Ops' camo
/// `n` (counted from 1) `BO1_FIRST + n`. All fit a class's camo byte.
pub const MW2_FIRST: u32 = 100;
pub const MW3_FIRST: u32 = 120;
pub const BO1_FIRST: u32 = 140;
/// Modern Warfare III's (2023) camos from `JUP_FIRST` and Modern Warfare
/// II's (2022) from `IW9_FIRST` (`asset_game::weapon_camo_models::JUP_CAMOS`,
/// `IW9_CAMOS`, counted from 0): past a byte, as the classes, the scripts'
/// weapon options and `bo2mp_camos` carry them (to `MAX_OPTION`).
pub const JUP_FIRST: u32 = 300;
pub const IW9_FIRST: u32 = 500;
/// Black Ops III's camos (`asset_game::weapon_camo_models::T7_CAMOS`): its
/// camo number `n` (1-138 in its own tables) is `T7_FIRST + n`.
pub const T7_FIRST: u32 = 850;
/// The glow pictures of Black Ops III's moving camos (Diamond 16, Ritual 26),
/// kept for the special camos only: `T7_GLOW_FIRST` + the camo's number, in
/// the numbers past the 42 its list uses.
pub const T7_GLOW_FIRST: u32 = T7_FIRST + 100;
/// Infinite Warfare's camos (the IW7 lane's camo table and pictures): its
/// camo `n` (counted from 0) is `IW7_FIRST + n`, up to 33 of them, in the
/// room left under `MAX_OPTION` past Black Ops III's.
pub const IW7_FIRST: u32 = 990;
/// The special camos' (`EFFECT_CAMOS`) numbers: `FX_FIRST + index`.
pub const FX_FIRST: u32 = 200;
/// The player's own camos (PNG pictures in the `camos` folder next to the
/// game, or `CODLEGACY_CAMOS`): `USER_FIRST` + their place in name order,
/// at most `USER_MAX`, in the room between the special camos and Modern
/// Warfare III's.
pub const USER_FIRST: u32 = 220;
pub const USER_MAX: u32 = 40;
const _: () = assert!(FX_FIRST + 15 <= USER_FIRST && USER_FIRST + USER_MAX <= JUP_FIRST);

pub use crate::camo_pack::UserCamo;

/// A file's name (no extension) as a camo: its words, and how it moves. A
/// last word of `scroll`, `pulse`, `sweep`, `plasma` or `gold`, with a speed
/// digit 0-3 after it when wanted (`lava_scroll2`: scrolls fast), makes it
/// move; any other name is a still camo.
#[must_use]
pub fn user_camo_from_stem(stem: &str) -> (String, u16) {
    let lower = stem.to_ascii_lowercase();
    let (head, last) = match lower.rsplit_once('_') {
        Some((h, l)) => (h, l),
        None => ("", lower.as_str()),
    };
    let (word, speed) = match last.chars().last() {
        Some(d @ '0'..='3') => (&last[..last.len() - 1], u16::from(d as u8 - b'0')),
        _ => (last, 1),
    };
    let style = match word {
        "scroll" => STYLE_SCROLL,
        "pulse" => STYLE_PULSE,
        "sweep" => STYLE_SWEEP,
        "plasma" => STYLE_PLASMA,
        "gold" => STYLE_GOLD,
        _ => return (stem.replace('_', " "), 0),
    };
    let name = if head.is_empty() { stem.to_owned() } else { stem[..head.len()].to_owned() };
    (name.replace('_', " "), style + STYLE_RADIX * speed)
}

/// The camos of the CUSTOM tab, read once: the built-in ones, then the
/// player's own, single pictures and camo packs from the folder (`camo_pack`).
#[must_use]
pub fn user_camos() -> Vec<UserCamo> {
    static LIST: std::sync::OnceLock<Vec<UserCamo>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        let dir = user_camos_dir();
        crate::camo_pack::builtin().into_iter().chain(crate::camo_pack::discover(&dir)).collect()
    })
    .clone()
}

/// The player's camos folder: `CODLEGACY_CAMOS`, else `camos` beside the
/// game (a folder that need not exist).
#[must_use]
pub fn user_camos_dir() -> std::path::PathBuf {
    std::env::var_os("CODLEGACY_CAMOS")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("camos"))))
        .unwrap_or_else(|| std::path::PathBuf::from("camos"))
}

/// The largest camo number a class or a gun's options carry.
pub const MAX_OPTION: u32 = 1023;
const _: () = assert!(IW9_FIRST + 331 <= T7_FIRST && T7_FIRST + 138 <= IW7_FIRST);
const _: () = assert!(IW7_FIRST + 33 <= MAX_OPTION);
const _: () = assert!(BO1_FIRST + 15 <= FX_FIRST && FX_FIRST + 100 <= JUP_FIRST);

/// How an effect camo moves (the shader's `camo_animate`): the picture
/// drifts across the gun.
pub const STYLE_SCROLL: u16 = 1;
/// The picture's brightness breathes.
pub const STYLE_PULSE: u16 = 2;
/// A bright band glides over the picture (the diamond / gold shimmer).
pub const STYLE_SWEEP: u16 = 3;
/// Procedural swirling colour (Dark Matter's look), the picture modulating it.
pub const STYLE_PLASMA: u16 = 4;
/// Procedural gold with a gliding highlight; no picture needed.
pub const STYLE_GOLD: u16 = 5;
/// White and see-through (the look Black Ops' guns once drew with by
/// mistake): the gun is drawn blended, in no picture.
pub const STYLE_GHOST: u16 = 6;
/// A camo pack's frame animation (`camo_pack`): the picture is a filmstrip
/// of square frames stacked top to bottom, one shown at a time, `6 * 2^speed`
/// a second.
pub const STYLE_FRAMES: u16 = 7;
/// A camo pack's scroll: this plus a compass step 0-7 (0 right, 1 up-right,
/// 2 up ... counter-clockwise, 45 degrees each): the picture slides that way.
pub const STYLE_SCROLL_DIR: u16 = 8;
/// A camo pack's reveal: the picture's alpha is a mask, and a threshold
/// sweeps through it (8 s at pace 1): the picture shows where the mask is
/// above it, dim elsewhere, a hot edge on the line between (Modern
/// Warfare's Dark Matter's reveal map).
pub const STYLE_REVEAL: u16 = 16;
/// Black Ops III's three-layer emissive camo (Dark Matter): the picture
/// read three times, each sliding its own way and tinted, glowing on a dark
/// gun (`lit_emissive_scroll_3layer_plus`, numbers read from core_common).
pub const STYLE_LAYERS: u16 = 17;
/// Black Ops III's Ritual camo: two layers of its glow picture in its own
/// colour, sliding slowly (numbers read from core_common).
pub const STYLE_RITUAL: u16 = 18;
/// Black Ops III's Diamond camo: its glow picture is a 4 x 4 flipbook, a
/// frame every 2 s, laid 6 x 6 over the gun and sliding.
pub const STYLE_DIAMOND: u16 = 19;
/// A style code is `style + STYLE_RADIX * speed step` (style 0-31, speed 0-3:
/// at most 127, so slot 22's float, 10000 * code + 3000 or 5000 + repeat, keeps
/// the repeat to an eighth).
pub const STYLE_RADIX: u16 = 32;

/// A special camo: a look the shader makes (moving over time, or see-through)
/// from a picture that is already loaded or none. Adding one is a row here
/// (and nothing else): its number is `FX_FIRST` + its index, below
/// `EFFECT_CAMOS.len()`, so only ever append rows.
#[derive(Clone, Copy, Debug)]
pub struct EffectCamo {
    pub name: &'static str,
    /// The camo whose picture it moves (another game's camo number; None:
    /// any loaded camo picture carries a procedural style).
    pub source: Option<u32>,
    pub style: u16,
    /// 0-3: the animation runs at 0.5, 1, 2 or 4 times its pace.
    pub speed: u16,
}

impl EffectCamo {
    /// `PatternCamo::style`: the style plus `STYLE_RADIX` times the speed step.
    #[must_use]
    pub const fn code(&self) -> u16 {
        self.style + STYLE_RADIX * self.speed
    }
}

/// The special camos. (Sources: `T7_FIRST + 17` Dark Matter, `+ 15` Gold,
/// `+ 16` Diamond, `BO1_FIRST + 14` Gold; each falls back to a procedural
/// look on a gun of a match without that game's pictures.)
pub const EFFECT_CAMOS: [EffectCamo; 15] = [
    EffectCamo { name: "Ghost", source: None, style: STYLE_GHOST, speed: 1 },
    EffectCamo { name: "Dark Matter Live", source: Some(T7_FIRST + 17), style: STYLE_LAYERS, speed: 1 },
    EffectCamo { name: "Nebula", source: None, style: STYLE_PLASMA, speed: 2 },
    EffectCamo { name: "Gold Shimmer", source: Some(T7_FIRST + 15), style: STYLE_GOLD, speed: 1 },
    EffectCamo { name: "Diamond Shimmer", source: Some(T7_FIRST + 16), style: STYLE_SWEEP, speed: 1 },
    EffectCamo { name: "Black Ops Gold Sweep", source: Some(BO1_FIRST + 14), style: STYLE_SWEEP, speed: 2 },
    EffectCamo { name: "Drift", source: Some(T7_FIRST + 17), style: STYLE_SCROLL, speed: 1 },
    EffectCamo { name: "Heartbeat", source: Some(T7_FIRST + 17), style: STYLE_PULSE, speed: 1 },
    // (Our own looks over Black Ops III's pictures, not Black Ops III's: in
    // that game only Dark Matter, Ritual and Diamond move; the rest are still.)
    EffectCamo { name: "Chameleon Swirl", source: Some(T7_FIRST + 14), style: STYLE_PLASMA, speed: 0 },
    EffectCamo { name: "Ritual Beat", source: Some(T7_FIRST + 26), style: STYLE_PULSE, speed: 1 },
    EffectCamo { name: "Ember Beat", source: Some(T7_FIRST + 11), style: STYLE_PULSE, speed: 2 },
    EffectCamo { name: "Arctic Shimmer", source: Some(T7_FIRST + 18), style: STYLE_SWEEP, speed: 1 },
    EffectCamo { name: "Etching Shimmer", source: Some(T7_FIRST + 42), style: STYLE_SWEEP, speed: 1 },
    // (Black Ops III's own Ritual and Diamond, on their glow pictures, which
    // `lane::t7` links under `T7_GLOW_FIRST` + the camo's number.)
    EffectCamo { name: "Ritual", source: Some(T7_GLOW_FIRST + 26), style: STYLE_RITUAL, speed: 1 },
    EffectCamo { name: "Diamond Flipbook", source: Some(T7_GLOW_FIRST + 16), style: STYLE_DIAMOND, speed: 1 },
];

/// The special camo a camo number is, if it is one.
#[must_use]
pub fn effect(option: u32) -> Option<&'static EffectCamo> {
    EFFECT_CAMOS.get(option.checked_sub(FX_FIRST)? as usize)
}

/// The match's camos: per material camo option, each gun material it
/// replaces (catalog index) and what it draws instead; per pattern camo
/// option, the pattern on each camo-ready gun material.
#[derive(Clone, Debug, Default)]
pub struct CamoTable {
    pub swaps: HashMap<(u32, u32), MaterialCamo>,
    pub patterns: HashMap<(u32, u32), PatternCamo>,
    /// Options that are material camos, and their names (`camo_gold`).
    pub options: HashMap<u32, String>,
}

/// Each material camo's most used material (and its tiling): what it
/// draws on a gun that has no camo material set of its own for it.
fn representatives(swaps: &HashMap<(u32, u32), MaterialCamo>) -> HashMap<u32, MaterialCamo> {
    let mut counts: HashMap<(u32, u32, u32), usize> = HashMap::new();
    for (&(option, _), c) in swaps {
        *counts.entry((option, c.material, c.tile.to_bits())).or_default() += 1;
    }
    let mut best: HashMap<u32, ((u32, u32), usize)> = HashMap::new();
    for ((option, material, tile), n) in counts {
        let e = best.entry(option).or_insert(((material, tile), n));
        if n > e.1 || (n == e.1 && (material, tile) < e.0) {
            *e = ((material, tile), n);
        }
    }
    best.into_iter().map(|(o, ((material, tile), _))| (o, MaterialCamo { material, tile: f32::from_bits(tile) })).collect()
}

/// Each pattern camo's most used pattern, laid over a whole material.
fn pattern_representatives(patterns: &HashMap<(u32, u32), PatternCamo>) -> HashMap<u32, PatternCamo> {
    let mut counts: HashMap<(u32, u32, u32), usize> = HashMap::new();
    for (&(option, _), p) in patterns {
        *counts.entry((option, p.image, p.scale.to_bits())).or_default() += 1;
    }
    let mut best: HashMap<u32, ((u32, u32), usize)> = HashMap::new();
    for ((option, image, scale), n) in counts {
        let e = best.entry(option).or_insert(((image, scale), n));
        if n > e.1 || (n == e.1 && (image, scale) < e.0) {
            *e = ((image, scale), n);
        }
    }
    best.into_iter().map(|(o, ((image, scale), _))| (o, PatternCamo::still(image, f32::from_bits(scale), true))).collect()
}

/// Every camo for a gun material with no camo of its own for it: material
/// camos (Black Ops II's, Modern Warfare 2's and 3's) as one material, the
/// pattern camos (Black Ops II's, Black Ops') as one pattern over it all.
#[derive(Default)]
struct Anywhere {
    materials: HashMap<u32, MaterialCamo>,
    patterns: HashMap<u32, PatternCamo>,
    /// Black Ops II's guns' camo-ready materials (the others' camos draw
    /// there on its guns).
    ready: std::collections::HashSet<u32>,
    /// Those its pattern camos lay under the colour map's alpha (its camo
    /// mask), with their usual repeat: the others' patterns follow the mask
    /// there too, at that repeat (as dense as Black Ops II's own).
    masked: HashMap<u32, f32>,
    /// Modern Warfare 2's and 3's camos, and the gun materials their own
    /// camo models change (where only their own swap draws).
    guest_options: std::collections::HashSet<u32>,
    guest_bases: std::collections::HashSet<u32>,
}

static ANYWHERE: RwLock<Option<Anywhere>> = RwLock::new(None);

fn rebuild_anywhere() {
    let mut a = Anywhere::default();
    if let Some(t) = table() {
        a.materials.extend(representatives(&t.swaps));
        a.patterns.extend(pattern_representatives(&t.patterns));
        a.ready.extend(t.swaps.keys().map(|k| k.1));
        a.ready.extend(t.patterns.keys().map(|k| k.1));
        let mut repeats: HashMap<(u32, u32), usize> = HashMap::new();
        for (&(_, material), p) in &t.patterns {
            *repeats.entry((material, p.scale.to_bits())).or_default() += 1;
        }
        let mut best: HashMap<u32, (u32, usize)> = HashMap::new();
        for ((material, scale), n) in repeats {
            let e = best.entry(material).or_insert((scale, n));
            if n > e.1 || (n == e.1 && scale < e.0) {
                *e = (scale, n);
            }
        }
        a.masked.extend(best.into_iter().map(|(m, (scale, _))| (m, f32::from_bits(scale))));
    }
    if let Some(g) = GUEST.read().ok().and_then(|g| g.clone()) {
        a.materials.extend(representatives(&g));
        a.guest_options.extend(g.keys().map(|k| k.0));
        a.guest_bases.extend(g.keys().map(|k| k.1));
    }
    if let Some(e) = EXTRA.read().ok().and_then(|e| e.clone()) {
        a.patterns.extend(e);
    }
    if let Ok(g) = GAMES.read() {
        for e in g.values() {
            a.patterns.extend(e.iter().map(|(k, v)| (*k, *v)));
        }
    }
    if let Ok(mut w) = ANYWHERE.write() {
        *w = Some(a);
    }
}

/// The other games' pattern camos (Black Ops' camo pictures), by camo
/// number, each over a whole material.
static EXTRA: RwLock<Option<HashMap<u32, PatternCamo>>> = RwLock::new(None);

/// The guest guns' lanes' camos (their pictures), by game and camo number,
/// each over a whole material: Modern Warfare III's and II's (`jup`), Black
/// Ops III's (`t7`). Installed once per game apart from Black Ops' (each
/// match load replaces those), so one game's lane never drops another's.
static GAMES: RwLock<std::collections::BTreeMap<&'static str, HashMap<u32, PatternCamo>>> =
    RwLock::new(std::collections::BTreeMap::new());

fn install_game_patterns(game: &'static str, patterns: HashMap<u32, PatternCamo>) {
    if let Ok(mut g) = GAMES.write() {
        if patterns.is_empty() {
            g.remove(game);
        } else {
            g.insert(game, patterns);
        }
    }
    rebuild_anywhere();
}

pub fn install_jup_patterns(patterns: HashMap<u32, PatternCamo>) {
    install_game_patterns("jup", patterns);
}

pub fn install_t7_patterns(patterns: HashMap<u32, PatternCamo>) {
    install_game_patterns("t7", patterns);
}

/// Infinite Warfare's camos (by `IW7_FIRST + n`), over a whole material.
pub fn install_iw7_patterns(patterns: HashMap<u32, PatternCamo>) {
    install_game_patterns("iw7", patterns);
}

pub fn install_extra_patterns(patterns: HashMap<u32, PatternCamo>) {
    if let Ok(mut e) = EXTRA.write() {
        *e = (!patterns.is_empty()).then_some(patterns);
    }
    rebuild_anywhere();
}

/// Whether another game's camo draws on this gun material: any game's gun
/// material in the wrap set, or one of Black Ops II's camo-ready ones.
fn camo_ready(material: u32) -> bool {
    wraps(material) || ANYWHERE.read().ok().is_some_and(|a| a.as_ref().is_some_and(|a| a.ready.contains(&material)))
}

static TABLE: RwLock<Option<Arc<CamoTable>>> = RwLock::new(None);

/// The match load's camos (None: no camos this match).
pub fn install(table: Option<CamoTable>) {
    if let Ok(mut t) = TABLE.write() {
        *t = table.map(Arc::new);
    }
    rebuild_anywhere();
}

/// bo2mp: the materials of every other game's guns (glass, sights and hands
/// left out): every game's camos draw over them whole.
static WRAP: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

pub fn install_wrap(materials: std::collections::HashSet<u32>) {
    if let Ok(mut w) = WRAP.write() {
        *w = (!materials.is_empty()).then_some(materials);
    }
}

/// Whether Black Ops II's material camos draw on this gun material.
#[must_use]
pub fn wraps(material: u32) -> bool {
    WRAP.read().ok().and_then(|w| w.as_ref().map(|w| w.contains(&material))).unwrap_or(false)
}

pub fn table() -> Option<Arc<CamoTable>> {
    TABLE.read().ok().and_then(|t| t.clone())
}

/// bo2mp: the other games' guns' camos (Modern Warfare 2's and 3's camo
/// models as material swaps), by (camo slot, gun material); kept apart from
/// Black Ops II's table, which each match load replaces.
static GUEST: RwLock<Option<HashMap<(u32, u32), MaterialCamo>>> = RwLock::new(None);

pub fn install_guest(swaps: HashMap<(u32, u32), MaterialCamo>) {
    if let Ok(mut g) = GUEST.write() {
        *g = (!swaps.is_empty()).then_some(swaps);
    }
    rebuild_anywhere();
}

/// bo2mp: Black Ops II's Pack-a-Punch camo on other games' guns in
/// Zombies (their upgrades: `bo2_profile::guest::ZmUpgrade`): the camo
/// option the server marks such a gun with, and the zombies camo's
/// material (the one Black Ops II's upgraded guns' camo set swaps in most).
static PAP: RwLock<Option<(u32, MaterialCamo)>> = RwLock::new(None);
/// The other games' guns' materials it draws over (every game's, Modern
/// Warfare 2's and 3's too; glass, sights and hands left out).
static PAP_WRAP: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// The match's Pack-a-Punch camo for other games' guns (None: off a
/// Zombies map, or the map's camo set gave none).
pub fn install_pap(pap: Option<(u32, MaterialCamo)>) {
    if let Ok(mut p) = PAP.write() {
        *p = pap;
    }
}

pub fn install_pap_wrap(materials: std::collections::HashSet<u32>) {
    if let Ok(mut w) = PAP_WRAP.write() {
        *w = (!materials.is_empty()).then_some(materials);
    }
}

/// The Pack-a-Punch camo's material for a guest gun's `material`, when
/// `option` is the Pack-a-Punch option.
fn pap_swap(option: u32, material: u32) -> Option<MaterialCamo> {
    let (o, camo) = (*PAP.read().ok()?)?;
    (o == option && PAP_WRAP.read().ok()?.as_ref()?.contains(&material)).then_some(camo)
}

/// What `option` draws a gun's `material` with, if it changes it.
#[must_use]
pub fn swap(option: u32, material: u32) -> Option<MaterialCamo> {
    if option == 0 {
        return None;
    }
    if let Some(camo) = pap_swap(option, material) {
        return Some(camo);
    }
    let own = TABLE.read().ok().and_then(|t| t.as_ref()?.swaps.get(&(option, material)).copied());
    own.or_else(|| GUEST.read().ok()?.as_ref()?.get(&(option, material)).copied()).or_else(|| {
        if own_pattern(option, material).is_some() || !camo_ready(material) {
            return None;
        }
        let a = ANYWHERE.read().ok()?;
        let a = a.as_ref()?;
        if a.guest_options.contains(&option) && a.guest_bases.contains(&material) {
            return None;
        }
        a.materials.get(&option).copied()
    })
}

fn own_pattern(option: u32, material: u32) -> Option<PatternCamo> {
    TABLE.read().ok()?.as_ref()?.patterns.get(&(option, material)).copied()
}

/// The pattern `option` lays on a gun's `material`, if it is camo-ready.
#[must_use]
pub fn pattern(option: u32, material: u32) -> Option<PatternCamo> {
    if option == 0 {
        return None;
    }
    if let Some(fx) = effect(option) {
        return effect_pattern(fx, material);
    }
    plain_pattern(option, material)
}

/// bo2mp: every pattern camo's picture (material catalog image slot), of
/// every game installed: Black Ops II's own patterns, Black Ops', the guest
/// lanes' (Black Ops III's, Modern Warfare III's). These pictures are bound
/// by no material (the gun shader lays them from the camo table), so the
/// world's upload has to be told they are reachable, or a worn camo draws
/// as nothing and its gun loses its shine maps with it. Sorted, no repeats.
#[must_use]
pub fn pattern_images() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    if let Some(t) = table() {
        out.extend(t.patterns.values().map(|p| p.image));
    }
    if let Some(e) = EXTRA.read().ok().and_then(|e| e.clone()) {
        out.extend(e.values().map(|p| p.image));
    }
    if let Ok(g) = GAMES.read() {
        out.extend(g.values().flat_map(|e| e.values().map(|p| p.image)));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A special camo on a gun material: its source camo's picture (else any
/// loaded camo's, which a procedural style only modulates) with its style;
/// over the whole material, whatever mask the gun's own camos follow.
fn effect_pattern(fx: &EffectCamo, material: u32) -> Option<PatternCamo> {
    let from_source = fx.source.filter(|&s| effect(s).is_none()).and_then(|s| plain_pattern(s, material));
    let base = from_source.or_else(|| {
        let carrier = {
            let a = ANYWHERE.read().ok()?;
            let a = a.as_ref()?;
            a.patterns.keys().copied().min()?
        };
        plain_pattern(carrier, material)
    })?;
    // (A picture that moves keeps its own repeat; the procedural looks take
    // the gun's uv as it comes.)
    Some(PatternCamo { full: true, style: fx.code(), ..base })
}

fn plain_pattern(option: u32, material: u32) -> Option<PatternCamo> {
    own_pattern(option, material).or_else(|| {
        if !camo_ready(material) {
            return None;
        }
        let a = ANYWHERE.read().ok()?;
        let a = a.as_ref()?;
        let p = a.patterns.get(&option).copied()?;
        // (On Black Ops II's own gun, under its camo mask as its own
        // patterns are, not over its metal and rubber too.)
        match a.masked.get(&material).filter(|_| !wraps(material)) {
            Some(&scale) => Some(PatternCamo { full: false, scale, ..p }),
            None => Some(p),
        }
    })
}

/// Each player's camos as the server sent them: client number -> (weapon,
/// option) pairs.
static WORN: RwLock<Option<HashMap<u32, Vec<(u32, u32)>>>> = RwLock::new(None);
/// The camo on the gun in the local player's hands (the first-person draw).
static VIEWMODEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// `bo2mp_camos`'s text: `<weapon>:<option>` pairs, comma separated.
#[must_use]
pub fn parse_worn(text: &str) -> Vec<(u32, u32)> {
    text.split(',')
        .filter_map(|pair| {
            let (w, o) = pair.split_once(':')?;
            Some((w.trim().parse().ok()?, o.trim().parse().ok()?))
        })
        .collect()
}

/// `parse_worn`'s text for these pairs.
#[must_use]
pub fn format_worn(pairs: &[(u32, u32)]) -> String {
    pairs.iter().map(|(w, o)| format!("{w}:{o}")).collect::<Vec<_>>().join(",")
}

/// This frame's camos of every player (client number, pairs).
pub fn set_worn(all: HashMap<u32, Vec<(u32, u32)>>) {
    if let Ok(mut w) = WORN.write() {
        *w = Some(all);
    }
}

/// The camo option player `client` has on `weapon` (0: none).
#[must_use]
pub fn worn(client: u32, weapon: u32) -> u32 {
    WORN.read()
        .ok()
        .and_then(|w| {
            w.as_ref()?
                .get(&client)?
                .iter()
                .find(|(x, _)| *x == weapon)
                .map(|(_, o)| *o)
        })
        .unwrap_or(0)
}

/// Whether any gun in sight wears the Ghost camo (the gun in his hands, or
/// one a player holds): the blended draw's pipelines are only made then.
#[must_use]
pub fn ghost_worn() -> bool {
    let ghost = |o: u32| effect(o).is_some_and(|fx| fx.style == STYLE_GHOST);
    ghost(viewmodel())
        || WORN
            .read()
            .ok()
            .is_some_and(|w| w.as_ref().is_some_and(|w| w.values().any(|v| v.iter().any(|(_, o)| ghost(*o)))))
}

pub fn set_viewmodel(option: u32) {
    VIEWMODEL.store(option, std::sync::atomic::Ordering::Relaxed);
}

#[must_use]
pub fn viewmodel() -> u32 {
    VIEWMODEL.load(std::sync::atomic::Ordering::Relaxed)
}

/// Instance slot 22's value for a camo surface: 1000 + its tiling (the
/// model shader tiles the surface's texture coordinates by it).
#[must_use]
pub fn tile_slot(tile: f32) -> f32 {
    1000.0 + if tile > 0.0 { tile } else { 1.0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn user_camo_names_say_how_they_move() {
        assert_eq!(super::user_camo_from_stem("red_tiger"), ("red tiger".to_owned(), 0));
        assert_eq!(super::user_camo_from_stem("lava_scroll2"), ("lava".to_owned(), super::STYLE_SCROLL + 2 * super::STYLE_RADIX));
        assert_eq!(super::user_camo_from_stem("fire_pulse"), ("fire".to_owned(), super::STYLE_PULSE + super::STYLE_RADIX));
        assert_eq!(super::user_camo_from_stem("sweep"), ("sweep".to_owned(), super::STYLE_SWEEP + super::STYLE_RADIX));
    }

    use super::*;

    /// (The tests share the tables: one at a time.)
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_material_camo_wraps_another_games_gun() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut t = CamoTable::default();
        t.swaps.insert((15, 1), MaterialCamo { material: 100, tile: 2.0 });
        t.swaps.insert((15, 2), MaterialCamo { material: 100, tile: 2.0 });
        t.swaps.insert((15, 3), MaterialCamo { material: 101, tile: 1.0 });
        install(Some(t));
        install_wrap([500].into_iter().collect());
        assert_eq!(swap(15, 500), Some(MaterialCamo { material: 100, tile: 2.0 }));
        assert_eq!(swap(15, 501), None);
        assert_eq!(swap(0, 500), None);
        assert_eq!(swap(15, 3), Some(MaterialCamo { material: 101, tile: 1.0 }));
        install(None);
        install_wrap(Default::default());
    }

    #[test]
    fn iw7_camos_wear_every_games_guns_and_leave_other_games_alone() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        install(None);
        install_wrap([710].into_iter().collect());
        install_t7_patterns([(T7_FIRST + 1, PatternCamo::still(4, 1.0, true))].into_iter().collect());
        install_iw7_patterns([(IW7_FIRST + 2, PatternCamo::still(7, 3.0, true))].into_iter().collect());
        let iw7 = pattern(IW7_FIRST + 2, 710).unwrap();
        assert_eq!((iw7.image, iw7.scale, iw7.full), (7, 3.0, true));
        // Not a camo-ready material, and a number past the installed ones.
        assert_eq!(pattern(IW7_FIRST + 2, 711), None);
        assert_eq!(pattern(IW7_FIRST + 3, 710), None);
        // Black Ops III's stays; replacing IW7's does not drop it.
        install_iw7_patterns(HashMap::new());
        assert_eq!(pattern(IW7_FIRST + 2, 710), None);
        assert_eq!(pattern(T7_FIRST + 1, 710).unwrap().image, 4);
        install_t7_patterns(HashMap::new());
        install_wrap(Default::default());
    }

    #[test]
    fn every_pattern_picture_is_listed_once() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut t = CamoTable::default();
        t.patterns.insert((20, 1), PatternCamo::still(40, 2.0, false));
        t.patterns.insert((20, 2), PatternCamo::still(40, 2.0, false));
        install(Some(t));
        install_extra_patterns([(BO1_FIRST + 1, PatternCamo::still(9, 2.0, true))].into_iter().collect());
        install_t7_patterns([(T7_FIRST + 1, PatternCamo::still(4, 1.0, true))].into_iter().collect());
        assert_eq!(pattern_images(), vec![4, 9, 40]);
        install(None);
        install_extra_patterns(Default::default());
        install_t7_patterns(HashMap::new());
        assert!(pattern_images().is_empty());
    }

    #[test]
    fn special_camos_ride_a_loaded_picture_with_their_style() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        install(None);
        install_wrap([700].into_iter().collect());
        // No picture loaded: nothing to carry the style.
        assert_eq!(pattern(FX_FIRST, 700), None);
        install_extra_patterns([(BO1_FIRST + 14, PatternCamo::still(9, 2.0, true))].into_iter().collect());
        let ghost = pattern(FX_FIRST, 700).unwrap();
        assert!(ghost.is_ghost() && ghost.full);
        assert_eq!(ghost.slot(), 10000.0 * f32::from(EFFECT_CAMOS[0].code()) + 5002.0);
        // A source camo's picture, moving.
        let gold = pattern(FX_FIRST + 5, 700).unwrap();
        assert_eq!((gold.image, gold.style), (9, EFFECT_CAMOS[5].code()));
        assert!(!gold.is_ghost());
        // Not a gun material the camos draw on.
        assert_eq!(pattern(FX_FIRST, 701), None);
        // A plain camo is still.
        assert_eq!(pattern(BO1_FIRST + 14, 700).unwrap().style, 0);
        assert!(effect(FX_FIRST + 100).is_none());
        install_extra_patterns(Default::default());
        install_wrap(Default::default());
    }

    #[test]
    fn special_camo_codes_decode_in_the_shader() {
        for fx in &EFFECT_CAMOS {
            assert!(((1..=7).contains(&(fx.code() % STYLE_RADIX)) || (STYLE_LAYERS..=STYLE_DIAMOND).contains(&(fx.code() % STYLE_RADIX))) && fx.code() / STYLE_RADIX <= 3, "{}", fx.name);
        }
        let names: std::collections::HashSet<_> = EFFECT_CAMOS.iter().map(|f| f.name).collect();
        assert_eq!(names.len(), EFFECT_CAMOS.len());
    }

    #[test]
    fn worn_text_round_trips() {
        let pairs = vec![(12, 15), (40, 3)];
        assert_eq!(parse_worn(&format_worn(&pairs)), pairs);
        assert!(parse_worn("").is_empty());
        assert_eq!(parse_worn("7:2,bad,9:x"), vec![(7, 2)]);
    }
}

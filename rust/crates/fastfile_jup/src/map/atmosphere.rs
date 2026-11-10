//! A Modern Warfare III map's own atmosphere: the vision set its zone
//! carries (fog, the sun's direction for the fog, the numbers the final
//! pass takes) and the placed lights' records.
//!
//! **Vision sets.** Every multiplayer map zone holds one or two small
//! text files, deflate-compressed, in the clear among its raw files. Each
//! line is `#x0<hex> "<value>"`: a 64-bit hash of the setting's name (not
//! the asset name hash; no name list is known) and its value as text. The
//! first file of a map names itself (`mp_jup_rust`, `mp_jup_derail_clut`: a
//! colour table) under key [`NAME`]; the global zones keep the same
//! sets for the thermal and other special visions. The settings are read
//! by hash. The ones named in [`Atmosphere`] were matched by what their
//! values do across seven maps (Rust, Derail, Afghan, Das Haus, Elevated,
//! Farm 18, Shipment) and the global sets, the way Infinity Ward's own
//! exponential fog is set (`setExpFog`/`create_vision_set_fog` in Ghosts:
//! a start and a half distance, a maximum opacity, a colour; a second colour
//! toward the sun with the sun's direction, the angles it fades in and out
//! over and a scale for the normal fog there; a base height and a half
//! height):
//!
//! - the sun-fog direction is a unit vector whose height is a round angle
//!   on the maps that use it (Rust 40, Das Haus 33, Elevated 9 degrees) and
//!   `0 0 1` on Derail, which sets its sun on worldspawn; on Shipment (a
//!   night) it points down;
//! - begin and end angles rise from the first to the second on every map
//!   (Rust 18.72 to 113.04, Shipment 0 to 90);
//! - the two distances follow the maps' look (Elevated: 700 and 15000, a
//!   clear desert; Farm 18: 350 and 12000);
//! - the two colours are 8-bit values over 255.
//!
//! The rest of a set (about sixty more numbers: the exposure and bloom
//! curves, the colour table's blend, ...) is kept raw in
//! [`VisionSet::values`]; nothing names them.
//!
//! **Placed lights.** A `light` entity of `map_ents` (the ten of Shipment
//! that scripts switch on) has a 200-byte record in the zone, found by its
//! origin; see [`PlacedLight`]. Lights baked into the volume lighting have
//! no record of their own.

use std::collections::BTreeMap;
use std::io::Read;

use super::index::{f32_at, find_all, u32_at};

/// The key every map's first set names itself by (its colour table).
pub const NAME: u64 = 0x2962_39ab_9596_8496;
/// The direction toward the sun for the fog (unit vector, z up).
pub const SUN_DIR: u64 = 0x909e_0343_c33f_0296;
/// The angle from the sun (degrees) where its fog begins to fade in.
pub const SUN_BEGIN: u64 = 0x62c8_f547_e67a_3753;
/// The angle from the sun (degrees) where it is all in.
pub const SUN_END: u64 = 0xfea3_3e4e_f0b5_5969;
/// Fog start distance.
pub const FOG_START: u64 = 0xf20b_8d6f_1754_f879;
/// Fog half distance.
pub const FOG_HALF: u64 = 0x0905_f173_a5ca_3fef;
/// Fog colour (8-bit values over 255).
pub const FOG_COLOUR: u64 = 0x56ab_b99d_944f_eebb;
/// The fog's colour toward the sun.
pub const SUN_FOG_COLOUR: u64 = 0xd412_8630_ca21_1ea9;
/// Fog opacity (its ceiling).
pub const FOG_OPACITY: u64 = 0x89a9_4552_cff7_9a58;
/// How much of the normal fog is left toward the sun (Infinity Ward's
/// `normalFogScale`).
pub const NORMAL_FOG_SCALE: u64 = 0x90db_7109_9e9d_f4a6;
/// Height fog: the height where it is thickest, and its half height.
pub const BASE_HEIGHT: u64 = 0x2d72_5e6a_9e05_ce42;
pub const HALF_HEIGHT: u64 = 0x56e8_6cc1_01ce_c126;

/// One text file of settings, by key hash.
#[derive(Clone, Debug, Default)]
pub struct VisionSet {
    pub values: BTreeMap<u64, String>,
}

impl VisionSet {
    #[must_use]
    pub fn get(&self, key: u64) -> Option<&str> {
        self.values.get(&key).map(String::as_str)
    }

    #[must_use]
    pub fn number(&self, key: u64) -> Option<f32> {
        self.get(key)?.trim().parse().ok().filter(|v: &f32| v.is_finite())
    }

    #[must_use]
    pub fn vector(&self, key: u64) -> Option<[f32; 3]> {
        let v: Vec<f32> = self.get(key)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.len() == 3 && v.iter().all(|x| x.is_finite())).then(|| [v[0], v[1], v[2]])
    }

    /// The set's own name, where it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.get(NAME).filter(|n| !n.is_empty())
    }
}

/// The marker before a compressed raw file: the "data follows" pointer.
const FOLLOW: [u8; 8] = [0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

fn parse_set(text: &str) -> Option<VisionSet> {
    let mut set = VisionSet::default();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("#x0") else { return None };
        let (hex, value) = rest.split_once(' ')?;
        let key = u64::from_str_radix(hex, 16).ok()?;
        set.values.insert(key, value.trim().trim_matches('"').to_owned());
    }
    (set.values.len() >= 16).then_some(set)
}

/// Every vision set among the zone's raw files, in zone order.
#[must_use]
pub fn scan(z: &[u8]) -> Vec<VisionSet> {
    let mut out = Vec::new();
    for at in find_all(z, &FOLLOW) {
        let s = at + 8;
        if z.get(s) != Some(&0x78) || !matches!(z.get(s + 1), Some(0x01 | 0x5e | 0x9c | 0xda)) {
            continue;
        }
        let end = z.len().min(s + (1 << 16));
        let mut text = Vec::new();
        let mut dec = flate2::read::ZlibDecoder::new(&z[s..end]).take(1 << 16);
        if dec.read_to_end(&mut text).is_err() && text.is_empty() {
            continue;
        }
        if !text.starts_with(b"#x0") {
            continue;
        }
        if let Some(set) = parse_set(&String::from_utf8_lossy(&text)) {
            out.push(set);
        }
    }
    out
}

/// The exponential fog a map sets, with a second colour toward the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub start: f32,
    pub half: f32,
    pub opacity: f32,
    /// How much of the normal fog is left toward the sun.
    pub normal_scale: f32,
    pub colour: [f32; 3],
    pub sun_colour: [f32; 3],
    /// Unit vector toward the sun (z up).
    pub sun_dir: [f32; 3],
    pub sun_begin_deg: f32,
    pub sun_end_deg: f32,
    pub base_height: f32,
    pub half_height: f32,
}

/// What a map's first vision set gives of the atmosphere.
#[derive(Clone, Debug)]
pub struct Atmosphere {
    pub name: String,
    pub fog: Option<Fog>,
    /// The set whole.
    pub set: VisionSet,
}

impl Atmosphere {
    /// Reads the zone's first vision set.
    #[must_use]
    pub fn read(z: &[u8]) -> Option<Self> {
        let set = scan(z).into_iter().find(|s| s.name().is_some())?;
        Some(Self::from_set(set))
    }

    #[must_use]
    pub fn from_set(set: VisionSet) -> Self {
        let n = |k| set.number(k);
        let fog = (|| {
            let dir = set.vector(SUN_DIR)?;
            let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
            if (len - 1.0).abs() > 0.02 {
                return None;
            }
            Some(Fog {
                start: n(FOG_START)?,
                half: n(FOG_HALF)?,
                opacity: n(FOG_OPACITY)?,
                normal_scale: n(NORMAL_FOG_SCALE).unwrap_or(1.0),
                colour: set.vector(FOG_COLOUR)?,
                sun_colour: set.vector(SUN_FOG_COLOUR)?,
                sun_dir: dir.map(|c| c / len),
                sun_begin_deg: n(SUN_BEGIN)?,
                sun_end_deg: n(SUN_END)?,
                base_height: n(BASE_HEIGHT)?,
                half_height: n(HALF_HEIGHT)?,
            })
        })();
        Self { name: set.name().unwrap_or_default().to_owned(), fog, set }
    }

    /// The direction toward the sun when the set gives one that stands for
    /// a sun: above the horizon and not straight up (`0 0 1` is a map
    /// whose worldspawn holds the sun).
    #[must_use]
    pub fn toward_sun(&self) -> Option<[f32; 3]> {
        let d = self.fog?.sun_dir;
        (d[2] > 0.02 && d[0].hypot(d[1]) > 0.05).then_some(d)
    }
}

/// A placed light's record (200 bytes, byte-packed; offsets from the
/// origin). Read: the origin; +12 the radius (150 to 260 on Shipment's
/// lights); +24 a number (2, 4 or 6); +40 and +44 two numbers that read
/// as cone cosines (0.342 and 0.643 are the cosines of 70 and 50 degrees);
/// +96 and +100 an intensity (50000) and a second number (50); +108 the
/// colour as three bytes. Whether a light is omni or spot is not decided, and
/// the units of the intensity are not known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedLight {
    pub origin: [f32; 3],
    pub radius: f32,
    /// A number of 2, 4 or 6 whose meaning is not known.
    pub shape: f32,
    pub cone: [f32; 2],
    pub intensity: f32,
    pub colour: [u8; 3],
    /// The way it points (unit; from the entity's `angles`).
    pub dir: [f32; 3],
    /// Cone numbers that make a cone (outer cosine under the inner, both
    /// in 0..1): a spot; else an omni. (Inferred: no map seen has a light
    /// of the other kind.)
    pub spot: bool,
}

/// Reads the records of the `light` entities standing at `(origin,
/// angles)`.
#[must_use]
pub fn lights(z: &[u8], places: &[([f32; 3], [f32; 3])]) -> Vec<PlacedLight> {
    let mut out = Vec::new();
    for &(o, angles) in places {
        let mut needle = [0u8; 12];
        for k in 0..3 {
            needle[4 * k..4 * k + 4].copy_from_slice(&o[k].to_le_bytes());
        }
        // The first place the origin stands with a record's shape around it.
        let found = find_all(z, &needle).into_iter().find(|&at| {
            let radius = f32_at(z, at + 12);
            (1.0..100_000.0).contains(&radius) && f32_at(z, at + 20) == 1.0 && u32_at(z, at + 64) == 3
        });
        let Some(at) = found else { continue };
        let colour = u32_at(z, at + 108);
        out.push(PlacedLight {
            origin: o,
            radius: f32_at(z, at + 12),
            shape: f32_at(z, at + 24),
            cone: [f32_at(z, at + 40), f32_at(z, at + 44)],
            intensity: f32_at(z, at + 96),
            colour: [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8],
            dir: {
                let (p, y) = (angles[0].to_radians(), angles[1].to_radians());
                [p.cos() * y.cos(), p.cos() * y.sin(), -p.sin()]
            },
            spot: {
                let c = [f32_at(z, at + 40), f32_at(z, at + 44)];
                c[0] > 0.0 && c[0] < c[1] && c[1] < 1.0
            },
        });
    }
    out
}

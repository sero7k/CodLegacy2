//! Weather grime as BlackOps3.exe sets it for a view (`141ddc190`, run once
//! a frame on the view's `LightingGlobals`):
//!
//! * the view's grime volume is the first of the world's
//!   (`GfxWeatherGrimeVolume`, 120 bytes, `GfxWorld` +0x1b0, planes +0x1c0)
//!   that holds the eye (`141dd5bb0` -> `141de4650`: inside when no plane
//!   gives `dot(n, eye) + d > 0`; a volume with no planes holds every
//!   point);
//! * `LightingGlobals.weather` (80 bytes at 0x260) takes the volume's
//!   numbers: 0x260 = +0xc, 0x264 = +0x14, (0x268, 0x26c) = (sin, cos) of
//!   +0x10 degrees, 0x270 = +0x28, 0x274 = the direction of pitch +0x18 /
//!   yaw +0x1c degrees `(cos p cos y, cos p sin y, sin p)`, 0x280 = the same
//!   of +0x20 / +0x24, 0x290..0x298 = +0x2c..0x34, 0x2a0..0x2a8 =
//!   +0x38..0x40; with no volume: zeros, (sin, cos) = (0, 1), direction
//!   (0, -1, 0), both tints 1;
//! * the code textures `gWeatherGlossMap`, `gWeatherColorMap`,
//!   `gWeatherNormalMap` and their `2`s (0x58..0x5d) take the volume's six
//!   images (+0x48 .. +0x70); a missing one, and all six with no volume,
//!   the engine's `$white_gloss`, `$gray_color`, `$normal`.
//!
//! No program in the multiplayer zones read so far (`core_common`,
//! `mp_common`, `mp_apartments`) reads the weather block or those code
//! textures (all mark `weather` unused), so on those maps this changes
//! nothing on screen; it is filled because the exe fills it.

pub const VOLUME: usize = 120;
/// The code textures the volume's images go to, in record order.
pub const CODE_TEXTURES: [u32; 6] = [0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d];
/// What each code texture takes without a volume image (the exe's
/// `141da66f0` defaults).
pub const DEFAULT_IMAGES: [&str; 6] = ["$white_gloss", "$gray_color", "$normal", "$white_gloss", "$gray_color", "$normal"];

#[derive(Clone, Debug, Default)]
pub struct Grime {
    volumes: Vec<([u8; VOLUME], [Option<String>; 6])>,
    planes: Vec<[f32; 4]>,
}

fn f32_at(r: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([r[at], r[at + 1], r[at + 2], r[at + 3]])
}

fn u32_at(r: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([r[at], r[at + 1], r[at + 2], r[at + 3]])
}

impl Grime {
    /// From the world's grime volumes and planes (file offsets into the
    /// zone image) and each volume's six image names.
    pub fn new(zone_image: &[u8], volumes: Option<(usize, usize)>, images: &[[Option<String>; 6]], planes: Option<(usize, usize)>) -> Self {
        let volumes = volumes.map_or(Vec::new(), |(at, n)| {
            (0..n)
                .filter_map(|i| {
                    let r: [u8; VOLUME] = zone_image.get(at + VOLUME * i..at + VOLUME * (i + 1))?.try_into().ok()?;
                    Some((r, images.get(i).cloned().unwrap_or_default()))
                })
                .collect()
        });
        let planes = planes.map_or(Vec::new(), |(at, n)| {
            (0..n).filter_map(|i| zone_image.get(at + 16 * i..at + 16 * (i + 1)).map(|p| [f32_at(p, 0), f32_at(p, 4), f32_at(p, 8), f32_at(p, 12)])).collect()
        });
        Self { volumes, planes }
    }

    pub fn len(&self) -> usize {
        self.volumes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.volumes.is_empty()
    }

    /// The view's volume: the first holding `eye` (`141de4650`).
    pub fn select(&self, eye: [f32; 3]) -> Option<usize> {
        self.volumes.iter().position(|(r, _)| {
            let (first, count) = (u32_at(r, 0) as usize, u32_at(r, 4) as usize);
            (first..first + count).all(|k| self.planes.get(k).is_none_or(|p| p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] + p[3] <= 0.0))
        })
    }

    /// `LightingGlobals.weather` (80 bytes, 0x260..0x2b0) for volume `sel`.
    pub fn block(&self, sel: Option<usize>) -> [u8; 80] {
        let mut b = [0u8; 80];
        let mut put = |at: usize, v: f32| b[at - 0x260..at - 0x260 + 4].copy_from_slice(&v.to_le_bytes());
        let dir = |p: f32, y: f32| {
            let (p, y) = (p.to_radians(), y.to_radians());
            [y.cos() * p.cos(), y.sin() * p.cos(), p.sin()]
        };
        match sel.and_then(|i| self.volumes.get(i)) {
            None => {
                put(0x26c, 1.0);
                put(0x278, -1.0);
                for at in [0x290, 0x294, 0x298, 0x2a0, 0x2a4, 0x2a8] {
                    put(at, 1.0);
                }
            }
            Some((r, _)) => {
                put(0x260, f32_at(r, 0xc));
                put(0x264, f32_at(r, 0x14));
                // (The exe: degrees * pi / 180, single precision.)
                let a = f32_at(r, 0x10) * std::f32::consts::PI / 180.0;
                put(0x26c, a.cos());
                put(0x268, a.sin());
                put(0x270, f32_at(r, 0x28));
                for (k, v) in dir(f32_at(r, 0x18), f32_at(r, 0x1c)).into_iter().enumerate() {
                    put(0x274 + 4 * k, v);
                }
                for (k, v) in dir(f32_at(r, 0x20), f32_at(r, 0x24)).into_iter().enumerate() {
                    put(0x280 + 4 * k, v);
                }
                for k in 0..3 {
                    put(0x290 + 4 * k, f32_at(r, 0x2c + 4 * k));
                    put(0x2a0 + 4 * k, f32_at(r, 0x38 + 4 * k));
                }
            }
        }
        b
    }

    /// The image each weather code texture takes for volume `sel`.
    pub fn images(&self, sel: Option<usize>) -> [String; 6] {
        let own = sel.and_then(|i| self.volumes.get(i)).map(|(_, im)| im.clone()).unwrap_or_default();
        std::array::from_fn(|j| own[j].clone().unwrap_or_else(|| DEFAULT_IMAGES[j].to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(first: u32, count: u32, fields: &[(usize, f32)]) -> Vec<u8> {
        let mut r = vec![0u8; VOLUME];
        r[0..4].copy_from_slice(&first.to_le_bytes());
        r[4..8].copy_from_slice(&count.to_le_bytes());
        for &(at, v) in fields {
            r[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        r
    }

    #[test]
    fn the_eyes_volume_fills_the_block_and_no_volume_the_defaults() {
        // One volume: the box x <= 10 (plane (1,0,0,-10)), wind 180 deg,
        // pitch 90, tiling 16, tints 1.
        let mut z = record(0, 1, &[(0xc, 0.5), (0x10, 180.0), (0x14, 6.0), (0x18, 90.0), (0x28, 16.0), (0x2c, 1.0), (0x30, 1.0), (0x34, 1.0), (0x38, 1.0), (0x3c, 1.0), (0x40, 1.0)]);
        let plane_at = z.len();
        for v in [1.0f32, 0.0, 0.0, -10.0] {
            z.extend_from_slice(&v.to_le_bytes());
        }
        let images = [[Some("snow_g".to_owned()), Some("snow_c".to_owned()), None, None, None, None]];
        let g = Grime::new(&z, Some((0, 1)), &images, Some((plane_at, 1)));
        assert_eq!(g.select([0.0; 3]), Some(0));
        assert_eq!(g.select([20.0, 0.0, 0.0]), None);
        let b = g.block(Some(0));
        let f = |at: usize| f32_at(&b, at - 0x260);
        assert_eq!((f(0x260), f(0x264), f(0x270)), (0.5, 6.0, 16.0));
        assert!((f(0x26c) + 1.0).abs() < 1e-6 && f(0x268).abs() < 1e-6);
        assert!((f(0x27c) - 1.0).abs() < 1e-6, "pitch 90 points up");
        let none = g.block(None);
        assert_eq!(f32_at(&none, 0x26c - 0x260), 1.0);
        assert_eq!(f32_at(&none, 0x278 - 0x260), -1.0);
        assert_eq!(g.images(Some(0))[0], "snow_g");
        assert_eq!(g.images(Some(0))[2], "$normal");
        assert_eq!(g.images(None)[1], "$gray_color");
    }

    #[test]
    fn a_volume_without_planes_holds_every_point() {
        let z = record(0, 0, &[]);
        let g = Grime::new(&z, Some((0, 1)), &[], None);
        assert_eq!(g.select([1e6, -1e6, 3.0]), Some(0));
    }
}

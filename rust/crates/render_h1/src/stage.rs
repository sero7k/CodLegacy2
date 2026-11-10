//! `HDR_STAGE_PARMS` and the sky's brightness, as MWR's renderer sets them
//! (read from the game's executable, `docs/H1_NATIVE.md`, "Sky, stages and
//! probes").
//!
//! * Each view, the engine finds the view's **stage**: the clip map's
//!   stages after the first are volumes, each with a trigger model; the
//!   first whose trigger holds the eye wins, else stage 0 (the whole map).
//!   The trigger is tested with the eye taken relative to the stage's
//!   origin (read as it is).
//! * The stage's `radiometricScale` is copied into the view, and the
//!   per-view constant block gets `HDR_STAGE_PARMS = (1000 x scale, 16, 0,
//!   0)`.
//! * The sky technique (`sky_cso_hss`) draws `min((cube x k.x + k.y) x
//!   HDR_STAGE_PARMS.x, 65504)` with `k` its material constant: the sky's
//!   brightness is the stage scale and the material, nothing else.
//!   `r_hdrSkyIntensity` and `r_hdrSkyColorTint` are among the 13 vision
//!   keys MWR's vision parser recognises and **drops** (with the glow keys).
//! * Effects read `.x` the same way; one additive colour-tint shader also
//!   reads `.y` (16) as a second scale.
//!
//! Every staged retail map (Shipment, Killhouse, Crash) has a single stage
//! with scale 0.001, so `.x` is 1. `BO2MP_H1_STAGE=fitted` brings back the
//! earlier (1, 1, 1, 1).

use glam::Vec3;

/// Which `HDR_STAGE_PARMS` the native MWR path uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMath {
    /// As read from the exe.
    Exact,
    /// The (1, 1, 1, 1) used before the exe was read.
    Fitted,
}

impl StageMath {
    pub fn parse(v: &str) -> Self {
        if v.trim().eq_ignore_ascii_case("fitted") {
            Self::Fitted
        } else {
            Self::Exact
        }
    }

    /// `BO2MP_H1_STAGE`; default exact.
    pub fn from_env() -> Self {
        std::env::var("BO2MP_H1_STAGE").map_or(Self::Exact, |v| Self::parse(&v))
    }
}

/// The engine's factor from a stage's radiometric scale to
/// `HDR_STAGE_PARMS.x`.
pub const STAGE_SCALE_TO_HDR: f32 = 1000.0;
/// `HDR_STAGE_PARMS.y` as the engine sets it.
pub const HDR_STAGE_Y: f32 = 16.0;
/// The radiometric scale assumed when a map has no stage (the exposure's
/// view scalar falls back to the same value).
pub const DEFAULT_RADIOMETRIC_SCALE: f32 = 0.001;

/// One hull of a stage trigger: its box and slabs (direction, middle,
/// half size).
#[derive(Clone, Debug, Default)]
pub struct Hull {
    pub mid: Vec3,
    pub half: Vec3,
    pub slabs: Vec<(Vec3, f32, f32)>,
}

impl Hull {
    /// The engine's test: strictly inside the box on every axis and
    /// strictly inside every slab.
    pub fn contains(&self, p: Vec3) -> bool {
        let d = (p - self.mid).abs();
        d.x < self.half.x
            && d.y < self.half.y
            && d.z < self.half.z
            && self
                .slabs
                .iter()
                .all(|&(dir, mid, half)| (dir.dot(p) - mid).abs() < half)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stage {
    pub origin: Vec3,
    pub trigger: usize,
    pub radiometric_scale: f32,
}

/// The map's stages and their trigger models.
#[derive(Clone, Debug, Default)]
pub struct Stages {
    pub stages: Vec<Stage>,
    pub triggers: Vec<Vec<Hull>>,
}

impl Stages {
    pub fn from_clip(clip: Option<&fastfile_h1::map::Clip>) -> Self {
        let Some(c) = clip else {
            return Self::default();
        };
        Self {
            stages: c
                .stages
                .iter()
                .map(|s| Stage {
                    origin: Vec3::from(s.origin),
                    trigger: usize::from(s.trigger_index),
                    radiometric_scale: s.radiometric_scale,
                })
                .collect(),
            triggers: c
                .stage_triggers
                .iter()
                .map(|m| {
                    m.iter()
                        .map(|h| Hull {
                            mid: Vec3::from(h.mid),
                            half: Vec3::from(h.half),
                            slabs: h
                                .slabs
                                .iter()
                                .map(|&(d, m, s)| (Vec3::from(d), m, s))
                                .collect(),
                        })
                        .collect()
                })
                .collect(),
        }
    }

    /// The stage the eye is in: the first stage after stage 0 whose
    /// trigger holds `eye - origin`, else 0 (the "none found" result is
    /// inferred: the decompile loses the return value).
    pub fn index_at(&self, eye: Vec3) -> usize {
        (1..self.stages.len())
            .find(|&i| {
                let s = &self.stages[i];
                self.triggers
                    .get(s.trigger)
                    .is_some_and(|hulls| hulls.iter().any(|h| h.contains(eye - s.origin)))
            })
            .unwrap_or(0)
    }

    /// The radiometric scale of the eye's stage.
    pub fn radiometric_scale_at(&self, eye: Vec3) -> f32 {
        self.stages
            .get(self.index_at(eye))
            .map_or(DEFAULT_RADIOMETRIC_SCALE, |s| s.radiometric_scale)
    }
}

/// `HDR_STAGE_PARMS` for a stage scale.
pub fn hdr_stage_parms(radiometric_scale: f32, math: StageMath) -> [f32; 4] {
    match math {
        StageMath::Exact => [
            STAGE_SCALE_TO_HDR * radiometric_scale,
            HDR_STAGE_Y,
            0.0,
            0.0,
        ],
        StageMath::Fitted => [1.0; 4],
    }
}

/// The sky technique's output for a probe colour `cube` and its material
/// constant `k` (`sky_cso_hss`, as translated from its bytecode).
pub fn sky_color(cube: Vec3, k: [f32; 2], hdr_stage: [f32; 4]) -> Vec3 {
    ((cube * k[0] + Vec3::splat(k[1])) * hdr_stage[0]).min(Vec3::splat(65504.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages() -> Stages {
        Stages {
            stages: vec![
                Stage {
                    origin: Vec3::ZERO,
                    trigger: 1024,
                    radiometric_scale: 0.001,
                },
                Stage {
                    origin: Vec3::new(100.0, 0.0, 0.0),
                    trigger: 0,
                    radiometric_scale: 0.002,
                },
            ],
            triggers: vec![vec![Hull {
                mid: Vec3::ZERO,
                half: Vec3::splat(50.0),
                slabs: vec![(Vec3::X, 0.0, 10.0)],
            }]],
        }
    }

    #[test]
    fn retail_stage_gives_unit_sky() {
        // Shipment, Killhouse, Crash: one stage, scale 0.001.
        let p = hdr_stage_parms(0.001, StageMath::Exact);
        assert!((p[0] - 1.0).abs() < 1e-6);
        assert_eq!(&p[1..], &[16.0, 0.0, 0.0]);
        assert_eq!(hdr_stage_parms(0.001, StageMath::Fitted), [1.0; 4]);
        let s = Stages::default();
        assert_eq!(s.radiometric_scale_at(Vec3::ZERO), 0.001);
    }

    #[test]
    fn stage_volume_is_tested_relative_to_its_origin() {
        let s = stages();
        // Eye at the stage origin: inside its trigger (taken relative).
        assert_eq!(s.index_at(Vec3::new(100.0, 0.0, 0.0)), 1);
        assert_eq!(s.radiometric_scale_at(Vec3::new(105.0, 0.0, 0.0)), 0.002);
        // Outside the slab (x relative 20 > 10): stage 0.
        assert_eq!(s.index_at(Vec3::new(120.0, 0.0, 0.0)), 0);
        // On the box face: strict test, outside.
        assert_eq!(s.index_at(Vec3::new(100.0, 50.0, 0.0)), 0);
        // Stage 0 is never tested (its trigger index is out of range).
        assert_eq!(s.index_at(Vec3::new(0.0, 0.0, 0.0)), 0);
    }

    #[test]
    fn sky_is_scaled_by_the_stage_and_clamped() {
        let hdr = hdr_stage_parms(0.002, StageMath::Exact);
        let c = sky_color(Vec3::new(1.0, 0.5, 0.25), [2.0, 0.0], hdr);
        assert!((c - Vec3::new(4.0, 2.0, 1.0)).abs().max_element() < 1e-5);
        let c = sky_color(Vec3::splat(1e6), [1.0, 0.0], hdr);
        assert_eq!(c, Vec3::splat(65504.0));
    }
}

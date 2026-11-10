# MWR (H1) lighting settings, read live from HorizonMW's console on Rust (2026-10-21)

H1-Mod maps the game's hashed setting names to names, so typing a name in HMW's console prints `is` / `default`. All of these were at their defaults on Rust. Rust's zone has no LightSet asset.

## Tone map
| Setting | Value | Notes |
|---|---|---|
| r_tonemap | **Linear** (3) | 0 Perceptual, 1 Reinhard, 2 Filmic, 3 Linear, 4 Luminance |
| r_tonemapAuto | **1** | Auto-exposure on |
| r_tonemapExposure | 10 | Used only when auto is off |
| r_tonemapExposureAdjust | 0 | |
| r_tonemapAutoExposureAdjust | 0 | 0 = automatic adjustment |
| r_tonemapDarkEv / MidEv / LightEv | 2.84 / 7.823 / 12.81 | The scene EV points of the adjustment curve |
| r_tonemapDark/Mid/LightExposureAdjust | -3.17 / -0.0651 / 1.47 | The adjustment at those points |
| r_tonemapMin/MaxExposureAdjust | -3.17 / 2.3 | Clamp |
| r_tonemapMaxExposure | -10 | |
| r_tonemapHighlightRange | 16 | "Dynamic range, which determines white point luminance" |
| r_tonemapGamma | 1 | |
| r_tonemapWhite / Black | 512 / 0 | Filmic only |
| r_tonemapShoulder / Toe / Crossover | 0.95 / 0.2 / 2 | Filmic only |
| r_tonemapBlend | 1 | |
| r_tonemapLockAutoExposureAdjust | 1 | |
| r_tonemapAdaptSpeed | 0.02 | |
| r_tonemapUseTweaks | 0 | "Override tone map LightSet settings with tweak dvar values" |

## Surfaces and lights
| Setting | Value |
|---|---|
| r_litSurfaceHDRScalar | 1 |
| r_unlitSurfaceHDRScalar | 1 (so the sky = cube × material constant 20 × 1) |
| r_diffuseColorScale | 1.64 (point lights) |
| r_specularColorScale | 0.64 |
| r_lightGridIntensity | 1 |

A test confirmed that r_tonemapAutoExposureAdjust changes the picture live. r_tonemapUseTweaks would not change; it is probably protected.

## What this means for the port
- Replace BO3's filmic curve with a linear tone map, out = saturate(x · exposure), before the film pass.
- Exposure is automatic: average scene luminance → EV → adjustment curve above. Ours must at least approximate it per view or per map.
- Sky global = 1 (not 0.034); auto-exposure is what balances it against the world.

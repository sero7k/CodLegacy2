//! The graphics settings the native Black Ops III renderer takes, in this
//! order: a whole [`Quality`] handed in with [`set`]; the
//! `BO2MP_T7_QUALITY` preset (`low` / `medium` / `high` / `ultra`); else
//! the game's shared setting (`native_quality::current()`: Graphics >
//! GRAPHICS QUALITY and its option rows, or `BO2MP_QUALITY`, default
//! ULTRA), mapped by [`Quality::from_shared`]. Each switch's own variable
//! (`T7_SSAO_QUALITY`,
//! `BO2MP_T7_AA`, `BO2MP_T7_MOTION_BLUR`, `BO2MP_T7_MOTION_BLUR_QUALITY`,
//! `T7_NO_SUN_SHADOW`, `T7_VOLUMETRIC_GPU`, `T7_NO_LIGHTS`,
//! `T7_IMAGE_MAX_DIM`, `BO2MP_T7_LOD`, `BO2MP_T7_TEX_FILTER`, `BO2MP_T7_BLOOM`,
//! `BO2MP_T7_SPOT_SHADOWS`) still wins
//! over the preset.
//!
//! Ultra is exactly what this renderer drew before the presets: nothing
//! changes on screen at ultra. Fog (the lit fog volumes too), colour
//! grading and tone mapping are never scaled by any preset. Local lights
//! and the motion blur follow on the next frame; SSAO, the sun shadow
//! cascades and the picture size cap at the next map load (the passes and
//! pictures are made then).

use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Low,
    Medium,
    High,
    Ultra,
}

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::Low => "low",
            Preset::Medium => "medium",
            Preset::High => "high",
            Preset::Ultra => "ultra",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" | "0" => Some(Preset::Low),
            "medium" | "1" => Some(Preset::Medium),
            "high" | "2" => Some(Preset::High),
            "ultra" | "extra" | "3" => Some(Preset::Ultra),
            _ => None,
        }
    }
}

/// What the renderer draws at a preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub preset: Preset,
    /// The exe's SSAO quality dvar (2 low .. 5 ultra GTAO; below 2 off).
    pub ssao: i32,
    /// The exe's AA mode (0 off .. 5 Filmic SMAA T2x; drawn as SMAA 1x).
    pub aa: i32,
    /// The motion blur mode (`post_mblur::OFF` / `AUTO` / `ON`) and its
    /// quality (0 low, 1 medium, 2 high).
    pub motion_blur: i32,
    pub motion_blur_quality: usize,
    /// The sun shadow cascades (off: lit as the exe's shadow mode 2).
    pub sun_shadows: bool,
    /// The lit fog volumes' GPU passes (fog: on at every preset).
    pub volumetrics: bool,
    /// The map's local lights (spot / omni) in the lighting.
    pub local_lights: bool,
    /// The largest picture level kept (texels on a side).
    pub image_max_dim: u32,
    /// Static models drawn at the LOD the exe picks, those too small on
    /// screen left out (`crate::lod`; off: the finest LOD everywhere).
    pub model_lods: bool,
    /// The exe's LOD distance scale (1..4: a coarser LOD sooner).
    pub lod_scale: f32,
    /// The smallest radius on screen drawn, pixels (the exe's default 1).
    pub lod_min_radius: f32,
    /// The materials' texture-filter quality row (`crate::sampler`: 0 low,
    /// 1 the renderer's default, 2 x16 everywhere).
    pub tex_filter: usize,
    /// Local lights shadowed per view (`crate::spot_shadow`; 0 off, the
    /// exe's 4 at most) and the shadow atlas layer's size in texels (spot
    /// tiles an eighth of it, omni tiles a quarter: the exe's 2048).
    pub spot_shadows: usize,
    pub spot_shadow_size: u32,
    /// The bloom's blurs (off: skipped and no bloom added; the chain's
    /// downsamples still meter the exposure). Not an exe setting.
    pub bloom: bool,
    /// The share of each side the view is drawn at (the game's upscaler;
    /// the view's size already carries it: logged only).
    pub render_scale: f32,
}

impl Quality {
    pub const fn of(preset: Preset) -> Self {
        match preset {
            // (Ultra: the renderer's defaults before the presets.)
            Preset::Ultra => Self {
                preset,
                ssao: 3,
                aa: 5,
                motion_blur: crate::post_mblur::AUTO,
                motion_blur_quality: 1,
                sun_shadows: true,
                volumetrics: true,
                local_lights: true,
                image_max_dim: 1024,
                model_lods: false,
                lod_scale: 1.0,
                lod_min_radius: 1.0,
                tex_filter: 1,
                spot_shadows: 4,
                spot_shadow_size: 2048,
                bloom: true,
                render_scale: 1.0,
            },
            Preset::High => Self {
                preset,
                ssao: 3,
                aa: 5,
                motion_blur: crate::post_mblur::AUTO,
                motion_blur_quality: 0,
                sun_shadows: true,
                volumetrics: true,
                local_lights: true,
                image_max_dim: 1024,
                model_lods: false,
                lod_scale: 1.0,
                lod_min_radius: 1.0,
                tex_filter: 1,
                spot_shadows: 2,
                spot_shadow_size: 2048,
                bloom: true,
                render_scale: 1.0,
            },
            Preset::Medium => Self {
                preset,
                ssao: 0,
                aa: 5,
                motion_blur: crate::post_mblur::OFF,
                motion_blur_quality: 0,
                sun_shadows: true,
                volumetrics: true,
                local_lights: true,
                image_max_dim: 1024,
                model_lods: true,
                lod_scale: 1.0 / 0.75,
                lod_min_radius: 1.0 / 0.75,
                tex_filter: 1,
                spot_shadows: 0,
                spot_shadow_size: 2048,
                bloom: true,
                render_scale: 1.0,
            },
            Preset::Low => Self {
                preset,
                ssao: 0,
                aa: 5,
                motion_blur: crate::post_mblur::OFF,
                motion_blur_quality: 0,
                sun_shadows: false,
                volumetrics: true,
                local_lights: false,
                image_max_dim: 512,
                model_lods: true,
                lod_scale: 2.0,
                lod_min_radius: 2.0,
                tex_filter: 0,
                spot_shadows: 0,
                spot_shadow_size: 2048,
                bloom: false,
                render_scale: 1.0,
            },
        }
    }

    /// The game's shared setting as this renderer applies it (the table in
    /// docs/T7_NATIVE.md, "Performance"). The picture cap is this
    /// renderer's 1024 at most (`T7_IMAGE_MAX_DIM=0` lifts it).
    pub fn from_shared(q: &native_quality::Quality) -> Self {
        let preset = match q.preset {
            native_quality::Preset::Low => Preset::Low,
            native_quality::Preset::Medium => Preset::Medium,
            native_quality::Preset::High => Preset::High,
            native_quality::Preset::Ultra => Preset::Ultra,
        };
        let full = Self::of(Preset::Ultra);
        Self {
            preset,
            ssao: if q.ambient_occlusion { full.ssao } else { 0 },
            aa: full.aa,
            motion_blur: if q.motion_blur {
                full.motion_blur
            } else {
                crate::post_mblur::OFF
            },
            motion_blur_quality: full.motion_blur_quality,
            sun_shadows: q.sun_shadow,
            volumetrics: true,
            local_lights: q.local_lights,
            image_max_dim: q.max_texture.min(full.image_max_dim),
            // (Ultra's 1 and 1: the finest LOD, as before.)
            model_lods: q.lod_distance < 1.0 || q.draw_distance < 1.0,
            lod_scale: if q.lod_distance > 0.0 { (1.0 / q.lod_distance).clamp(1.0, 4.0) } else { 4.0 },
            lod_min_radius: if q.draw_distance > 0.0 { (1.0 / q.draw_distance).clamp(1.0, 10000.0) } else { 10000.0 },
            // (Anisotropy off: the exe's low row; on: its default row.)
            tex_filter: if q.anisotropy <= 1 { 0 } else { 1 },
            spot_shadows: usize::from(q.spot_shadows).min(crate::spot_shadow::EXE_PER_VIEW),
            spot_shadow_size: (q.spot_shadow_res * 4).clamp(512, 8192),
            bloom: q.bloom,
            render_scale: q.render_scale,
        }
    }

    /// The settings as a log line (`t7 fps:`).
    pub fn line(&self) -> String {
        format!(
            "preset {} (ssao {}, aa {}, motion blur {}/{}, sun shadows {}, lit fog {}, local lights {}, pictures up to {}, model LODs {}, texture filter {}, spot shadows {} at {} px, bloom {}, view at {:.0}%)",
            self.preset.name(),
            self.ssao,
            self.aa,
            self.motion_blur,
            self.motion_blur_quality,
            on_off(self.sun_shadows),
            on_off(self.volumetrics),
            on_off(self.local_lights),
            self.image_max_dim,
            if self.model_lods { format!("the exe's (scale {:.2}, smallest {:.2} px)", self.lod_scale, self.lod_min_radius) } else { "finest".into() },
            self.tex_filter,
            self.spot_shadows,
            self.spot_shadow_size,
            on_off(self.bloom),
            self.render_scale * 100.0
        )
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

static SET: RwLock<Option<Quality>> = RwLock::new(None);

/// The settings in effect: what [`set`] gave, else the `BO2MP_T7_QUALITY`
/// preset, else ultra.
pub fn get() -> Quality {
    if let Some(q) = SET.read().ok().and_then(|g| *g) {
        return q;
    }
    static ENV: std::sync::OnceLock<Option<Preset>> = std::sync::OnceLock::new();
    let mut q = match *ENV.get_or_init(|| {
        std::env::var("BO2MP_T7_QUALITY")
            .ok()
            .and_then(|v| Preset::parse(&v))
    }) {
        Some(p) => Quality::of(p),
        None => Quality::from_shared(&native_quality::current()),
    };
    // (`BO2MP_T7_BLOOM=0|1` over the preset.)
    static BLOOM: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    if let Some(b) = *BLOOM.get_or_init(|| std::env::var("BO2MP_T7_BLOOM").ok().map(|v| v.trim() != "0")) {
        q.bloom = b;
    }
    q
}

/// Whether the settings changed since revision `seen` (the shared
/// setting's revision); returns the revision now.
pub fn revision() -> u64 {
    native_quality::revision()
}

/// The game's settings hand the renderer its settings (None: back to the
/// variable / ultra).
pub fn set(q: Option<Quality>) {
    if let Ok(mut g) = SET.write() {
        *g = q;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ultra_is_the_old_default() {
        let u = Quality::of(Preset::Ultra);
        assert_eq!(u.ssao, crate::ao::QUALITY_RUN);
        assert_eq!(u.aa, crate::post_aa::EXE_DEFAULT);
        assert_eq!(u.motion_blur, crate::post_mblur::AUTO);
        assert_eq!(u.motion_blur_quality, 1);
        assert!(u.sun_shadows && u.volumetrics && u.local_lights);
        assert_eq!(u.image_max_dim, 1024);
        assert!(!u.model_lods, "ultra draws the finest LOD, as before");
        assert_eq!(u.tex_filter, crate::sampler::ANISO_QUALITY);
    }

    #[test]
    fn shared_ultra_is_ours_and_presets_only_take_away() {
        let u = Quality::from_shared(&native_quality::Quality::preset(
            native_quality::Preset::Ultra,
        ));
        assert_eq!(
            Quality {
                preset: Preset::Ultra,
                ..u
            },
            Quality::of(Preset::Ultra)
        );
        for p in [
            native_quality::Preset::Low,
            native_quality::Preset::Medium,
            native_quality::Preset::High,
        ] {
            let q = Quality::from_shared(&native_quality::Quality::preset(p));
            assert!(q.volumetrics, "fog is never scaled");
            assert!(q.image_max_dim <= u.image_max_dim && q.ssao <= u.ssao);
        }
        let low = Quality::from_shared(&native_quality::Quality::preset(
            native_quality::Preset::Low,
        ));
        assert!(
            !low.sun_shadows
                && !low.local_lights
                && low.ssao == 0
                && low.motion_blur == crate::post_mblur::OFF
        );
    }

    #[test]
    fn presets_parse() {
        assert_eq!(Preset::parse(" Medium "), Some(Preset::Medium));
        assert_eq!(Preset::parse("extra"), Some(Preset::Ultra));
        assert_eq!(Preset::parse("nope"), None);
    }
}

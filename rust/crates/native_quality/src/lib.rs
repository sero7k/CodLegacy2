//! The game's graphics settings as the native renderers (`render_iw6`,
//! `render_h1`, `render_t7`, `render_jup`, `render_iw7`) apply them: one
//! preset (Graphics > GRAPHICS QUALITY: LOW, MEDIUM, HIGH, ULTRA) and the
//! option rows that switch parts off over it (SHADOWS, BLOOM, DEPTH OF
//! FIELD, TEXTURE QUALITY, texture filtering, REAL-TIME LIGHTING), reduced
//! to plain values every renderer reads the same way ([`Quality`]).
//!
//! The settings side publishes the value whenever the settings change
//! ([`publish`]); a renderer reads [`current`] and compares [`revision`]
//! with the one it last applied (the change notification). ULTRA is
//! everything on at the native renderers' own maxima. The table is
//! `docs/IW6_NATIVE.md`, "Performance".
//!
//! `BO2MP_QUALITY=low|medium|high|ultra` sets the preset over the menu's
//! (offline tools read it with no settings at all).

use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Graphics > GRAPHICS QUALITY.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Preset {
    Low,
    Medium,
    High,
    #[default]
    Ultra,
}

impl Preset {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    pub fn key(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "ultra",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key().eq_ignore_ascii_case(key.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Ultra => "ULTRA",
        }
    }

    /// The next preset (`by` +1 or -1), wrapping.
    pub fn step(self, by: i32) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(3) as i32;
        Self::ALL[(i + by).rem_euclid(4) as usize]
    }
}

/// The option rows that work over the preset (each can only take away).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Graphics > SHADOWS.
    pub shadows: bool,
    /// Graphics > BLOOM.
    pub bloom: bool,
    /// Graphics > DEPTH OF FIELD.
    pub depth_of_field: bool,
    /// Graphics > MOTION BLUR.
    pub motion_blur: bool,
    /// Graphics > TEXTURE QUALITY: PERFORMANCE (halves the picture cap).
    pub texture_performance: bool,
    /// The settings file's texture quality: -1 automatic, 0 extra, 1 high,
    /// 2 medium, 3 low.
    pub texture_quality: i8,
    /// The settings file's texture filtering: 0 low, 1 medium, 2 high.
    pub texture_filter: u8,
    /// Graphics > REAL-TIME LIGHTING (the maps' lamps: spot and omni lights).
    pub local_lights: bool,
    /// The share of each side the match is drawn at (UPSCALING's RENDER
    /// QUALITY; 1 without upscaling). The view's size already carries it.
    pub render_scale: f32,
    /// Display > V-SYNC and the frame cap (the presentation's; recorded).
    pub vsync: bool,
    pub max_fps: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shadows: true,
            bloom: true,
            depth_of_field: true,
            motion_blur: true,
            texture_performance: false,
            texture_quality: -1,
            texture_filter: 2,
            local_lights: true,
            render_scale: 1.0,
            vsync: true,
            max_fps: 0,
        }
    }
}

/// What a native renderer applies. Fields marked "map reload" are read as
/// the map's scene is made; the rest take effect on the next frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub preset: Preset,
    /// The sun shadow map (cascades).
    pub sun_shadow: bool,
    /// One sun cascade's side, texels.
    pub sun_shadow_res: u32,
    /// How many spot lights get a shadow map this frame (0 none).
    pub spot_shadows: u8,
    /// One spot shadow tile's side, texels.
    pub spot_shadow_res: u32,
    /// Spot and omni lights' direct light (off: the baked light only).
    pub local_lights: bool,
    /// How many effect lights (the omni and spot light elements of effects)
    /// light the world at once (0 none; Ghosts' renderer).
    pub fx_lights: u8,
    /// The largest picture side loaded (map reload).
    pub max_texture: u32,
    /// Anisotropic filtering, 1 (off) to 16 (map reload).
    pub anisotropy: u16,
    pub bloom: bool,
    pub depth_of_field: bool,
    /// Screen-space ambient occlusion where a renderer has it.
    pub ambient_occlusion: bool,
    /// Motion blur where a renderer has it.
    pub motion_blur: bool,
    /// Scale on placed models' cull distances (1: the map's own).
    pub draw_distance: f32,
    /// Scale on the distance a model's next LOD starts at (1: the model's
    /// own; under 1 the coarser LODs come sooner).
    pub lod_distance: f32,
    /// The share of each side drawn (informational: the view carries it).
    pub render_scale: f32,
    pub vsync: bool,
    pub max_fps: u16,
    /// Which option rows took something away from the preset
    /// (`OFF_*` bits; 0 with every row on), named in `summary`.
    pub options_off: u8,
}

/// `Quality::options_off`: SHADOWS off.
pub const OFF_SHADOWS: u8 = 1;
/// BLOOM off.
pub const OFF_BLOOM: u8 = 2;
/// DEPTH OF FIELD off.
pub const OFF_DEPTH_OF_FIELD: u8 = 4;
/// REAL-TIME LIGHTING off.
pub const OFF_LOCAL_LIGHTS: u8 = 8;
/// TEXTURE QUALITY performance, or the file's `tex_quality` / `tex_filter`.
pub const OFF_TEXTURES: u8 = 16;
/// MOTION BLUR off.
pub const OFF_MOTION_BLUR: u8 = 32;

impl Quality {
    /// A preset with every option row on.
    pub fn preset(p: Preset) -> Self {
        let base = Self {
            preset: p,
            sun_shadow: true,
            sun_shadow_res: 1024,
            spot_shadows: 4,
            spot_shadow_res: 512,
            local_lights: true,
            fx_lights: 8,
            max_texture: 2048,
            anisotropy: 16,
            bloom: true,
            depth_of_field: true,
            ambient_occlusion: true,
            motion_blur: true,
            draw_distance: 1.0,
            lod_distance: 1.0,
            render_scale: 1.0,
            vsync: true,
            max_fps: 0,
            options_off: 0,
        };
        match p {
            Preset::Ultra => base,
            Preset::High => Self {
                spot_shadows: 2,
                fx_lights: 6,
                max_texture: 1024,
                anisotropy: 8,
                ..base
            },
            Preset::Medium => Self {
                sun_shadow_res: 512,
                spot_shadows: 0,
                fx_lights: 3,
                max_texture: 1024,
                anisotropy: 4,
                ambient_occlusion: false,
                motion_blur: false,
                draw_distance: 0.75,
                lod_distance: 0.75,
                ..base
            },
            Preset::Low => Self {
                sun_shadow: false,
                sun_shadow_res: 512,
                spot_shadows: 0,
                local_lights: false,
                fx_lights: 0,
                max_texture: 512,
                anisotropy: 1,
                bloom: false,
                depth_of_field: false,
                ambient_occlusion: false,
                motion_blur: false,
                draw_distance: 0.5,
                lod_distance: 0.5,
                ..base
            },
        }
    }

    /// A preset with the option rows over it (a row can only switch a part
    /// off or lower it).
    pub fn new(p: Preset, o: &Options) -> Self {
        let mut q = Self::preset(p);
        let before = q;
        if !o.shadows {
            q.sun_shadow = false;
            q.spot_shadows = 0;
        }
        if before.sun_shadow != q.sun_shadow || before.spot_shadows != q.spot_shadows {
            q.options_off |= OFF_SHADOWS;
        }
        q.bloom &= o.bloom;
        if before.bloom != q.bloom {
            q.options_off |= OFF_BLOOM;
        }
        q.depth_of_field &= o.depth_of_field;
        if before.depth_of_field != q.depth_of_field {
            q.options_off |= OFF_DEPTH_OF_FIELD;
        }
        q.motion_blur &= o.motion_blur;
        if before.motion_blur != q.motion_blur {
            q.options_off |= OFF_MOTION_BLUR;
        }
        q.local_lights &= o.local_lights;
        if before.local_lights != q.local_lights {
            q.options_off |= OFF_LOCAL_LIGHTS;
        }
        if !q.local_lights {
            q.spot_shadows = 0;
            q.fx_lights = 0;
        }
        let cap = match o.texture_quality {
            3 => 512,
            2 => 1024,
            1 => 2048,
            _ => u32::MAX,
        };
        q.max_texture = q.max_texture.min(cap);
        if o.texture_performance {
            q.max_texture = (q.max_texture / 2).max(256);
        }
        q.anisotropy = q.anisotropy.min(match o.texture_filter {
            0 => 1,
            1 => 4,
            _ => 16,
        });
        if (q.max_texture, q.anisotropy) != (before.max_texture, before.anisotropy) {
            q.options_off |= OFF_TEXTURES;
        }
        q.render_scale = o.render_scale;
        q.vsync = o.vsync;
        q.max_fps = o.max_fps;
        q
    }

    /// Whether going from `self` to `next` needs the map read again (the
    /// pictures' size and filtering are made as the scene loads).
    pub fn needs_reload(&self, next: &Quality) -> bool {
        self.max_texture != next.max_texture || self.anisotropy != next.anisotropy
    }

    /// One line for the logs (`iw6 fps:` and the other renderers' lines).
    pub fn summary(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "{} (sun shadow {} {}, spot shadows {} x {}, local lights {} (effect lights {}), textures <= {}, aniso {}x, bloom {}, dof {}, ao {}, draw distance x{}, lod x{}, render scale {:.0}%)",
            self.preset.label(),
            on(self.sun_shadow),
            self.sun_shadow_res,
            self.spot_shadows,
            self.spot_shadow_res,
            on(self.local_lights),
            self.fx_lights,
            self.max_texture,
            self.anisotropy,
            on(self.bloom),
            on(self.depth_of_field),
            on(self.ambient_occlusion),
            self.draw_distance,
            self.lod_distance,
            self.render_scale * 100.0,
        ) + &self.options_note()
    }

    /// What the option rows took away from the preset, for the log line
    /// (empty with every row on): e.g. `; taken away by the options: BLOOM
    /// off, REAL-TIME LIGHTING off`.
    pub fn options_note(&self) -> String {
        let rows: Vec<&str> = [
            (OFF_SHADOWS, "SHADOWS off"),
            (OFF_BLOOM, "BLOOM off"),
            (OFF_DEPTH_OF_FIELD, "DEPTH OF FIELD off"),
            (OFF_LOCAL_LIGHTS, "REAL-TIME LIGHTING off (spot and omni lights, spot shadows)"),
            (OFF_TEXTURES, "TEXTURE QUALITY / filtering lowered"),
            (OFF_MOTION_BLUR, "MOTION BLUR off"),
            (OFF_VRAM, "pictures and shadow maps lowered to fit the graphics card's memory (BO2MP_LOW_VRAM=0 keeps the preset's)"),
        ]
        .into_iter()
        .filter(|(b, _)| self.options_off & b != 0)
        .map(|(_, l)| l)
        .collect();
        if rows.is_empty() { String::new() } else { format!("; taken away by the options: {}", rows.join(", ")) }
    }
}

impl Default for Quality {
    fn default() -> Self {
        Self::preset(Preset::Ultra)
    }
}

/// `Quality::options_off`: lowered to fit the graphics card's memory
/// (`Vram::class`).
pub const OFF_VRAM: u8 = 64;

/// The graphics card as the renderer found it: its own memory (dedicated
/// video memory on DirectX 12, the device-local heaps on Vulkan; None when
/// the backend cannot say), whether it shares the PC's memory (an
/// integrated chip) and its name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Vram {
    pub bytes: Option<u64>,
    pub integrated: bool,
    pub name: String,
}

/// How much of the frame fits on the card: pictures at their full size,
/// shadow maps and targets at the preset's (`Plenty`), or lowered for a
/// card with little memory (`Low`: 4 GB and the 6 GB laptop parts; `Tight`:
/// 2 GB and the integrated chips).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VramClass {
    Tight,
    Low,
    Plenty,
}

const GIB: u64 = 1 << 30;

impl Vram {
    /// The class, from the memory when known (a card reports a little under
    /// its label: 4 GB cards say 3.8 to 4.0), else from the name (a laptop
    /// part) or an integrated chip. `BO2MP_VRAM_MB=<n>` stands in for the
    /// memory read.
    pub fn class(&self) -> VramClass {
        let bytes = std::env::var("BO2MP_VRAM_MB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|mb| mb << 20)
            .or(self.bytes);
        Self::class_of(bytes, self.integrated, &self.name)
    }

    pub fn class_of(bytes: Option<u64>, integrated: bool, name: &str) -> VramClass {
        match bytes {
            Some(b) if b <= 3 * GIB => VramClass::Tight,
            // 4 GB cards and the 6 GB laptop parts that share their bus
            // and power budget with a CPU.
            Some(b) if b <= 9 * GIB / 2 || (b <= 13 * GIB / 2 && laptop_name(name)) => VramClass::Low,
            Some(_) => VramClass::Plenty,
            None if integrated => VramClass::Tight,
            None if laptop_name(name) => VramClass::Low,
            None => VramClass::Plenty,
        }
    }

    /// One line for the log.
    pub fn line(&self) -> String {
        format!(
            "graphics card memory: {} ({}{}) -> {:?}",
            match self.bytes {
                Some(b) => format!("{:.1} GB", b as f64 / GIB as f64),
                None => "not reported by this backend".to_string(),
            },
            self.name,
            if self.integrated { ", integrated" } else { "" },
            self.class(),
        )
    }
}

/// Whether an adapter's name says it is a laptop part.
fn laptop_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["laptop", "mobile", "max-q", " m ", "notebook"].iter().any(|k| n.contains(k)) || n.ends_with(" m")
}

impl Quality {
    /// The preset's values lowered to what fits the card: `Low` keeps every
    /// picture at or under 1024 a side (the top level of a 2048 picture is
    /// three quarters of its memory), one sun cascade at or under 1024,
    /// two spot shadows at 512; `Tight` halves those again and keeps six
    /// effect lights at most. Nothing is switched off: what the player
    /// chose stays, smaller. `BO2MP_LOW_VRAM=0` leaves the preset as is.
    pub fn fit_vram(mut self, class: VramClass) -> Self {
        let before = self;
        match class {
            VramClass::Plenty => return self,
            VramClass::Low => {
                self.max_texture = self.max_texture.min(1024);
                self.sun_shadow_res = self.sun_shadow_res.min(1024);
                self.spot_shadows = self.spot_shadows.min(2);
                self.spot_shadow_res = self.spot_shadow_res.min(512);
            }
            VramClass::Tight => {
                self.max_texture = self.max_texture.min(512);
                self.sun_shadow_res = self.sun_shadow_res.min(512);
                self.spot_shadows = self.spot_shadows.min(1);
                self.spot_shadow_res = self.spot_shadow_res.min(256);
                self.fx_lights = self.fx_lights.min(6);
            }
        }
        if self != before {
            self.options_off |= OFF_VRAM;
        }
        self
    }
}

/// `BO2MP_LOW_VRAM=0`: the card's memory does not lower the preset.
fn low_vram_fit_on() -> bool {
    !std::env::var("BO2MP_LOW_VRAM").is_ok_and(|v| v.trim() == "0")
}

impl VramClass {
    /// The largest picture side the card's memory allows, apart from any
    /// preset (`u32::MAX`: as stored): what Black Ops II's own pictures,
    /// which no preset caps, are held to.
    pub fn picture_cap(self) -> u32 {
        match self {
            VramClass::Plenty => u32::MAX,
            VramClass::Low => 1024,
            VramClass::Tight => 512,
        }
    }
}

/// The picture cap the card's memory sets now (`u32::MAX` with plenty, or
/// `BO2MP_LOW_VRAM=0`).
pub fn vram_picture_cap() -> u32 {
    if low_vram_fit_on() { vram_class().picture_cap() } else { u32::MAX }
}

static CURRENT: RwLock<Option<Quality>> = RwLock::new(None);
/// What was last published: the preset and options, applied again when
/// the card's memory becomes known.
static PUBLISHED: RwLock<Option<(Preset, Options)>> = RwLock::new(None);
static VRAM: RwLock<Option<Vram>> = RwLock::new(None);
static REVISION: AtomicU64 = AtomicU64::new(0);

/// `BO2MP_QUALITY`: a preset over the menu's.
pub fn env_preset() -> Option<Preset> {
    std::env::var("BO2MP_QUALITY").ok().and_then(|v| Preset::from_key(&v))
}

/// The card's class as known now (`Plenty` until the renderer reports).
pub fn vram_class() -> VramClass {
    VRAM.read().ok().and_then(|g| g.as_ref().map(Vram::class)).unwrap_or(VramClass::Plenty)
}

/// The card as reported (None until the renderer reports).
pub fn vram() -> Option<Vram> {
    VRAM.read().ok().and_then(|g| g.clone())
}

/// The renderer found the card: what was published is applied again over
/// its memory (`Quality::fit_vram`).
pub fn set_vram(v: Vram) {
    if let Ok(mut g) = VRAM.write() {
        *g = Some(v);
    }
    if let Some((preset, options)) = PUBLISHED.read().ok().and_then(|g| *g) {
        publish(preset, &options);
    } else {
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
}

fn fitted(preset: Preset, options: &Options) -> Quality {
    let q = Quality::new(env_preset().unwrap_or(preset), options);
    if low_vram_fit_on() { q.fit_vram(vram_class()) } else { q }
}

/// The settings changed: the value every renderer applies from now (the
/// revision moves on only when it differs).
pub fn publish(preset: Preset, options: &Options) {
    if let Ok(mut g) = PUBLISHED.write() {
        *g = Some((preset, *options));
    }
    let q = fitted(preset, options);
    if let Ok(mut g) = CURRENT.write()
        && *g != Some(q)
    {
        *g = Some(q);
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
}

/// What to apply now: the published value, else (no settings: offline
/// tools) `BO2MP_QUALITY`'s preset or ULTRA with every row on, both
/// lowered to the card's memory once that is known.
pub fn current() -> Quality {
    CURRENT
        .read()
        .ok()
        .and_then(|g| *g)
        .unwrap_or_else(|| fitted(Preset::default(), &Options::default()))
}

/// Counts the published changes: a renderer that applied revision `r`
/// re-reads [`current`] when this differs.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ultra_is_everything_on() {
        let q = Quality::default();
        assert!(q.sun_shadow && q.local_lights && q.bloom && q.depth_of_field && q.ambient_occlusion && q.motion_blur);
        assert_eq!((q.spot_shadows, q.anisotropy, q.draw_distance, q.lod_distance), (4, 16, 1.0, 1.0));
        assert_eq!(q.fx_lights, 8);
    }

    #[test]
    fn presets_only_go_down() {
        for w in Preset::ALL.windows(2) {
            let (lo, hi) = (Quality::preset(w[0]), Quality::preset(w[1]));
            assert!(lo.sun_shadow_res <= hi.sun_shadow_res && lo.spot_shadows <= hi.spot_shadows);
            assert!(lo.fx_lights <= hi.fx_lights);
            assert!(lo.max_texture <= hi.max_texture && lo.anisotropy <= hi.anisotropy);
            assert!(lo.draw_distance <= hi.draw_distance && lo.lod_distance <= hi.lod_distance);
            assert!(!lo.sun_shadow || hi.sun_shadow);
        }
        assert!(!Quality::preset(Preset::Low).sun_shadow);
    }

    #[test]
    fn rows_switch_parts_off() {
        let o = Options {
            shadows: false,
            bloom: false,
            motion_blur: false,
            local_lights: false,
            texture_quality: 3,
            texture_filter: 0,
            ..Options::default()
        };
        let q = Quality::new(Preset::Ultra, &o);
        assert!(!q.sun_shadow && q.spot_shadows == 0 && !q.bloom && !q.motion_blur && !q.local_lights);
        assert_eq!(q.fx_lights, 0);
        assert_eq!((q.max_texture, q.anisotropy), (512, 1));
        assert_eq!(q.options_off, OFF_SHADOWS | OFF_BLOOM | OFF_MOTION_BLUR | OFF_LOCAL_LIGHTS | OFF_TEXTURES);
        assert!(q.summary().contains("BLOOM off") && q.summary().contains("REAL-TIME LIGHTING off"));
        assert_eq!(Quality::new(Preset::Ultra, &Options::default()).options_off, 0);
        // A row already off in the preset is not named.
        assert_eq!(Quality::new(Preset::Low, &Options { bloom: false, ..Options::default() }).options_off & OFF_BLOOM, 0);
        let half = Quality::new(Preset::High, &Options { texture_performance: true, ..Options::default() });
        assert_eq!(half.max_texture, 512);
    }

    #[test]
    fn keys_round_trip_and_step_wraps() {
        for p in Preset::ALL {
            assert_eq!(Preset::from_key(p.key()), Some(p));
        }
        assert_eq!(Preset::Ultra.step(1), Preset::Low);
        assert_eq!(Preset::Low.step(-1), Preset::Ultra);
    }

    #[test]
    fn reload_only_for_load_time_fields() {
        let u = Quality::default();
        assert!(!u.needs_reload(&Quality { sun_shadow: false, ..u }));
        assert!(u.needs_reload(&Quality { max_texture: 512, ..u }));
    }

    #[test]
    fn the_card_is_classed_by_its_memory_then_its_name() {
        let class = Vram::class_of;
        assert_eq!(class(Some(16 * GIB), false, "AMD Radeon RX 9070 XT"), VramClass::Plenty);
        assert_eq!(class(Some(8 * GIB), false, "NVIDIA GeForce RTX 4060 Laptop GPU"), VramClass::Plenty);
        // A 4 GB card reports a little under 4 GB.
        assert_eq!(class(Some(4_095 << 20), false, "NVIDIA GeForce RTX 3050 Laptop GPU"), VramClass::Low);
        assert_eq!(class(Some(4 * GIB), false, "NVIDIA GeForce GTX 1650"), VramClass::Low);
        // 6 GB: fine on a desktop, lowered on a laptop part.
        assert_eq!(class(Some(6 * GIB), false, "NVIDIA GeForce RTX 2060"), VramClass::Plenty);
        assert_eq!(class(Some(6 * GIB), false, "NVIDIA GeForce RTX 3060 Laptop GPU"), VramClass::Low);
        assert_eq!(class(Some(2 * GIB), false, "NVIDIA GeForce GTX 1050"), VramClass::Tight);
        // Nothing reported: the name and the kind of chip decide.
        assert_eq!(class(None, true, "Intel(R) Iris(R) Xe Graphics"), VramClass::Tight);
        assert_eq!(class(None, false, "NVIDIA GeForce RTX 3050 Laptop GPU"), VramClass::Low);
        assert_eq!(class(None, false, "NVIDIA GeForce RTX 3080"), VramClass::Plenty);
    }

    #[test]
    fn a_low_card_lowers_pictures_and_shadows_but_switches_nothing_off() {
        let u = Quality::default();
        let low = u.fit_vram(VramClass::Low);
        assert_eq!((low.max_texture, low.sun_shadow_res, low.spot_shadows, low.spot_shadow_res), (1024, 1024, 2, 512));
        assert!(low.sun_shadow && low.local_lights && low.bloom && low.ambient_occlusion);
        assert_eq!(low.options_off, OFF_VRAM);
        assert!(low.summary().contains("graphics card's memory"));
        let tight = u.fit_vram(VramClass::Tight);
        assert_eq!((tight.max_texture, tight.sun_shadow_res, tight.spot_shadows, tight.spot_shadow_res, tight.fx_lights), (512, 512, 1, 256, 6));
        assert_eq!(u.fit_vram(VramClass::Plenty), u);
        // A preset already under the cap is left as it is, and not marked.
        let l = Quality::preset(Preset::Low);
        assert_eq!(l.fit_vram(VramClass::Low), l);
        // The player's own lowering stays under the card's.
        let o = Options { texture_quality: 3, ..Options::default() };
        assert_eq!(Quality::new(Preset::Ultra, &o).fit_vram(VramClass::Low).max_texture, 512);
    }
}

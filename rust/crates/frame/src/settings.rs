use bevy::prelude::Resource;

/// The graphics settings as the native renderers apply them (the shared
/// hook: presets, the option rows over them, the current value).
pub use native_quality;

/// bo2mp: Options > BO3 NATIVE RENDERER, mirrored here so the map loader
/// (which has no access to the settings resource) can read it.
pub static T7_NATIVE_RENDER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// bo2mp: Options > REAL-TIME LIGHTING, mirrored here so the map loader can
/// read it (the ambient probe rebake and the local and effect lights of the
/// Black Ops III look). Read at each map load; `BO2MP_RT_AMBIENT` and
/// `BO2MP_RT_LOCAL` override it when set.
pub static RT_LIGHTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// bo2mp: the local player is out of bounds on a Black Ops III map (the
/// server's client dvar `bo2mp_oob`, read by `ui::oob_screen`): the native
/// renderer draws BO3's out-of-bounds filter while set.
pub static BO3_OOB: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// bo2mp: the local player's ADS amount (`ps.fWeaponPosFrac`, f32 bits;
/// 0 while a scope overlay is up or not aiming), set by the presentation
/// every frame: the native renderer draws Black Ops III's ADS blur from it
/// (`render_t7::post_ads`).
pub static BO3_ADS_AMOUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// bo2mp: where a Black Ops III map's native scene is in its load, for the
/// loading screen: the map loader sets `PENDING` as the load starts (with
/// the native renderer on); the renderer sets `READY` with the first
/// finished frame, or `FAILED` when the scene could not be made. While
/// `PENDING` the loading screen is held (the converted world is not built
/// then, so nothing else could be shown).
/// bo2mp: whether `render_t7` draws the Black Ops III map's placed effects
/// (its effect sprite materials made; on unless `BO2MP_T7_NATIVE_FX=0`): set by the
/// native renderer once its forward pass is prepared, cleared as a map load
/// begins. While false (no native scene, or it failed), the converted effect
/// runtime plays the placed effects the map lane read, so they never vanish.
pub mod t7_native_fx {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DRAWING: AtomicBool = AtomicBool::new(false);

    pub fn set_drawing(on: bool) {
        DRAWING.store(on, Ordering::Relaxed);
    }

    pub fn drawing() -> bool {
        DRAWING.load(Ordering::Relaxed)
    }
}

pub mod t7_native_load {
    use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
    pub const IDLE: u8 = 0;
    pub const PENDING: u8 = 1;
    pub const READY: u8 = 2;
    pub const FAILED: u8 = 3;
    static STATE: AtomicU8 = AtomicU8::new(IDLE);
    static SINCE_MS: AtomicU64 = AtomicU64::new(0);

    fn now_ms() -> u64 {
        static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        EPOCH.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }

    pub fn set(state: u8) {
        if state == PENDING {
            SINCE_MS.store(now_ms(), Ordering::Relaxed);
        }
        STATE.store(state, Ordering::Relaxed);
    }

    pub fn state() -> u8 {
        STATE.load(Ordering::Relaxed)
    }

    /// Whether the loading screen should still be held for the native
    /// scene. Gives up after a quarter of an hour (a wait that no frame
    /// ever ends must not hold the game).
    pub fn pending() -> bool {
        state() == PENDING && now_ms().saturating_sub(SINCE_MS.load(Ordering::Relaxed)) < 15 * 60 * 1000
    }
}

/// bo2mp: Options > UPSCALING. Off draws at the window's size. FSR 1 draws
/// smaller (the quality preset's share of the window) and enlarges with
/// AMD's spatial FidelityFX Super Resolution 1 (EASU + RCAS, a shader of our
/// own: any GPU). `Fsr3` (FSR 3.1, `wgpu-ffx`, Vulkan, any GPU) and `Dlss`
/// (NVIDIA RTX, Vulkan, a build with the `dlss` feature) are the temporal
/// upscalers: they take the sub-pixel camera jitter, motion vectors and depth
/// the renderer makes at the render size (`wants_temporal`). What this machine
/// can actually run is told by the renderer at start-up (`set_support`); a
/// choice it cannot run draws as the next one down (DLSS, FSR 3.1, FSR 1) and
/// says so in the log, so a settings file moved between machines still works.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Upscaler {
    #[default]
    Off,
    Fsr1,
    Fsr3,
    Dlss,
}

static FSR3_SUPPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DLSS_SUPPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// bo2mp: the safety net. A set flag makes `effective()` answer Off (every
// upscaler draws as a plain frame), whatever the setting says: the start-up
// guard (the last run ended with an upscaler on, or `BO2MP_UPSCALE_SAFE=1`),
// a machine that cannot run even FSR 1, or a render error while FSR 1 drew.
static FORCED_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// A temporal upscaler that failed at run time falls back to the next one down.
static FSR3_BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DLSS_BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// What drew the last frame (an `Upscaler` as a number), for the render-error rule.
static IN_USE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
// Render errors seen since the upscaler in use last changed.
static ERRORS_SINCE_ON: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// This many render errors while an upscaler draws switch it off whatever
/// they say: a pass that is invalid drops the whole frame's commands, which
/// freezes the picture, and the error text does not always name the culprit.
const ERRORS_BEFORE_TRIP: u32 = 3;

/// What a wgpu error's text says about the upscaler: the labels of the
/// objects our passes make (`bo2mp_upscale_*`, `bo2mp_temporal_*`,
/// `bo2mp_fsr3_*`, `bo2mp_motion_*`, FidelityFX's own "FSR3 ..." passes) and
/// the motion target's format. Lower case.
const UPSCALER_ERROR_WORDS: [&str; 7] = [
    "bo2mp_upscale",
    "bo2mp_temporal",
    "bo2mp_fsr3",
    "bo2mp_motion",
    "fsr3 ",
    "bo2mp_dlss",
    "rg16float",
];

impl Upscaler {
    /// What the OPTIONS row cycles through (all of them; the ones this
    /// machine cannot run are marked in the label and fall back).
    pub const AVAILABLE: [Self; 4] = [Self::Off, Self::Fsr1, Self::Fsr3, Self::Dlss];
    pub const ALL: [Self; 4] = [Self::Off, Self::Fsr1, Self::Fsr3, Self::Dlss];

    /// The renderer's word on what this machine runs: FSR 3.1 needs the
    /// Vulkan backend, DLSS an NVIDIA RTX card on Vulkan and a build with
    /// the `dlss` feature.
    pub fn set_support(fsr3: bool, dlss: bool) {
        FSR3_SUPPORTED.store(fsr3, std::sync::atomic::Ordering::Relaxed);
        DLSS_SUPPORTED.store(dlss, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether this machine runs it as itself (not through a fallback).
    pub fn supported(self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        match self {
            Self::Off | Self::Fsr1 => true,
            Self::Fsr3 => FSR3_SUPPORTED.load(Relaxed) && !FSR3_BROKEN.load(Relaxed),
            Self::Dlss => DLSS_SUPPORTED.load(Relaxed) && !DLSS_BROKEN.load(Relaxed),
        }
    }

    /// Turns every upscaler off for this run (`effective()` answers Off), or
    /// lifts that. The setting itself is not touched.
    pub fn force_off(on: bool) {
        FORCED_OFF.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn forced_off() -> bool {
        FORCED_OFF.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Marks this upscaler as failed for the run: DLSS and FSR 3.1 draw as
    /// the next one down, FSR 1 turns every upscaler off.
    pub fn mark_broken(self) {
        use std::sync::atomic::Ordering::Relaxed;
        match self {
            Self::Dlss => DLSS_BROKEN.store(true, Relaxed),
            Self::Fsr3 => FSR3_BROKEN.store(true, Relaxed),
            Self::Fsr1 => FORCED_OFF.store(true, Relaxed),
            Self::Off => {}
        }
    }

    /// Whether `mark_broken` was called on this one (FSR 3.1, DLSS).
    pub fn broken(self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        match self {
            Self::Dlss => DLSS_BROKEN.load(Relaxed),
            Self::Fsr3 => FSR3_BROKEN.load(Relaxed),
            Self::Fsr1 | Self::Off => false,
        }
    }

    /// Forgets every `mark_broken` and `force_off` (tests).
    pub fn reset_safety() {
        use std::sync::atomic::Ordering::Relaxed;
        FORCED_OFF.store(false, Relaxed);
        FSR3_BROKEN.store(false, Relaxed);
        DLSS_BROKEN.store(false, Relaxed);
        IN_USE.store(0, Relaxed);
    }

    /// The renderer's note of what it drew this frame.
    pub fn note_in_use(self) {
        use std::sync::atomic::Ordering::Relaxed;
        if IN_USE.swap(self as u8, Relaxed) != self as u8 {
            ERRORS_SINCE_ON.store(0, Relaxed);
        }
    }

    pub fn in_use() -> Self {
        match IN_USE.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Fsr1,
            2 => Self::Fsr3,
            3 => Self::Dlss,
            _ => Self::Off,
        }
    }

    /// Which upscaler a wgpu error (its text) is blamed on while `in_use`
    /// drew: the text names one of our passes or the motion target's format.
    /// None for an error that has nothing to do with them.
    pub fn blamed_for_error(in_use: Self, description: &str) -> Option<Self> {
        if in_use == Self::Off {
            return None;
        }
        let text = description.to_ascii_lowercase();
        UPSCALER_ERROR_WORDS
            .iter()
            .any(|word| text.contains(word))
            .then_some(in_use)
    }

    /// A render error that is not fatal arrived: when it blames the upscaler
    /// that is drawing, that one is switched off for the run (FSR 3.1 and
    /// DLSS fall to the next one down, FSR 1 to Off). Returns what was
    /// switched off, for the log.
    pub fn trip_for_render_error(description: &str) -> Option<Self> {
        use std::sync::atomic::Ordering::Relaxed;
        let in_use = Self::in_use();
        if in_use == Self::Off {
            return None;
        }
        let seen = ERRORS_SINCE_ON.fetch_add(1, Relaxed) + 1;
        let blamed = Self::blamed_for_error(in_use, description)
            .or_else(|| (seen >= ERRORS_BEFORE_TRIP).then_some(in_use))?;
        blamed.mark_broken();
        Some(blamed)
    }

    /// What is drawn for this choice on this machine: itself, or the next one
    /// down (DLSS, FSR 3.1, FSR 1).
    pub fn effective(self) -> Self {
        if Self::forced_off() {
            return Self::Off;
        }
        let mut u = self;
        while !u.supported() {
            u = match u {
                Self::Dlss => Self::Fsr3,
                _ => Self::Fsr1,
            };
        }
        u
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Fsr1 => "fsr1",
            Self::Fsr3 => "fsr3",
            Self::Dlss => "dlss",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|u| u.key() == key)
    }

    pub fn label(self) -> &'static str {
        if self != Self::Off && Self::forced_off() {
            return "OFF (TURNED OFF: SEE THE LOG)";
        }
        match self {
            Self::Off => "OFF",
            Self::Fsr1 => "FSR 1",
            Self::Fsr3 if self.broken() => "FSR 3.1 (FAILED ON THIS PC: USES FSR 1)",
            Self::Dlss if self.broken() => "DLSS (FAILED ON THIS PC: USES NEXT BEST)",
            Self::Fsr3 if self.supported() => "FSR 3.1",
            Self::Fsr3 => "FSR 3.1 (NEEDS VULKAN: USES FSR 1)",
            Self::Dlss if self.supported() => "DLSS",
            Self::Dlss => "DLSS (NOT AVAILABLE: USES NEXT BEST)",
        }
    }

    /// The next (`by` 1) or previous (-1) choice, round.
    pub fn step(self, by: i32) -> Self {
        let at = Self::AVAILABLE.iter().position(|u| *u == self).unwrap_or(0) as i32;
        Self::AVAILABLE[(at + by).rem_euclid(Self::AVAILABLE.len() as i32) as usize]
    }

    /// Whether the renderer draws smaller and enlarges (what it reads).
    pub fn active(self) -> bool {
        self != Self::Off
    }

    /// Whether this upscaler needs the temporal inputs (camera jitter, motion
    /// vectors, depth) at the render size.
    pub fn wants_temporal(self) -> bool {
        matches!(self, Self::Fsr3 | Self::Dlss)
    }
}

/// bo2mp: Graphics > GRAPHICS API, the backend wgpu is asked for at start-up
/// (wgpu has no DirectX 11). Read from the settings file before the App is
/// built, so a change applies on the next launch. `WGPU_BACKEND` still wins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphicsApi {
    #[default]
    Auto,
    Vulkan,
    Dx12,
    OpenGl,
}

impl GraphicsApi {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Vulkan, Self::Dx12, Self::OpenGl];

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vulkan => "vulkan",
            Self::Dx12 => "dx12",
            Self::OpenGl => "opengl",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Vulkan => "VULKAN",
            Self::Dx12 => "DIRECTX 12",
            Self::OpenGl => "OPENGL (UNSUPPORTED: RUNS AS AUTO)",
        }
    }

    pub fn step(self, by: i32) -> Self {
        let at = Self::ALL.iter().position(|a| *a == self).unwrap_or(0) as i32;
        Self::ALL[(at + by).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

/// bo2mp: how much of the window the game draws before the upscaler
/// enlarges it (the vendors' own presets: 67 / 58 / 50 / 33 percent of the
/// width and height).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpscaleQuality {
    #[default]
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl UpscaleQuality {
    pub const ALL: [Self; 4] = [Self::Quality, Self::Balanced, Self::Performance, Self::UltraPerformance];

    pub fn key(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::UltraPerformance => "ultra_performance",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|q| q.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quality => "QUALITY (67%)",
            Self::Balanced => "BALANCED (58%)",
            Self::Performance => "PERFORMANCE (50%)",
            Self::UltraPerformance => "ULTRA PERFORMANCE (33%)",
        }
    }

    /// The share of the window's width and height that is drawn.
    pub fn scale(self) -> f32 {
        match self {
            Self::Quality => 0.67,
            Self::Balanced => 0.58,
            Self::Performance => 0.50,
            Self::UltraPerformance => 0.33,
        }
    }

    pub fn step(self, by: i32) -> Self {
        let at = Self::ALL.iter().position(|q| *q == self).unwrap_or(0) as i32;
        Self::ALL[(at + by).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayResolution {
    pub width: u32,
    pub height: u32,
}

impl DisplayResolution {
    pub const HD: Self = Self {
        width: 1280,
        height: 720,
    };

    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl core::fmt::Display for DisplayResolution {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// bo2mp: whose soldiers the players are drawn as: Black Ops' (the CIA
/// against the Spetsnaz), Black Ops II's own, or Modern Warfare III's (2023)
/// operators (Task Force 141 against Makarov's men).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Soldiers {
    #[default]
    BlackOps,
    BlackOps2,
    Mw3,
}

impl Soldiers {
    pub const ALL: [Self; 3] = [Self::BlackOps, Self::BlackOps2, Self::Mw3];

    /// Its name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::BlackOps => "bo1",
            Self::BlackOps2 => "bo2",
            Self::Mw3 => "mw3",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BlackOps => "BLACK OPS",
            Self::BlackOps2 => "BLACK OPS II",
            Self::Mw3 => "MODERN WARFARE III",
        }
    }

    /// The next (`by` 1) or previous (-1) choice, round.
    pub fn step(self, by: i32) -> Self {
        let at = Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as i32;
        Self::ALL[(at + by).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct GameSettings {
    pub resolution: DisplayResolution,
    pub fullscreen: bool,
    pub vsync: bool,
    pub fov: f32,
    pub master_volume: f32,
    /// The music director's level (menu, lobby and match music), under
    /// `master_volume` (Black Ops II's Sound > Music slider).
    pub music_volume: f32,
    /// Low ambient music during a match (the music director's match playlist).
    pub match_music: bool,
    /// A short, map-specific music track when a player joins a match (the
    /// music director's map intro; Black Ops' join music).
    pub map_intro_music: bool,
    /// bo2zm: Black Ops II's other Sound sliders, each over the master:
    /// effects, voices, cinematics (0..1).
    pub sfx_volume: f32,
    pub voice_volume: f32,
    pub cinematic_volume: f32,
    pub brightness: f32,
    pub shadows: bool,
    pub depth_of_field: bool,
    pub bloom: bool,
    /// Graphics > MOTION BLUR (renderers that have it: Infinite Warfare's).
    pub motion_blur: bool,
    pub sensitivity: f32,
    pub invert_mouse: bool,
    pub player_name: String,

    pub pad_layout: u8,
    pub pad_stick_layout: u8,
    pub pad_sensitivity: f32,
    pub pad_ads_sensitivity: f32,
    pub pad_invert: bool,
    pub pad_curve: u8,
    pub pad_aim_assist: u8,
    pub pad_prompts: u8,
    pub pad_vibration: bool,
    pub pad_deadzone_left: f32,
    pub pad_deadzone_right: f32,

    /// bo2zm: `bo2_quality=fast` (Black Ops II maps: half-size textures,
    /// single-layer ground, props hidden sooner); `full` by default.
    pub bo2_fast: bool,
    /// bo2mp: Graphics > "Player Models": on another game's map, its own
    /// soldiers and animations (true), or Black Ops II's characters.
    pub map_game_bodies: bool,
    /// bo2mp: whose soldiers the players are (Options: SOLDIERS).
    pub soldiers: Soldiers,
    /// bo2mp: animated calling cards and emblems move (menus, match HUD,
    /// killcam), else they stand still.
    pub animated_cards: bool,
    /// bo2mp: the main menu in the earlier Classic layout (CALL OF DUTY logo,
    /// ZOMBIES / WARZONE / SPEC OPS rows) instead of the current one.
    pub classic_menu: bool,
    /// bo2mp: chat lines are read aloud in R.E.P.O.'s text-to-speech voice
    /// (needs R.E.P.O. installed); off by default.
    pub chat_tts: bool,
    /// bo2mp: Black Ops III maps are drawn by the native renderer (true,
    /// the default), else by the converted world. Read at the next map load.
    pub t7_native: bool,
    /// bo2mp: Options > GAME FOLDERS > PREPARE MAPS AT LAUNCH: at each start
    /// every installed map's fast-load files are made or brought up to date
    /// (`map_bake::ahead`), with a bar on the menus; Esc stops it and the
    /// next start goes on from there. Off by default.
    pub prepare_maps_at_launch: bool,
    /// bo2mp: Options > GAME FOLDERS > PREPARE MAPS: the games left
    /// unchecked (folder keys, comma separated, "bo3,mw2"): PREPARE MAPS
    /// skips them. Kept as the unchecked ones so a newly set game starts
    /// checked. Empty by default (every game).
    pub prepare_games_off: String,
    /// bo2mp: Video > REAL-TIME LIGHTING (the load-time ambient rebake and
    /// the local and effect lights of the Black Ops III look). On by default;
    /// read at the next map load.
    pub rt_lighting: bool,
    /// bo2mp: his friend code (8 digits, made once on this PC; 0 until
    /// then), which others add him by.
    pub friend_code: u32,
    /// bo2mp: the friend codes he added.
    pub friends: Vec<u32>,

    /// bo2zm: more of Black Ops II's Settings rows: mature content (heads
    /// and limbs come off), how many corpses stay (3, 5, 10, 16), the fps
    /// counter, the frame cap (0 = none; only without sync), FXAA, the
    /// anti-aliasing row (1 = off), texture filtering (0 low .. 2 high) and
    /// texture quality (-1 automatic, 3 low .. 0 extra; low = `bo2_fast`).
    pub mature: bool,
    pub corpses: u8,
    pub draw_fps: bool,
    /// With `draw_fps`: the full telemetry panel (frame time, graph, 1% low,
    /// CPU time) instead of the bare rate.
    pub fps_full: bool,
    /// Telemetry panel in the top left corner (default top right).
    pub fps_left: bool,
    pub max_fps: u16,
    pub fxaa: bool,
    pub aa_samples: u8,
    pub tex_filter: u8,
    pub tex_quality: i8,

    /// bo2mp: Graphics > UPSCALING, RENDER QUALITY and SHARPNESS (see
    /// `Upscaler`). Off by default. The sharpness is 0..1 (FSR 1's RCAS;
    /// 0.8 is close to AMD's default).
    pub upscaler: Upscaler,
    pub upscale_quality: UpscaleQuality,
    pub upscale_sharpness: f32,

    /// bo2mp: Graphics > GRAPHICS API (applies on the next launch).
    pub graphics_api: GraphicsApi,

    /// bo2mp: Graphics > GRAPHICS QUALITY (LOW .. ULTRA), what the native
    /// renderers draw with under the option rows (`native_quality`).
    pub graphics_quality: native_quality::Preset,

    pub revision: u64,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            resolution: DisplayResolution::HD,
            fullscreen: true,
            vsync: true,
            fov: Self::FOV_DEFAULT,
            master_volume: 1.0,
            music_volume: Self::MUSIC_VOLUME_DEFAULT,
            match_music: true,
            map_intro_music: true,
            sfx_volume: 1.0,
            voice_volume: 1.0,
            cinematic_volume: 1.0,
            brightness: 0.0,
            shadows: true,
            depth_of_field: true,
            bloom: true,
            motion_blur: true,
            sensitivity: 5.0,
            invert_mouse: false,
            player_name: "Player".to_owned(),
            pad_layout: 0,
            pad_stick_layout: 0,
            pad_sensitivity: Self::PAD_SENSITIVITY_DEFAULT,
            pad_ads_sensitivity: 1.0,
            pad_invert: false,
            pad_curve: 0,
            pad_aim_assist: 1,
            pad_prompts: 0,
            pad_vibration: true,
            pad_deadzone_left: 0.2,
            pad_deadzone_right: 0.2,
            bo2_fast: false,
            map_game_bodies: false,
            soldiers: Soldiers::BlackOps,
            animated_cards: true,
            classic_menu: false,
            chat_tts: false,
            t7_native: true,
            prepare_maps_at_launch: false,
            prepare_games_off: String::new(),
            rt_lighting: true,
            friend_code: 0,
            friends: Vec::new(),
            mature: true,
            corpses: 16,
            // The frame rate counter (SIMPLE) is on until he turns it off
            // (a saved `draw_fps=` in his settings file still wins).
            draw_fps: true,
            fps_full: false,
            fps_left: false,
            max_fps: 0,
            fxaa: false,
            aa_samples: 1,
            tex_filter: 2,
            tex_quality: -1,
            upscaler: Upscaler::Off,
            upscale_quality: UpscaleQuality::Quality,
            upscale_sharpness: Self::UPSCALE_SHARPNESS_DEFAULT,
            graphics_api: GraphicsApi::Auto,
            graphics_quality: native_quality::Preset::Ultra,
            revision: 0,
        }
    }
}

impl GameSettings {
    pub const FOV_DEFAULT: f32 = 65.0;
    pub const FOV_MIN: f32 = 65.0;
    pub const FOV_MAX: f32 = 120.0;
    /// Black Ops II's MEDIUM Look Sensitivity (step 4, input_viewSensitivity 1).
    pub const PAD_SENSITIVITY_DEFAULT: f32 = 4.0;

    /// bo2mp: Black Ops II's Look Sensitivity steps 1-14 as its options
    /// menu sets `input_viewSensitivity` (CoDBase's `CoD.SENSITIVITY_1`
    /// .. `_14`: VERY LOW 0.4, LOW 0.6, 0.8, MEDIUM 1.0, ... HIGH 1.8, ...
    /// VERY HIGH 2.5, ... INSANE 4.0). (Ours were our own: 1.0 at step 3 and
    /// every step above BO2's, 4.5 at 14.)
    pub const PAD_SENSITIVITY_STEPS: [f32; 14] = [0.4, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.25, 2.5, 3.0, 3.5, 4.0];

    /// The stick's turn-speed multiplier (`input_viewSensitivity`) for a
    /// Look Sensitivity step (between steps, in a straight line).
    pub fn pad_look_scale(step: f32) -> f32 {
        const STEPS: [f32; 14] = GameSettings::PAD_SENSITIVITY_STEPS;
        let step = step.clamp(1.0, 14.0);
        let lower = (step.floor() as usize).clamp(1, 14);
        let upper = (lower + 1).min(14);
        let t = step - lower as f32;
        STEPS[lower - 1] + (STEPS[upper - 1] - STEPS[lower - 1]) * t
    }

    /// The Look Sensitivity step of an `input_viewSensitivity` value (BO2's
    /// own options menu writes the value).
    pub fn pad_step_for_scale(scale: f32) -> f32 {
        const STEPS: [f32; 14] = GameSettings::PAD_SENSITIVITY_STEPS;
        if !scale.is_finite() || scale <= STEPS[0] {
            return 1.0;
        }
        for (k, pair) in STEPS.windows(2).enumerate() {
            if scale <= pair[1] {
                return k as f32 + 1.0 + (scale - pair[0]) / (pair[1] - pair[0]);
            }
        }
        14.0
    }
    pub const PAD_LAYOUT_CUSTOM: u8 = 255;
    pub const UPSCALE_SHARPNESS_DEFAULT: f32 = 0.8;
    pub const MUSIC_VOLUME_DEFAULT: f32 = 0.8;

    /// bo2mp: an 8-digit friend code.
    pub const fn is_friend_code(code: u32) -> bool {
        code >= 10_000_000 && code <= 99_999_999
    }

    /// bo2mp: a code as shown, `1234 5678`.
    pub fn friend_code_text(code: u32) -> String {
        if Self::is_friend_code(code) {
            format!("{} {:04}", code / 10_000, code % 10_000)
        } else {
            "-".to_owned()
        }
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The option rows the native renderers apply over the preset.
    pub fn native_options(&self) -> native_quality::Options {
        native_quality::Options {
            shadows: self.shadows,
            bloom: self.bloom,
            depth_of_field: self.depth_of_field,
            motion_blur: self.motion_blur,
            texture_performance: self.bo2_fast,
            texture_quality: self.tex_quality,
            texture_filter: self.tex_filter,
            local_lights: self.rt_lighting,
            render_scale: if self.upscaler.active() { self.upscale_quality.scale() } else { 1.0 },
            vsync: self.vsync,
            max_fps: self.max_fps,
        }
    }

    /// Publishes what the native renderers apply now (`native_quality`).
    pub fn publish_native_quality(&self) {
        native_quality::publish(self.graphics_quality, &self.native_options());
    }

    pub fn sanitize(&mut self) {
        self.resolution.width = self.resolution.width.clamp(640, 7680);
        self.resolution.height = self.resolution.height.clamp(480, 4320);
        self.fov = if self.fov.is_finite() {
            self.fov.clamp(Self::FOV_MIN, Self::FOV_MAX)
        } else {
            Self::FOV_DEFAULT
        };
        self.brightness = if self.brightness.is_finite() {
            self.brightness.clamp(-0.2, 0.2)
        } else {
            0.0
        };
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.music_volume = if self.music_volume.is_finite() {
            self.music_volume.clamp(0.0, 1.0)
        } else {
            Self::MUSIC_VOLUME_DEFAULT
        };
        for v in [&mut self.sfx_volume, &mut self.voice_volume, &mut self.cinematic_volume] {
            *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        }
        self.sensitivity = self.sensitivity.clamp(0.1, 30.0);
        if self.pad_layout != Self::PAD_LAYOUT_CUSTOM {
            self.pad_layout = self.pad_layout.min(4);
        }
        self.pad_stick_layout = self.pad_stick_layout.min(3);
        self.pad_curve = self.pad_curve.min(2);
        self.pad_aim_assist = 0;
        self.pad_prompts = self.pad_prompts.min(3);
        let finite = |v: f32, lo: f32, hi: f32, default: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            }
        };
        self.pad_sensitivity = finite(
            self.pad_sensitivity,
            1.0,
            // BO2's Look Sensitivity runs to 14 (Ultra).
            14.0,
            Self::PAD_SENSITIVITY_DEFAULT,
        );
        self.pad_ads_sensitivity = finite(self.pad_ads_sensitivity, 0.5, 1.5, 1.0);
        if ![3, 5, 10, 16].contains(&self.corpses) {
            self.corpses = 16;
        }
        self.max_fps = self.max_fps.min(1000);
        if ![1, 2, 4, 8, 16, 17, 18].contains(&self.aa_samples) {
            self.aa_samples = 1;
        }
        self.tex_filter = self.tex_filter.min(2);
        self.tex_quality = self.tex_quality.clamp(-1, 3);
        self.upscale_sharpness = if self.upscale_sharpness.is_finite() {
            self.upscale_sharpness.clamp(0.0, 1.0)
        } else {
            Self::UPSCALE_SHARPNESS_DEFAULT
        };
        self.pad_deadzone_left = finite(self.pad_deadzone_left, 0.0, 0.4, 0.12);
        self.pad_deadzone_right = finite(self.pad_deadzone_right, 0.0, 0.4, 0.12);
        if !Self::is_friend_code(self.friend_code) {
            self.friend_code = 0;
        }
        let mine = self.friend_code;
        let mut seen = Vec::new();
        self.friends.retain(|&c| Self::is_friend_code(c) && c != mine && !seen.contains(&c) && {
            seen.push(c);
            true
        });
        self.friends.truncate(100);
        self.player_name = self.player_name.trim().chars().take(16).collect();
        if self.player_name.is_empty() {
            self.player_name = "Player".to_owned();
        }
    }
}

#[cfg(test)]
mod friend_tests {
    use super::GameSettings;

    #[test]
    fn friend_codes_are_eight_digits_and_kept_tidy() {
        let mut s = GameSettings { friend_code: 48_213_907, friends: vec![12_345_678, 12_345_678, 48_213_907, 5, 99_999_999], ..GameSettings::default() };
        s.sanitize();
        assert_eq!(s.friends, vec![12_345_678, 99_999_999]);
        assert_eq!(GameSettings::friend_code_text(48_213_907), "4821 3907");
        assert_eq!(GameSettings::friend_code_text(0), "-");
        let mut bad = GameSettings { friend_code: 123, ..GameSettings::default() };
        bad.sanitize();
        assert_eq!(bad.friend_code, 0);
    }
}

impl GameSettings {
    /// The name he plays under (lobby, scoreboard, the game's own
    /// messages): a name his settings give (the family launcher's
    /// profile), else the Steam account this PC signs into.
    pub fn shown_name(&self) -> String {
        if self.player_name == "Player" {
            steam_name().unwrap_or_else(|| self.player_name.clone())
        } else {
            self.player_name.clone()
        }
    }
}

/// The Steam name of the account Steam signs into on this PC (its
/// `config/loginusers.vdf`: the auto-login one, else the newest), as BO2
/// shows the player's Steam name; read once.
pub fn steam_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let root = std::env::var_os("ProgramFiles(x86)")?;
        let path = std::path::Path::new(&root).join("Steam/config/loginusers.vdf");
        let text = std::fs::read_to_string(path).ok()?;
        // Each account is a { } block of "key" "value" lines.
        let mut best: Option<(bool, u64, String)> = None;
        for block in text.split('{').skip(2) {
            let block = block.split('}').next().unwrap_or("");
            let field = |key: &str| {
                block.lines().find_map(|l| {
                    let mut parts = l.split('"').filter(|p| !p.trim().is_empty());
                    (parts.next()? == key).then(|| parts.next().map(str::to_owned)).flatten()
                })
            };
            let Some(name) = field("PersonaName").filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            let auto = field("AutoLogin").as_deref() == Some("1");
            let time = field("Timestamp").and_then(|t| t.parse().ok()).unwrap_or(0);
            if best.as_ref().is_none_or(|b| (auto, time) > (b.0, b.1)) {
                best = Some((auto, time, name));
            }
        }
        best.map(|(_, _, n)| n.trim().chars().take(16).collect())
    })
    .clone()
}

#[cfg(test)]
mod upscaler_tests {
    use super::Upscaler;

    #[test]
    fn every_upscaler_is_offered_and_falls_back() {
        for u in Upscaler::ALL {
            assert_eq!(Upscaler::from_key(u.key()), Some(u));
        }
        assert_eq!(Upscaler::from_key(" FSR3 "), Some(Upscaler::Fsr3));
        assert_eq!(Upscaler::Off.step(1), Upscaler::Fsr1);
        assert_eq!(Upscaler::Fsr1.step(1), Upscaler::Fsr3);
        assert_eq!(Upscaler::Fsr3.step(1), Upscaler::Dlss);
        assert_eq!(Upscaler::Dlss.step(1), Upscaler::Off);
        assert_eq!(Upscaler::Off.step(-1), Upscaler::Dlss);
        assert!(Upscaler::Fsr3.active() && Upscaler::Dlss.active());
        assert!(!Upscaler::Off.active() && !Upscaler::Off.wants_temporal());
        assert!(!Upscaler::Fsr1.wants_temporal());
        assert!(Upscaler::Fsr3.wants_temporal() && Upscaler::Dlss.wants_temporal());
        // (The support flags are process-wide; this is the only test that sets them.)
        Upscaler::set_support(false, false);
        assert_eq!(Upscaler::Dlss.effective(), Upscaler::Fsr1);
        assert_eq!(Upscaler::Fsr3.effective(), Upscaler::Fsr1);
        assert!(Upscaler::Dlss.label().contains("NOT AVAILABLE"));
        Upscaler::set_support(true, false);
        assert_eq!(Upscaler::Dlss.effective(), Upscaler::Fsr3);
        assert_eq!(Upscaler::Fsr3.effective(), Upscaler::Fsr3);
        Upscaler::set_support(true, true);
        assert_eq!(Upscaler::Dlss.effective(), Upscaler::Dlss);
        assert_eq!(Upscaler::Dlss.label(), "DLSS");
        assert_eq!(Upscaler::Off.effective(), Upscaler::Off);

        // The safety net (also process-wide, so in this test): a failed
        // FSR 3.1 draws as FSR 1, a failed FSR 1 turns everything off, and
        // `force_off` beats every setting.
        Upscaler::reset_safety();
        Upscaler::set_support(true, false);
        Upscaler::Fsr3.mark_broken();
        assert_eq!(Upscaler::Fsr3.effective(), Upscaler::Fsr1);
        assert_eq!(Upscaler::Dlss.effective(), Upscaler::Fsr1);
        assert!(Upscaler::Fsr3.label().contains("FAILED"));
        Upscaler::Fsr1.mark_broken();
        assert!(Upscaler::forced_off());
        for u in Upscaler::ALL {
            assert_eq!(u.effective(), Upscaler::Off, "{u:?}");
            assert!(!u.effective().active());
        }
        assert!(Upscaler::Fsr1.label().contains("TURNED OFF"));
        assert_eq!(Upscaler::Off.label(), "OFF");
        Upscaler::reset_safety();
        Upscaler::force_off(true);
        assert_eq!(Upscaler::Fsr1.effective(), Upscaler::Off);
        Upscaler::force_off(false);
        assert_eq!(Upscaler::Fsr1.effective(), Upscaler::Fsr1);

        // A render error is blamed on the upscaler that drew, when its text
        // names one of our passes.
        let motion_mismatch = "In a RenderPass: label \"iw4_geometry_diagnostic_pass\": the pipeline's \
            colour target #1 Rg16Float does not match";
        let fsr_pass = "In a ComputePass: label \"FSR3 Luma Pyramid\": bind group is invalid";
        let unrelated = "Texture \"gun_albedo\" is invalid";
        assert_eq!(
            Upscaler::blamed_for_error(Upscaler::Fsr3, motion_mismatch),
            Some(Upscaler::Fsr3)
        );
        assert_eq!(
            Upscaler::blamed_for_error(Upscaler::Fsr3, fsr_pass),
            Some(Upscaler::Fsr3)
        );
        assert_eq!(Upscaler::blamed_for_error(Upscaler::Fsr3, unrelated), None);
        assert_eq!(Upscaler::blamed_for_error(Upscaler::Off, fsr_pass), None);
        Upscaler::Fsr3.note_in_use();
        assert_eq!(Upscaler::in_use(), Upscaler::Fsr3);
        assert_eq!(Upscaler::trip_for_render_error(unrelated), None);
        assert_eq!(Upscaler::trip_for_render_error(unrelated), None);
        assert!(!Upscaler::Fsr3.broken());
        // (The third error of any kind while it draws is enough.)
        assert_eq!(Upscaler::trip_for_render_error(unrelated), Some(Upscaler::Fsr3));
        assert!(Upscaler::Fsr3.broken());
        Upscaler::reset_safety();
        Upscaler::set_support(true, false);
        Upscaler::Fsr3.note_in_use();
        assert_eq!(Upscaler::trip_for_render_error(fsr_pass), Some(Upscaler::Fsr3));
        assert!(Upscaler::Fsr3.broken());
        Upscaler::reset_safety();
        Upscaler::set_support(false, false);
    }
}

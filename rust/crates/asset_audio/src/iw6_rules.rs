//! bo2mp: how Call of Duty: Ghosts plays its sound aliases, for Ghosts maps
//! and Ghosts sounds (`AssetNamespace::Iw6`). Black Ops II's own sounds keep
//! Black Ops II's rules; nothing here touches them.
//!
//! Each rule is a plain function or small state type with its tests; the
//! audio crate calls them behind the `BO2MP_IW6_*` switches
//! ([`Iw6Switches`]). The values come from the alias records
//! `fastfile_iw6` reads (`AliasParams`) and the map's zone.
//!
//! * Alias choice: weighted by each alias's probability with a reservoir
//!   pass over Microsoft's C runtime `rand` (15 bits); in a list of more
//!   than two, the alias played last is not picked again straight away.
//! * Per play: volume `clamp01(lerp(volMin, volMax, r) * scale)`, pitch
//!   `lerp(pitchMin, pitchMax, r)`.
//! * Gating: flag 0x40 drops a sound nearer than its `distMin`; any sound
//!   past its `distMax`, or whose listener moves slower than `velocityMin`,
//!   does not start.
//! * Distance: `t = clamp01((d - distMin) / (distMax - distMin))` through
//!   the alias's volume falloff curve (piecewise linear); the reverb send
//!   takes the reverb-send curve when there is one, else the same value.
//! * Dry/wet: flag 0x08 is full dry; flag 0x10 takes the alias's own wet
//!   level (`wetMixOverride`); otherwise the reverb in force.
//! * Doppler, the six-slot reverb stack, occlusion shapes, whizbys, ambient
//!   elements, surface-named movement aliases and the sound tables' zone
//!   and reverb rows: see each type.

use crate::sound_catalog::CapturedSndCurve;

/// Alias flag bits (the 32-bit word at alias offset 84).
pub mod flags {
    /// Loops (the `_lp` aliases carry it).
    pub const LOOPING: u32 = 0x01;
    /// Full dry level whatever the reverb.
    pub const FULL_DRY: u32 = 0x08;
    /// The alias's own wet level (`wetMixOverride`).
    pub const REVERB_OVERRIDE: u32 = 0x10;
    /// Dropped when the listener is inside `distMin`.
    pub const CULL_INSIDE_MIN: u32 = 0x40;

    /// 1 loaded, 2 or 3 streamed.
    pub fn file_mode(flags: u32) -> u32 {
        (flags >> 7) & 3
    }

    /// The entity channel (index into the channel table).
    pub fn channel(flags: u32) -> u32 {
        (flags >> 9) & 0x7f
    }
}

/// The Ghosts values an alias carries past the shared ones.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Iw6Params {
    pub flags: u32,
    pub velocity_min: f32,
    pub wet_mix_override: f32,
    pub allow_doppler: bool,
    pub master_priority: u8,
    /// The reverb-send curve (name `iw6/curve/<name>`; knots when read).
    pub reverb_send: Option<CapturedSndCurve>,
    /// The low-pass curve's name (how it is applied was not found: kept,
    /// not used).
    pub lpf_curve: Option<String>,
    pub speaker_map: Option<String>,
    pub occlusion: Option<OcclusionShape>,
    pub doppler: Option<DopplerPreset>,
}

/// `BO2MP_IW6_<NAME>`: `1` on, `0` off, unset the default.
pub fn switch(name: &str, default: bool) -> bool {
    std::env::var(format!("BO2MP_IW6_{name}")).map_or(default, |v| match v.trim() {
        "0" => false,
        "1" => true,
        _ => default,
    })
}

/// Every audible Ghosts sound rule's switch, read once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Iw6Switches {
    /// `BO2MP_IW6_MAP_SOUNDS` (on): a Ghosts map's own aliases and ambient bed.
    pub map_sounds: bool,
    /// `BO2MP_IW6_CURVES` (on): each alias's own falloff curve.
    pub curves: bool,
    /// `BO2MP_IW6_GATING` (on): distMin cull flag, distMax, volume clamp.
    pub gating: bool,
    /// `BO2MP_IW6_PICK` (off): Ghosts' alias choice and random numbers.
    pub pick: bool,
    /// `BO2MP_IW6_DOPPLER` (off): Doppler pitch on aliases that allow it.
    pub doppler: bool,
    /// `BO2MP_IW6_REVERB` (off): dry/wet flags and the reverb stack.
    pub reverb: bool,
    /// `BO2MP_IW6_FOOTSTEPS` (off): Ghosts' surface-named movement sounds.
    pub footsteps: bool,
}

impl Iw6Switches {
    pub fn from_env() -> Self {
        Self {
            map_sounds: switch("MAP_SOUNDS", true),
            curves: switch("CURVES", true),
            gating: switch("GATING", true),
            pick: switch("PICK", false),
            doppler: switch("DOPPLER", false),
            reverb: switch("REVERB", false),
            footsteps: switch("FOOTSTEPS", false),
        }
    }

    /// Read once per process.
    pub fn get() -> Self {
        static ONCE: std::sync::OnceLock<Iw6Switches> = std::sync::OnceLock::new();
        *ONCE.get_or_init(Self::from_env)
    }

    pub fn summary(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "map sounds {}, curves {}, gating {}, pick {}, doppler {}, reverb {}, footsteps {}",
            on(self.map_sounds),
            on(self.curves),
            on(self.gating),
            on(self.pick),
            on(self.doppler),
            on(self.reverb),
            on(self.footsteps)
        )
    }
}

/// The catalog key of a Ghosts curve (`,$default` -> `iw6/curve/$default`).
pub fn curve_key(name: &str) -> String {
    format!("iw6/curve/{}", name.trim_start_matches(','))
}

// ---------------------------------------------------------------------------
// Random numbers and alias choice

/// Microsoft's C runtime `rand`: 15 bits from a 32-bit linear
/// congruential state.
pub fn rand15(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    (*seed >> 16) & 0x7FFF
}

/// A number in [0, 1] from the same generator.
pub fn rand01(seed: &mut u32) -> f32 {
    rand15(seed) as f32 / 32767.0
}

/// One reservoir pass over `candidates`: each is taken over the one held
/// when `rand15 * accumulated < probability * 32768`.
fn reservoir(probs: &[f32], candidates: impl Iterator<Item = usize>, seed: &mut u32) -> Option<usize> {
    let mut acc = 0.0f32;
    let mut chosen = None;
    for i in candidates {
        let p = probs[i].max(0.0);
        acc += p;
        if chosen.is_none() || (rand15(seed) as f32) * acc < p * 32768.0 {
            chosen = Some(i);
        }
    }
    chosen
}

/// The alias Ghosts picks from a list (`probs`: each alias's probability;
/// `last`: the one played last, the highest sequence). Lists of more than
/// two never repeat the last one straight away.
pub fn pick_alias(probs: &[f32], last: Option<usize>, seed: &mut u32) -> usize {
    let n = probs.len();
    if n <= 1 {
        return 0;
    }
    let first = reservoir(probs, 0..n, seed).unwrap_or(0);
    match last {
        Some(last) if n > 2 && first == last => {
            reservoir(probs, (0..n).filter(|&i| i != last), seed).unwrap_or(first)
        }
        _ => first,
    }
}

/// The volume one play takes.
pub fn play_volume(vol_min: f32, vol_max: f32, scale: f32, r: f32) -> f32 {
    ((r * (vol_max - vol_min) + vol_min) * scale).clamp(0.0, 1.0)
}

/// The pitch one play takes.
pub fn play_pitch(pitch_min: f32, pitch_max: f32, r: f32) -> f32 {
    r * (pitch_max - pitch_min) + pitch_min
}

/// One play's (volume, pitch) of a Ghosts alias row. With Ghosts' choice on
/// the numbers are its generator's; with gating on the volume is clamped to
/// [0, 1] as Ghosts clamps it. Both off: the shared rule (unit random, an
/// all-zero volume range plays at 1).
pub fn volume_pitch(row: &crate::sound_catalog::CapturedAlias, sw: Iw6Switches, rng: &mut u32) -> (f32, f32) {
    let (rv, rp) = if sw.pick {
        (rand01(rng), rand01(rng))
    } else {
        (crate::unit_random(rng), crate::unit_random(rng))
    };
    let volume = if !sw.pick && row.vol_min == 0.0 && row.vol_max == 0.0 {
        1.0
    } else if sw.gating {
        play_volume(row.vol_min, row.vol_max, 1.0, rv)
    } else {
        crate::lerp_range(row.vol_min, row.vol_max, rv)
    };
    let pitch = if row.pitch_min == 0.0 && row.pitch_max == 0.0 {
        1.0
    } else {
        play_pitch(row.pitch_min, row.pitch_max, rp)
    };
    (volume, pitch)
}

// ---------------------------------------------------------------------------
// Map sound zones (the map's sound tables), shared with the audio crate

type MapZones = Option<(SoundZones, ReverbStack)>;

fn zones_cell() -> &'static std::sync::Mutex<MapZones> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<MapZones>> = std::sync::OnceLock::new();
    CELL.get_or_init(Default::default)
}

/// Sets the loaded Ghosts map's zones and reverbs (none: not a Ghosts map,
/// or no sound tables) and applies the zone the map starts in.
pub fn set_map_zones(zones: Option<SoundZones>, map: &str) {
    let state = zones.map(|z| {
        let mut stack = ReverbStack::default();
        if let Some(start) = z.initial_zone(map).cloned() {
            z.apply(&start, &mut stack, 0);
        }
        (z, stack)
    });
    if let Ok(mut cell) = zones_cell().lock() {
        *cell = state;
    }
}

/// The loaded Ghosts map's zones and reverbs.
pub fn map_zones() -> Option<SoundZones> {
    zones_cell().lock().ok().and_then(|z| z.as_ref().map(|(z, _)| z.clone()))
}

/// The Ghosts map reverb in force (dry, wet) and its preset, once faded in.
pub fn map_reverb() -> Option<(String, f32, f32)> {
    let cell = zones_cell().lock().ok()?;
    let (_, stack) = cell.as_ref()?;
    let preset = stack.preset()?.to_owned();
    let (dry, wet) = stack.levels(u64::MAX);
    Some((preset, dry, wet))
}

/// A string table's cells as rows (`fastfile_iw6`'s row-major cells).
pub fn table_cells(columns: usize, cells: &[String]) -> Vec<Vec<String>> {
    if columns == 0 {
        return Vec::new();
    }
    cells.chunks(columns).map(|r| r.to_vec()).collect()
}

// ---------------------------------------------------------------------------
// Gating and distance

/// Why a sound does not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    InsideMin,
    PastMax,
    TooSlow,
}

/// Whether a positioned sound starts at distance `dist` (inches). The
/// listener's speed is checked only when known.
pub fn play_gate(
    flag_word: u32,
    dist: f32,
    dist_min: f32,
    dist_max: f32,
    velocity_min: f32,
    listener_speed: Option<f32>,
) -> Result<(), Gate> {
    let d2 = dist * dist;
    if flag_word & flags::CULL_INSIDE_MIN != 0 && d2 < dist_min * dist_min {
        return Err(Gate::InsideMin);
    }
    if dist_max > 0.0 && d2 > dist_max * dist_max {
        return Err(Gate::PastMax);
    }
    if let Some(speed) = listener_speed
        && velocity_min > 0.0
        && speed < velocity_min
    {
        return Err(Gate::TooSlow);
    }
    Ok(())
}

/// The distance fraction a curve is read at.
pub fn distance_fraction(dist: f32, dist_min: f32, dist_max: f32) -> f32 {
    if dist_max <= dist_min {
        return if dist <= dist_min { 0.0 } else { 1.0 };
    }
    ((dist - dist_min) / (dist_max - dist_min)).clamp(0.0, 1.0)
}

/// A curve's value at `t` (piecewise linear between knots; before the first
/// knot its value, past the last the last one's; no knots: -1).
pub fn curve_value(knots: &[(f32, f32)], t: f32) -> f32 {
    let Some(&(x0, y0)) = knots.first() else {
        return -1.0;
    };
    if t <= x0 {
        return y0;
    }
    for w in knots.windows(2) {
        let ((xa, ya), (xb, yb)) = (w[0], w[1]);
        if t <= xb && xb > xa {
            return ya + (yb - ya) * (t - xa) / (xb - xa);
        }
    }
    knots.last().map_or(-1.0, |k| k.1)
}

/// The volume and reverb send at distance `dist`.
pub fn falloff(
    volume_curve: &[(f32, f32)],
    reverb_curve: Option<&[(f32, f32)]>,
    dist: f32,
    dist_min: f32,
    dist_max: f32,
) -> (f32, f32) {
    let t = distance_fraction(dist, dist_min, dist_max);
    let volume = curve_value(volume_curve, t).max(0.0);
    let send = match reverb_curve.filter(|k| !k.is_empty()) {
        Some(k) => curve_value(k, t).max(0.0),
        None => volume,
    };
    (volume, send)
}

/// The dry and wet levels of one voice: `reverb` is the reverb in force
/// (dry, wet), `None` when there is none or reverb is off.
pub fn dry_wet(flag_word: u32, wet_mix_override: f32, reverb: Option<(f32, f32)>) -> (f32, f32) {
    let (dry, wet) = reverb.unwrap_or((1.0, 0.0));
    let dry = if flag_word & flags::FULL_DRY != 0 { 1.0 } else { dry };
    let wet = if reverb.is_none() {
        0.0
    } else if flag_word & flags::REVERB_OVERRIDE != 0 {
        wet_mix_override
    } else {
        wet
    };
    (dry, wet)
}

// ---------------------------------------------------------------------------
// Occlusion shape

/// An alias's occlusion shape (directional gain).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OcclusionShape {
    pub is_default: bool,
    pub inner_angle: f32,
    pub outer_angle: f32,
    pub occluded_volume: f32,
}

impl OcclusionShape {
    /// The gain at `angle` degrees between the emitter's forward and the
    /// direction to the listener: 1 inside the inner angle, the occluded
    /// volume past the outer one, linear between (the linear blend is
    /// inferred). A default shape is always 1.
    pub fn gain(&self, angle: f32) -> f32 {
        if self.is_default || angle <= self.inner_angle {
            return 1.0;
        }
        if angle >= self.outer_angle || self.outer_angle <= self.inner_angle {
            return self.occluded_volume;
        }
        let t = (angle - self.inner_angle) / (self.outer_angle - self.inner_angle);
        1.0 + (self.occluded_volume - 1.0) * t
    }
}

// ---------------------------------------------------------------------------
// Doppler

/// A Doppler preset (the dvar defaults unless an alias carries its own).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DopplerPreset {
    pub speed_of_sound: f32,
    pub player_velocity_scale: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub smoothing: f32,
}

impl Default for DopplerPreset {
    fn default() -> Self {
        Self {
            speed_of_sound: 900.0,
            player_velocity_scale: 0.1,
            min_pitch: 0.1,
            max_pitch: 4.0,
            smoothing: 6.0,
        }
    }
}

/// One voice's Doppler state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Doppler {
    pub preset: DopplerPreset,
    pub ratio: f32,
}

impl Doppler {
    pub fn new(preset: DopplerPreset) -> Self {
        Self { preset, ratio: 1.0 }
    }

    /// The raw ratio this frame. `to_source`: listener -> source; the
    /// deltas are how far each moved over `dt` seconds.
    pub fn raw_ratio(&self, to_source: [f32; 3], listener_delta: [f32; 3], source_delta: [f32; 3], dt: f32) -> f32 {
        let p = &self.preset;
        let len = (to_source[0].powi(2) + to_source[1].powi(2) + to_source[2].powi(2)).sqrt();
        if len <= 0.001 || dt <= 0.0 {
            return 1.0;
        }
        let r = [to_source[0] / len, to_source[1] / len, to_source[2] / len];
        let dot = |a: [f32; 3]| r[0] * a[0] + r[1] * a[1] + r[2] * a[2];
        let c = p.speed_of_sound * dt;
        let num = (dot(listener_delta) * p.player_velocity_scale + c).clamp(0.1, (4.0 * c).max(0.1));
        let den = (dot(source_delta) + c).clamp(0.1, (4.0 * c).max(0.1));
        let mut ratio = num / den;
        if ratio < 1.0 && p.max_pitch > 1.0 {
            let t = (1.0 - ratio) / (p.max_pitch - 1.0);
            ratio = t * p.min_pitch + (1.0 - t);
        }
        ratio.clamp(p.min_pitch, p.max_pitch)
    }

    /// Steps the smoothed ratio and returns it.
    pub fn step(&mut self, to_source: [f32; 3], listener_delta: [f32; 3], source_delta: [f32; 3], dt: f32) -> f32 {
        let raw = self.raw_ratio(to_source, listener_delta, source_delta, dt);
        let s = self.preset.smoothing.max(0.0);
        self.ratio = (s * self.ratio + raw) / (s + 1.0);
        self.ratio
    }
}

// ---------------------------------------------------------------------------
// Reverb stack

pub const REVERB_SLOTS: usize = 6;
/// The slot sound zones use.
pub const ZONE_REVERB_SLOT: usize = 1;

#[derive(Clone, Debug, Default, PartialEq)]
struct ReverbSlot {
    preset: String,
    active: bool,
    dry: f32,
    wet: f32,
}

/// The reverb in force: the highest active of six slots, faded linearly
/// from what was in force when it changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReverbStack {
    slots: [ReverbSlot; REVERB_SLOTS],
    from: (f32, f32),
    start_ms: u64,
    fade_ms: u64,
}

impl ReverbStack {
    fn top(&self) -> Option<&ReverbSlot> {
        self.slots.iter().rev().find(|s| s.active)
    }

    fn begin_fade(&mut self, now_ms: u64, fade_ms: u64) {
        self.from = self.levels(now_ms);
        self.start_ms = now_ms;
        self.fade_ms = fade_ms.max(1);
    }

    pub fn set(&mut self, slot: usize, preset: &str, dry: f32, wet: f32, fade_ms: u64, now_ms: u64) {
        let Some(_) = self.slots.get(slot) else { return };
        self.begin_fade(now_ms, fade_ms);
        self.slots[slot] = ReverbSlot { preset: preset.to_owned(), active: true, dry, wet };
    }

    pub fn clear(&mut self, slot: usize, fade_ms: u64, now_ms: u64) {
        if self.slots.get(slot).is_none_or(|s| !s.active) {
            return;
        }
        self.begin_fade(now_ms, fade_ms);
        self.slots[slot].active = false;
    }

    /// The preset in force (none: no slot active).
    pub fn preset(&self) -> Option<&str> {
        self.top().map(|s| s.preset.as_str())
    }

    /// (dry, wet) now; (1, 0) with no slot active.
    pub fn levels(&self, now_ms: u64) -> (f32, f32) {
        let target = self.top().map_or((1.0, 0.0), |s| (s.dry, s.wet));
        if self.fade_ms == 0 {
            return target;
        }
        let t = (now_ms.saturating_sub(self.start_ms) as f32 / self.fade_ms as f32).clamp(0.0, 1.0);
        (
            self.from.0 + (target.0 - self.from.0) * t,
            self.from.1 + (target.1 - self.from.1) * t,
        )
    }
}

// ---------------------------------------------------------------------------
// Sound tables (`soundtables/mp_common_zone.csv`, `..._reverb.csv`)

/// A reverb row: name, preset (room type), dry, wet, fade (ms).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReverbRow {
    pub name: String,
    pub preset: String,
    pub dry: f32,
    pub wet: f32,
    pub fade_ms: u64,
}

/// A zone row: name and the reverb, filter and occlusion row names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneRow {
    pub name: String,
    pub reverb: String,
    pub filter: String,
    pub occlusion: String,
}

/// Table rows capacity per kind.
pub const TABLE_CAPACITY: usize = 256;

fn table_rows<'a>(cells: &'a [Vec<String>]) -> impl Iterator<Item = &'a Vec<String>> {
    // Row 0 is the header; empty or `#` rows are skipped.
    cells
        .iter()
        .skip(1)
        .filter(|r| r.first().is_some_and(|c| !c.is_empty() && !c.starts_with('#')))
        .take(TABLE_CAPACITY)
}

fn cell(row: &[String], i: usize) -> &str {
    row.get(i).map_or("", String::as_str)
}

fn num(s: &str) -> f32 {
    s.trim().parse().unwrap_or(0.0)
}

/// Clips a name to the engine's 64-byte field.
fn name64(s: &str) -> String {
    s.chars().take(63).collect()
}

/// The reverb table's rows (columns 0 name, 2 preset, 3 dry, 4 wet, 5 fade
/// seconds).
pub fn parse_reverb_rows(cells: &[Vec<String>]) -> Vec<ReverbRow> {
    table_rows(cells)
        .map(|r| ReverbRow {
            name: name64(cell(r, 0)),
            preset: name64(cell(r, 2)),
            dry: num(cell(r, 3)),
            wet: num(cell(r, 4)),
            fade_ms: (num(cell(r, 5)) * 1000.0).max(0.0) as u64,
        })
        .collect()
}

/// The zone table's rows (columns 0 name, 2 reverb, 3 filter, 4 occlusion).
/// A row with no reverb ends the table.
pub fn parse_zone_rows(cells: &[Vec<String>]) -> Vec<ZoneRow> {
    table_rows(cells)
        .map_while(|r| {
            let reverb = cell(r, 2);
            (!reverb.is_empty()).then(|| ZoneRow {
                name: name64(cell(r, 0)),
                reverb: name64(reverb),
                filter: name64(cell(r, 3)),
                occlusion: name64(cell(r, 4)),
            })
        })
        .collect()
}

/// The `zone_name` section of a row-typed table (`soundtables/<map>.csv`,
/// `soundtables/common_mp.csv`): column 0 the row type or the zone's name;
/// the section runs from its `zone_name` header to the next `*_name`
/// header (or `END_OF_FILE`). Columns: name, ambience, ambient, mix, reverb,
/// filter, occlusion (a zone with no reverb is kept: it clears the slot).
pub fn parse_typed_zone_rows(cells: &[Vec<String>]) -> Vec<ZoneRow> {
    let mut out = Vec::new();
    let mut inside = false;
    for r in cells {
        let c0 = cell(r, 0);
        if c0 == "zone_name" {
            inside = true;
            continue;
        }
        if c0.ends_with("_name") || c0 == "ambient_event" || c0 == "ambient_element" || c0 == "END_OF_FILE" {
            inside = false;
            continue;
        }
        if inside && !c0.is_empty() && !c0.starts_with('#') && out.len() < TABLE_CAPACITY {
            out.push(ZoneRow {
                name: name64(c0),
                reverb: name64(cell(r, 4)),
                filter: name64(cell(r, 5)),
                occlusion: name64(cell(r, 6)),
            });
        }
    }
    out
}

/// A map's zones and reverbs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundZones {
    pub zones: Vec<ZoneRow>,
    pub reverbs: Vec<ReverbRow>,
}

impl SoundZones {
    /// The zone a map starts in: the one named as the map, else `default`.
    pub fn initial_zone(&self, map: &str) -> Option<&ZoneRow> {
        self.zones
            .iter()
            .find(|z| z.name.eq_ignore_ascii_case(map))
            .or_else(|| self.zones.iter().find(|z| z.name.eq_ignore_ascii_case("default")))
    }

    /// A zone's reverb row (unknown names take `default`).
    pub fn reverb_of(&self, zone: &ZoneRow) -> Option<&ReverbRow> {
        self.reverbs
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(&zone.reverb))
            .or_else(|| self.reverbs.iter().find(|r| r.name.eq_ignore_ascii_case("default")))
    }

    /// Applies a zone to the stack: its reverb on the zone slot, or the
    /// slot cleared over 1000 ms when it has none.
    pub fn apply(&self, zone: &ZoneRow, stack: &mut ReverbStack, now_ms: u64) {
        match self.reverb_of(zone) {
            Some(r) => stack.set(ZONE_REVERB_SLOT, &r.preset, r.dry, r.wet, r.fade_ms, now_ms),
            None => stack.clear(ZONE_REVERB_SLOT, 1000, now_ms),
        }
    }
}

// ---------------------------------------------------------------------------
// Whizby

/// How near a bullet passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhizbyBand {
    Near,
    Medium,
    Far,
}

impl WhizbyBand {
    pub fn word(self) -> &'static str {
        match self {
            Self::Near => "near",
            Self::Medium => "medium",
            Self::Far => "far",
        }
    }
}

/// The whizby band edges (inches).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhizbyDistances {
    pub near: f32,
    pub medium: f32,
    pub far: f32,
}

impl Default for WhizbyDistances {
    fn default() -> Self {
        Self { near: 40.0, medium: 80.0, far: 160.0 }
    }
}

/// A bullet pass-by: its band, the two emitters (left, right) and the
/// perpendicular distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Whizby {
    pub band: WhizbyBand,
    pub left: [f32; 3],
    pub right: [f32; 3],
    pub dist: f32,
}

/// The pass-by of a bullet from `start` to `end` heard at `ear`, if near
/// enough. The closest point is searched from 64 inches along the shot.
pub fn whizby(start: [f32; 3], end: [f32; 3], ear: [f32; 3], d: WhizbyDistances) -> Option<Whizby> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let seg = sub(end, start);
    let length = dot(seg, seg).sqrt();
    if length < 64.0 {
        return None;
    }
    let dir = [seg[0] / length, seg[1] / length, seg[2] / length];
    let t = dot(sub(ear, start), dir).clamp(64.0, length);
    let closest = [start[0] + dir[0] * t, start[1] + dir[1] * t, start[2] + dir[2] * t];
    let off = sub(ear, closest);
    let dist = dot(off, off).sqrt().max(1.0);
    let (band, width) = if dist < d.near {
        (WhizbyBand::Near, 400.0 + 200.0 * (dist / d.near))
    } else if dist < d.medium {
        (WhizbyBand::Medium, 600.0 + 400.0 * ((dist - d.near) / (d.medium - d.near)))
    } else if dist <= d.far {
        (WhizbyBand::Far, 1000.0)
    } else {
        return None;
    };
    let h = width / 2.0;
    let at = |s: f32| [closest[0] + dir[0] * s, closest[1] + dir[1] * s, closest[2] + dir[2] * s];
    Some(Whizby { band, left: at(-h), right: at(h), dist })
}

/// The two aliases of a pass-by's variation `index` (0..15), then the
/// fallback `whizby`.
pub fn whizby_aliases(band: WhizbyBand, index: u32) -> [String; 3] {
    let b = band.word();
    [
        format!("whizby_{b}_{index:02}_L"),
        format!("whizby_{b}_{index:02}_R"),
        "whizby".to_owned(),
    ]
}

/// A shuffled bag of variation indices: every index once before any again.
#[derive(Clone, Debug, Default)]
pub struct ShuffleBag {
    left: Vec<u32>,
}

impl ShuffleBag {
    pub fn next(&mut self, count: u32, seed: &mut u32) -> u32 {
        if count == 0 {
            return 0;
        }
        if self.left.is_empty() {
            self.left = (0..count).collect();
            for i in (1..self.left.len()).rev() {
                let j = rand15(seed) as usize % (i + 1);
                self.left.swap(i, j);
            }
        }
        self.left.pop().unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Ambient elements

/// An ambient element: alias, distance range, azimuth range (degrees) and
/// the time between plays (ms).
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientElement {
    pub alias: String,
    pub dist: (f32, f32),
    pub azimuth: (f32, f32),
    pub time_ms: (u32, u32),
}

impl AmbientElement {
    /// Where the next play is (around `ear`) and how long until the one
    /// after.
    pub fn next_play(&self, ear: [f32; 3], seed: &mut u32) -> ([f32; 3], u32) {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let dist = lerp(self.dist.0, self.dist.1, rand01(seed));
        let az = lerp(self.azimuth.0, self.azimuth.1, rand01(seed)).to_radians();
        let wait = lerp(self.time_ms.0 as f32, self.time_ms.1 as f32, rand01(seed)).max(0.0) as u32;
        ([ear[0] + az.cos() * dist, ear[1] + az.sin() * dist, ear[2]], wait)
    }
}

// ---------------------------------------------------------------------------
// Movement and impacts by surface

/// The surface-named alias and its `_default` fallback
/// (`step_run_plr` + `concrete`).
pub fn surface_aliases(base: &str, surface: &str) -> [String; 2] {
    [format!("{base}_{surface}"), format!("{base}_default")]
}

/// The Ghosts movement alias an IW-family surface alias stands for: the
/// same `<base>_<surface>` names, lower case (`Land_` is `land_`).
pub fn ghosts_movement_alias(alias: &str) -> Option<String> {
    let lower = alias.to_ascii_lowercase();
    ["step_", "land_", "slide_"]
        .iter()
        .any(|p| lower.starts_with(p))
        .then_some(lower)
}

/// Ghosts' bullet impact sound of a hit: `bullet_<size>[_exit]_<surface>`
/// and its `_default` fallback (B8: `bullet_small/large/ap`, `+_exit`), by
/// the gun's impact type (Ghosts' numbering is IW4's: 1 small, 2 large, 3
/// armour piercing, 4 explosive; a shotgun's 5 and 6 take the small ones,
/// INFERRED) and the surface's Ghosts name (`t6_surface`: Black Ops II's
/// list; its `player` is flesh, `tallgrass` grass).
pub fn bullet_impact_aliases(impact_type: i32, exit: bool, t6_surface: &str) -> Option<[String; 2]> {
    let size = match impact_type {
        1 | 5 | 6 => "bullet_small",
        2 => "bullet_large",
        3 => "bullet_ap",
        4 => "bullet_explode",
        _ => return None,
    };
    let surface = match t6_surface {
        "player" => "flesh",
        "tallgrass" => "grass",
        s => s,
    };
    // (Explosive bullets have no exit sound of their own.)
    let exit = if exit && impact_type != 4 { "_exit" } else { "" };
    Some([format!("{size}{exit}_{surface}"), format!("{size}{exit}_default")])
}

/// The footstep volume scales (player, ally, enemy).
pub const FOOTSTEP_VOLUME: (f32, f32, f32) = (0.25, 0.25, 2.0);

#[cfg(test)]
mod typed_table_tests {
    use super::*;

    fn rows(t: &[&[&str]]) -> Vec<Vec<String>> {
        t.iter().map(|r| r.iter().map(|c| (*c).to_owned()).collect()).collect()
    }

    #[test]
    fn a_map_table_s_zone_section_reads_and_starts_the_map_s_zone() {
        // (As retail `soundtables/mp_frag.csv` opens.)
        let map = rows(&[
            &["zone_name", "ambience", "ambient_name", "mix", "reverb", "filter", "occlusion", "player_adsr"],
            &["mp_frag", "ambient_mp_frag", "", "", "mp_lonestar_ext", "default", "default", ""],
            &["frag_lg_room", "ambient_mp_frag", "", "", "lg_room", "interior", "interior", "large_spaces"],
            &["filter_name", "channel", "", "", "", "", "", ""],
            &["interior", "ambient", "0", "highshelf", "", "", "", ""],
            &["END_OF_FILE", "", "", "", "", "", "", ""],
        ]);
        let zones = parse_typed_zone_rows(&map);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[1].reverb, "lg_room");
        let reverbs = parse_reverb_rows(&rows(&[
            &["reverb_names", "comments", "roomtype", "drylevel", "wetlevel", "fadetime"],
            &["mp_lonestar_ext", "", "city", "0.9", "0.2", "1"],
        ]));
        let z = SoundZones { zones, reverbs };
        let start = z.initial_zone("mp_frag").unwrap();
        assert_eq!(z.reverb_of(start).unwrap().wet, 0.2);
    }
}

#[cfg(test)]
mod bullet_impact_tests {
    use super::*;

    #[test]
    fn impact_sounds_take_size_exit_and_surface() {
        assert_eq!(bullet_impact_aliases(1, false, "concrete").unwrap(), ["bullet_small_concrete".to_owned(), "bullet_small_default".to_owned()]);
        assert_eq!(bullet_impact_aliases(2, true, "metal").unwrap()[0], "bullet_large_exit_metal");
        assert_eq!(bullet_impact_aliases(3, false, "player").unwrap()[0], "bullet_ap_flesh");
        assert_eq!(bullet_impact_aliases(5, false, "tallgrass").unwrap()[0], "bullet_small_grass");
        assert!(bullet_impact_aliases(0, false, "wood").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_is_msvc() {
        let mut s = 1u32;
        // MSVC srand(1): 41, 18467, 6334.
        assert_eq!([rand15(&mut s), rand15(&mut s), rand15(&mut s)], [41, 18467, 6334]);
    }

    #[test]
    fn pick_weights_and_never_repeats_in_long_lists() {
        let mut seed = 7;
        let probs = [1.0, 1.0, 1.0];
        for _ in 0..200 {
            let i = pick_alias(&probs, Some(1), &mut seed);
            assert_ne!(i, 1);
        }
        // Two aliases may repeat.
        let mut repeats = 0;
        for _ in 0..200 {
            repeats += usize::from(pick_alias(&[1.0, 1.0], Some(0), &mut seed) == 0);
        }
        assert!(repeats > 50, "{repeats}");
        // Weighted: 3:1.
        let mut counts = [0; 2];
        for _ in 0..4000 {
            counts[pick_alias(&[3.0, 1.0], None, &mut seed)] += 1;
        }
        let share = counts[0] as f32 / 4000.0;
        assert!((share - 0.75).abs() < 0.04, "{share}");
        assert_eq!(pick_alias(&[0.5], Some(0), &mut seed), 0);
    }

    #[test]
    fn volume_and_pitch() {
        assert_eq!(play_volume(0.2, 0.4, 1.0, 0.5), 0.3);
        assert_eq!(play_volume(0.8, 1.2, 2.0, 1.0), 1.0);
        assert!((play_pitch(0.85, 1.05, 0.5) - 0.95).abs() < 1e-6);
    }

    #[test]
    fn gate() {
        assert_eq!(play_gate(0x40, 10.0, 50.0, 1000.0, 0.0, None), Err(Gate::InsideMin));
        assert_eq!(play_gate(0, 10.0, 50.0, 1000.0, 0.0, None), Ok(()));
        assert_eq!(play_gate(0, 1001.0, 50.0, 1000.0, 0.0, None), Err(Gate::PastMax));
        assert_eq!(play_gate(0, 100.0, 50.0, 1000.0, 10.0, Some(5.0)), Err(Gate::TooSlow));
        assert_eq!(play_gate(0, 100.0, 50.0, 1000.0, 10.0, None), Ok(()));
    }

    #[test]
    fn curves() {
        // mp_prisonbreak's own `prisonbreak_radio` (common_core_mp).
        let radio = [(0.0, 1.0), (0.0641, 0.9372), (0.2019, 0.3194), (0.4893, 0.0), (1.0, 0.0)];
        assert_eq!(curve_value(&radio, 0.0), 1.0);
        assert!((curve_value(&radio, 0.0641) - 0.9372).abs() < 1e-6);
        assert_eq!(curve_value(&radio, 0.7), 0.0);
        assert_eq!(curve_value(&[], 0.5), -1.0);
        let lin = [(0.0, 1.0), (1.0, 0.0)];
        assert_eq!(falloff(&lin, None, 550.0, 100.0, 1000.0), (0.5, 0.5));
        assert_eq!(falloff(&lin, Some(&[(0.0, 0.2), (1.0, 0.2)]), 50.0, 100.0, 1000.0), (1.0, 0.2));
        assert_eq!(distance_fraction(5.0, 10.0, 10.0), 0.0);
    }

    #[test]
    fn dry_and_wet() {
        assert_eq!(dry_wet(0, 0.0, Some((0.6, 0.4))), (0.6, 0.4));
        assert_eq!(dry_wet(flags::FULL_DRY, 0.0, Some((0.6, 0.4))), (1.0, 0.4));
        // mp_frag's footsteps: flag 0x10, wetMixOverride 1.1554.
        assert_eq!(dry_wet(0x5490, 1.1554, Some((0.6, 0.4))), (0.6, 1.1554));
        assert_eq!(dry_wet(0x5490, 1.1554, None), (1.0, 0.0));
        assert_eq!(flags::file_mode(0x5490), 1);
        assert_eq!(flags::channel(0x5490), 42);
        assert_eq!(flags::file_mode(0x9539), 2);
    }

    #[test]
    fn occlusion_shape() {
        let s = OcclusionShape { is_default: false, inner_angle: 30.0, outer_angle: 90.0, occluded_volume: 0.4 };
        assert_eq!(s.gain(10.0), 1.0);
        assert!((s.gain(60.0) - 0.7).abs() < 1e-6);
        assert_eq!(s.gain(120.0), 0.4);
        assert_eq!(OcclusionShape { is_default: true, ..s }.gain(120.0), 1.0);
    }

    #[test]
    fn doppler() {
        let d = Doppler::new(DopplerPreset::default());
        // Still: 1.
        assert_eq!(d.raw_ratio([100.0, 0.0, 0.0], [0.0; 3], [0.0; 3], 0.05), 1.0);
        // Source closing at 450 in/s (half the speed of sound): 45 / (45 - 22.5) = 2.
        let up = d.raw_ratio([100.0, 0.0, 0.0], [0.0; 3], [-22.5, 0.0, 0.0], 0.05);
        assert!((up - 2.0).abs() < 1e-4, "{up}");
        // Moving away: below 1, remapped toward the min pitch.
        let down = d.raw_ratio([100.0, 0.0, 0.0], [0.0; 3], [22.5, 0.0, 0.0], 0.05);
        assert!(down < 1.0 && down >= 0.1, "{down}");
        assert_eq!(d.raw_ratio([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0], 0.05), 1.0);
        // Smoothing 6: one step moves 1/7 of the way.
        let mut s = Doppler::new(DopplerPreset::default());
        let r = s.step([100.0, 0.0, 0.0], [0.0; 3], [-22.5, 0.0, 0.0], 0.05);
        assert!((r - (6.0 + 2.0) / 7.0).abs() < 1e-4, "{r}");
    }

    #[test]
    fn reverb_stack() {
        let mut s = ReverbStack::default();
        assert_eq!(s.levels(0), (1.0, 0.0));
        s.set(1, "hangar", 0.8, 0.5, 1000, 0);
        assert_eq!(s.levels(500), (0.9, 0.25));
        assert_eq!(s.levels(2000), (0.8, 0.5));
        s.set(3, "shock", 1.0, 0.5, 250, 2000);
        assert_eq!(s.preset(), Some("shock"));
        s.clear(3, 1000, 3000);
        assert_eq!(s.preset(), Some("hangar"));
        assert_eq!(s.levels(4000), (0.8, 0.5));
        s.clear(1, 1000, 4000);
        assert_eq!(s.preset(), None);
        assert_eq!(s.levels(6000), (1.0, 0.0));
    }

    fn rows(t: &[&[&str]]) -> Vec<Vec<String>> {
        t.iter().map(|r| r.iter().map(|c| (*c).to_owned()).collect()).collect()
    }

    #[test]
    fn sound_tables() {
        let reverbs = parse_reverb_rows(&rows(&[
            &["name", "x", "room", "dry", "wet", "fade"],
            &["default", "", "generic", "1", "0.2", "1"],
            &["#note", "", "", "", "", ""],
            &["hangar", "", "hangar", "0.8", "0.5", "0.5"],
        ]));
        assert_eq!(reverbs.len(), 2);
        assert_eq!(reverbs[1], ReverbRow { name: "hangar".into(), preset: "hangar".into(), dry: 0.8, wet: 0.5, fade_ms: 500 });
        let zones = parse_zone_rows(&rows(&[
            &["name", "x", "reverb", "filter", "occlusion"],
            &["mp_frag", "", "hangar", "f", "o"],
            &["default", "", "nosuch", "", ""],
            &["stop", "", "", "", ""],
            &["after", "", "hangar", "", ""],
        ]));
        assert_eq!(zones.len(), 2);
        let z = SoundZones { zones, reverbs };
        let mut stack = ReverbStack::default();
        let start = z.initial_zone("MP_FRAG").unwrap();
        z.apply(start, &mut stack, 0);
        assert_eq!(stack.preset(), Some("hangar"));
        let other = z.initial_zone("mp_x").unwrap();
        assert_eq!(z.reverb_of(other).unwrap().name, "default");
    }

    #[test]
    fn whizby_bands() {
        let d = WhizbyDistances::default();
        let w = whizby([0.0; 3], [1000.0, 0.0, 0.0], [500.0, 20.0, 0.0], d).unwrap();
        assert_eq!(w.band, WhizbyBand::Near);
        assert_eq!(w.dist, 20.0);
        // Width 400 -> 600 over the near band: 500 at 20 inches.
        assert_eq!((w.left[0], w.right[0]), (250.0, 750.0));
        let w = whizby([0.0; 3], [1000.0, 0.0, 0.0], [500.0, 60.0, 0.0], d).unwrap();
        assert_eq!((w.band, w.right[0] - w.left[0]), (WhizbyBand::Medium, 800.0));
        assert_eq!(whizby([0.0; 3], [1000.0, 0.0, 0.0], [500.0, 160.0, 0.0], d).unwrap().band, WhizbyBand::Far);
        assert!(whizby([0.0; 3], [1000.0, 0.0, 0.0], [500.0, 161.0, 0.0], d).is_none());
        // Behind the first 64 inches: measured from 64 along.
        let w = whizby([0.0; 3], [1000.0, 0.0, 0.0], [0.0, 10.0, 0.0], d).unwrap();
        assert!((w.dist - (64.0f32 * 64.0 + 100.0).sqrt()).abs() < 1e-3);
        assert_eq!(whizby_aliases(WhizbyBand::Medium, 3)[1], "whizby_medium_03_R");
        let mut bag = ShuffleBag::default();
        let mut seed = 3;
        let mut seen: Vec<u32> = (0..4).map(|_| bag.next(4, &mut seed)).collect();
        seen.sort();
        assert_eq!(seen, [0, 1, 2, 3]);
    }

    #[test]
    fn ambient_elements() {
        let e = AmbientElement { alias: "a".into(), dist: (100.0, 100.0), azimuth: (90.0, 90.0), time_ms: (2000, 2000) };
        let (p, wait) = e.next_play([10.0, 0.0, 5.0], &mut 1);
        assert!((p[0] - 10.0).abs() < 1e-3 && (p[1] - 100.0).abs() < 1e-3 && p[2] == 5.0);
        assert_eq!(wait, 2000);
    }

    #[test]
    fn movement_names() {
        assert_eq!(surface_aliases("step_run_plr", "concrete"), ["step_run_plr_concrete".to_owned(), "step_run_plr_default".to_owned()]);
        assert_eq!(ghosts_movement_alias("Land_metal").as_deref(), Some("land_metal"));
        assert_eq!(ghosts_movement_alias("step_run_plr_wood").as_deref(), Some("step_run_plr_wood"));
        assert_eq!(ghosts_movement_alias("weap_fire"), None);
        assert_eq!(curve_key(",$default"), "iw6/curve/$default");
    }
}

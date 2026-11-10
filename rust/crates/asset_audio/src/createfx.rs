use std::collections::HashMap;

use bevy::prelude::Resource;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateFxLoopSound {
    pub soundalias: String,

    pub origin_inches: [f32; 3],
    /// bo2zm: a line emitter's other end (a Black Ops II map's wind and
    /// power lines): the loop is heard from the line's point nearest the
    /// listener.
    pub line_end_inches: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateFxOneshot {
    pub fxid: String,

    pub origin_inches: [f32; 3],

    pub angles_deg: [f32; 3],

    pub delay: f32,

    /// bo2mp: seconds between plays after the first; `0.0` plays once (every
    /// lane but Ghosts', whose placements repeat: see `ghosts_repeat_secs`).
    pub repeat_secs: f32,
}

/// bo2mp: Ghosts' createfx default delay (`-15`, what a placement holds when
/// its map never set one): the repeat time of a one-shot or loop effect with
/// no delay of its own, in seconds.
pub const GHOSTS_CREATEFX_DEFAULT_DELAY_SECS: f32 = 15.0;

/// bo2mp: the repeat interval of a Ghosts createfx placement from its stored
/// `delay`: the delay itself when positive, else (unset, zero or negative,
/// the `-15` default) the engine's default. Never below a tenth of a second.
pub fn ghosts_repeat_secs(delay: f32) -> f32 {
    if delay.is_finite() && delay > 0.0 {
        delay.max(0.1)
    } else {
        GHOSTS_CREATEFX_DEFAULT_DELAY_SECS
    }
}

/// bo2mp: when each repeating placement plays next (`CreateFxOneshot::
/// repeat_secs > 0`); the first play is the boot's, so the first repeat is one
/// interval after it. Times are the effect system's wrapping milliseconds.
#[derive(Clone, Debug, Default, Resource)]
pub struct CreateFxRepeats {
    /// Per placement: its index in the emitter list, interval, next play.
    rows: Vec<(usize, i32, i32)>,
}

impl CreateFxRepeats {
    pub fn new(shots: &[CreateFxOneshot], boot_msec: i32) -> Self {
        let rows = shots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.repeat_secs > 0.0)
            .map(|(i, s)| {
                let ms = ((s.repeat_secs * 1000.0) as i32).max(1);
                (i, ms, boot_msec.wrapping_add(ms))
            })
            .collect();
        Self { rows }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The placements due at `now_msec` (one play each, however long the
    /// gap: a stall does not play a backlog), as `(emitter index, play
    /// time)`; their next play moves past `now_msec` on their own grid.
    pub fn due(&mut self, now_msec: i32) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        for (index, every, next) in &mut self.rows {
            let late = now_msec.wrapping_sub(*next);
            if late < 0 {
                continue;
            }
            out.push((*index, *next));
            let steps = late / *every + 1;
            *next = next.wrapping_add(every.saturating_mul(steps));
        }
        out
    }
}

#[derive(Clone, Debug, Default, Resource)]
pub struct CreateFxOneshotEmitters(pub Vec<CreateFxOneshot>);

/// bo2zm: a sound a map plays now and then at one of a few places.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RandomPointSound {
    pub soundalias: String,
    /// Seconds between plays: [min, max].
    pub wait_secs: [f32; 2],
    pub places_inches: Vec<[f32; 3]>,
}

/// bo2zm: what a Black Ops II map's compiled scripts place: its effects,
/// the loops on them and at points, and its now-and-then sounds (read by
/// the asset lane; the map has no source scripts to parse).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScriptedMapFx {
    pub oneshots: Vec<CreateFxOneshot>,
    pub loop_sounds: Vec<CreateFxLoopSound>,
    pub random_sounds: Vec<RandomPointSound>,
    /// bo2zm: the map's ambient rooms and their echoes
    /// (`setambientroomreverb(room, preset, dry, wet)`): room, preset, dry,
    /// wet. The default room (`declareambientroom(room, true)`) is first.
    pub rooms: Vec<(String, String, f32, f32)>,
    /// bo2mp: each room's tone (a Black Ops III map's rooms carry their
    /// own loop: room, alias); empty for Black Ops II maps (one tone for
    /// the whole map, the map script's `ambientplay`).
    pub room_tones: Vec<(String, String)>,
    /// bo2mp: rooms the listener's place chooses (an Infinite Warfare map's
    /// audio client triggers), tried in order; outside them all the default
    /// room (the first of `rooms`) plays.
    pub room_volumes: Vec<RoomVolume>,
}

/// bo2mp: a room's volume: inside any of its hulls.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoomVolume {
    pub room: String,
    pub hulls: Vec<RoomHull>,
}

/// A trigger hull (`TriggerHull`): a box (middle, half size) cut by slabs
/// (`TriggerSlab`: a direction, the slab's middle along it from the box's
/// middle, its half thickness). Read on IW7's maps: with the slab's middle
/// taken from the box's middle every hull of geneva's (19) and 10 of riot's
/// 11 hold their own middle; taken from the origin, 2 and 1.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoomHull {
    pub mid: [f32; 3],
    pub half: [f32; 3],
    pub slabs: Vec<([f32; 3], f32, f32)>,
}

impl RoomHull {
    pub fn contains(&self, p: [f32; 3]) -> bool {
        (0..3).all(|k| (p[k] - self.mid[k]).abs() <= self.half[k])
            && self.slabs.iter().all(|(d, m, h)| {
                let q = [p[0] - self.mid[0], p[1] - self.mid[1], p[2] - self.mid[2]];
                (d[0] * q[0] + d[1] * q[1] + d[2] * q[2] - m).abs() <= *h
            })
    }
}

impl ScriptedMapFx {
    /// The room whose volume holds `p` (inches), if any.
    pub fn room_at(&self, p: [f32; 3]) -> Option<&str> {
        self.room_volumes.iter().find(|v| v.hulls.iter().any(|h| h.contains(p))).map(|v| v.room.as_str())
    }
}

pub fn parse_createfx_effect_aliases(source: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in source.lines() {
        let Some((alias, path)) = parse_loadfx_alias_line(line.trim()) else {
            continue;
        };
        if !alias.is_empty() && !path.is_empty() {
            out.insert(alias, path);
        }
    }
    out
}

pub fn apply_createfx_effect_aliases(
    shots: &mut [CreateFxOneshot],
    aliases: &HashMap<String, String>,
) {
    for shot in shots {
        if let Some(path) = aliases.get(&shot.fxid) {
            shot.fxid = path.clone();
        }
    }
}

pub fn parse_createfx_loop_sounds(source: &str) -> Vec<CreateFxLoopSound> {
    let mut out = Vec::new();
    let mut pending_origin: Option<[f32; 3]> = None;
    let mut pending_alias: Option<String> = None;
    let mut in_loop = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("createLoopSound") {
            flush_loop(&mut out, &mut pending_origin, &mut pending_alias, in_loop);
            in_loop = true;
            pending_origin = None;
            pending_alias = None;
            continue;
        }
        if !in_loop {
            continue;
        }
        if trimmed.contains("createOneshotEffect")
            || trimmed.contains("createExploder")
            || trimmed.contains("createIntervalSound")
        {
            flush_loop(&mut out, &mut pending_origin, &mut pending_alias, in_loop);
            in_loop = false;
            continue;
        }
        if let Some(origin) = parse_origin_line(trimmed) {
            pending_origin = Some(origin);
        }
        if let Some(alias) = parse_soundalias_line(trimmed) {
            pending_alias = Some(alias);
        }
    }
    flush_loop(&mut out, &mut pending_origin, &mut pending_alias, in_loop);
    out
}

pub fn parse_createfx_oneshots(source: &str) -> Vec<CreateFxOneshot> {
    parse_createfx_effects(source, false)
}

/// bo2mp: the createfx effects, the loop effects too (`createLoopEffect`,
/// started at once: World at War's fires and smoke are loop effects).
pub fn parse_createfx_effects_and_loops(source: &str) -> Vec<CreateFxOneshot> {
    parse_createfx_effects(source, true)
}

fn parse_createfx_effects(source: &str, loops: bool) -> Vec<CreateFxOneshot> {
    let parse_create_oneshot_line = |line: &str| {
        parse_create_oneshot_line(line).or_else(|| loops.then(|| parse_create_line(line, "createLoopEffect")).flatten())
    };
    let mut out = Vec::new();
    let mut pending = PendingOneshot::default();
    let mut in_oneshot = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(fxid) = parse_create_oneshot_line(trimmed) {
            flush_oneshot(&mut out, &mut pending, in_oneshot);
            in_oneshot = true;
            pending = PendingOneshot {
                fxid: Some(fxid),
                ..Default::default()
            };
            continue;
        }
        if !in_oneshot {
            continue;
        }
        if trimmed.contains("createLoopSound")
            || trimmed.contains("createExploder")
            || trimmed.contains("createIntervalSound")
            || trimmed.contains("createOneshotEffect")
            || (loops && trimmed.contains("createLoopEffect"))
        {
            flush_oneshot(&mut out, &mut pending, in_oneshot);
            in_oneshot = false;
            if let Some(fxid) = parse_create_oneshot_line(trimmed) {
                in_oneshot = true;
                pending = PendingOneshot {
                    fxid: Some(fxid),
                    ..Default::default()
                };
            }
            continue;
        }
        if let Some(origin) = parse_origin_line(trimmed) {
            pending.origin = Some(origin);
        }
        if let Some(angles) = parse_angles_line(trimmed) {
            pending.angles = Some(angles);
        }
        if let Some(fxid) = parse_string_field(trimmed, "fxid") {
            pending.fxid = Some(fxid);
        }
        if let Some(delay) = parse_delay_line(trimmed) {
            pending.delay = Some(delay);
        }
    }
    flush_oneshot(&mut out, &mut pending, in_oneshot);
    out
}

#[derive(Default)]
struct PendingOneshot {
    fxid: Option<String>,
    origin: Option<[f32; 3]>,
    angles: Option<[f32; 3]>,
    delay: Option<f32>,
}

fn flush_loop(
    out: &mut Vec<CreateFxLoopSound>,
    origin: &mut Option<[f32; 3]>,
    alias: &mut Option<String>,
    in_loop: bool,
) {
    if !in_loop {
        return;
    }
    if let (Some(origin), Some(soundalias)) = (origin.take(), alias.take()) {
        if !soundalias.is_empty() {
            out.push(CreateFxLoopSound {
                soundalias,
                origin_inches: origin,
                line_end_inches: None,
            });
        }
    } else {
        *origin = None;
        *alias = None;
    }
}

fn flush_oneshot(out: &mut Vec<CreateFxOneshot>, pending: &mut PendingOneshot, in_oneshot: bool) {
    if !in_oneshot {
        return;
    }
    let Some(fxid) = pending.fxid.take() else {
        *pending = PendingOneshot::default();
        return;
    };
    if fxid.is_empty() {
        *pending = PendingOneshot::default();
        return;
    }
    let Some(origin) = pending.origin.take() else {
        *pending = PendingOneshot::default();
        return;
    };
    out.push(CreateFxOneshot {
        fxid,
        origin_inches: origin,
        angles_deg: pending.angles.take().unwrap_or([0.0, 0.0, 0.0]),
        delay: pending.delay.take().unwrap_or(0.0),
        repeat_secs: 0.0,
    });
    *pending = PendingOneshot::default();
}

fn parse_create_oneshot_line(line: &str) -> Option<String> {
    parse_create_line(line, "createOneshotEffect")
}

fn parse_create_line(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let open = rest.find('(')?;
    let rest = rest[open + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn parse_loadfx_alias_line(line: &str) -> Option<(String, String)> {
    let key = "level._effect[";
    let idx = line.find(key)?;
    let rest = line[idx + key.len()..].trim_start();
    let alias = parse_quoted(rest)?;
    let after_alias = rest.find(&format!("\"{alias}\""))? + alias.len() + 2;
    let rest = rest[after_alias..].trim_start();
    let loadfx = "loadfx";
    let load_idx = rest.find(loadfx)?;
    let rest = rest[load_idx + loadfx.len()..].trim_start();
    let open = rest.find('(')?;
    let rest = rest[open + 1..].trim_start();
    let path = parse_quoted(rest)?;
    Some((alias, path))
}

fn parse_soundalias_line(line: &str) -> Option<String> {
    parse_string_field(line, "soundalias")
}

fn parse_string_field(line: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let key_pos = line.find(&key)?;
    let rest = &line[key_pos + key.len()..];
    let first = rest.find('"')?;
    let rest = &rest[first + 1..];
    let second = rest.find('"')?;
    Some(rest[..second].to_owned())
}

fn parse_origin_line(line: &str) -> Option<[f32; 3]> {
    parse_vec3_field(line, "origin")
}

fn parse_angles_line(line: &str) -> Option<[f32; 3]> {
    parse_vec3_field(line, "angles")
}

fn parse_vec3_field(line: &str, field: &str) -> Option<[f32; 3]> {
    let key = format!("\"{field}\"");
    let key_pos = line.find(&key)?;
    let rest = &line[key_pos..];
    let open = rest.find('(')?;
    let rest = &rest[open + 1..];
    let close = rest.find(')')?;
    let body = &rest[..close];
    let mut parts = body.split(',').map(|p| p.trim().parse::<f32>().ok());
    let x = parts.next()??;
    let y = parts.next()??;
    let z = parts.next()??;
    Some([x, y, z])
}

fn parse_delay_line(line: &str) -> Option<f32> {
    let key = "\"delay\"";
    let key_pos = line.find(key)?;
    let rest = &line[key_pos + key.len()..];
    let eq = rest.find('=')?;
    let rest = rest[eq + 1..].trim();
    let end = rest
        .find(|c: char| c == ';' || c.is_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn parse_quoted(s: &str) -> Option<String> {
    let rest = s.trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod loop_effect_tests {
    use super::*;

    const DOME: &str = r#"
     	ent = maps\mp\_utility::createOneshotEffect( "mp_smoke_column_tall" );
     	ent.v[ "origin" ] = ( 907.471, -10815.3, -568.597 );
     	ent.v[ "angles" ] = ( 270, 357.397, 136.603 );
     	ent.v[ "fxid" ] = "mp_smoke_column_tall";
     	ent.v[ "delay" ] = -15;
     	ent = maps\mp\_utility::createLoopEffect( "mp_battlesmoke_large" );
     	ent.v[ "origin" ] = ( -657.383, 845.405, 233.108 );
     	ent.v[ "angles" ] = ( 270, 0, 116 );
     	ent.v[ "fxid" ] = "mp_battlesmoke_large";
     	ent.v[ "delay" ] = 7;
"#;

    #[test]
    fn loop_effects_only_when_asked() {
        assert_eq!(parse_createfx_oneshots(DOME).len(), 1);
        let both = parse_createfx_effects_and_loops(DOME);
        assert_eq!(both.len(), 2);
        assert_eq!(both[1].fxid, "mp_battlesmoke_large");
        assert_eq!(both[1].origin_inches, [-657.383, 845.405, 233.108]);
        assert_eq!(both[0].origin_inches, [907.471, -10815.3, -568.597]);
    }
}

#[cfg(test)]
mod ghosts_repeat_tests {
    use super::*;

    fn shot(repeat_secs: f32) -> CreateFxOneshot {
        CreateFxOneshot {
            fxid: "x".into(),
            origin_inches: [0.0; 3],
            angles_deg: [0.0; 3],
            delay: 0.0,
            repeat_secs,
        }
    }

    #[test]
    fn delay_sets_the_interval_and_unset_takes_the_default() {
        assert_eq!(ghosts_repeat_secs(4.0), 4.0);
        assert_eq!(ghosts_repeat_secs(0.01), 0.1);
        for unset in [-15.0, -1.0, 0.0, f32::NAN] {
            assert_eq!(
                ghosts_repeat_secs(unset),
                GHOSTS_CREATEFX_DEFAULT_DELAY_SECS
            );
        }
    }

    #[test]
    fn repeats_fire_on_their_grid_and_once_only_stay_quiet() {
        let shots = [shot(2.0), shot(0.0), shot(5.0)];
        let mut r = CreateFxRepeats::new(&shots, 1000);
        assert!(r.due(1000).is_empty());
        assert!(r.due(2999).is_empty());
        assert_eq!(r.due(3000), vec![(0, 3000)]);
        assert!(r.due(3001).is_empty());
        assert_eq!(r.due(5000), vec![(0, 5000)]);
        assert_eq!(r.due(6000), vec![(2, 6000)]);
    }

    #[test]
    fn a_stall_plays_once_and_rejoins_the_grid() {
        let mut r = CreateFxRepeats::new(&[shot(2.0)], 0);
        assert_eq!(r.due(11_500), vec![(0, 2000)]);
        assert!(r.due(11_999).is_empty());
        assert_eq!(r.due(12_000), vec![(0, 12_000)]);
    }

    #[test]
    fn wrapping_milliseconds_still_fire() {
        let mut r = CreateFxRepeats::new(&[shot(1.0)], i32::MAX - 500);
        assert!(r.due(i32::MAX - 100).is_empty());
        assert_eq!(r.due(i32::MIN + 600).len(), 1);
    }
}

#[cfg(test)]
mod room_volume_tests {
    use super::{RoomHull, RoomVolume, ScriptedMapFx};

    /// A point is in a room when it is in one of its hulls: inside the box
    /// and between every slab's faces; the first volume holding it wins.
    #[test]
    fn a_room_is_chosen_by_the_box_and_its_slabs() {
        let hall = RoomHull { mid: [0.0, 0.0, 50.0], half: [100.0, 100.0, 50.0], slabs: vec![([0.70710677, 0.70710677, 0.0], 0.0, 50.0)] };
        let small = RoomHull { mid: [10.0, -10.0, 50.0], half: [5.0, 5.0, 50.0], slabs: Vec::new() };
        // (A slab's middle is measured from its box's middle.)
        let off = RoomHull { mid: [500.0, 0.0, 0.0], half: [100.0, 100.0, 100.0], slabs: vec![([1.0, 0.0, 0.0], 50.0, 10.0)] };
        assert!(off.contains([550.0, 0.0, 0.0]) && !off.contains([500.0, 0.0, 0.0]));
        let fx = ScriptedMapFx {
            room_volumes: vec![RoomVolume { room: "closet".into(), hulls: vec![small] }, RoomVolume { room: "hall".into(), hulls: vec![hall] }],
            ..Default::default()
        };
        assert_eq!(fx.room_at([10.0, -10.0, 10.0]), Some("closet"));
        assert_eq!(fx.room_at([20.0, 20.0, 10.0]), Some("hall"));
        // In the box, outside the diagonal slab (x + y over 70.7).
        assert_eq!(fx.room_at([60.0, 60.0, 10.0]), None);
        assert_eq!(fx.room_at([0.0, 0.0, 200.0]), None);
    }
}

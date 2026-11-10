//! bo2mp: a Modern Warfare Remastered map's placed ambient effects (its
//! createfx), read from the map's own source scripts.
//!
//! Unlike Ghosts' compiled scripts, the scripts a Remastered map ships are
//! plain text raw files: `maps/createfx/<map>_fx.gsc` (the placements) and
//! `maps/mp/<map>_fx.gsc` (`level._effect["id"] = loadfx("path")`), in the
//! map's own zone for a Horizon MW usermap and in `patch_<map>.ff` for a
//! retail map (the retail map zone holds only compiled copies).
//!
//! One placement is a statement group:
//!
//! ```text
//! ent = createOneshotEffect( "rain_heavy_mist" );
//! ent set_origin_and_angles( (2661.26, 7.9, 681.9), (270, 0, 0) );   // or
//! ent.v[ "origin" ] = ( 61.5, -251.1, -145.2 );
//! ent.v[ "angles" ] = ( 270, 0, 0 );
//! ent.v[ "fxid" ] = "rain_heavy_mist";
//! ent.v[ "delay" ] = -15;
//! ```

use std::collections::HashMap;

/// How a placement is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Oneshot,
    Loop,
    /// Waits for a script's `exploder(n)`.
    Exploder,
    /// A sound (`createLoopSound`, `createIntervalSound`, ...).
    Sound,
    Other,
}

/// One placement of the script.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub kind: Kind,
    /// The level's effect id (`createOneshotEffect("id")`, else `fxid`).
    pub fxid: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// The createfx delay (negative: the editor's default, -15).
    pub delay: f32,
    /// Set when the placement waits for an exploder number.
    pub exploder: Option<String>,
    /// A sound placement's alias (`ent.v[ "soundalias" ]`); empty for an
    /// effect.
    pub soundalias: String,
    /// An effect placement's own looping sound (`ent.v[ "loopsound" ]`, a
    /// fire's crackle: the createfx editor writes the key, read in the
    /// exe's strings); empty for the rest.
    pub loopsound: String,
    /// A sound placement made by `createIntervalSound` (played now and
    /// then); `createLoopSound`'s plays for good.
    pub interval: bool,
    /// An interval sound's wait range in seconds (`delay_min`,
    /// `delay_max`; INFERRED defaults 1 and 3, the script helper's own are
    /// in a compiled zone), 0 for the rest.
    pub delay_min: f32,
    pub delay_max: f32,
}

/// What a map's createfx scripts hold.
#[derive(Clone, Debug, Default)]
pub struct CreateFx {
    pub placed: Vec<Placement>,
    /// Level effect id -> effect path (`loadfx`).
    pub paths: HashMap<String, String>,
}

impl CreateFx {
    /// The sounds that play for good, as (alias, origin): `createLoopSound`
    /// placements with an alias, and any placement's own `loopsound`.
    pub fn loop_sounds(&self) -> impl Iterator<Item = (&str, [f32; 3])> {
        self.placed.iter().filter_map(|p| {
            let alias = if p.kind == Kind::Sound {
                if p.interval { return None; }
                p.soundalias.as_str()
            } else {
                p.loopsound.as_str()
            };
            (!alias.is_empty()).then_some((alias, p.origin))
        })
    }

    /// The sounds played now and then (`createIntervalSound`).
    pub fn interval_sounds(&self) -> impl Iterator<Item = &Placement> {
        self.placed
            .iter()
            .filter(|p| p.kind == Kind::Sound && p.interval && !p.soundalias.is_empty())
    }

    /// The placements that play at once: effects, not sounds and not
    /// exploders.
    pub fn ambient(&self) -> impl Iterator<Item = &Placement> {
        self.placed
            .iter()
            .filter(|p| matches!(p.kind, Kind::Oneshot | Kind::Loop) && p.exploder.is_none())
    }
}

fn strip_comment(line: &str) -> &str {
    // (A `//` outside a string; the scripts keep paths with single slashes.)
    let mut in_str = false;
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' => in_str = !in_str,
            b'/' if !in_str && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The first quoted string after byte `from`.
fn quoted(line: &str, from: usize) -> Option<&str> {
    let rest = line.get(from..)?;
    let a = rest.find('"')?;
    let rest = &rest[a + 1..];
    let b = rest.find('"')?;
    Some(&rest[..b])
}

/// Every `(x, y, z)` triple of the line, in order.
fn vectors(line: &str) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else { break };
        let parts: Vec<Option<f32>> = after[..close]
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect();
        if let [Some(x), Some(y), Some(z)] = parts[..] {
            out.push([x, y, z]);
            rest = &after[close + 1..];
        } else {
            // (An outer paren around a vector: step inside it.)
            rest = after;
        }
    }
    out
}

fn kind_of(line: &str) -> Option<(Kind, Option<String>)> {
    const KINDS: [(&str, Kind); 7] = [
        ("createOneshotEffect", Kind::Oneshot),
        ("createLoopEffect", Kind::Loop),
        ("createExploder", Kind::Exploder),
        ("createLoopSound", Kind::Sound),
        ("createIntervalSound", Kind::Sound),
        ("createReactiveEnt", Kind::Other),
        ("createNewFx", Kind::Other),
    ];
    for (name, kind) in KINDS {
        if let Some(at) = line.find(name) {
            if !line.contains("ent") || !line.contains('=') {
                continue;
            }
            let id = quoted(line, at + name.len()).map(str::to_owned);
            return Some((kind, id));
        }
    }
    None
}

/// Parses `maps/createfx/<map>_fx.gsc`.
pub fn parse_placements(source: &str) -> Vec<Placement> {
    let mut out: Vec<Placement> = Vec::new();
    let mut cur: Option<Placement> = None;
    let flush = |cur: &mut Option<Placement>, out: &mut Vec<Placement>| {
        if let Some(p) = cur.take() {
            out.push(p);
        }
    };
    for raw in source.lines() {
        let line = strip_comment(raw).trim();
        if let Some((kind, id)) = kind_of(line) {
            flush(&mut cur, &mut out);
            cur = Some(Placement {
                kind,
                fxid: id.unwrap_or_default(),
                origin: [0.0; 3],
                angles: [0.0; 3],
                delay: 0.0,
                exploder: None,
                soundalias: String::new(),
                loopsound: String::new(),
                interval: line.contains("createIntervalSound"),
                delay_min: 0.0,
                delay_max: 0.0,
            });
            if let Some(p) = cur.as_mut() {
                if p.interval {
                    (p.delay_min, p.delay_max) = (1.0, 3.0);
                }
            }
            continue;
        }
        let Some(p) = cur.as_mut() else { continue };
        if line.contains("set_origin_and_angles") {
            let v = vectors(line);
            if let Some(o) = v.first() {
                p.origin = *o;
            }
            if let Some(a) = v.get(1) {
                p.angles = *a;
            }
            continue;
        }
        let Some(key_at) = line.find("ent.v[") else {
            continue;
        };
        let Some(key) = quoted(line, key_at) else {
            continue;
        };
        let value = line
            .split_once('=')
            .map_or("", |(_, v)| v.trim().trim_end_matches(';').trim());
        match key {
            "origin" => {
                if let Some(v) = vectors(value).first() {
                    p.origin = *v;
                }
            }
            "angles" => {
                if let Some(v) = vectors(value).first() {
                    p.angles = *v;
                }
            }
            "fxid" => {
                if let Some(s) = quoted(value, 0) {
                    p.fxid = s.to_owned();
                }
            }
            "delay" => {
                if let Ok(d) = value.parse() {
                    p.delay = d;
                }
            }
            "exploder" => p.exploder = Some(value.trim_matches('"').to_owned()),
            "loopsound" => {
                if let Some(s) = quoted(value, 0) {
                    p.loopsound = s.to_owned();
                }
            }
            "soundalias" => {
                if let Some(s) = quoted(value, 0) {
                    p.soundalias = s.to_owned();
                }
            }
            "delay_min" if p.interval => {
                if let Ok(d) = value.parse() {
                    p.delay_min = d;
                }
            }
            "delay_max" if p.interval => {
                if let Ok(d) = value.parse() {
                    p.delay_max = d;
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Parses `maps/mp/<map>_fx.gsc`'s `level._effect["id"] = loadfx("path")`
/// lines (commented ones skipped).
pub fn parse_effect_paths(source: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in source.lines() {
        let line = strip_comment(raw);
        let Some(at) = line.find("_effect[") else {
            continue;
        };
        let Some(id) = quoted(line, at) else { continue };
        let Some(load) = line.find("loadfx") else {
            continue;
        };
        let Some(path) = quoted(line, load) else {
            continue;
        };
        out.insert(id.to_owned(), path.to_owned());
    }
    out
}

/// Both scripts' text into one [`CreateFx`].
pub fn parse(createfx: &str, effects: &str) -> CreateFx {
    CreateFx {
        placed: parse_placements(createfx),
        paths: parse_effect_paths(effects),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RETAIL: &str = r#"//_createfx generated. Do not touch!!
main()
{
	// CreateFX fx entities placed: 3
	ent = createLoopSound();
	ent set_origin_and_angles( (-916, -100, 123), (270, 0, 0) );
	ent.v[ "soundalias" ] = "emt_rain_metal_car_lp";

	ent = createOneshotEffect( "rain_heavy_mist" );
	ent set_origin_and_angles( (2661.26, 7.93919, 681.971), (270, 0, 0) );
	ent.v[ "fxid" ] = "rain_heavy_mist";
	ent.v[ "delay" ] = -15;

	ent = createOneshotEffect( "lightning" );
	ent set_origin_and_angles( (1, 2, 3), (0, 90, 0) );
	ent.v[ "fxid" ] = "lightning";
	ent.v[ "delay_min" ] = 3;
	ent.v[ "delay" ] = 10;
}"#;

    const USERMAP: &str = r#"
	ent = createOneshotEffect( "sand_spray_detail_runner400x400" );
	ent.v[ "origin" ] = ( -58.8515, 181.543, -236.057 );
	ent.v[ "angles" ] = ( 360, 180, 180 );
	ent.v[ "fxid" ] = "sand_spray_detail_runner400x400";
	ent.v[ "delay" ] = -15;

	ent = maps\mp\_utility::createLoopEffect( "fire" );
	ent.v[ "origin" ] = ( 1, 2, 3 );
	ent.v[ "fxid" ] = "fire";
	ent.v[ "exploder" ] = 5;

	ent = createExploder( "boom" );
	ent.v[ "origin" ] = ( 4, 5, 6 );
"#;

    #[test]
    fn retail_form_with_set_origin_and_angles() {
        let p = parse_placements(RETAIL);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].kind, Kind::Sound);
        assert_eq!(p[1].kind, Kind::Oneshot);
        assert_eq!(p[1].fxid, "rain_heavy_mist");
        assert_eq!(p[1].origin, [2661.26, 7.93919, 681.971]);
        assert_eq!(p[1].angles, [270.0, 0.0, 0.0]);
        assert_eq!(p[1].delay, -15.0);
        assert_eq!(p[2].delay, 10.0);
        assert_eq!(p[2].angles, [0.0, 90.0, 0.0]);
    }

    #[test]
    fn usermap_form_and_exploders() {
        let fx = CreateFx {
            placed: parse_placements(USERMAP),
            paths: HashMap::new(),
        };
        assert_eq!(fx.placed.len(), 3);
        assert_eq!(fx.placed[0].origin, [-58.8515, 181.543, -236.057]);
        assert_eq!(fx.placed[0].angles, [360.0, 180.0, 180.0]);
        assert_eq!(fx.placed[1].kind, Kind::Loop);
        assert_eq!(fx.placed[1].exploder.as_deref(), Some("5"));
        assert_eq!(fx.placed[2].kind, Kind::Exploder);
        let ambient: Vec<_> = fx.ambient().map(|p| p.fxid.as_str()).collect();
        assert_eq!(ambient, ["sand_spray_detail_runner400x400"]);
    }

    #[test]
    fn sounds_keep_alias_kind_and_delays() {
        let src = r#"
	ent = createLoopSound();
	ent set_origin_and_angles( (1, 2, 3), (0, 0, 0) );
	ent.v[ "soundalias" ] = "emt_rain_metal_car_lp";

	ent = createIntervalSound();
	ent.v[ "origin" ] = ( 4, 5, 6 );
	ent.v[ "soundalias" ] = "emt_thunder_dist";
	ent.v[ "delay_min" ] = 8;
	ent.v[ "delay_max" ] = 20.5;

	ent = createIntervalSound();
	ent.v[ "origin" ] = ( 7, 8, 9 );
	ent.v[ "soundalias" ] = "emt_bird";

	ent = createLoopSound();
	ent.v[ "origin" ] = ( 0, 0, 0 );

	ent = createLoopEffect( "fire" );
	ent.v[ "origin" ] = ( 9, 9, 9 );
	ent.v[ "loopsound" ] = "fire_crackle_lp";
"#;
        let fx = CreateFx { placed: parse_placements(src), paths: HashMap::new() };
        let loops: Vec<_> = fx.loop_sounds().collect();
        assert_eq!(loops, [("emt_rain_metal_car_lp", [1.0, 2.0, 3.0]), ("fire_crackle_lp", [9.0, 9.0, 9.0])]);
        let intervals: Vec<_> = fx.interval_sounds().collect();
        assert_eq!(intervals.len(), 2);
        assert_eq!((intervals[0].delay_min, intervals[0].delay_max), (8.0, 20.5));
        assert_eq!((intervals[1].delay_min, intervals[1].delay_max), (1.0, 3.0));
        assert_eq!(fx.ambient().count(), 1);
    }

    #[test]
    fn effect_paths_skip_comments() {
        let src = r#"main()
{
	level._effect[ "wood" ]					 				= loadfx( "fx/explosions/grenadeExp_wood" );
//	level._effect[ "rain_noise" ]		 					= loadfx( "fx/weather/rain_noise" );
	level._effect["screen_rain_mp"]			 				= loadfx("vfx/rain/screen_heavy_rain_cargoship_mp");
	level._effect[ "lightning strikes" ] = loadfx( "vfx/lightning/lightning_strike_runner_cargoship_mp" ); // c
}"#;
        let p = parse_effect_paths(src);
        assert_eq!(p.len(), 3);
        assert_eq!(p["wood"], "fx/explosions/grenadeExp_wood");
        assert_eq!(
            p["screen_rain_mp"],
            "vfx/rain/screen_heavy_rain_cargoship_mp"
        );
        assert_eq!(
            p["lightning strikes"],
            "vfx/lightning/lightning_strike_runner_cargoship_mp"
        );
        assert!(!p.contains_key("rain_noise"));
    }
}

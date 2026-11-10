//! bo2mp: a Ghosts map's placed ambient effects (its createfx), read from the
//! compiled `maps/createfx/<map>_fx` script in the map zone.
//!
//! Ghosts keeps no source: the script is a `ScriptFile` (a string stream plus
//! bytecode, named by a hash). Each placement is one statement group
//! (`ent = createLoopEffect("name"); ent.v["origin"] = ...; ent.v["angles"] =
//! ...; ent.v["fxid"] = ...; ent.v["delay"] = ...`) that compiles to a fixed
//! shape, and its strings sit in the stream in the same order:
//!
//! - bytecode: `88`, a vector (`56` + three f32: the angles), a vector (the
//!   origin), then the rest of the statements. The delay is the first value
//!   after `88 96 5b`: `2b f32`, `98 u8` (negative byte, the default -15) or
//!   `9f u8`;
//! - stream: a token `94 03 3c 6f`, then the NUL-terminated strings of the
//!   statement: the effect's name, `fxid`, `delay`, (`exploder`, its number),
//!   then the next statement's name again.
//!
//! The i-th record of the bytecode goes with the i-th token in the stream
//! (checked by count: a script that does not agree is left unread). Sound
//! placements (`soundalias`) share the script and are skipped here; exploders
//! (`exploder`) wait for a script trigger and are only counted.
//!
//! The names are the level's effect ids; the map's `_fx` script (`level.
//! _effect["id"] = loadfx("path")`) holds id/path pairs in the stream, read by
//! `effect_paths`.

use std::collections::HashMap;
use std::io::Read;

use crate::walk::Capture;

/// One placed effect.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedFx {
    /// The level's effect id (`vfx_mist_ground`).
    pub fxid: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// Seconds: negative is the createfx default (-15).
    pub delay: f32,
}

/// What the map's createfx script holds.
#[derive(Clone, Debug, Default)]
pub struct CreateFx {
    /// The script's name as the zone stores it.
    pub script: String,
    pub placed: Vec<PlacedFx>,
    /// Placements that wait for an exploder (left out).
    pub exploders: usize,
    /// Sound placements (not read here).
    pub sounds: usize,
    /// Level effect id -> effect path, from the map's `_fx` script.
    pub paths: HashMap<String, String>,
    pub notes: Vec<String>,
}

const TOKEN: [u8; 4] = [0x94, 0x03, 0x3c, 0x6f];

fn inflate(buffer: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = flate2::read::ZlibDecoder::new(buffer).read_to_end(&mut out);
    out
}

fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn vec3_at(b: &[u8], at: usize) -> Option<[f32; 3]> {
    Some([f32_at(b, at)?, f32_at(b, at + 4)?, f32_at(b, at + 8)?])
}

/// The printable NUL-terminated strings of `bytes`, in order.
fn strings(bytes: &[u8]) -> Vec<&str> {
    bytes
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty() && s.iter().all(|b| (0x20..0x7f).contains(b)))
        .filter_map(|s| std::str::from_utf8(s).ok())
        .collect()
}

/// Statement records of the bytecode: the offset of each `88 56 <v> 56 <v>`.
fn record_starts(code: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 27 <= code.len() {
        if code[i] == 0x88 && code[i + 1] == 0x56 && code[i + 14] == 0x56 {
            out.push(i);
            i += 27;
        } else {
            i += 1;
        }
    }
    out
}

fn delay_of(record: &[u8]) -> f32 {
    let Some(k) = record.windows(3).position(|w| w == [0x88, 0x96, 0x5b]) else {
        return 0.0;
    };
    match record.get(k + 3) {
        Some(0x2b) => f32_at(record, k + 4).unwrap_or(0.0),
        Some(0x98) => -f32::from(record.get(k + 4).copied().unwrap_or(0)),
        Some(0x9f) => f32::from(record.get(k + 4).copied().unwrap_or(0)),
        _ => 0.0,
    }
}

/// What the createfx source text (`maps/createfx/<map>_fx.gsc`, a raw file
/// in the map zone: `ent = createOneshotEffect( "id" ); ent
/// set_origin_and_angles( (x, y, z), (p, y, r) ); ent.v[ "fxid" ] = "id";
/// ent.v[ "delay" ] = -15;`) places: every `createOneshotEffect`,
/// `createLoopEffect` and other `create*Effect*` statement with an `fxid`,
/// the exploders and the sound placements counted. The compiled script's
/// bytecode pattern (`read`) is a fallback: it gives up a whole map when one
/// statement has another shape, the source does not.
pub fn read_text(src: &str) -> CreateFx {
    let mut out = CreateFx::default();
    for chunk in src.split("ent = ").skip(1) {
        let Some(kind) = chunk.split('(').next().map(str::trim) else { continue };
        if !kind.starts_with("create") {
            continue;
        }
        // One statement group ends at the next `ent = ` (the split) or the
        // function's closing brace.
        let chunk = chunk.split("\n}").next().unwrap_or(chunk);
        let lower = kind.to_ascii_lowercase();
        let mut key_values: HashMap<&str, &str> = HashMap::new();
        for line in chunk.lines() {
            let Some(at) = line.find("ent.v[") else { continue };
            let rest = &line[at + 6..];
            let Some(key) = rest.split('"').nth(1) else { continue };
            let Some((_, value)) = rest.split_once('=') else { continue };
            key_values.insert(key, value.trim().trim_end_matches(';').trim().trim_matches('"'));
        }
        if lower.contains("exploder") {
            out.exploders += 1;
            continue;
        }
        if !lower.contains("effect") && !key_values.contains_key("fxid") {
            out.sounds += 1;
            continue;
        }
        let fxid = key_values
            .get("fxid")
            .map(|s| (*s).to_owned())
            .or_else(|| chunk.split('"').nth(1).map(str::to_owned));
        let Some(fxid) = fxid.filter(|f| !f.is_empty()) else { continue };
        let (mut origin, mut angles) = ([0.0f32; 3], [0.0f32; 3]);
        if let Some(at) = chunk.find("set_origin_and_angles(") {
            let rest = &chunk[at + "set_origin_and_angles(".len()..];
            let tuples: Vec<[f32; 3]> = rest
                .split(')')
                .take(2)
                .filter_map(|t| {
                    let t = t.trim_start_matches([' ', ',', '(']);
                    let v: Vec<f32> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    (v.len() == 3).then(|| [v[0], v[1], v[2]])
                })
                .collect();
            if let Some(o) = tuples.first() {
                origin = *o;
            }
            if let Some(a) = tuples.get(1) {
                angles = *a;
            }
        } else if let Some(at) = chunk.find("ent.v[ \"origin\" ]") {
            let rest = &chunk[at..];
            if let Some(t) = rest.split('(').nth(1).and_then(|t| t.split(')').next()) {
                let v: Vec<f32> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if v.len() == 3 {
                    origin = [v[0], v[1], v[2]];
                }
            }
        }
        let delay = key_values.get("delay").and_then(|d| d.parse::<f32>().ok()).unwrap_or(0.0);
        out.placed.push(PlacedFx { fxid, origin, angles, delay });
    }
    out
}

/// `level._effect[ "id" ] = loadfx( "path" );` lines of a map's `_fx` source.
pub fn effect_paths_text(src: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in src.lines() {
        let Some(at) = line.find("_effect[") else { continue };
        let q: Vec<&str> = line[at..].split('"').collect();
        if q.len() >= 4 && line.contains("loadfx") {
            out.insert(q[1].to_owned(), q[3].to_owned());
        }
    }
    out
}

/// Reads the map's createfx script and its effect id table; `None` when the
/// zone holds no createfx script.
pub fn read_compiled(cap: &Capture, image: &[u8]) -> Option<CreateFx> {
    let mut out = CreateFx::default();
    let mut found = false;
    for sf in &cap.script_files {
        let (Some(buffer), Some(code)) = (sf.buffer, sf.bytecode) else {
            continue;
        };
        let stream = inflate(buffer.get(image));
        // (The effect id table: paths followed by their ids.)
        let strs = strings(&stream);
        for w in strs.windows(2) {
            if (w[0].starts_with("vfx/") || w[0].starts_with("fx/")) && !w[1].contains('/') {
                out.paths
                    .entry(w[1].to_owned())
                    .or_insert_with(|| w[0].to_owned());
            }
        }
        if found || !stream.windows(5).any(|w| w == b"fxid\0") {
            continue;
        }
        let code = code.get(image);
        let starts = record_starts(code);
        let mut tokens = Vec::new();
        let mut i = 0;
        while i + 4 <= stream.len() {
            if stream[i..i + 4] == TOKEN {
                tokens.push(i + 4);
                i += 4;
            } else {
                i += 1;
            }
        }
        if starts.is_empty() || starts.len() != tokens.len() {
            out.notes.push(format!(
                "createfx script {}: {} records but {} string groups; not read",
                sf.name,
                starts.len(),
                tokens.len()
            ));
            continue;
        }
        found = true;
        out.script = sf.name.clone();
        for (n, &start) in starts.iter().enumerate() {
            let end = starts.get(n + 1).copied().unwrap_or(code.len());
            let record = &code[start..end];
            let s_end = tokens.get(n + 1).map_or(stream.len(), |t| t - 4);
            let group = strings(&stream[tokens[n]..s_end]);
            // (The group's first string follows the token, whose last byte
            // is part of the token: `strings` already starts after it.)
            if group.get(1) != Some(&"fxid") {
                out.sounds += usize::from(group.contains(&"soundalias"));
                continue;
            }
            if group.contains(&"exploder") {
                out.exploders += 1;
                continue;
            }
            let (Some(angles), Some(origin)) = (vec3_at(record, 2), vec3_at(record, 15)) else {
                continue;
            };
            out.placed.push(PlacedFx {
                fxid: group[0].to_owned(),
                origin,
                angles,
                delay: delay_of(record),
            });
        }
    }
    (found || !out.notes.is_empty()).then_some(out)
}

/// The map's createfx: the source text when the zone holds it (raw file
/// `maps/createfx/<map>_fx.gsc`; the effect ids' paths from
/// `maps/mp/<map>_fx.gsc` and the compiled scripts' strings), else what the
/// compiled script's bytecode gives (`read_compiled`). `BO2MP_IW6_CREATEFX_TEXT=0`
/// reads the compiled script only.
pub fn read(cap: &Capture, image: &[u8]) -> Option<CreateFx> {
    let compiled = read_compiled(cap, image);
    if std::env::var("BO2MP_IW6_CREATEFX_TEXT").is_ok_and(|v| v.trim() == "0") {
        return compiled;
    }
    let raw = |name: &str| crate::map::raw_file(cap, image, name).map(|b| String::from_utf8_lossy(&b).into_owned());
    let created = cap
        .raw_files
        .iter()
        .map(|r| r.name.clone())
        .find(|n| n.starts_with("maps/createfx/") && n.ends_with("_fx.gsc"));
    let Some(created) = created else { return compiled };
    let Some(src) = raw(&created) else { return compiled };
    let mut text = read_text(&src);
    if text.placed.is_empty() && compiled.as_ref().is_none_or(|c| c.placed.is_empty()) {
        return compiled;
    }
    let map_fx = created.replacen("maps/createfx/", "maps/mp/", 1);
    if let Some(fx_src) = raw(&map_fx) {
        text.paths = effect_paths_text(&fx_src);
    }
    if let Some(c) = &compiled {
        for (id, path) in &c.paths {
            text.paths.entry(id.clone()).or_insert_with(|| path.clone());
        }
        text.notes.extend(c.notes.iter().cloned());
        if c.placed.len() != text.placed.len() {
            text.notes.push(format!(
                "createfx: the source text places {} effects, the compiled script's bytecode pattern read {}; the text is used",
                text.placed.len(),
                c.placed.len()
            ));
        }
    }
    text.script = created;
    Some(text)
}

#[cfg(test)]
mod text_tests {
    use super::*;

    const SRC: &str = "main()\n{\n\tent = createLoopSound();\n\tent set_origin_and_angles( (1, 2, 3), (270, 0, 0) );\n\tent.v[ \"soundalias\" ] = \"a\";\n\n\tent = createOneshotEffect( \"vfx_a\" );\n\tent set_origin_and_angles( (352.193, 2079.25, -1381.13), (270, 357.138, 132.862) );\n\tent.v[ \"fxid\" ] = \"vfx_a\";\n\tent.v[ \"delay\" ] = -15;\n\n\tent = createLoopEffect( \"vfx_b\" );\n\tent set_origin_and_angles( (0, 0, 8), (0, 90, 0) );\n\tent.v[ \"fxid\" ] = \"vfx_b\";\n\tent.v[ \"delay\" ] = 2.5;\n\n\tent = createExploder( \"vfx_c\" );\n\tent set_origin_and_angles( (5, 5, 5), (0, 0, 0) );\n\tent.v[ \"fxid\" ] = \"vfx_c\";\n\tent.v[ \"exploder\" ] = \"1\";\n}\n";

    #[test]
    fn the_source_text_gives_every_placement() {
        let c = read_text(SRC);
        assert_eq!((c.placed.len(), c.exploders, c.sounds), (2, 1, 1));
        assert_eq!(c.placed[0].fxid, "vfx_a");
        assert_eq!(c.placed[0].origin, [352.193, 2079.25, -1381.13]);
        assert_eq!(c.placed[0].angles, [270.0, 357.138, 132.862]);
        assert_eq!(c.placed[0].delay, -15.0);
        assert_eq!(c.placed[1].fxid, "vfx_b");
        assert_eq!(c.placed[1].delay, 2.5);
    }

    #[test]
    fn effect_paths_come_from_the_fx_source() {
        let p = effect_paths_text("\tlevel._effect[ \"x\" ] \t= loadfx( \"vfx/ambient/x\" );\n\tfoo();\n");
        assert_eq!(p.get("x").map(String::as_str), Some("vfx/ambient/x"));
    }
}

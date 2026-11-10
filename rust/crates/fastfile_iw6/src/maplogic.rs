//! bo2mp: what a Ghosts map's own script data says moves, opens and goes
//! off: its door switches and the pieces they move, its exploders, and a
//! count of the scripted things the data alone does not drive.
//!
//! Read from the map's entity text and its createfx script (the source,
//! `maps/createfx/<map>_fx.gsc`, a raw file in the map zone; the compiled
//! copy is `createfx::read`'s). Ghosts' common scripts (`_movers`,
//! `_exploder`, the door switch) are in zones that are not the map's; the
//! map's own compiled script (`maps/mp/<map>`) is bytecode. So:
//!
//! - **Door switches** (READ from the entities): a `trigger_use_touch`
//!   named `door_switch` targets a name; every `script_brushmodel` and
//!   `script_model` of that name that targets a `script_struct` is a piece
//!   that goes from where it stands to that struct (its origin, and its
//!   angles when it has them). The switch's and the pieces'
//!   `script_parameters` (`start_sound=...;stop_sound=...;loop_sound=...`)
//!   are kept. How long a move takes and how it eases are the common
//!   script's (not here).
//! - **Exploders** (READ from the createfx source): `createExploder`
//!   placements with their exploder id, effect, origin, angles, delay and
//!   sounds. What sets an exploder off is the map's compiled script.
//! - **Not driven** (counted): script models the map script animates
//!   (`script_animation`-like key, the crane), destructible toys, pop-up
//!   targets, the switches' lights and blockers, damage triggers.

use std::collections::HashMap;

use crate::map::named_entity_text;

/// The numbered key that holds a mover's `script_parameters`
/// (`start_sound=scn_tele_door_open;...`), named by its values.
pub const SCRIPT_PARAMETERS_KEY: &str = "iw6_27766";

/// The numbered key that names a script model's animation
/// (`mp_frag_metal_door_chain`), named by its values.
pub const SCRIPT_ANIMATION_KEY: &str = "iw6_3571";

/// One entity block: its keys in order.
pub type Block = Vec<(String, String)>;

/// The entity text's blocks (keys named, `named_entity_text`).
pub fn blocks(entities: &str) -> Vec<Block> {
    let text = named_entity_text(entities.trim_end_matches('\0'));
    let mut out = Vec::new();
    for block in text.split('{').skip(1) {
        let mut kv = Vec::new();
        for line in block.lines() {
            let mut parts = line.split('"').skip(1).step_by(2);
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                kv.push((k.to_owned(), v.to_owned()));
            }
        }
        if !kv.is_empty() {
            out.push(kv);
        }
    }
    out
}

fn get<'a>(b: &'a Block, k: &str) -> Option<&'a str> {
    b.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
}

fn vec3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|x| x.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

/// `a=b;c=d` pairs.
pub fn parameters(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// One thing a door switch moves.
#[derive(Clone, Debug, PartialEq)]
pub struct MoverPiece {
    /// Its brush model (`*N`), or its model's name.
    pub brush: Option<u32>,
    pub model: Option<String>,
    pub start: [f32; 3],
    pub start_angles: [f32; 3],
    pub end: [f32; 3],
    pub end_angles: [f32; 3],
    pub parameters: Vec<(String, String)>,
}

impl MoverPiece {
    pub fn parameter(&self, k: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

/// The pieces one name of door switches moves.
#[derive(Clone, Debug, PartialEq)]
pub struct DoorGroup {
    /// The name the switches target (`n_door`).
    pub name: String,
    /// The switches' origins.
    pub switches: Vec<[f32; 3]>,
    /// The switches' own parameters (`button_sound`, `open_interrupt`).
    pub parameters: Vec<(String, String)>,
    pub pieces: Vec<MoverPiece>,
}

/// One effect placement that waits for its exploder.
#[derive(Clone, Debug, PartialEq)]
pub struct ExploderFx {
    /// The exploder id (`ent.v["exploder"]`, a string in the source).
    pub id: String,
    pub fxid: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// Seconds after the exploder fires.
    pub delay: f32,
    pub soundalias: Option<String>,
    pub loopsound: Option<String>,
}

/// What the map's data says moves and goes off, and what it leaves to the
/// map's own script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapLogic {
    pub doors: Vec<DoorGroup>,
    pub exploders: Vec<ExploderFx>,
    /// Placements of the createfx source by kind (`createOneshotEffect`, ...).
    pub createfx_kinds: Vec<(String, usize)>,
    /// What the data does not drive, by kind, with counts.
    pub not_driven: Vec<(&'static str, usize)>,
}

impl MapLogic {
    /// The map's logic from its entity text and, if the zone has it, its
    /// createfx source.
    pub fn read(entities: &str, createfx_source: Option<&str>) -> Self {
        let bl = blocks(entities);
        let mut out = Self {
            doors: doors(&bl),
            ..Self::default()
        };
        if let Some(src) = createfx_source {
            let (ex, kinds) = createfx_exploders(src);
            out.exploders = ex;
            out.createfx_kinds = kinds;
        }
        out.not_driven = not_driven(&bl, &out.doors);
        out
    }

    /// Exploder ids, sorted, each with its placements' count.
    pub fn exploder_ids(&self) -> Vec<(String, usize)> {
        let mut m: Vec<(String, usize)> = Vec::new();
        for e in &self.exploders {
            match m.iter_mut().find(|(i, _)| *i == e.id) {
                Some(x) => x.1 += 1,
                None => m.push((e.id.clone(), 1)),
            }
        }
        m.sort();
        m
    }

    /// One line for the map log.
    pub fn summary(&self) -> String {
        let pieces: usize = self.doors.iter().map(|d| d.pieces.len()).sum();
        let ids: Vec<String> = self
            .exploder_ids()
            .iter()
            .map(|(i, n)| format!("{i} x{n}"))
            .collect();
        let nd: Vec<String> = self
            .not_driven
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        format!(
            "{} door groups ({} pieces: {}), exploders [{}], not driven by the data: {}",
            self.doors.len(),
            pieces,
            self.doors
                .iter()
                .map(|d| format!("{} {}", d.name, d.pieces.len()))
                .collect::<Vec<_>>()
                .join(", "),
            ids.join(", "),
            if nd.is_empty() {
                "none".to_owned()
            } else {
                nd.join(", ")
            }
        )
    }
}

fn doors(bl: &[Block]) -> Vec<DoorGroup> {
    let structs: HashMap<&str, &Block> = bl
        .iter()
        .filter(|b| get(b, "classname").is_some_and(|c| c.starts_with("script_struct")))
        .filter_map(|b| Some((get(b, "targetname")?, b)))
        .collect();
    let mut out: Vec<DoorGroup> = Vec::new();
    for b in bl {
        if get(b, "classname") != Some("trigger_use_touch")
            || get(b, "targetname") != Some("door_switch")
        {
            continue;
        }
        let Some(target) = get(b, "target") else {
            continue;
        };
        let origin = get(b, "origin").and_then(vec3).unwrap_or_default();
        if let Some(g) = out.iter_mut().find(|g| g.name == target) {
            g.switches.push(origin);
            continue;
        }
        let parameters = get(b, SCRIPT_PARAMETERS_KEY)
            .map(parameters)
            .unwrap_or_default();
        let mut pieces = Vec::new();
        for p in bl {
            let class = get(p, "classname").unwrap_or("");
            if !matches!(class, "script_brushmodel" | "script_model")
                || get(p, "targetname") != Some(target)
            {
                continue;
            }
            // (The switch's lights and the AI sight blockers stay put.)
            if get(p, "script_noteworthy")
                .is_some_and(|n| matches!(n, "light_on" | "light_off" | "ai_sight_brush"))
            {
                continue;
            }
            let Some(end) = get(p, "target").and_then(|t| structs.get(t)) else {
                continue;
            };
            let Some(end_origin) = get(end, "origin").and_then(vec3) else {
                continue;
            };
            let start = get(p, "origin").and_then(vec3).unwrap_or_default();
            let start_angles = get(p, "angles").and_then(vec3).unwrap_or_default();
            let model = get(p, "model").unwrap_or("");
            let brush = model.strip_prefix('*').and_then(|n| n.parse().ok());
            pieces.push(MoverPiece {
                brush,
                model: (brush.is_none() && !model.is_empty()).then(|| model.to_owned()),
                start,
                start_angles,
                end: end_origin,
                end_angles: get(end, "angles").and_then(vec3).unwrap_or(start_angles),
                parameters: get(p, SCRIPT_PARAMETERS_KEY)
                    .map(self::parameters)
                    .unwrap_or_default(),
            });
        }
        out.push(DoorGroup {
            name: target.to_owned(),
            switches: vec![origin],
            parameters,
            pieces,
        });
    }
    out.retain(|g| !g.pieces.is_empty());
    out
}

fn not_driven(bl: &[Block], doors: &[DoorGroup]) -> Vec<(&'static str, usize)> {
    let door_name =
        |b: &Block| get(b, "targetname").is_some_and(|t| doors.iter().any(|d| d.name == t));
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut add = |k: &'static str| match counts.iter_mut().find(|(n, _)| *n == k) {
        Some(c) => c.1 += 1,
        None => counts.push((k, 1)),
    };
    for b in bl {
        let class = get(b, "classname").unwrap_or("");
        let tn = get(b, "targetname").unwrap_or("");
        let note = get(b, "script_noteworthy").unwrap_or("");
        if class == "script_model"
            && (get(b, SCRIPT_ANIMATION_KEY).is_some() || tn == "animated_model")
        {
            add("animated script models");
        } else if tn == "destructible_toy" {
            add("destructible toys");
        } else if tn == "sliding_target" {
            add("pop-up target brushes");
        } else if door_name(b) && matches!(note, "light_on" | "light_off") {
            add("door switch lights");
        } else if door_name(b) && class == "trigger_multiple" {
            add("door blockers");
        } else if class == "trigger_damage" && get(b, "script_gameobjectname").is_none() {
            add("damage triggers of the map script");
        }
    }
    counts
}

/// The createfx source's exploder placements and every placement kind's
/// count.
pub fn createfx_exploders(src: &str) -> (Vec<ExploderFx>, Vec<(String, usize)>) {
    let mut out = Vec::new();
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for chunk in src.split("ent = ").skip(1) {
        let Some(kind) = chunk.split('(').next().map(str::trim) else {
            continue;
        };
        if !kind.starts_with("create") {
            continue;
        }
        match kinds.iter_mut().find(|(k, _)| k == kind) {
            Some(k) => k.1 += 1,
            None => kinds.push((kind.to_owned(), 1)),
        }
        if kind != "createExploder" {
            continue;
        }
        let mut fx = ExploderFx {
            id: String::new(),
            fxid: chunk.split('"').nth(1).unwrap_or_default().to_owned(),
            origin: [0.0; 3],
            angles: [0.0; 3],
            delay: 0.0,
            soundalias: None,
            loopsound: None,
        };
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
                fx.origin = *o;
            }
            if let Some(a) = tuples.get(1) {
                fx.angles = *a;
            }
        }
        for line in chunk.lines() {
            let Some(at) = line.find("ent.v[") else {
                continue;
            };
            let rest = &line[at..];
            let key = rest.split('"').nth(1).unwrap_or("");
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_end_matches(';').trim().trim_matches('"');
            match key {
                "exploder" => fx.id = value.to_owned(),
                "fxid" => fx.fxid = value.to_owned(),
                "delay" => fx.delay = value.parse().unwrap_or(0.0),
                "soundalias" => fx.soundalias = Some(value.to_owned()),
                "loopsound" => fx.loopsound = Some(value.to_owned()),
                _ => {}
            }
        }
        if !fx.id.is_empty() {
            out.push(fx);
        }
    }
    (out, kinds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTS: &str = "{\n97 \"trigger_use_touch\"\n642 \"door_switch\"\n640 \"n_door\"\n430 \"445 1557 181\"\n27766 \"button_sound=scn_tele_door_button_push\"\n}\n\
{\n97 \"script_brushmodel\"\n642 \"n_door\"\n640 \"s1\"\n392 \"*51\"\n430 \"451 1542 240\"\n27766 \"start_sound=scn_tele_door_open;stop_sound=scn_tele_door_close\"\n}\n\
{\n97 \"script_struct\"\n642 \"s1\"\n430 \"451 1606 240\"\n}\n\
{\n97 \"script_model\"\n642 \"n_door\"\n506 \"light_on\"\n392 \"mp_frag_button_on_green\"\n430 \"441 1669 179.5\"\n}\n\
{\n97 \"trigger_multiple\"\n642 \"n_door\"\n430 \"447 1562 189\"\n}\n\
{\n97 \"script_model\"\n642 \"animated_model\"\n392 \"mp_frag_crane_anim\"\n430 \"1408 191.5 1395\"\n}\n\
{\n97 \"script_model\"\n642 \"wheel_owner\"\n640 \"s2\"\n392 \"mp_frag_beer_vat_wheel_02\"\n430 \"0 0 0\"\n}\n";

    #[test]
    fn a_door_switch_moves_its_pieces_to_their_structs() {
        let l = MapLogic::read(ENTS, None);
        assert_eq!(l.doors.len(), 1);
        let d = &l.doors[0];
        assert_eq!(d.name, "n_door");
        assert_eq!(d.switches, vec![[445.0, 1557.0, 181.0]]);
        assert_eq!(d.parameters[0].1, "scn_tele_door_button_push");
        assert_eq!(d.pieces.len(), 1, "the light stays");
        let p = &d.pieces[0];
        assert_eq!(p.brush, Some(51));
        assert_eq!(
            (p.start, p.end),
            ([451.0, 1542.0, 240.0], [451.0, 1606.0, 240.0])
        );
        assert_eq!(p.parameter("stop_sound"), Some("scn_tele_door_close"));
        assert_eq!(
            l.not_driven,
            vec![
                ("door switch lights", 1),
                ("door blockers", 1),
                ("animated script models", 1)
            ]
        );
    }

    #[test]
    fn exploders_come_from_the_createfx_source() {
        let src = "main()\n{\n\tent = createLoopSound();\n\tent set_origin_and_angles( (1, 2, 3), (270, 0, 0) );\n\tent.v[ \"soundalias\" ] = \"emt_a\";\n\n\
\tent = createExploder( \"vfx_sprinkler_system_frag\" );\n\tent set_origin_and_angles( (-471.921, 736.257, 115.534), (90, 0, 0) );\n\tent.v[ \"fxid\" ] = \"vfx_sprinkler_system_frag\";\n\tent.v[ \"delay\" ] = 0.5;\n\tent.v[ \"exploder\" ] = \"1\";\n\tent.v[ \"soundalias\" ] = \"emt_spray\";\n}\n";
        let (ex, kinds) = createfx_exploders(src);
        assert_eq!(
            kinds,
            vec![
                ("createLoopSound".to_owned(), 1),
                ("createExploder".to_owned(), 1)
            ]
        );
        assert_eq!(ex.len(), 1);
        let e = &ex[0];
        assert_eq!(
            (e.id.as_str(), e.fxid.as_str(), e.delay),
            ("1", "vfx_sprinkler_system_frag", 0.5)
        );
        assert_eq!(e.origin, [-471.921, 736.257, 115.534]);
        assert_eq!(e.angles, [90.0, 0.0, 0.0]);
        assert_eq!(e.soundalias.as_deref(), Some("emt_spray"));
    }

    /// `IW6_TEST_MAP=<mp_frag.ff> cargo test -p fastfile_iw6 maplogic_real_zone -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn maplogic_real_zone() {
        let Ok(path) = std::env::var("IW6_TEST_MAP") else {
            return;
        };
        let zone = crate::Zone::open(std::path::Path::new(&path)).unwrap();
        let (cap, _) = crate::walk::walk_zone(&zone);
        let map = crate::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
        let name = std::path::Path::new(&path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let src = crate::map::raw_file(&cap, &zone.image, &format!("maps/createfx/{name}_fx.gsc"))
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let l = MapLogic::read(&map.entities, src.as_deref());
        println!("{}", l.summary());
        for d in &l.doors {
            for p in &d.pieces {
                println!(
                    "{} {:?} {:?} {:?} -> {:?}",
                    d.name, p.brush, p.model, p.start, p.end
                );
            }
        }
    }
}

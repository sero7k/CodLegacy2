//! Names of the Ghosts (IW6 MP) script builtins and of the engine-defined entity / hud / node
//! fields, read from the exe's own handler functions and class field tables.
//!
//! Scripts carry only numeric ids: functions are `< 0x8000` (a slot id), methods are
//! `0x8000 + slot` (called on an entity or object). `data/builtins_iw6.names` holds
//! `builtin <id> <name>` lines (the trailing `# V`, `# L` or `# G` is the confidence: verified,
//! likely or guess); `data/fields_iw6.names` holds `token <id> <name>` lines for the code-defined
//! fields (`origin`, `angles`, `targetname`, hud elem fields, player fields, ...). The evidence for
//! every entry is in `docs/exe_notes/iw6_builtins.tsv` and `docs/exe_notes/IW6_GSC_BUILTINS.md`.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::NameTable;

const BUILTINS: &str = include_str!("../data/builtins_iw6.names");
const FIELDS: &str = include_str!("../data/fields_iw6.names");

/// A builtin is a plain function (`isdefined(x)`) or a method (`ent delete()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Function,
    Method,
}

/// Confidence of a builtin name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    Verified,
    Likely,
    Guess,
}

struct Tables {
    by_id: HashMap<u16, (&'static str, Confidence)>,
    by_name: HashMap<(&'static str, Kind), u16>,
    fields: HashMap<u16, &'static str>,
    fields_by_name: HashMap<&'static str, u16>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Tables {
            by_id: HashMap::new(),
            by_name: HashMap::new(),
            fields: HashMap::new(),
            fields_by_name: HashMap::new(),
        };
        for line in BUILTINS.lines() {
            let (body, comment) = line.split_once('#').unwrap_or((line, ""));
            let mut it = body.split_whitespace();
            let (Some("builtin"), Some(id), Some(name)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let Some(id) = id.strip_prefix("0x").and_then(|h| u16::from_str_radix(h, 16).ok()) else {
                continue;
            };
            let conf = match comment.trim() {
                "V" => Confidence::Verified,
                "L" => Confidence::Likely,
                _ => Confidence::Guess,
            };
            t.by_id.insert(id, (name, conf));
            let kind = if id >= 0x8000 { Kind::Method } else { Kind::Function };
            t.by_name.entry((name, kind)).or_insert(id);
        }
        for line in FIELDS.lines() {
            let body = line.split('#').next().unwrap_or("");
            let mut it = body.split_whitespace();
            let (Some("token"), Some(id), Some(name)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            if let Ok(id) = id.parse::<u16>() {
                t.fields.insert(id, name);
                t.fields_by_name.entry(name).or_insert(id);
            }
        }
        t
    })
}

/// Normalises a script id: a method slot below `0x8000` is given the `0x8000` bias.
fn script_id(kind: Kind, id: u16) -> u16 {
    match kind {
        Kind::Method if id < 0x8000 => id + 0x8000,
        _ => id,
    }
}

/// The name of a builtin by script id (methods: `0x8000 + slot`; a bare slot is accepted too).
pub fn name(kind: Kind, id: u16) -> Option<&'static str> {
    tables().by_id.get(&script_id(kind, id)).map(|(n, _)| *n)
}

/// Name and confidence of a builtin.
pub fn name_with_confidence(kind: Kind, id: u16) -> Option<(&'static str, Confidence)> {
    tables().by_id.get(&script_id(kind, id)).copied()
}

/// The script id of a builtin by name (case-sensitive, lower-case GSC spelling).
pub fn lookup(name: &str) -> Option<(Kind, u16)> {
    let t = tables();
    [Kind::Function, Kind::Method]
        .into_iter()
        .find_map(|k| t.by_name.get(&(name, k)).map(|id| (k, *id)))
}

/// The id of a builtin by name and kind.
pub fn lookup_kind(kind: Kind, name: &str) -> Option<u16> {
    tables().by_name.get(&(name, kind)).copied()
}

/// Number of named builtins.
pub fn count() -> usize {
    tables().by_id.len()
}

/// Every named builtin as `(script id, name)`, unordered.
pub fn all() -> impl Iterator<Item = (u16, &'static str)> {
    tables().by_id.iter().map(|(id, (n, _))| (*id, *n))
}

/// Name of an engine-defined field token (`origin`, `angles`, `targetname`, ...).
pub fn field_name(id: u16) -> Option<&'static str> {
    tables().fields.get(&id).copied()
}

/// Token id of an engine-defined field.
pub fn field_id(name: &str) -> Option<u16> {
    tables().fields_by_name.get(name).copied()
}

/// A [`NameTable`] holding `main`, every named builtin and every engine field.
pub fn name_table() -> NameTable {
    let mut t = NameTable::known();
    t.merge(BUILTINS);
    t.merge(FIELDS);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_function_names() {
        assert_eq!(name(Kind::Function, 0x2d), Some("isdefined"));
        assert_eq!(name(Kind::Function, 0x32), Some("setdvar"));
        assert_eq!(name(Kind::Function, 0x63), Some("spawn"));
        assert_eq!(name(Kind::Function, 0x143), Some("loadfx"));
        assert_eq!(name(Kind::Function, 0x188), Some("getent"));
        assert_eq!(name(Kind::Function, 0x189), Some("getentarray"));
        assert_eq!(name(Kind::Function, 0x1c1), Some("getfirstarraykey"));
        assert_eq!(name(Kind::Function, 0x1a6), Some("getnextarraykey"));
        assert_eq!(name(Kind::Function, 0x10d), Some("anglestoforward"));
        assert_eq!(name(Kind::Function, 0x107), Some("vectornormalize"));
        assert_eq!(name(Kind::Function, 0x136), Some("visionsetnaked"));
    }

    #[test]
    fn verified_method_names() {
        assert_eq!(name(Kind::Method, 0x80a9), Some("delete"));
        assert_eq!(name(Kind::Method, 0x80aa), Some("setmodel"));
        assert_eq!(name(Kind::Method, 0x82a3), Some("moveto"));
        assert_eq!(name(Kind::Method, 0x82b2), Some("rotateto"));
        assert_eq!(name(Kind::Method, 0x82b3), Some("solid"));
        assert_eq!(name(Kind::Method, 0x82b4), Some("notsolid"));
        assert_eq!(name(Kind::Method, 0x804a), Some("linkto"));
        assert_eq!(name(Kind::Method, 0x8056), Some("connectpaths"));
        assert_eq!(name(Kind::Method, 0x8057), Some("disconnectpaths"));
        // a bare method slot is accepted
        assert_eq!(name(Kind::Method, 0xa9), Some("delete"));
    }

    #[test]
    fn lookup_round_trips() {
        assert_eq!(lookup("isdefined"), Some((Kind::Function, 0x2d)));
        assert_eq!(lookup("moveto"), Some((Kind::Method, 0x82a3)));
        assert_eq!(lookup_kind(Kind::Method, "delete"), Some(0x80a9));
        assert_eq!(lookup("no_such_builtin"), None);
        for (id, n) in all() {
            let kind = if id >= 0x8000 { Kind::Method } else { Kind::Function };
            assert_eq!(name(kind, id), Some(n));
        }
        assert!(count() > 150);
    }

    #[test]
    fn confidence_is_recorded() {
        assert_eq!(
            name_with_confidence(Kind::Function, 0x2d).map(|x| x.1),
            Some(Confidence::Verified)
        );
        assert_eq!(
            name_with_confidence(Kind::Method, 0x8050).map(|x| x.1),
            Some(Confidence::Likely)
        );
    }

    #[test]
    fn engine_field_ids() {
        assert_eq!(field_name(430), Some("origin"));
        assert_eq!(field_name(47), Some("angles"));
        assert_eq!(field_name(642), Some("targetname"));
        assert_eq!(field_name(640), Some("target"));
        assert_eq!(field_name(392), Some("model"));
        assert_eq!(field_name(97), Some("classname"));
        assert_eq!(field_id("health"), Some(271));
        assert_eq!(field_name(731), Some("x"));
    }

    #[test]
    fn name_table_has_everything() {
        let t = name_table();
        assert_eq!(t.builtin(0x2d), "isdefined");
        assert_eq!(t.builtin(0x82a3), "moveto");
        assert_eq!(t.token(&crate::Name::Id(430)), "origin");
        assert_eq!(t.token(&crate::Name::Id(20445)), "main");
    }
}

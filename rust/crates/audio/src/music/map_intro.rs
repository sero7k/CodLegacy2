//! Map intro music: the track a map plays for about 50 seconds when a player
//! joins a match there (Black Ops' join music). The per-map table is plain
//! text, `map_intro.cfg` beside this file, compiled in and read here; see its
//! header for the line forms. This module is only the table: which entries a
//! map gets and which of them plays. The director carries it out.

use std::collections::HashMap;
use std::path::Path;

use super::config::{Entry, parse_entry};

/// The compiled-in table.
pub const BUILTIN: &str = include_str!("map_intro.cfg");

/// How long an intro plays at most, in seconds (Black Ops' is about this).
pub const DEFAULT_SECONDS: f64 = 50.0;

/// Its level: a trim on in-match music's own base gain
/// (`bus_gain::MATCH_MUSIC_GAIN`), under `music_volume`.
pub const DEFAULT_LEVEL: f32 = 1.0;

/// Where a map's entries came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// The map's own line.
    Map,
    /// Another game's line for the same zone name (Horizon MW's ports).
    SameName,
    /// The game's default.
    GameDefault,
    /// The catch-all default.
    Fallback,
}

#[derive(Clone, Debug, Default)]
pub struct IntroTable {
    sets: HashMap<String, Vec<Entry>>,
    defaults: HashMap<String, Vec<Entry>>,
    maps: HashMap<(String, String), Vec<Entry>>,
    pub warnings: Vec<String>,
}

/// A zone as the load names it, `t5:mp_nuked` or `mp_nuked`, in lower case:
/// (game prefix when it has one, zone name).
pub fn split_zone(zone: &str) -> (Option<String>, String) {
    let z = zone.trim().to_ascii_lowercase();
    match z.split_once(':') {
        Some((game, stem)) if !game.is_empty() && !stem.is_empty() => (canonical_game(game), stem.to_owned()),
        _ => (None, z),
    }
}

/// The zone prefix a game goes by here.
pub fn canonical_game(name: &str) -> Option<String> {
    let n = name.trim().to_ascii_lowercase();
    let g = match n.as_str() {
        "t6" | "bo2" => "t6",
        "t5" | "bo1" => "t5",
        "t4" | "waw" => "t4",
        "iw3" | "cod4" => "iw3",
        "iw4" | "mw2" => "iw4",
        "iw5" | "mw3" => "iw5",
        "iw6" | "ghosts" => "iw6",
        "iw7" | "iw" => "iw7",
        "t7" | "bo3" => "t7",
        "s1" | "aw" => "s1",
        "h1" | "mwr" => "h1",
        "jup" | "mwiii" => "jup",
        _ => return None,
    };
    Some(g.to_owned())
}

/// A map with no intro at all: Zombies keeps its own music.
pub fn is_zombies(stem: &str) -> bool {
    stem.starts_with("zm_")
}

impl IntroTable {
    pub fn builtin() -> Self {
        Self::parse(BUILTIN, Path::new("."))
    }

    /// Parses `text`; relative paths are taken from `base`.
    pub fn parse(text: &str, base: &Path) -> Self {
        let mut table = Self::default();
        let mut raw: Vec<(usize, String, String)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                table.warnings.push(format!("line {}: no `=` in `{line}`", n + 1));
                continue;
            };
            raw.push((n + 1, key.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
        // The named sets first, so a line may use one before it is defined.
        for (n, key, value) in &raw {
            if let Some(name) = key.strip_prefix('@') {
                let entries = table.entries_of(value, base, *n);
                table.sets.insert(name.to_owned(), entries);
            }
        }
        for (n, key, value) in &raw {
            if key.starts_with('@') {
                continue;
            }
            let entries = table.entries_of(value, base, *n);
            if let Some(game) = key.strip_prefix("default") {
                let game = game.trim();
                let game = if game == "*" { Some("*".to_owned()) } else { canonical_game(game) };
                match game {
                    Some(game) => {
                        table.defaults.insert(game, entries);
                    }
                    None => table.warnings.push(format!("line {n}: unknown game in `{key}`")),
                }
                continue;
            }
            for one in key.split_whitespace() {
                let (game, stem) = split_zone(one);
                match game {
                    Some(game) => {
                        table.maps.insert((game, stem), entries.clone());
                    }
                    None => table.warnings.push(format!("line {n}: `{one}` needs a game prefix (t6:mp_la)")),
                }
            }
        }
        table
    }

    fn entries_of(&mut self, value: &str, base: &Path, n: usize) -> Vec<Entry> {
        let mut out = Vec::new();
        for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            if let Some(name) = item.strip_prefix('@') {
                match self.sets.get(&name.to_ascii_lowercase()) {
                    Some(set) => out.extend(set.iter().cloned()),
                    None => self.warnings.push(format!("line {n}: no set `@{name}`")),
                }
                continue;
            }
            match parse_entry(item, base) {
                Entry::None => {}
                entry => out.push(entry),
            }
        }
        out
    }

    /// Replaces (or adds) one map's entries: `music.cfg`'s `intro =` lines.
    pub fn set_map(&mut self, game: &str, stem: &str, entries: Vec<Entry>) {
        self.maps.insert((game.to_owned(), stem.to_owned()), entries);
    }

    /// Does the map have a line of its own?
    pub fn has_map(&self, game: &str, stem: &str) -> bool {
        self.maps.contains_key(&(game.to_ascii_lowercase(), stem.to_ascii_lowercase()))
    }

    /// How many maps have a line of their own.
    pub fn map_count(&self) -> usize {
        self.maps.len()
    }

    /// The entries for a map. `game` is its zone prefix when known.
    pub fn entries(&self, game: Option<&str>, stem: &str) -> (&[Entry], Found) {
        let stem = stem.to_ascii_lowercase();
        let game = game.and_then(canonical_game);
        if let Some(game) = &game
            && let Some(e) = self.maps.get(&(game.clone(), stem.clone()))
        {
            return (e, Found::Map);
        }
        // The game unknown, or its table silent on this name: the same zone
        // name under another game (the lines are sorted so the pick is stable).
        let mut same: Vec<&(String, String)> = self.maps.keys().filter(|(_, s)| *s == stem).collect();
        same.sort();
        if game.is_none()
            && let Some(key) = same.first()
        {
            return (&self.maps[*key], Found::SameName);
        }
        if let Some(game) = &game {
            // Horizon MW's ports and the like: a name some other game has.
            if let Some(key) = same.first() {
                return (&self.maps[*key], Found::SameName);
            }
            if let Some(e) = self.defaults.get(game) {
                return (e, Found::GameDefault);
            }
        }
        (self.defaults.get("*").map_or(&[][..], Vec::as_slice), Found::Fallback)
    }
}

/// A stable hash of a map's name (FNV-1a): the same map always gets the same
/// track out of its set.
pub fn stable_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `items` rotated so the map's own pick comes first and the rest follow as
/// fallbacks.
pub fn ordered<T: Clone>(items: &[T], key: &str) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    let start = (stable_hash(key) % items.len() as u64) as usize;
    (0..items.len()).map(|i| items[(start + i) % items.len()].clone()).collect()
}

/// Does the project's own table give this map a line? (For the map lists'
/// tests: every listed map should have one.)
pub fn builtin_has(game: &str, stem: &str) -> bool {
    static TABLE: std::sync::OnceLock<IntroTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(IntroTable::builtin).has_map(game, stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_core::AssetNamespace;

    #[test]
    fn the_builtin_table_parses_cleanly() {
        let t = IntroTable::builtin();
        assert!(t.warnings.is_empty(), "{:?}", t.warnings);
        assert!(t.map_count() > 300, "{}", t.map_count());
    }

    #[test]
    fn maps_follow_their_game_and_mood() {
        let t = IntroTable::builtin();
        let (e, found) = t.entries(Some("t4"), "mp_makin");
        assert_eq!(found, Found::Map);
        assert_eq!(e, &[Entry::Game(AssetNamespace::T5, "mus_mp_frontend".into())]);
        let (e, _) = t.entries(Some("t6"), "mp_la");
        assert_eq!(e, &[Entry::Game(AssetNamespace::T6, "mus_mp_frontend".into())]);
        // Nuketown is Black Ops' wherever it appears.
        assert_eq!(t.entries(Some("iw4"), "mp_nuked").0, t.entries(Some("t5"), "mp_nuked").0);
        let (e, _) = t.entries(Some("iw6"), "mp_chasm");
        assert!(e.iter().all(|x| matches!(x, Entry::Ghosts(_))), "{e:?}");
    }

    #[test]
    fn unknown_maps_get_the_game_default() {
        let t = IntroTable::builtin();
        let (e, found) = t.entries(Some("t5"), "mp_made_up");
        assert_eq!(found, Found::GameDefault);
        assert_eq!(e, &[Entry::Game(AssetNamespace::T5, "mus_mp_frontend".into())]);
        let (e, found) = t.entries(Some("t4"), "mp_nobody");
        assert_eq!((found, e.len()), (Found::GameDefault, 1));
        let (_, found) = t.entries(None, "mp_nobody");
        assert_eq!(found, Found::Fallback);
        // A usermap port in MWR takes its original's line.
        let (_, found) = t.entries(Some("h1"), "mp_nuked");
        assert_eq!(found, Found::SameName);
    }

    #[test]
    fn zones_split_into_game_and_name() {
        assert_eq!(split_zone("T5:MP_Nuked"), (Some("t5".into()), "mp_nuked".into()));
        assert_eq!(split_zone("mp_nuked"), (None, "mp_nuked".into()));
        assert_eq!(split_zone("cod4:mp_bog"), (Some("iw3".into()), "mp_bog".into()));
        assert!(is_zombies("zm_transit") && !is_zombies("mp_la"));
    }

    #[test]
    fn a_maps_pick_is_stable_and_the_rest_follow() {
        let set = ["a", "b", "c"];
        let one = ordered(&set, "t6:mp_la");
        assert_eq!(one, ordered(&set, "t6:mp_la"));
        let mut sorted = one.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, set);
        // Different maps spread over the set.
        let firsts: std::collections::HashSet<_> =
            (0..40).map(|i| ordered(&set, &format!("iw4:mp_{i}"))[0]).collect();
        assert_eq!(firsts.len(), 3);
    }

    #[test]
    fn lines_can_name_sets_defined_later_and_files() {
        let t = IntroTable::parse(
            "t5:mp_a t5:mp_b = @x, /abs/song.mp3 # two maps\n@x = t5:mus_mp_frontend\ndefault t5 = @x\nbogus\n",
            Path::new("/base"),
        );
        let (e, _) = t.entries(Some("t5"), "mp_b");
        assert_eq!(e.len(), 2);
        assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
        assert!(t.has_map("t5", "mp_a"));
    }
}

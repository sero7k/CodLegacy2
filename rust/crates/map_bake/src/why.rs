//! bo2mp: why a map's baked copy is made again, in words for the loading
//! screen ("game files changed").
//!
//! The key is one number, so a miss alone cannot say what moved. A bake
//! also keeps a key per part of what it was made from ([`Parts`]: the
//! game's files, the settings, the list of items, the code) in its
//! manifest; the next load compares its own parts with them and names the
//! ones that differ. Only the words come from here: whether the copy is
//! used is still the whole key.

use std::path::Path;

use crate::Key;

/// The keys of an entry's parts, by name, in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parts(Vec<(String, u64)>);

/// The part names [`reason`] has words for (others: "<name> changed").
pub const FORMAT: &str = "format";
pub const GAME_FILES: &str = "game_files";
pub const SETTINGS: &str = "settings";
pub const ITEMS: &str = "items";
pub const CODE: &str = "code";

impl Parts {
    /// One part: `f` feeds both the entry's key and the part's own.
    pub fn add(&mut self, name: &str, key: &mut Key, f: impl Fn(&mut Key)) {
        f(key);
        let mut own = Key::new();
        f(&mut own);
        self.0.push((name.to_owned(), own.value()));
    }

    /// The `"parts": {...}` member for a manifest (no trailing comma).
    pub fn to_json(&self) -> String {
        let inner: Vec<String> = self.0.iter().map(|(n, v)| format!("{}: \"{v:016x}\"", crate::pictures::json_str(n))).collect();
        format!("\"parts\": {{{}}}", inner.join(", "))
    }

    /// The parts a manifest written by [`Parts::to_json`] holds (none: an
    /// older manifest, or not one of ours).
    pub fn from_manifest(text: &str) -> Option<Self> {
        let at = text.find("\"parts\"")?;
        let rest = &text[at + 7..];
        let open = rest.find('{')?;
        let close = rest[open..].find('}')? + open;
        let mut out = Vec::new();
        for item in rest[open + 1..close].split(',') {
            let Some((n, v)) = item.split_once(':') else { continue };
            let n = n.trim().trim_matches('"');
            let v = u64::from_str_radix(v.trim().trim_matches('"'), 16).ok()?;
            out.push((n.to_owned(), v));
        }
        Some(Self(out))
    }

    fn get(&self, name: &str) -> Option<u64> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

/// Why the entry in `dir` (its file `file`, its manifest beside it) is made
/// again, for an entry whose parts are now `now`.
pub fn reason(dir: &Path, file: &str, now: &Parts) -> String {
    let manifest = std::fs::read_to_string(dir.join("manifest.json")).ok();
    let had_file = dir.join(file).is_file();
    match (manifest, had_file) {
        (None, false) => "first load".into(),
        (None, true) => "made by an older version".into(),
        (Some(m), had) => match Parts::from_manifest(&m) {
            None => "made by an older version".into(),
            // (A new layout changes the other parts' meaning too: that alone.)
            Some(old) if now.get(FORMAT).is_some() && old.get(FORMAT) != now.get(FORMAT) => "made by an older version".into(),
            Some(old) => {
                let changed: Vec<String> = now.0.iter().filter(|(n, v)| old.get(n) != Some(*v)).map(|(n, _)| words(n)).collect();
                if !changed.is_empty() {
                    changed.join(", ")
                } else if had {
                    "the last copy did not read back whole".into()
                } else {
                    "the last copy was cleared".into()
                }
            }
        },
    }
}

fn words(part: &str) -> String {
    match part {
        GAME_FILES => "game files changed".into(),
        SETTINGS => "graphics setting changed".into(),
        ITEMS => "map's list changed".into(),
        CODE => "game updated".into(),
        other => format!("{other} changed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(files: u64, code: u64) -> Parts {
        let mut k = Key::new();
        let mut p = Parts::default();
        p.add(GAME_FILES, &mut k, |k| {
            k.u64(files);
        });
        p.add(SETTINGS, &mut k, |k| {
            k.u64(7);
        });
        p.add(CODE, &mut k, |k| {
            k.u64(code);
        });
        p
    }

    #[test]
    fn manifest_round_trip_and_reasons() {
        let p = parts(1, 2);
        let m = format!("{{\n  \"map\": \"x\",\n  {},\n  \"pictures\": 3\n}}\n", p.to_json());
        assert_eq!(Parts::from_manifest(&m), Some(p.clone()));

        let dir = std::env::temp_dir().join(format!("map_bake_why_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        assert_eq!(reason(&dir, "pictures.pak", &p), "first load");
        std::fs::write(dir.join("pictures.pak"), b"x").expect("write");
        assert_eq!(reason(&dir, "pictures.pak", &p), "made by an older version");
        std::fs::write(dir.join("manifest.json"), &m).expect("write");
        assert_eq!(reason(&dir, "pictures.pak", &parts(5, 2)), "game files changed");
        assert_eq!(reason(&dir, "pictures.pak", &parts(5, 9)), "game files changed, game updated");
        assert_eq!(reason(&dir, "pictures.pak", &p), "the last copy did not read back whole");
        std::fs::remove_file(dir.join("pictures.pak")).expect("remove");
        assert_eq!(reason(&dir, "pictures.pak", &p), "the last copy was cleared");
        // (A manifest from before the format part, or another format.)
        let mut k = Key::new();
        let mut f = parts(5, 9);
        f.add(FORMAT, &mut k, |k| {
            k.u64(2);
        });
        assert_eq!(reason(&dir, "pictures.pak", &f), "made by an older version");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parts_feed_the_same_key() {
        let mut a = Key::new();
        a.u64(1).u64(7).u64(2);
        let mut b = Key::new();
        let mut p = Parts::default();
        p.add(GAME_FILES, &mut b, |k| {
            k.u64(1);
        });
        p.add(SETTINGS, &mut b, |k| {
            k.u64(7);
        });
        p.add(CODE, &mut b, |k| {
            k.u64(2);
        });
        assert_eq!(a, b);
    }
}

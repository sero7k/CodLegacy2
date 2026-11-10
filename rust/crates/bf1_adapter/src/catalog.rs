//! The Battlefield 1 map list.
//!
//! One map is known to be readable: Verdun Heights, the only level the
//! upstream readers were written against. Every other level is found in the
//! player's own install (`Data\Win32\[<pack>\]levels\<id>\<id>.toc`) and
//! listed as not yet loadable; no map id is hard-coded on a guess.

use std::path::{Path, PathBuf};

use crate::config::Bf1Error;

/// What can be done with a map today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStatus {
    /// The reader opens it and decodes its objects and terrain (`bf1_report`).
    /// It cannot be played: nothing draws it yet.
    Readable,
    /// Listed only. Nothing was tried on it.
    NotYetLoadable,
}

impl MapStatus {
    /// The tag the menus show after the title.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Readable => "reader only",
            Self::NotYetLoadable => "not yet loadable",
        }
    }
}

/// A level the readers know how to open.
#[derive(Clone, Copy, Debug)]
pub struct KnownMap {
    /// Lower-case level id, the part after `bf1:`.
    pub id: &'static str,
    pub title: &'static str,
    /// The level's superbundle, relative to the install's `Data`/`Patch`.
    pub superbundle: &'static str,
    /// The level's root asset name inside it.
    pub level: &'static str,
}

/// Levels the readers were checked against (upstream's own run; not re-run here).
pub const KNOWN: [KnownMap; 1] = [KnownMap {
    id: "mp_verdun",
    title: "Verdun Heights",
    superbundle: "Win32/xpack1/levels/mp_verdun/mp_verdun",
    level: "Xpack1/Levels/MP_Verdun/MP_Verdun",
}];

/// One row of the map list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedMap {
    pub id: String,
    pub title: String,
    pub status: MapStatus,
}

impl ListedMap {
    /// `Verdun Heights (reader only)`.
    pub fn label(&self) -> String {
        format!("{} ({})", self.title, self.status.tag())
    }
}

/// The known map for `id` (any case).
pub fn known(id: &str) -> Option<&'static KnownMap> {
    KNOWN.iter().find(|m| m.id.eq_ignore_ascii_case(id))
}

/// `bf1:mp_verdun` (or just `mp_verdun`) -> the known map; anything else
/// is [`Bf1Error::UnknownMap`].
pub fn parse_spec(spec: &str) -> Result<&'static KnownMap, Bf1Error> {
    let id = spec.trim().strip_prefix("bf1:").unwrap_or(spec.trim());
    known(id).ok_or_else(|| Bf1Error::UnknownMap(id.to_owned()))
}

/// The `mp_*` level ids the install holds, sorted. Looks at
/// `Data/Win32/levels/*` and `Data/Win32/<pack>/levels/*`; a level counts
/// when `<id>/<id>.toc` is there. Not tested against a real install.
pub fn discover_levels(root: &Path) -> Vec<String> {
    let win32 = root.join("Data").join("Win32");
    let mut level_dirs: Vec<PathBuf> = vec![];
    let Ok(entries) = std::fs::read_dir(&win32) else { return vec![] };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if name == "levels" {
            level_dirs.push(p);
        } else if p.is_dir() {
            let inner = p.join("levels");
            if inner.is_dir() {
                level_dirs.push(inner);
            }
            // (Case differs on some installs.)
            let inner = p.join("Levels");
            if inner.is_dir() {
                level_dirs.push(inner);
            }
        }
    }
    let mut ids: Vec<String> = vec![];
    for dir in level_dirs {
        let Ok(levels) = std::fs::read_dir(dir) else { continue };
        for l in levels.flatten() {
            let id = l.file_name().to_string_lossy().to_ascii_lowercase();
            if id.starts_with("mp_") && l.path().join(format!("{}.toc", l.file_name().to_string_lossy())).is_file() {
                ids.push(id);
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// `mp_fields` -> `Fields` (the title of a level nothing names yet).
pub fn title_of(id: &str) -> String {
    let stem = id.strip_prefix("mp_").unwrap_or(id);
    stem.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The map list. With an install, its levels (the known ones `Readable`,
/// the rest `NotYetLoadable`); a known map the install does not hold is
/// left out. Without one (`None`), the known maps only.
pub fn catalog(root: Option<&Path>) -> Vec<ListedMap> {
    let found: Option<Vec<String>> = root.map(discover_levels);
    let mut out: Vec<ListedMap> = KNOWN
        .iter()
        .filter(|k| found.as_ref().is_none_or(|f| f.iter().any(|id| id == k.id)))
        .map(|k| ListedMap { id: k.id.to_owned(), title: k.title.to_owned(), status: MapStatus::Readable })
        .collect();
    for id in found.into_iter().flatten() {
        if known(&id).is_none() {
            out.push(ListedMap { title: title_of(&id), id, status: MapStatus::NotYetLoadable });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parsing() {
        assert_eq!(parse_spec("bf1:mp_verdun").unwrap().title, "Verdun Heights");
        assert_eq!(parse_spec("MP_Verdun").unwrap().id, "mp_verdun");
        assert_eq!(parse_spec("bf1:mp_nope").unwrap_err(), Bf1Error::UnknownMap("mp_nope".to_owned()));
    }

    #[test]
    fn titles() {
        assert_eq!(title_of("mp_fields"), "Fields");
        assert_eq!(title_of("mp_ice_lake"), "Ice Lake");
    }

    #[test]
    fn without_an_install_only_the_known_map_is_listed() {
        let c = catalog(None);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label(), "Verdun Heights (reader only)");
    }

    #[test]
    fn an_install_lists_its_levels_and_marks_the_unknown_ones() {
        let root = std::env::temp_dir().join(format!("bf1-adapter-cat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, id) in [("xpack1/levels", "mp_verdun"), ("xpack1/levels", "mp_fields"), ("levels", "mp_other"), ("levels", "sp_story")] {
            let d = root.join("Data/Win32").join(dir).join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{id}.toc")), b"x").unwrap();
        }
        // A folder with no toc is not a level.
        std::fs::create_dir_all(root.join("Data/Win32/levels/mp_empty")).unwrap();
        let c = catalog(Some(&root));
        let ids: Vec<(&str, MapStatus)> = c.iter().map(|m| (m.id.as_str(), m.status)).collect();
        assert_eq!(
            ids,
            [("mp_verdun", MapStatus::Readable), ("mp_fields", MapStatus::NotYetLoadable), ("mp_other", MapStatus::NotYetLoadable)]
        );
        // An install without Verdun does not list it.
        std::fs::remove_dir_all(root.join("Data/Win32/xpack1/levels/mp_verdun")).unwrap();
        assert!(catalog(Some(&root)).iter().all(|m| m.id != "mp_verdun"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

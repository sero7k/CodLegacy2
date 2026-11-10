//! bo2mp: the Black Ops III material pool's colour statistics, kept on disk.
//!
//! Building the substitution pool (`t7::bo3_pool_run`) reads each candidate
//! material's colour map from the install's packages and measures a small
//! mip level of it (thousands of pictures per map zone). For one install the
//! numbers are the same every start, so they are written to the game's cache
//! (`codlegacy-artifacts/cache/bo_pool_stats`), one small text file per zone, and
//! the next start measures only what the file lacks.
//!
//! The key covers what the numbers are made from: the zone (name, size, time
//! written), the `.xpak` packages beside it, this format's number and the
//! code that classifies and measures (a change makes a new entry, never a
//! stale one). A file that does not parse whole is a miss.

use std::collections::HashMap;
use std::path::Path;

use super::bo3_subst::{Class, PoolItem, TexStats};

const KIND: &str = "bo_pool_stats";
const FORMAT: u64 = 1;

/// The text of the code the numbers come out of.
const CODE: &[&str] = &[
    include_str!("bo3_pool_cache.rs"),
    include_str!("bo3_subst.rs"),
    include_str!("../../../fastfile_t7/src/texture.rs"),
];

/// What the pool made of one material: a candidate, or none (no stats could
/// be read).
pub(crate) type Entries = HashMap<String, Option<PoolItem>>;

/// The cache key for one zone of `dir`, or `None` when the zone is not there.
pub(crate) fn key_for(dir: &Path, zone: &str) -> Option<String> {
    let mut h = asset_transport::fnv1a64(&FORMAT.to_le_bytes());
    for text in CODE {
        h = asset_transport::fnv1a64_more(h, text.as_bytes());
    }
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            name == format!("{}.ff", zone.to_ascii_lowercase()) || name.ends_with(".xpak")
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return None;
    }
    for f in &files {
        let meta = std::fs::metadata(f).ok()?;
        let written = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        h = asset_transport::fnv1a64_more(h, f.file_name()?.to_string_lossy().to_ascii_lowercase().as_bytes());
        h = asset_transport::fnv1a64_more(h, &meta.len().to_le_bytes());
        h = asset_transport::fnv1a64_more(h, &written.to_le_bytes());
    }
    Some(format!("{}_{h:016x}", zone.to_ascii_lowercase()))
}

fn class_name(c: Class) -> &'static str {
    match c {
        Class::Concrete => "concrete",
        Class::Snow => "snow",
        Class::Dirt => "dirt",
        Class::Rock => "rock",
        Class::Metal => "metal",
        Class::Wood => "wood",
        Class::Brick => "brick",
        Class::Asphalt => "asphalt",
        Class::Grass => "grass",
        Class::Foliage => "foliage",
    }
}

fn class_of(name: &str) -> Option<Class> {
    [Class::Concrete, Class::Snow, Class::Dirt, Class::Rock, Class::Metal, Class::Wood, Class::Brick, Class::Asphalt, Class::Grass, Class::Foliage]
        .into_iter()
        .find(|c| class_name(*c) == name)
}

/// Writes entries as text: `name <TAB> -` for a material with no stats, else
/// `name <TAB> class <TAB> name_conf <TAB> r <TAB> g <TAB> b <TAB> spread`.
pub(crate) fn encode(entries: &Entries) -> String {
    let mut rows: Vec<(&String, &Option<PoolItem>)> = entries.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::from("bo3 pool stats 1\n");
    for (name, item) in rows {
        match item {
            None => out.push_str(&format!("{name}\t-\n")),
            Some(it) => out.push_str(&format!(
                "{name}\t{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}\n",
                class_name(it.class),
                it.name_conf,
                it.stats.lin[0],
                it.stats.lin[1],
                it.stats.lin[2],
                it.stats.spread
            )),
        }
    }
    out
}

/// Reads `encode`'s text back (`None` when any row does not parse).
pub(crate) fn decode(text: &str) -> Option<Entries> {
    let mut lines = text.lines();
    if lines.next()? != "bo3 pool stats 1" {
        return None;
    }
    let mut out = Entries::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            [name, "-"] => {
                out.insert((*name).to_owned(), None);
            }
            [name, class, conf, r, g, b, spread] => {
                let num = |s: &str| s.parse::<f32>().ok();
                out.insert(
                    (*name).to_owned(),
                    Some(PoolItem {
                        name: (*name).to_owned(),
                        class: class_of(class)?,
                        name_conf: num(conf)?,
                        stats: TexStats { lin: [num(r)?, num(g)?, num(b)?], spread: num(spread)? },
                    }),
                );
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The entries kept under `key` (empty on a miss).
pub(crate) fn load(key: &str) -> Entries {
    asset_transport::cache_get(KIND, key).and_then(|b| String::from_utf8(b).ok()).and_then(|t| decode(&t)).unwrap_or_default()
}

/// Keeps `entries` under `key`.
pub(crate) fn store(key: &str, entries: &Entries) -> Result<(), String> {
    asset_transport::cache_put(KIND, key, encode(entries).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_a_broken_file_is_a_miss() {
        let mut e = Entries::new();
        e.insert("mtl_concrete_a".into(), Some(PoolItem { name: "mtl_concrete_a".into(), class: Class::Concrete, name_conf: 0.95, stats: TexStats { lin: [0.31, 0.3, 0.28], spread: 0.0625 } }));
        e.insert("mtl_no_pic".into(), None);
        let text = encode(&e);
        let back = decode(&text).expect("round trip");
        assert_eq!(back.len(), 2);
        let it = back["mtl_concrete_a"].as_ref().unwrap();
        assert_eq!((it.class, it.name_conf, it.stats.lin, it.stats.spread), (Class::Concrete, 0.95, [0.31, 0.3, 0.28], 0.0625));
        assert!(back["mtl_no_pic"].is_none());
        assert!(decode(&text.replace("concrete\t", "notaclass\t")).is_none());
        assert!(decode("something else").is_none());
        assert!(decode(&format!("{text}garbage line\n")).is_none());
    }
}

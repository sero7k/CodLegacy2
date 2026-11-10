//! The same copy as `run`, as a library: a plan (what is kept and left per game), a copy that
//! reports progress and hashes every file as it goes, and a check of the finished folder against
//! the hashes. The launcher uses it; nothing here deletes or changes the games it reads.
//!
//! The folder gets `call_of_claude.manifest.json`: every file's size and SHA-256 as copied. That is
//! the record a later check (and the servers' allowed-pack list) compares against.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{rules, up_to_date, walk};

/// The games the pack carries, copied from the player's own installs: each has copy rules and is still used.
/// Modern Warfare Remastered is here because the player needs their own copy of it. The community projects
/// and mods below are not: CodLegacy supplies those itself (see [`SUPPLIED`]).
pub const PACKED: [&str; 8] = ["bo2", "bo1", "waw", "cod4", "bo3", "jup", "ghosts", "mwr"];

/// Community projects and mods CodLegacy supplies in its own signed build, so the player needs none of the
/// original games for them: `game_paths` key and the folder under the install where the build puts them.
/// The MW2 and MW3 remasters and the Advanced Warfare maps (guns, maps, sound effects and so on), and the two
/// Black Ops II community mods.
pub const SUPPLIED: [(&str, &str); 5] = [
    ("mw2", "projects/mw2r"),
    ("mw3", "projects/mw3r"),
    ("aw", "projects/aw-maps"),
    ("bo2mod", "mods/leetin"),
    ("bo2extra", "mods/dlc5"),
];

/// Games in `game_paths` that are never packed, and why.
pub const NOT_PACKED: [(&str, &str); 4] = [
    ("iw", "Infinite Warfare is parked (owner, 5 Nov 2026): no IW files go into any package"),
    ("bf1", "Battlefield 1 is read in place (about 80 GB the rules do not trim)"),
    ("cw", "Black Ops Cold War is used for its banner only; no zones are read"),
    ("vg", "Vanguard is used for its banner only; no zones are read"),
];

/// Whether `key` is carried by the pack.
pub fn is_packed(key: &str) -> bool {
    PACKED.contains(&key)
}

pub const MANIFEST: &str = "call_of_claude.manifest.json";

/// What would be copied from one game.
#[derive(Clone, Debug)]
pub struct GamePlan {
    pub key: String,
    pub source: PathBuf,
    /// (source file, path relative to the game's folder with `/`, size)
    pub files: Vec<(PathBuf, String, u64)>,
    pub keep_bytes: u64,
    /// Bytes of the game's folder that are not copied (videos, other languages, ...).
    pub left_bytes: u64,
}

/// Works out what to keep from each `(game key, folder)`.
pub fn plan(sources: &[(String, PathBuf)]) -> Vec<GamePlan> {
    sources
        .iter()
        .map(|(key, dir)| {
            let rules = rules::for_game(key);
            let (mut files, mut left) = (Vec::new(), 0u64);
            for (path, rel) in walk(dir) {
                let size = std::fs::metadata(&path).map_or(0, |m| m.len());
                if rules.keeps(&rel) {
                    files.push((path, rel, size));
                } else {
                    left += size;
                }
            }
            let keep_bytes = files.iter().map(|f| f.2).sum();
            GamePlan { key: key.clone(), source: dir.clone(), files, keep_bytes, left_bytes: left }
        })
        .collect()
}

pub struct Progress<'a> {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub game: &'a str,
    pub file: &'a str,
}

#[derive(Debug, Default)]
pub struct Built {
    pub files: usize,
    pub copied_bytes: u64,
    /// Source paths that could not be copied (the pack is not complete if any).
    pub failed: Vec<String>,
}

type Manifest = BTreeMap<String, (u64, String)>;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut f = std::fs::File::open(path)?;
    let (mut h, mut n, mut buf) = (Sha256::new(), 0u64, vec![0u8; 1 << 20]);
    loop {
        let r = f.read(&mut buf)?;
        if r == 0 {
            break;
        }
        h.update(&buf[..r]);
        n += r as u64;
    }
    Ok((n, hex(&h.finalize())))
}

/// Copies `src` to `dst` through a `.part` file, hashing what it writes.
fn copy_hashed(src: &Path, dst: &Path, step: &mut dyn FnMut(u64)) -> std::io::Result<(u64, String)> {
    let mut input = std::fs::File::open(src)?;
    let part = dst.with_extension("part-cc");
    let mut out = std::fs::File::create(&part)?;
    let (mut h, mut n, mut buf) = (Sha256::new(), 0u64, vec![0u8; 1 << 20]);
    loop {
        let r = input.read(&mut buf)?;
        if r == 0 {
            break;
        }
        h.update(&buf[..r]);
        out.write_all(&buf[..r])?;
        n += r as u64;
        step(r as u64);
    }
    out.flush()?;
    drop(out);
    if dst.exists() {
        std::fs::remove_file(dst)?;
    }
    std::fs::rename(&part, dst)?;
    Ok((n, hex(&h.finalize())))
}

fn read_manifest(dest: &Path) -> Manifest {
    let parsed: Option<Manifest> = std::fs::read_to_string(dest.join(MANIFEST)).ok().and_then(|t| serde_json::from_str(&t).ok());
    if let Some(m) = parsed {
        return m;
    }
    Manifest::new()
}

/// Copies the plan into `dest/<key>/<rel>`, skipping files already there and unchanged.
pub fn build(plan: &[GamePlan], dest: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Built, String> {
    if plan.iter().any(|g| dest.starts_with(&g.source)) {
        return Err("The destination is inside a game's folder; pick another.".into());
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("cannot make {}: {e}", dest.display()))?;
    let old = read_manifest(dest);
    let mut manifest = Manifest::new();
    let total: u64 = plan.iter().map(|g| g.keep_bytes).sum();
    let (mut done, mut built) = (0u64, Built::default());
    for g in plan {
        for (from, rel, size) in &g.files {
            let name = format!("{}/{rel}", g.key);
            let to = dest.join(&g.key).join(rel);
            let entry = if up_to_date(from, &to) && old.get(&name).is_some_and(|e| e.0 == *size) {
                done += size;
                old.get(&name).cloned()
            } else {
                if let Some(parent) = to.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let before = done;
                let result = copy_hashed(from, &to, &mut |n| {
                    done += n;
                    progress(&Progress { done_bytes: done, total_bytes: total, game: &g.key, file: rel });
                });
                match result {
                    Ok(e) => {
                        built.copied_bytes += e.0;
                        Some(e)
                    }
                    Err(e) => {
                        done = before + size;
                        built.failed.push(format!("{}: {e}", from.display()));
                        None
                    }
                }
            };
            if let Some(e) = entry {
                manifest.insert(name, e);
                built.files += 1;
            }
            progress(&Progress { done_bytes: done, total_bytes: total, game: &g.key, file: rel });
        }
    }
    let text = serde_json::to_string(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join(MANIFEST), text).map_err(|e| format!("cannot write the manifest: {e}"))?;
    let mut marker = String::from("Call of Claude: the files the BO2 rebuild reads, copied out of each game by call_of_claude.\nOne folder per game (its key in game_paths.cfg):\n");
    for g in plan {
        marker.push_str(&format!("{}: {} files, {:.2} GB\n", g.key, g.files.len(), g.keep_bytes as f64 / 1e9));
    }
    let _ = std::fs::write(dest.join(asset_transport::call_of_claude::MARKER), marker);
    Ok(built)
}

/// Re-reads every file in the folder and returns the ones that are missing or no longer match
/// the manifest (empty means the pack is exactly as it was built).
pub fn verify(dest: &Path, progress: &mut dyn FnMut(usize, usize)) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(dest.join(MANIFEST)).map_err(|_| "This folder has no manifest to check against.".to_string())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("The manifest is damaged: {e}"))?;
    let total = manifest.len();
    let mut bad = Vec::new();
    for (i, (name, (size, sha))) in manifest.iter().enumerate() {
        match hash_file(&dest.join(name)) {
            Ok((n, h)) if n == *size && h == *sha => {}
            _ => bad.push(name.clone()),
        }
        progress(i + 1, total);
    }
    Ok(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(dir: &Path) {
        for (rel, body) in [("zone/all/mp_a.ff", "aaaa"), ("zone/all/mp_b.ipak", "bbbbbb"), ("video/intro.bik", "videovideo"), ("zone/french/x.ff", "fr")] {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
    }

    #[test]
    fn plan_copy_verify_and_detect_tampering() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("game");
        game(&src);
        let dest = t.path().join("pack");
        let p = plan(&[("bo2".into(), src.clone())]);
        assert_eq!(p[0].files.len(), 2);
        assert_eq!(p[0].keep_bytes, 10);
        assert_eq!(p[0].left_bytes, 12, "video and French stay behind");

        let mut last = 0;
        let b = build(&p, &dest, &mut |pr| last = pr.done_bytes).unwrap();
        assert_eq!((b.files, b.copied_bytes, b.failed.len(), last), (2, 10, 0, 10));
        assert!(dest.join("bo2/zone/all/mp_a.ff").is_file());
        assert!(!dest.join("bo2/video").exists());
        assert!(asset_transport::call_of_claude::is_pack(&dest));
        assert!(verify(&dest, &mut |_, _| {}).unwrap().is_empty());

        // A second run copies nothing.
        let again = build(&p, &dest, &mut |_| {}).unwrap();
        assert_eq!((again.files, again.copied_bytes), (2, 0));

        // An edited pak is caught, and so is a missing one.
        std::fs::write(dest.join("bo2/zone/all/mp_a.ff"), "evil").unwrap();
        std::fs::remove_file(dest.join("bo2/zone/all/mp_b.ipak")).unwrap();
        let mut bad = verify(&dest, &mut |_, _| {}).unwrap();
        bad.sort();
        assert_eq!(bad, ["bo2/zone/all/mp_a.ff", "bo2/zone/all/mp_b.ipak"]);
    }

    #[test]
    fn parked_and_banner_only_games_are_never_packed() {
        for (key, _) in NOT_PACKED {
            assert!(!is_packed(key), "{key}");
        }
        assert!(is_packed("bo2") && is_packed("jup") && is_packed("mwr"));
        for (key, _) in SUPPLIED {
            assert!(!is_packed(key), "{key} is supplied by CodLegacy, not copied from the player");
        }
    }

    #[test]
    fn the_destination_cannot_be_inside_a_game() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("game");
        game(&src);
        let p = plan(&[("bo2".into(), src.clone())]);
        assert!(build(&p, &src.join("pack"), &mut |_| {}).is_err());
    }
}

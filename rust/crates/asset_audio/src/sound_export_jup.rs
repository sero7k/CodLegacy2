//! bo2mp: the Modern Warfare III (2023) guns' sounds exported to a folder
//! (`CodLegacy export-jup`) and read back from it when the game is not
//! installed.
//!
//! A child of `sound_load_jup`: the export is the same rows the guns'
//! catalog is made from (`read_rows`: every alias the guns name and the
//! layers chained from them, with their clips), but each clip's Opus stream
//! is copied out of the game's package into `sounds/sounds_NNN.pack`
//! (`asset_transport::jup_export::PackWriter`): `JOPS1`, the rate, channels,
//! frame count and seek table length, and the stream as the game plays it.
//! `sounds/aliases.tsv` lists `alias hash<TAB>next layer or -<TAB>clip
//! keys, comma apart`. Reading back builds the same rows with
//! `pack:<key>` clips, so the catalog, the layers and the player are the
//! ones used with the install; only the bytes come from the folder.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use asset_transport::jup_export as ex;

use super::{AliasRows, JUP_GUN_ZONES, JupRows, Path, Pcm, decode_opus, gun_keep, packets, read_jup_stream, read_rows};

/// The clip text of an exported clip: `pack:<key>`.
pub(super) const CLIP_PREFIX: &str = "pack:";
const PREFIX: &str = "sounds";
const ALIASES: &str = "aliases.tsv";
const MAGIC: &[u8; 5] = b"JOPS1";

/// What the export of the sounds made.
#[derive(Clone, Debug, Default)]
pub struct JupSoundExport {
    pub aliases: usize,
    pub clips: usize,
    pub bytes: u64,
    /// Clips that could not be read from the install.
    pub failed: Vec<String>,
}

/// Writes the guns' sounds of `install` into `dest`'s `sounds` folder.
/// `say` hears how far it is.
pub fn export_jup_gun_sounds(install: &Path, dest: &Path, say: &mut dyn FnMut(&str)) -> Result<JupSoundExport, String> {
    let rows = read_rows(install, JUP_GUN_ZONES, Some(&gun_keep()))?;
    let dir = dest.join(ex::SOUNDS_DIR);
    let mut pack = ex::PackWriter::new(&dir, PREFIX)?;
    let mut out = JupSoundExport::default();
    let mut written: HashSet<String> = HashSet::new();
    let mut failed: HashSet<String> = HashSet::new();
    let mut tsv = String::new();
    for (n, a) in rows.aliases.iter().enumerate() {
        if n % 50 == 0 {
            say(&format!("gun sounds: {n} of {} aliases", rows.aliases.len()));
        }
        let mut keys: Vec<String> = Vec::new();
        for (package, clip) in &a.clips {
            let key = clip.split(':').next().unwrap_or_default().to_owned();
            if failed.contains(&key) {
                continue;
            }
            if !written.contains(&key) {
                let read = rows
                    .packages
                    .get(*package as usize)
                    .ok_or_else(|| "a clip names no package".to_owned())
                    .and_then(|p| read_jup_stream(p, clip));
                match read {
                    Ok(c) => {
                        let mut b = MAGIC.to_vec();
                        b.extend_from_slice(&c.rate.to_le_bytes());
                        b.push(c.channels);
                        b.extend_from_slice(&c.frames.to_le_bytes());
                        b.extend_from_slice(&c.table.to_le_bytes());
                        b.extend_from_slice(&(c.stream.len() as u32).to_le_bytes());
                        b.extend_from_slice(&c.stream);
                        pack.add(&key, &b)?;
                        written.insert(key.clone());
                    }
                    Err(e) => {
                        failed.insert(key.clone());
                        out.failed.push(format!("{key}: {e}"));
                        continue;
                    }
                }
            }
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        if keys.is_empty() {
            continue;
        }
        let next = a.secondary.map_or_else(|| "-".to_owned(), |s| format!("{s:016x}"));
        tsv.push_str(&format!("{:016x}\t{next}\t{}\n", a.hash, keys.join(",")));
        out.aliases += 1;
    }
    out.clips = pack.count;
    out.bytes = pack.bytes;
    pack.finish()?;
    let path = dir.join(ALIASES);
    std::fs::write(&path, tsv).map_err(|e| format!("{}: {e}", path.display()))?;
    say(&format!("exported {} gun sound aliases, {} clips ({:.1} MB)", out.aliases, out.clips, out.bytes as f64 / 1e6));
    Ok(out)
}

/// The rows of an exported folder's gun sounds (`None`: not an export, or
/// it has none).
pub(super) fn rows(dir: &Path) -> Option<JupRows> {
    if !ex::is_export(dir) {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(ex::SOUNDS_DIR).join(ALIASES)).ok()?;
    let mut aliases = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [hash, next, keys] = f[..] else { continue };
        let Ok(hash) = u64::from_str_radix(hash, 16) else { continue };
        let secondary = u64::from_str_radix(next, 16).ok();
        let clips = keys.split(',').filter(|k| !k.is_empty()).map(|k| (0u32, format!("{CLIP_PREFIX}{k}"))).collect();
        aliases.push(AliasRows { hash, secondary, clips });
    }
    let streams = aliases.iter().map(|a| a.clips.len()).sum();
    Some(JupRows { packages: vec![dir.join(ex::SOUNDS_DIR).to_string_lossy().into_owned()], aliases, missing: 0, streams })
}

/// The pack lists opened, by folder.
fn indexes() -> &'static Mutex<HashMap<std::path::PathBuf, Option<Arc<ex::PackIndex>>>> {
    static CACHE: OnceLock<Mutex<HashMap<std::path::PathBuf, Option<Arc<ex::PackIndex>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// A clip of an exported folder's `sounds` folder, decoded.
pub(super) fn read_clip(dir: &Path, key: &str) -> Result<Pcm, String> {
    let index = {
        let mut c = indexes().lock().map_err(|_| "sound lists locked".to_owned())?;
        c.entry(dir.to_path_buf()).or_insert_with(|| ex::PackIndex::open(dir, PREFIX).map(Arc::new)).clone()
    }
    .ok_or_else(|| format!("{}: no {PREFIX}.idx", dir.display()))?;
    let b = index.get(key).ok_or_else(|| format!("clip `{key}` is not in {}", dir.display()))?;
    let bad = || format!("clip `{key}`: not an exported clip");
    let body = b.strip_prefix(MAGIC).ok_or_else(bad)?;
    let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(body.get(at..at + 4)?.try_into().ok()?)) };
    let (rate, channels, frames, table, len) = (
        word(0).ok_or_else(bad)?,
        *body.get(4).ok_or_else(bad)?,
        word(5).ok_or_else(bad)?,
        word(9).ok_or_else(bad)?,
        word(13).ok_or_else(bad)? as usize,
    );
    let stream = body.get(17..17 + len).ok_or_else(bad)?;
    decode_opus(packets(stream, table as usize), rate, channels, frames).map_err(|e| e.to_string())
}

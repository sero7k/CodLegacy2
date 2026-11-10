//! bo2mp (load speed): the driver's pipeline cache kept on disk between
//! runs, `wgpu::PipelineCache` on the backends that have one (Vulkan), so a
//! second load of a map skips compiling the pipelines the first one made.
//! One cache per device, handed to every pipeline the load makes (the
//! `prewarm` and `warm` workers, the stages); written back once the load
//! thread has made the stages. The file is `<cache dir>/t7_pipelines.bin`:
//! 8 bytes of checksum, then the driver's blob (which wgpu and the driver
//! check against the device again; a blob that does not fit is dropped).
//! The folder is `CODLEGACY_CACHE_DIR` (a fresh one is a cold load), else
//! `%LOCALAPPDATA%\iw4l\pipelines` (`XDG_CACHE_HOME`, `~/.cache` elsewhere).
//! `BO2MP_T7_PIPELINE_CACHE=0` turns it off.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const FILE: &str = "t7_pipelines.bin";

fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_PIPELINE_CACHE").map_or(true, |v| v.trim() != "0"))
}

/// The folder the file goes in (None: no folder known).
pub fn cache_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = codlegacy_env::var_os("CODLEGACY_CACHE_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = var("LOCALAPPDATA") {
        return Some(d.join("iw4l").join("pipelines"));
    }
    var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache"))).map(|d| d.join("iw4l"))
}

pub(crate) fn checksum(data: &[u8]) -> u64 {
    // FNV-1a, 64 bit.
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The file's bytes for a driver blob.
fn encode(blob: &[u8]) -> Vec<u8> {
    let mut raw = checksum(blob).to_le_bytes().to_vec();
    raw.extend_from_slice(blob);
    raw
}

/// The driver blob in a file's bytes (None: short or not matching its checksum).
fn decode(raw: &[u8]) -> Option<&[u8]> {
    let (head, body) = raw.split_at_checked(8)?;
    (u64::from_le_bytes(head.try_into().ok()?) == checksum(body)).then_some(body)
}

/// Writes `blob` to `path` through a temporary file (a load that dies
/// mid-write leaves the old file).
fn write(path: &Path, blob: &[u8]) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, encode(blob)).is_ok() && std::fs::rename(&tmp, path).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&tmp);
    false
}

struct Cache {
    cache: wgpu::PipelineCache,
    path: PathBuf,
    /// The blob's size on disk (written back only when it grew).
    saved: AtomicUsize,
}

fn open(device: &wgpu::Device) -> Option<Arc<Cache>> {
    if !on() || !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    let path = cache_dir()?.join(FILE);
    let raw = std::fs::read(&path).ok();
    let data = raw.as_deref().and_then(decode);
    // SAFETY: the blob is wgpu's own `get_data` output, checked by checksum
    // here and by the device's header check (`fallback`: an unfit blob is an
    // empty cache).
    let cache = unsafe { device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor { label: Some("t7_pipelines"), data, fallback: true }) };
    let loaded = data.map_or(0, <[u8]>::len);
    crate::prewarm::note(&format!("t7 pipeline cache: {} ({loaded} bytes loaded)", path.display()));
    Some(Arc::new(Cache { cache, path, saved: AtomicUsize::new(loaded) }))
}

fn entry(device: &wgpu::Device) -> Option<Arc<Cache>> {
    crate::memo::get(device, "t7_pipeline_cache".into(), || open(device))
}

/// The cache every pipeline made on `device` is given (None: the device has
/// none, or it is off).
pub fn get(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
    entry(device).map(|c| c.cache.clone())
}

/// Writes the cache of `device` back when it grew.
pub fn save(device: &wgpu::Device) {
    let Some(c) = entry(device) else { return };
    let Some(blob) = c.cache.get_data() else { return };
    if blob.len() > c.saved.load(Ordering::Relaxed) && write(&c.path, &blob) {
        c.saved.store(blob.len(), Ordering::Relaxed);
        crate::prewarm::note(&format!("t7 pipeline cache: {} bytes written", blob.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trip() {
        let blob: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let raw = encode(&blob);
        assert_eq!(raw.len(), blob.len() + 8);
        assert_eq!(decode(&raw), Some(&blob[..]));
        assert_eq!(decode(&encode(&[])), Some(&[][..]));
    }

    #[test]
    fn damaged_file_is_no_cache() {
        let mut raw = encode(b"driver blob");
        assert_eq!(decode(&raw[..7]), None);
        assert_eq!(decode(&raw[..raw.len() - 1]), None);
        raw[9] ^= 1;
        assert_eq!(decode(&raw), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn write_replaces_the_file() {
        let dir = std::env::temp_dir().join(format!("t7_pipecache_test_{}", std::process::id()));
        let path = dir.join(FILE);
        assert!(write(&path, b"first"));
        assert!(write(&path, b"second, longer"));
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(decode(&raw), Some(&b"second, longer"[..]));
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_cache_without_the_feature() {
        // (The headless device asks for no PIPELINE_CACHE: every pipeline
        // is made as before, with no cache.)
        let Ok(gpu) = render_h1::gpu::Gpu::headless() else { return eprintln!("skipped: no device") };
        if !gpu.device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            assert!(get(&gpu.device).is_none());
            save(&gpu.device);
        }
    }
}

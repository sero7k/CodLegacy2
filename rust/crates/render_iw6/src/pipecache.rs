//! The driver's pipeline cache kept on disk between runs (personal use):
//! `wgpu::PipelineCache` on the backends that have one (Vulkan). The file is
//! `<cache dir>/iw6_pipelines_<map>.bin`: 8 bytes of checksum, then the
//! driver's blob (which wgpu and the driver check against the device again;
//! a blob that does not fit is dropped). `BO2MP_IW6_PIPELINE_CACHE=0` turns
//! it off; the directory is `BO2MP_CACHE_DIR`, else the platform's cache
//! folder (`XDG_CACHE_HOME`, `LOCALAPPDATA`, `~/.cache`) plus `bo2mp`.

use std::path::PathBuf;

use render_h1::gpu::Gpu;

fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_PIPELINE_CACHE");
    S.get()
}

pub fn cache_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = var("BO2MP_CACHE_DIR") {
        return Some(d);
    }
    let base = var("XDG_CACHE_HOME")
        .or_else(|| var("LOCALAPPDATA"))
        .or_else(|| var("HOME").map(|h| h.join(".cache")))?;
    Some(base.join("bo2mp"))
}

fn checksum(data: &[u8]) -> u64 {
    // FNV-1a, 64 bit.
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The cache for `map` (None: not supported by this device, or off) with the file and the size loaded.
pub fn open(gpu: &Gpu, map: &str) -> Option<(wgpu::PipelineCache, PathBuf, usize)> {
    if !on() || !gpu.device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    let path = cache_dir()?.join(format!("iw6_pipelines_{}.bin", map.to_ascii_lowercase()));
    let data = std::fs::read(&path).ok().and_then(|raw| decode(&raw));
    let loaded = data.as_ref().map_or(0, Vec::len);
    // SAFETY: the blob is wgpu's own `get_data` output, checked by checksum
    // here and by the device's header check (`fallback`: an unfit blob is an
    // empty cache).
    let cache = unsafe {
        gpu.device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("iw6_pipelines"),
            data: data.as_deref(),
            fallback: true,
        })
    };
    eprintln!("note: iw6 pipeline cache: {} ({} bytes loaded)", path.display(), loaded);
    Some((cache, path, loaded))
}

/// Writes the cache back when it grew.
pub fn save(cache: &(wgpu::PipelineCache, PathBuf, usize)) {
    let Some(data) = cache.0.get_data() else { return };
    if data.len() <= cache.2 {
        return;
    }
    if data.len() > MAX_FILE_BYTES {
        return;
    }
    let raw = encode(&data);
    if let Some(dir) = cache.1.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = cache.1.with_extension("tmp");
    if std::fs::write(&tmp, &raw).is_ok() && std::fs::rename(&tmp, &cache.1).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    if let Some(dir) = cache.1.parent() {
        prune(dir, MAX_DIR_BYTES);
    }
}

/// One map's blob above this is not stored (a runaway cache).
const MAX_FILE_BYTES: usize = 256 << 20;
/// All `iw6_pipelines_*.bin` files together stay under this; the oldest go first.
const MAX_DIR_BYTES: u64 = 1 << 30;

/// The file image: 8 bytes of checksum, then the blob.
pub fn encode(blob: &[u8]) -> Vec<u8> {
    let mut raw = checksum(blob).to_le_bytes().to_vec();
    raw.extend_from_slice(blob);
    raw
}

/// The blob of a file image (None when the checksum does not fit).
pub fn decode(raw: &[u8]) -> Option<Vec<u8>> {
    let (head, body) = raw.split_at_checked(8)?;
    (u64::from_le_bytes(head.try_into().ok()?) == checksum(body)).then(|| body.to_vec())
}

/// Removes the oldest cache files in `dir` until the total is within `cap` bytes.
pub fn prune(dir: &std::path::Path, cap: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("iw6_pipelines_") && n.ends_with(".bin")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_corruption() {
        let blob = vec![1u8, 2, 3, 4, 5];
        let raw = encode(&blob);
        assert_eq!(decode(&raw), Some(blob));
        let mut bad = raw.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(decode(&bad), None);
        assert_eq!(decode(&raw[..4]), None);
    }

    #[test]
    fn prune_drops_oldest_first() {
        let dir = std::env::temp_dir().join(format!("bo2mp_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, n) in ["a", "b", "c"].iter().enumerate() {
            let p = dir.join(format!("iw6_pipelines_{n}.bin"));
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i as u64);
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
        }
        std::fs::write(dir.join("other.bin"), vec![0u8; 500]).unwrap();
        prune(&dir, 200);
        assert!(!dir.join("iw6_pipelines_a.bin").exists());
        assert!(dir.join("iw6_pipelines_b.bin").exists());
        assert!(dir.join("iw6_pipelines_c.bin").exists());
        assert!(dir.join("other.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! bo2mp (load speed): the driver's pipeline cache for the exact colour
//! pipelines (`exact_pipeline.rs`), kept on disk between runs, so a second
//! load of an IW map skips compiling the pipelines an earlier run made
//! (Terminal: 3712 pipelines, 7.6 s). `wgpu::PipelineCache` on the backends
//! that have one (Vulkan); none elsewhere, and the pipelines build as before.
//! One cache per process, handed to every exact pipeline; written back when
//! the builds in flight are done and the blob grew. The file is
//! `<cache dir>/exact_pipelines.bin` (the folder of `t7_pipelines.bin`):
//! 8 bytes of checksum, then the driver's blob (which wgpu and the driver
//! check against the device again; a blob that does not fit is dropped).
//! `BO2MP_EXACT_PIPELINE_CACHE=0` turns it off.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::render::renderer::RenderDevice;

const FILE: &str = "exact_pipelines.bin";
/// A blob above this is not stored (a runaway cache).
const MAX_FILE_BYTES: usize = 512 << 20;

fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_EXACT_PIPELINE_CACHE").map_or(true, |v| v.trim() != "0"))
}

pub(super) struct DiskCache {
    pub(super) cache: wgpu::PipelineCache,
    path: PathBuf,
    /// The blob size on disk (written back only above it).
    saved: AtomicUsize,
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

/// Writes `raw` to `path` through a temporary file (a run that dies
/// mid-write leaves the old file).
fn write(path: &Path, raw: &[u8]) -> bool {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, raw).is_ok() && std::fs::rename(&tmp, path).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&tmp);
    false
}

/// The cache, from its file when there is one (None: off, no folder, or a
/// device without pipeline caches).
pub(super) fn open(device: &RenderDevice) -> Option<Arc<DiskCache>> {
    let wgpu_device = device.wgpu_device();
    if !on() || !wgpu_device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    let path = render_t7::pipecache::cache_dir()?.join(FILE);
    let raw = std::fs::read(&path).ok();
    let data = raw.as_deref().and_then(decode);
    let loaded = data.map_or(0, <[u8]>::len);
    // SAFETY: the blob is wgpu's own `get_data` output, checked by checksum
    // here and by the device's header check (`fallback`: an unfit blob is an
    // empty cache).
    let cache = unsafe {
        wgpu_device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("exact_pipelines"),
            data,
            fallback: true,
        })
    };
    diag::info!(World, "exact pipeline cache: {} ({loaded} bytes loaded)", path.display());
    Some(Arc::new(DiskCache {
        cache,
        path,
        saved: AtomicUsize::new(loaded),
    }))
}

impl DiskCache {
    /// Writes the blob back when it grew (on the load pool: tens of MB).
    pub(super) fn save(self: &Arc<Self>) {
        let me = self.clone();
        assets::load_pool()
            .spawn(async move {
                let Some(blob) = me.cache.get_data() else { return };
                let had = me.saved.load(Ordering::Relaxed);
                if blob.len() <= had || blob.len() > MAX_FILE_BYTES {
                    return;
                }
                if write(&me.path, &encode(&blob)) {
                    me.saved.fetch_max(blob.len(), Ordering::Relaxed);
                    diag::info!(World, "exact pipeline cache: wrote {} bytes (had {had})", blob.len());
                }
            })
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_damage() {
        let blob = vec![1u8, 2, 3, 4, 5];
        let raw = encode(&blob);
        assert_eq!(decode(&raw), Some(&blob[..]));
        let mut bad = raw.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(decode(&bad), None);
        assert_eq!(decode(&raw[..4]), None);
    }

    #[test]
    fn write_replaces_the_file() {
        let dir = std::env::temp_dir().join(format!("exact_pipecache_test_{}", std::process::id()));
        let path = dir.join(FILE);
        assert!(write(&path, &encode(&[7, 8])));
        assert!(write(&path, &encode(&[9, 10, 11])));
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(decode(&raw), Some(&[9u8, 10, 11][..]));
        assert!(!dir.join("exact_pipelines.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

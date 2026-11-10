//! bo2mp patch: a Vulkan pipeline cache for every pipeline made without one
//! of its own (all of Bevy's: it passes `cache: None`), kept on disk so the
//! driver does not compile the same pipelines again on the next start.
//!
//! The file is `codlegacy-artifacts/cache/vk_pipeline_cache.bin` under the working
//! directory (only when `codlegacy-artifacts` is already there), or the path in
//! `BO2MP_VK_PIPELINE_CACHE`; `BO2MP_VK_PIPELINE_CACHE=0` turns it off. It
//! holds the 16-byte validation key (the driver version) and the driver's
//! own blob, which the driver checks again itself. It is saved when the
//! device goes and every 30 s while it grows, written beside and swapped in
//! whole.

use alloc::{borrow::ToOwned, sync::{Arc, Weak}, vec::Vec};
use ash::vk;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct AutoPipelineCache {
    pub(super) raw: vk::PipelineCache,
    path: PathBuf,
    saved_len: AtomicUsize,
}

fn cache_path() -> Option<PathBuf> {
    match std::env::var("BO2MP_VK_PIPELINE_CACHE") {
        Ok(v) if v.trim() == "0" => None,
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v.trim())),
        _ => {
            let dir = PathBuf::from("codlegacy-artifacts");
            dir.is_dir()
                .then(|| dir.join("cache").join("vk_pipeline_cache.bin"))
        }
    }
}

impl AutoPipelineCache {
    pub(super) fn open(device: &ash::Device, key: &[u8; 16]) -> Option<Self> {
        let path = cache_path()?;
        let kept = std::fs::read(&path)
            .ok()
            .filter(|b| b.len() > 16 && b[..16] == key[..])
            .map(|b| b[16..].to_vec());
        let mut info = vk::PipelineCacheCreateInfo::default();
        if let Some(data) = kept.as_deref() {
            info = info.initial_data(data);
        }
        let raw = match unsafe { device.create_pipeline_cache(&info, None) } {
            Ok(raw) => raw,
            Err(_) if kept.is_some() => {
                // (A blob the driver will not take: start empty.)
                unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None) }.ok()?
            }
            Err(_) => return None,
        };
        log::info!(
            "pipeline cache: {} from {}",
            kept.as_ref().map_or("empty".to_owned(), |b| format!("{} KiB kept", b.len() / 1024)),
            path.display()
        );
        Some(Self {
            raw,
            path,
            saved_len: AtomicUsize::new(kept.map_or(0, |b| b.len())),
        })
    }

    pub(super) fn save(&self, device: &ash::Device, key: &[u8; 16]) {
        let Ok(data) = (unsafe { device.get_pipeline_cache_data(self.raw) }) else {
            return;
        };
        if data.is_empty() || data.len() == self.saved_len.load(Ordering::Relaxed) {
            return;
        }
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("tmp");
        let mut out = Vec::with_capacity(16 + data.len());
        out.extend_from_slice(key);
        out.extend_from_slice(&data);
        if std::fs::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, &self.path).is_ok() {
            self.saved_len.store(data.len(), Ordering::Relaxed);
        }
    }
}

/// Saves the cache every 30 s while the device lives and it has grown.
pub(super) fn start_saving(shared: &Arc<super::DeviceShared>) {
    if shared.auto_cache.is_none() {
        return;
    }
    let weak: Weak<super::DeviceShared> = Arc::downgrade(shared);
    let _ = std::thread::Builder::new()
        .name("vk pipeline cache".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let Some(shared) = weak.upgrade() else { break };
            if let Some(cache) = shared.auto_cache.as_ref() {
                cache.save(&shared.raw, &shared.pipeline_cache_validation_key);
            }
        });
}

//! bo2mp: how much memory the graphics card has, asked of the backend once
//! the adapter is known, so the quality the renderers apply can be lowered
//! to fit a 4 GB laptop card before the first map's pictures and shadow
//! maps are made (`native_quality::Vram`).
//!
//! wgpu itself does not say: DirectX 12's adapter reports its dedicated
//! video memory (`DXGI_ADAPTER_DESC1`), Vulkan's physical device the sizes
//! of its device-local heaps. Other backends (OpenGL, Metal) report nothing,
//! and the adapter's kind and name decide instead.

use frame::settings::native_quality::Vram;

/// The card's own memory in bytes, when the backend can say.
pub fn adapter_memory(adapter: &wgpu::Adapter) -> Option<u64> {
    if let Ok(v) = std::env::var("BO2MP_VRAM_MB")
        && let Ok(mb) = v.trim().parse::<u64>()
    {
        return Some(mb << 20);
    }
    vulkan_memory(adapter).or_else(|| dx12_memory(adapter))
}

/// The device-local heaps' sizes summed (a discrete card's own memory; an
/// integrated chip's device-local heap is its share of the PC's).
#[cfg(any(windows, target_os = "linux"))]
fn vulkan_memory(adapter: &wgpu::Adapter) -> Option<u64> {
    let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }?;
    let props = unsafe {
        hal.shared_instance()
            .raw_instance()
            .get_physical_device_memory_properties(hal.raw_physical_device())
    };
    let local: u64 = props
        .memory_heaps_as_slice()
        .iter()
        // `VK_MEMORY_HEAP_DEVICE_LOCAL_BIT` is bit 0.
        .filter(|h| h.flags.as_raw() & 1 != 0)
        .map(|h| h.size)
        .sum();
    (local > 0).then_some(local)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn vulkan_memory(_adapter: &wgpu::Adapter) -> Option<u64> {
    None
}

/// `DXGI_ADAPTER_DESC1::DedicatedVideoMemory`.
#[cfg(windows)]
fn dx12_memory(adapter: &wgpu::Adapter) -> Option<u64> {
    let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }?;
    let desc = unsafe { hal.raw_adapter().GetDesc1() }.ok()?;
    let dedicated = desc.DedicatedVideoMemory as u64;
    (dedicated > 0).then_some(dedicated)
}

#[cfg(not(windows))]
fn dx12_memory(_adapter: &wgpu::Adapter) -> Option<u64> {
    None
}

/// What the renderer found, for `native_quality::set_vram`.
pub fn probe(adapter: &wgpu::Adapter, info: &wgpu::AdapterInfo) -> Vram {
    Vram {
        bytes: adapter_memory(adapter),
        integrated: matches!(info.device_type, wgpu::DeviceType::IntegratedGpu),
        name: info.name.clone(),
    }
}

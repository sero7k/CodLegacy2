//! bo2mp: can this adapter and device run the upscalers at all? Decided once,
//! from what wgpu reports, before any texture, pipeline or camera of the
//! upscalers exists, so a missing capability is a line in the log and a
//! fall-back (FSR 3.1 to FSR 1, FSR 1 to Off), not a validation error in the
//! middle of a frame (which drops the whole frame's commands: a frozen picture).

use bevy::render::render_resource::{
    TextureFormat, TextureFormatFeatureFlags, TextureFormatFeatures, TextureUsages,
};
use bevy::render::settings::{WgpuFeatures, WgpuLimits};

/// What the checks need to know about the machine.
pub(crate) struct DeviceCaps<'a> {
    pub vulkan: bool,
    /// `DownlevelFlags::VIEW_FORMATS`: a texture can be viewed in the other
    /// sRGB-ness (the lens image is read as plain `Unorm`). OpenGL has no such thing.
    pub view_formats: bool,
    pub features: WgpuFeatures,
    pub limits: &'a WgpuLimits,
    pub format: &'a dyn Fn(TextureFormat) -> TextureFormatFeatures,
}

/// A texture format and what it is used for.
struct Need {
    format: TextureFormat,
    usage: TextureUsages,
    /// Read-write storage access (`STORAGE_READ_WRITE`), not just write.
    read_write: bool,
}

const fn need(format: TextureFormat, usage: TextureUsages, read_write: bool) -> Need {
    Need {
        format,
        usage,
        read_write,
    }
}

const SAMPLED: TextureUsages = TextureUsages::TEXTURE_BINDING;
const TARGET: TextureUsages =
    TextureUsages::RENDER_ATTACHMENT.union(TextureUsages::TEXTURE_BINDING);
const STORAGE: TextureUsages = TextureUsages::STORAGE_BINDING.union(TextureUsages::TEXTURE_BINDING);

/// FSR 1 and the lens: the sRGB image the lens draws into (read as `Unorm`
/// through a view) and the enlarged frame.
fn fsr1_needs() -> [Need; 2] {
    [
        need(
            TextureFormat::Rgba8UnormSrgb,
            TARGET.union(TextureUsages::COPY_SRC),
            false,
        ),
        need(TextureFormat::Rgba8Unorm, TARGET, false),
    ]
}

/// FSR 3.1: our copies and the motion target (render attachments), and the
/// formats `wgpu-ffx` binds as storage textures (read from its
/// `resources.rs`: read-write unless noted), and its Lanczos table.
fn fsr3_needs() -> [Need; 13] {
    [
        need(TextureFormat::Rgba16Float, TARGET, false), // colour copy
        need(TextureFormat::R32Float, TARGET.union(STORAGE), false), // depth copy, dilated depth
        need(TextureFormat::Rg16Float, TARGET.union(STORAGE), false), // motion, dilated motion
        need(TextureFormat::Rgba16Float, STORAGE, true), // output, upscaled history, luma history
        need(TextureFormat::Rg16Float, STORAGE, true),   // SPD mips
        need(TextureFormat::R16Float, STORAGE, true),    // luma, farthest depth, instability
        need(TextureFormat::R8Unorm, STORAGE, true),     // accumulation, shading change, locks
        need(TextureFormat::Rgba32Float, STORAGE, true), // frame info
        need(TextureFormat::Rgba8Unorm, STORAGE, false), // reactive masks
        need(TextureFormat::R32Float, STORAGE, false),
        need(TextureFormat::R16Snorm, SAMPLED, false), // Lanczos table
        need(TextureFormat::Rg32Float, SAMPLED, false), // default exposure
        need(TextureFormat::R8Unorm, SAMPLED, false),  // default reactivity
    ]
}

/// The most storage textures one FSR 3.1 compute pass binds (the luma pyramid:
/// 6 SPD mips, the farthest depth and the frame info).
pub(crate) const FSR3_STORAGE_TEXTURES: u32 = 8;
/// SPD's workgroup: 256 invocations, 16 KB of shared memory.
pub(crate) const FSR3_WORKGROUP_INVOCATIONS: u32 = 256;
pub(crate) const FSR3_WORKGROUP_STORAGE: u32 = 16384;

fn check_formats(caps: &DeviceCaps, needs: &[Need], out: &mut Vec<String>) {
    for n in needs {
        let have = (caps.format)(n.format);
        if !have.allowed_usages.contains(n.usage) {
            out.push(format!(
                "{:?} cannot be used as {:?} (it allows {:?})",
                n.format, n.usage, have.allowed_usages
            ));
        } else if n.read_write
            && !have
                .flags
                .contains(TextureFormatFeatureFlags::STORAGE_READ_WRITE)
        {
            out.push(format!(
                "{:?} has no read-write storage access on this adapter",
                n.format
            ));
        }
    }
}

/// Why FSR 1 (and the lens image it needs) cannot run here; empty when it can.
pub(crate) fn fsr1_issues(caps: &DeviceCaps) -> Vec<String> {
    let mut out = Vec::new();
    if !caps.view_formats {
        out.push(
            "the backend cannot view an sRGB texture as plain Unorm (no view formats), which the lens image needs"
                .into(),
        );
    }
    check_formats(caps, &fsr1_needs(), &mut out);
    out
}

/// Why FSR 3.1 cannot run here; empty when it can.
pub(crate) fn fsr3_issues(caps: &DeviceCaps) -> Vec<String> {
    let mut out = Vec::new();
    if !caps.vulkan {
        out.push("FSR 3.1 ships SPIR-V shaders and needs the Vulkan backend".into());
    }
    for (feature, name) in [
        (
            WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            "TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES",
        ),
        (
            WgpuFeatures::TEXTURE_FORMAT_16BIT_NORM,
            "TEXTURE_FORMAT_16BIT_NORM",
        ),
    ] {
        if !caps.features.contains(feature) {
            out.push(format!("the device was made without {name}"));
        }
    }
    let l = caps.limits;
    for (have, want, name) in [
        (
            l.max_storage_textures_per_shader_stage,
            FSR3_STORAGE_TEXTURES,
            "max_storage_textures_per_shader_stage",
        ),
        (
            l.max_compute_invocations_per_workgroup,
            FSR3_WORKGROUP_INVOCATIONS,
            "max_compute_invocations_per_workgroup",
        ),
        (
            l.max_compute_workgroup_size_x,
            FSR3_WORKGROUP_INVOCATIONS,
            "max_compute_workgroup_size_x",
        ),
        (
            l.max_compute_workgroup_storage_size,
            FSR3_WORKGROUP_STORAGE,
            "max_compute_workgroup_storage_size",
        ),
    ] {
        if have < want {
            out.push(format!("{name} is {have}, FSR 3.1 needs {want}"));
        }
    }
    check_formats(caps, &fsr3_needs(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An adapter that allows everything, read-write storage included.
    fn everything(_: TextureFormat) -> TextureFormatFeatures {
        TextureFormatFeatures {
            allowed_usages: TextureUsages::all(),
            flags: TextureFormatFeatureFlags::all(),
        }
    }

    fn caps<'a>(
        limits: &'a WgpuLimits,
        format: &'a dyn Fn(TextureFormat) -> TextureFormatFeatures,
    ) -> DeviceCaps<'a> {
        DeviceCaps {
            vulkan: true,
            view_formats: true,
            features: WgpuFeatures::all(),
            limits,
            format,
        }
    }

    #[test]
    fn a_capable_vulkan_device_runs_both() {
        let limits = WgpuLimits::default();
        // (WebGPU's default limits are 4 storage textures; FSR 3.1 needs 8, so
        // the adapter's own limits, which bevy asks for, are what count.)
        let mut adapter = limits.clone();
        adapter.max_storage_textures_per_shader_stage = 16;
        let c = caps(&adapter, &everything);
        assert!(fsr1_issues(&c).is_empty(), "{:?}", fsr1_issues(&c));
        assert!(fsr3_issues(&c).is_empty(), "{:?}", fsr3_issues(&c));
    }

    #[test]
    fn the_webgpu_default_limits_are_too_few_storage_textures_for_fsr3() {
        let limits = WgpuLimits::default();
        let c = caps(&limits, &everything);
        let issues = fsr3_issues(&c);
        assert!(
            issues
                .iter()
                .any(|i| i.contains("max_storage_textures_per_shader_stage")),
            "{issues:?}"
        );
        assert!(fsr1_issues(&c).is_empty());
    }

    #[test]
    fn missing_pieces_are_named() {
        let mut limits = WgpuLimits::default();
        limits.max_storage_textures_per_shader_stage = 16;
        // Not Vulkan, no view formats, no adapter-specific format features.
        let mut c = caps(&limits, &everything);
        c.vulkan = false;
        c.view_formats = false;
        c.features = WgpuFeatures::empty();
        let fsr3 = fsr3_issues(&c).join("; ");
        assert!(fsr3.contains("Vulkan"), "{fsr3}");
        assert!(
            fsr3.contains("TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES"),
            "{fsr3}"
        );
        assert!(fsr3.contains("TEXTURE_FORMAT_16BIT_NORM"), "{fsr3}");
        assert!(fsr1_issues(&c).join(";").contains("view formats"));

        // A format that cannot be a storage texture, or only written.
        let write_only = |f: TextureFormat| TextureFormatFeatures {
            allowed_usages: TextureUsages::all(),
            flags: if f == TextureFormat::R16Float {
                TextureFormatFeatureFlags::empty()
            } else {
                TextureFormatFeatureFlags::all()
            },
        };
        let c = caps(&limits, &write_only);
        let issues = fsr3_issues(&c);
        assert!(
            issues
                .iter()
                .any(|i| i.contains("R16Float") && i.contains("read-write")),
            "{issues:?}"
        );
        let no_srgb_target = |f: TextureFormat| TextureFormatFeatures {
            allowed_usages: if f == TextureFormat::Rgba8UnormSrgb {
                TextureUsages::TEXTURE_BINDING
            } else {
                TextureUsages::all()
            },
            flags: TextureFormatFeatureFlags::all(),
        };
        let c = caps(&limits, &no_srgb_target);
        assert!(!fsr1_issues(&c).is_empty());
    }
}

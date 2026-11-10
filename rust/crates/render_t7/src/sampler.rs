//! A sampler state (14 bits: a material's sampler def, `& 0x3fff`) as
//! BlackOps3.exe turns it into a `D3D11_SAMPLER_DESC`, mapped to wgpu.
//!
//! ```text
//! bits 0..2   filter level: filter = table[level * 2 + bit 4]
//!             (point, point+linear mip, linear, linear, anisotropic x4..,
//!             anisotropic, comparison anisotropic)
//! bits 3..4   mips: max LOD 0 when both clear, else FLT_MAX
//! bits 5..6, 7..8, 9..10  address U, V, W: wrap, clamp, border, mirror
//! bit 11      border colour opaque white (else transparent black)
//! ```
//! Maximum anisotropy: a table by the texture-filter quality setting (rows)
//! and the filter level: quality 0 gives 2, quality 2 gives 16, quality 1
//! (`ANISO_QUALITY`) gives 2, 4, 8, 16 for levels 3..6; levels 0..2 none.
//! LOD bias 0, min LOD 0, comparison GREATER (only comparison filters use it).

/// The exe's filter table (D3D11_FILTER), by `level * 2 + bit 4`.
const FILTER: [u32; 16] = [0, 1, 0, 1, 0x15, 0x15, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x95, 0x95];
/// The exe's anisotropy rows (texture-filter quality 0, 1, 2) by filter level
/// (level 7's entry is never written in the exe; level 7 is the comparison
/// filter: taken as level 6's).
const ANISO: [[u16; 8]; 3] = [[0, 0, 0, 2, 2, 2, 2, 2], [0, 0, 0, 2, 4, 8, 16, 16], [0, 0, 0, 16, 16, 16, 16, 16]];
/// The texture-filter quality row used for the code passes (the exe reads
/// it from a setting; 1 is what this renderer always drew with).
pub const ANISO_QUALITY: usize = 1;

/// The texture-filter quality row the materials' samplers take: the exe's
/// texture-filter dvar (0 low: x2 at most, 1: x2..x16 by the state's level,
/// 2 high: x16 on every anisotropic state). `BO2MP_T7_TEX_FILTER=0|1|2`,
/// else the settings (`crate::quality`: row 1 at ultra, high and medium,
/// as before; 0 when the shared anisotropy is off). Read as a map's
/// samplers are made.
pub fn material_quality() -> usize {
    match std::env::var("BO2MP_T7_TEX_FILTER").ok().and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(q) => q.min(2),
        None => crate::quality::get().tex_filter.min(2),
    }
}

fn address(i: u32) -> wgpu::AddressMode {
    match i & 3 {
        0 => wgpu::AddressMode::Repeat,
        1 => wgpu::AddressMode::ClampToEdge,
        2 => wgpu::AddressMode::ClampToBorder,
        _ => wgpu::AddressMode::MirrorRepeat,
    }
}

/// The sampler descriptor of a state; `border_ok`: the device can clamp to
/// a border colour (else such a state is an error, not a stand-in).
pub fn descriptor(state: u16, quality: usize, border_ok: bool) -> Result<wgpu::SamplerDescriptor<'static>, String> {
    let s = u32::from(state & 0x3fff);
    let level = (s & 7) as usize;
    let filter = FILTER[level * 2 + ((s >> 4) & 1) as usize];
    let (mag, min, mip, cmp) = match filter {
        0 => (wgpu::FilterMode::Nearest, wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, false),
        1 => (wgpu::FilterMode::Nearest, wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Linear, false),
        0x15 | 0x55 => (wgpu::FilterMode::Linear, wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, false),
        _ => (wgpu::FilterMode::Linear, wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, true),
    };
    let aniso = if filter == 0x55 || filter == 0x95 { ANISO[quality.min(2)][level].max(1) } else { 1 };
    let modes = [address(s >> 5), address(s >> 7), address(s >> 9)];
    let border = modes.contains(&wgpu::AddressMode::ClampToBorder);
    if border && !border_ok {
        return Err(format!("sampler {state:#x}: border addressing needs ADDRESS_MODE_CLAMP_TO_BORDER"));
    }
    Ok(wgpu::SamplerDescriptor {
        label: Some("t7 sampler"),
        address_mode_u: modes[0],
        address_mode_v: modes[1],
        address_mode_w: modes[2],
        mag_filter: mag,
        min_filter: min,
        mipmap_filter: mip,
        lod_min_clamp: 0.0,
        lod_max_clamp: if s & 0x18 == 0 { 0.0 } else { f32::MAX },
        compare: cmp.then_some(wgpu::CompareFunction::Greater),
        anisotropy_clamp: aniso,
        border_color: border.then_some(if s & 0x800 != 0 { wgpu::SamplerBorderColor::OpaqueWhite } else { wgpu::SamplerBorderColor::TransparentBlack }),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn nuketown_states() {
        // 0x13: anisotropic (level 3, quality 1: x2), mips, wrap.
        let d = super::descriptor(0x13, 1, false).unwrap();
        assert_eq!(d.anisotropy_clamp, 2);
        assert_eq!(super::descriptor(0x13, 0, true).unwrap().anisotropy_clamp, 2);
        assert_eq!(super::descriptor(0x13, 2, true).unwrap().anisotropy_clamp, 16);
        assert_eq!(d.address_mode_u, wgpu::AddressMode::Repeat);
        assert!(d.lod_max_clamp > 1e30);
        // 0x4002: linear, no mips.
        let d = super::descriptor(0x4002, 1, false).unwrap();
        assert_eq!(d.lod_max_clamp, 0.0);
        assert_eq!(d.mag_filter, wgpu::FilterMode::Linear);
    }
}

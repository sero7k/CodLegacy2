//! A technique's state map (`GfxStateMap`, 112 bytes) as BlackOps3.exe turns
//! it into Direct3D 11 blend, depth-stencil and rasterizer states, mapped to
//! wgpu.
//!
//! ```text
//! 0x00 u32 blend[8]     per render target (one for all unless raster bit 0):
//!                       bits 2..5 src (factor table), 6..8 op, 9..12 dest,
//!                       13..16 src alpha, 17..19 alpha op, 20..23 dest alpha,
//!                       byte 3 low nibble the write mask; colour blending on
//!                       when bits 6..8 are set, alpha blending when 17..19 are
//!                       (off: colour ZERO/ONE/ADD (the target kept), alpha
//!                       likewise)
//! 0x20 u32 depth        bit 0 write; bits 1..3 test (table: always, always,
//!                       greater, equal, greater-equal, less); test on when 1..3
//!                       are not all 0
//! 0x24 u32 stencil front, 0x28 back: bit 0 on (either side: stencil on, masks
//!                       0xff), 1..3 pass op, 4..6 fail op, 7..9 depth-fail op,
//!                       10..12 test (op and test tables 1..8 as D3D numbers them)
//! 0x2c u32 raster       bit 0 independent blend, 1..2 cull (none, back, front),
//!                       3..4 depth bias (n: 250 n units, slope 2 n, clamp n
//!                       1e-6; 3: -4, slope -4, clamp -0.1), 5 wireframe
//! 0x30 u8  stencil reference
//! ```
//! Front faces wind clockwise (D3D's default, which the exe keeps), scissor
//! on, depth clip on (the variant ordinary draws use).

/// The exe's blend factor table, by the state map's 4-bit index (D3D11_BLEND).
const BLEND_FACTOR: [u32; 16] = [1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 19, 1, 3, 2, 0];
/// The exe's depth test table, by the 3-bit index (D3D11_COMPARISON_FUNC).
const DEPTH_FUNC: [u32; 8] = [8, 8, 5, 3, 7, 2, 0, 0];
/// The exe's cull table (D3D11_CULL_MODE).
const CULL: [u32; 4] = [1, 3, 2, 0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateMap {
    pub blend: [u32; 8],
    pub depth: u32,
    pub stencil: [u32; 2],
    pub raster: u32,
    pub stencil_ref: u8,
}

impl StateMap {
    pub fn read(b: &[u8]) -> Option<Self> {
        let w = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        Some(Self {
            blend: std::array::from_fn(|i| w(4 * i).unwrap_or(0)),
            depth: w(0x20)?,
            stencil: [w(0x24)?, w(0x28)?],
            raster: w(0x2c)?,
            stencil_ref: *b.get(0x30)?,
        })
    }
}

fn factor(d3d: u32) -> Result<wgpu::BlendFactor, String> {
    use wgpu::BlendFactor as F;
    Ok(match d3d {
        1 => F::Zero,
        2 => F::One,
        3 => F::Src,
        4 => F::OneMinusSrc,
        5 => F::SrcAlpha,
        6 => F::OneMinusSrcAlpha,
        7 => F::DstAlpha,
        8 => F::OneMinusDstAlpha,
        9 => F::Dst,
        10 => F::OneMinusDst,
        11 => F::SrcAlphaSaturated,
        14 => F::Constant,
        15 => F::OneMinusConstant,
        16 => F::Src1,
        17 => F::OneMinusSrc1,
        18 => F::Src1Alpha,
        19 => F::OneMinusSrc1Alpha,
        _ => return Err(format!("blend factor {d3d}")),
    })
}

fn op(d3d: u32) -> Result<wgpu::BlendOperation, String> {
    use wgpu::BlendOperation as O;
    Ok(match d3d {
        1 => O::Add,
        2 => O::Subtract,
        3 => O::ReverseSubtract,
        4 => O::Min,
        5 => O::Max,
        _ => return Err(format!("blend op {d3d}")),
    })
}

fn compare(d3d: u32) -> Result<wgpu::CompareFunction, String> {
    use wgpu::CompareFunction as C;
    Ok(match d3d {
        1 => C::Never,
        2 => C::Less,
        3 => C::Equal,
        4 => C::LessEqual,
        5 => C::Greater,
        6 => C::NotEqual,
        7 => C::GreaterEqual,
        8 => C::Always,
        _ => return Err(format!("comparison {d3d}")),
    })
}

fn stencil_op(i: u32) -> wgpu::StencilOperation {
    use wgpu::StencilOperation as S;
    // The exe's table maps 0..7 to D3D's 1..8.
    match i {
        0 => S::Keep,
        1 => S::Zero,
        2 => S::Replace,
        3 => S::IncrementClamp,
        4 => S::DecrementClamp,
        5 => S::Invert,
        6 => S::IncrementWrap,
        _ => S::DecrementWrap,
    }
}

/// One render target's blend (None: blending off) and write mask.
pub fn target_blend(word: u32) -> Result<(Option<wgpu::BlendState>, wgpu::ColorWrites), String> {
    let colour_on = word & 0x1c0 != 0;
    let alpha_on = word & 0xe0000 != 0;
    let mask = wgpu::ColorWrites::from_bits_truncate((word >> 24) & 0xf);
    if !colour_on && !alpha_on {
        return Ok((None, mask));
    }
    let f = |shift: u32| factor(BLEND_FACTOR[((word >> shift) & 0xf) as usize]);
    let color = if colour_on {
        wgpu::BlendComponent { src_factor: f(2)?, dst_factor: f(9)?, operation: op((word >> 6) & 7)? }
    } else {
        wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }
    };
    let alpha = if alpha_on {
        wgpu::BlendComponent { src_factor: f(13)?, dst_factor: f(20)?, operation: op((word >> 17) & 7)? }
    } else {
        wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }
    };
    Ok((Some(wgpu::BlendState { color, alpha }), mask))
}

/// The colour targets' states for `formats` (render target `i` takes blend
/// word `i` with independent blending, else word 0).
pub fn targets(s: &StateMap, formats: &[wgpu::TextureFormat]) -> Result<Vec<Option<wgpu::ColorTargetState>>, String> {
    formats
        .iter()
        .enumerate()
        .map(|(i, &format)| {
            let word = if s.raster & 1 != 0 { s.blend[i] } else { s.blend[0] };
            let (blend, write_mask) = target_blend(word)?;
            Ok(Some(wgpu::ColorTargetState { format, blend, write_mask }))
        })
        .collect()
}

pub fn depth_stencil(s: &StateMap, format: wgpu::TextureFormat) -> Result<wgpu::DepthStencilState, String> {
    let test_on = s.depth & 0xe != 0;
    let depth_compare = if test_on { compare(DEPTH_FUNC[((s.depth >> 1) & 7) as usize])? } else { wgpu::CompareFunction::Always };
    let face = |w: u32| -> Result<wgpu::StencilFaceState, String> {
        Ok(wgpu::StencilFaceState {
            compare: compare(((w >> 10) & 7) + 1)?,
            fail_op: stencil_op((w >> 4) & 7),
            depth_fail_op: stencil_op((w >> 7) & 7),
            pass_op: stencil_op((w >> 1) & 7),
        })
    };
    let stencil_on = (s.stencil[0] | s.stencil[1]) & 1 != 0;
    let stencil = if stencil_on {
        wgpu::StencilState { front: face(s.stencil[0])?, back: face(s.stencil[1])?, read_mask: 0xff, write_mask: 0xff }
    } else {
        wgpu::StencilState::default()
    };
    // D3D: the depth write mask only writes when the test is on.
    let depth_write_enabled = s.depth & 1 != 0 && test_on;
    let n = (s.raster >> 3) & 3;
    let bias = match n {
        0 => wgpu::DepthBiasState::default(),
        3 => wgpu::DepthBiasState { constant: -4, slope_scale: -4.0, clamp: -0.1 },
        n => wgpu::DepthBiasState { constant: 250 * n as i32, slope_scale: 2.0 * n as f32, clamp: n as f32 * 1e-6 },
    };
    Ok(wgpu::DepthStencilState { format, depth_write_enabled: Some(depth_write_enabled), depth_compare: Some(depth_compare), stencil, bias })
}

pub fn primitive(s: &StateMap) -> Result<wgpu::PrimitiveState, String> {
    let cull_mode = match CULL[((s.raster >> 1) & 3) as usize] {
        1 => None,
        3 => Some(wgpu::Face::Back),
        2 => Some(wgpu::Face::Front),
        c => return Err(format!("cull mode {c}")),
    };
    Ok(wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Cw,
        cull_mode,
        polygon_mode: if s.raster & 0x20 != 0 { wgpu::PolygonMode::Line } else { wgpu::PolygonMode::Fill },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nuketown_opaque_and_decal() {
        // wc/lit's map: one target word 0x0f104209, depth 0x09, raster 0.
        let mut b = [0u8; 112];
        b[0..4].copy_from_slice(&0x0f10_4209u32.to_le_bytes());
        b[0x20] = 9;
        let s = StateMap::read(&b).unwrap();
        let (blend, mask) = target_blend(s.blend[0]).unwrap();
        assert!(blend.is_none());
        assert_eq!(mask, wgpu::ColorWrites::ALL);
        let d = depth_stencil(&s, wgpu::TextureFormat::Depth32FloatStencil8).unwrap();
        assert_eq!(d.depth_compare, Some(wgpu::CompareFunction::GreaterEqual));
        assert_eq!(d.depth_write_enabled, Some(true));
        // A decal: ONE, INV_SRC_ALPHA, write RGB; depth test without write; bias 1.
        let (blend, mask) = target_blend(0x0710_4c49).unwrap();
        let blend = blend.unwrap();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.alpha.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(mask, wgpu::ColorWrites::COLOR);
        let mut b2 = b;
        b2[0x20] = 8;
        b2[0x2c] = 0x0b;
        let s2 = StateMap::read(&b2).unwrap();
        let d2 = depth_stencil(&s2, wgpu::TextureFormat::Depth32FloatStencil8).unwrap();
        assert_eq!(d2.depth_write_enabled, Some(false));
        assert_eq!(d2.bias.constant, 250);
        assert_eq!(primitive(&s2).unwrap().cull_mode, Some(wgpu::Face::Back));
    }
}

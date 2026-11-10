//! A pass's fixed-function state from the material's `GfxStateBits`:
//! cull mode, depth test and write, polygon offset and blending
//! (`loadBits[0]` cull, `[1]` depth/stencil, `[3]` blend). Alpha testing is
//! the shaders' own (`discard`) under D3D11.

/// The decoded state, as a pipeline key and descriptor parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PassState {
    pub cull: u8,
    pub depth_write: bool,
    /// GFXS1 depth test field (0 always, 1 less, 2 equal, 3 lessequal,
    /// 4 greater, 5 greaterequal, 6 disabled).
    pub depth_test: u8,
    pub polygon_offset: u8,
    /// GFXS3 blend fields (src, dst, op for rgb then alpha; 0 op = off).
    pub blend: [u8; 6],
    pub write_mask: u8,
}

impl Default for PassState {
    fn default() -> Self {
        Self {
            cull: 2,
            depth_write: true,
            depth_test: 3,
            polygon_offset: 0,
            blend: [0; 6],
            write_mask: 0xf,
        }
    }
}

impl PassState {
    pub fn from_bits(bits: &[u8]) -> Self {
        let w = |k: usize| {
            u32::from_le_bytes([
                bits[4 * k],
                bits[4 * k + 1],
                bits[4 * k + 2],
                bits[4 * k + 3],
            ])
        };
        let (b0, b1, b3) = (w(0), w(1), w(3));
        Self {
            cull: ((b0 >> 3) & 3) as u8,
            depth_write: b1 & 1 != 0,
            depth_test: ((b1 >> 1) & 7) as u8,
            polygon_offset: ((b1 >> 4) & 3) as u8,
            blend: [
                (b3 & 0xf) as u8,
                ((b3 >> 4) & 0xf) as u8,
                ((b3 >> 8) & 7) as u8,
                ((b3 >> 12) & 0xf) as u8,
                ((b3 >> 16) & 0xf) as u8,
                ((b3 >> 20) & 7) as u8,
            ],
            write_mask: ((b3 >> 24) & 0xf) as u8,
        }
    }

    pub fn cull_mode(&self) -> Option<wgpu::Face> {
        if std::env::var("H1_DBG_CULL").is_ok_and(|v| v == "none") {
            return None;
        }
        match self.cull {
            2 => Some(wgpu::Face::Back),
            3 => Some(wgpu::Face::Front),
            _ => None,
        }
    }

    /// The game's depth test for our reversed depth (near is 1).
    pub fn depth_compare(&self) -> wgpu::CompareFunction {
        use wgpu::CompareFunction as C;
        if std::env::var("H1_DBG_DEPTH").is_ok_and(|v| v == "always") {
            return C::Always;
        }
        match self.depth_test {
            0 | 6 => C::Always,
            1 => C::Greater,
            2 if std::env::var("H1_DBG_DEPTH").is_ok_and(|v| v == "ge") => C::GreaterEqual,
            2 => C::Equal,
            3 => C::GreaterEqual,
            4 => C::Less,
            5 => C::LessEqual,
            _ => C::GreaterEqual,
        }
    }

    /// The shadow map's depth bias. Casters' `build_shadowmap` states carry
    /// polygon offset 3, the engine's shadow map mode, which its rasteriser
    /// states turn into a depth bias of -128 and a slope bias of -4 (its
    /// shadow offset dvars' defaults, negated), clamp 0: away from the sun
    /// in the reversed shadow depth.
    pub fn shadow_bias(&self) -> wgpu::DepthBiasState {
        match self.polygon_offset {
            0 => wgpu::DepthBiasState::default(),
            _ => wgpu::DepthBiasState {
                constant: crate::sun_shadow::CASTER_DEPTH_BIAS,
                slope_scale: crate::sun_shadow::CASTER_SLOPE_BIAS,
                clamp: crate::sun_shadow::CASTER_BIAS_CLAMP,
            },
        }
    }

    /// Depth bias for the polygon offset settings (decals sit on walls).
    pub fn depth_bias(&self) -> wgpu::DepthBiasState {
        match self.polygon_offset {
            0 => wgpu::DepthBiasState::default(),
            n => wgpu::DepthBiasState {
                constant: i32::from(n) * 2,
                slope_scale: f32::from(n),
                clamp: 0.0,
            },
        }
    }

    fn factor(f: u8) -> wgpu::BlendFactor {
        use wgpu::BlendFactor as F;
        match f {
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
            _ => F::One,
        }
    }

    fn op(o: u8) -> wgpu::BlendOperation {
        use wgpu::BlendOperation as O;
        match o {
            2 => O::Subtract,
            3 => O::ReverseSubtract,
            4 => O::Min,
            5 => O::Max,
            _ => O::Add,
        }
    }

    pub fn blend_state(&self) -> Option<wgpu::BlendState> {
        let [sc, dc, oc, sa, da, oa] = self.blend;
        if oc == 0 && oa == 0 {
            return None;
        }
        let comp = |s, d, o| {
            if o == 0 {
                wgpu::BlendComponent::REPLACE
            } else {
                let (mut s, mut d) = (Self::factor(s), Self::factor(d));
                // Min and max take no factors.
                if matches!(o, 4 | 5) {
                    s = wgpu::BlendFactor::One;
                    d = wgpu::BlendFactor::One;
                }
                wgpu::BlendComponent {
                    src_factor: s,
                    dst_factor: d,
                    operation: Self::op(o),
                }
            }
        };
        Some(wgpu::BlendState {
            color: comp(sc, dc, oc),
            alpha: comp(sa, da, oa),
        })
    }

    pub fn color_writes(&self) -> wgpu::ColorWrites {
        wgpu::ColorWrites::from_bits_truncate(u32::from(self.write_mask))
    }

    /// Whether an opaque surface's own pass has to write depth because no
    /// depth pre-pass is drawn for it (`prepassed` false). The lit passes
    /// of opaque materials test less-equal and write no depth: the game
    /// fills the depth first in the `zprepass` technique. A material whose
    /// set has no pre-pass technique, or whose pre-pass could not be made,
    /// then leaves no depth: the fog pass reads its pixels as sky and
    /// anything drawn after it paints over it, so the prop looks
    /// see-through. Such a pass writes depth itself (nearest wins). Passes
    /// that do not really test depth (the sky, `always`), decals (polygon
    /// offset) and colourless passes are left alone.
    pub fn needs_own_depth(&self, opaque: bool, prepassed: bool) -> bool {
        opaque
            && !prepassed
            && !self.depth_write
            && matches!(self.depth_test, 1..=3)
            && self.polygon_offset == 0
            && self.write_mask != 0
    }

    /// The pass with the depth it has to write itself (`needs_own_depth`):
    /// depth written, and an `equal` test (made for a pre-pass that is not
    /// there) relaxed to less-equal.
    pub fn with_own_depth(mut self) -> Self {
        self.depth_write = true;
        if self.depth_test == 2 {
            self.depth_test = 3;
        }
        self
    }
}

/// `BO2MP_H1_OWN_DEPTH=0`: an opaque pass with no pre-pass writes no depth,
/// as before.
pub fn own_depth_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_H1_OWN_DEPTH").is_ok_and(|v| v.trim() == "0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit() -> PassState {
        PassState { depth_write: false, depth_test: 3, ..PassState::default() }
    }

    #[test]
    fn an_opaque_lit_pass_without_a_prepass_writes_its_own_depth() {
        assert!(lit().needs_own_depth(true, false));
        let s = lit().with_own_depth();
        assert!(s.depth_write);
        assert_eq!(s.depth_test, 3);
    }

    #[test]
    fn a_prepassed_blended_or_writing_pass_is_left_alone() {
        assert!(!lit().needs_own_depth(true, true));
        assert!(!lit().needs_own_depth(false, false));
        assert!(!PassState::default().needs_own_depth(true, false));
    }

    #[test]
    fn sky_decal_and_colourless_passes_are_left_alone() {
        assert!(!PassState { depth_test: 0, ..lit() }.needs_own_depth(true, false));
        assert!(!PassState { depth_test: 6, ..lit() }.needs_own_depth(true, false));
        assert!(!PassState { polygon_offset: 2, ..lit() }.needs_own_depth(true, false));
        assert!(!PassState { write_mask: 0, ..lit() }.needs_own_depth(true, false));
    }

    #[test]
    fn an_equal_test_made_for_a_missing_prepass_relaxes_to_less_equal() {
        let s = PassState { depth_test: 2, ..lit() };
        assert!(s.needs_own_depth(true, false));
        assert_eq!(s.with_own_depth().depth_test, 3);
    }
}

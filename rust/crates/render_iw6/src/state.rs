//! A pass's fixed-function state from the material's `GfxStateBits`
//! (28 bytes: three load words, then the zone and the D3D11 state object
//! indices). Read from mp_frag's materials (each technique slot's block
//! against what the slot is): word 0 holds the cull mode (bits 3..4: 1
//! none, 2 back, 3 front), word 1 the depth state in the IW engines'
//! layout (bit 0 write, bit 1 no test, bits 2..3 the test: 0 always,
//! 1 less, 2 equal, 3 less-equal; bits 4..5 the polygon offset, 3 the
//! shadow map's), word 2 the blend (colour source / destination / op in
//! bits 0..3, 4..7, 8..10, alpha in 12..15, 16..19, 20..22, the colour
//! write mask in 24..27): the depth pre-pass writes no colour, the lit
//! passes after it test less-equal without writing depth. Alpha testing
//! is the pixel shader's own (`discard`) under D3D11.

/// The decoded state, as a pipeline key and descriptor parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PassState {
    /// 1 none, 2 back, 3 front.
    pub cull: u8,
    pub depth_write: bool,
    /// None: no test; else 0 always, 1 less, 2 equal, 3 less-equal.
    pub depth_test: Option<u8>,
    pub polygon_offset: u8,
    /// Blend fields (src, dst, op for colour, then alpha; op 0 = off).
    pub blend: [u8; 6],
    pub write_rgb: bool,
    pub write_alpha: bool,
}

impl Default for PassState {
    fn default() -> Self {
        Self {
            cull: 2,
            depth_write: true,
            depth_test: Some(3),
            polygon_offset: 0,
            blend: [0; 6],
            write_rgb: true,
            write_alpha: true,
        }
    }
}

impl PassState {
    pub fn from_bits(bits: &[u8]) -> Self {
        let w = |k: usize| {
            bits.get(4 * k..4 * k + 4)
                .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let (b0, b1, b2) = (w(0), w(1), w(2));
        let mask = (b2 >> 24) & 0xf;
        Self {
            cull: ((b0 >> 3) & 3) as u8,
            depth_write: b1 & 1 != 0,
            depth_test: (b1 & 2 == 0).then_some(((b1 >> 2) & 3) as u8),
            polygon_offset: ((b1 >> 4) & 3) as u8,
            blend: [
                (b2 & 0xf) as u8,
                ((b2 >> 4) & 0xf) as u8,
                ((b2 >> 8) & 7) as u8,
                ((b2 >> 12) & 0xf) as u8,
                ((b2 >> 16) & 0xf) as u8,
                ((b2 >> 20) & 7) as u8,
            ],
            write_rgb: mask & 7 != 0,
            write_alpha: mask & 8 != 0,
        }
    }

    pub fn cull_mode(&self) -> Option<wgpu::Face> {
        // IW6_DBG_CULL=none: nothing culled (debug).
        if std::env::var("IW6_DBG_CULL").is_ok_and(|v| v == "none") {
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
        match self.depth_test {
            None | Some(0) => C::Always,
            Some(1) => C::Greater,
            Some(2) => C::Equal,
            _ => C::GreaterEqual,
        }
    }

    /// Depth bias for the polygon offset settings (decals sit on walls),
    /// toward the eye in the reversed depth.
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

    /// The sun shadow casters' bias, away from the sun in the shadow map's
    /// depth (0 nearest the sun). The engine's offsets (128 constant, 2
    /// slope) are for its own depth format and range; here a 32-bit float
    /// depth over the world's whole depth along the sun: about 2 units at
    /// mp_frag's range (`BO2MP_IW6_SHADOW_BIAS=constant,slope`). With the
    /// engine's 128 and 2 here the snow shows acne rings (checked by eye on
    /// mp_frag, 2026-11-03), so they are used for the spot maps only
    /// (perspective depth from a near plane, `spot_shadow_bias`).
    pub fn shadow_bias(&self) -> wgpu::DepthBiasState {
        let (constant, slope_scale) = std::env::var("BO2MP_IW6_SHADOW_BIAS")
            .ok()
            .and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .unwrap_or((4096, 3.0));
        wgpu::DepthBiasState {
            constant,
            slope_scale,
            clamp: 0.0,
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

    /// Whether the pass blends (op 1 = add with factors that are not
    /// one/zero counts too).
    pub fn blend_state(&self) -> Option<wgpu::BlendState> {
        let [sc, dc, oc, sa, da, oa] = self.blend;
        let plain = |s: u8, d: u8, o: u8| o == 0 || (o == 1 && s == 2 && d == 1);
        if plain(sc, dc, oc) && plain(sa, da, oa) {
            return None;
        }
        let comp = |s, d, o| {
            if plain(s, d, o) {
                wgpu::BlendComponent::REPLACE
            } else {
                let (mut s, mut d) = (Self::factor(s), Self::factor(d));
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
        let mut w = wgpu::ColorWrites::empty();
        if self.write_rgb {
            w |= wgpu::ColorWrites::COLOR;
        }
        if self.write_alpha {
            w |= wgpu::ColorWrites::ALPHA;
        }
        w
    }

    /// Whether an opaque surface's own pass has to write depth because no
    /// depth pre-pass was drawn for it (`prepassed` false): the lit passes
    /// test less-equal without writing depth, trusting the `zprepass`
    /// technique to have filled it. A surface whose set has no pre-pass
    /// technique, or whose pre-pass pipeline could not be made, then leaves
    /// no depth, and whatever draws after it paints over it. Passes that
    /// do not test depth, decals (polygon offset) and colourless passes
    /// are left alone.
    pub fn needs_own_depth(&self, opaque: bool, prepassed: bool) -> bool {
        opaque
            && !prepassed
            && !self.depth_write
            && matches!(self.depth_test, Some(1..=3))
            && self.polygon_offset == 0
            && self.write_rgb
    }

    /// The pass with the depth it has to write itself: depth written, an
    /// `equal` test (made for a pre-pass that is not there) relaxed to
    /// less-equal.
    pub fn with_own_depth(mut self) -> Self {
        self.depth_write = true;
        if self.depth_test == Some(2) {
            self.depth_test = Some(3);
        }
        self
    }
}

/// `BO2MP_IW6_OWN_DEPTH=0`: an opaque pass with no pre-pass writes no
/// depth, as before.
pub fn own_depth_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_OWN_DEPTH").is_ok_and(|v| v.trim() == "0"))
}

/// The spot shadow casters' depth offset: the engine's shadow raster state,
/// `sm_polygonOffsetBias` 128 and `sm_polygonOffsetScale` 2 (clamp
/// `sm_polygonOffsetClamp` 0), read from the dvars; `BO2MP_IW6_SPOT_SHADOW_BIAS=c,s`
/// overrides.
pub fn spot_shadow_bias() -> wgpu::DepthBiasState {
    let (constant, slope_scale) = std::env::var("BO2MP_IW6_SPOT_SHADOW_BIAS")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .unwrap_or((128, 2.0));
    wgpu::DepthBiasState {
        constant,
        slope_scale,
        clamp: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mp_frag_blocks() {
        let block = |w0: u32, w1: u32, w2: u32| {
            let mut b = [0u8; 28];
            b[..4].copy_from_slice(&w0.to_le_bytes());
            b[4..8].copy_from_slice(&w1.to_le_bytes());
            b[8..12].copy_from_slice(&w2.to_le_bytes());
            PassState::from_bits(&b)
        };
        // The depth pre-pass: back culled, depth written, less-equal, no colour.
        let s = block(0x11, 0xe49e_490d, 0x0001_2012);
        assert_eq!(s.cull_mode(), Some(wgpu::Face::Back));
        assert!(s.depth_write);
        assert_eq!(s.depth_compare(), wgpu::CompareFunction::GreaterEqual);
        assert!(s.color_writes().is_empty());
        // The shadow casters: not culled, the shadow map's offset.
        let s = block(0x09, 0x3d, 0x0001_2012);
        assert_eq!(s.cull_mode(), None);
        assert_eq!(s.polygon_offset, 3);
        // The lit pass: no depth write, every colour channel, no blend.
        let s = block(0x51, 0x0c, 0x1f01_2012);
        assert!(!s.depth_write);
        assert_eq!(s.color_writes(), wgpu::ColorWrites::ALL);
        assert!(s.blend_state().is_none());
        // An additive pass (one, one, add).
        let s = block(0x51, 0xe004_006c, 0x0f01_2122);
        let b = s.blend_state().expect("blend");
        assert_eq!(b.color.src_factor, wgpu::BlendFactor::One);
        assert_eq!(b.color.dst_factor, wgpu::BlendFactor::One);
    }

    #[test]
    fn an_opaque_lit_pass_with_no_prepass_writes_its_own_depth() {
        let lit = PassState { depth_write: false, depth_test: Some(3), ..PassState::default() };
        assert!(lit.needs_own_depth(true, false));
        assert!(!lit.needs_own_depth(true, true));
        assert!(!lit.needs_own_depth(false, false));
        assert!(!PassState::default().needs_own_depth(true, false));
        assert!(!PassState { depth_test: None, ..lit }.needs_own_depth(true, false));
        assert!(!PassState { depth_test: Some(0), ..lit }.needs_own_depth(true, false));
        assert!(!PassState { polygon_offset: 1, ..lit }.needs_own_depth(true, false));
        assert!(!PassState { write_rgb: false, ..lit }.needs_own_depth(true, false));
        let s = PassState { depth_test: Some(2), ..lit }.with_own_depth();
        assert!(s.depth_write);
        assert_eq!(s.depth_test, Some(3));
        assert_eq!(lit.with_own_depth().depth_test, Some(3));
    }
}

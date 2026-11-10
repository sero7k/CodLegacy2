//! A pass's fixed-function state from the material's `GfxStateBits`
//! (`Material::stateBitsTable`, chosen per technique slot by
//! `stateBitsEntry`): its six load words read as MWR's are
//! (`render_h1::state::PassState`), except that IW7 keeps the blend word
//! in the third (word 2) where MWR keeps it in the fourth.
//!
//! Read on riot's materials (slot 8): opaque `replace` sets blend
//! `0x0f012012` (one, zero, off), `w_unlit_blend` / `add` `0x0f062162`
//! (one, inverse source alpha, add), `multiply` `0x0f012131` (zero, source
//! colour, add), lit decals `0x0f012162` / `0x0f012165`; word 1 bit 0 is
//! the depth write (clear on every blended set), bits 4-5 the polygon
//! offset (decals). Word 1's depth test bits read 6 on every set, which
//! MWR's table calls "disabled": not taken; every pass tests depth.

use render_h1::state::PassState;

pub fn from_bits(bits: &[u8]) -> PassState {
    let w = |k: usize| u32::from_le_bytes([bits[4 * k], bits[4 * k + 1], bits[4 * k + 2], bits[4 * k + 3]]);
    let (b0, b1, b2) = (w(0), w(1), w(2));
    PassState {
        cull: ((b0 >> 3) & 3) as u8,
        depth_write: b1 & 1 != 0,
        // greater-or-equal in the reversed depth
        depth_test: 3,
        polygon_offset: ((b1 >> 4) & 3) as u8,
        blend: [(b2 & 0xf) as u8, ((b2 >> 4) & 0xf) as u8, ((b2 >> 8) & 7) as u8, ((b2 >> 12) & 0xf) as u8, ((b2 >> 16) & 0xf) as u8, ((b2 >> 20) & 7) as u8],
        write_mask: match ((b2 >> 24) & 0xf) as u8 {
            0 => 0xf,
            m => m,
        },
    }
}

/// A material's state for technique slot `slot` (opaque defaults when it
/// has none).
pub fn of_material(m: &fastfile_iw7::map::MapMaterial, slot: usize) -> PassState {
    m.state_bits_entry
        .get(slot)
        .and_then(|&e| m.state_bits.get(usize::from(e)))
        .map_or_else(PassState::default, |b| from_bits(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(w: [u32; 3]) -> Vec<u8> {
        let mut b: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        b.resize(44, 0);
        b
    }

    #[test]
    fn riot_states_read_as_drawn() {
        let opaque = from_bits(&bits([0x51, 0xd, 0x0f01_2012]));
        assert!(opaque.blend_state().is_none() && opaque.depth_write);
        assert_eq!(opaque.cull_mode(), Some(wgpu::Face::Back));
        let blend = from_bits(&bits([0x51, 0x1c, 0x0f06_2162]));
        let b = blend.blend_state().unwrap();
        assert_eq!((b.color.src_factor, b.color.dst_factor), (wgpu::BlendFactor::One, wgpu::BlendFactor::OneMinusSrcAlpha));
        assert!(!blend.depth_write && blend.polygon_offset == 1);
        let mul = from_bits(&bits([0x51, 0xc, 0x0f01_2131])).blend_state().unwrap();
        assert_eq!((mul.color.src_factor, mul.color.dst_factor), (wgpu::BlendFactor::Zero, wgpu::BlendFactor::Src));
    }
}

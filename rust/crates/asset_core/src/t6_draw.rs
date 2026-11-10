//! bo2zm: how a Black Ops II material draws in the renderer's fallback pass.
//!
//! BO2 techniques are D3D11 shaders the engine cannot run, so the fallback
//! pass draws each material from two sources. The material's own draw state
//! (`GfxStateBits::loadBits` of its main technique, read from the zone) says
//! how it blends, whether it is alpha tested, which faces it culls and how
//! far it is pushed toward the camera (decals). The technique set's name
//! says what the shader does with the colour: `*_unlit*` is unlit, `*_b1*`
//! layered, `wpc_shadowcaster_*` only casts shadow, `*_skycubemaphdr_*` is
//! the sky. Without a draw state, the name alone decides, as before: the
//! base layer's token says how it covers (`r0c0..` replaces, `b0c0..`
//! blends, `t0c0..` alpha tests), `*_blend_*` / `*_add_*` / `*_multiply_*`.

/// bo2zm: the player's Black Ops II quality setting (`bo2_quality` in
/// settings.cfg). `full` (the default) draws as close to the real game as
/// the fallback pass can; `fast` trades looks for speed: half-size
/// textures, single-layer ground, props hidden sooner.
static T6_FAST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn t6_set_fast(fast: bool) {
    T6_FAST.store(fast, core::sync::atomic::Ordering::Relaxed);
}

pub fn t6_fast() -> bool {
    T6_FAST.load(core::sync::atomic::Ordering::Relaxed)
}

/// bo2zm: Black Ops II's texture filtering row (`tex_filter` in
/// settings.cfg): the samplers' anisotropy, 1 (low), 4 or 16 (high).
static T6_ANISO: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(16);

pub fn t6_set_aniso(aniso: u16) {
    T6_ANISO.store(aniso.clamp(1, 16), core::sync::atomic::Ordering::Relaxed);
}

pub fn t6_aniso() -> u16 {
    T6_ANISO.load(core::sync::atomic::Ordering::Relaxed)
}

/// bo2mp: the shared look on Black Ops II's own maps
/// (`BO2MP_BO2_SHARED_LOOK=1`, also `on` / `true` / `yes`; off by default,
/// so Black Ops II draws as before): Black Ops III's lighting branch with
/// Black Ops II's own sun term, the sun cascades of `render_t7`, and its
/// SMAA and motion blur over the frame (`docs/BO2_SHARED_LOOK.md`). Read
/// once a run.
pub fn bo2_shared_look() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| bo2_shared_look_value(std::env::var("BO2MP_BO2_SHARED_LOOK").ok().as_deref()))
}

/// The switch's reading of a value (unset or anything else: off).
pub fn bo2_shared_look_value(v: Option<&str>) -> bool {
    matches!(v.map(|v| v.trim().to_ascii_lowercase()).as_deref(), Some("1" | "on" | "true" | "yes"))
}

#[cfg(test)]
mod shared_look_tests {
    #[test]
    fn the_shared_look_is_off_unless_asked() {
        use super::bo2_shared_look_value as v;
        assert!(!v(None));
        assert!(!v(Some("")));
        assert!(!v(Some("0")));
        assert!(!v(Some("off")));
        assert!(v(Some("1")));
        assert!(v(Some(" On ")));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum T6Blend {
    Opaque,
    Blend,
    Add,
    Multiply,
    /// Never drawn in colour: casts shadow only, a screen distortion
    /// (`distortion_*`, an effect's heat haze), which refracts the frame in
    /// a pass this renderer does not have, or a light sphere
    /// (`*_sw4_3d_light_sphere_*`, a glow the shader works out from the
    /// sphere's depth; as a plain model it drew a pale hard-edged disc, Mob
    /// of the Dead's fires).
    ShadowOnly,
    /// The sky (`*_skycubemaphdr_*`): drawn behind everything from its cube
    /// colour map, never as a surface.
    Sky,
}

/// Which faces a material culls (`GFXS_CULL_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum T6Cull {
    None,
    Back,
    Front,
}

/// The extra layers of a layered material (`*_b1c1n1_m2c2..`), read from
/// its technique set's name, and where each layer's texcoord sits in the
/// world's second vertex stream. Per layer, that stream holds a half2
/// texcoord, then 4 more bytes (the layer's tangent frame) when the layer
/// has a normal map and so does the base layer (measured on every layered
/// Nuketown material: `t6mat` `T6MAT_VD1`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct T6Layers {
    /// Per extra layer 1..3: 0 none, 1 blend (`bK`: mixed in by the layer
    /// colour's alpha times the vertex colour's green, blue, alpha), 2
    /// multiply (`mK`: colour times 1 + weight * (layer - 1)), 3 add (`aK`,
    /// bo2mp: colour plus layer * layer alpha * weight, before squaring;
    /// `lit_sm_*_a1c1` pixel shader, fxc disassembly), 4 test (`tK`:
    /// the layer replaces the colour where layer alpha * weight >= 0.5;
    /// `lit_sm_*_t1c1n1`).
    pub kinds: [u8; 3],
    /// Per extra layer, its texcoord's byte offset in a vertex record.
    pub uv_offsets: [u8; 3],
    /// Bytes per vertex in the second stream.
    pub stride: u8,
    /// bo2mp: per extra layer, an alpha-revealed blend (`bKcK..vK`): the
    /// layer's alpha is a height its weight reveals, not a coverage.
    pub reveal: [bool; 3],
    /// bo2mp: layers 1 and 2's reveal parms (the material's
    /// `alphaRevealParms` and `alphaRevealParms2`), x the edge's softness
    /// and y its exponent, in 1/256ths up to 1023 (the lane fills them).
    /// The layer blends in by saturate((alpha - lo) / (hi - lo)), lo =
    /// saturate((1 - w) - x w^y), hi = saturate((1 - w) + x (1 - w)^y), w
    /// its weight (`lit_sm_*_b1c1n1v1_b2c2n2v2` pixel shader, fxc
    /// disassembly).
    pub reveal_parms: [[u16; 2]; 2],
}

impl T6Layers {
    /// bo2mp: the vertex colour of a vertex of a lit lightmapped layered
    /// surface: its weights (green, blue) as they are; red and alpha carry
    /// layers 1 and 2's reveal parms, negative (-(1 + 2 (1024 x + y)),
    /// odd so the interpolated value rounds back), when the layer reveals.
    pub fn reveal_color(&self, mut color: [f32; 4]) -> [f32; 4] {
        for (k, channel) in [(0, 0), (1, 3)] {
            if self.reveal[k] {
                let [x, y] = self.reveal_parms[k];
                color[channel] = -(1.0 + 2.0 * (1024.0 * f32::from(x) + f32::from(y)));
            }
        }
        color
    }

    pub fn from_technique_set(name: &str) -> Self {
        let tokens: Vec<&str> = name.split('_').collect();
        let base_normal = tokens.iter().any(|t| {
            let b = t.as_bytes();
            b.len() >= 2 && matches!(b[0], b'r' | b'b' | b't') && b[1] == b'0' && t.contains("n0")
        });
        let mut layers = Self::default();
        for t in tokens {
            let b = t.as_bytes();
            if b.len() < 4
                || !matches!(b[0], b'b' | b'm' | b'a' | b't')
                || !(b'1'..=b'3').contains(&b[1])
            {
                continue;
            }
            // A layer token names its colour map: `b1c1..`, `m2c2..`.
            if b[2] != b'c' {
                continue;
            }
            let k = usize::from(b[1] - b'1');
            layers.kinds[k] = match b[0] {
                b'b' => 1,
                b'm' => 2,
                b'a' => 3,
                _ => 4,
            };
            layers.reveal[k] = b[0] == b'b' && t.contains(&format!("v{}", b[1] as char));
            layers.uv_offsets[k] = layers.stride;
            layers.stride += 4;
            if base_normal && t.contains(&format!("n{}", b[1] as char)) {
                layers.stride += 4;
            }
        }
        layers
    }

    pub fn any(&self) -> bool {
        self.kinds.iter().any(|&k| k != 0)
    }
}

/// bo2mp: how a material moves, whatever the game: the kind of animation the
/// fallback pass gives its surfaces. Every game's lane maps its own markers
/// (technique names, material names, material constants) to one of these;
/// the shader does the rest from the game time. Two bits of a draw code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum T6Anim {
    /// Still.
    #[default]
    None,
    /// Flowing water, slowly: a stream, a river, a pool's drift. The
    /// picture slides along the surface (down a slope, else along v) at a
    /// walking pace, whatever its texture scale.
    Flow,
    /// Falling or rushing water (a waterfall, rapids): the same, fast.
    Fall,
    /// Cloth and foliage in the wind: a flag, a banner, a tarp, ivy, ferns.
    /// The vertices ripple along the wind (`vertex_prop`).
    Cloth,
}

impl T6Anim {
    /// The two bits of a draw code.
    pub fn bits(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Flow => 1,
            Self::Fall => 2,
            Self::Cloth => 3,
        }
    }

    pub fn from_bits(bits: u32) -> Self {
        match bits & 3 {
            1 => Self::Flow,
            2 => Self::Fall,
            3 => Self::Cloth,
            _ => Self::None,
        }
    }

    /// This one if it moves, else the other.
    pub fn or(self, other: Self) -> Self {
        if self == Self::None { other } else { self }
    }
}

/// Where a draw code keeps its animation: the top two bits of the sort key
/// (bits 22 and 23; sort keys are below 64 in every game read so far, and
/// `T6Draw::code` clamps them to 63).
pub const T6_ANIM_SHIFT: u32 = 22;

/// The tokens of a technique or material name, lower case: split at the
/// separators the games use (`_` `/` `#` `@` `.` `-` `:` `,` and spaces).
fn name_tokens(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(['_', '/', '#', '@', '.', '-', ':', ',', ' '])
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
}

/// A technique set whose vertex shader waves the surface in the wind:
/// Black Ops II's `*_flag_*` (`mc_lit_sm_flag_t0c0s0`), Ghosts' and Modern
/// Warfare Remastered's `*_flag_*` and `*_flagext_wv0wv1wv2_*` (their cloth
/// and foliage: `flagWave0Par`.. hold the waves) and Black Ops III's
/// `lit_flag`. (`lmcloth` is a character shader's rim light, not a wave.)
fn cloth_technique(name: &str) -> bool {
    name_tokens(name)
        .any(|t| matches!(t.as_str(), "flag" | "flags" | "banner" | "sway") || t.starts_with("flagext"))
}

/// A technique set that scrolls its picture by name (`*_scroll*`,
/// `*scrolling*`), when it is not an effect (an effect's quads are moved by
/// the effect itself).
fn scroll_technique(name: &str) -> bool {
    let name = name.trim_start_matches(',');
    !name.starts_with("effect")
        && !name.starts_with("particlecloud")
        && !name.contains("zfeather")
        && !name.contains("cloud")
        && name_tokens(name).any(|t| t.starts_with("scroll"))
}

/// Black Ops 1's flowing water (`wpc_cod7waterflowreflect`).
fn water_flow_technique(name: &str) -> bool {
    name_tokens(name).any(|t| t.starts_with("cod7waterflow") || t == "waterflow")
}

/// A world material's name by the prefix the games give world surfaces
/// (`wpc/`, `wc/`, `w/`) against props (`mc/`, `m/`, `mlv/`).
pub fn t6_world_material_name(material: &str) -> bool {
    let material = material.trim_start_matches(',');
    matches!(material.split('/').next(), Some("wpc" | "wc" | "w"))
}

/// The animation a material's names ask for. `technique_set` is its technique
/// set's name (any game), `material` its own, `world` whether the surface is
/// part of the map's world (brush models included) and not a prop: water by
/// name is only taken from the world (a water tower, a cooler and a bottle
/// carry the word too).
pub fn t6_anim_kind(technique_set: &str, material: &str, world: bool) -> T6Anim {
    if cloth_technique(technique_set) {
        return T6Anim::Cloth;
    }
    let tech = technique_set.trim_start_matches(',');
    if tech.starts_with("effect")
        || tech.starts_with("particlecloud")
        || tech.contains("distortion")
        || tech.contains("shadowcaster")
    {
        return T6Anim::None;
    }
    if water_flow_technique(technique_set) {
        return T6Anim::Flow;
    }
    if !(world || t6_world_material_name(material)) {
        return T6Anim::None;
    }
    // Words that make a "water" material not a water surface: stains and
    // puddles lying on other surfaces, fixtures, effects.
    const NOT_WATER: &[&str] = &[
        "decal", "dec", "dcl", "puddle", "leak", "drip", "distortion", "splash", "splat", "tank", "tower",
        "cooler", "bottle", "pipe", "pipes", "heater", "hose", "jug", "gallon", "damage", "stain", "mist",
        "droplets", "trail", "fx", "gfx", "particles", "bubble", "bubbles", "ripple", "drops", "rain",
        "fountain", "pump", "can", "bucket", "barrel", "tap", "faucet", "sink", "stained", "mark", "marks",
        "wet", "reflection", "refl", "foam", "wake", "cloth", "glass",
    ];
    let words: Vec<String> = name_tokens(material).chain(name_tokens(technique_set)).collect();
    if words.iter().any(|t| NOT_WATER.contains(&t.as_str())) {
        return T6Anim::None;
    }
    if words
        .iter()
        .any(|t| t.contains("waterfall") || t.starts_with("cascade") || t == "rapids" || t == "falls")
    {
        return T6Anim::Fall;
    }
    let fast = words.iter().any(|t| t == "fast" || t.starts_with("rush"));
    if words.iter().any(|t| t.starts_with("scroll")) {
        return if fast { T6Anim::Fall } else { T6Anim::Flow };
    }
    const WATER: &[&str] = &[
        "water", "river", "stream", "creek", "canal", "sewage", "sewer", "sludge", "slime", "lake", "ocean",
        "sea", "pond", "swamp", "lava", "magma", "current",
    ];
    if words
        .iter()
        .any(|t| WATER.contains(&t.as_str()) || (t.starts_with("water") && !t.contains("proof")))
    {
        return if fast { T6Anim::Fall } else { T6Anim::Flow };
    }
    T6Anim::None
}

/// A scroll rate (u and v per second, Ghosts' and Modern Warfare Remastered's
/// `uvAnimParms`) as an animation: nothing under 0.005, a flow up to 0.25 a
/// second, a fall above.
pub fn t6_anim_for_scroll_rate(rate: [f32; 2]) -> T6Anim {
    let speed = rate[0].abs().max(rate[1].abs());
    if speed.is_nan() || speed <= 0.005 {
        T6Anim::None
    } else if speed <= 0.25 {
        T6Anim::Flow
    } else {
        T6Anim::Fall
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct T6Draw {
    pub blend: T6Blend,
    pub unlit: bool,
    /// Layered (`*_b1c1n1*`): the vertex colour carries the layer blend
    /// weights (green = layer 1), not a tint.
    pub layered: bool,
    /// Alpha tested: drawn where the colour map's alpha is at least 128/255.
    pub alpha_test: bool,
    pub cull: T6Cull,
    /// `GFXS_POLYGON_OFFSET_*` 0..2: decals sit this many steps toward the
    /// camera (the shadow-map level 3 counts as none).
    pub polygon_offset: u8,
    /// The material's sort key: blended surfaces draw in this order.
    pub sort_key: u8,
    /// Unlit materials: log2 of the colour scale (`scaleRGB`), 0..7.
    pub unlit_scale_exp: u8,
    /// Extra layers 1 and 2: 0 none, 1 blend, 2 multiply (`T6Layers`).
    pub layers: [u8; 2],
    /// bo2zm M2: an `effect_*` technique: vertex colour times texture,
    /// written in display space (no exposure), fogged in display space.
    pub effect: bool,
    /// bo2zm M2: a `particlecloud_*` technique (embers, ash): an effect
    /// whose vertices are the points of a particle cloud, each drawn as a
    /// small quad facing the camera.
    pub cloud: bool,
    /// bo2zm M3: an `mc_objective` technique (the magic box's question
    /// marks): a colour pulsing between the material's colorObjMin and
    /// colorObjMax, added on.
    pub objective: bool,
    /// bo2mp: a `treecanopy` technique (ferns, leaves): its vertex colour
    /// picks the wind variant (rgb) and scales the grid light (alpha); it
    /// is no tint (`mc_treecanopy` vertex shader, fxc disassembly).
    pub foliage: bool,
    /// bo2mp: BO2's water (`cod7water` pools, `cod7watershore` seas and
    /// rivers): normal maps scrolled over the world, the reflection probe by
    /// fresnel, the sun's highlight; drawn blended (the game refracts the
    /// scene behind instead).
    pub water: bool,
    /// bo2mp: BO2's emissive flow (`cod7emissiveflow`: molten lava): the
    /// rock lit by the sun, a glow from its colour remap by emissive masks.
    pub flow: bool,
    /// bo2mp: BO2's emissive tile (`sw4_3d_cod7_emissive_tile`: Magma's
    /// lava rocks): a tiled colour map lit, plus two emissive maps' glow.
    /// Packed in bit 12 of a lit code (the unlit colour scale's place).
    pub tile: bool,
    /// bo2mp: BO2's tattered flag (`sw4_3d_phong_simple_flag_tatters`: Dig's
    /// tarps): its colour mixes a frayed edge texture into the diffuse by
    /// the vertex colour's red, alpha-tested on the mix. Bit 14 of a lit
    /// code.
    pub flag: bool,
    /// bo2mp: BO2's tile blend (`sw4_3d_cod7_tile_blend`, `_edge`, `_spec`:
    /// cliffs, rocks, vista mountains): two tiled colour maps mixed by a
    /// macro mask, with macro AO. Bits 12 and 14 of a lit code together.
    pub tile_blend: bool,
    /// bo2zm M4 retest 5: an `mc_projecteddecal_*` technique (the blood a
    /// hit paints on a zombie): its model is a box; the pixel shader (fxc
    /// disassembly) reads the scene depth and paints the texture on what lies
    /// inside the box, projected along its x axis.
    pub projected_decal: bool,
    /// bo2zm: a ghost technique (`sw4_3d_char_ghost*`, Mob's Afterlife
    /// ghost hands): a bright cyan glow, its alpha from how the surface
    /// faces the eye times two drifting edge noises.
    pub ghost: bool,
    /// bo2zm 27c: a scrolling cloud sheet (`mc_*_scrolling_zfeather`, Die
    /// Rise's cloud floor), drawn unlit: its texture drifts with the game
    /// time and it fades where the scene behind it is near.
    pub sheet: bool,
    /// bo2zm round 1: a zombie's eyes (`unlit_cheap_zombie_eyes`): the
    /// texture is scaled by the eye brightness its client script set.
    pub eyes: bool,
    /// bo2mp: a Ghosts effect technique that blends by screen
    /// (`effect_*_screen_*`: INV_DEST_COLOR / ONE, so the colour times its
    /// alpha is added only where the frame is dark). Drawn as `Add` with this
    /// set; bit 6 of an effect's code (the layered bit, which effects never
    /// use).
    pub screen: bool,
    /// bo2mp: how the surface moves (flowing water, a flag in the wind);
    /// bits 22 and 23 of the code. The lanes set it from their game's
    /// markers (`t6_anim_kind`); Black Ops II's own water and lava variants
    /// move by their own shaders and keep it `None`.
    pub anim: T6Anim,
}

/// A tile blend technique set (`*_cod7_tile_blend*`).
fn tile_blend_technique(name: &str) -> bool {
    name.contains("cod7_tile_blend")
}

/// A tattered flag technique set (`*_flag_tatters_*`).
fn flag_technique(name: &str) -> bool {
    name.contains("flag_tatters")
}

/// A layer kind in a draw code's two bits: a test layer as blend (its own
/// bit says test).
fn layer_code(kind: u8) -> u32 {
    if kind == 4 { 1 } else { u32::from(kind & 3) }
}

/// An emissive tile technique set (`*_cod7_emissive_tile_*`).
fn tile_technique(name: &str) -> bool {
    name.contains("cod7_emissive_tile")
}

/// An emissive flow technique set (`*_cod7emissiveflow_*`).
fn flow_technique(name: &str) -> bool {
    name.split('_').any(|t| t == "cod7emissiveflow")
}

/// A foliage technique set (`*_treecanopy_*`).
fn foliage_technique(name: &str) -> bool {
    name.split('_').any(|t| t.starts_with("treecanopy"))
}

/// A water technique set this engine draws as water (`*_cod7water_*`,
/// `*_cod7watershore_*`).
pub fn t6_water_kind(name: &str) -> Option<u8> {
    name.trim_start_matches(',')
        .split('_')
        .find_map(|t| match t {
            "cod7water" => Some(0),
            "cod7watershore" => Some(1),
            _ => None,
        })
}

/// A projected decal technique set (`mc_projecteddecal_*`).
fn projected_decal_technique(name: &str) -> bool {
    name.trim_start_matches(',')
        .split('_')
        .any(|t| t == "projecteddecal")
}

/// An objective technique set (`mc_objective_*`).
fn objective_technique(name: &str) -> bool {
    name.trim_start_matches(',')
        .split('_')
        .any(|t| t == "objective")
}

/// A ghost technique set (`mc_sw4_3d_char_ghost2_*`).
fn ghost_technique(name: &str) -> bool {
    name.contains("_char_ghost")
}

/// A technique set that draws an effect: `effect_*`, or `particlecloud_*`
/// (an effect's particle cloud).
fn effect_technique(name: &str) -> bool {
    let name = name.trim_start_matches(',');
    name.starts_with("effect") || name.starts_with("particlecloud")
}

/// `BO2MP_T6_NAME_BLEND=sort`: a technique set read by its name alone (no
/// state bits) is blended when its sort key is over 4, even with a
/// replacing or alpha-tested base layer, as before.
fn name_blend_by_sort_key() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T6_NAME_BLEND").is_ok_and(|v| v.trim().eq_ignore_ascii_case("sort")))
}

/// A technique set never drawn in colour (`T6Blend::ShadowOnly`): a shadow
/// caster, a distortion or a light sphere.
fn never_coloured(name: &str) -> bool {
    name.split('_').any(|t| t == "shadowcaster")
        || name.trim_start_matches(',').starts_with("distortion")
        || name.contains("sw4_3d_light_sphere")
}

/// Decoded fields of a T6 `GfxStateBits::loadBits` pair (bit positions per
/// the T6 `GfxStateBitsLoadBits` layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct T6StateBits {
    /// GFXS_BLEND_*: 1 zero, 2 one, 3 src colour, 5 src alpha, 6 inv src
    /// alpha, 9 dst colour.
    pub src_blend: u8,
    pub dst_blend: u8,
    /// GFXS_BLENDOP_*: 0 = blending off.
    pub blend_op: u8,
    /// `Some(1)` = alpha test at 128/255, `Some(0)` = above 0.
    pub alpha_test: Option<u8>,
    /// GFXS_CULL_*: 1 none, 2 back, 3 front.
    pub cull: u8,
    pub color_write_rgb: bool,
    pub polygon_offset: u8,
}

impl T6StateBits {
    pub fn decode(bits: [u32; 2]) -> Self {
        let [a, b] = bits;
        let f = |v: u32, at: u32, n: u32| ((v >> at) & ((1 << n) - 1)) as u8;
        Self {
            src_blend: f(a, 0, 4),
            dst_blend: f(a, 4, 4),
            blend_op: f(a, 8, 3),
            alpha_test: (f(a, 11, 1) == 0).then(|| f(a, 12, 1)),
            cull: f(a, 14, 2),
            color_write_rgb: f(a, 27, 1) != 0,
            polygon_offset: f(b, 4, 2),
        }
    }
}

const GFXS_BLEND_ZERO: u8 = 1;
const GFXS_BLEND_ONE: u8 = 2;
const GFXS_BLEND_SRCCOLOR: u8 = 3;
const GFXS_BLEND_DESTCOLOR: u8 = 9;

/// The animation a technique set's name alone gives: a wind-waved cloth,
/// Black Ops 1's flowing water, a scrolling technique. Black Ops II's water
/// and lava variants (and the effects) move by their own shaders.
fn technique_anim(name: &str, water: bool) -> T6Anim {
    if water || flow_technique(name) || effect_technique(name) {
        return T6Anim::None;
    }
    if cloth_technique(name) {
        return T6Anim::Cloth;
    }
    if water_flow_technique(name) || scroll_technique(name) {
        return T6Anim::Flow;
    }
    T6Anim::None
}

impl T6Draw {
    /// From the technique set's name, the material's sort key and, when the
    /// material has one, its main technique's `loadBits`.
    pub fn from_technique_set(name: &str, sort_key: u8, state: Option<[u32; 2]>) -> Self {
        // `effect_*` techniques (sprites, flashes, smoke) sample no lightmap
        // or grid: the effect system bakes their lighting into the vertex
        // colour (`lightingFrac`), so they draw unlit.
        let effect = effect_technique(name);
        let cloud = name.trim_start_matches(',').starts_with("particlecloud");
        let objective = objective_technique(name);
        let unlit = name.split('_').any(|t| t.starts_with("unlit")) || effect || objective;
        // bo2zm M3 fix list 2: a multiply layer (`m1c1`) is a layer too: its
        // vertex colour is the layer weights, not a tint (the garage door's
        // blast-crater layer drew the whole door purple, 255/63/255).
        let layered = name.split('_').any(|t| t.starts_with("b1"))
            || T6Layers::from_technique_set(name).any();
        let sky = name.split('_').any(|t| t.starts_with("sky"));
        let shadow_only = never_coloured(name);
        let Some(bits) = state else {
            return Self::from_name(name, sort_key);
        };
        let st = T6StateBits::decode(bits);
        let water = t6_water_kind(name).is_some();
        let blend = if sky {
            T6Blend::Sky
        } else if shadow_only || !st.color_write_rgb {
            T6Blend::ShadowOnly
        } else if water {
            T6Blend::Blend
        } else if st.blend_op == 0 {
            T6Blend::Opaque
        } else if st.dst_blend == GFXS_BLEND_ONE {
            T6Blend::Add
        } else if (st.src_blend == GFXS_BLEND_ZERO && st.dst_blend == GFXS_BLEND_SRCCOLOR)
            || (st.src_blend == GFXS_BLEND_DESTCOLOR && st.dst_blend == GFXS_BLEND_ZERO)
        {
            T6Blend::Multiply
        } else {
            T6Blend::Blend
        };
        Self {
            blend,
            unlit,
            layered,
            alpha_test: st.alpha_test == Some(1),
            // A cloud's quads always face the camera.
            cull: match st.cull {
                _ if cloud => T6Cull::None,
                2 => T6Cull::Back,
                3 => T6Cull::Front,
                _ => T6Cull::None,
            },
            polygon_offset: if st.polygon_offset == 3 {
                0
            } else {
                st.polygon_offset
            },
            sort_key,
            unlit_scale_exp: 0,
            layers: {
                let l = T6Layers::from_technique_set(name);
                [l.kinds[0], l.kinds[1]]
            },
            effect,
            cloud,
            objective,
            foliage: foliage_technique(name),
            water,
            flow: flow_technique(name),
            tile: tile_technique(name),
            flag: flag_technique(name),
            tile_blend: tile_blend_technique(name),
            projected_decal: projected_decal_technique(name),
            ghost: ghost_technique(name),
            sheet: false,
            eyes: false,
            screen: false,
            anim: technique_anim(name, water),
        }
    }

    /// The name-only reading, for a material without a draw state.
    fn from_name(name: &str, sort_key: u8) -> Self {
        let has = |word: &str| name.split('_').any(|t| t == word);
        let unlit = name.split('_').any(|t| t.starts_with("unlit"));
        // bo2zm M3 fix list 2: a multiply layer (`m1c1`) is a layer too: its
        // vertex colour is the layer weights, not a tint (the garage door's
        // blast-crater layer drew the whole door purple, 255/63/255).
        let layered = name.split('_').any(|t| t.starts_with("b1"))
            || T6Layers::from_technique_set(name).any();
        let base = name.split('_').find(|t| {
            let b = t.as_bytes();
            b.len() >= 4 && matches!(b[0], b'r' | b'b' | b't') && b[1] == b'0' && b[2] == b'c'
        });
        let base_kind = base.map(|t| t.as_bytes()[0]);
        let alpha_test =
            base_kind == Some(b't') || name.split('_').any(|t| t.starts_with("treecanopy"));
        // bo2mp: a replacing (`r0c0`) or alpha-tested (`t0c0`) base layer
        // is opaque whatever the material's sort key: the sort key alone
        // (over 4 = blended) read another game's opaque props and guns
        // (their keys run past 4) as blended, so they drew see-through
        // with no depth. `BO2MP_T6_NAME_BLEND=sort` is the sort key alone,
        // as before.
        let solid_base = matches!(base_kind, Some(b'r' | b't')) && !name_blend_by_sort_key();
        let blend = if name.split('_').any(|t| t.starts_with("sky")) {
            T6Blend::Sky
        } else if never_coloured(name) {
            T6Blend::ShadowOnly
        } else if has("multiply") {
            T6Blend::Multiply
        } else if has("add") {
            T6Blend::Add
        } else if has("blend") || base_kind == Some(b'b') {
            T6Blend::Blend
        } else if has("replace") || solid_base || sort_key <= 4 {
            T6Blend::Opaque
        } else {
            T6Blend::Blend
        };
        Self {
            blend,
            unlit,
            layered,
            alpha_test,
            cull: T6Cull::None,
            polygon_offset: 0,
            sort_key,
            unlit_scale_exp: 0,
            layers: {
                let l = T6Layers::from_technique_set(name);
                [l.kinds[0], l.kinds[1]]
            },
            effect: effect_technique(name),
            cloud: name.trim_start_matches(',').starts_with("particlecloud"),
            objective: objective_technique(name),
            foliage: foliage_technique(name),
            water: t6_water_kind(name).is_some(),
            flow: flow_technique(name),
            tile: tile_technique(name),
            flag: flag_technique(name),
            tile_blend: tile_blend_technique(name),
            projected_decal: projected_decal_technique(name),
            ghost: ghost_technique(name),
            sheet: false,
            eyes: false,
            screen: false,
            anim: technique_anim(name, t6_water_kind(name).is_some()),
        }
    }

    /// One word for per-surface tables: blend in bits 0..3, unlit in bit 4,
    /// layered in bit 6, alpha tested in bit 7, cull in bits 8..9 (0 none,
    /// 1 back, 2 front), polygon offset in bits 10..11, the unlit colour
    /// scale's log2 in bits 12..14, a particle cloud in bit 15 (an effect;
    /// without the effect bit 5, a cloud sheet), the sort
    /// key in bits 16..21 (at most 63), its animation (`T6Anim`) in bits
    /// 22..23, extra layers 1 and 2 in bits 24..25 and 26..27, an
    /// objective in bit 28, foliage in bit 29, water in bit 30, emissive flow in bit 31;
    /// a lit emissive tile in bit 12, a lit layer 1 test layer in bit 13 (its kind
    /// bits say blend; a layer 2 test layer is drawn as blend).
    /// bo2zm merge: a ghost sets bits 28 and 29 (objective + foliage), a
    /// projected decal bits 29 and 30 (foliage + water); no technique is
    /// either pair, so decoders test the pairs before the single bits.
    /// `NONE_CODE` marks a surface with no BO2 draw (no real code
    /// has a low nibble of 0xf).
    pub fn code(self) -> u32 {
        let blend: u32 = match self.blend {
            T6Blend::Opaque => 0,
            T6Blend::Blend => 1,
            T6Blend::Add => 2,
            T6Blend::Multiply => 3,
            T6Blend::ShadowOnly => 4,
            T6Blend::Sky => 5,
        };
        let cull: u32 = match self.cull {
            T6Cull::None => 0,
            T6Cull::Back => 1,
            T6Cull::Front => 2,
        };
        blend
            | if self.unlit { 0x10 } else { 0 }
            | if self.layered { 0x40 } else { 0 }
            | if self.screen && self.effect && !self.layered { 0x40 } else { 0 }
            | if self.alpha_test { 0x80 } else { 0 }
            | cull << 8
            | u32::from(self.polygon_offset & 3) << 10
            | u32::from(self.unlit_scale_exp & 7) << 12
            | u32::from(self.sort_key.min(63)) << 16
            | self.anim.bits() << T6_ANIM_SHIFT
            | layer_code(self.layers[0]) << 24
            | layer_code(self.layers[1]) << 26
            | if self.effect { 0x20 } else { 0 }
            | if self.cloud || self.sheet { 0x8000 } else { 0 }
            | if self.objective { 0x1000_0000 } else { 0 }
            | if self.foliage { 0x2000_0000 } else { 0 }
            | if self.water { 0x4000_0000 } else { 0 }
            | if self.flow { 0x8000_0000 } else { 0 }
            | if self.tile && !self.unlit { 0x1000 } else { 0 }
            | if self.layers[0] == 4 && !self.unlit {
                0x2000
            } else {
                0
            }
            | if self.flag && !self.unlit { 0x4000 } else { 0 }
            | if self.tile_blend && !self.unlit {
                0x5000
            } else {
                0
            }
            | if self.projected_decal { 0x6000_0000 } else { 0 }
            | if self.ghost { 0x3000_0000 } else { 0 }
    }

    pub const NONE_CODE: u32 = 0xffff_ffff;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacing_or_alpha_tested_base_is_opaque_whatever_its_sort_key() {
        // Another game's lit prop or gun material read by name (no state
        // bits) with a late sort key: opaque, not blended.
        let d = T6Draw::from_technique_set("mc_l_sm_r0c0n0s0_nfwpf", 9, None);
        assert_eq!(d.blend, T6Blend::Opaque);
        let t = T6Draw::from_technique_set("mc_l_sm_t0c0n0s0_nfwpf", 9, None);
        assert_eq!(t.blend, T6Blend::Opaque);
        assert!(t.alpha_test);
        // A blended base, an added or a blended name keep their blend.
        assert_eq!(T6Draw::from_technique_set("mc_l_sm_b0c0n0s0_nfwpf", 9, None).blend, T6Blend::Blend);
        assert_eq!(T6Draw::from_technique_set("mc_unlit_add_r0c0", 9, None).blend, T6Blend::Add);
        assert_eq!(T6Draw::from_technique_set("mc_unlit_blend_r0c0", 9, None).blend, T6Blend::Blend);
        // A name with no base token still goes by its sort key.
        assert_eq!(T6Draw::from_technique_set("mc_unlit_lin", 9, None).blend, T6Blend::Blend);
        assert_eq!(T6Draw::from_technique_set("mc_unlit_lin", 1, None).blend, T6Blend::Opaque);
    }

    #[test]
    fn particle_clouds_are_effects() {
        // Additive (one, one), back-face culled: drawn as an uncull effect.
        let add = [2 | 2 << 4 | 1 << 8 | 1 << 11 | 2 << 14 | 1 << 27, 0];
        let d = T6Draw::from_technique_set("particlecloud_zqe90390", 9, Some(add));
        assert!(d.effect && d.cloud && d.unlit);
        assert_eq!(d.blend, T6Blend::Add);
        assert_eq!(d.cull, T6Cull::None);
        assert_eq!(d.code() & 0x8020, 0x8020);
        let sprite = T6Draw::from_technique_set("effect_50567j38", 9, Some(add));
        assert!(sprite.effect && !sprite.cloud);
        assert_eq!(sprite.cull, T6Cull::Back);
    }

    /// The animation each game's own markers give (names read from the
    /// games' zones: Carrier, Ghosts' Swamp and Prison Break, Modern Warfare
    /// Remastered's Bloc).
    #[test]
    fn animated_materials_by_game() {
        use T6Anim::{Cloth, Fall, Flow};
        let kind = |tech: &str, mat: &str, world: bool| t6_anim_kind(tech, mat, world);
        // Black Ops II: its flags and tarps wave; the sea has its own shader.
        assert_eq!(kind("mc_lit_sm_flag_t0c0s0_3q8j598f", "mc/mtl_fxanim_gp_flag_chn", false), Cloth);
        assert_eq!(kind("mc_lit_sm_flag_t0c0d0n0s0_w75eqzzj", "mc/mtl_fxanim_gp_tarp_fence_blue", false), Cloth);
        assert_eq!(T6Draw::from_technique_set("mc_lit_sm_flag_t0c0s0_3q8j598f", 4, None).anim, Cloth);
        assert_eq!(T6Draw::from_technique_set("wpc_cod7watershore_69ww38j2", 4, None).anim, T6Anim::None);
        assert_eq!(T6Draw::from_technique_set("mc_sw4_3d_cod7_emissiveflow_x", 4, None).anim, T6Anim::None);
        assert_eq!(T6Draw::from_technique_set("mc_lit_sm_r0c0n0s0_zqq1fze7", 4, None).anim, T6Anim::None);
        // ... and a water cooler is not water.
        assert_eq!(kind("mc_lit_sm_r0c0n0s0_zqq1fze7", "mc/mtl_p6_water_cooler_metal", false), T6Anim::None);
        assert_eq!(kind("mc_lit_sm_r0c0n0s0_zqq1fze7", "mc/mtl_p6_water_cooler_metal", true), T6Anim::None);
        assert_eq!(kind("wpc_unlit_blend_x", "wpc/me_decal_water_puddle", true), T6Anim::None);
        // Black Ops 1: the flowing water techset.
        assert_eq!(T6Draw::from_technique_set("wpc_cod7waterflowreflect_xyz", 4, None).anim, Flow);
        // Ghosts and Modern Warfare Remastered: `flag` and `flagext` cloth and foliage.
        assert_eq!(kind(",mc_l_sm_flag_t0c0n0s0l0_nfwpf", "mc/mtl_foliage_ehq_ivy_01", false), Cloth);
        assert_eq!(kind(",mc_l_sm_flagext_wv0wv1wv2_t0c0n0s0l0_nfwpf", "mc/mtl_swamp_ivy_01", false), Cloth);
        assert_eq!(kind(",m_ambient_t0c0l0_flag_nfwpf", "m/x", false), Cloth);
        assert_eq!(kind(",mc_l_sm_lmpb_flagext_wv0_fdir_fuv_posoffset_t0c0_nfwpf_sst", "mc/x", false), Cloth);
        assert_eq!(kind(",ms_l_sm_lmcloth_r0c0n0s0l0_nfwpf", "ms/mtl_viewmodel_mp_mmyers_a", false), T6Anim::None);
        assert_eq!(kind(",m_l_sm_r0c0n0s0l0_nfwpf", "m/mtl_hanging_clothes", false), T6Anim::None);
        // A scrolling stream in the world flows; effects and fixtures do not.
        assert_eq!(kind(",wc_l_sm_ndw_ua_b0c0nf0s0l0_nocastp0_cltrans", "wc/water_stream_scrolling_slow", true), Flow);
        assert_eq!(kind(",wc_distortion_scale_ua_zfeather", "wc/water_distortion_w_z_feather", true), T6Anim::None);
        assert_eq!(kind(",effect_zfeather_blend_lin_ndw_bl_abl", "gfx_falling_water", true), T6Anim::None);
        assert_eq!(kind(",m_l_sm_r0c0n0s0l0_nfwpf", "m/mtl_h1_mwr_water_tower1", false), T6Anim::None);
        assert_eq!(kind(",wc_l_sm_r0c0_nfwpf", "wc/mp_flooded_water_01", true), Flow);
        assert_eq!(kind("l_sm_r0c0", "waterfall_mist_01", true), T6Anim::None);
        assert_eq!(kind("l_sm_r0c0", "mtl_waterfall_01", true), Fall);
        assert_eq!(kind("unlit_scroll_blend", "jup_water_fast", true), Fall);
        // A world material is told from a prop's by its prefix.
        assert!(t6_world_material_name("wpc/water_ocean"));
        assert!(t6_world_material_name(",wc/x"));
        assert!(!t6_world_material_name("mc/x"));
        assert_eq!(kind("wpc_lit_sm_r0c0", "wpc/river_01", false), Flow);
        assert_eq!(kind("mc_lit_sm_r0c0", "mc/river_01", false), T6Anim::None);
        // Scroll rates (`uvAnimParms`).
        assert_eq!(t6_anim_for_scroll_rate([0.0, 0.0]), T6Anim::None);
        assert_eq!(t6_anim_for_scroll_rate([0.0, 0.01]), Flow);
        assert_eq!(t6_anim_for_scroll_rate([-3.0, 0.0]), Fall);
        assert_eq!(t6_anim_for_scroll_rate([f32::NAN, 0.0]), T6Anim::None);
    }

    #[test]
    fn animation_rides_in_the_code() {
        for anim in [T6Anim::None, T6Anim::Flow, T6Anim::Fall, T6Anim::Cloth] {
            let mut d = T6Draw::from_technique_set("wpc_lit_sm_r0c0n0_x", 40, None);
            d.anim = anim;
            let code = d.code();
            assert_eq!(T6Anim::from_bits(code >> T6_ANIM_SHIFT), anim);
            // The sort key keeps its six bits.
            assert_eq!((code >> 16) & 0x3f, 40);
        }
        let mut d = T6Draw::from_technique_set("wpc_lit_sm_r0c0n0_x", 200, None);
        d.anim = T6Anim::Cloth;
        assert_eq!((d.code() >> 16) & 0x3f, 63);
    }

    #[test]
    fn ghost_hands_are_ghosts() {
        let d = T6Draw::from_technique_set("mc_sw4_3d_char_ghost2_noscript_954f9few", 40, None);
        assert!(d.ghost && !d.objective && !d.projected_decal);
        assert_eq!(d.code() & 0x3000_0000, 0x3000_0000);
    }

    #[test]
    fn nuketown_technique_sets() {
        let d = |n: &str, s: u8| T6Draw::from_technique_set(n, s, None);
        assert_eq!(d("wpc_lit_sm_r0c0n0x0_f488034e", 4).blend, T6Blend::Opaque);
        assert!(!d("lit_sm_r0c0n0_b1c1n1", 4).unlit);
        assert!(d("lit_sm_r0c0n0_b1c1n1", 4).layered);
        assert!(!d("wpc_lit_sm_r0c0n0x0_f488034e", 4).layered);
        assert!(d("lit_sm_r0c0n0x0_m1c1", 4).layered);
        assert!(d("mc_lit_sm_t0c0n0_9qf6e4qj", 4).alpha_test);
        assert!(d("mlv_treecanopy_sm_5q2j2275", 4).alpha_test);
        assert!(!d("wpc_lit_sm_r0c0n0x0_f488034e", 4).alpha_test);
        assert_eq!(d("mc_lit_sm_b0c0n0_x", 4).blend, T6Blend::Blend);
        let decal = d("wpc_unlitdecalblend_multiply_35079164", 12);
        assert_eq!(decal.blend, T6Blend::Multiply);
        assert!(decal.unlit);
        assert_eq!(d("wpc_unlit_blend_2840z6q0", 40).blend, T6Blend::Blend);
        assert_eq!(d("wpc_shadowcaster_wj6w5j60", 4).blend, T6Blend::ShadowOnly);
        assert_eq!(d(",distortion_81587199", 48).blend, T6Blend::ShadowOnly);
        assert_eq!(
            d("mc_sw4_3d_light_sphere_46j58w7u", 48).blend,
            T6Blend::ShadowOnly
        );
        assert_eq!(d("mc_skycubemaphdr_1f117w7u", 4).blend, T6Blend::Sky);
    }

    /// Draw states as Nuketown's materials carry them (t6mat T6MAT_STATES).
    #[test]
    fn nuketown_draw_states() {
        // Opaque, cull back: the rocks' "tile_blend" shader is not a blend.
        let opaque_back = [(2 << 14) | (1 << 11) | (1 << 27) | (1 << 28), 1 | (3 << 2)];
        let rock =
            T6Draw::from_technique_set("mc_sw4_3d_cod7_tile_blend_q2q2zej6", 4, Some(opaque_back));
        assert_eq!(rock.blend, T6Blend::Opaque);
        assert_eq!(rock.cull, T6Cull::Back);
        assert!(!rock.alpha_test);
        // Premultiplied blend (1, 1-sa), alpha test above 0, decal offset 1.
        let blend = [
            2 | (6 << 4) | (1 << 8) | (2 << 14) | (1 << 27),
            (3 << 2) | (1 << 4),
        ];
        let b = T6Draw::from_technique_set("wpc_lit_sm_b0c0n0x0_z1wzf739", 40, Some(blend));
        assert_eq!(b.blend, T6Blend::Blend);
        assert!(!b.alpha_test);
        assert_eq!(b.polygon_offset, 1);
        assert_eq!(b.code() >> 16, 40);
        // Multiply decal: 0 * src + src colour * dst, offset 2.
        let mul = [
            1 | (3 << 4) | (1 << 8) | (1 << 11) | (2 << 14) | (1 << 27),
            (3 << 2) | (2 << 4),
        ];
        let m = T6Draw::from_technique_set("wpc_unlitdecalblend_multiply_35079164", 12, Some(mul));
        assert_eq!(m.blend, T6Blend::Multiply);
        assert!(m.unlit);
        assert_eq!(m.polygon_offset, 2);
        // Alpha tested at 128, both faces: burning planks, hedges.
        let atest = [(1 << 12) | (1 << 14) | (1 << 27), 1 | (3 << 2)];
        let a = T6Draw::from_technique_set("mlv_treecanopy_sm_5q2j2275", 4, Some(atest));
        assert!(a.alpha_test);
        assert_eq!(a.cull, T6Cull::None);
        assert_eq!(a.blend, T6Blend::Opaque);
    }

    /// Layer layouts as measured on Nuketown's second vertex stream.
    #[test]
    fn layer_layouts() {
        let l = |n: &str| T6Layers::from_technique_set(n);
        assert_eq!(l("lit_sm_r0c0n0_b1c1n1").stride, 8);
        assert_eq!(l("lit_sm_r0c0_b1c1n1").stride, 4);
        assert_eq!(l("lit_sm_r0c0n0x0_m1c1").stride, 4);
        let m = l("lit_sm_r0c0n0x0_b1c1n1_m2c2");
        assert_eq!(m.stride, 12);
        assert_eq!(m.kinds, [1, 2, 0]);
        assert_eq!(m.uv_offsets[1], 8);
        assert_eq!(l("lit_sm_r0c0n0s0_b1c1n1x1_m2c2_m3c3").stride, 16);
        assert_eq!(l("lit_sm_r0c0n0x0_b1c1n1_b2c2n2").stride, 16);
        assert_eq!(l("lit_sm_r0c0n0x0_b1c1v1").stride, 4);
        // Die Rise's paper decals: a tested layer, then a blend layer.
        let t = l("lit_sm_r0c0n0x0_t1c1n1_b2c2n2s2v2");
        assert_eq!(t.kinds, [4, 1, 0]);
        assert_eq!(t.stride, 16);
        // Origins: an add layer, then a multiply layer.
        let a = l("lit_sm_r0c0n0x0_a1c1_m2c2");
        assert_eq!(a.kinds, [3, 2, 0]);
        assert_eq!(a.uv_offsets[1], 4);
        assert!(!l("wpc_lit_sm_r0c0n0x0_f488034e").any());
        assert!(!l("wpc_lit_sm_b0c0n0_19f3588f").any());
        // bo2mp: Turbine's alpha-revealed layers (no stream bytes of their
        // own: `T6MAT_VD1` measured 8 and 16).
        let v = l("lit_sm_r0c0n0_b1c1n1v1");
        assert_eq!((v.stride, v.reveal), (8, [true, false, false]));
        let v2 = l("lit_sm_r0c0n0_b1c1n1_b2c2n2v2");
        assert_eq!((v2.stride, v2.reveal), (16, [false, true, false]));
        assert_eq!(t.reveal, [false, true, false]);
        assert_eq!(m.reveal, [false; 3]);
    }

    /// bo2mp: a revealed layer's parms ride negative in red (layer 1) and
    /// alpha (layer 2); a plain layer's colour is left as it is.
    #[test]
    fn reveal_colour() {
        let mut v = T6Layers::from_technique_set("lit_sm_r0c0n0_b1c1n1v1");
        v.reveal_parms[0] = [64, 192];
        let c = v.reveal_color([1.0, 0.5, 0.25, 1.0]);
        assert_eq!(c, [-(1.0 + 2.0 * (1024.0 * 64.0 + 192.0)), 0.5, 0.25, 1.0]);
        let plain = T6Layers::from_technique_set("lit_sm_r0c0n0_b1c1n1");
        assert_eq!(plain.reveal_color([1.0, 0.5, 0.25, 1.0]), [1.0, 0.5, 0.25, 1.0]);
    }
}

//! bo2zm: Black Ops II materials into the engine's material catalog.
//!
//! BO2 materials carry D3D11 techniques the engine cannot run, so a row here
//! is only what the renderer's fallback draw samples: the material's colour
//! map, read from the game's image packs (IWI v27) into a GPU image in its
//! own block format (BC1-3). Rows have no technique set; their surfaces keep
//! drawing through the fallback pass, which samples the colour map.

use std::collections::HashMap;
use std::sync::Arc;

use asset_core::{AssetEdge, AssetNamespace, AssetRef, T6Draw, ZoneOwner};
use asset_material::{
    AuthoredImage, AuthoredMaterial, ImageVariantId, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP,
    TS_NORMAL_MAP, TS_SPECULAR_MAP,
};
use asset_t6::{ImageRef, ImageSource, PackSet, ZoneCapture};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::Image;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

/// `R_HashString("colorMap")`, the colour map's slot in a texture table.
const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
/// bo2mp: `DiffuseAndGloss`, the sw4 shaders' colour map.
const DIFFUSE_AND_GLOSS_HASH: u32 = 0xd50e_4161;
/// bo2mp: the picture of BO2's laser-scan 2D shader (`sw4_2d_laser_scan`:
/// the big scorestreak icons, `hud_ks_*_big`; its other two images are
/// the scan's noise and wireframe).
const LASER_SCAN_IMAGE_HASH: u32 = 0x079c_6367;

/// bo2mp: the 3-D tile-blend shaders (`sw4_3d_cod7_tile_blend*`, rocks and
/// cliffs on most multiplayer maps) have no `colorMap`: they mix two tiling
/// colour maps (`Micro_1_ColorMap`, `Micro_2_ColorMap`) by a macro map in
/// this slot, whose red, green and blue are blend weights (named `_ao`, or
/// `_c` on the cliffs: Downhill's red and yellow rocks).
const TILE_BLEND_MACRO_HASH: u32 = 0xed0a_4f83;
const TILE_BLEND_MICRO1_HASH: u32 = name_hash("Micro_1_ColorMap");
const TILE_BLEND_MICRO2_HASH: u32 = name_hash("Micro_2_ColorMap");

/// `R_HashString`: case-folded, xor with 33 times the running hash.
pub(crate) const fn name_hash(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h = 0u32;
    let mut i = 0;
    while i < b.len() {
        h = (b[i] | 0x20) as u32 ^ h.wrapping_mul(33);
        i += 1;
    }
    h
}

/// A layered material's extra colour maps (`colorMap1`, `colorMap2`) ride
/// in the catalog under these private semantics; the fallback draw reads
/// them by semantic. Real T6 semantics stay below 0x20.
pub const T6_LAYER1_SEMANTIC: u8 = 0xf1;
pub const T6_LAYER2_SEMANTIC: u8 = 0xf2;
pub(crate) const LAYER_MAPS: [(u32, u8); 2] = [
    (name_hash("colorMap1"), T6_LAYER1_SEMANTIC),
    (name_hash("colorMap2"), T6_LAYER2_SEMANTIC),
];

/// What linking did: capture material index -> catalog material index.
pub(crate) struct T6Materials {
    pub local: HashMap<usize, usize>,
    pub report: Vec<String>,
}

/// How many of a picture's top levels are left out: one under the fast
/// setting (pictures of 128 and over), and as many as it takes for the
/// side to come under `cap` (a card with little memory), as long as a
/// level is left and the level kept is whole 4x4 blocks. A cube map (the
/// sky) keeps every level.
pub fn mip_skip(width: u32, height: u32, levels: u32, cube: bool, fast: bool, cap: u32) -> u32 {
    if cube || levels <= 1 {
        return 0;
    }
    let mut skip = u32::from(fast && width >= 128 && height >= 128);
    while skip + 1 < levels && (width >> skip).max(height >> skip) > cap && (width >> (skip + 1)) % 4 == 0 && (height >> (skip + 1)) % 4 == 0 {
        skip += 1;
    }
    skip
}

/// The card's picture cap for Black Ops II's own pictures
/// (`native_quality::vram_picture_cap`).
fn bo2_picture_cap() -> u32 {
    frame::settings::native_quality::vram_picture_cap()
}

/// A GPU image over an IWI's own blocks: every mip level, largest first.
/// A cube map (the sky's) is six faces, each with its mips, read linear:
/// the sky shader decodes it as rgb / a in linear space.
pub(crate) fn gpu_image(iwi: &ipak_t6::IwiImage<'_>) -> Result<Image, String> {
    use ipak_t6::IwiFormat as F;
    let cube = iwi.is_cube();
    let format = match (iwi.format, cube) {
        (F::Dxt1, false) => TextureFormat::Bc1RgbaUnormSrgb,
        (F::Dxt3, false) => TextureFormat::Bc2RgbaUnormSrgb,
        (F::Dxt5, false) => TextureFormat::Bc3RgbaUnormSrgb,
        // Normal maps: two channels, read as stored.
        (F::Dxn, false) => TextureFormat::Bc5RgUnorm,
        (F::Dxt1, true) => TextureFormat::Bc1RgbaUnorm,
        (F::Dxt3, true) => TextureFormat::Bc2RgbaUnorm,
        (F::Dxt5, true) => TextureFormat::Bc3RgbaUnorm,
        // Uncompressed: widened to RGBA8 below.
        (F::Rgba8 | F::Rgb8 | F::LuminanceAlpha | F::Luminance | F::Alpha, false) => {
            TextureFormat::Rgba8UnormSrgb
        }
        (other, _) => return Err(format!("{other:?} colour maps are not drawn yet")),
    };
    let widen = |level: &[u8]| -> Vec<u8> {
        match iwi.format {
            F::Rgba8 => level.to_vec(),
            F::Rgb8 => level
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            F::LuminanceAlpha => level
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            F::Luminance => level.iter().flat_map(|&l| [l, l, l, 255]).collect(),
            F::Alpha => level.iter().flat_map(|&a| [255, 255, 255, a]).collect(),
            _ => level.to_vec(),
        }
    };
    if iwi.is_volume() {
        return Err("a volume image".to_owned());
    }
    if iwi.format.is_block_compressed() && (iwi.width % 4 != 0 || iwi.height % 4 != 0) {
        return Err(format!(
            "{}x{} is not whole 4x4 blocks",
            iwi.width, iwi.height
        ));
    }
    // The fast quality setting starts at the second level (half size)
    // where that is still whole blocks; the sky keeps every level. A card
    // with little memory starts lower still, at the first level that fits
    // its picture cap (`native_quality::Quality::max_texture` after
    // `fit_vram`; only the card's own cap, not the preset's).
    let skip = mip_skip(
        iwi.width,
        iwi.height,
        iwi.levels,
        cube,
        asset_core::t6_fast(),
        bo2_picture_cap(),
    );
    // IWI keeps each level's faces together; the GPU image wants each face
    // with all its levels (layer-major).
    let faces = iwi.faces as usize;
    let mut data = Vec::new();
    for face in 0..faces {
        for level in skip..iwi.levels {
            let all = iwi.level(level);
            let one = all.len() / faces;
            data.extend_from_slice(&widen(&all[face * one..(face + 1) * one]));
        }
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: iwi.width >> skip,
            height: iwi.height >> skip,
            depth_or_array_layers: iwi.faces,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = iwi.levels - skip;
    if cube {
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..Default::default()
        });
    }
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Ok(image)
}

/// The normal and specular maps BO2's lit shaders read (the shine), by
/// their material slot hashes (`normalMap`, `specularMap`).
/// (`SpecularAndGloss`, 0x8c297e80: a weapon camo's specular colour and
/// gloss, which BO2's camo shader reads where the lit one reads its
/// specular map: Pack-a-Punch's blue and gold sheen.)
pub(crate) const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
pub(crate) const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;
/// A layered material's layer 1 specular map (`specularMap1`).
const SPECULAR_MAP1_HASH: u32 = name_hash("specularMap1");

const SHINE_MAPS: [(u32, u8); 3] = [
    (0x59d3_0d0f, TS_NORMAL_MAP),
    (0x34ec_ccb3, TS_SPECULAR_MAP),
    (0x8c29_7e80, TS_SPECULAR_MAP),
];

/// The private semantic the reflection probes' cube array is linked under
/// (a material named `t6/reflection_probes` draws nothing).
pub const T6_PROBES_SEMANTIC: u8 = 0xf3;
pub const T6_PROBES_MATERIAL: &str = "t6/reflection_probes";

/// bo2mp: the seas' own sky cubes (`cod7watershore`'s `t4` in its shader,
/// 06.ps.wgsl: Socotra's skybox_sp_socotra_water), in the probes' form,
/// for the probe array's last layers. The game's sea reflects that cube,
/// never the world's probes: our near and far seas each reflected a
/// different probe and met in a hue step (Yemen's square). Per sea
/// material (capture index), its cube's index among those returned. A cube
/// under 128x128, or not DXT1 / DXT5, is left to the world's probes.
pub(crate) fn sea_sky_cubes(
    capture: &ZoneCapture,
    packs: Option<&PackSet>,
    world: &asset_t6::WorldRef,
) -> (HashMap<usize, usize>, Vec<Vec<u8>>) {
    const SKY_MAP: u32 = 0xf973_2228;
    let mut of_material = HashMap::new();
    let mut by_image: HashMap<usize, Option<usize>> = HashMap::new();
    let mut cubes = Vec::new();
    let Some(packs) = packs else {
        return (of_material, cubes);
    };
    for m in world.surfaces.iter().filter_map(|s| s.material.map(|k| k.index)) {
        if of_material.contains_key(&m) {
            continue;
        }
        let Some(material) = capture.materials.get(m) else {
            continue;
        };
        let sea = material
            .technique_set
            .and_then(|t| capture.technique_sets.get(t.index))
            .and_then(|t| asset_core::t6_water_kind(&t.name))
            == Some(1);
        let Some(key) = material
            .textures
            .iter()
            .find(|t| t.name_hash == SKY_MAP)
            .and_then(|t| t.image)
            .filter(|_| sea)
        else {
            continue;
        };
        let cube = *by_image.entry(key.index).or_insert_with(|| {
            let form = capture
                .images
                .get(key.index)
                .and_then(|img| decode(packs, img).ok())
                .and_then(|img| probe_form(&img))?;
            cubes.push(form);
            Some(cubes.len() - 1)
        });
        if let Some(i) = cube {
            of_material.insert(m, i);
        }
    }
    (of_material, cubes)
}

/// A cube image (six faces, each with its mips) in the probes' form: BC3,
/// 128x128, 8 levels (its levels from 128 down). DXT1 blocks go behind an
/// opaque alpha block; BC3 reads every colour block in 4-colour order, so a
/// 3-colour block's ends are swapped, its midpoint read as the nearer
/// third and its black as its lower end.
fn probe_form(image: &Image) -> Option<Vec<u8>> {
    const SIZE: u32 = 128;
    const LEVELS: u32 = 8;
    let d = &image.texture_descriptor;
    let bc1 = match d.format {
        TextureFormat::Bc1RgbaUnorm => true,
        TextureFormat::Bc3RgbaUnorm => false,
        _ => return None,
    };
    let width = d.size.width;
    if d.size.depth_or_array_layers != 6 || d.size.height != width || width < SIZE || !width.is_power_of_two() {
        return None;
    }
    let skip = (width / SIZE).trailing_zeros();
    if d.mip_level_count < skip + LEVELS {
        return None;
    }
    let block = if bc1 { 8 } else { 16 };
    let level_bytes = |l: u32| {
        let blocks = ((width >> l).max(1) as usize).div_ceil(4);
        blocks * blocks * block
    };
    let face: usize = (0..d.mip_level_count).map(level_bytes).sum();
    let data = image.data.as_ref().filter(|data| data.len() == face * 6)?;
    let mut out = Vec::new();
    for f in 0..6 {
        let mut at = f * face + (0..skip).map(level_bytes).sum::<usize>();
        for l in skip..skip + LEVELS {
            let bytes = &data[at..at + level_bytes(l)];
            at += level_bytes(l);
            if !bc1 {
                out.extend_from_slice(bytes);
                continue;
            }
            for b in bytes.chunks_exact(8) {
                out.extend_from_slice(&[255, 255, 0, 0, 0, 0, 0, 0]);
                if u16::from_le_bytes([b[0], b[1]]) > u16::from_le_bytes([b[2], b[3]]) {
                    out.extend_from_slice(b);
                    continue;
                }
                let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                let swapped = (0..16).fold(0u32, |acc, p| {
                    acc | ([1u32, 0, 2, 1][((bits >> (2 * p)) & 3) as usize] << (2 * p))
                });
                out.extend_from_slice(&[b[2], b[3], b[0], b[1]]);
                out.extend_from_slice(&swapped.to_le_bytes());
            }
        }
    }
    Some(out)
}

/// The world's reflection probes as one cube-array image. BO2 keeps each
/// probe's cube in the zone: BC3, 128x128, six faces each with its whole
/// mip chain, the order a GPU cube array is uploaded in. A probe in any
/// other form (Nuketown's probe 0 is a 4x4 stand-in) is black. Linked as
/// the private semantic of a material that draws nothing. `extra` (in that
/// form: the seas' sky cubes) follow the world's probes.
pub(crate) fn link_reflection_probes(
    catalog: &mut MaterialCatalog,
    capture: &ZoneCapture,
    world: &asset_t6::WorldRef,
    zone: &str,
    extra: &[Vec<u8>],
) -> Option<String> {
    const SIZE: u32 = 128;
    const LEVELS: u32 = 8;
    if world.reflection_probes.is_empty() {
        return None;
    }
    let face_bytes: usize = (0..LEVELS)
        .map(|l| {
            let blocks = ((SIZE >> l).max(1) as usize).div_ceil(4);
            blocks * blocks * 16
        })
        .sum();
    let probe_bytes = face_bytes * 6;
    let mut data = Vec::with_capacity(probe_bytes * world.reflection_probes.len());
    let mut real = 0usize;
    for probe in &world.reflection_probes {
        let embedded = probe
            .image
            .and_then(|k| capture.images.get(k.index))
            .and_then(|img| img.embedded.as_ref())
            .filter(|e| matches!(e.dxgi_format, 77 | 78) && e.data.len() == probe_bytes);
        match embedded {
            Some(e) => {
                data.extend_from_slice(&e.data);
                real += 1;
            }
            None => data.resize(data.len() + probe_bytes, 0),
        }
    }
    for cube in extra {
        data.extend_from_slice(cube);
        data.resize(data.len().next_multiple_of(probe_bytes), 0);
    }
    let layers = 6 * (world.reflection_probes.len() + extra.len()) as u32;
    let mut image = Image::new_uninit(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: layers,
        },
        TextureDimension::D2,
        TextureFormat::Bc3RgbaUnorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = LEVELS;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::CubeArray),
        ..Default::default()
    });
    image.data = Some(data);
    let slot = catalog.link_image(AuthoredImage {
        namespace: AssetNamespace::T6,
        name: AssetRef::Real(T6_PROBES_MATERIAL.to_owned()),
        map_type: 5,
        semantic: T6_PROBES_SEMANTIC,
        category: 0,
        use_srgb_reads: false,
        width: SIZE as u16,
        height: SIZE as u16,
        depth: 1,
        level_count: LEVELS as u8,
        format: 0,
        payload: Arc::new(Vec::new()),
        decoded: Some(Arc::new(image)),
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    });
    catalog.link_material(AuthoredMaterial {
        name: AssetRef::Real(T6_PROBES_MATERIAL.to_owned()),
        namespace: AssetNamespace::T6,
        technique_set: AssetRef::default(),
        technique_set_edge: AssetEdge::Absent,
        draw_surf: 0,
        sort_key: 0,
        info_game_flags: 0,
        texture_atlas: None,
        surface_type_bits: None,
        t5_layered_surface_types: None,
        state_flags: 0,
        camera_region: 0,
        state_bits: Vec::new(),
        state_bits_entry: None,
        t5_state_bits_entry: None,
        iw5_state_bits_entry: None,
        technique_table: None,
        route: None,
        textures: vec![MaterialTextureBinding {
            name_hash: 0,
            name_start: 0,
            name_end: 0,
            sampler_state: 0,
            semantic: T6_PROBES_SEMANTIC,
            image: Some(slot),
        }],
        constants: Vec::new(),
        zone: ZoneOwner::intern(zone),
        t6_draw: None,
    });
    Some(format!(
        "t6 reflection probes: {} ({real} cube maps, {} stand-ins black) and {} sea sky cubes, one {SIZE}x{SIZE} cube array, {LEVELS} mips",
        world.reflection_probes.len(),
        world.reflection_probes.len() - real,
        extra.len()
    ))
}

/// Decode one captured image from the packs.
pub(crate) fn decode(packs: &PackSet, image: &ImageRef) -> Result<Image, String> {
    // bo2mp: the mod's archive wins a picture name it holds (`CODLEGACY_T6_MOD`),
    // as the game reads a mod's pictures over the packs'.
    if mod_holds(image)
        && let Some(found) = mod_iwd_image(&image.name)
    {
        return found;
    }
    match packs.locate(image) {
        Some(ImageSource::Pack(i, entry)) => pack_image(packs.packs[i].read(entry)),
        Some(ImageSource::Embedded) => embedded_image(image),
        Some(ImageSource::Empty) => Err("no pixels".to_owned()),
        None => mod_iwd_image(&image.name)
            .or_else(|| legacy_iwd_image(&image.name))
            .unwrap_or_else(|| Err("in no pack".to_owned())),
    }
}

/// bo2mp: the mod's archive holds this streamed picture (and so wins it).
pub(super) fn mod_holds(image: &ImageRef) -> bool {
    image.embedded.is_none()
        && image.streamed_parts > 0
        && asset_transport::t6_mod::has_entry(&format!("images/{}.iwi", image.name.trim_start_matches(',')))
}

/// A pack entry's decoded bytes as a GPU image.
fn pack_image(bytes: Result<Vec<u8>, String>) -> Result<Image, String> {
    let bytes = bytes?;
    let iwi = ipak_t6::parse_iwi(&bytes).map_err(|e| e.to_string())?;
    gpu_image(&iwi)
}

/// bo2mp: a picture the mod's zone streams that no Black Ops II pack holds,
/// from the mod's archive (`CODLEGACY_T6_MOD`: `images/<name>.iwi`, Black Ops
/// II's own picture format, normal maps included).
fn mod_iwd_image(name: &str) -> Option<Result<Image, String>> {
    let mut bytes = asset_transport::t6_mod::image(name)?;
    mend_plain_iwi(&mut bytes);
    Some(
        ipak_t6::parse_iwi(&bytes)
            .map_err(|e| format!("{name} in the mod's archive: {e}"))
            .and_then(|iwi| gpu_image(&iwi)),
    )
}

/// bo2mp: a mod picture written with no format and no level size in its
/// header (the @leetinservers mod's `scav_c`: one level of 32-bit pixels
/// after the 64-byte header) is read as the RGBA8 picture it is.
fn mend_plain_iwi(bytes: &mut [u8]) {
    const HEADER: usize = 64;
    if bytes.len() < HEADER || &bytes[..4] != b"IWi\x1b" || bytes[4] != 0 || bytes[5] & 0x02 == 0 {
        return;
    }
    let (w, h) = (u16::from_le_bytes([bytes[6], bytes[7]]) as usize, u16::from_le_bytes([bytes[8], bytes[9]]) as usize);
    if bytes.len() != HEADER + w * h * 4 {
        return;
    }
    let total = (bytes.len() as u32).to_le_bytes();
    bytes[4] = 0x01;
    bytes[32..36].copy_from_slice(&total);
}

/// bo2mp: a picture of the mod's archive as RGBA (`CODLEGACY_T6_MOD`), else from
/// Black Ops II's packs beside `zone_dir` by name: the mod's
/// Create-a-Class pictures (`menu_mp_weapons_cheytac`).
pub fn mod_picture_rgba(name: &str, zone_dir: Option<&std::path::Path>) -> Option<(u32, u32, Vec<u8>)> {
    let img = match mod_iwd_image(name) {
        Some(Ok(img)) => img,
        _ => {
            let packs = asset_t6::PackSet::open_dir(zone_dir?).ok()?;
            let (i, entry) = packs.locate_name(name)?;
            let bytes = packs.packs[i].read(entry).ok()?;
            gpu_image(&ipak_t6::parse_iwi(&bytes).ok()?).ok()?
        }
    };
    let (w, h, rgba) = top_level_rgba8(&img)?;
    Some((w as u32, h as u32, rgba))
}

/// bo2zm: a picture no Black Ops II pack holds, from Black Ops' or World at
/// War's IWDs (`asset_t6::legacy_images`): the Zombies Declassified "DLC5"
/// maps (Kino, Ascension, Moon ...) name Black Ops' own art, which only
/// those games' installs carry. Colour pictures only.
fn legacy_iwd_image(name: &str) -> Option<Result<Image, String>> {
    use asset_t6::legacy_images as legacy;
    if !legacy::wants_legacy(name) {
        return None;
    }
    let mains = legacy::legacy_iwd_mains();
    let found = |main: &std::path::Path, candidate: &str| {
        asset_material::decode_iwd_colour_image(main, candidate)
            .map(|r| r.map_err(|e| format!("{candidate} in {}: {e}", main.display())))
    };
    mains
        .iter()
        .find_map(|main| legacy::legacy_names(name).iter().find_map(|c| found(main, c)))
        .or_else(|| {
            mains.iter().find_map(|main| {
                let sorted = asset_material::iwd_image_names_sorted(main);
                found(main, &legacy::legacy_cut_name(name, &sorted)?)
            })
        })
}

/// An image whose pixels ride in the zone itself (`GfxImageLoadDef`):
/// The Black Ops 1 map pack keeps its fire, embers and chalk there. The data is
/// each face's levels, largest first, as the GPU takes it.
fn embedded_image(image: &ImageRef) -> Result<Image, String> {
    let e = image.embedded.as_ref().ok_or("no pixels")?;
    // DXGI formats: BC1-3 and BC5 (UNORM / SRGB), R8G8B8A8.
    let (format, unit, block) = match e.dxgi_format {
        71 | 72 => (TextureFormat::Bc1RgbaUnormSrgb, 8, 4),
        74 | 75 => (TextureFormat::Bc2RgbaUnormSrgb, 16, 4),
        77 | 78 => (TextureFormat::Bc3RgbaUnormSrgb, 16, 4),
        83 => (TextureFormat::Bc5RgUnorm, 16, 4),
        28 | 29 => (TextureFormat::Rgba8UnormSrgb, 4, 1),
        other => return Err(format!("in-zone DXGI format {other} is not drawn yet")),
    };
    let (w, h) = (u32::from(image.width), u32::from(image.height));
    let levels = u32::from(e.level_count.max(1));
    let size = |l: u32| {
        let (lw, lh) = ((w >> l).max(1), (h >> l).max(1));
        (lw.div_ceil(block) * lh.div_ceil(block)) as usize * unit
    };
    let need: usize = (0..levels).map(size).sum();
    // A cube (the sky's; `MAPTYPE_CUBE`) carries six faces, each with its
    // mips, in GPU upload order; read linear, as the pack path's sky.
    let cube = image.map_type == 5 || (need > 0 && e.data.len() == need * 6);
    let (faces, format) = match (cube, format) {
        (true, TextureFormat::Bc1RgbaUnormSrgb) => (6, TextureFormat::Bc1RgbaUnorm),
        (true, TextureFormat::Bc2RgbaUnormSrgb) => (6, TextureFormat::Bc2RgbaUnorm),
        (true, TextureFormat::Bc3RgbaUnormSrgb) => (6, TextureFormat::Bc3RgbaUnorm),
        (true, TextureFormat::Rgba8UnormSrgb) => (6, TextureFormat::Rgba8Unorm),
        (true, _) => {
            return Err(format!(
                "in-zone DXGI {} cube is not drawn yet",
                e.dxgi_format
            ));
        }
        (false, f) => (1, f),
    };
    let need = need * faces as usize;
    if w == 0 || h == 0 || (block == 4 && (w % 4 != 0 || h % 4 != 0)) {
        return Err(format!("in-zone {w}x{h} is not whole 4x4 blocks"));
    }
    if e.data.len() < need {
        return Err(format!(
            "in-zone pixels {} bytes, {need} needed",
            e.data.len()
        ));
    }
    let mut out = Image::new_uninit(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: faces,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.texture_descriptor.mip_level_count = levels;
    if cube {
        out.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..Default::default()
        });
    }
    out.data = Some(e.data[..need].to_vec());
    out.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Ok(out)
}

/// The texture a material is coloured with: its `colorMap`; else, among its
/// colour-map-semantic textures (layered and ember materials name their
/// slots differently), the one whose image is named as a colour map (`_c`,
/// `_col`), not an ember or glow layer (`_e`); else the first of them.
/// A tile-blend material is coloured by its `<name>_cheap` stand-in's
/// colour map (BO2's own mix of the two tiles, `..._c_cmb`), else by its
/// first tile; never by its macro map.
pub(crate) fn colour_texture<'a>(
    material: &'a asset_t6::MaterialRef,
    capture: &'a ZoneCapture,
) -> Option<&'a asset_t6::MaterialTexture> {
    if material
        .textures
        .iter()
        .any(|t| t.name_hash == TILE_BLEND_MACRO_HASH)
    {
        let cheap = format!("{}_cheap", material.name);
        let mixed = capture
            .materials
            .iter()
            .find(|m| m.name == cheap)
            .and_then(|m| {
                m.textures
                    .iter()
                    .find(|t| t.name_hash == COLOR_MAP_HASH && t.image.is_some())
            });
        let tile = || {
            material
                .textures
                .iter()
                .find(|t| t.name_hash == TILE_BLEND_MICRO1_HASH && t.image.is_some())
        };
        if let Some(t) = mixed.or_else(tile) {
            return Some(t);
        }
    }
    // A colour map is named `_c` / `_col`; the burning-ember materials also
    // carry an animated glow noise named `_c` (`glowcycle_random_01_c`),
    // which sorts first in their table, so it never counts as colour.
    // Masks (`c_gen_arm_rim_mask_c`, 32x32 on the viewmodel arms) are named
    // like colour maps too; the real one is the largest.
    let colour_named = |t: &&asset_t6::MaterialTexture| {
        t.image
            .and_then(|k| capture.images.get(k.index))
            .is_some_and(|img| {
                (img.name.ends_with("_c") || img.name.ends_with("_col"))
                    && !img.name.contains("glowcycle")
                    && !img.name.contains("_mask")
            })
    };
    let area = |t: &&asset_t6::MaterialTexture| {
        t.image
            .and_then(|k| capture.images.get(k.index))
            .map_or(0u32, |img| u32::from(img.width) * u32::from(img.height))
    };
    let semantic = || {
        material
            .textures
            .iter()
            .filter(|t| t.semantic == TS_COLOR_MAP && t.image.is_some())
    };
    // bo2mp: the sw4 shaders' `DiffuseAndGloss` (Dig's tarps: their frayed
    // edge texture is named `_c`, the diffuse is not).
    material
        .textures
        .iter()
        .find(|t| t.name_hash == COLOR_MAP_HASH && t.image.is_some())
        .or_else(|| {
            material
                .textures
                .iter()
                .find(|t| t.name_hash == DIFFUSE_AND_GLOSS_HASH && t.image.is_some())
        })
        .or_else(|| semantic().filter(colour_named).max_by_key(area))
        .or_else(|| semantic().next())
        .or_else(|| {
            material
                .textures
                .iter()
                .find(|t| t.name_hash == LASER_SCAN_IMAGE_HASH && t.image.is_some())
        })
        // bo2mp: a 2D picture's one image (BO2's menu backings,
        // `sw4_2d_tile`: semantic 0, no colour-map name).
        .or_else(|| {
            let mut t = material.textures.iter().filter(|t| t.semantic == 0 && t.image.is_some());
            let first = t.next();
            first.filter(|_| t.next().is_none())
        })
}

/// A 1x1 stand-in for a colour map with no pixels here, by its name: white,
/// black, else mid grey ("grey").
fn flat_image(name: &str) -> Image {
    let n = name.to_ascii_lowercase();
    let v = if n.contains("white") {
        255
    } else if n.contains("black") {
        0
    } else {
        128
    };
    let mut image = Image::new_uninit(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(vec![v, v, v, 255]);
    image
}

/// bo2mp: an f32 as an IEEE half float (round to nearest; tiny values to
/// zero, huge ones to infinity).
fn half_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let mant = b & 0x7f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        return sign;
    }
    let half = sign | ((exp as u16) << 10) | (mant >> 13) as u16;
    // Round to nearest (a carry into the exponent is still right).
    if mant & 0x1000 != 0 { half + 1 } else { half }
}

/// bo2mp: a water material's constants for the fallback draw's water
/// shader, one RGBA16F texel each (BO2's own `cod7water` / `cod7watershore`
/// shaders, fxc disassembly; the constants by their hashes, as the shaders'
/// register arguments name them):
/// 0..3 the four normal-map layers (world xy scale, scroll per second),
/// 4 normal scale and bias (first map, second map), 5 the layers' weights,
/// 6 highlight powers, fresnel bias and scale, 7 highlight strengths, their
/// overall scale and the kind (0 pool, 1 sea, 2 open sea: no vertex
/// alpha), 8 and 9 the water colour and opacity (pool: grazing and facing;
/// sea: scatter and body, deep),
/// 10 the sea's shadow light (the sea's further rows: `sea_layers`). A
/// sea's colours take the waves map's depth (`depth`: 1 on open water).
fn water_table(material: &asset_t6::MaterialRef, kind: u8, depth: f32) -> Vec<[f32; 4]> {
    let c = |hash: u32| {
        material
            .constants
            .iter()
            .find(|c| c.0 == hash)
            .map_or([0.0; 4], |c| c.2)
    };
    let add = |a: [f32; 4], b: [f32; 4]| {
        [a[0] + b[0] * depth, a[1] + b[1] * depth, a[2] + b[2] * depth, a[3] + b[3] * depth]
    };
    if kind == 0 {
        // cod7water: waterNormalPositionScale, waterLoNormalScroll,
        // waterHiNormalScroll, waterNormalScrollSpeed (vertex shader:
        // uv = pos.xy * scale + gameTime * scroll * speed), waterNormalScale,
        // waterControlVar0/1, waterColorN, waterColorS.
        let scale = c(0xc728_b4a7);
        let lo = c(0x6e97_1968);
        let hi = c(0x01a2_0bea);
        let speed = c(0x134c_d3cc);
        let control1 = c(0xa7d6_56a6);
        vec![
            [scale[0], scale[0], lo[0] * speed[0], lo[1] * speed[0]],
            [scale[1], scale[1], lo[2] * speed[1], lo[3] * speed[1]],
            [scale[2], scale[2], hi[0] * speed[2], hi[1] * speed[2]],
            [scale[3], scale[3], hi[2] * speed[3], hi[3] * speed[3]],
            c(0x8632_8e1e),
            [1.0; 4],
            c(0xa7d6_56a7),
            [control1[0], control1[1], control1[2], 0.0],
            c(0xbd74_8e86),
            c(0xbd74_8e9b),
            [0.0; 4],
        ]
    } else {
        // cod7watershore: normalUVscroll_0123, normalUVcontrol0/2/3 (uv =
        // pos.xy * control.xy + gameTime * scroll * control.zw; layer 1 is
        // unused), normalScaleLoHi, controlVar0/1, the deep colours
        // (shallow + deep delta times the waves map's depth, 1 on open
        // water), waterOpacityControl.w, customLightmapShadowColor.
        let scroll = c(0x95ba_cba2);
        let c0 = c(0x9ea1_a767);
        let c2 = c(0x9ea1_a765);
        let c3 = c(0x9ea1_a764);
        let control1 = c(0x483f_c933);
        // The open-sea variants (Carrier, Takeoff, Frostbite's distant sea)
        // have no opacity control: they draw opaque and never read the
        // vertex colour (kind 2; their shader refracts nothing).
        let opacity_control = material.constants.iter().find(|c| c.0 == 0x8898_e1ab);
        let opacity = opacity_control.map_or(1.0, |c| c.2[3]);
        let kind = if opacity_control.is_some() { 1.0 } else { 2.0 };
        let mut scatter = add(c(0xe887_d68c), c(0xeef4_77a2));
        let mut body = add(c(0x7429_46aa), c(0x9092_abc4));
        scatter[3] = opacity;
        body[3] = opacity;
        // bo2mp: Carrier's dark dusk sea (`carrier_look`).
        if let Some(t) = super::carrier_look::sea_tint() {
            for k in 0..3 {
                scatter[k] *= t[k];
                body[k] *= t[k];
            }
        }
        vec![
            [c0[0], c0[1], scroll[0] * c0[2], scroll[0] * c0[3]],
            [0.0; 4],
            [c2[0], c2[1], scroll[2] * c2[2], scroll[2] * c2[3]],
            [c3[0], c3[1], scroll[3] * c3[2], scroll[3] * c3[3]],
            c(0xb47c_f4c9),
            [1.0, 0.0, 1.0, 1.0],
            c(0x483f_c932),
            [control1[0], control1[1], 0.3184, kind],
            scatter,
            body,
            c(0xc9e7_ae4a),
        ]
    }
}

/// bo2mp: a sea's (`cod7watershore`) layers past its normal maps, from its
/// near-sea shader (fxc disassembly; textures and constants by the hashes
/// its register arguments name). Without them its three normal layers
/// repeat bare over the whole sea (Takeoff). One RGBA8 picture at the
/// whitewater mask's size: whiteWaterMask green and alpha, the shore map's
/// green and blue (resampled). The table's rows 11..19: the mask's two uv
/// pairs (world xy scale, scroll per second), whiteWaterColor,
/// shoreUVcontrol, the noise weights, the waves map's foam and depth, the
/// shore's red over green, which layers there are (1 shore, 2 whitewater,
/// 4 the waves map), then controlVar1.w (the shore's normal distortion),
/// the waves map's alpha (the sea's share over the shore picture) and
/// controlVar1.z (the waves map's); rows 20 and 21 the deep colours'
/// deltas (the colours move with the waves map's depth), 22
/// horizonColor (alpha 1 when the material has one: the distant seas), 23
/// its fade (waterColorShallow.w per unit of distance, waterColorDeepDelta.w
/// at the eye), 24 customReflectionScale (the reflection's multiplier; 1
/// when the material has none). The noise map is all but flat: it goes in as its mean. The
/// waves map goes in as its open-water (median) values, and whole when it
/// is a real one (`sea_table`): the game reads it by the mesh's own uv.
/// Also the depth.
fn sea_layers(
    material: &asset_t6::MaterialRef,
    decoded: &dyn Fn(u32) -> Option<(usize, usize, Vec<u8>)>,
) -> (Option<Image>, f32, Vec<[f32; 4]>, Option<(usize, usize, Vec<u8>)>) {
    let c = |hash: u32| {
        material
            .constants
            .iter()
            .find(|c| c.0 == hash)
            .map_or([0.0; 4], |c| c.2)
    };
    let channel = |p: &(usize, usize, Vec<u8>), k: usize| -> Vec<f32> {
        p.2.iter().skip(k).step_by(4).map(|&v| f32::from(v) / 255.0).collect()
    };
    let median = |p: &(usize, usize, Vec<u8>), k: usize| {
        let mut v = channel(p, k);
        v.sort_unstable_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    let mean = |p: &(usize, usize, Vec<u8>), k: usize| {
        let v = channel(p, k);
        v.iter().sum::<f32>() / v.len().max(1) as f32
    };
    // No waves map: all sea, no shore (as before).
    let waves = decoded(0x6709_8a9b);
    let (foam, depth, share) = waves
        .as_ref()
        .map_or((0.0, 1.0, 1.0), |p| (median(p, 0), median(p, 1), median(p, 3)));
    // A flat one (the distant seas' white) needs no more than that.
    let waves = waves.filter(|p| p.0 >= 64 && p.1 >= 2);
    // Two noise samples multiplied, plus a quarter, normalised.
    let weights = decoded(0x7350_2222).map_or([0.25; 4], |p| {
        let w = [0, 1, 2, 3].map(|k| mean(&p, k) * mean(&p, k) + 0.25);
        let sum: f32 = w.iter().sum();
        w.map(|v| v / sum)
    });
    let mask = decoded(0x9e7b_a008);
    let shore = decoded(0xb1e0_21df);
    let mut flags = 0.0;
    let mut ratio = 1.0;
    let aux = mask.as_ref().or(shore.as_ref()).map(|&(w, h, _)| {
        let at = |p: &Option<(usize, usize, Vec<u8>)>, x: usize, y: usize, k: usize| {
            p.as_ref()
                .map_or(0, |(pw, ph, px)| px[((y * ph / h) * pw + x * pw / w) * 4 + k])
        };
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[
                    at(&mask, x, y, 1),
                    at(&mask, x, y, 3),
                    at(&shore, x, y, 1),
                    at(&shore, x, y, 2),
                ]);
            }
        }
        // Its mips (2x2 box), as the game's textures have: the mask repeats
        // every 400 units and would shimmer far out without them.
        let (mut lw, mut lh, mut at0, mut levels) = (w, h, 0, 1);
        while lw > 1 || lh > 1 {
            let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
            let next = px.len();
            for y in 0..nh {
                for x in 0..nw {
                    for k in 0..4 {
                        let s = |sx: usize, sy: usize| {
                            u32::from(px[at0 + ((sy.min(lh - 1)) * lw + sx.min(lw - 1)) * 4 + k])
                        };
                        let v = s(2 * x, 2 * y) + s(2 * x + 1, 2 * y) + s(2 * x, 2 * y + 1) + s(2 * x + 1, 2 * y + 1);
                        px.push(((v + 2) / 4) as u8);
                    }
                }
            }
            (lw, lh, at0, levels) = (nw, nh, next, levels + 1);
        }
        let mut image = Image::new_uninit(
            Extent3d {
                width: w as u32,
                height: h as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = levels;
        image.data = Some(px);
        image
    });
    if let Some(p) = &shore {
        flags += 1.0;
        ratio = mean(p, 0) / mean(p, 1).max(1.0 / 255.0);
    }
    if mask.is_some() {
        flags += 2.0;
    }
    if waves.is_some() {
        flags += 4.0;
    }
    // bo2mp: Carrier's dark dusk sea (`carrier_look`), as `water_table`'s.
    let tint = super::carrier_look::sea_tint().unwrap_or([1.0; 3]);
    let delta = |hash: u32| {
        let d = c(hash);
        [d[0] * tint[0], d[1] * tint[1], d[2] * tint[2], 0.0]
    };
    let horizon = material
        .constants
        .iter()
        .find(|c| c.0 == 0x4980_5a5a)
        .map_or([0.0; 4], |c| [c.2[0], c.2[1], c.2[2], 1.0]);
    let rows = vec![
        c(0x4c53_b0ff),
        c(0xa5ba_130a),
        c(0x4c53_b0bf),
        c(0xa5ba_12ca),
        c(0x2cc3_720f),
        c(0x19cc_0727),
        weights,
        [foam, depth, ratio, flags],
        [c(0x483f_c933)[3], share, c(0x483f_c933)[2], 0.0],
        delta(0xeef4_77a2),
        delta(0x9092_abc4),
        horizon,
        [c(0x7429_46aa)[3], c(0x9092_abc4)[3], 0.0, 0.0],
        material
            .constants
            .iter()
            .find(|c| c.0 == 0x0b19_8186)
            .map_or([1.0; 4], |c| c.2),
    ];
    // Rows 25 and 26: the sea's filtering (`sea_filter`).
    let mut rows = rows;
    rows.extend(super::sea_filter::rows());
    (aux, depth, rows, waves)
}

/// bo2mp: a sea's table with its waves map under it (rows 1.., RGBA16F,
/// the table's row padded to the map's width) for the shader to read by
/// the mesh's uv; without one, the table alone.
fn sea_table(rows: &[[f32; 4]], waves: Option<&(usize, usize, Vec<u8>)>) -> Image {
    let Some((w, h, px)) = waves.filter(|p| p.0 >= rows.len()) else {
        return water_image(rows);
    };
    let mut all = rows.to_vec();
    all.resize(*w, [0.0; 4]);
    all.extend(px.chunks_exact(4).map(|p| [0, 1, 2, 3].map(|k| f32::from(p[k]) / 255.0)));
    let mut image = water_image(&all);
    image.texture_descriptor.size = Extent3d {
        width: *w as u32,
        height: *h as u32 + 1,
        depth_or_array_layers: 1,
    };
    image
}

/// bo2mp: an emissive flow material's (molten lava) extra pictures, from
/// BO2's `cod7emissiveflow` shaders (fxc disassembly; textures and
/// constants by the hashes its register arguments name): its masks packed
/// in one RGBA8 picture (emissiveMapHi alpha, emissiveMapLo alpha,
/// noiseMap green, noiseMap alpha; resampled to the high mask's size), and
/// its colour remap with a last row of constants (RGBA16F): diffuse uv
/// control, emissive uv scale, emissive uv scroll, noise uv control,
/// diffuse control, emissive remap uv, diffuse filter, emissive filter.
fn flow_images(
    material: &asset_t6::MaterialRef,
    decoded: &dyn Fn(u32) -> Option<(usize, usize, Vec<u8>)>,
) -> Option<(Image, Image)> {
    let (w, h, hi) = decoded(0xd1bf_3646)?;
    let lo = decoded(0xd1bf_35c4)?;
    let noise = decoded(0x7350_2222)?;
    let at = |p: &(usize, usize, Vec<u8>), x: usize, y: usize, c: usize| {
        let (pw, ph, px) = p;
        px[((y * ph / h) * pw + x * pw / w) * 4 + c]
    };
    let mut mask = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            mask.extend_from_slice(&[
                hi[(y * w + x) * 4 + 3],
                at(&lo, x, y, 3),
                at(&noise, x, y, 1),
                at(&noise, x, y, 3),
            ]);
        }
    }
    let mask = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        mask,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    let (rw, rh, remap) = decoded(0xd0f6_1c30)?;
    if rw < 8 {
        return None;
    }
    let c = |hash: u32| {
        material
            .constants
            .iter()
            .find(|c| c.0 == hash)
            .map_or([0.0; 4], |c| c.2)
    };
    let constants = [
        c(0xb7a9_a24a),
        c(0xe75d_ae00),
        c(0xd31b_b535),
        c(0x2e12_415a),
        c(0x557d_6789),
        c(0xe6ed_ea93),
        c(0xd4f6_818e),
        c(0x1d90_b95b),
    ];
    let mut rows: Vec<[f32; 4]> = remap
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|k| f32::from(p[k]) / 255.0))
        .collect();
    let mut last = vec![[0.0f32; 4]; rw];
    last[..constants.len()].copy_from_slice(&constants);
    rows.extend(last);
    let mut table = water_image(&rows);
    table.texture_descriptor.size = Extent3d {
        width: rw as u32,
        height: rh as u32 + 1,
        depth_or_array_layers: 1,
    };
    Some((mask, table))
}

/// bo2mp: an emissive tile material's (Magma's lava rocks) glow as one
/// RGBA16F picture, from BO2's `sw4_3d_cod7_emissive_tile` shaders (fxc
/// disassembly): (emissive A + emissive B at Micro_Scale) * A's alpha *
/// hdrAmount, before the shader squares it; B averaged over the texels one
/// A texel covers. Then its constants (Micro_Scale, Micro_Height,
/// Macro_Height) as a 1x1 RGBA16F picture.
fn tile_images(
    material: &asset_t6::MaterialRef,
    decoded: &dyn Fn(u32) -> Option<(usize, usize, Vec<u8>)>,
) -> Option<(Image, Image)> {
    let (w, h, a) = decoded(0x7c73_d0d6)?;
    let (bw, bh, b) = decoded(0xa4af_0cde)?;
    let c = |hash: u32| {
        material
            .constants
            .iter()
            .find(|c| c.0 == hash)
            .map_or([0.0; 4], |c| c.2)
    };
    let hdr = c(0x00e2_62b2)[0];
    let micro = c(0x00ef_311d);
    let (sx, sy) = (micro[0].max(0.0), micro[1].max(0.0));
    let mut rows = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) * 4;
            let ta = [0, 1, 2, 3].map(|k| f32::from(a[at + k]) / 255.0);
            // B's texels under this A texel (micro scale times), wrapped.
            let span = |p: usize, s: f32, n: usize, bn: usize| {
                let lo = (p as f32 * s * bn as f32 / n as f32).floor() as i64;
                let hi = (((p + 1) as f32 * s * bn as f32 / n as f32).ceil() as i64).max(lo + 1);
                (lo, hi)
            };
            let (x0, x1) = span(x, sx, w, bw);
            let (y0, y1) = span(y, sy, h, bh);
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for by in y0..y1 {
                for bx in x0..x1 {
                    let px = bx.rem_euclid(bw as i64) as usize;
                    let py = by.rem_euclid(bh as i64) as usize;
                    let o = (py * bw + px) * 4;
                    for k in 0..3 {
                        sum[k] += f32::from(b[o + k]) / 255.0;
                    }
                    n += 1.0;
                }
            }
            let glow = [0, 1, 2].map(|k| (ta[k] + sum[k] / n.max(1.0)) * ta[3] * hdr);
            rows.push([glow[0], glow[1], glow[2], 1.0]);
        }
    }
    let mut glow = water_image(&rows);
    glow.texture_descriptor.size = Extent3d {
        width: w as u32,
        height: h as u32,
        depth_or_array_layers: 1,
    };
    let consts = water_image(&[[sx, sy, c(0x0e10_685a)[0], c(0x1f32_ca52)[0]]]);
    Some((glow, consts))
}

/// bo2mp: the water table as an RGBA16F picture (read by texel).
pub(super) fn water_image(rows: &[[f32; 4]]) -> Image {
    let data: Vec<u8> = rows
        .iter()
        .flatten()
        .flat_map(|&v| half_bits(v).to_le_bytes())
        .collect();
    let mut image = Image::new_uninit(
        Extent3d {
            width: rows.len() as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image
}

/// An objective material's two colours as a 2x1 sRGB picture (linear
/// colours in, so a sampled texel gives them back).
fn objective_image(lo: [f32; 4], hi: [f32; 4]) -> Image {
    let enc = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    let data = vec![
        enc(lo[0]),
        enc(lo[1]),
        enc(lo[2]),
        255,
        enc(hi[0]),
        enc(hi[1]),
        enc(hi[2]),
        255,
    ];
    let mut image = Image::new_uninit(
        Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image
}

/// The string hash of the water colours (`waterColorNear`, `waterColorS..`).
const WATER_COLOR_NEAR_HASH: u32 = 0xbd74_8e86;
const WATER_COLOR_S_HASH: u32 = 0xbd74_8e9b;
/// Black Ops 1's water (the Black Ops 1 maps' `wpc_cod7waterflowreflect`):
/// its colour in the shallows and the change to its colour in the deep.
const WATER_SHALLOW_HASH: u32 = name_hash("waterColorShallow");
const WATER_DEEP_DELTA_HASH: u32 = name_hash("waterColorDeepDelta");

/// A water material's colour as a 1x1 picture: waterColorNear and
/// waterColorS mixed half and half (the shader's mix by N.V at about 60
/// degrees), less its fresnel share there (0.05), alpha theirs. The world
/// shader squares the texel's raw value, so the raw value is the root of the
/// linear colour. Black Ops 1's water has a shallow colour and a deep one
/// (shallow plus the delta, by the depth behind it): drawn at half depth
/// (Shi No Numa's swamp: a muddy brown, clear at the banks, solid deep).
/// (bo2zm; named apart from bo2mp's `water_image` constant table.)
fn water_colour_image(material: &asset_t6::MaterialRef) -> Image {
    let colour = |hash: u32| material.constants.iter().find(|c| c.0 == hash).map(|c| c.2);
    let mix: [f32; 4] = match (colour(WATER_COLOR_NEAR_HASH), colour(WATER_SHALLOW_HASH)) {
        (None, Some(shallow)) => {
            let delta = colour(WATER_DEEP_DELTA_HASH).unwrap_or([0.0; 4]);
            std::array::from_fn(|k| shallow[k] + 0.5 * delta[k])
        }
        (near, _) => {
            let near = near.unwrap_or([0.08, 0.07, 0.04, 0.85]);
            let far = colour(WATER_COLOR_S_HASH).unwrap_or(near);
            std::array::from_fn(|k| 0.5 * (near[k] + far[k]))
        }
    };
    let alpha = mix[3].clamp(0.0, 1.0);
    // bo2mp: not one flat texel but a tile of faint ripples (three
    // crossing waves, whole cycles over the tile so it repeats): water
    // that flows (`T6Anim::Flow`) has something to carry. The mean stays
    // the colour above; the ripples vary it by a tenth.
    const SIDE: usize = 32;
    let mut data = Vec::with_capacity(SIDE * SIDE * 4);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (x, y) = (x as f32, y as f32);
            let wave = |a: f32, b: f32, phase: f32| {
                (std::f32::consts::TAU * (a * x + b * y) / SIDE as f32 + phase).sin()
            };
            let ripple = (wave(1.0, 2.0, 0.0) + wave(3.0, -1.0, 1.3) + wave(2.0, 3.0, 2.1)) / 3.0;
            let lift = 1.0 + 0.1 * ripple;
            for k in 0..3 {
                data.push(((mix[k] * 0.95 * lift).clamp(0.0, 1.0).sqrt() * 255.0).round() as u8);
            }
            data.push((alpha * 255.0).round() as u8);
        }
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: SIDE as u32,
            height: SIDE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image
}

/// Link the wanted capture materials into `catalog`, each with its colour
/// map decoded.
pub(crate) fn link_materials(
    catalog: &mut MaterialCatalog,
    capture: &ZoneCapture,
    packs: Option<&PackSet>,
    wanted: impl IntoIterator<Item = usize>,
    zone: &str,
) -> T6Materials {
    link_materials_with(catalog, capture, packs, wanted, zone, None)
}

/// A capture image by index; a comma-named one is a reference to the image
/// another zone defines, read from that zone when it is known.
fn resolve_image<'a>(
    capture: &'a ZoneCapture,
    foreign_images: Option<&'a HashMap<String, ImageRef>>,
    index: usize,
) -> Option<&'a ImageRef> {
    let image = capture.images.get(index)?;
    Some(
        image
            .name
            .strip_prefix(',')
            .and_then(|name| foreign_images?.get(name))
            .unwrap_or(image),
    )
}

/// Reads in flight at once while decoding up front.
const PREDECODE_THREADS: usize = 8;

/// `PREDECODE_THREADS`, or `CODLEGACY_PREDECODE_THREADS` (1 to 32). (Lane B
/// 11-05: a cold Village's map pictures read at 2.7 MB/s through 8 in
/// flight on the hard drive, the guns' at 9.4; the knob lets a quiet run
/// measure fewer without a build. The same pictures either way.)
fn predecode_threads() -> usize {
    codlegacy_env::var("CODLEGACY_PREDECODE_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| (1..=32).contains(n))
        .unwrap_or(PREDECODE_THREADS)
}

/// bo2mp maps: every image the wanted materials name, decoded up front on
/// several threads in pack-offset order. Read one by one in material order
/// a cold map's images seek all over the packs (about 3 MB/s on a hard
/// disk, 51 s for a 160-250 MB map); in offset order with several reads in
/// flight the disk serves them in about one sweep (3.5 s).
///
/// lane B (B3): each thread takes a batch of disk neighbours in one pack and
/// reads the ones not yet cached in a few long reads (`Pack::read_many`):
/// a first visit still read about 1,000 colour maps a seek each (Village
/// 98.5 s in materials).
fn predecode(
    packs: &PackSet,
    capture: &ZoneCapture,
    foreign_images: Option<&HashMap<String, ImageRef>>,
    materials: &[usize],
    skip: &dyn Fn(&ImageRef) -> bool,
) -> (HashMap<usize, Result<Image, String>>, String) {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    let start = std::time::Instant::now();
    let keys = image_keys(capture, materials);
    // The pack entry when the pack is where the picture comes from (not the
    // mod's archive); the rest are read alone by `decode`.
    let mut jobs: Vec<(usize, &ImageRef, Option<(usize, ipak_t6::IndexEntry)>)> = keys
        .into_iter()
        .filter_map(|k| {
            let image = resolve_image(capture, foreign_images, k)?;
            if skip(image) {
                return None;
            }
            let at = match packs.locate(image) {
                Some(ImageSource::Pack(i, e)) if !mod_holds(image) => Some((i, e)),
                _ => None,
            };
            Some((k, image, at))
        })
        .collect();
    jobs.sort_by_key(|j| j.2.map(|(i, e)| (i, e.offset)));
    let in_packs: Vec<_> = jobs.iter().filter_map(|j| j.2).collect();
    let stored: u64 = in_packs.iter().map(|(_, e)| u64::from(e.size)).sum();
    let (pictures, from_packs) = (jobs.len(), in_packs.len());
    // Batches: disk neighbours in one pack, `RUN_MAX` of the pack at most.
    let mut batches: Vec<&[_]> = Vec::new();
    let mut first = 0;
    for n in 1..=jobs.len() {
        let joins = jobs.get(n).is_some_and(|j| match (jobs[first].2, j.2) {
            (Some((a, s)), Some((b, e))) => {
                a == b
                    && u64::from(e.offset) + u64::from(e.size) - u64::from(s.offset)
                        <= asset_t6::images::RUN_MAX
            }
            _ => false,
        });
        if !joins {
            batches.push(&jobs[first..n]);
            first = n;
        }
    }
    let next = AtomicUsize::new(0);
    // (The pack reads' summed and slowest times, in microseconds: whether
    // the stage waits on the disk or on the decodes.)
    let (read_us, slowest_us) = (AtomicU64::new(0), AtomicU64::new(0));
    let threads = predecode_threads().min(batches.len());
    let images = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    while let Some(batch) = batches.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let Some((i, _)) = batch[0].2 else {
                            out.extend(batch.iter().map(|&(k, image, _)| (k, decode(packs, image))));
                            continue;
                        };
                        let entries: Vec<_> = batch.iter().filter_map(|j| j.2.map(|(_, e)| e)).collect();
                        let at = std::time::Instant::now();
                        let read = packs.packs[i].read_many(&entries);
                        let us = u64::try_from(at.elapsed().as_micros()).unwrap_or(u64::MAX);
                        read_us.fetch_add(us, Ordering::Relaxed);
                        slowest_us.fetch_max(us, Ordering::Relaxed);
                        out.extend(batch.iter().zip(read).map(|(&(k, _, _), bytes)| (k, pack_image(bytes))));
                    }
                    out
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });
    let report = format!(
        "t6 materials read ahead: {pictures} pictures ({from_packs} from packs, {:.1} MB stored, {} batches) in {} ms; {threads} threads, pack reads {} ms summed, slowest batch {} ms",
        stored as f64 / 1e6,
        batches.len(),
        start.elapsed().as_millis(),
        read_us.into_inner() / 1000,
        slowest_us.into_inner() / 1000
    );
    (images, report)
}

/// Whether a map's pictures come from its bundle file (`bundle_map`).
/// `BO2MP_T6_MAP_BUNDLE=0`: one cached file per picture (the old way).
fn map_bundle_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !codlegacy_env::var("BO2MP_T6_MAP_BUNDLE").is_ok_and(|v| v.trim() == "0"))
}

/// The fewest pictures worth a bundle file.
const MAP_BUNDLE_MIN: usize = 64;

/// Where map `map`'s bundle file lives: the map bake's folder
/// (`CODLEGACY_BAKE_DIR`, else `D:\codlegacy-bake`, `t6\<map>\t6pictures.pak`),
/// which CLEAR CACHE empties. `None`: the bake is off or has no drive.
pub(crate) fn map_bundle_path(map: &str) -> Option<std::path::PathBuf> {
    Some(map_bake::map_dir("t6", map)?.join(map_bake::clear::T6_PICTURES))
}

/// The pack entries the pictures of `materials` (by index) are stored in,
/// once each, in disk order (what `predecode` reads from the packs).
pub(crate) fn pack_entries(packs: &PackSet, capture: &ZoneCapture, materials: &[usize]) -> Vec<(usize, ipak_t6::IndexEntry)> {
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = image_keys(capture, materials)
        .into_iter()
        .filter_map(|k| resolve_image(capture, None, k))
        .filter(|image| !mod_holds(image))
        .filter_map(|image| match packs.locate(image) {
            Some(ImageSource::Pack(i, e)) => Some((i, e)),
            _ => None,
        })
        .collect();
    entries.sort_unstable_by_key(|&(i, e)| (i, e.offset, e.size));
    entries.dedup_by_key(|&mut (i, e)| (i, e.offset, e.size));
    entries
}

/// Load speed (22b6, BO2 Nuketown, warm cache): the map's 999 pictures
/// came from 999 files of the artifact cache, opened and read one by one
/// (materials 3.75 s, 29.4 s of reads summed on 8 threads, on a busy PC).
/// They are now one file per map in the map bake's folder on the data drive
/// (`map_bundle_path`; BAKE ALL writes it ahead with every picture the
/// map's materials name), read start to end, and `predecode`'s reads take
/// them from memory (`PackSet::drop_preloaded` frees what is left). A file
/// missing any of `materials`' pictures is written anew from the cache or
/// the packs, those ones handed out as it goes. Returns the report line
/// (`None`: off, no bake folder, or too few pictures).
pub(crate) fn bundle_map(packs: &PackSet, capture: &ZoneCapture, materials: &[usize], map: &str) -> Option<String> {
    if !map_bundle_on() {
        return None;
    }
    let path = map_bundle_path(map)?;
    let wanted = pack_entries(packs, capture, materials);
    if wanted.len() < MAP_BUNDLE_MIN {
        return None;
    }
    let start = std::time::Instant::now();
    let got = packs.preload_file(&path, &wanted);
    if got == wanted.len() {
        return Some(format!("t6 map bundle: {got} pictures from {} in {} ms", path.display(), start.elapsed().as_millis()));
    }
    Some(match packs.write_bundle_file(&path, &wanted, &wanted) {
        Ok(bytes) => format!(
            "t6 map bundle: {} pictures ({got} were in the old file) written to {} ({:.1} MB) in {} ms",
            wanted.len(),
            path.display(),
            bytes as f64 / 1e6,
            start.elapsed().as_millis()
        ),
        Err(e) => format!("t6 map bundle gap: {e} ({got} of {} pictures from the file)", wanted.len()),
    })
}

/// BAKE ALL (Black Ops II): map zone `zone`'s bundle file, with every
/// picture its materials name (a load reads the ones it links). Gameplay
/// data is not in it: only the packs' stored pictures. `Ok`: the line.
pub fn bake_map_bundle(zone: &std::path::Path) -> Result<String, String> {
    let map = zone.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let path = map_bundle_path(&map).ok_or("the map bake is off (CODLEGACY_BAKE=0) or has no drive")?;
    let capture = asset_t6::capture_zone(zone).map_err(|e| format!("{map}: {e}"))?;
    let packs = PackSet::open_dir(&asset_transport::t6_extra::install_zone_dir(zone)).map_err(|e| format!("{map}: {e}"))?;
    let all: Vec<usize> = (0..capture.materials.len()).collect();
    let entries = pack_entries(&packs, &capture, &all);
    if entries.is_empty() {
        return Ok(format!("{map}: no pictures in the packs"));
    }
    if packs.bundle_file_holds(&path, &entries) {
        return Ok(format!("{map} already baked"));
    }
    // (The bake's row: game, map, step and pictures.)
    let job = map_bake::status::begin("t6", &map, "pictures", entries.len() as u64);
    if job.is_canceled() {
        job.end(false);
        return Err(format!("{map}: canceled"));
    }
    let start = std::time::Instant::now();
    let written = packs.write_bundle_file(&path, &entries, &[]);
    job.add(entries.len() as u64);
    job.end(written.is_ok());
    let bytes = written?;
    Ok(format!(
        "{map}: {} pictures, {:.1} MB, to {} in {:.1} s",
        entries.len(),
        bytes as f64 / 1e6,
        path.display(),
        start.elapsed().as_secs_f32()
    ))
}

/// lane B: the texture slots the link loop hands to `link` (besides the
/// colour map `colour_texture` picks): the water normal maps, the layers,
/// the shine maps, the tile blend's tiles and macro map, a flag's edge. A
/// material's other slots (detail and occlusion maps, the sea and flow
/// pictures, which `decode` reads on their own) were read from the packs,
/// decoded and dropped. A slot missing here is still linked, read alone by
/// `decode`.
const LINKED_SLOTS: [u32; 15] = [
    0x1aca_8a8f,
    0x1aca_8a8e,
    0x1aca_7e8c,
    0x1aca_7f0e,
    LAYER_MAPS[0].0,
    LAYER_MAPS[1].0,
    SPECULAR_MAP1_HASH,
    SHINE_MAPS[0].0,
    SHINE_MAPS[1].0,
    SHINE_MAPS[2].0,
    0x13a7_8442,
    TILE_BLEND_MICRO1_HASH,
    TILE_BLEND_MICRO2_HASH,
    TILE_BLEND_MACRO_HASH,
    0xcdab_26c0,
];

/// The images `materials` of `capture` link, by index, once each.
fn image_keys(capture: &ZoneCapture, materials: &[usize]) -> Vec<usize> {
    let mut keys: Vec<usize> = materials
        .iter()
        .filter_map(|&mi| capture.materials.get(mi))
        .flat_map(|m| {
            m.textures
                .iter()
                .filter(|t| LINKED_SLOTS.contains(&t.name_hash))
                .chain(colour_texture(m, capture))
        })
        .filter_map(|t| t.image.map(|k| k.index))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// lane B: warms, in one disk-order sweep per pack, the pack entries every
/// capture's wanted materials read (and `extra`), so the per-capture
/// `predecode`s that follow find them cached. Each `predecode` sweeps only
/// its own capture's pictures: a stage that links many zones (the guns, 16
/// zones whose pictures share packs) swept the same packs once per zone.
/// Returns (entries, bytes, of them read from the packs: the artifact cache
/// did not hold them).
pub(crate) fn warm_captures<'a>(
    packs: &PackSet,
    captures: &[&'a ZoneCapture],
    wanted: &[std::collections::BTreeSet<usize>],
    foreign_images: Option<&'a HashMap<String, ImageRef>>,
    extra: impl IntoIterator<Item = &'a ImageRef>,
) -> (usize, usize, usize) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = captures
        .iter()
        .zip(wanted)
        .flat_map(|(c, want)| {
            let want: Vec<usize> = want.iter().copied().collect();
            image_keys(c, &want)
                .into_iter()
                .filter_map(|k| resolve_image(c, foreign_images, k))
                .collect::<Vec<_>>()
        })
        .chain(extra)
        .filter(|image| !mod_holds(image))
        .filter_map(|image| match packs.locate(image) {
            Some(ImageSource::Pack(i, e)) => Some((i, e)),
            _ => None,
        })
        .collect();
    entries.sort_by_key(|&(i, e)| (i, e.offset, e.size));
    entries.dedup_by_key(|&mut (i, e)| (i, e.offset, e.size));
    // Batches as `predecode` makes them: disk neighbours in one pack,
    // `RUN_MAX` of the pack at most, several in flight.
    let mut batches: Vec<&[(usize, ipak_t6::IndexEntry)]> = Vec::new();
    let mut first = 0;
    for n in 1..=entries.len() {
        let (a, s) = entries[first];
        let joins = entries.get(n).is_some_and(|&(b, e)| {
            a == b && u64::from(e.offset) + u64::from(e.size) - u64::from(s.offset) <= asset_t6::images::RUN_MAX
        });
        if !joins {
            batches.push(&entries[first..n]);
            first = n;
        }
    }
    let next = AtomicUsize::new(0);
    let (bytes, cold) = (AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|s| {
        for _ in 0..predecode_threads().min(batches.len()) {
            s.spawn(|| {
                while let Some(batch) = batches.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let list: Vec<_> = batch.iter().map(|&(_, e)| e).collect();
                    let (all, from_pack) = packs.packs[batch[0].0].warm_many_cold(&list);
                    bytes.fetch_add(all, Ordering::Relaxed);
                    cold.fetch_add(from_pack, Ordering::Relaxed);
                }
            });
        }
    });
    (entries.len(), bytes.into_inner(), cold.into_inner())
}

/// bo2mp: whether a material whose lit pass blends draws as an alpha-tested
/// cut-out instead. Black Ops II fills such a material's depth first in an
/// alpha-tested, colourless depth pre-pass (technique 0) and blends its
/// colour over that; the fallback pass has no pre-pass, so drawn blended
/// the surface left no depth and the sky and everything after it painted
/// over it (Turbine's and Yemen's leaves; the same for nets, tarps, wires
/// and grates built the same way). With the depth from the cut-out the
/// prop keeps its place; only the soft edge is lost. `foliage_only` is the
/// earlier rule (`BO2MP_T6_PREPASS_CUTOUT=foliage`): leaves alone.
pub fn prepass_cutout(
    blend: asset_core::T6Blend,
    foliage: bool,
    prepass: Option<&asset_core::T6StateBits>,
    foliage_only: bool,
) -> bool {
    (foliage || !foliage_only)
        && blend == asset_core::T6Blend::Blend
        && prepass.is_some_and(|p| p.alpha_test == Some(1) && p.blend_op == 0 && !p.color_write_rgb)
}

fn prepass_cutout_foliage_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("BO2MP_T6_PREPASS_CUTOUT").is_ok_and(|v| v.trim().eq_ignore_ascii_case("foliage")))
}

/// Load speed (one Black Ops II map to the next): the decoded pictures of
/// the zones every multiplayer map loads first (`common_mp`'s guns and
/// camos, the patch's), by zone and picture name. A map change read and
/// decoded common_mp's 721 again (about 190-350 ms of the 3.0-3.4 s), the
/// same pixels each time. Held, they cost about 230 MB through a match
/// (the resident map otherwise lets them go once they are on the GPU), so
/// it is opt-in: `CODLEGACY_T6_KEEP_GUN_PICTURES=1`.
static KEPT_PICTURES: std::sync::Mutex<Option<HashMap<String, HashMap<String, Arc<Image>>>>> =
    std::sync::Mutex::new(None);

/// Whether `KEPT_PICTURES` keeps them (see there).
fn keep_gun_pictures() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_T6_KEEP_GUN_PICTURES").is_ok_and(|v| v.trim() == "1"))
}

/// The kept pictures of `zone`, or none when it is not a shared zone.
fn kept_pictures(zone: &str) -> Option<HashMap<String, Arc<Image>>> {
    if !keep_gun_pictures() || !super::t6_m2::T6_MP_PRE_ZONES.contains(&zone) {
        return None;
    }
    let held = KEPT_PICTURES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(held.as_ref().and_then(|zones| zones.get(zone)).cloned().unwrap_or_else(HashMap::new))
}

/// Load speed (one Black Ops II map to the next): the pictures of the zones
/// every multiplayer map loads first (`common_mp`'s guns and camos, the
/// patch's) stay on the card between maps, with no copy of their pixels in
/// memory. The render keeps their textures by variant for the common set
/// (`ResidentGpuImages`); a picture it still holds is linked the next map
/// as a bare image (no pixels) under the same variant, so it is neither read
/// nor decoded nor uploaded again. One the card no longer has is decoded
/// again from the packs (`rebuild_card_image`). `CODLEGACY_T6_CARD_KEEP=0`:
/// the old way (every map decodes and uploads them).
fn card_keep() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_T6_CARD_KEEP").map_or(true, |v| v.trim() != "0"))
}

/// The `usage` half of a card-kept picture's variant (one sampler and
/// colour space per picture name, set by `gpu_image`).
const CARD_USAGE: u32 = 0x7436_6361;

/// One shared picture the card may hold: its pixel-less copy (the shape
/// the render looks up), whether the card held it at the last upload, and
/// where to decode it again.
struct CardPicture {
    bare: Arc<Image>,
    held: bool,
    image: ImageRef,
}

#[derive(Default)]
struct CardKept {
    /// The packs' folder of the last Black Ops II map load.
    dir: Option<std::path::PathBuf>,
    pictures: HashMap<ImageVariantId, CardPicture>,
    /// The packs opened for `rebuild_card_image`, until the upload ends.
    packs: Option<(std::path::PathBuf, Arc<PackSet>)>,
}

static CARD_KEPT: std::sync::Mutex<Option<CardKept>> = std::sync::Mutex::new(None);

fn card_lock() -> std::sync::MutexGuard<'static, Option<CardKept>> {
    CARD_KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Everything that decides a byte of the decoded picture: its pack entry,
/// the quality setting's skipped levels, the card's cap and the mod.
fn card_variant(image: &ImageRef) -> ImageVariantId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        "t6 card",
        &image.name,
        image.map_type,
        image.width,
        image.height,
        image.depth,
        image.level_count,
        image.streamed_parts,
        image.part_hash,
        asset_core::t6_fast(),
        bo2_picture_cap(),
        mod_holds(image),
    )
        .hash(&mut h);
    ImageVariantId {
        payload: h.finish(),
        usage: CARD_USAGE,
    }
}

/// The card variant of a picture of shared `zone` (none for another zone,
/// a picture carried in the zone itself, or the old way).
fn card_picture(image: &ImageRef, zone: &str) -> Option<ImageVariantId> {
    (card_keep() && image.embedded.is_none() && super::t6_m2::T6_MP_PRE_ZONES.contains(&zone)).then(|| card_variant(image))
}

/// The pictures the card held at the last upload.
fn cards_held() -> HashMap<ImageVariantId, Arc<Image>> {
    card_lock()
        .as_ref()
        .map(|kept| {
            kept.pictures
                .iter()
                .filter(|(_, p)| p.held)
                .map(|(v, p)| (*v, Arc::clone(&p.bare)))
                .collect()
        })
        .unwrap_or_else(HashMap::new)
}

/// A picture's shape with no pixels.
fn bare_image(gpu: &Image) -> Image {
    let mut bare = Image::default();
    bare.data = None;
    bare.texture_descriptor = gpu.texture_descriptor.clone();
    bare.sampler = gpu.sampler.clone();
    bare.texture_view_descriptor = gpu.texture_view_descriptor.clone();
    bare.asset_usage = gpu.asset_usage;
    bare
}

/// The packs' folder of the Black Ops II map being loaded (for
/// `rebuild_card_image`).
pub(crate) fn note_card_dir(dir: &std::path::Path) {
    if !card_keep() {
        return;
    }
    let mut slot = card_lock();
    let kept = slot.get_or_insert_with(CardKept::default);
    if kept.dir.as_deref() != Some(dir) {
        kept.dir = Some(dir.to_path_buf());
    }
}

/// The render's upload ended: which shared pictures the card holds now
/// (`live`). Those stay for the next map; the rest are decoded again.
/// Returns how many stay.
pub fn note_t6_card_held(live: &dyn Fn(ImageVariantId) -> bool) -> Option<usize> {
    let mut slot = card_lock();
    let kept = slot.as_mut()?;
    kept.packs = None;
    kept.pictures.retain(|v, p| {
        p.held = live(*v);
        p.held
    });
    Some(kept.pictures.len())
}

/// A card-kept picture whose texture the card no longer has, decoded again
/// from the packs (`None` for any other variant).
pub(crate) fn rebuild_card_image(variant: ImageVariantId) -> Option<Image> {
    if variant.usage != CARD_USAGE {
        return None;
    }
    let mut slot = card_lock();
    let kept = slot.as_mut()?;
    let image = kept.pictures.get(&variant)?.image.clone();
    let dir = kept.dir.clone()?;
    let packs = match &kept.packs {
        Some((at, packs)) if *at == dir => Arc::clone(packs),
        _ => {
            let packs = Arc::new(PackSet::open_dir(&dir).ok()?);
            kept.packs = Some((dir, Arc::clone(&packs)));
            packs
        }
    };
    drop(slot);
    decode(&packs, &image).ok()
}

/// `link_materials`, with the real images of other zones by name: a
/// comma-named image is a reference to one another zone defines.
pub(crate) fn link_materials_with(
    catalog: &mut MaterialCatalog,
    capture: &ZoneCapture,
    packs: Option<&PackSet>,
    wanted: impl IntoIterator<Item = usize>,
    zone: &str,
    foreign_images: Option<&HashMap<String, ImageRef>>,
) -> T6Materials {
    let owner = ZoneOwner::intern(zone);
    let mut images: HashMap<usize, Option<usize>> = HashMap::new();
    let mut local = HashMap::new();
    let (mut coloured, mut uncoloured, mut decoded) = (0usize, 0usize, 0usize);
    let mut emissive_only = 0usize;
    let mut emissive_names: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut draws: HashMap<T6Draw, usize> = HashMap::new();
    let wanted: Vec<usize> = wanted.into_iter().collect();
    let keep = kept_pictures(zone);
    let kept = keep.clone().unwrap_or_else(HashMap::new);
    let mut fresh: Vec<(String, Arc<Image>)> = Vec::new();
    let mut reused = 0usize;
    let card_zone = card_keep() && super::t6_m2::T6_MP_PRE_ZONES.contains(&zone);
    let cards = if card_zone { cards_held() } else { HashMap::new() };
    let mut card_fresh: Vec<(ImageVariantId, Arc<Image>, ImageRef)> = Vec::new();
    let mut card_reused = 0usize;
    let skip = |image: &ImageRef| {
        kept.contains_key(&image.name) || card_picture(image, zone).is_some_and(|v| cards.contains_key(&v))
    };
    let (mut predecoded, read_ahead) = packs
        .map(|p| predecode(p, capture, foreign_images, &wanted, &skip))
        .unwrap_or_default();
    for mi in wanted {
        if local.contains_key(&mi) {
            continue;
        }
        let Some(material) = capture.materials.get(mi) else {
            continue;
        };
        let technique_set = material
            .technique_set
            .and_then(|k| capture.technique_sets.get(k.index))
            .map_or("", |t| t.name.as_str());
        // The main technique's draw state: lit with sun shadow, lit with
        // sun, lit, emissive, then unlit (the first the material draws in).
        let state = [6usize, 5, 4, 3, 2].iter().find_map(|&t| {
            let entry = *material.state_bits_entry.get(t)?;
            material.state_bits.get(usize::from(entry)).copied()
        });
        let mut draw = T6Draw::from_technique_set(technique_set, material.sort_key, state);
        // bo2mp (Turbine/Yemen fix list): leaves draw their colour blended
        // with no depth write; BO2 fills the depth first in an alpha-tested
        // depth pre-pass (technique 0). Without that depth the sky, drawn
        // last, painted over every leaf with no terrain behind it. Drawn as
        // an alpha-tested cutout, the leaves keep their depth.
        let prepass = material
            .state_bits_entry
            .first()
            .and_then(|&e| material.state_bits.get(usize::from(e)))
            .map(|&b| asset_core::T6StateBits::decode(b));
        if prepass_cutout(draw.blend, draw.foliage, prepass.as_ref(), prepass_cutout_foliage_only()) {
            draw.blend = asset_core::T6Blend::Opaque;
            draw.alpha_test = true;
        }
        // bo2mp test aid: `CODLEGACY_T6_HIDE_MAT=a,b` leaves out every material
        // whose name contains one of them (bisecting a stray surface).
        if let Ok(hide) = codlegacy_env::var("CODLEGACY_T6_HIDE_MAT")
            && hide
                .split(',')
                .any(|h| !h.is_empty() && material.name.contains(h))
        {
            draw.blend = asset_core::T6Blend::ShadowOnly;
        }
        // bo2mp: a material whose surface flags say "only cast shadow"
        // (0x80000, the `onlycastshadow` infoParm: `wpc/shadowcaster` and
        // trees' shadow cards like Raid's `mtl_ctl_tree_shadow_caster`)
        // never draws in colour.
        if material.surface_flags & 0x8_0000 != 0 {
            draw.blend = asset_core::T6Blend::ShadowOnly;
        }
        // bo2zm 27c: a material that draws in the emissive technique and in
        // none of the lit ones (TECHNIQUE_LIT_BEGIN 4 to TECHNIQUE_LIT_END 26),
        // like Die Rise's cloud sheets, takes no light: BO2 draws it unlit.
        let drawn = |t: usize| {
            material
                .state_bits_entry
                .get(t)
                .is_some_and(|&e| usize::from(e) < material.state_bits.len())
        };
        if !draw.unlit && drawn(3) && !(4..26).any(drawn) {
            draw.unlit = true;
            emissive_only += 1;
            if emissive_names.len() < 8 {
                let constants: Vec<_> = material
                    .constants
                    .iter()
                    .map(|c| (c.1.as_str(), c.2))
                    .collect();
                emissive_names.push(format!("{} [{technique_set}] {constants:?}", material.name));
            }
        }
        // Unlit shaders scale their colour by the material's `scaleRGB`
        // (the light dome's is 32); powers of two up to 128 are kept.
        // bo2mp: the `sw4_3d_unlit` shaders (screens, backlit glass) scale
        // the texel by `hdrAmount` before squaring it into linear (fxc
        // disassembly: (rgb * a * hdrAmount)^2), so by its square.
        if draw.unlit {
            let sw4 = technique_set.split('_').any(|t| t == "sw4")
                && technique_set.split('_').any(|t| t == "unlit");
            let hdr_amount = material
                .constants
                .iter()
                .find(|c| c.1 == "hdrAmount")
                .map(|c| c.2[0] * c.2[0])
                .filter(|_| sw4);
            let scale = material
                .constants
                .iter()
                .find(|c| c.1.starts_with("scaleRGB"))
                .map(|c| c.2[0])
                .or(hdr_amount)
                .unwrap_or(1.0);
            draw.unlit_scale_exp = scale.max(1.0).log2().round().clamp(0.0, 7.0) as u8;
            // bo2zm 27c: Die Rise's cloud sheets (`scrolling_zfeather`) drift
            // and fade where the scene behind them is near; their colour
            // scale (`hdrAmount` squared) rides in their instance.
            draw.sheet = technique_set.contains("scrolling_zfeather");
            // bo2zm round 1: a zombie's eyes, scaled by its eye brightness.
            draw.eyes = technique_set.contains("zombie_eyes");
        }
        *draws
            .entry(T6Draw {
                sort_key: 0,
                polygon_offset: 0,
                unlit_scale_exp: 0,
                layers: [0, 0],
                cull: asset_core::T6Cull::None,
                ..draw
            })
            .or_default() += 1;
        let mut textures = Vec::new();
        let mut link = |key: asset_t6::AssetKey,
                        catalog: &mut MaterialCatalog,
                        failures: &mut Vec<String>,
                        decoded: &mut usize|
         -> Option<_> {
            *images.entry(key.index).or_insert_with(|| {
                let image = resolve_image(capture, foreign_images, key.index)?;
                let card = card_picture(image, zone).filter(|_| packs.is_some());
                let on_card = card.and_then(|v| cards.get(&v));
                let held = kept.get(&image.name).filter(|_| packs.is_some() && on_card.is_none());
                let decoded_image = match on_card.or(held).map(|h| Ok(Arc::clone(h))).or_else(|| {
                    packs.map(|p| {
                        predecoded
                            .remove(&key.index)
                            .unwrap_or_else(|| decode(p, image))
                            .map(|gpu| {
                                if let Some(v) = card {
                                    card_fresh.push((v, Arc::new(bare_image(&gpu)), image.clone()));
                                }
                                let gpu = Arc::new(gpu);
                                if keep.is_some() {
                                    fresh.push((image.name.clone(), Arc::clone(&gpu)));
                                }
                                gpu
                            })
                    })
                }) {
                    Some(Ok(gpu)) => {
                        *decoded += 1;
                        reused += usize::from(held.is_some());
                        card_reused += usize::from(on_card.is_some());
                        Some(gpu)
                    }
                    Some(Err(e)) => {
                        failures.push(format!("{}: {e}", image.name));
                        None
                    }
                    None => None,
                };
                let decoded_image = decoded_image?;
                Some(catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(image.name.clone()),
                    map_type: image.map_type,
                    semantic: TS_COLOR_MAP,
                    category: 0,
                    use_srgb_reads: true,
                    width: image.width,
                    height: image.height,
                    depth: image.depth,
                    level_count: image.level_count,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(decoded_image),
                    // A shared picture: the render keeps its texture for
                    // the next map under this variant (`card_keep`).
                    common_owned: card.is_some(),
                    decoded_variant: card,
                    decoded_by: None,
                    pending_decode: None,
                }))
            })
        };
        // bo2mp: a water material draws from its constants (a picture of
        // them in the colour map's place) and its two normal maps (in the
        // normal and specular maps' places).
        let water = asset_core::t6_water_kind(technique_set).filter(|_| draw.water);
        if let Some(kind) = water {
            // A sea also its whitewater and shore (layer 1's slot).
            let decoded_rgba = |hash: u32| {
                let key = material
                    .textures
                    .iter()
                    .find(|t| t.name_hash == hash)?
                    .image?;
                let image = capture.images.get(key.index)?;
                let gpu = decode(packs?, image).ok()?;
                top_level_rgba8(&gpu)
            };
            let (aux, depth, sea, waves) = if kind == 0 {
                (None, 1.0, Vec::new(), None)
            } else {
                sea_layers(material, &decoded_rgba)
            };
            let mut rows = water_table(material, kind, depth);
            rows.extend(sea);
            let table = sea_table(&rows, waves.as_ref());
            let table_size = table.texture_descriptor.size;
            if let Some(image) = aux {
                let size = image.texture_descriptor.size;
                let slot = catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(format!("{}#water_sea", material.name)),
                    map_type: 0,
                    semantic: 0xf1,
                    category: 0,
                    use_srgb_reads: false,
                    width: size.width as u16,
                    height: size.height as u16,
                    depth: 1,
                    level_count: 1,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(image)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                });
                textures.push(MaterialTextureBinding {
                    name_hash: 0,
                    name_start: 0,
                    name_end: 0,
                    sampler_state: 0x14,
                    semantic: 0xf1,
                    image: Some(slot),
                });
            }
            let slot = catalog.link_image(AuthoredImage {
                namespace: AssetNamespace::T6,
                name: AssetRef::Real(format!("{}#water", material.name)),
                map_type: 0,
                semantic: TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: false,
                width: table_size.width as u16,
                height: table_size.height as u16,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: Arc::new(Vec::new()),
                decoded: Some(Arc::new(table)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            });
            textures.push(MaterialTextureBinding {
                name_hash: 0xa0ab_1041,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x14,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
            // Pool: normalMap00, normalMap01; sea: normalMapLo, normalMapHi.
            let maps = if kind == 0 {
                [0x1aca_8a8f, 0x1aca_8a8e]
            } else {
                [0x1aca_7e8c, 0x1aca_7f0e]
            };
            for (hash, semantic) in maps.into_iter().zip([TS_NORMAL_MAP, TS_SPECULAR_MAP]) {
                if let Some(texture) = material.textures.iter().find(|t| t.name_hash == hash)
                    && let Some(key) = texture.image
                    && let Some(slot) = link(key, catalog, &mut failures, &mut decoded)
                {
                    textures.push(MaterialTextureBinding {
                        name_hash: texture.name_hash,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: texture.sampler_state,
                        semantic,
                        image: Some(slot),
                    });
                }
            }
        } else if draw.objective {
            // bo2zm M3: an objective material (the box's question marks)
            // draws its two pulse colours, not its (black) colour map: they
            // go in a 2x1 picture its shader reads (texel 0 colorObjMin, 1
            // colorObjMax).
            let colour = |name: &str| {
                material
                    .constants
                    .iter()
                    .find(|c| c.1 == name)
                    .map_or([0.0; 4], |c| c.2)
            };
            let image = objective_image(colour("colorObjMin"), colour("colorObjMax"));
            let slot = catalog.link_image(AuthoredImage {
                namespace: AssetNamespace::T6,
                name: AssetRef::Real(format!("{}#objective", material.name)),
                map_type: 0,
                semantic: TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width: 2,
                height: 1,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: Arc::new(Vec::new()),
                decoded: Some(Arc::new(image)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            });
            textures.push(MaterialTextureBinding {
                name_hash: 0xa0ab_1041,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x14,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
        } else if technique_set.contains("cod7water")
            && colour_texture(material, capture).is_none_or(|t| {
                // Black Ops 1's water keeps its sky reflection (a cube map)
                // in the colour slot; drawn as a colour map it was a flat
                // bright-blue sheet (Shi No Numa's swamp).
                t.image
                    .and_then(|k| capture.images.get(k.index))
                    .is_some_and(|img| img.map_type == 5)
            })
        {
            // bo2zm visuals: water (`cod7water`) the bo2mp water draw above
            // did not take, without a colour map. Its pixel shader (fxc
            // disassembly) mixes the scene behind towards
            // waterColorNear..waterColorS (by the view angle) times the
            // lightmap, by their alpha, under a fresnel reflection; drawn
            // here as that colour at a middle view angle, blended by its
            // alpha (it drew as the untextured blue diagnostic).
            draw.blend = asset_core::T6Blend::Blend;
            // (bo2mp: it flows: a tile of ripples slides along it.)
            draw.anim = asset_core::T6Anim::Flow;
            let slot = catalog.link_image(AuthoredImage {
                namespace: AssetNamespace::T6,
                name: AssetRef::Real(format!("{}#water_colour", material.name)),
                map_type: 0,
                semantic: TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width: 32,
                height: 32,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: Arc::new(Vec::new()),
                decoded: Some(Arc::new(water_colour_image(material))),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            });
            textures.push(MaterialTextureBinding {
                name_hash: COLOR_MAP_HASH,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x14,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
        } else if let Some(texture) = colour_texture(material, capture)
            && let Some(key) = texture.image
        {
            // bo2zm M3 fix list 1: a colour map whose pixels are in no pack
            // here (a reference into another zone, e.g. the leftover Maya
            // `lambert1` material's "grey") draws flat, as its name says,
            // not in the untextured diagnostic colours (his "weird
            // splotches ... of random colors").
            let slot = link(key, catalog, &mut failures, &mut decoded).unwrap_or_else(|| {
                let name = capture
                    .images
                    .get(key.index)
                    .map_or("", |i| i.name.as_str());
                catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(format!("{name}#flat")),
                    map_type: 0,
                    semantic: TS_COLOR_MAP,
                    category: 0,
                    use_srgb_reads: true,
                    width: 1,
                    height: 1,
                    depth: 1,
                    level_count: 1,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(flat_image(name))),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                })
            });
            textures.push(MaterialTextureBinding {
                name_hash: texture.name_hash,
                name_start: 0,
                name_end: 0,
                sampler_state: texture.sampler_state,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
        }
        // A layered material's extra colour maps (the fast quality setting
        // draws the base layer alone).
        if asset_core::t6_fast() {
            draw.layers = [0, 0];
        }
        if draw.layers.iter().any(|&k| k != 0) {
            for (hash, semantic) in LAYER_MAPS {
                if let Some(texture) = material.textures.iter().find(|t| t.name_hash == hash)
                    && let Some(key) = texture.image
                    && let Some(slot) = link(key, catalog, &mut failures, &mut decoded)
                {
                    textures.push(MaterialTextureBinding {
                        name_hash: texture.name_hash,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: texture.sampler_state,
                        semantic,
                        image: Some(slot),
                    });
                }
            }
        }
        // bo2mp: a material with one blend layer has no layer 2, so layer
        // 1's own specular map (`specularMap1`: Downhill's snow over its
        // ice) rides in layer 2's place for the shine.
        if draw.layers == [1, 0]
            && !draw.unlit
            && let Some(texture) = material.textures.iter().find(|t| t.name_hash == SPECULAR_MAP1_HASH)
            && let Some(key) = texture.image
            && let Some(slot) = link(key, catalog, &mut failures, &mut decoded)
        {
            textures.push(MaterialTextureBinding {
                name_hash: texture.name_hash,
                name_start: 0,
                name_end: 0,
                sampler_state: texture.sampler_state,
                semantic: T6_LAYER2_SEMANTIC,
                image: Some(slot),
            });
        }
        // The normal and specular maps BO2's lit shaders read (the shine);
        // the fast quality setting draws without them.
        // (`CODLEGACY_T6_NO_SHINE` leaves them out: a test aid for comparing.)
        if !asset_core::t6_fast()
            && !draw.unlit
            && water.is_none()
            && codlegacy_env::var_os("CODLEGACY_T6_NO_SHINE").is_none()
        {
            for (hash, semantic) in SHINE_MAPS {
                if let Some(texture) = material.textures.iter().find(|t| t.name_hash == hash)
                    && let Some(key) = texture.image
                    && let Some(slot) = link(key, catalog, &mut failures, &mut decoded)
                {
                    textures.push(MaterialTextureBinding {
                        name_hash: texture.name_hash,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: texture.sampler_state,
                        semantic,
                        image: Some(slot),
                    });
                }
            }
        }
        // bo2mp: an emissive flow material's masks (in the specular map's
        // place) and colour remap with constants (in layer 1's).
        // (`CODLEGACY_T6_NO_FLOW` leaves them out: a test aid for comparing.)
        // bo2mp: an emissive tile model material's glow (in the specular
        // map's place), macro normal map and constants (in layer 1's).
        if draw.tile && !technique_set.starts_with("wpc_") {
            let decoded_rgba = |hash: u32| {
                let key = material
                    .textures
                    .iter()
                    .find(|t| t.name_hash == hash)?
                    .image?;
                let image = capture.images.get(key.index)?;
                let gpu = decode(packs?, image).ok()?;
                top_level_rgba8(&gpu)
            };
            if let Some((glow, consts)) = tile_images(material, &decoded_rgba) {
                for (image, suffix, semantic) in
                    [(glow, "glow", TS_SPECULAR_MAP), (consts, "tile", 0xf1u8)]
                {
                    let size = image.texture_descriptor.size;
                    let slot = catalog.link_image(AuthoredImage {
                        namespace: AssetNamespace::T6,
                        name: AssetRef::Real(format!("{}#tile_{suffix}", material.name)),
                        map_type: 0,
                        semantic,
                        category: 0,
                        use_srgb_reads: false,
                        width: size.width as u16,
                        height: size.height as u16,
                        depth: 1,
                        level_count: 1,
                        format: 0,
                        payload: Arc::new(Vec::new()),
                        decoded: Some(Arc::new(image)),
                        common_owned: false,
                        decoded_variant: None,
                        decoded_by: None,
                        pending_decode: None,
                    });
                    textures.push(MaterialTextureBinding {
                        name_hash: 0,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: 0x14,
                        semantic,
                        image: Some(slot),
                    });
                }
                if let Some(texture) = material
                    .textures
                    .iter()
                    .find(|t| t.name_hash == 0x13a7_8442)
                    && let Some(key) = texture.image
                    && let Some(slot) = link(key, catalog, &mut failures, &mut decoded)
                {
                    textures.push(MaterialTextureBinding {
                        name_hash: texture.name_hash,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: texture.sampler_state,
                        semantic: TS_NORMAL_MAP,
                        image: Some(slot),
                    });
                }
            } else {
                draw.tile = false;
            }
        } else {
            draw.tile = false;
        }
        // bo2mp: a tile blend model material (cliffs, rocks, vista mountains):
        // Micro_1 as its colour map, Micro_2 in the specular map's place, the
        // macro AO/mix map in the normal map's, and Micro_1_Scale,
        // Micro_2_Scale, AO_Diffuse_Adj, EdgeHighlight (2x1 RGBA16F) in layer
        // 1's. World tile blends keep the stand-in colour map.
        if draw.tile_blend && !technique_set.starts_with("wpc_") {
            let mut slots = [None; 3];
            for (slot, hash) in slots.iter_mut().zip([
                TILE_BLEND_MICRO1_HASH,
                TILE_BLEND_MICRO2_HASH,
                TILE_BLEND_MACRO_HASH,
            ]) {
                *slot = material
                    .textures
                    .iter()
                    .find(|t| t.name_hash == hash)
                    .and_then(|t| t.image)
                    .and_then(|key| link(key, catalog, &mut failures, &mut decoded));
            }
            let [micro1, micro2, macro_map] = slots;
            if let (Some(micro1), Some(micro2), Some(macro_map)) = (micro1, micro2, macro_map) {
                let c = |hash: u32, default: [f32; 4]| {
                    material
                        .constants
                        .iter()
                        .find(|c| c.0 == hash)
                        .map_or(default, |c| c.2)
                };
                let s1 = c(0x1818_f373, [1.0; 4]);
                let s2 = c(0x7d8c_def0, [1.0; 4]);
                let ao = c(0x9923_36af, [0.0; 4])[0];
                let edge = c(0x259b_0793, [1.0; 4])[0];
                let consts = water_image(&[[s1[0], s1[1], s2[0], s2[1]], [ao, edge, 0.0, 0.0]]);
                let consts = catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(format!("{}#tile_blend", material.name)),
                    map_type: 0,
                    semantic: 0xf1,
                    category: 0,
                    use_srgb_reads: false,
                    width: 2,
                    height: 1,
                    depth: 1,
                    level_count: 1,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(consts)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                });
                textures.retain(|t| t.semantic != TS_COLOR_MAP);
                for (image, semantic) in [
                    (micro1, TS_COLOR_MAP),
                    (micro2, TS_SPECULAR_MAP),
                    (macro_map, TS_NORMAL_MAP),
                    (consts, 0xf1u8),
                ] {
                    textures.push(MaterialTextureBinding {
                        name_hash: if semantic == TS_COLOR_MAP {
                            COLOR_MAP_HASH
                        } else {
                            0
                        },
                        name_start: 0,
                        name_end: 0,
                        sampler_state: 0x14,
                        semantic,
                        image: Some(image),
                    });
                }
            } else {
                draw.tile_blend = false;
            }
        } else {
            draw.tile_blend = false;
        }
        // bo2mp: a tattered flag's frayed edge texture (in the specular map's
        // place) and its EdgeScale (a 1x1 picture in layer 1's place).
        if draw.flag {
            let edge = material
                .textures
                .iter()
                .find(|t| t.name_hash == 0xcdab_26c0)
                .and_then(|t| t.image)
                .and_then(|key| link(key, catalog, &mut failures, &mut decoded));
            if let Some(slot) = edge {
                let scale = material
                    .constants
                    .iter()
                    .find(|c| c.0 == 0x8240_aa7b)
                    .map_or([1.0; 4], |c| c.2);
                let consts = water_image(&[[scale[0], scale[1], 0.0, 0.0]]);
                let consts = catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(format!("{}#flag", material.name)),
                    map_type: 0,
                    semantic: 0xf1,
                    category: 0,
                    use_srgb_reads: false,
                    width: 1,
                    height: 1,
                    depth: 1,
                    level_count: 1,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(consts)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                });
                for (image, semantic) in [(slot, TS_SPECULAR_MAP), (consts, 0xf1u8)] {
                    textures.push(MaterialTextureBinding {
                        name_hash: 0,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: 0x14,
                        semantic,
                        image: Some(image),
                    });
                }
            } else {
                draw.flag = false;
            }
        }
        if draw.flow && codlegacy_env::var_os("CODLEGACY_T6_NO_FLOW").is_none() {
            let decoded_rgba = |hash: u32| {
                let key = material
                    .textures
                    .iter()
                    .find(|t| t.name_hash == hash)?
                    .image?;
                let image = capture.images.get(key.index)?;
                let gpu = decode(packs?, image).ok()?;
                top_level_rgba8(&gpu)
            };
            if let Some((mask, table)) = flow_images(material, &decoded_rgba) {
                for (image, suffix, semantic) in
                    [(mask, "mask", TS_SPECULAR_MAP), (table, "remap", 0xf1u8)]
                {
                    let size = image.texture_descriptor.size;
                    let slot = catalog.link_image(AuthoredImage {
                        namespace: AssetNamespace::T6,
                        name: AssetRef::Real(format!("{}#flow_{suffix}", material.name)),
                        map_type: 0,
                        semantic,
                        category: 0,
                        use_srgb_reads: false,
                        width: size.width as u16,
                        height: size.height as u16,
                        depth: 1,
                        level_count: 1,
                        format: 0,
                        payload: Arc::new(Vec::new()),
                        decoded: Some(Arc::new(image)),
                        common_owned: false,
                        decoded_variant: None,
                        decoded_by: None,
                        pending_decode: None,
                    });
                    textures.push(MaterialTextureBinding {
                        name_hash: 0,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: 0x14,
                        semantic,
                        image: Some(slot),
                    });
                }
            } else {
                draw.flow = false;
            }
        } else {
            draw.flow = false;
        }
        // bo2mp: how it moves. Black Ops II's own water and lava shaders
        // move by themselves; flags and tarps wave their vertices; a world
        // material by the name of a stream or a scroll flows.
        draw.anim = if draw.water || draw.flow || draw.effect || draw.cloud {
            asset_core::T6Anim::None
        } else if draw.flag {
            asset_core::T6Anim::Cloth
        } else {
            draw.anim.or(asset_core::t6_anim_kind(technique_set, &material.name, false))
        };
        if textures.is_empty() {
            uncoloured += 1;
        } else {
            coloured += 1;
        }
        let index = catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(material.name.clone()),
            namespace: AssetNamespace::T6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: material.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: material
                .constants
                .iter()
                .map(|(hash, name, literal)| {
                    let mut short = [0u8; 12];
                    for (d, b) in short.iter_mut().zip(name.bytes()) {
                        *d = b;
                    }
                    asset_material::MaterialConstant {
                        name_hash: *hash,
                        name: short,
                        literal: *literal,
                    }
                })
                .collect(),
            zone: owner,
            t6_draw: Some(draw),
        });
        local.insert(mi, index);
    }
    let mut report = vec![format!(
        "t6 materials: {} linked ({coloured} with a colour map, {uncoloured} without), {decoded} colour maps decoded from packs, {emissive_only} emissive-only drawn unlit, {} failed; draws {:?}",
        local.len(),
        failures.len(),
        draws
    )];
    if !read_ahead.is_empty() {
        report.push(read_ahead);
    }
    if card_zone {
        let added = card_fresh.len();
        let mut slot = card_lock();
        let kept_cards = slot.get_or_insert_with(CardKept::default);
        for (variant, bare, image) in card_fresh {
            kept_cards.pictures.insert(variant, CardPicture { bare, held: false, image });
        }
        report.push(format!(
            "t6 card keep ({zone}): {card_reused} pictures still on the card from the last map (not read, decoded or uploaded), {added} decoded (CODLEGACY_T6_CARD_KEEP)"
        ));
    }
    if keep.is_some() {
        let added = fresh.len();
        let mut held = KEPT_PICTURES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let zone_held = held.get_or_insert_with(HashMap::new).entry(zone.to_owned()).or_insert_with(HashMap::new);
        zone_held.extend(fresh);
        report.push(format!(
            "t6 kept pictures ({zone}): {reused} reused, {added} newly kept, {} held (CODLEGACY_T6_KEEP_GUN_PICTURES)",
            zone_held.len()
        ));
    }
    for f in failures.iter().take(8) {
        report.push(format!("t6 colour map gap: {f}"));
    }
    for n in &emissive_names {
        report.push(format!("t6 emissive-only material: {n}"));
    }
    T6Materials { local, report }
}

/// bo2zm M4: whether a material adds its colour onto what is under it
/// (blend ONE/ONE, or SRC_ALPHA/ONE) - BO2's menu brackets and glows,
/// black where nothing shows.
pub(crate) fn is_additive(material: &asset_t6::MaterialRef) -> bool {
    (0..36).filter_map(|t| material.draw_state(t)).any(|d| d.dst_blend == 2 && matches!(d.src_blend, 2 | 5))
}

/// bo2mp vehicle screens: a 2D material's AddMap picture (texture name
/// hash 0xe98a255d, the `sw4_2d_color_add` pixel shader's `AddMap`, t0),
/// when it has one: the VTOL Warship's reticles (`mp_hud_sentry_outer_white`
/// = `mp_hud_sentry_outer_blend` colour map + `mp_hud_sentry_outer_add`).
pub(crate) fn add_map<'a>(material: &asset_t6::MaterialRef, capture: &'a ZoneCapture) -> Option<&'a ImageRef> {
    let t = material.textures.iter().find(|t| t.name_hash == ADD_MAP_HASH)?;
    capture.images.get(t.image?.index)
}

const ADD_MAP_HASH: u32 = 0xe98a_255d;

/// bo2mp vehicle screens: BO2's `sw4_2d_color_add` pixel shader reads only
/// the colour map's alpha (`rgb = (ColorMap.a * colour.rgb + AddMap.rgb) *
/// colour.a`, blend ONE/ONE). Packed: (AddMap colour, colour map alpha) for
/// the additive pass, and a white-lit flat copy (colour map alpha plus the
/// AddMap's colour as brightness-alpha) for layers that only alpha-blend.
pub(crate) fn pack_color_add(color: &Image, added: &Image) -> Option<(Image, Image)> {
    let (w, h, mut rgba) = top_level_rgba8(color)?;
    let (aw, ah, add) = top_level_rgba8(added)?;
    let mut flat = rgba.clone();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            // (The AddMap read at the same place, nearest texel.)
            let j = ((y * ah / h) * aw + x * aw / w) * 4;
            let a = rgba[i + 3];
            for c in 0..3 {
                rgba[i + c] = add[j + c];
                flat[i + c] = a.saturating_add(add[j + c]);
            }
            flat[i + 3] = 255;
        }
    }
    let packed = rgba8_image(color, w, h, rgba);
    let flat = additive_to_alpha(&rgba8_image(color, w, h, flat))?;
    Some((packed, flat))
}

/// bo2zm M4: an additive picture for a 2D layer that only alpha-blends: its
/// top level as RGBA with the brightness as alpha (black = see-through),
/// colour divided back out.
pub(crate) fn additive_to_alpha(image: &Image) -> Option<Image> {
    let (w, h, mut rgba) = top_level_rgba8(image)?;
    for px in rgba.chunks_exact_mut(4) {
        let a = u32::from(px[0].max(px[1]).max(px[2]));
        let alpha = a * u32::from(px[3]) / 255;
        if a > 0 {
            for c in &mut px[..3] {
                *c = (u32::from(*c) * 255 / a).min(255) as u8;
            }
        }
        px[3] = alpha as u8;
    }
    Some(rgba8_image(image, w, h, rgba))
}

/// bo2zm M4: the left `keep` of a picture's columns, the rest see-through
/// (the lobby's quiet speaker: `voice_on` without its sound waves).
pub(crate) fn left_part(image: &Image, keep: f32) -> Option<Image> {
    let (w, h, mut rgba) = top_level_rgba8(image)?;
    let cut = (w as f32 * keep).round() as usize;
    for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
        if i % w >= cut {
            px[3] = 0;
        }
    }
    Some(rgba8_image(image, w, h, rgba))
}

/// bo2mp: a picture's top level as plain RGBA8 (the minimap reads its
/// pixels to turn it).
pub(crate) fn to_rgba8(image: &Image) -> Option<Image> {
    let (w, h, rgba) = top_level_rgba8(image)?;
    Some(rgba8_image(image, w, h, rgba))
}

fn rgba8_image(like: &Image, w: usize, h: usize, rgba: Vec<u8>) -> Image {
    let mut out = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.sampler = like.sampler.clone();
    out
}

/// A decoded picture's top level as RGBA8 bytes (block formats decoded, the
/// bytes as stored: no sRGB conversion): width, height, pixels.
fn top_level_rgba8(image: &Image) -> Option<(usize, usize, Vec<u8>)> {
    let w = image.texture_descriptor.size.width as usize;
    let h = image.texture_descriptor.size.height as usize;
    let data = image.data.as_ref()?;
    let (block, decode): (usize, fn(&[u8], &mut [u8], usize)) = match image.texture_descriptor.format {
        TextureFormat::Bc1RgbaUnormSrgb | TextureFormat::Bc1RgbaUnorm => (8, bcdec_rs::bc1),
        TextureFormat::Bc2RgbaUnormSrgb | TextureFormat::Bc2RgbaUnorm => (16, bcdec_rs::bc2),
        TextureFormat::Bc3RgbaUnormSrgb | TextureFormat::Bc3RgbaUnorm => (16, bcdec_rs::bc3),
        TextureFormat::Rgba8UnormSrgb => (0, |_, _, _| {}),
        _ => return None,
    };
    let mut rgba = vec![0u8; w * h * 4];
    if block == 0 {
        rgba.copy_from_slice(data.get(..w * h * 4)?);
    } else {
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        let mut tile = [0u8; 64];
        for by in 0..bh {
            for bx in 0..bw {
                let at = (by * bw + bx) * block;
                decode(data.get(at..at + block)?, &mut tile, 16);
                for y in 0..4 {
                    for x in 0..4 {
                        let (px, py) = (bx * 4 + x, by * 4 + y);
                        if px < w && py < h {
                            let d = (py * w + px) * 4;
                            let s = (y * 4 + x) * 4;
                            rgba[d..d + 4].copy_from_slice(&tile[s..s + 4]);
                        }
                    }
                }
            }
        }
    }
    Some((w, h, rgba))
}

#[cfg(test)]
mod image_keys_tests {
    use super::*;

    fn image(name: &str) -> ImageRef {
        ImageRef {
            name: name.into(),
            name_hash: 0,
            map_type: 3,
            width: 64,
            height: 64,
            depth: 1,
            level_count: 1,
            streamed_parts: 1,
            part_hash: 0,
            embedded: None,
        }
    }

    fn slot(name_hash: u32, semantic: u8, index: usize) -> asset_t6::MaterialTexture {
        asset_t6::MaterialTexture {
            name_hash,
            semantic,
            sampler_state: 0,
            image: Some(asset_t6::AssetKey {
                ty: fastfile_t6::AssetType::Image,
                index,
            }),
        }
    }

    fn material(textures: Vec<asset_t6::MaterialTexture>) -> asset_t6::MaterialRef {
        asset_t6::MaterialRef {
            name: "mc/test".into(),
            sort_key: 0,
            game_flags: 0,
            surface_type_bits: 0,
            surface_flags: 0,
            contents: 0,
            technique_set: None,
            textures,
            constants: Vec::new(),
            state_bits_entry: [0xff; 36],
            state_bits: Vec::new(),
            state_flags: 0,
            camera_region: 0,
        }
    }

    /// lane B: the read-ahead takes the colour map and the slots the link
    /// loop links, and leaves the rest (a detail map, the sea's wave map,
    /// which `decode` reads on its own) out.
    #[test]
    fn read_ahead_takes_only_linked_slots() {
        let mut capture = ZoneCapture::default();
        capture.images = ["wall_c", "wall_n", "wall_s", "wall_detail", "sea_waves", "wall_c1"]
            .map(image)
            .to_vec();
        capture.materials = vec![material(vec![
                slot(COLOR_MAP_HASH, TS_COLOR_MAP, 0),
                slot(NORMAL_MAP_HASH, TS_NORMAL_MAP, 1),
                slot(SPECULAR_MAP_HASH, TS_SPECULAR_MAP, 2),
                slot(name_hash("detailMap"), 0, 3),
                slot(0x6709_8a9b, 0, 4),
                slot(LAYER_MAPS[0].0, TS_COLOR_MAP, 5),
            ])];
        assert_eq!(image_keys(&capture, &[0]), vec![0, 1, 2, 5]);
    }

    /// A colour map in a slot the list does not name is still read ahead:
    /// `colour_texture` picks it.
    #[test]
    fn read_ahead_keeps_the_colour_pick() {
        let mut capture = ZoneCapture::default();
        capture.images = ["laser_c", "laser_detail"].map(image).to_vec();
        capture.materials = vec![material(vec![
            slot(LASER_SCAN_IMAGE_HASH, 0, 0),
            slot(name_hash("detailMap"), 0, 1),
        ])];
        assert_eq!(image_keys(&capture, &[0]), vec![0]);
    }
}

#[cfg(test)]
mod prepass_cutout_tests {
    use super::prepass_cutout;
    use asset_core::{T6Blend, T6StateBits};

    fn prepass(alpha_test: Option<u8>, colour: bool) -> T6StateBits {
        T6StateBits { src_blend: 0, dst_blend: 0, blend_op: 0, alpha_test, cull: 2, color_write_rgb: colour, polygon_offset: 0 }
    }

    #[test]
    fn a_blended_material_with_an_alpha_tested_colourless_prepass_draws_as_a_cut_out() {
        let p = prepass(Some(1), false);
        assert!(prepass_cutout(T6Blend::Blend, false, Some(&p), false));
        assert!(prepass_cutout(T6Blend::Blend, true, Some(&p), false));
        assert!(prepass_cutout(T6Blend::Blend, true, Some(&p), true));
        assert!(!prepass_cutout(T6Blend::Blend, false, Some(&p), true));
    }

    #[test]
    fn other_materials_keep_their_blend() {
        let p = prepass(Some(1), false);
        assert!(!prepass_cutout(T6Blend::Opaque, true, Some(&p), false));
        assert!(!prepass_cutout(T6Blend::Add, true, Some(&p), false));
        assert!(!prepass_cutout(T6Blend::Blend, true, None, false));
        assert!(!prepass_cutout(T6Blend::Blend, true, Some(&prepass(None, false)), false));
        assert!(!prepass_cutout(T6Blend::Blend, true, Some(&prepass(Some(1), true)), false));
    }
}

#[cfg(test)]
mod mip_skip_tests {
    use super::mip_skip;

    #[test]
    fn the_card_cap_drops_top_levels_down_to_the_cap() {
        // Plenty of memory: as stored, one level under the fast setting.
        assert_eq!(mip_skip(2048, 2048, 12, false, false, u32::MAX), 0);
        assert_eq!(mip_skip(2048, 2048, 12, false, true, u32::MAX), 1);
        // A 4 GB card: 2048 pictures lose one level, 1024 ones none.
        assert_eq!(mip_skip(2048, 2048, 12, false, false, 1024), 1);
        assert_eq!(mip_skip(1024, 1024, 11, false, false, 1024), 0);
        assert_eq!(mip_skip(2048, 1024, 12, false, false, 1024), 1);
        // The fast setting and the cap take the larger of the two.
        assert_eq!(mip_skip(2048, 2048, 12, false, true, 1024), 1);
        assert_eq!(mip_skip(2048, 2048, 12, false, true, 512), 2);
        // A picture with one level, a cube map, or a level that would not
        // be whole blocks, keeps what it has.
        assert_eq!(mip_skip(2048, 2048, 1, false, false, 512), 0);
        assert_eq!(mip_skip(2048, 2048, 12, true, false, 512), 0);
        assert_eq!(mip_skip(2048, 2048, 2, false, false, 256), 1);
    }
}

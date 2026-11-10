//! bo2zm: Black Ops II XModel -> ModelMesh. Not part of upstream IW4L.
//!
//! A T6 model vertex (`GfxPackedVertex`, 32 bytes; measured on Nuketown's
//! placed models, t6mat probe) is laid out like the world's: position f32x3
//! @0, binormal sign @12, colour R,G,B,A @16, texcoord half2 @20 (u low),
//! normal @24 and tangent @28 packed 10:10:10 signed. Every vertex lies in
//! its model's bounds (model space). Each is repacked into the 32-byte IW4
//! packed vertex the engine's static-model path carries, and the decoded
//! position, normal, colour and texcoord go into the surface's mesh.
//!
//! Only the first LOD is built. Surfaces have no collision payload yet.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use asset_iw4::size as iw4_sz;
use asset_t6::{AssetKey, XModelRef};

use crate::model_mesh::{ModelMesh, ModelSurfaceDraw, XSurfaceCollisionPayload};
use crate::world_t6::unpack_t6_unit_vec;

const T6_PACKED_VERTEX: usize = 32;

fn rd_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn rd_f32(b: &[u8], at: usize) -> f32 {
    f32::from_bits(rd_u32(b, at))
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// The IW4 packed unit vector: biased bytes over a fixed scale of 1/127.
fn pack_iw4_unit_vec(n: [f32; 3]) -> u32 {
    let b = |v: f32| ((v * 127.0 + 127.5) as i32).clamp(0, 255) as u8;
    u32::from_le_bytes([b(n[0]), b(n[1]), b(n[2]), 63])
}

/// Build a T6 model's first LOD. `material` maps each surface's material to
/// the lane's catalog index. `None` when the model has no drawable surface.
pub fn build_t6_model_mesh(
    model: &XModelRef,
    material: impl Fn(AssetKey) -> Option<usize>,
) -> Option<ModelMesh> {
    let mut surfaces = Vec::with_capacity(model.lod0.len());
    let (mut vertices, mut triangles) = (0usize, 0usize);
    for srf in &model.lod0 {
        let n = usize::from(srf.vert_count);
        if n == 0 || srf.indices.len() < 3 || srf.verts.len() < n * T6_PACKED_VERTEX {
            continue;
        }
        if srf.indices.iter().any(|&i| usize::from(i) >= n) {
            continue;
        }
        let mut positions = Vec::with_capacity(n);
        let mut normals = Vec::with_capacity(n);
        let mut colors = Vec::with_capacity(n);
        let mut uvs = Vec::with_capacity(n);
        let mut packed_vertices = Vec::with_capacity(n);
        for v in srf.verts.chunks_exact(T6_PACKED_VERTEX).take(n) {
            let position = [rd_f32(v, 0), rd_f32(v, 4), rd_f32(v, 8)];
            let [r, g, b, a] = [v[16], v[17], v[18], v[19]];
            let tex = rd_u32(v, 20);
            let uv = [
                asset_model::half_to_f32(tex as u16),
                asset_model::half_to_f32((tex >> 16) as u16),
            ];
            let normal = normalized(unpack_t6_unit_vec(rd_u32(v, 24)));
            let tangent = normalized(unpack_t6_unit_vec(rd_u32(v, 28)));

            let mut packed = [0u8; iw4_sz::GFX_PACKED_VERTEX];
            packed[0..16].copy_from_slice(&v[0..16]);
            packed[16..20].copy_from_slice(&[b, g, r, a]);
            // IW4 packed texcoords hold u in the high half.
            packed[20..24].copy_from_slice(&tex.rotate_left(16).to_le_bytes());
            packed[24..28].copy_from_slice(&pack_iw4_unit_vec(normal).to_le_bytes());
            packed[28..32].copy_from_slice(&pack_iw4_unit_vec(tangent).to_le_bytes());

            positions.push(position);
            normals.push(normal);
            colors.push([r, g, b, a].map(|c| f32::from(c) / 255.0));
            uvs.push(uv);
            packed_vertices.push(packed);
        }
        let indices: Vec<u32> = srf.indices.iter().map(|&i| u32::from(i)).collect();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        vertices += n;
        triangles += indices.len() / 3;
        mesh.insert_indices(Indices::U32(indices));
        surfaces.push(ModelSurfaceDraw {
            mesh,
            material: srf.material.and_then(&material),
            packed_vertices,
            xsurface_plus_1: None,
            xsurface_base_index: 0,
            xsurface_vert_offset: 0,
            collision: XSurfaceCollisionPayload::Unavailable {
                source_layout: "t6",
            },
        });
    }
    if surfaces.is_empty() {
        return None;
    }
    Some(ModelMesh {
        name: model.name.clone(),
        lod_surfaces: [surfaces, Vec::new(), Vec::new(), Vec::new()],
        vertices,
        triangles,
        lod_smc: None,
        lod: None,
    })
}

/// A copy of `mesh` whose vertex colours hold an instance's baked vertex
/// light (`lmapVertexColors`, R,G,B,A bytes, one per first-LOD vertex in
/// surface order); the fallback draw lights such an instance with
/// `colour^2 * 32`, as the game's `mlv_*` shaders do. `None` when the counts
/// disagree. Surfaces whose material `unlit` names keep their own colour:
/// an unlit technique reads no baked light, and the map's compiler stored
/// black for them (Nuketown's moving truck: its multiply logo drew black).
/// Surfaces whose material `foliage` names (tree canopies) keep the light's
/// alpha too, packed as the canopy's vertex shader takes it apart: alpha is
/// 2 + the model's own alpha byte + 256 * the light's alpha byte, and each of
/// red, green and blue the model's own byte + the light's byte / 256.
pub fn t6_vertex_lit_mesh(
    mesh: &ModelMesh,
    colors: &[u32],
    unlit: impl Fn(Option<usize>) -> bool,
    foliage: impl Fn(Option<usize>) -> bool,
) -> Option<ModelMesh> {
    let mut lit = mesh.clone();
    let mut at = 0usize;
    for srf in &mut lit.lod_surfaces[0] {
        let n = srf.packed_vertices.len();
        let block = colors.get(at..at + n)?;
        at += n;
        if unlit(srf.material) {
            continue;
        }
        // bo2zm M3 fix list 2: the light takes the colour's red, green and
        // blue; the alpha stays the model's own, which fades its decals
        // (the garage doors' burn layer is see-through but where painted:
        // drawn solid, it covered the doors in black-and-white speckle).
        let canopy = foliage(srf.material);
        let own = own_vertex_colors(&srf.mesh);
        let baked: Vec<[f32; 4]> = block
            .iter()
            .enumerate()
            .map(|(k, &word)| bake_vertex(word, own.get(k).copied().unwrap_or([1.0; 4]), canopy))
            .collect();
        srf.mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, baked);
    }
    (at == colors.len()).then_some(lit)
}

/// The colours a surface's mesh carries already (none when it has no colour set).
fn own_vertex_colors(mesh: &Mesh) -> Vec<[f32; 4]> {
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(c)) => c.clone(),
        _ => Vec::new(),
    }
}

/// One vertex's colour from its baked-light word and its own colour: plain
/// surfaces take the light's RGB with their own alpha; a canopy packs its own
/// bytes plus the light's into all four channels.
fn bake_vertex(word: u32, own: [f32; 4], canopy: bool) -> [f32; 4] {
    let bytes = word.to_le_bytes();
    if !canopy {
        let [r, g, b, _] = bytes.map(|v| f32::from(v) / 255.0);
        return [r, g, b, own[3]];
    }
    let own = own.map(|v| (v.clamp(0.0, 1.0) * 255.0).round());
    let channel = |i: usize| own[i] + f32::from(bytes[i]) / 256.0;
    [channel(0), channel(1), channel(2), 2.0 + own[3] + 256.0 * f32::from(bytes[3])]
}

#[cfg(test)]
mod bake_tests {
    use super::*;

    #[test]
    fn plain_vertex_takes_light_rgb_and_own_alpha() {
        let v = bake_vertex(0x8040_2010, [0.2, 0.3, 0.4, 0.5], false);
        assert_eq!(v, [16.0 / 255.0, 32.0 / 255.0, 64.0 / 255.0, 0.5]);
    }

    #[test]
    fn canopy_vertex_packs_own_bytes_and_light() {
        let v = bake_vertex(0x8040_2010, [0.2, 0.4, 0.6, 0.5], true);
        assert_eq!(v, [51.0 + 16.0 / 256.0, 102.0 + 32.0 / 256.0, 153.0 + 64.0 / 256.0, 2.0 + 128.0 + 256.0 * 128.0]);
        assert_eq!(bake_vertex(0, [2.0, -1.0, 0.0, 9.0], true)[..3], [255.0, 0.0, 0.0]);
    }
}

/// A T6 light grid in the engine's IW4 form, so its sampler can find the
/// entries around a point and blend them. T6 grids keep SH coefficient sets
/// (`GfxCompressedLightGridCoeffs`: 27 u16, coefficient = (u16 - 32768) /
/// 32768, the first three the base colour) where IW4 keeps 56 directional
/// RGB8 colours; each set becomes 56 copies of its base colour, encoded so a
/// sampled byte `v` decodes as the game's model lighting does,
/// `(v / 255)^2 * 32`. Directional terms are not used yet.
pub fn t6_light_grid(grid: &asset_t6::LightGridRef) -> Option<asset_model::OwnedLightGrid> {
    const SET: usize = 54;
    const DIRS: usize = 56;
    if grid.row_axis > 2 || grid.col_axis > 2 || grid.coeffs.len() < SET {
        return None;
    }
    let encode =
        |light: f32| -> u8 { ((light / 32.0).clamp(0.0, 1.0).sqrt() * 255.0).round() as u8 };
    let mut colors = Vec::with_capacity(grid.coeffs.len() / SET * DIRS * 3);
    for set in grid.coeffs.chunks_exact(SET) {
        let base = [0usize, 1, 2].map(|c| {
            let raw = u16::from_le_bytes([set[c * 2], set[c * 2 + 1]]);
            (f32::from(raw) - 32768.0) / 32768.0
        });
        let rgb = base.map(encode);
        for _ in 0..DIRS {
            colors.extend_from_slice(&rgb);
        }
    }
    Some(asset_model::OwnedLightGrid {
        mins: grid.mins,
        maxs: grid.maxs,
        row_axis: grid.row_axis as usize,
        col_axis: grid.col_axis as usize,
        color_count: (grid.coeffs.len() / SET) as u32,
        has_light_regions: false,
        sun_primary_light_index: grid.sun_primary_light_index,
        row_data_start: grid.row_data_start.clone(),
        raw_row_data: grid.raw_row_data.clone(),
        entries: grid.entries.clone(),
        colors,
        color_encoding: asset_model::LightGridColorEncoding::Rgb8,
    })
}

/// The light (linear, before exposure) a T6 grid gives at `pos`: the mean
/// of the sampled tile, decoded `(v / 255)^2 * 32`.
pub fn t6_grid_light(grid: &asset_model::OwnedLightGrid, pos: [f32; 3]) -> Option<[f32; 3]> {
    let sampled = asset_model::sample_light_grid(&grid.view(), pos).ok()?;
    Some(t6_sampled_light(&sampled))
}

/// `t6_grid_light` for a sample already taken.
pub fn t6_sampled_light(sampled: &asset_model::SampledLighting) -> [f32; 3] {
    let mean = asset_model::mean_tile_rgba01(&sampled.tile);
    mean.map(|v| v * v * 32.0)
}

/// The visibility (0..255) a T6 grid gives primary light `primary` at
/// `pos`: each grid entry carries a primary light and that light's
/// visibility; this is their weighted mean over the corners lit by
/// `primary`, or over every live corner when none is.
pub fn t6_grid_visibility(
    grid: &asset_model::OwnedLightGrid,
    pos: [f32; 3],
    primary: u8,
) -> Option<u8> {
    let sampled = asset_model::sample_light_grid(&grid.view(), pos).ok()?;
    t6_sampled_visibility(&sampled, primary)
}

/// `t6_grid_visibility` for a sample already taken.
pub fn t6_sampled_visibility(sampled: &asset_model::SampledLighting, primary: u8) -> Option<u8> {
    let mean = |only: Option<u8>| {
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        for i in 0..8 {
            let (Some(p), Some(v)) = (sampled.corner_primaries[i], sampled.corner_flags[i]) else {
                continue;
            };
            if only.is_some_and(|o| o != p) {
                continue;
            }
            sum += sampled.weights[i] * f32::from(v);
            weight += sampled.weights[i];
        }
        (weight > 1e-6).then(|| (sum / weight).round().clamp(0.0, 255.0) as u8)
    };
    mean(Some(primary)).or_else(|| mean(None))
}

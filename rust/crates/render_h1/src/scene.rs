//! A map ready to draw: the zones it needs read once, its materials,
//! the world and static model meshes, the draws, the model lighting tiles,
//! lightmaps, probes, sun and look settings. Pictures are read on demand.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fastfile_h1::image::{ImagePaks, ImagePixels, image_pixels};
use fastfile_h1::walk::{AssetId, Capture};
use glam::{Mat4, Vec3};

use crate::code::{SunLight, code_index};
use crate::look::Settings;
use crate::pool::{Pool, key};
use crate::world::{Draw, ImageSource, Material, Object, VertexKind};

pub struct Zone {
    pub path: PathBuf,
    pub zone: fastfile_h1::Zone,
    pub cap: Capture,
}

/// Owned vertices and indices.
pub struct MeshData {
    pub kind: VertexKind,
    pub vertex_bytes: Vec<u8>,
    pub indices: Vec<u16>,
    /// The vertex layer stream (world only; `layers`).
    pub layer_bytes: Vec<u8>,
}

/// Pictures by name from the zones (and the pak files beside them).
pub struct ZoneImages {
    zones: std::sync::Arc<Vec<Zone>>,
    paks: Vec<ImagePaks>,
    /// Every (zone, image) holding a picture of that name, in zone order.
    by_name: HashMap<String, Vec<(usize, usize)>>,
    pub max_size: u32,
}

impl ImageSource for ZoneImages {
    fn image(&mut self, name: &str) -> Option<(ImagePixels, u32)> {
        if let Some(px) = fastfile_h1::shared::builtin_pixels(name) {
            return Some((px, 1));
        }
        // The first zone holding the picture that reads (a usermap may name
        // a streamed picture whose pak it does not ship; another zone may
        // hold it whole).
        let candidates = self.by_name.get(&key(name))?.clone();
        candidates.into_iter().find_map(|(z, i)| self.read(z, i))
    }
}

impl ZoneImages {
    fn read(&mut self, z: usize, i: usize) -> Option<(ImagePixels, u32)> {
        let l = &self.zones[z];
        let rec = &l.cap.images[i];
        // map_type 5: a cube, its six faces one after another.
        if rec.map_type == 5 {
            let data = rec.pixels?.get(&l.zone.image).to_vec();
            return Some((
                ImagePixels {
                    width: u32::from(rec.width),
                    height: u32::from(rec.height),
                    levels: u32::from(rec.level_count.max(1)),
                    dxgi_format: rec.format,
                    data,
                },
                6,
            ));
        }
        image_pixels(rec, &l.zone.image, &mut self.paks[z], self.max_size)
            .ok()
            .map(|px| (px, 1))
    }
}

pub struct Scene {
    pub name: String,
    pub zones: std::sync::Arc<Vec<Zone>>,
    pub pool: std::sync::Arc<Pool>,
    pub materials: Vec<Material>,
    /// 0: the world, 1: static models.
    pub meshes: Vec<MeshData>,
    pub objects: Vec<Object>,
    pub draws: Vec<Draw>,
    /// Per lighting tile: 56 grid colours and the sun weight.
    pub tiles: Vec<([[f32; 3]; 56], f32)>,
    pub lightmaps: Vec<(Option<String>, Option<String>)>,
    pub probes: Vec<Option<String>>,
    pub sun: SunLight,
    /// The map's spot (type 2) and omni (type 3) lights with a radius.
    pub local_lights: Vec<fastfile_h1::map::PrimaryLight>,
    pub vision: Settings,
    pub fog: Settings,
    /// The map's HDR light set (`maps/createart/<map>_lightsets_hdr.csv`):
    /// tone map, exposure, colour scales, veil.
    pub lightset: Settings,
    /// The clip map's stages (`HDR_STAGE_PARMS` per view; `stage.rs`).
    pub stages: crate::stage::Stages,
    /// The world's shadow bounds (`GfxWorld::shadowBounds`, min and max):
    /// the sun shadow map's depth range is this box along the sun.
    pub shadow_bounds: (Vec3, Vec3),
    pub entities: String,
    pub images: ZoneImages,
    pub notes: Vec<String>,
    /// Each glass pane's object (by pane number, `glass::panes` order over
    /// the map's glass systems; None: not drawn), so a broken pane can be
    /// hidden by moving its object.
    pub glass_objects: Vec<Option<u32>>,
    /// The `bo2mp h1 props:` line of what lights the placed models.
    pub props_line: String,
}

impl Scene {
    /// `zones[0]` is the map; the others supply what it only names
    /// (`techsets_common_mp`, `code_post_gfx_mp`, ...).
    pub fn load(zones: &[PathBuf]) -> Result<Self, String> {
        let mut notes = Vec::new();
        // The map's zone must open; a shared zone that does not (Horizon
        // MW's `hmw_launcher.ff` is not a zone) is left out with a note.
        let mut loaded: Vec<Zone> = Vec::with_capacity(zones.len());
        for (i, p) in zones.iter().enumerate() {
            let zone = match fastfile_h1::Zone::open(p) {
                Ok(zone) => zone,
                Err(e) if i == 0 => return Err(format!("{}: {e:?}", p.display())),
                Err(e) => {
                    notes.push(format!("{}: left out, does not open ({e:?})", p.display()));
                    continue;
                }
            };
            let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
            if let Some(s) = stop {
                notes.push(format!(
                    "{}: walk stopped at asset {} ({:?})",
                    p.display(),
                    s.asset_index,
                    s.asset_type
                ));
            }
            loaded.push(Zone {
                path: p.clone(),
                zone,
                cap,
            });
        }
        let map_l = loaded.first().ok_or("no map zone")?;
        let name = zones[0]
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let mut pool = Pool::default();
        for l in loaded.iter().skip(1).chain(loaded.iter().take(1)) {
            pool.add(&l.cap, &l.zone.image);
        }
        let mut mats_by_name: HashMap<String, (usize, usize)> = HashMap::new();
        let mut imgs_by_name: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (zi, l) in loaded.iter().enumerate() {
            for (mi, m) in l.cap.materials.iter().enumerate() {
                if m.techset.is_some() {
                    mats_by_name.entry(key(&m.name)).or_insert((zi, mi));
                }
            }
            for (ii, r) in l.cap.images.iter().enumerate() {
                if r.format != 0 && !r.name.starts_with(',') {
                    imgs_by_name.entry(key(&r.name)).or_default().push((zi, ii));
                }
            }
        }
        let map = fastfile_h1::map::read_map(
            &map_l.cap,
            &map_l.zone.image,
            &map_l.zone.assets.script_strings,
        );
        notes.extend(map.notes.iter().cloned());
        let world = map.world.as_ref().ok_or("no world in the map zone")?;
        let name_of = |id: Option<AssetId>| id.map(|id| map_l.cap.name_of(id).to_string());

        let mut materials: Vec<Material> = Vec::new();
        let mut mat_index: HashMap<String, usize> = HashMap::new();
        let mut material_of = |id: AssetId| -> Option<usize> {
            let nm = key(map_l.cap.name_of(id));
            if let Some(&i) = mat_index.get(&nm) {
                return Some(i);
            }
            let &(zi, mi) = mats_by_name.get(&nm)?;
            let l = &loaded[zi];
            let m = &l.cap.materials[mi];
            // Horizon MW's usermaps give some converted surfaces (water,
            // clear plastic, the estate fences) the `2d` technique set: a
            // screen-space menu technique whose vertex shader takes the
            // model's positions as screen positions, which draws spikes
            // across the view. They are left out of the 3-D scene
            // (INFERRED: what the game does with them was not read);
            // `BO2MP_H1_DRAW_2D=1` draws them.
            if m.techset.is_some_and(|t| l.cap.name_of(t) == "2d")
                && std::env::var("BO2MP_H1_DRAW_2D").is_err()
            {
                return None;
            }
            materials.push(Material {
                name: m.name.clone(),
                techset: m
                    .techset
                    .map_or(String::new(), |t| l.cap.name_of(t).to_string()),
                textures: m
                    .textures
                    .iter()
                    .map(|t| {
                        (
                            t.name_hash,
                            t.sampler_state,
                            t.semantic,
                            t.image
                                .map_or(String::new(), |i| l.cap.name_of(i).to_string()),
                        )
                    })
                    .collect(),
                constants: {
                    if let Ok(w) = std::env::var("H1_DBG_MATCONST")
                        && m.name.contains(w.as_str())
                    {
                        eprintln!("dbg matconst {}: {:?}", m.name, m.constants);
                    }
                    m.constants.clone()
                },
                state_bits: m
                    .state_bits
                    .iter()
                    .map(|b| {
                        let mut a = [0u8; 40];
                        let n = b.len().min(40);
                        a[..n].copy_from_slice(&b[..n]);
                        a
                    })
                    .collect(),
                state_entry: m.state_bits_entry.clone(),
                sort_key: m.sort_key,
                camera_region: m.camera_region,
            });
            mat_index.insert(nm, materials.len() - 1);
            Some(materials.len() - 1)
        };

        // World surfaces: every drawn range and the sky.
        let mut draws = Vec::new();
        let mut seen = HashSet::new();
        for s in world
            .ranges
            .iter()
            .flat_map(|&(b, e)| b.min(e)..e)
            .chain(world.sky_surfaces.iter().copied())
            .filter(|i| seen.insert(*i))
            .filter_map(|i| world.surfaces.get(i as usize))
        {
            let Some(mi) = s.material.and_then(&mut material_of) else {
                continue;
            };
            draws.push(Draw {
                mesh: 0,
                object: None,
                material: mi,
                lightmap: s.lightmap_index,
                probe: s.reflection_probe_index,
                first_index: s.base_index,
                index_count: u32::from(s.tri_count) * 3,
                base_vertex: s.first_vertex as i32,
                layer: Some(s.vertex_layer_data),
                light_env: u16::from(s.primary_light_env),
                lod: None,
            });
        }

        // Static models: each xmodel's first LOD once in the model mesh.
        let mut model_verts: Vec<u8> = Vec::new();
        let mut model_idx: Vec<u16> = Vec::new();
        // Per xmodel: (material, base vertex, first index, index count) per surface.
        type Placed = Vec<(Option<AssetId>, i32, u32, u32, u32, u32, (u8, f32, f32))>;
        // The vertex-lit placements' lighting values (stream 1 of the model mesh).
        let mut model_layer: Vec<u8> = Vec::new();
        let mut vlit_mismatch = 0usize;
        let mut placed: HashMap<u32, Placed> = HashMap::new();
        let mut objects: Vec<Object> = Vec::new();
        let mut tiles: Vec<([[f32; 3]; 56], f32)> = Vec::new();
        let world_matrix = code_index("TRANSPOSE_WORLD_MATRIX0");
        let base_coords = code_index("BASE_LIGHTING_COORDS");
        let probe_ambient = code_index("LIGHT_PROBE_AMBIENT");
        let model_lightmap_params = code_index("MODEL_LIGHTMAP_PARAMS");
        // Zone tools' usermaps leave every weight 0 (not baked).
        let weights_baked = world
            .static_models
            .iter()
            .any(|m| m.primary_light_weight.is_some_and(|w| w > 0.0));
        if !weights_baked {
            notes.push("static model sun weights not baked: full sun".into());
        }
        // Retail maps: the colour index is a palette entry (the exe's
        // single-entry model lighting). `BO2MP_H1_LIGHTGRID=off` leaves the
        // palette unread, as before (those models went unlit).
        let palette = world
            .light_grid_palette
            .as_ref()
            .filter(|_| palette_from_env());
        if let Some(p) = palette {
            notes.push(format!(
                "h1 light grid: palette of {} entries, range exponents {:?}",
                p.len(),
                p.exponents
            ));
        }
        let mut palette_cache: HashMap<u32, Option<[[f32; 3]; 56]>> = HashMap::new();
        // The clutter and destructibles (the dynamic entity list), placed
        // models in their starting pose.
        let dyn_models = map
            .clip
            .as_ref()
            .map(|c| fastfile_h1::map::dyn_ent_models(c, &world.static_models))
            .unwrap_or_default();
        notes.push(format!("h1 dynamic entities drawn as placed models: {}", dyn_models.len()));
        for sm in world.static_models.iter().chain(dyn_models.iter()) {
            let Some(mid) = sm.model else { continue };
            let xm = &map_l.cap.xmodels[mid.index as usize];
            let surfs = placed
                .entry(mid.index)
                .or_insert_with(|| {
                    let mut out = Vec::new();
                    // The levels that hold surfaces, each used from where the
                    // one before ends (`dist` is a level's far end; the last
                    // level goes on). `BO2MP_H1_LODS=0`: the first level only.
                    let one = std::env::var("BO2MP_H1_LODS").is_ok_and(|v| v.trim() == "0");
                    let valid: Vec<usize> = xm
                        .lods
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.numsurfs > 0 && l.surfs.is_some())
                        .map(|(i, _)| i)
                        .take(if one { 1 } else { usize::MAX })
                        .collect();
                    for (level, &li) in valid.iter().enumerate() {
                    let lod = &xm.lods[li];
                    let min = if level == 0 { 0.0 } else { xm.lods[valid[level - 1]].dist };
                    let max = if level + 1 == valid.len() { f32::INFINITY } else { lod.dist };
                    let range = (level as u8, min, max);
                    let Some(ms) = lod
                        .surfs
                        .and_then(|s| map_l.cap.model_surfs.get(s.index as usize))
                    else {
                        continue;
                    };
                    // (Each surface's first vertex in the model's vertex
                    // order: where its vertex lighting values start.)
                    let mut vstart = 0u32;
                    for (j, surf) in ms.surfs.iter().enumerate() {
                        let surf_vstart = vstart;
                        vstart += u32::from(surf.vert_count);
                        let (Some(v), Some(t)) = (surf.verts, surf.tris) else {
                            continue;
                        };
                        // `GfxPackedVertex` (32 bytes) and the lightmap
                        // unwrap (4 bytes, zero when the surface has none).
                        let stride = VertexKind::Model.stride() as usize;
                        let base = (model_verts.len() / stride) as i32;
                        let unwrap = surf.lmap_unwrap.map(|u| u.get(&map_l.zone.image));
                        if sm.model_lightmap.is_some() && std::env::var("H1_DBG_MLM_PRINT").is_ok() {
                            eprintln!("mlm surf {j} verts {} unwrap {}", surf.vert_count, unwrap.is_some());
                        }
                        for (k, pv) in v.get(&map_l.zone.image).chunks_exact(32).enumerate() {
                            model_verts.extend_from_slice(pv);
                            match unwrap.and_then(|u| u.get(4 * k..4 * k + 4)) {
                                Some(u) => model_verts.extend_from_slice(u),
                                None => model_verts.extend_from_slice(&[0; 4]),
                            }
                        }
                        let first = model_idx.len() as u32;
                        model_idx.extend(
                            t.get(&map_l.zone.image)
                                .chunks_exact(2)
                                .map(|b| u16::from_le_bytes([b[0], b[1]])),
                        );
                        let mat = xm
                            .materials
                            .get(usize::from(lod.surf_index) + j)
                            .copied()
                            .flatten();
                        out.push((
                            mat,
                            base,
                            first,
                            u32::from(surf.tri_count) * 3,
                            surf_vstart,
                            u32::from(surf.vert_count),
                            range,
                        ));
                    }
                    }
                    out
                })
                .clone();
            let shell = sm
                .grid_color
                .and_then(|c| world.light_grid_colors.get(c as usize).copied())
                .or_else(|| {
                    world
                        .light_grid
                        .as_ref()?
                        .sample(&world.light_grid_colors, sm.origin)
                })
                .or_else(|| {
                    let (p, c) = (palette?, sm.grid_color?);
                    *palette_cache.entry(c).or_insert_with(|| p.shell(c))
                });
            let tile = shell.map(|c| {
                let w = if weights_baked {
                    sm.primary_light_weight.unwrap_or(1.0)
                } else {
                    1.0
                };
                tiles.push((c, w.clamp(0.0, 1.0)));
                tiles.len() - 1
            });
            let (s, a) = (sm.scale, sm.axis);
            let rows: Vec<[f32; 4]> = (0..3)
                .map(|i| [a[0][i] * s, a[1][i] * s, a[2][i] * s, sm.origin[i]])
                .chain(std::iter::once([0.0, 0.0, 0.0, 1.0]))
                .collect();
            let world_m =
                Mat4::from_cols_array_2d(&[rows[0], rows[1], rows[2], rows[3]]).transpose();
            let mut obj = Object {
                rows: vec![(world_matrix, rows)],
                world: world_m,
                tile,
            };
            if let Some(t) = tile {
                obj.rows
                    .push((base_coords, vec![[t as f32, 0.0, 0.0, 0.0]]));
            }
            // The engine's static model draw sets LIGHT_PROBE_AMBIENT from
            // the model's ground lighting (flag 0x20) as it is.
            if sm.flags & 0x20 != 0 {
                obj.rows.push((probe_ambient, vec![sm.ground_lighting]));
            }
            if sm.flags & 0x20 == 0
                && let Some(v) = std::env::var("H1_DBG_PROBE_AMB").ok().and_then(|v| v.parse::<f32>().ok())
            {
                obj.rows.push((probe_ambient, vec![[v, v, v, 0.0]]));
            }
            // Model lightmap (flag 0x100): the unwrap lands on its page at
            // `MODEL_LIGHTMAP_PARAMS.xy * uv + .zw` (the shader's maths; the
            // scale-then-offset order is inferred from the field names).
            let mut page = 0u8;
            if let Some(ml) = sm
                .model_lightmap
                .filter(|_| std::env::var("H1_DBG_NOMLM").is_err())
            {
                page = u8::try_from(ml.page).unwrap_or(0);
                if let Some(p) = std::env::var("H1_DBG_MLM_PAGE").ok().and_then(|v| v.parse().ok()) {
                    page = p;
                }
                if std::env::var("H1_DBG_MLM_PRINT").is_ok() {
                    eprintln!(
                        "mlm at {:?}: offset {:?} scale {:?} page {} flags {:#x}",
                        sm.origin, ml.offset, ml.scale, ml.page, sm.flags
                    );
                }
                let row = if std::env::var("H1_DBG_MLM_SWAP").is_ok() {
                    [ml.offset[0], ml.offset[1], ml.scale[0], ml.scale[1]]
                } else {
                    [ml.scale[0], ml.scale[1], ml.offset[0], ml.offset[1]]
                };
                let mut row = row;
                match std::env::var("H1_DBG_MLM_FLIP").as_deref() {
                    Ok("v") => {
                        row[3] += row[1];
                        row[1] = -row[1];
                    }
                    Ok("u") => {
                        row[2] += row[0];
                        row[0] = -row[0];
                    }
                    Ok("t") => row = [row[1], row[0], row[3], row[2]],
                    _ => {}
                }
                obj.rows.push((model_lightmap_params, vec![row]));
            }
            objects.push(obj);
            let oi = objects.len() - 1;
            for (mat, base, first, count, vstart, vcount, (level, lmin, lmax)) in surfs {
                // (A vertex-lit placement's lighting values are its first
                // level's: no other level for it, so the first goes on to
                // every distance, or the placement vanished past its first
                // level's end; `BO2MP_H1_VLIT_LOD_END=1` keeps that end.)
                if level > 0 && sm.vertex_lighting.is_some() {
                    continue;
                }
                let lmax = if vlit_lod_end() { lmax } else { lod_far_end(level, lmax, sm.vertex_lighting.is_some()) };
                let lod = (lmax.is_finite() || level > 0).then_some(crate::world::Lod { level, min: lmin, max: lmax, center: sm.origin });
                let Some(mi) = mat.and_then(&mut material_of) else {
                    continue;
                };
                // A vertex-lit placement (flag 0x80): its surface's
                // lighting values become the model mesh's second stream
                // (`TEXCOORD4..6` of the `_vlc` techniques).
                let layer = sm.vertex_lighting.and_then(|(pos, len)| {
                    let vl = &map_l.zone.image[pos..pos + len];
                    let (a, b) = (20 * vstart as usize, 20 * (vstart + vcount) as usize);
                    match vl.get(a..b) {
                        Some(bytes) => {
                            let at = model_layer.len() as u32;
                            model_layer.extend_from_slice(bytes);
                            Some(at)
                        }
                        None => {
                            vlit_mismatch += 1;
                            None
                        }
                    }
                });
                draws.push(Draw {
                    mesh: 1,
                    object: Some(oi as u32),
                    material: mi,
                    lightmap: page,
                    probe: sm.reflection_probe_index,
                    first_index: first,
                    index_count: count,
                    base_vertex: base,
                    layer,
                    light_env: sm.primary_light_env,
                    lod,
                });
            }
        }
        // The intact breakable glass: whole two-sided panes in the model mesh
        // (a placed object with the identity transform).
        let mut glass_objects: Vec<Option<u32>> = Vec::new();
        if crate::glass::enabled() {
            let mut drawn = 0usize;
            let mut skipped = 0usize;
            let identity: Vec<[f32; 4]> = vec![
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
            for rec in &map_l.cap.fx_glass {
                for pane in crate::glass::panes(rec, &map_l.zone.image) {
                    glass_objects.push(None);
                    let number = glass_objects.len() - 1;
                    let Some(mat) = rec.defs.get(pane.def).and_then(|d| d.material) else {
                        skipped += 1;
                        continue;
                    };
                    let Some(mi) = material_of(mat) else {
                        skipped += 1;
                        continue;
                    };
                    // An object of its own: a broken pane is hidden by
                    // moving it (`Host::sync_glass`).
                    objects.push(Object {
                        rows: vec![(world_matrix, identity.clone())],
                        world: Mat4::IDENTITY,
                        tile: None,
                    });
                    let glass_obj = (objects.len() - 1) as u32;
                    glass_objects[number] = Some(glass_obj);
                    let stride = VertexKind::Model.stride() as usize;
                    let base = (model_verts.len() / stride) as i32;
                    for v in &pane.verts {
                        model_verts.extend_from_slice(v);
                        model_verts.extend_from_slice(&[0; 4]);
                    }
                    let first = model_idx.len() as u32;
                    model_idx.extend_from_slice(&pane.indices);
                    draws.push(Draw {
                        mesh: 1,
                        object: Some(glass_obj),
                        material: mi,
                        lightmap: 0,
                        probe: 0,
                        first_index: first,
                        index_count: pane.indices.len() as u32,
                        base_vertex: base,
                        layer: None,
                        light_env: 0,
                        lod: None,
                    });
                    drawn += 1;
                }
            }
            notes.push(format!("h1 glass: {drawn} intact panes drawn, {skipped} without a material"));
        }
        if std::env::var("H1_DBG_VLIT").is_ok() {
            for c in model_layer
                .chunks_exact(20)
                .filter(|c| c[..4] != [0; 4])
                .step_by(997)
                .take(8)
            {
                let h = |o: usize| fastfile_h1::map::half_to_f32(u16::from_le_bytes([c[o], c[o + 1]]));
                eprintln!(
                    "vlit {:02x?} vis-as-halves ({:.3},{:.3}) amb ({:.3},{:.3},{:.3},{:.3}) hi ({:.3},{:.3},{:.3},{:.3})",
                    &c[..4], h(0), h(2), h(4), h(6), h(8), h(10), h(12), h(14), h(16), h(18)
                );
            }
        }
        notes.push(format!(
            "h1 vertex-lit placements: {} ({} KiB of lighting values, {vlit_mismatch} surfaces past their values)",
            world.static_models.iter().filter(|m| m.vertex_lighting.is_some()).count(),
            model_layer.len() / 1024
        ));
        // The game draws by material sort key; the order within a key is
        // the world's.
        draws.sort_by_key(|d| materials[d.material].sort_key);

        let lightmaps: Vec<_> = world
            .lightmaps
            .iter()
            .map(|(p, s)| (name_of(*p), name_of(*s)))
            .collect();
        if std::env::var("H1_DBG_MLM_PRINT").is_ok() {
            eprintln!("lightmap pages: {lightmaps:?}");
        }
        let probes = world
            .reflection_probes
            .iter()
            .map(|(p, _)| name_of(*p))
            .collect();
        let sun = map.sun.unwrap_or(fastfile_h1::map::Sun {
            dir: [0.0, 0.0, 1.0],
            color: [1.0; 3],
        });
        // A raw file (the vision, the map's scripts) from Horizon MW's own
        // zones first: `hmw_files.ff` replaces the visions and map scripts of
        // several maps (Trailer Park, Favela, Derail, Boneyard, Bootleg,
        // Dome, Highrise, Rundown: warm film tints, contrast 1.35, the sun's
        // tweak 1.4, found 9 Nov 2026), and in the game it loads after the
        // map, so its copy is the one in use. Then the map and the rest, in
        // order. `BO2MP_H1_HMW_RAW=0`: the map's own copies first, as before.
        let hmw_first = !std::env::var("BO2MP_H1_HMW_RAW").is_ok_and(|v| v.trim() == "0");
        let is_hmw = |l: &Zone| {
            l.path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().to_ascii_lowercase().starts_with("hmw_"))
        };
        let raw = |n: &str| {
            let from = |l: &Zone| fastfile_h1::map::raw_file(&l.cap, &l.zone.image, n);
            hmw_first
                .then(|| loaded.iter().filter(|l| is_hmw(l)).find_map(from))
                .flatten()
                .or_else(|| loaded.iter().find_map(from))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        let lightset_name = format!("maps/createart/{name}_lightsets_hdr.csv");
        let mut lightset = loaded
            .iter()
            .find_map(|l| {
                l.cap
                    .string_tables
                    .iter()
                    .find(|t| key(&t.0) == lightset_name)
            })
            .map_or_else(Settings::default, |(_, columns, cells)| {
                Settings::from_table(*columns, cells)
            });
        // Old-style (MW2/MW3 port) suns: a fixed exposure is metered instead
        // (`light::meters_old_fixed`), and the sun and the baked shade take
        // one more factor (`light::usermap_lift`).
        let keep = crate::light::keeps_look(&name);
        // The map script's lighting tweaks (Horizon MW sets them there):
        // colour scales, tone map, the sun's tweak strength (each under its
        // engine switch), and the light grid's intensity. Not on a map kept
        // as it looks. `BO2MP_H1_MAP_SCRIPT=off` ignores them.
        let script = if keep || std::env::var("BO2MP_H1_MAP_SCRIPT").is_ok_and(|v| v == "off") {
            Default::default()
        } else {
            raw(&format!("maps/mp/{name}.gsc")).map(|t| crate::look::script_dvars(&t)).unwrap_or_default()
        };
        let mut script_vision = Settings::default();
        let applied = crate::look::apply_script_dvars(&script, &mut lightset, &mut script_vision);
        let grid_intensity = crate::look::script_grid_intensity(&script);
        if !applied.is_empty() || (grid_intensity - 1.0).abs() > 1e-3 {
            notes.push(format!(
                "h1 map script: {}{}",
                applied.join(", "),
                if (grid_intensity - 1.0).abs() > 1e-3 { format!(" light grid intensity {grid_intensity}") } else { String::new() }
            ));
        }
        let metered = !keep && crate::light::meters_old_fixed(&lightset, sun.color);
        if metered {
            lightset.values.insert("tonemapauto".into(), vec![1.0]);
            notes.push(format!(
                "h1 light: old-style sun ({:.2}): fixed EV {} metered instead",
                crate::light::sun_power(&lightset, sun.color),
                lightset.f("tonemapexposure", 10.0)
            ));
        }
        if keep {
            // (A kept look keeps the guessed shade scale too.)
            lightset.values.insert("keeplook".into(), vec![1.0]);
        }
        let lift = if keep || !crate::light::guesses() { 1.0 } else { crate::light::usermap_lift(&lightset, sun.color, metered) };
        if lift > 1.0 + 1e-3 {
            let d = lightset.f("diffusecolorscale", 1.0);
            lightset.values.insert("diffusecolorscale".into(), vec![d * lift]);
            lightset.values.insert("h1sunlift".into(), vec![lift]);
            notes.push(format!("h1 light: old-style sun lifted x{lift:.2}"));
        }
        let vision_from = {
            let n = format!("vision/{name}.vision");
            let has = |l: &Zone| fastfile_h1::map::raw_file(&l.cap, &l.zone.image, &n).is_some();
            hmw_first
                .then(|| loaded.iter().filter(|l| is_hmw(l)).find(|l| has(l)))
                .flatten()
                .or_else(|| loaded.iter().find(|l| has(l)))
                .map_or("none".to_owned(), |l| l.path.display().to_string())
        };
        let mut vision = raw(&format!("vision/{name}.vision"))
            .map_or_else(Settings::default, |t| Settings::parse_vision(&t));
        notes.push(format!(
            "h1 vision: from {vision_from}: film {} contrast {} brightness {} light tint {:?}, sun tweak {} x{}",
            vision.f("r_filmEnable", 0.0),
            vision.f("r_filmContrast", 1.0),
            vision.f("r_filmBrightness", 0.0),
            vision.v3("r_filmLightTint", [1.0; 3]).to_array(),
            vision.f("r_primaryLightUseTweaks", 0.0),
            vision.f("r_primaryLightTweakDiffuseStrength", 1.0)
        ));
        vision.values.extend(script_vision.values);
        let fog = raw(&format!("maps/createart/{name}_fog_hdr.gsc"))
            .map_or_else(Settings::default, |t| {
                if std::env::var("BO2MP_H1_FOG_BLOCK").is_ok_and(|v| v == "last") {
                    Settings::parse_fog_script(&t)
                } else {
                    Settings::parse_fog_script_for(&t, &name)
                }
            });
        notes.push(format!(
            "h1 fog: {} (post-opaque {:?})",
            if fog.values.is_empty() {
                "no fog script"
            } else if crate::look::legacy_fog_on(&fog) {
                "legacy distance fog"
            } else if fog.f("atmosFogEnabled", 0.0) != 0.0 {
                "atmospheric"
            } else {
                "off"
            },
            crate::look::post_opaque_technique(&fog, 0)
        ));
        // The sun's LIGHT_DIFFUSE / LIGHT_SPECULAR: exact (the exe: the
        // light's colour times the light set's colour scales) or fitted
        // (colour x pi, matched to HorizonMW's picture of Rust); `light.rs`.
        let math = crate::light::LightMath::from_env();
        let scales = crate::light::ColorScales::read(&lightset, &vision);
        // The baked shade of the models (the light grid) takes the same
        // factor as the baked lightmap shade (`light::secondary_scale`).
        let shade = crate::light::secondary_scale(&lightset, &vision);
        // (times the map script's light grid intensity tweak)
        let model_shade = crate::light::model_shade(shade) * grid_intensity;
        let lum = |shell: &[[f32; 3]; 56]| {
            shell.iter().map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).sum::<f32>() / 56.0
        };
        let stats = |tiles: &[([[f32; 3]; 56], f32)]| {
            let mut l: Vec<f32> = tiles.iter().map(|(s, _)| lum(s)).collect();
            l.sort_by(|a, b| a.total_cmp(b));
            let at = |q: f32| l.get(((l.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
            (at(0.5), at(0.95), at(1.0))
        };
        let before = stats(&tiles);
        if (model_shade - 1.0).abs() > 1e-3 {
            for (shell, _) in &mut tiles {
                for c in shell.iter_mut() {
                    for v in c.iter_mut() {
                        *v *= model_shade;
                    }
                }
            }
            notes.push(format!(
                "h1 shade: baked lightmap x{shade:.2}, model grid light x{model_shade:.2}"
            ));
        }
        let after = stats(&tiles);
        // The one line the owner can send back for props that come out
        // washed out or see-through (`BO2MP_H1_PROPS_SHADE=full` for the
        // uncapped model shade).
        let (mut grid, mut vertex_lit, mut model_lm, mut ground, mut bare) = (0, 0, 0, 0, 0);
        for m in world.static_models.iter().chain(dyn_models.iter()) {
            grid += usize::from(m.grid_color.is_some() || m.flags & 0x40 != 0);
            vertex_lit += usize::from(m.vertex_lighting.is_some());
            model_lm += usize::from(m.model_lightmap.is_some());
            ground += usize::from(m.flags & 0x20 != 0);
            bare += usize::from(m.flags & 0x1e0 == 0);
        }
        let props_line = format!(
            "bo2mp h1 props: {name}: {} placed models ({} grid-lit, {} vertex-lit, {} model-lightmapped, {} ground-lit, {} no lighting flag); {} lighting tiles, sun weights {}; shade factor x{shade:.2}, model grid x{model_shade:.2} (cap {}, BO2MP_H1_PROPS_SHADE=full uncapped); tile luminance median/p95/max {:.2}/{:.2}/{:.2} -> {:.2}/{:.2}/{:.2}; sun diffuse {:?}",
            world.static_models.len() + dyn_models.len(),
            grid,
            vertex_lit,
            model_lm,
            ground,
            bare,
            tiles.len(),
            if weights_baked { "baked" } else { "NOT baked (full sun)" },
            crate::light::MODEL_SHADE_CAP,
            before.0, before.1, before.2, after.0, after.1, after.2,
            crate::light::sun_colors(Vec3::from(sun.color), scales, math).0,
        );
        notes.push(props_line.clone());
        let (diffuse, specular) = crate::light::sun_colors(Vec3::from(sun.color), scales, math);
        notes.push(format!(
            "h1 light: {math:?} sun (scales diffuse {:.3} specular {:.3}, colour {:?})",
            scales.diffuse, scales.specular, sun.color
        ));
        let sun = SunLight {
            dir: Vec3::from(sun.dir),
            diffuse,
            specular,
        };
        let meshes = vec![
            MeshData {
                kind: VertexKind::World,
                vertex_bytes: world.vertex_bytes.clone(),
                indices: world.indices.clone(),
                layer_bytes: world.layer_bytes.clone(),
            },
            MeshData {
                kind: VertexKind::Model,
                vertex_bytes: model_verts,
                indices: model_idx,
                layer_bytes: model_layer,
            },
        ];
        let stages = crate::stage::Stages::from_clip(map.clip.as_ref());
        notes.push(format!(
            "h1 stages: {} (stage 0 radiometric scale {:?}), HDR_STAGE_PARMS {:?}",
            stages.stages.len(),
            stages.stages.first().map(|s| s.radiometric_scale),
            crate::stage::StageMath::from_env()
        ));
        let entities = map.entities.clone();
        // The retail `imagefile<N>.pak`s are in the install's zone folder
        // (the one holding `common_mp.ff`); a usermap or Horizon MW zone
        // names them too, from its own folder.
        let retail_dir = loaded
            .iter()
            .filter_map(|l| l.path.parent())
            .find(|d| d.join("common_mp.ff").is_file())
            .map(Path::to_path_buf);
        let paks = loaded
            .iter()
            .map(|l| ImagePaks::for_zone(&l.path, retail_dir.as_deref()))
            .collect();
        let shadow_bounds = (Vec3::from(world.shadow_mins), Vec3::from(world.shadow_maxs));
        let zones = std::sync::Arc::new(loaded);
        let images = ZoneImages {
            zones: zones.clone(),
            paks,
            by_name: imgs_by_name,
            max_size: 2048,
        };
        let local_lights = map
            .primary_lights
            .iter()
            .filter(|l| matches!(l.ty, 2 | 3) && l.radius > 0.0)
            .cloned()
            .collect();
        Ok(Self {
            name,
            local_lights,
            zones,
            pool: std::sync::Arc::new(pool),
            materials,
            meshes,
            objects,
            draws,
            tiles,
            lightmaps,
            probes,
            sun,
            vision,
            fog,
            lightset,
            stages,
            shadow_bounds,
            entities,
            images,
            notes,
            glass_objects,
            props_line,
        })
    }

    /// The map's intermission camera: (origin, angles).
    pub fn intermission(&self) -> Option<(Vec3, Vec3)> {
        let text = fastfile_h1::map::named_entity_text(&self.entities);
        for block in text.split('}') {
            if !block.contains("mp_global_intermission") {
                continue;
            }
            let field = |k: &str| -> Option<Vec3> {
                let at = block.find(&format!("\"{k}\""))?;
                let rest = &block[at + k.len() + 2..];
                let q1 = rest.find('"')?;
                let q2 = rest[q1 + 1..].find('"')?;
                let v: Vec<f32> = rest[q1 + 1..q1 + 1 + q2]
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
            };
            return Some((field("origin")?, field("angles").unwrap_or(Vec3::ZERO)));
        }
        None
    }

    /// The zones a map needs, from its folder and the game's zone folder:
    /// the map, then `techsets_<map>`, `patch_<map>`, `techsets_common_mp`,
    /// `code_post_gfx_mp` where present.
    pub fn zones_for(map: &Path, zone_dir: &Path) -> Vec<PathBuf> {
        let name = map
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let mut out = vec![map.to_path_buf()];
        for z in [
            format!("techsets_{name}.ff"),
            format!("patch_{name}.ff"),
            "techsets_common_mp.ff".into(),
            "code_post_gfx_mp.ff".into(),
        ] {
            let p = zone_dir.join(&z);
            if p.is_file() {
                out.push(p);
            }
        }
        out
    }
}

/// `BO2MP_H1_VLIT_LOD_END=1`: a vertex-lit placement's only level keeps its
/// first level's far end (the placement is not drawn past it), as before.
fn vlit_lod_end() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_H1_VLIT_LOD_END").is_ok_and(|v| v.trim() == "1"))
}

/// The far end of a placement's level of detail as drawn: a vertex-lit
/// placement draws its first level only, so that level has no end.
pub fn lod_far_end(level: u8, own_end: f32, vertex_lit: bool) -> f32 {
    if vertex_lit && level == 0 { f32::INFINITY } else { own_end }
}

/// `BO2MP_H1_LIGHTGRID`: the palette light grid lights static models
/// (default); `off` leaves it unread.
pub fn palette_from_env() -> bool {
    !matches!(std::env::var("BO2MP_H1_LIGHTGRID"), Ok(v) if v.trim().eq_ignore_ascii_case("off"))
}

#[cfg(test)]
mod lod_tests {
    use super::lod_far_end;

    #[test]
    fn a_vertex_lit_placement_keeps_its_first_level_at_every_distance() {
        assert_eq!(lod_far_end(0, 1200.0, true), f32::INFINITY);
        assert_eq!(lod_far_end(0, 1200.0, false), 1200.0);
        assert_eq!(lod_far_end(1, 2400.0, true), 2400.0);
    }
}

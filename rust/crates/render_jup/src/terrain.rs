//! The map's terrain: the coarse height grid as a mesh and the virtual
//! texture's colour tiles as one picture (the converted lane's way, written
//! for the native scene's vertices).

use fastfile_jup::map::materials::Packages;
use fastfile_jup::map::terrain::{Terrain, VirtualTexture, level_start};
use fastfile_jup::oodle::Decompressor;

use crate::scene::{SceneImage, Vertex};

/// Cells a side of a chunk (one draw, one culling box).
pub const CHUNK: usize = 16;
/// BC1 blocks a tile side, and its border (one block).
const TILE_BLOCKS: usize = 64;
const BORDER: usize = 1;
const INNER: usize = TILE_BLOCKS - 2 * BORDER;
const BC1: u32 = 71;
/// A plain sandy block for a tile the install lacks (565 colour twice).
const SAND: [u8; 8] = {
    let c: u16 = ((180 >> 3) << 11) | ((160 >> 2) << 5) | (120 >> 3);
    let b = c.to_le_bytes();
    [b[0], b[1], b[0], b[1], 0, 0, 0, 0]
};

/// One chunk of the terrain mesh.
pub struct Chunk {
    pub first_index: u32,
    pub index_count: u32,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

/// Appends the terrain's chunks to `vertices` and `indices`.
pub fn add_mesh(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, terrain: &Terrain, vt: Option<&VirtualTexture>) -> Vec<Chunk> {
    let n = terrain.size;
    let (o, cell) = (terrain.origin, terrain.cell);
    let pos = |i: usize, j: usize| [o[0] + i as f32 * cell, o[1] + j as f32 * cell, terrain.at(i, j)];
    let normal = |i: usize, j: usize| {
        let (il, ir) = (i.saturating_sub(1), (i + 1).min(n - 1));
        let (jl, jr) = (j.saturating_sub(1), (j + 1).min(n - 1));
        let dx = (terrain.at(ir, j) - terrain.at(il, j)) / ((ir - il).max(1) as f32 * cell);
        let dy = (terrain.at(i, jr) - terrain.at(i, jl)) / ((jr - jl).max(1) as f32 * cell);
        let l = (dx * dx + dy * dy + 1.0).sqrt();
        [-dx / l, -dy / l, 1.0 / l]
    };
    let uv = |p: [f32; 3]| match vt {
        Some(v) => [(p[0] - v.origin[0]) / v.extent, (p[1] - v.origin[1]) / v.extent],
        None => [p[0] / 512.0, p[1] / 512.0],
    };
    let mut out = Vec::new();
    let cells = n.saturating_sub(1);
    for cj in (0..cells).step_by(CHUNK) {
        for ci in (0..cells).step_by(CHUNK) {
            let (wi, wj) = (CHUNK.min(cells - ci), CHUNK.min(cells - cj));
            let base = vertices.len() as u32;
            let first_index = indices.len() as u32;
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for j in 0..=wj {
                for i in 0..=wi {
                    let p = pos(ci + i, cj + j);
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                    vertices.push(Vertex { pos: p, normal: normal(ci + i, cj + j), uv: uv(p), blend: [0; 4], uv2: uv(p) });
                }
            }
            let row = (wi + 1) as u32;
            for j in 0..wj as u32 {
                for i in 0..wi as u32 {
                    let a = base + j * row + i;
                    let (b, c, d) = (a + 1, a + row, a + row + 1);
                    indices.extend([a, d, b, a, c, d]);
                }
            }
            out.push(Chunk { first_index, index_count: indices.len() as u32 - first_index, mins: lo, maxs: hi });
        }
    }
    out
}

/// The virtual texture's colour as one BC1 picture: level `top` of the
/// quadtree its biggest mip, each coarser level the next. `None` when
/// fewer than half the tiles could be read.
pub fn picture(vt: &VirtualTexture, paks: &Packages, oodle: &dyn Decompressor, top: u32, report: &mut Vec<String>) -> Option<SceneImage> {
    let top = top.min(vt.levels.checked_sub(1)?);
    let started = std::time::Instant::now();
    let wanted: Vec<usize> = (0..=top).flat_map(|l| level_start(l)..level_start(l) + (1usize << (2 * l))).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let got: std::sync::Mutex<std::collections::HashMap<usize, Vec<u8>>> = Default::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&t) = wanted.get(k) else { break };
                    let Some(Some(img)) = vt.tiles.get(t) else { continue };
                    if let Ok(l) = paks.level(img, 256, oodle)
                        && l.dxgi == BC1
                        && l.width == 256
                        && l.height == 256
                        && l.data.len() >= TILE_BLOCKS * TILE_BLOCKS * 8
                        && let Ok(mut g) = got.lock()
                    {
                        g.insert(t, l.data);
                    }
                }
            });
        }
    });
    let got = got.into_inner().unwrap_or_default();
    let mut levels: Vec<Vec<u8>> = Vec::new();
    let mut missing = 0usize;
    for level in (0..=top).rev() {
        let n = 1usize << level;
        let side = n * INNER;
        let mut blocks = vec![0u8; side * side * 8];
        for ty in 0..n {
            for tx in 0..n {
                let tile = got.get(&(level_start(level) + ty * n + tx));
                if tile.is_none() {
                    missing += 1;
                }
                for by in 0..INNER {
                    for bx in 0..INNER {
                        let dst = ((ty * INNER + by) * side + tx * INNER + bx) * 8;
                        let src = ((by + BORDER) * TILE_BLOCKS + bx + BORDER) * 8;
                        let b = tile.map_or(&SAND[..], |t| &t[src..src + 8]);
                        blocks[dst..dst + 8].copy_from_slice(b);
                    }
                }
            }
        }
        levels.push(blocks);
    }
    let side = (1u32 << top) * (INNER as u32) * 4;
    report.push(format!(
        "terrain: texture {side} px a side, {} levels, {} of {} tiles ({missing} missing) in {:.1}s",
        top + 1,
        got.len(),
        wanted.len(),
        started.elapsed().as_secs_f32()
    ));
    (got.len() > wanted.len() / 2).then(|| SceneImage { hash: 0x7e44_a100_0000_0001, dxgi: BC1, width: side, height: side, levels })
}

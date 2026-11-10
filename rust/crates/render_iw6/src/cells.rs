//! The map's cells and portals (the engine's DPVS), read from `GfxWorld`:
//! the plane tree that finds the camera's cell, each cell's portals into
//! its neighbours, and each cell's AABB tree (which world surfaces and
//! static placements it holds). Each frame the portals are walked from
//! the camera's cell (`dpvs_iw4::visit_cells`, the same walk the Black Ops
//! II path runs): a world surface or placement is drawn only if a cell that
//! holds it is seen (then its bounds go through the view's frustum).
//! `BO2MP_IW6_CELLS=1` turns it on (off by default, see `on`).

use std::collections::HashMap;

use dpvs_iw4::{CPlane, CellClipPlanes, CellPortalGraph, PortalView, VisBits, WalkScratch};
use fastfile_iw6::layout as L;

/// `BO2MP_IW6_CELLS=1`: draws are culled by the cells and portals (off by
/// default: on mp_frag it lost a door frame's edge seen through a doorway
/// and the camera's cell is not sure in non-convex cells; see `docs/IW6_NATIVE.md`).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_IW6_CELLS").is_ok_and(|v| v.trim() == "1"))
}

/// What a draw is in the cells' terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellItem {
    /// A world surface (its index in `GfxWorld::surfaces`).
    Surface(u32),
    /// A static placement (its index in the world's static models).
    Smodel(u32),
}

pub struct Portal {
    /// The portal's plane (`DpvsPlane::coeffs`).
    pub plane: [f32; 4],
    /// The cell it leads into (`GfxPortal::cellIndex`).
    pub neighbor: u16,
    pub vertices: Vec<[f32; 3]>,
    pub hull_axis: [[f32; 3]; 2],
}

pub struct Cells {
    pub planes: Vec<CPlane>,
    pub nodes: Vec<u16>,
    pub portals: Vec<Vec<Portal>>,
    /// The cells whose trees hold each world surface / static placement.
    pub surface_cells: HashMap<u32, Vec<u16>>,
    pub smodel_cells: HashMap<u32, Vec<u16>>,
}

fn f32_at(image: &[u8], at: usize) -> f32 {
    image.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u32_at(image: &[u8], at: usize) -> u32 {
    image.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(image: &[u8], at: usize) -> u16 {
    image.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

impl Cells {
    /// The cells as the zone holds them; None without cells or planes.
    pub fn read(image: &[u8], g: &fastfile_iw6::walk::GfxWorldRec) -> Option<Self> {
        let cell_count = g.cell_count as usize;
        if cell_count == 0 || g.cells.len() != cell_count {
            return None;
        }
        let pa = g.dpvs_planes?;
        let planes: Vec<CPlane> = (0..g.plane_count as usize)
            .map(|i| {
                let at = pa + L::cplane_s::SIZE * i;
                CPlane {
                    normal: [f32_at(image, at), f32_at(image, at + 4), f32_at(image, at + 8)],
                    dist: f32_at(image, at + L::cplane_s::dist),
                    r#type: image.get(at + L::cplane_s::r#type).copied().unwrap_or(0),
                }
            })
            .collect();
        let na = g.dpvs_nodes?;
        let nodes: Vec<u16> = (0..g.node_count as usize).map(|i| u16_at(image, na + 2 * i)).collect();
        let mut portals = Vec::with_capacity(cell_count);
        for c in &g.cells {
            let mut list = Vec::with_capacity(c.portals.len());
            for &(p, verts) in &c.portals {
                let at = p + L::GfxPortal::plane__coeffs;
                let plane = [f32_at(image, at), f32_at(image, at + 4), f32_at(image, at + 8), f32_at(image, at + 12)];
                let neighbor = u16_at(image, p + L::GfxPortal::cellIndex);
                let n = image.get(p + L::GfxPortal::vertexCount).copied().unwrap_or(0) as usize;
                let vertices = verts.map_or(Vec::new(), |v| {
                    (0..n).map(|k| [f32_at(image, v + 12 * k), f32_at(image, v + 12 * k + 4), f32_at(image, v + 12 * k + 8)]).collect()
                });
                let h = p + L::GfxPortal::hullAxis;
                let axis = |o: usize| [f32_at(image, h + o), f32_at(image, h + o + 4), f32_at(image, h + o + 8)];
                list.push(Portal {
                    plane,
                    neighbor,
                    vertices,
                    hull_axis: [axis(0), axis(12)],
                });
            }
            portals.push(list);
        }
        // Each cell's tree: a node's surfaces are `sortedSurfIndex[start ..
        // start + count]` (u32 surface indices), its placements its own
        // index array (u16).
        let mut surface_cells: HashMap<u32, Vec<u16>> = HashMap::new();
        let mut smodel_cells: HashMap<u32, Vec<u16>> = HashMap::new();
        let sorted = g.sorted_surfs;
        for (ci, c) in g.cells.iter().enumerate() {
            let ci = ci as u16;
            for &(t, idx) in &c.tree {
                let count = usize::from(u16_at(image, t + L::GfxAabbTree::surfaceCount));
                let start = u32_at(image, t + L::GfxAabbTree::startSurfIndex) as usize;
                if let Some(sa) = sorted {
                    for k in start..start + count {
                        if k >= g.static_surface_count as usize {
                            break;
                        }
                        let s = u32_at(image, sa + 4 * k);
                        let v = surface_cells.entry(s).or_default();
                        if v.last() != Some(&ci) {
                            v.push(ci);
                        }
                    }
                }
                let m = u32_at(image, t + L::GfxAabbTree::smodelIndexCount) as usize;
                if let Some(ia) = idx {
                    for k in 0..m {
                        let s = u32::from(u16_at(image, ia + 2 * k));
                        let v = smodel_cells.entry(s).or_default();
                        if v.last() != Some(&ci) {
                            v.push(ci);
                        }
                    }
                }
            }
        }
        Some(Self {
            planes,
            nodes,
            portals,
            surface_cells,
            smodel_cells,
        })
    }

    pub fn cell_count(&self) -> usize {
        self.portals.len()
    }

    /// The cells a draw's item lies in (empty: in none, not portal culled).
    pub fn cells_of(&self, item: CellItem) -> &[u16] {
        match item {
            CellItem::Surface(s) => self.surface_cells.get(&s),
            CellItem::Smodel(m) => self.smodel_cells.get(&m),
        }
        .map_or(&[], Vec::as_slice)
    }

    /// The camera's cell.
    pub fn cell_for_point(&self, p: [f32; 3]) -> Option<usize> {
        dpvs_iw4::DpvsPlanes {
            planes: &self.planes,
            nodes: &self.nodes,
            cell_count: self.cell_count() as u32,
        }
        .cell_for_point(p)
    }
}

/// One frame's walk: the cells seen and each one's clip planes.
pub struct CellView {
    /// Valid this frame (the camera is in a cell): draws are culled by it.
    pub valid: bool,
    pub camera_cell: Option<usize>,
    pub visible: Vec<bool>,
    pub clips: Vec<CellClipPlanes>,
    words: Vec<u32>,
    scratch: Box<WalkScratch>,
    /// Cells seen last frame (for the profile).
    pub seen: usize,
    /// Frames not culled by the portals (the eye's cell not sure).
    pub ambiguous: u64,
}

impl CellView {
    pub fn new(cells: usize) -> Self {
        Self {
            valid: false,
            camera_cell: None,
            visible: vec![false; cells],
            clips: vec![CellClipPlanes::EMPTY; cells],
            words: vec![0; dpvs_iw4::words_for_bits(cells.max(1))],
            scratch: Box::new(WalkScratch::new()),
            seen: 0,
            ambiguous: 0,
        }
    }

    /// Walks the portals from the cell holding `eye`. `frustum`: near, far,
    /// then the side planes (`n . p + d > 0` inside); `forward` the view
    /// direction.
    pub fn update(&mut self, cells: &Cells, eye: [f32; 3], forward: [f32; 3], frustum: &[[f32; 4]]) {
        self.valid = false;
        self.camera_cell = cells.cell_for_point(eye);
        let Some(cell) = self.camera_cell else { return };
        // An eye on the far side of one of its own cell's portal planes (a
        // cell that is not convex, or the eye against solid where the tree's
        // leaf is arbitrary): what the walk sees from there is not sure, so
        // nothing is culled by the portals this frame (INFERRED; on
        // mp_frag's interior view the eye stands under two upper cells' floor
        // portals and the walk lost the room in front of it).
        let ambiguous = cells.portals[cell].iter().any(|p| {
            p.plane[0] * eye[0] + p.plane[1] * eye[1] + p.plane[2] * eye[2] + p.plane[3] > 0.0
        });
        if ambiguous {
            self.ambiguous += 1;
            return;
        }
        let views: Vec<Vec<PortalView<'_>>> = cells
            .portals
            .iter()
            .map(|list| {
                list.iter()
                    .map(|p| PortalView {
                        plane: p.plane,
                        neighbor: p.neighbor,
                        vertices: &p.vertices,
                        hull_axis: (!std::env::var("IW6_DBG_CELLHULL").is_ok_and(|v| v == "0")).then_some(p.hull_axis),
                    })
                    .collect()
            })
            .collect();
        let slices: Vec<&[PortalView<'_>]> = views.iter().map(Vec::as_slice).collect();
        let graph = CellPortalGraph { portals: &slices };
        for c in &mut self.clips {
            *c = CellClipPlanes::EMPTY;
        }
        let mut vis = VisBits::new(&mut self.words);
        dpvs_iw4::visit_cells(
            &graph,
            cell,
            eye,
            dpvs_iw4::dpvs_view_plane(forward, eye),
            frustum,
            0,
            &mut vis,
            &mut self.scratch,
            Some(&mut self.clips),
            None,
        );
        let mut seen = 0;
        for (i, v) in self.visible.iter_mut().enumerate() {
            *v = vis.get(i);
            seen += usize::from(*v);
        }
        self.seen = seen;
        self.valid = true;
        // IW6_DBG_CELLS=1: the camera's cell and the cells seen (debug).
        if std::env::var("IW6_DBG_CELLS").is_ok_and(|v| v == "1") {
            let list: Vec<usize> = (0..self.visible.len()).filter(|&i| self.visible[i]).collect();
            eprintln!("iw6 cells: camera cell {cell}, seen {seen} {list:?}");
            for (k, p) in cells.portals[cell].iter().enumerate() {
                let d = p.plane[0] * eye[0] + p.plane[1] * eye[1] + p.plane[2] * eye[2] + p.plane[3];
                eprintln!("  portal {k} -> cell {}: plane {:?} eye side {d:.1}, {} verts, first {:?}", p.neighbor, p.plane, p.vertices.len(), p.vertices.first());
            }
        }
    }

    /// Whether a draw in `cells` is seen: one of its cells is (its bounds
    /// then go through the view's frustum, as the Black Ops II path tests
    /// a seen cell's tree; the portals' narrower clip planes are kept in
    /// `clips` but not culled by: they took too much on mp_frag).
    pub fn admits(&self, cells: &[u16]) -> bool {
        cells.iter().any(|&c| self.visible.get(usize::from(c)).copied().unwrap_or(false))
    }
}

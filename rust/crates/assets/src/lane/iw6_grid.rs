//! bo2mp: a Ghosts map's own light grid (`fastfile_iw6::lightgrid`) as the
//! light of what has no baked light of its own.
//!
//! Ghosts lights every model (players, the first-person gun, entities and
//! the placed models) from its light grid: at the model's position the
//! grid's spherical harmonics are turned into a table of 56 directions
//! (`lightgrid::light_table`, the square root of the light times 127.5),
//! which the model's shader reads. The direct sun is not in the grid; it is
//! added on top by the sun's own term and shadow map.
//!
//! Black Ops II's rules light moving things from a light grid in Modern
//! Warfare 2's layout (`asset_model::OwnedLightGrid`, rows of solid
//! columns, each entry naming a 56-direction colour set): `owned_grid`
//! makes one over the area of the map's spawn points, every grid point the
//! Ghosts cell at the same place (the two games' grids share their cell
//! size, 32 x 32 x 64 units, and their origin), each colour set one palette
//! entry's table. Points the Ghosts grid has no data for (in walls and
//! ground, out in the void) take the nearest cell with data in their
//! column, else a neighbouring column's: Ghosts drops such corners and
//! shares their weight among the others, which a grid point of the other
//! layout cannot do.
//!
//! Switches (all off by default: nothing here has been seen on a GPU):
//! - `BO2MP_IW6_GRID` (on by default, `=0` turns it off): players, the gun, entities and effects' models are
//!   lit by the grid (else by the placed models' light round them, as before).
//! - `BO2MP_IW6_GRID_PROPS=1`: a placed model with no light of its own takes
//!   the grid's at its origin (else its neighbours' light, as before; the
//!   calibration in `docs/exe_notes/IW6_LIGHTGRID.md` found the grid a worse
//!   predictor of the models' stored light than the neighbours).
//! - `BO2MP_IW6_GRID_SCALE=<x>` (default 1): a factor on the grid's light,
//!   which otherwise takes the same scale as the map's baked light.

use std::collections::HashMap;
use std::sync::Mutex;

use fastfile_iw6::lightgrid::{self, LightGrid};

/// Whether moving things are lit by the grid (`BO2MP_IW6_GRID`, on unless `=0`).
pub(super) fn moving_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_GRID", true)
}

/// Whether placed models without light of their own take the grid's
/// (`BO2MP_IW6_GRID_PROPS=1`, off by default). Off is the engine's rule: a
/// placed model whose `groundLighting` is all zero is lit per vertex
/// (`fastfile_iw6::map`), from its baked vertex light, not from the grid.
pub(super) fn props_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_GRID_PROPS", false)
}

/// The factor on the grid's light (`BO2MP_IW6_GRID_SCALE`, default 1).
pub(super) fn scale() -> f32 {
    std::env::var("BO2MP_IW6_GRID_SCALE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0)
}

/// The map's grid, when its zone holds one in the palette and tree format.
pub(super) fn grid_of<'a>(cap: &fastfile_iw6::walk::Capture, image: &'a [u8]) -> Option<LightGrid<'a>> {
    let rec = cap.gfx_worlds.first()?.light_grid.as_ref()?;
    LightGrid::new(image, rec)
}

/// The light a placed model takes from the grid: the mean of the table's 56
/// directions at its origin raised 16 units off the ground it stands on
/// (Ghosts' units, before the map's light scale).
pub(super) fn model_light(grid: &LightGrid, origin: [f32; 3]) -> Option<[f32; 3]> {
    let sh = grid.sample([origin[0], origin[1], origin[2] + 16.0])?;
    Some(lightgrid::mean_light(&sh))
}

/// The ratio of the placed models' stored light to the grid's at their
/// origins (mean of red, green and blue each): its median and how many
/// models were compared. For the report only.
pub(super) fn calibration(grid: &LightGrid, models: &[([f32; 3], [f32; 4])]) -> Option<(f32, usize)> {
    let mut ratios: Vec<f32> = models
        .iter()
        .filter_map(|(o, g)| {
            let stored = (g[0] + g[1] + g[2]) / 3.0;
            let m = model_light(grid, *o)?;
            let read = (m[0] + m[1] + m[2]) / 3.0;
            (stored.is_finite() && stored > 0.0 && read > 1e-4).then_some(stored / read)
        })
        .collect();
    if ratios.is_empty() {
        return None;
    }
    ratios.sort_by(f32::total_cmp);
    Some((ratios[ratios.len() / 2], ratios.len()))
}

/// What `t6::foreign_t6_lighting` needs to know of a map whose light grid
/// is Ghosts' own: its name and the light scale its grid's tables take.
static PENDING: Mutex<Option<(String, f32)>> = Mutex::new(None);

pub(super) fn set_pending(map: Option<(String, f32)>) {
    if let Ok(mut g) = PENDING.lock() {
        *g = map;
    }
}

/// The light scale of the grid made for `map`, if the map being given Black
/// Ops II's rules is the one it was made for (taken: a later map does not
/// see it).
pub(super) fn take_pending(map: &str) -> Option<f32> {
    let (name, k) = PENDING.lock().ok()?.take()?;
    let stem = |s: &str| s.rsplit(['/', '\\']).next().unwrap_or(s).trim_end_matches(".ff").to_ascii_lowercase();
    (stem(&name) == stem(map)).then_some(k)
}

const BIAS: f32 = 131_072.0;
const SPACING: [f32; 3] = [32.0, 32.0, 64.0];
const GRID_MARGIN: f32 = 1024.0;
const MAX_POINTS: usize = 6_000_000;
/// How many columns away an empty column looks for one with data.
const FILL_REACH: usize = 64;

/// The cell box the grid is made over: the 1st to 99th percentile of the
/// spawn points on each axis, `GRID_MARGIN` round it (less when the box
/// would be too large), from 192 units under the lowest to 704 over the
/// highest, inside the Ghosts tree's own box. Cell coordinates of its first
/// cell and its size in cells.
fn cell_box(places: &[[f32; 3]], tree: ([i32; 3], [i32; 3])) -> Result<([i64; 3], [usize; 3]), String> {
    let spawns: Vec<[f32; 3]> = places.iter().copied().filter(|p| p.iter().all(|v| v.is_finite())).collect();
    if spawns.is_empty() {
        return Err("no spawn points".to_owned());
    }
    let bound = |k: usize, q: f32| {
        let mut v: Vec<f32> = spawns.iter().map(|p| p[k]).collect();
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q).round() as usize]
    };
    let lo: [f32; 3] = std::array::from_fn(|k| bound(k, 0.01));
    let hi: [f32; 3] = std::array::from_fn(|k| bound(k, 0.99));
    let mut margin = GRID_MARGIN;
    loop {
        let wlo = [lo[0] - margin, lo[1] - margin, lo[2] - 192.0];
        let whi = [hi[0] + margin, hi[1] + margin, hi[2] + 704.0];
        let cmin: [i64; 3] = std::array::from_fn(|k| (((wlo[k] + BIAS) / SPACING[k]).floor() as i64).max(i64::from(tree.0[k])));
        let cmax: [i64; 3] = std::array::from_fn(|k| (((whi[k] + BIAS) / SPACING[k]).ceil() as i64).min(i64::from(tree.1[k])));
        if (0..3).any(|k| cmax[k] < cmin[k]) {
            return Err("the spawn points are outside the grid's cells".to_owned());
        }
        let dims: [usize; 3] = std::array::from_fn(|k| (cmax[k] - cmin[k] + 1) as usize);
        if dims[0] * dims[1] * dims[2] <= MAX_POINTS || margin <= 0.0 {
            if cmin.iter().any(|&c| c < 0 || c > i64::from(u16::MAX)) || dims[2] > 255 || dims[0] > 0xffff || dims[1] > 0xffff || dims[0] * dims[1] * dims[2] > MAX_POINTS * 2 {
                return Err(format!("grid {dims:?} out of range"));
            }
            return Ok((cmin, dims));
        }
        margin -= 256.0;
    }
}

/// Each point's palette slot (`None`: no data) filled: from the nearest
/// point with data in its column (the one above on a tie), then whole empty
/// columns from a neighbouring column, up to `FILL_REACH` columns away.
/// What is still empty takes `fallback`. Counts of the points filled down
/// the column, from a neighbour and with the fallback.
fn fill(slots: &mut [Option<u16>], dims: [usize; 3], fallback: u16) -> (usize, usize, usize) {
    let (nx, ny, nz) = (dims[0], dims[1], dims[2]);
    let at = |ix: usize, iy: usize, iz: usize| (ix * ny + iy) * nz + iz;
    let mut column_filled = 0usize;
    let mut has = vec![false; nx * ny];
    for ix in 0..nx {
        for iy in 0..ny {
            let col = &mut slots[at(ix, iy, 0)..at(ix, iy, 0) + nz];
            let known: Vec<(usize, u16)> = col.iter().enumerate().filter_map(|(z, s)| s.map(|v| (z, v))).collect();
            if known.is_empty() {
                continue;
            }
            has[ix * ny + iy] = true;
            for (z, s) in col.iter_mut().enumerate() {
                if s.is_none() {
                    let best = known.iter().min_by_key(|(k, _)| (k.abs_diff(z), usize::from(*k < z))).map(|k| k.1);
                    *s = best;
                    column_filled += 1;
                }
            }
        }
    }
    let mut neighbour_filled = 0usize;
    for _ in 0..FILL_REACH {
        let mut next = Vec::new();
        for ix in 0..nx {
            for iy in 0..ny {
                if has[ix * ny + iy] {
                    continue;
                }
                let from = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)].into_iter().find_map(|(dx, dy)| {
                    let (x, y) = (ix as i64 + dx, iy as i64 + dy);
                    (x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny && has[x as usize * ny + y as usize]).then_some((x as usize, y as usize))
                });
                if let Some(src) = from {
                    next.push(((ix, iy), src));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        for ((ix, iy), (sx, sy)) in next {
            for iz in 0..nz {
                slots[at(ix, iy, iz)] = slots[at(sx, sy, iz)];
            }
            has[ix * ny + iy] = true;
            neighbour_filled += nz;
        }
    }
    let mut fell_back = 0usize;
    for s in slots.iter_mut().filter(|s| s.is_none()) {
        *s = Some(fallback);
        fell_back += 1;
    }
    (column_filled, neighbour_filled, fell_back)
}

/// The grid in Modern Warfare 2's layout over the spawn points' area (see
/// the module notes), and a line for the report. The tables hold Ghosts'
/// own units; the light scale is applied where they are read
/// (`t6::foreign_t6_lighting`).
pub(super) fn owned_grid(grid: &LightGrid, places: &[[f32; 3]]) -> Result<(asset_model::OwnedLightGrid, String), String> {
    let started = std::time::Instant::now();
    let (cmin, dims) = cell_box(places, grid.cell_bounds())?;
    let total = dims[0] * dims[1] * dims[2];
    let mut palette: HashMap<u32, u16> = HashMap::new();
    let mut colors: Vec<u8> = Vec::new();
    let mut levels: Vec<(f32, u16)> = Vec::new();
    let mut slots: Vec<Option<u16>> = Vec::with_capacity(total);
    let mut with_data = 0usize;
    let mut over = 0usize;
    for ix in 0..dims[0] {
        for iy in 0..dims[1] {
            for iz in 0..dims[2] {
                let c = [cmin[0] + ix as i64, cmin[1] + iy as i64, cmin[2] + iz as i64].map(|v| v as i32);
                let cell = grid.cell(c[0], c[1], c[2]);
                if cell.colour == 0 {
                    slots.push(None);
                    continue;
                }
                let slot = match palette.get(&cell.colour) {
                    Some(&s) => Some(s),
                    None if palette.len() < usize::from(u16::MAX) => match grid.palette_entry(cell.colour as usize) {
                        Some(sh) => {
                            let s = palette.len() as u16;
                            palette.insert(cell.colour, s);
                            colors.extend(lightgrid::light_table(&sh).iter().flatten());
                            let m = lightgrid::mean_light(&sh);
                            levels.push((0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2], s));
                            Some(s)
                        }
                        None => None,
                    },
                    None => {
                        over += 1;
                        None
                    }
                };
                with_data += usize::from(slot.is_some());
                slots.push(slot);
            }
        }
    }
    if palette.is_empty() {
        return Err(format!("no cell of the grid in the box {dims:?} from cell {cmin:?} has data"));
    }
    levels.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (median_level, fallback) = levels[levels.len() / 2];
    let (down, across, fell_back) = fill(&mut slots, dims, fallback);
    let mut entries = Vec::with_capacity(total * 4);
    for s in &slots {
        entries.extend_from_slice(&s.unwrap_or(fallback).to_le_bytes());
        // (No primary light named: the sun's own term is left as it was.)
        entries.extend_from_slice(&[0, 0]);
    }
    let (cols, heights) = (dims[1] as u16, dims[2] as u16);
    let row_len = lighting_iw4::grid_build::full_box_row_len(cols);
    let mut row_data_start = Vec::with_capacity(dims[0] * 2);
    let mut raw_row_data = vec![0u8; dims[0] * row_len];
    for ix in 0..dims[0] {
        let at = u16::try_from(ix * row_len / 4).map_err(|_| "row data too large".to_owned())?;
        row_data_start.extend_from_slice(&at.to_le_bytes());
        if lighting_iw4::grid_build::full_box_row(ix, cmin[1] as u16, cols, cmin[2] as u16, heights, &mut raw_row_data[ix * row_len..]) == 0 {
            return Err(format!("row {ix} of {dims:?} does not fit"));
        }
    }
    let world0: [f32; 3] = std::array::from_fn(|k| cmin[k] as f32 * SPACING[k] - BIAS);
    let owned = asset_model::OwnedLightGrid {
        mins: cmin.map(|c| c as u16),
        maxs: std::array::from_fn(|k| (cmin[k] + dims[k] as i64 - 1) as u16),
        row_axis: 0,
        col_axis: 1,
        color_count: palette.len() as u32,
        has_light_regions: false,
        sun_primary_light_index: 1,
        row_data_start,
        raw_row_data,
        entries,
        colors,
        color_encoding: asset_model::LightGridColorEncoding::Rgb8,
    };
    Ok((
        owned,
        format!(
            "{dims:?} = {total} points from {world0:?}: {with_data} from their own cell, {down} from the nearest cell with data in their column, {across} from a neighbouring column, {fell_back} the middle light ({median_level:.3}){}; {} palette entries as colour sets [{:.2}s]",
            if over > 0 { format!(", {over} past the colour set limit") } else { String::new() },
            palette.len(),
            started.elapsed().as_secs_f32()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_covers_the_spawns_inside_the_tree() {
        let places = [[0.0, 0.0, 0.0], [1000.0, 500.0, 64.0]];
        let tree = ([0, 0, 0], [i32::from(u16::MAX), i32::from(u16::MAX), 4000]);
        let (cmin, dims) = cell_box(&places, tree).unwrap();
        let world = |k: usize, c: i64| c as f32 * SPACING[k] - BIAS;
        assert!(world(0, cmin[0]) <= -GRID_MARGIN && world(0, cmin[0] + dims[0] as i64 - 1) >= 1000.0 + GRID_MARGIN - 32.0);
        assert!(world(2, cmin[2]) <= -192.0 && world(2, cmin[2] + dims[2] as i64 - 1) >= 64.0 + 704.0 - 64.0);
        // Held to the tree's cells.
        let c = |v: f32, k: usize| ((v + BIAS) / SPACING[k]).floor() as i32;
        let small = ([c(0.0, 0), c(0.0, 1), c(0.0, 2)], [c(512.0, 0), c(256.0, 1), c(128.0, 2)]);
        let (cmin, dims) = cell_box(&places, small).unwrap();
        assert_eq!(cmin, small.0.map(i64::from));
        assert_eq!(dims, [17, 9, 3]);
        assert!(cell_box(&[], tree).is_err());
    }

    #[test]
    fn empty_points_take_their_columns_then_their_neighbours_light() {
        let dims = [3, 1, 4];
        // Column 0: data at z 1 (3) and z 3 (5); column 1: none; column 2: z 0 (7).
        let mut s = vec![None, Some(3), None, Some(5), None, None, None, None, Some(7), None, None, None];
        let (down, across, fell) = fill(&mut s, dims, 9);
        assert_eq!(&s[0..4], &[Some(3), Some(3), Some(5), Some(5)], "a tie goes to the one above");
        assert_eq!(&s[8..12], &[Some(7); 4]);
        assert_eq!(&s[4..8], &s[0..4].to_vec()[..], "the empty column copies its first neighbour");
        assert_eq!((down, across, fell), (5, 4, 0));
        let mut none = vec![None; 4];
        assert_eq!(fill(&mut none, [1, 1, 4], 2), (0, 0, 4));
        assert!(none.iter().all(|s| *s == Some(2)));
    }

    #[test]
    fn the_pending_scale_is_for_its_own_map_only() {
        set_pending(Some(("mp_frag".to_owned(), 3.0)));
        assert_eq!(take_pending("MP_FRAG.ff"), Some(3.0));
        assert_eq!(take_pending("mp_frag"), None, "taken once");
        set_pending(Some(("mp_frag".to_owned(), 3.0)));
        assert_eq!(take_pending("mp_prisonbreak"), None);
        assert_eq!(take_pending("mp_frag"), None, "a different map clears it");
    }

    /// On a real zone (`GHOSTS_TEST_MAP=<mp_x.ff> cargo test -p assets --lib
    /// iw6_grid_on_a_zone -- --ignored --nocapture`): the grid laid out over
    /// the spawns, read back by Black Ops II's sampler at points with data,
    /// gives each point's own Ghosts table.
    #[test]
    #[ignore]
    fn iw6_grid_on_a_zone() {
        let Ok(path) = std::env::var("GHOSTS_TEST_MAP") else { return };
        let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).unwrap();
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
        let grid = grid_of(&cap, &zone.image).expect("a palette and tree grid");
        let places: Vec<[f32; 3]> = map.spawns.iter().map(|s| s.origin).collect();
        let (owned, line) = owned_grid(&grid, &places).expect("laid out");
        println!("{line}");
        let (mut checked, mut worst) = (0usize, 0f32);
        for s in map.spawns.iter().take(200) {
            // The grid point at or under the spawn, raised a cell.
            let c = [((s.origin[0] + BIAS) / 32.0).floor(), ((s.origin[1] + BIAS) / 32.0).floor(), ((s.origin[2] + BIAS) / 64.0).floor() + 1.0];
            let Some((_, sh)) = grid.cell_light(c[0] as i32, c[1] as i32, c[2] as i32) else { continue };
            let p = [c[0] * 32.0 - BIAS, c[1] * 32.0 - BIAS, c[2] * 64.0 - BIAS];
            let Some((mean, _)) = asset_world::world_iw3::iw3_grid_light(&owned, p) else { continue };
            let table = lightgrid::light_table(&sh);
            let want: [f32; 3] = std::array::from_fn(|ch| table.iter().map(|t| f32::from(t[ch])).sum::<f32>() / (56.0 * 255.0));
            let d = (0..3).map(|k| (mean[k] - want[k]).abs()).fold(0.0, f32::max);
            worst = worst.max(d);
            checked += 1;
        }
        println!("{checked} spawn points' cells compared, worst difference {worst:.4} (of 1)");
        assert!(checked > 0);
        assert!(worst < 0.02, "{worst}");
    }

    #[test]
    fn the_switches_defaults() {
        if std::env::var_os("BO2MP_IW6_GRID").is_none() {
            assert!(moving_on());
        }
        if std::env::var_os("BO2MP_IW6_GRID_PROPS").is_none() {
            assert!(!props_on());
        }
        if std::env::var_os("BO2MP_IW6_GRID_SCALE").is_none() {
            assert_eq!(scale(), 1.0);
        }
    }
}

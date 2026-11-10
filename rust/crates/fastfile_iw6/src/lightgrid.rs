//! Sampling a Ghosts map's light grid (the palette and tree format).
//!
//! A position falls in a grid of 32 x 32 x 64 unit cells whose corner cell
//! indices come from a tree of octants down to blocks of 4 x 4 x 1 cells. A
//! block gives each cell a palette index and an environment index; the palette
//! entry is the cell's second-order spherical harmonics (nine coefficients per
//! colour channel) at 8, 12 or 16 bits per coefficient.

use crate::walk::LightGridRec;

const WORLD_LIMIT: f64 = 131072.0;
const MIN_CORNER_WEIGHT: f64 = 0.001;

const LANE_PATTERNS: [[u8; 4]; 16] = [
    [0, 1, 4, 5],
    [2, 3, 6, 7],
    [8, 9, 12, 13],
    [10, 11, 14, 15],
    [0, 2, 8, 10],
    [1, 3, 9, 11],
    [4, 6, 12, 14],
    [5, 7, 13, 15],
    [0, 1, 2, 3],
    [1, 4, 3, 6],
    [4, 5, 6, 7],
    [2, 3, 8, 9],
    [6, 7, 12, 13],
    [8, 9, 10, 11],
    [9, 12, 11, 14],
    [12, 13, 14, 15],
];

const LANE_OF: [u8; 16] = [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15];

/// Nine coefficients, each red, green, blue.
pub type Sh = [[f32; 3]; 9];

/// What a grid cell holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// Palette index; 0 means the cell has no data.
    pub colour: u32,
    pub environment: u32,
    /// The block's two per-cell flag bits.
    pub flags: u8,
}

fn bit_length(x: u32) -> u32 {
    32 - x.leading_zeros()
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], byte: usize) -> Self {
        Self { data, pos: byte * 8 }
    }

    fn read(&mut self, n: u32) -> u32 {
        let mut value = 0u32;
        let mut got = 0;
        while got < n {
            let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0) as u32;
            let shift = (self.pos & 7) as u32;
            let take = (8 - shift).min(n - got);
            value |= ((byte >> shift) & ((1u32 << take) - 1)) << got;
            got += take;
            self.pos += take as usize;
        }
        value
    }

    fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }
}

fn pattern_mask(k: u32, bits: u32) -> u16 {
    let mut m = 0u16;
    for (i, lane) in LANE_PATTERNS[(k & 15) as usize].iter().enumerate() {
        if (bits >> i) & 1 != 0 {
            m |= 1 << lane;
        }
    }
    m
}

fn lane_mask_runs(r: &mut Bits) -> u16 {
    if r.read(1) == 0 {
        return r.read(16) as u16;
    }
    let k = r.read(4);
    let invert = r.read(1) != 0;
    let m = pattern_mask(k, r.read(4));
    if invert { !m } else { m }
}

fn lane_mask_flags(r: &mut Bits) -> u16 {
    match r.read(2) {
        0 => r.read(16) as u16,
        1 => 0xffff,
        s => {
            let k = r.read(4);
            let m = pattern_mask(k, r.read(4));
            if s == 3 { !m } else { m }
        }
    }
}

struct Run {
    mask: u16,
    values: Vec<u32>,
    each: bool,
}

fn read_runs(r: &mut Bits, width: u32) -> Vec<Run> {
    let mut out = Vec::new();
    loop {
        let more;
        match r.read(2) {
            0 => {
                let values = (0..16).map(|_| r.read(width)).collect();
                out.push(Run { mask: 0xffff, values, each: true });
                more = false;
            }
            1 => {
                let count = r.read(1) + 2;
                let mut mask = 0u16;
                for _ in 0..count {
                    mask |= 1 << r.read(4);
                }
                let v = r.read(width);
                more = r.read(1) != 0;
                out.push(Run { mask, values: vec![v], each: false });
            }
            2 => {
                let mask = lane_mask_runs(r);
                let values = (0..mask.count_ones()).map(|_| r.read(width)).collect();
                out.push(Run { mask, values, each: true });
                more = false;
            }
            _ => {
                let mask = lane_mask_runs(r);
                let v = r.read(width);
                more = r.read(1) != 0;
                out.push(Run { mask, values: vec![v], each: false });
            }
        }
        if !more || out.len() >= 64 {
            return out;
        }
    }
}

fn apply_runs(runs: &[Run], default: u32) -> [u32; 16] {
    let mut lanes = [default; 16];
    for run in runs {
        let mut j = 0;
        for (i, lane) in lanes.iter_mut().enumerate() {
            if (run.mask >> i) & 1 == 0 {
                continue;
            }
            if run.each {
                if let Some(v) = run.values.get(j) {
                    *lane = *v;
                }
                j += 1;
            } else if let Some(v) = run.values.first() {
                *lane = *v;
            }
        }
    }
    lanes
}

#[derive(Default)]
struct Param {
    base: u32,
    width: u32,
    val: u32,
    n: u32,
    list_bits: u32,
    raw: Vec<u32>,
    list: Vec<u32>,
    default: u32,
}

impl Param {
    fn resolve(&mut self) {
        if self.n != 0 {
            self.list = self
                .raw
                .iter()
                .map(|&e| if e == 0 { 0 } else { self.base.wrapping_add(e).wrapping_sub(1) })
                .collect();
            self.default = self.list.get(self.val as usize).copied().unwrap_or(0);
        } else {
            self.list.clear();
            self.default = if self.val == 0 { 0 } else { self.base.wrapping_add(self.val).wrapping_sub(1) };
        }
    }
}

fn index_bits(width: u32) -> u32 {
    if width == 0 { 0 } else { bit_length(width.saturating_sub(1).max(1)) }
}

fn read_param(r: &mut Bits, flags: u32, bits: u32, shifts: [u32; 3], default_width: u32) -> Param {
    let mut p = Param::default();
    if flags & (1 << shifts[0]) != 0 {
        p.base = r.read(bits);
        p.width = r.read(index_bits(bits)) + 1;
    } else {
        p.base = 0;
        p.width = default_width;
    }
    p.val = if flags & (1 << shifts[1]) != 0 { r.read(p.width) } else { 0 };
    p.raw = vec![0];
    if flags & (1 << shifts[2]) != 0 {
        p.n = r.read(4) + 1;
        p.list_bits = bit_length((p.n - 1).max(1));
        for _ in 1..p.n {
            p.raw.push(r.read(p.width));
        }
    }
    p
}

struct Block {
    colour: [u32; 16],
    environment: [u32; 16],
    flags_low: u16,
    flags_high: u16,
}

fn read_block(r: &mut Bits, colour: &Param, env: &Param, size: u32) -> Block {
    r.align();
    let g = if size == 0 { 0 } else { r.read(8) };

    let (c_start, c_width, c_default, c_runs) = read_field(r, colour, g, 0);
    let (e_start, e_width, e_default, e_runs) = read_field(r, env, g >> 3, 1);

    let flags_low = if g & 0x40 != 0 { lane_mask_flags(r) } else { 0 };
    let flags_high = if g & 0x80 != 0 { lane_mask_flags(r) } else { 0 };
    r.align();

    Block {
        colour: resolve_lanes(colour, c_start, c_width, c_default, &c_runs),
        environment: resolve_lanes(env, e_start, e_width, e_default, &e_runs),
        flags_low,
        flags_high,
    }
}

fn read_field(r: &mut Bits, p: &Param, g: u32, which: u32) -> (u32, u32, u32, Vec<Run>) {
    let list_in_use;
    let (start, width);
    if g & 1 == 0 {
        if p.n == 0 {
            start = u32::from(p.base != 0);
            width = p.width;
            list_in_use = false;
        } else {
            start = 1;
            width = p.list_bits;
            list_in_use = true;
        }
    } else {
        start = r.read(p.width);
        width = r.read(index_bits(p.width)) + 1;
        list_in_use = false;
    }
    let default = if g & 2 == 0 {
        if list_in_use {
            p.val
        } else {
            let a = if start == 0 { 0 } else { start.wrapping_sub(1).wrapping_add(p.base) };
            if which == 0 {
                if p.default == 0 { 0 } else { p.default.wrapping_sub(a).wrapping_add(1) }
            } else if p.default != 0 {
                p.default.wrapping_sub(a).wrapping_add(1)
            } else {
                0
            }
        }
    } else {
        r.read(width)
    };
    let runs = if g & 4 != 0 { read_runs(r, width) } else { Vec::new() };
    (start, width, default, runs)
}

fn resolve_lanes(p: &Param, start: u32, width: u32, default: u32, runs: &[Run]) -> [u32; 16] {
    let lanes = apply_runs(runs, default);
    if p.n != 0 && p.list_bits <= width {
        return lanes.map(|v| p.list.get(v as usize).copied().unwrap_or(0));
    }
    let a = if start == 0 { 0 } else { start.wrapping_sub(1).wrapping_add(p.base) };
    lanes.map(|v| if v == 0 { 0 } else { v.wrapping_sub(1).wrapping_add(a) })
}

/// A light grid in the palette and tree format.
pub struct LightGrid<'a> {
    image: &'a [u8],
    min: [i32; 3],
    max: [i32; 3],
    max_depth: u32,
    colour_bits: u32,
    env_bits: u32,
    nodes: Vec<u32>,
    leaf_table: usize,
    palette_count: usize,
    palette_addresses: usize,
    bitstream: usize,
    missing: usize,
    exponent_limits: [i32; 2],
}

impl<'a> LightGrid<'a> {
    /// `None` when the zone's grid is not in the palette and tree format.
    pub fn new(image: &'a [u8], rec: &LightGridRec) -> Option<Self> {
        if rec.table_version == 0 || rec.tree_node_count == 0 {
            return None;
        }
        let nodes_at = rec.tree_nodes?;
        let nodes = (0..rec.tree_node_count)
            .map(|i| {
                let at = nodes_at + 4 * i;
                image.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            })
            .collect::<Option<Vec<_>>>()?;
        let leaf_table = rec.tree_leaf_table?;
        let palette_addresses = rec.palette_entries?;
        let bitstream = rec.palette_bitstream?;
        Some(Self {
            image,
            min: rec.tree_coord_min,
            max: rec.tree_coord_max,
            max_depth: rec.tree_max_depth,
            colour_bits: rec.tree_default_color_index_bits,
            env_bits: rec.tree_default_light_index_bits,
            nodes,
            leaf_table,
            palette_count: rec.palette_entry_count,
            palette_addresses,
            bitstream,
            missing: rec.missing_color_index as usize,
            exponent_limits: [i32::from(rec.range_exponents[0] as i8), i32::from(rec.range_exponents[1] as i8)],
        })
    }

    /// The cell at grid coordinates (`colour` 0 outside the grid's data).
    pub fn cell(&self, cx: i32, cy: i32, cz: i32) -> Cell {
        let (mn, mx) = (self.min, self.max);
        if cx < mn[0] || cx > mx[0] || cy < mn[1] || cy > mx[1] || cz < mn[2] || cz > mx[2] {
            return Cell::default();
        }
        let zr = (cz - mn[2]) as u32;
        let yr = ((cy - mn[1]) >> 2) as u32;
        let xr = ((cx - mn[0]) >> 2) as u32;
        let lane = LANE_OF[((cx & 3) + 4 * (cy & 3)) as usize] as usize;
        let mut bit = 1u32 << self.max_depth;
        let mut idx = 0usize;
        let (mut leaf, mut mask, mut octant) = (0u32, 0u32, 0u32);
        for depth in 0..=self.max_depth {
            octant = (if zr & bit != 0 { 4 } else { 0 }) | (if yr & bit != 0 { 2 } else { 0 }) | u32::from(xr & bit != 0);
            let Some(&node) = self.nodes.get(idx) else { return Cell::default() };
            mask = node >> 24;
            if (mask >> octant) & 1 == 0 {
                return Cell::default();
            }
            if depth < self.max_depth {
                idx = (node & 0x00ff_ffff) as usize + (mask & ((1 << octant) - 1)).count_ones() as usize;
            } else {
                leaf = node & 0x00ff_ffff;
            }
            bit >>= 1;
        }
        let block = self.leaf_block(leaf as usize, mask, octant);
        Cell {
            colour: block.colour[lane],
            environment: block.environment[lane],
            flags: ((block.flags_low >> lane) & 1) as u8 | ((((block.flags_high >> lane) & 1) as u8) << 1),
        }
    }

    fn leaf_block(&self, leaf: usize, mask: u32, octant: u32) -> Block {
        let mut r = Bits::new(self.image, self.leaf_table + leaf);
        let octants = mask.count_ones();
        let flags = r.read(8);
        let sizes: Vec<u32> = (0..octants).map(|_| r.read(6)).collect();
        let mut colour = read_param(&mut r, flags, self.colour_bits, [0, 1, 2], 0x10);
        let mut env = read_param(&mut r, flags, self.env_bits, [3, 4, 5], 1);
        r.align();
        colour.resolve();
        env.resolve();
        let k = (mask & ((1 << octant) - 1)).count_ones() as usize;
        r.pos += 8 * sizes.iter().take(k).sum::<u32>() as usize;
        read_block(&mut r, &colour, &env, sizes.get(k).copied().unwrap_or(0))
    }

    /// The cells the tree covers, in grid coordinates (both ends inclusive).
    pub fn cell_bounds(&self) -> ([i32; 3], [i32; 3]) {
        (self.min, self.max)
    }

    /// The light of the cell at grid coordinates (its palette entry); `None`
    /// for a cell without data.
    pub fn cell_light(&self, cx: i32, cy: i32, cz: i32) -> Option<(u32, Sh)> {
        let cell = self.cell(cx, cy, cz);
        if cell.colour == 0 {
            return None;
        }
        Some((cell.colour, self.palette_entry(cell.colour as usize)?))
    }

    /// The palette entry's coefficients. An index outside the palette reads
    /// the grid's missing-data entry.
    pub fn palette_entry(&self, index: usize) -> Option<Sh> {
        let index = if index >= self.palette_count { self.missing } else { index };
        let at = self.palette_addresses + 4 * index;
        let addr = u32::from_le_bytes(self.image.get(at..at + 4)?.try_into().ok()?) as usize;
        let p = self.bitstream + addr;
        let head = u16::from_le_bytes(self.image.get(p..p + 2)?.try_into().ok()?);
        let exponents = [
            ((head >> 10) & 31) as i32 - 8,
            ((head >> 5) & 31) as i32 - 8,
            (head & 31) as i32 - 8,
        ];
        let mut pos = 2usize;
        let mut out = [[0f32; 3]; 9];
        for (ch, &e) in exponents.iter().enumerate() {
            let s = 2f32.powi(e);
            if e <= self.exponent_limits[0] {
                let u = self.image.get(p + pos..p + pos + 9)?;
                out[0][ch] = s / 256.0 * f32::from(u[0]);
                for k in 1..9 {
                    out[k][ch] = f32::from(u[k]) * (s * 2.0 / 256.0) - s;
                }
                pos += 9;
            } else if e <= self.exponent_limits[1] {
                pos += pos & 1;
                let u = self.image.get(p + pos..p + pos + 14)?;
                out[0][ch] = s / 65536.0 * f32::from(u16::from(u[0]) | (u16::from(u[1]) << 8));
                let high = &u[2..10];
                let low = [u[9] >> 4, u[10] & 15, u[10] >> 4, u[11] & 15, u[11] >> 4, u[12] & 15, u[12] >> 4, u[13] & 15];
                for k in 1..9 {
                    let v = (u16::from(high[k - 1]) << 4) | u16::from(low[k - 1]);
                    out[k][ch] = f32::from(v) * (s * 2.0 / 4096.0) - s;
                }
                pos += 14;
            } else {
                pos += pos & 1;
                let u = self.image.get(p + pos..p + pos + 18)?;
                let v = |k: usize| f32::from(u16::from_le_bytes([u[2 * k], u[2 * k + 1]]));
                out[0][ch] = s / 65536.0 * v(0);
                for k in 1..9 {
                    out[k][ch] = v(k) * (s * 2.0 / 65536.0) - s;
                }
                pos += 18;
            }
        }
        Some(out)
    }

    /// The primary light environment at a world position (an index into
    /// `ComWorld`'s environments) and the corner's two flag bits: those of
    /// the corner with data that weighs most (`docs/exe_notes/IW6_LIGHTGRID.md`
    /// 9: the engine takes the environment from the largest-weight corner).
    /// Environment 0 is none (no primary light reaches the cell); `None`
    /// when no corner has data.
    pub fn environment(&self, position: [f32; 3]) -> Option<(u32, u8)> {
        let p = position.map(|v| f64::from(v).clamp(-WORLD_LIMIT, WORLD_LIMIT));
        let cx = ((p[0].floor() as i64 + 131072) >> 5) as i32;
        let cy = ((p[1].floor() as i64 + 131072) >> 5) as i32;
        let cz = ((p[2].floor() as i64 + 131072) >> 6) as i32;
        let fx = (p[0] + WORLD_LIMIT) * 0.03125 - f64::from(cx);
        let fy = (p[1] + WORLD_LIMIT) * 0.03125 - f64::from(cy);
        let fz = (p[2] + WORLD_LIMIT) * 0.015625 - f64::from(cz);
        let mut best: Option<(f64, Cell)> = None;
        for c in 0..8 {
            let (dx, dy, dz) = ((c >> 2) & 1, (c >> 1) & 1, c & 1);
            let w = (if dx == 1 { fx } else { 1.0 - fx })
                * (if dy == 1 { fy } else { 1.0 - fy })
                * (if dz == 1 { fz } else { 1.0 - fz });
            let cell = self.cell(cx + dx, cy + dy, cz + dz);
            if cell.colour == 0 || w < MIN_CORNER_WEIGHT {
                continue;
            }
            if best.is_none_or(|(bw, _)| w > bw) {
                best = Some((w, cell));
            }
        }
        best.map(|(_, c)| (c.environment, c.flags))
    }

    /// The light at a world position: the trilinear blend of the corner
    /// cells that hold data (weights under 0.001 dropped, the rest
    /// renormalised). `None` when no corner has data.
    pub fn sample(&self, position: [f32; 3]) -> Option<Sh> {
        let p = position.map(|v| f64::from(v).clamp(-WORLD_LIMIT, WORLD_LIMIT));
        let cx = ((p[0].floor() as i64 + 131072) >> 5) as i32;
        let cy = ((p[1].floor() as i64 + 131072) >> 5) as i32;
        let cz = ((p[2].floor() as i64 + 131072) >> 6) as i32;
        let fx = (p[0] + WORLD_LIMIT) * 0.03125 - f64::from(cx);
        let fy = (p[1] + WORLD_LIMIT) * 0.03125 - f64::from(cy);
        let fz = (p[2] + WORLD_LIMIT) * 0.015625 - f64::from(cz);

        let mut weights: Vec<(u32, f64)> = Vec::with_capacity(8);
        for c in 0..8 {
            let (dx, dy, dz) = ((c >> 2) & 1, (c >> 1) & 1, c & 1);
            let w = (if dx == 1 { fx } else { 1.0 - fx })
                * (if dy == 1 { fy } else { 1.0 - fy })
                * (if dz == 1 { fz } else { 1.0 - fz });
            let cell = self.cell(cx + dx, cy + dy, cz + dz);
            if cell.colour == 0 || w < MIN_CORNER_WEIGHT {
                continue;
            }
            match weights.iter_mut().find(|(i, _)| *i == cell.colour) {
                Some(slot) => slot.1 += w,
                None => weights.push((cell.colour, w)),
            }
        }
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return None;
        }
        let mut sh = [[0f32; 3]; 9];
        for (index, w) in weights {
            let entry = self.palette_entry(index as usize)?;
            let w = (w / total) as f32;
            for k in 0..9 {
                for ch in 0..3 {
                    sh[k][ch] += entry[k][ch] * w;
                }
            }
        }
        Some(sh)
    }
}

/// The light a unit direction receives from `sh` (no negative light).
pub fn evaluate(sh: &Sh, d: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = d;
    let basis = [1.0, x, y, z, z * x, z * y, y * x, 3.0 * z * z - 1.0, x * x - y * y];
    let mut out = [0f32; 3];
    for (ch, o) in out.iter_mut().enumerate() {
        *o = basis.iter().enumerate().map(|(k, b)| b * sh[k][ch]).sum::<f32>().max(0.0);
    }
    out
}

/// One sample of a model's 56-direction light table as the engine stores
/// it: the square root of the light, times 127.5, at most 255 (light 0..4).
pub fn table_byte(light: f32) -> u8 {
    if light.is_finite() && light > 0.0 { (light.sqrt() * 127.5).min(255.0) as u8 } else { 0 }
}

/// The light a table byte holds: `(byte / 127.5)^2`.
pub fn table_light(byte: u8) -> f32 {
    let v = f32::from(byte) / 127.5;
    v * v
}

/// A model's light table from `sh`: each of `table_directions` evaluated
/// (negative light clamped to none), stored as `table_byte`.
pub fn light_table(sh: &Sh) -> [[u8; 3]; 56] {
    let mut out = [[0u8; 3]; 56];
    for (o, d) in out.iter_mut().zip(table_directions()) {
        *o = evaluate(sh, d).map(table_byte);
    }
    out
}

/// The mean light of `sh` over the table's 56 directions (each clamped to
/// no negative light first), red, green, blue.
pub fn mean_light(sh: &Sh) -> [f32; 3] {
    let dirs = table_directions();
    let mut m = [0f32; 3];
    for d in &dirs {
        let v = evaluate(sh, *d);
        for ch in 0..3 {
            m[ch] += v[ch];
        }
    }
    m.map(|v| v / dirs.len() as f32)
}

/// The 56 directions a model's light table is sampled in: the vertices of a
/// 4 x 4 x 4 lattice on its outer shell.
pub fn table_directions() -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(56);
    for k in 0..4 {
        for j in 0..4 {
            for i in 0..4 {
                if (1..=2).contains(&i) && (1..=2).contains(&j) && (1..=2).contains(&k) {
                    continue;
                }
                let v = [i as f32 * 2.0 / 3.0 - 1.0, j as f32 * 2.0 / 3.0 - 1.0, k as f32 * 2.0 / 3.0 - 1.0];
                let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                out.push([v[0] / n, v[1] / n, v[2] / n]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_direction_table_has_fifty_six_unit_vectors() {
        let d = table_directions();
        assert_eq!(d.len(), 56);
        for v in d {
            assert!(((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn evaluation_of_a_constant_term_is_that_constant() {
        let mut sh = [[0f32; 3]; 9];
        sh[0] = [0.5, 0.25, 0.125];
        assert_eq!(evaluate(&sh, [0.0, 0.0, 1.0]), [0.5, 0.25, 0.125]);
    }

    #[test]
    fn table_bytes_hold_the_square_root_of_the_light() {
        assert_eq!(table_byte(0.0), 0);
        assert_eq!(table_byte(-1.0), 0);
        assert_eq!(table_byte(f32::NAN), 0);
        assert_eq!(table_byte(4.0), 255);
        assert_eq!(table_byte(9.0), 255);
        for b in [1u8, 37, 128, 200, 254] {
            assert_eq!(table_byte(table_light(b) + 1e-6), b);
        }
        assert!((table_light(255) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_constant_light_fills_the_table_evenly() {
        let mut sh = [[0f32; 3]; 9];
        sh[0] = [1.0, 0.25, 0.0];
        let t = light_table(&sh);
        assert!(t.iter().all(|s| *s == [127, 63, 0]));
        let m = mean_light(&sh);
        assert!((m[0] - 1.0).abs() < 1e-6 && (m[1] - 0.25).abs() < 1e-6 && m[2] == 0.0);
    }

    #[test]
    fn a_light_from_above_is_brighter_on_the_tables_upper_half() {
        let mut sh = [[0f32; 3]; 9];
        sh[0] = [1.0; 3];
        sh[3] = [0.8; 3];
        let t = light_table(&sh);
        let dirs = table_directions();
        // The table's order is x fastest, then y, then z (the first is
        // straight down the -x -y -z corner, the last up the +x +y +z one).
        assert!(dirs[0][2] < 0.0 && dirs[55][2] > 0.0);
        assert!(t[55][0] > t[0][0]);
        // A negative lobe is no light, so the mean is above the constant.
        assert!(mean_light(&sh)[0] >= 1.0 - 1e-6);
    }

    #[test]
    fn bits_read_least_significant_first() {
        let data = [0b1010_0101u8, 0b0000_0011];
        let mut r = Bits::new(&data, 0);
        assert_eq!(r.read(4), 0b0101);
        assert_eq!(r.read(6), 0b11_1010);
        assert_eq!(r.read(2), 0);
    }

    #[test]
    fn lane_patterns_cover_each_lane_equally() {
        let mut seen = [0u32; 16];
        for p in LANE_PATTERNS {
            for l in p {
                seen[l as usize] += 1;
            }
        }
        assert!(seen.iter().all(|&n| n >= 1));
        assert_eq!(LANE_OF.iter().map(|&l| 1u32 << l).fold(0, |a, b| a | b), 0xffff);
    }
}

//! bo2mp: Ghosts' flare element (type 4), translated from the exe's flare
//! draw handler (`docs/exe_notes/IW6_FX_EXE.md`, "Flares"). The exe builds a
//! screen-space quad: the element's position is taken into the view, the
//! quad's centre is that position's NDC point scaled by the flare def's
//! `position`, its corners are world-sized offsets divided by the depth and
//! projection scale (so a perspective size at the flare's real depth), and
//! its alpha and size run through the def's gradient tables. Here the quad
//! is placed as a camera-facing billboard at the flare's depth with the
//! same centre and size, which projects to the same screen quad.
//!
//! Read from the decompile: depth, the view-angle alpha (def flag 2), the
//! source-cosine alpha and size tables (flag bits 3 and 4 equal to 8), the
//! position scale, the screen x / y alpha tables (flag 4), the alpha
//! threshold, the distance scale, the radial size ramps. INFERRED: the
//! angle's arguments (the decompile dropped them: the angle of the screen
//! offset times `angularRotCount`, zero without it), the screen radius
//! being the length of the scaled NDC offset, the source cosine's axis
//! being the def's direction in the element's frame, and the occlusion (the
//! exe fades a flare by a GPU visibility query at spawn: not here; the
//! billboard is depth tested instead). `BO2MP_IW6_FX_FLARE=0` plays flares
//! silent again.

use asset_game::Iw6FlareDef;
use asset_world::Iw6FxView;

/// Where and how a flare is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The billboard's world centre.
    pub origin: [f32; 3],
    /// Its half width and height (world units at its depth).
    pub size0: f32,
    pub size1: f32,
    /// Its turn about the view axis (radians).
    pub rotation: f32,
    /// The final alpha, 0..1.
    pub alpha: f32,
}

/// `BO2MP_IW6_FX_FLARE` (on).
pub fn on() -> bool {
    asset_game::iw6_flare_on()
}

/// The exe's gradient lookup: `table` holds `count + 1` values; `x` times
/// `count` picks the interval (the last for an overshoot, the first for
/// anything below 1), the weight is not clamped; the result is clamped to
/// 0..1 by the caller (`clamp01`).
pub fn table(t: &[f32], x: f32) -> f32 {
    let count = t.len().saturating_sub(1);
    if count == 0 {
        return t.first().copied().unwrap_or(0.0);
    }
    let v = count as f32 * x;
    let mut i = v as i64;
    if i >= count as i64 - 1 {
        i = count as i64 - 1;
    }
    if i < 1 {
        i = 0;
    }
    let frac = v - i as f32;
    let i = i as usize;
    (1.0 - frac) * t[i] + frac * t[i + 1]
}

fn clamp01(x: f32) -> f32 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The flare for an element at `origin` (in the frame `axis`), with the
/// sample's half sizes (`size1` 0: as `size0`), alpha and the draw
/// threshold. `None`: behind the eye, or too faint.
pub fn place(def: &Iw6FlareDef, view: &Iw6FxView, origin: [f32; 3], axis: [[f32; 3]; 3], sizes: [f32; 2], alpha: f32, threshold: f32) -> Option<Placement> {
    let rel = [origin[0] - view.eye[0], origin[1] - view.eye[1], origin[2] - view.eye[2]];
    let depth = dot(rel, view.forward);
    if !(depth > 0.0) {
        return None;
    }
    let (sx, sy) = (dot(rel, view.right), dot(rel, view.up));
    let mut a = 1.0f32;
    let mut size_k = 1.0f32;
    if def.flags & 2 != 0 {
        // The cosine of the angle off the view axis.
        a = depth / (sx * sx + sy * sy + depth * depth).sqrt();
    }
    if def.flags & 0x18 == 8 {
        let to_eye = [-rel[0], -rel[1], -rel[2]];
        let len = dot(to_eye, to_eye).sqrt().max(1e-9);
        let dir = [
            def.dir[0] * axis[0][0] + def.dir[1] * axis[1][0] + def.dir[2] * axis[2][0],
            def.dir[0] * axis[0][1] + def.dir[1] * axis[1][1] + def.dir[2] * axis[2][1],
            def.dir[0] * axis[0][2] + def.dir[1] * axis[1][2] + def.dir[2] * axis[2][2],
        ];
        let c = dot(dir, to_eye) / len;
        a *= clamp01(table(&def.tables[2], c));
        size_k = clamp01(table(&def.tables[3], c));
    }
    // The NDC point, scaled by the def's position.
    let nx = sx / (depth * (1.0 / view.tan_x)) * def.position;
    let ny = sy / (depth * (1.0 / view.tan_y)) * def.position;
    let dist = (nx * nx + ny * ny).sqrt();
    if def.flags & 4 != 0 {
        let ax = clamp01(table(&def.tables[0], (nx + 1.0) * 0.5));
        let ay = clamp01(table(&def.tables[1], (ny + 1.0) * 0.5));
        a *= ax * ay;
    }
    a *= alpha;
    if !(a > threshold) {
        return None;
    }
    if def.depth_scale_range[1] != 0.0 {
        size_k *= ((depth - def.depth_scale_range[0]) / def.depth_scale_range[1]) * def.depth_scale_value[1] + def.depth_scale_value[0];
    }
    let angle = if def.angular_rot_count != 0.0 { ny.atan2(nx) * def.angular_rot_count } else { 0.0 };
    let rx = dist * def.radial_scale_x[1] + def.radial_scale_x[0];
    let ry = if def.flags & 1 != 0 { dist * def.radial_scale_y[1] + def.radial_scale_y[0] } else { rx };
    let size1 = if sizes[1] > 0.0 { sizes[1] } else { sizes[0] };
    let half = [rx * size_k * sizes[0], ry * size_k * size1];
    // The view-space centre at the real depth, back to the world.
    let (cx, cy) = (nx * depth * view.tan_x, ny * depth * view.tan_y);
    let world = core::array::from_fn(|k| view.eye[k] + view.right[k] * cx + view.up[k] * cy + view.forward[k] * depth);
    Some(Placement { origin: world, size0: half[0], size1: half[1], rotation: angle, alpha: clamp01(a) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Iw6FxView {
        Iw6FxView { eye: [0.0; 3], right: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], forward: [1.0, 0.0, 0.0], tan_x: 1.0, tan_y: 0.5 }
    }

    fn def() -> Iw6FlareDef {
        Iw6FlareDef {
            position: 1.0,
            radial_scale_x: [1.0, 0.0],
            radial_scale_y: [1.0, 0.0],
            tables: [vec![1.0, 1.0], vec![1.0, 1.0], vec![0.0, 1.0], vec![1.0, 1.0]],
            ..Default::default()
        }
    }

    const AXIS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn the_table_lookup_is_the_exes() {
        let t = [0.0, 0.5, 1.0];
        assert!((table(&t, 0.25) - 0.25).abs() < 1e-6);
        assert!((table(&t, 0.75) - 0.75).abs() < 1e-6);
        // Past the end the last interval's line goes on (clamped by the caller).
        assert!(table(&t, 1.5) > 1.0);
        // Below the start: the first interval, extended.
        assert!(table(&t, -0.5) < 0.0);
        assert_eq!(table(&[0.7], 3.0), 0.7);
        assert_eq!(table(&[], 3.0), 0.0);
    }

    #[test]
    fn behind_the_eye_is_not_drawn() {
        assert!(place(&def(), &view(), [-10.0, 0.0, 0.0], AXIS, [5.0, 0.0], 1.0, 0.0).is_none());
    }

    #[test]
    fn a_flare_on_the_axis_sits_there_at_its_size() {
        let p = place(&def(), &view(), [100.0, 0.0, 0.0], AXIS, [5.0, 0.0], 0.8, 0.0).unwrap();
        assert!((p.origin[0] - 100.0).abs() < 1e-4 && p.origin[1].abs() < 1e-4 && p.origin[2].abs() < 1e-4);
        assert_eq!((p.size0, p.size1), (5.0, 5.0));
        assert!((p.alpha - 0.8).abs() < 1e-6);
        assert_eq!(p.rotation, 0.0);
    }

    #[test]
    fn the_position_scale_moves_the_centre_on_screen_not_in_depth() {
        let mut d = def();
        d.position = -1.0;
        // 20 units right at depth 100: ndc x = 0.2 (tan_x 1); mirrored to -0.2.
        let p = place(&d, &view(), [100.0, 20.0, 0.0], AXIS, [5.0, 0.0], 1.0, 0.0).unwrap();
        assert!((p.origin[0] - 100.0).abs() < 1e-3);
        assert!((p.origin[1] + 20.0).abs() < 1e-3, "{:?}", p.origin);
    }

    #[test]
    fn the_view_angle_flag_fades_off_axis() {
        let mut d = def();
        d.flags = 2;
        let on = place(&d, &view(), [100.0, 0.0, 0.0], AXIS, [1.0, 0.0], 1.0, 0.0).unwrap().alpha;
        let off = place(&d, &view(), [100.0, 100.0, 0.0], AXIS, [1.0, 0.0], 1.0, 0.0).unwrap().alpha;
        assert!((on - 1.0).abs() < 1e-6);
        assert!((off - 0.5f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn the_source_cosine_tables_set_alpha_and_size() {
        let mut d = def();
        d.flags = 8;
        d.dir = [1.0, 0.0, 0.0];
        d.tables[3] = vec![0.0, 1.0];
        // The flare's axis points away from the eye (+x), the eye at the origin: cos -1.
        let away = place(&d, &view(), [100.0, 0.0, 0.0], AXIS, [4.0, 0.0], 1.0, 0.0);
        assert!(away.is_none(), "intensity table is 0 at cos -1 and the threshold is not exceeded");
        // Pointing at the eye (-x): cos 1: intensity 1, size scale 1 (tables clamp to 0..1).
        let axis = [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]];
        let at = place(&d, &view(), [100.0, 0.0, 0.0], axis, [4.0, 0.0], 1.0, 0.0).unwrap();
        assert!((at.alpha - 1.0).abs() < 1e-5 && (at.size0 - 4.0).abs() < 1e-4, "{at:?}");
    }

    #[test]
    fn the_threshold_and_the_distance_scale() {
        let mut d = def();
        d.depth_scale_range = [0.0, 100.0];
        d.depth_scale_value = [1.0, 1.0];
        // depth 100: k = (100 / 100) * 1 + 1 = 2.
        let p = place(&d, &view(), [100.0, 0.0, 0.0], AXIS, [3.0, 0.0], 0.5, 0.1).unwrap();
        assert!((p.size0 - 6.0).abs() < 1e-4);
        assert!(place(&d, &view(), [100.0, 0.0, 0.0], AXIS, [3.0, 0.0], 0.05, 0.1).is_none());
    }
}

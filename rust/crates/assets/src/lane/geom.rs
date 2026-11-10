//! bo2mp: small geometry tests shared by the map lanes.

/// Whether a triangle overlaps a box (`mid`, `half`): the separating axis
/// test (the box's axes, the triangle's normal, their edges' crossings).
pub(crate) fn tri_overlaps_box(t: &[[f32; 3]; 3], mid: [f32; 3], half: [f32; 3]) -> bool {
    let v: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|k| t[i][k] - mid[k]));
    let sub = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|k| a[k] - b[k]) };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let separated = |axis: [f32; 3]| {
        let p = v.map(|q| dot(q, axis));
        let r = half[0] * axis[0].abs() + half[1] * axis[1].abs() + half[2] * axis[2].abs();
        p.iter().copied().fold(f32::MAX, f32::min) > r || p.iter().copied().fold(f32::MIN, f32::max) < -r
    };
    let edges = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    let units = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if units.iter().any(|&u| separated(u)) || separated(cross(edges[0], edges[1])) {
        return false;
    }
    !edges.iter().any(|&e| units.iter().any(|&u| {
        let axis = cross(e, u);
        dot(axis, axis) > 1e-12 && separated(axis)
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn triangles_in_a_box() {
        let mid = [0.0, 0.0, 37.0];
        let half = [16.0, 16.0, 33.0];
        // Through the box, beside it, under its floor, a big one across it.
        assert!(super::tri_overlaps_box(&[[-50.0, 0.0, 30.0], [50.0, 0.0, 30.0], [0.0, 0.0, 60.0]], mid, half));
        assert!(!super::tri_overlaps_box(&[[20.0, 20.0, 30.0], [60.0, 20.0, 30.0], [20.0, 60.0, 30.0]], mid, half));
        assert!(!super::tri_overlaps_box(&[[-500.0, -500.0, 0.0], [500.0, -500.0, 0.0], [0.0, 500.0, 0.0]], mid, half));
        assert!(super::tri_overlaps_box(&[[-500.0, -500.0, 40.0], [500.0, -500.0, 40.0], [0.0, 500.0, 40.0]], mid, half));
        // Diagonal past a corner.
        assert!(!super::tri_overlaps_box(&[[45.0, -5.0, 0.0], [-5.0, 45.0, 0.0], [45.0, -5.0, 80.0]], mid, half));
        assert!(super::tri_overlaps_box(&[[30.0, -10.0, 0.0], [-10.0, 30.0, 0.0], [30.0, -10.0, 80.0]], mid, half));
    }
}

use crate::flags::elem_spawn_frustum_cull;

pub const FX_ELEM_FLAG_CULL_DRAW_5_PLANES: i32 = 0x400;

#[inline]
pub fn cull_elem_for_spawn_allows(
    spawn_range_base: f32,
    spawn_range_amplitude: f32,
    camera_distance_inches: f32,
    flags: i32,
    sphere_outside_frustum: bool,
) -> bool {
    if spawn_range_amplitude != 0.0 {
        let dist = camera_distance_inches - spawn_range_base;
        if dist < 0.0 || spawn_range_amplitude < dist {
            return false;
        }
    }
    if elem_spawn_frustum_cull(flags) && sphere_outside_frustum {
        return false;
    }
    true
}

#[inline]
pub const fn cull_cloud_plane_count(elem_flags: i32, view_plane_count: u32) -> u32 {
    if (elem_flags & FX_ELEM_FLAG_CULL_DRAW_5_PLANES) != 0 {
        5
    } else {
        view_plane_count
    }
}

#[inline]
pub fn cull_cloud_radius(size0: f32, size1: f32, scale: f32) -> f32 {
    let mut r = size1;
    if r < size0 {
        r = size0;
    }
    r + scale
}

#[inline]
pub fn cull_sphere(planes: &[[f32; 4]], plane_count: u32, pos: [f32; 3], radius: f32) -> bool {
    if plane_count == 0 {
        return false;
    }
    let neg_r = -radius;
    let n = plane_count as usize;
    let mut i = 0usize;
    while i < n && i < planes.len() {
        let p = planes[i];
        let f = p[0] * pos[0] + p[1] * pos[1] + p[2] * pos[2] + p[3];
        if f < neg_r {
            return true;
        }
        i += 1;
    }
    false
}

#[inline]
pub fn cull_cloud(
    cull_elem_draw: bool,
    planes: &[[f32; 4]],
    view_plane_count: u32,
    elem_flags: i32,
    pos: [f32; 3],
    size0: f32,
    size1: f32,
    scale: f32,
) -> bool {
    if !cull_elem_draw {
        return false;
    }
    let count = cull_cloud_plane_count(elem_flags, view_plane_count);
    cull_sphere(planes, count, pos, cull_cloud_radius(size0, size1, scale))
}

#[inline]
pub fn cull_elem_light(
    cull_elem_draw: bool,
    planes: &[[f32; 4]],
    view_plane_count: u32,
    elem_flags: i32,
    pos: [f32; 3],
    size0: f32,
) -> bool {
    if !cull_elem_draw {
        return false;
    }
    let count = cull_cloud_plane_count(elem_flags, view_plane_count);
    cull_sphere(planes, count, pos, size0)
}

#[inline]
pub fn elem_light_color_bgr(color_rgba: [u8; 4], scale: f32) -> [f32; 3] {
    let s = scale * (crate::FX_RECIP_255 as f32);
    [
        color_rgba[2] as f32 * s,
        color_rgba[1] as f32 * s,
        color_rgba[0] as f32 * s,
    ]
}

/// Whether a Ghosts element's draw cull applies to a sphere of this radius at
/// `pos` (`cull_sphere` on the view planes, 5 of them with flag 0x400): the
/// billboard, oriented sprite, light and flare test. `radius` is the larger of
/// the two sizes.
#[inline]
pub fn cull_elem_sprite(
    cull_elem_draw: bool,
    planes: &[[f32; 4]],
    elem_flags: i32,
    pos: [f32; 3],
    size0: f32,
    size1: f32,
) -> bool {
    if !cull_elem_draw {
        return false;
    }
    let count = cull_cloud_plane_count(elem_flags, planes.len() as u32);
    cull_sphere(planes, count, pos, size0.max(size1))
}

/// A tail's draw cull (the exe's per-plane test on the tail's capsule): for
/// each view plane (unit normal `n`, offset `d`, inside positive) the tail is
/// outside when its nearest end `|n.v| * half_length + (n.c + d)` lies beyond
/// the disc radius it sweeps across the plane, `radius * sqrt(1 - (n.v)^2)`.
/// `centre` is the middle of the tail (the origin pulled back by `half_length`
/// along the velocity), `vel_dir` its unit direction.
pub fn cull_tail(
    cull_elem_draw: bool,
    planes: &[[f32; 4]],
    elem_flags: i32,
    centre: [f32; 3],
    vel_dir: [f32; 3],
    half_length: f32,
    radius: f32,
) -> bool {
    if !cull_elem_draw {
        return false;
    }
    let count = cull_cloud_plane_count(elem_flags, planes.len() as u32) as usize;
    for p in planes.iter().take(count) {
        let s = p[0] * vel_dir[0] + p[1] * vel_dir[1] + p[2] * vel_dir[2];
        let f = s.abs() * half_length + (p[0] * centre[0] + p[1] * centre[1] + p[2] * centre[2] + p[3]);
        // (f |f| < r^2 (s^2 - 1): f negative and f^2 beyond the disc.)
        if f * f.abs() < radius * radius * (s * s - 1.0) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod sprite_cull_tests {
    use super::*;

    /// One plane facing +x through x = 0: inside is x > 0.
    const PLANE_X: [[f32; 4]; 1] = [[1.0, 0.0, 0.0, 0.0]];

    #[test]
    fn a_sprite_behind_the_plane_by_more_than_its_size_is_culled() {
        assert!(cull_elem_sprite(true, &PLANE_X, 0, [-11.0, 0.0, 0.0], 10.0, 4.0));
        assert!(!cull_elem_sprite(true, &PLANE_X, 0, [-9.0, 0.0, 0.0], 10.0, 4.0));
        // The larger size is the radius.
        assert!(!cull_elem_sprite(true, &PLANE_X, 0, [-11.0, 0.0, 0.0], 4.0, 12.0));
        assert!(!cull_elem_sprite(false, &PLANE_X, 0, [-100.0, 0.0, 0.0], 1.0, 1.0));
    }

    #[test]
    fn the_flagged_element_tests_five_planes_only() {
        let mut planes = [[0.0, 0.0, 0.0, 0.0]; 6];
        planes[5] = [1.0, 0.0, 0.0, 0.0];
        assert!(cull_elem_sprite(true, &planes, 0, [-50.0, 0.0, 0.0], 1.0, 1.0));
        assert!(!cull_elem_sprite(true, &planes, FX_ELEM_FLAG_CULL_DRAW_5_PLANES, [-50.0, 0.0, 0.0], 1.0, 1.0));
    }

    #[test]
    fn a_tail_across_the_plane_is_kept_and_one_wholly_behind_is_culled() {
        // A tail along +x from x = -30 to x = -10 (centre -20, half 10): behind.
        assert!(cull_tail(true, &PLANE_X, 0, [-20.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10.0, 3.0));
        // Along +x from -30 to +5: its head is inside.
        assert!(!cull_tail(true, &PLANE_X, 0, [-12.5, 0.0, 0.0], [1.0, 0.0, 0.0], 17.5, 3.0));
        // Lying in the plane's direction (v perpendicular to n), 2 behind with
        // radius 3: the disc reaches across (3 > 2), kept; 4 behind: culled.
        assert!(!cull_tail(true, &PLANE_X, 0, [-2.0, 0.0, 0.0], [0.0, 1.0, 0.0], 50.0, 3.0));
        assert!(cull_tail(true, &PLANE_X, 0, [-4.0, 0.0, 0.0], [0.0, 1.0, 0.0], 50.0, 3.0));
    }
}

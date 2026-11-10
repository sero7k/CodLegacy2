//! Code constants: the values the engine itself puts in a pass's constant
//! buffers, by `code_source::CODE_CONST` index. Each is one or more 16-byte
//! rows; a matrix's rows are the rows of the matrix that takes a column
//! vector (the shaders dot a position with each row).

use std::collections::HashMap;

use fastfile_h1::code_source::code_const;
use glam::{Mat4, Vec3, Vec4};

/// A code constant's index by name (panics on an unknown name: names are
/// fixed by the game).
pub fn code_index(name: &str) -> u16 {
    code_const(name).unwrap_or_else(|| panic!("no code constant named {name}"))
}

#[derive(Clone, Debug, Default)]
pub struct CodeValues {
    rows: HashMap<u16, Vec<[f32; 4]>>,
    /// Indices a pass asked for that nothing set (reported once).
    pub missing: std::cell::RefCell<std::collections::BTreeSet<u16>>,
    /// The frame's view-projection (eye-relative) and eye: objects' full
    /// matrices are made from these.
    pub view_projection: Mat4,
    pub eye: Vec3,
    /// Eye-relative world to view.
    pub rotation: Mat4,
}

impl CodeValues {
    pub fn set(&mut self, name: &str, rows: Vec<[f32; 4]>) {
        match code_const(name) {
            Some(i) => {
                self.rows.insert(i, rows);
            }
            None => panic!("no code constant named {name}"),
        }
    }

    pub fn set_vec(&mut self, name: &str, v: [f32; 4]) {
        self.set(name, vec![v]);
    }

    pub fn set_matrix(&mut self, name: &str, m: Mat4) {
        self.set(name, (0..4).map(|r| m.row(r).to_array()).collect());
    }

    /// Row `row` of constant `index` (zero when not set).
    pub fn row(&self, index: u16, row: usize) -> [f32; 4] {
        match self.rows.get(&index) {
            Some(r) => r.get(row).copied().unwrap_or([0.0; 4]),
            None => {
                self.missing.borrow_mut().insert(index);
                [0.0; 4]
            }
        }
    }
}

/// What one view of the world needs.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub eye: Vec3,
    /// Pitch, yaw, roll in degrees (the game's angles).
    pub angles: Vec3,
    /// Horizontal field of view in degrees.
    pub fov_x: f32,
    pub width: u32,
    pub height: u32,
    pub near: f32,
}

/// The game's axes for angles: forward, left, up.
pub fn angle_axes(angles: Vec3) -> (Vec3, Vec3, Vec3) {
    let (p, y, r) = (
        angles.x.to_radians(),
        angles.y.to_radians(),
        angles.z.to_radians(),
    );
    let (sp, cp) = p.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sr, cr) = r.sin_cos();
    let forward = Vec3::new(cp * cy, cp * sy, -sp);
    let right = Vec3::new(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (forward, -right, up)
}

impl View {
    /// View rotation (eye-relative world → view: x right, y up, z forward).
    pub fn rotation(&self) -> Mat4 {
        let (f, l, u) = angle_axes(self.angles);
        let r = -l;
        Mat4::from_cols(
            Vec4::new(r.x, u.x, f.x, 0.0),
            Vec4::new(r.y, u.y, f.y, 0.0),
            Vec4::new(r.z, u.z, f.z, 0.0),
            Vec4::W,
        )
    }

    /// Reversed-infinite projection: depth 1 at `near`, 0 at infinity.
    pub fn projection(&self) -> Mat4 {
        let aspect = self.width as f32 / self.height as f32;
        let tan_x = (self.fov_x.to_radians() * 0.5).tan();
        let tan_y = tan_x / aspect;
        let fov_y = 2.0 * tan_y.atan();
        Mat4::perspective_infinite_reverse_lh(fov_y, aspect, self.near)
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection() * self.rotation()
    }
}

/// What the code constants need of a view, however it was made: from
/// angles (`View`) or from a host's matrices.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    /// Eye-relative world to view (x right, y up, z forward).
    pub rotation: Mat4,
    /// Eye-relative world to clip, reversed infinite depth.
    pub view_projection: Mat4,
    /// Tangents of the half fields of view.
    pub tan_x: f32,
    pub tan_y: f32,
    pub near: f32,
}

impl Camera {
    /// From a host's world-to-view matrix in the right-handed convention
    /// (x right, y up, looking down -z) and its world-to-clip matrix.
    pub fn from_rh(
        eye: Vec3,
        view_from_world: Mat4,
        clip_from_world: Mat4,
        tan_x: f32,
        tan_y: f32,
        near: f32,
    ) -> Self {
        let flip = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
        let mut rotation = flip * view_from_world;
        rotation.w_axis = Vec4::W;
        Self {
            eye,
            rotation,
            view_projection: clip_from_world * Mat4::from_translation(eye),
            tan_x,
            tan_y,
            near,
        }
    }
}

impl From<&View> for Camera {
    fn from(v: &View) -> Self {
        let tan_x = (v.fov_x.to_radians() * 0.5).tan();
        Self {
            eye: v.eye,
            rotation: v.rotation(),
            view_projection: v.view_projection(),
            tan_x,
            tan_y: tan_x * v.height as f32 / v.width as f32,
            near: v.near,
        }
    }
}

/// The sun as the engine passes it (`LIGHT_*` of the primary light).
#[derive(Clone, Copy, Debug)]
pub struct SunLight {
    /// Toward the sun.
    pub dir: Vec3,
    pub diffuse: Vec3,
    pub specular: Vec3,
}

/// `GAMETIME` as the engine sets it each frame: (sin 2πf, cos 2πf, f, t) with
/// `t` the game time in seconds (wrapped at 43200) and `f` its fraction.
/// Water scrolls, flags and foliage wave by it; it stood at zero before, so
/// nothing moved.
pub fn set_game_time(c: &mut CodeValues, seconds: f32) {
    let frac = seconds - seconds.floor();
    let angle = f64::from(frac) * std::f64::consts::TAU;
    let wrapped = (f64::from(seconds) % 43200.0) as f32;
    c.set_vec(
        "GAMETIME",
        [angle.sin() as f32, angle.cos() as f32, frac, wrapped],
    );
}

/// The per-view values for world surfaces drawn with the sun as primary
/// light and no dynamic shadow map (the baked sun visibility only).
pub fn world_view(view: &Camera, sun: &SunLight) -> CodeValues {
    let mut c = CodeValues {
        view_projection: view.view_projection,
        eye: view.eye,
        rotation: view.rotation,
        ..CodeValues::default()
    };
    // The projection alone (view space to clip), and for world surfaces
    // (identity placement) the inverse of world to view: its rows' w is the
    // eye in the surface's own space, as the shaders read it.
    c.set_matrix(
        "TRANSPOSE_PROJECTION_MATRIX",
        view.view_projection * view.rotation.inverse(),
    );
    c.set_matrix(
        "INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0",
        (view.rotation * Mat4::from_translation(-view.eye)).inverse(),
    );
    // Inferred (the engine's formula is not read yet): neutral
    // tessellation values, so the no-tessellation displacement programs
    // divide by 1 and fade nothing.
    let tz = std::env::var("H1_DBG_TESSZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    c.set_vec("TESSELLATION_PARMS", [1.0, 0.0, tz, 0.0]);
    c.set_vec("TESSELLATION_PARMS2", [0.0, 0.0, 0.0, 1.0]);
    // World surfaces: identity world matrix, positions made eye-relative
    // by the eye offset.
    c.set_matrix("TRANSPOSE_WORLD_MATRIX0", Mat4::IDENTITY);
    c.set_vec(
        "WORLD_MATRIX_EYE_OFFSET",
        [view.eye.x, view.eye.y, view.eye.z, 0.0],
    );
    c.set_matrix("TRANSPOSE_VIEW_PROJECTION_MATRIX", view.view_projection);
    // The full chain for shaders that take it whole: eye-relative world
    // positions (world matrix identity).
    c.set_matrix(
        "TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0",
        view.view_projection * Mat4::from_translation(-view.eye),
    );
    c.set_vec("EYEOFFSET", [view.eye.x, view.eye.y, view.eye.z, 1.0]);
    set_game_time(&mut c, 0.0);
    // The view's stage scale (`stage.rs`); the host replaces it with the
    // eye's stage. Retail maps' only stage has 0.001: (1, 16, 0, 0).
    c.set_vec(
        "HDR_STAGE_PARMS",
        crate::stage::hdr_stage_parms(
            crate::stage::DEFAULT_RADIOMETRIC_SCALE,
            crate::stage::StageMath::from_env(),
        ),
    );
    c.set_vec("LIGHT_PROBE_AMBIENT", [0.0; 4]);
    c.set_vec("LIGHT_POSITION", [sun.dir.x, sun.dir.y, sun.dir.z, 0.0]);
    c.set_vec(
        "LIGHT_DIFFUSE",
        [sun.diffuse.x, sun.diffuse.y, sun.diffuse.z, 1.0],
    );
    c.set_vec(
        "LIGHT_SPECULAR",
        [sun.specular.x, sun.specular.y, sun.specular.z, 1.0],
    );
    c.set_vec("LIGHT_PHYSICALSIZE", [0.0; 4]);
    c.set_vec("LIGHT_FADEOFFSET", [0.0; 4]);
    // No dynamic sun shadow: every cascade weight zero, so the baked
    // visibility alone decides.
    c.set_matrix("TRANSPOSE_SHADOW_LOOKUP_MATRIX", Mat4::IDENTITY);
    for n in [
        "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0",
        "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_1",
        "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_2",
        "SUN_SHADOWMAP_CASCADE_V_CLAMP",
        "SHADOWMAP_SWITCH_PARTITION_ARRAY_0",
        "SHADOWMAP_SWITCH_PARTITION_ARRAY_1",
        "SHADOWMAP_DISTANCE_BIAS",
        "SHADOWMAP_SCALE",
        "SHADOWMAP_PARTITION_UV_OFFSET",
        "SHADOWMAP_CASCADE_MASK",
        "SHADOWMAP_DISTANCE_BLEND",
        "SCRIPT_PARMS",
    ] {
        c.set_vec(n, [0.0; 4]);
    }
    c
}

#[cfg(test)]
mod game_time_tests {
    use super::*;

    #[test]
    fn game_time_moves() {
        let mut c = CodeValues::default();
        set_game_time(&mut c, 12.25);
        let [s, co, f, t] = c.row(code_index("GAMETIME"), 0);
        assert!((s - 1.0).abs() < 1e-6 && co.abs() < 1e-6, "{s} {co}");
        assert_eq!((f, t), (0.25, 12.25));
        // It wraps at 43200 seconds, as the engine's does.
        set_game_time(&mut c, 43201.5);
        assert_eq!(c.row(code_index("GAMETIME"), 0)[3], 1.5);
    }
}

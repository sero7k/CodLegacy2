//! The view as the game hands it over, and the one function that turns it
//! into the host's [`Camera`]. The game's lens is a right-handed camera
//! (x right, y up, looking down -z) over IW world units with a reversed,
//! infinite-far perspective (`Mat4::perspective_infinite_reverse_rh`: depth
//! `near / z`, 1 at the near plane, 0 at infinity). `jup_native` (the game's
//! glue) and `jup_shot` (offline, `--game-camera`, the default) both build
//! their camera here, so a convention bug shows offline too.
//!
//! The host's camera is made from the lens's own numbers (the eye, the view's
//! axes, the tangents of its half angles, its near plane), not from the frame's
//! clip matrix: a jittered, finite-far or forward-depth matrix cannot reach
//! the host's culling, depth or fog that way. Where the frame's matrix is
//! not the one the lens would make, [`GameView::describe`] says so.
use glam::{Mat3, Mat4, Vec3, Vec4};
use render_h1::code::Camera;

/// The lens's default horizontal field of view (degrees, a 4:3 frame's).
pub const GAME_HFOV_DEG: f32 = 65.0;
/// The lens's near plane in world units.
pub const GAME_NEAR: f32 = 2.0;
/// A standing player's eye above the spawn origin.
pub const EYE_HEIGHT: f32 = 60.0;

/// What the game's render world knows of a view.
#[derive(Clone, Copy, Debug)]
pub struct GameView {
    pub eye: Vec3,
    /// World to view, right-handed (x right, y up, -z forward).
    pub view_from_world: Mat4,
    pub clip_from_view: Mat4,
    /// The frame's own world to clip when it has one (it may carry a jitter).
    pub clip_from_world: Option<Mat4>,
}

/// The depth a projection gives a point `distance` units straight ahead.
fn ndc_depth(clip_from_view: &Mat4, distance: f32) -> f32 {
    let c = *clip_from_view * Vec4::new(0.0, 0.0, -distance, 1.0);
    c.z / c.w
}

/// The near plane of a projection: where its depth is 1 (reversed) or 0
/// (forward). The lens's own (reversed, infinite) answers at once.
fn near_of(clip_from_view: &Mat4) -> f32 {
    let (d1, d2) = (ndc_depth(clip_from_view, 1.0), ndc_depth(clip_from_view, 2.0));
    if d2 > 0.0 && ((d1 / d2) - 2.0).abs() < 1e-3 && d1 > 0.0 {
        return d1;
    }
    let reversed = d1 > d2;
    let target = if reversed { 1.0 } else { 0.0 };
    let (mut lo, mut hi) = (1e-3f32, 1e6f32);
    for _ in 0..80 {
        let mid = (lo * hi).sqrt();
        let d = ndc_depth(clip_from_view, mid);
        // Reversed: depth falls with distance; forward: it rises.
        let past = if reversed { d < target } else { d > target };
        if past {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (lo * hi).sqrt()
}

impl GameView {
    /// The lens the game spawns for the game's angles (pitch, yaw, roll in
    /// degrees): the field of view as the game's `calc_fov` does for the
    /// default (tan half y = tan half x of 65 degrees times 0.75).
    pub fn from_angles(eye: Vec3, angles: Vec3, width: u32, height: u32) -> Self {
        let [pitch, yaw, roll] = angles.to_array().map(f32::to_radians);
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        let (sr, cr) = roll.sin_cos();
        let forward = Vec3::new(cp * cy, cp * sy, -sp);
        let right = Vec3::new(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
        let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
        let world_from_view = Mat4::from_translation(eye) * Mat4::from_mat3(Mat3::from_cols(right, up, -forward));
        let tan_half_y = (GAME_HFOV_DEG.to_radians() * 0.5).tan() * 0.75;
        let fov_y = 2.0 * tan_half_y.atan();
        let aspect = width as f32 / height.max(1) as f32;
        Self { eye, view_from_world: world_from_view.inverse(), clip_from_view: Mat4::perspective_infinite_reverse_rh(fov_y, aspect, GAME_NEAR), clip_from_world: None }
    }

    /// The tangents of the half angles and the near plane of the lens.
    pub fn lens(&self) -> (f32, f32, f32) {
        (1.0 / self.clip_from_view.x_axis.x, 1.0 / self.clip_from_view.y_axis.y, near_of(&self.clip_from_view))
    }

    /// The host's camera: positions eye-relative, depth reversed and infinite,
    /// the view's axes taken without any scale it carries.
    pub fn camera(&self) -> Camera {
        let (tan_x, tan_y, near) = self.lens();
        // The view's rows (right, up, back) are the axes in the world.
        let m = Mat3::from_mat4(self.view_from_world).transpose();
        let (right, up, back) = (m.x_axis.normalize_or_zero(), m.y_axis.normalize_or_zero(), m.z_axis.normalize_or_zero());
        // Eye-relative world to view: x right, y up, z forward.
        let rotation = Mat4::from_cols(
            Vec4::new(right.x, up.x, -back.x, 0.0),
            Vec4::new(right.y, up.y, -back.y, 0.0),
            Vec4::new(right.z, up.z, -back.z, 0.0),
            Vec4::W,
        );
        let fov_y = 2.0 * tan_y.atan();
        let projection = Mat4::perspective_infinite_reverse_lh(fov_y, tan_x / tan_y, near);
        Camera { eye: self.eye, rotation, view_projection: projection * rotation, tan_x, tan_y, near }
    }

    /// How far the frame's own world-to-clip matrix is from the lens's, as
    /// the largest difference in normalised device coordinates of eight
    /// points ahead of the eye (`None` when the frame has none).
    pub fn clip_mismatch(&self) -> Option<f32> {
        let frame = self.clip_from_world?;
        let ours = self.camera();
        let forward = ours.rotation.row(2).truncate();
        let right = ours.rotation.row(0).truncate();
        let up = ours.rotation.row(1).truncate();
        let mut worst = 0.0f32;
        for (dist, x, y) in [(50.0, 0.0, 0.0), (50.0, 0.4, 0.2), (500.0, -0.4, 0.2), (500.0, 0.3, -0.3), (5000.0, 0.0, 0.0), (20.0, -0.5, -0.3), (200.0, 0.5, 0.3), (2000.0, 0.1, 0.1)] {
            let p = self.eye + forward * dist + right * (x * dist) + up * (y * dist);
            let a = frame * p.extend(1.0);
            let b = ours.view_projection * (p - self.eye).extend(1.0);
            if a.w.abs() < 1e-6 || b.w.abs() < 1e-6 {
                continue;
            }
            let (na, nb) = (a.truncate() / a.w, b.truncate() / b.w);
            // (Depth is compared as a fraction of the nearer one: it runs over decades.)
            worst = worst.max((na.x - nb.x).abs()).max((na.y - nb.y).abs()).max((na.z - nb.z).abs() / nb.z.abs().max(1e-9));
        }
        Some(worst)
    }

    /// A line for the log: where the view is, which way, how wide, and the
    /// depth convention the projection has.
    pub fn describe(&self, width: u32, height: u32) -> String {
        let world_from_view = self.view_from_world.inverse();
        let forward = world_from_view.transform_vector3(Vec3::NEG_Z).normalize_or_zero();
        let scale = world_from_view.transform_vector3(Vec3::X).length();
        let (tan_x, tan_y, near) = self.lens();
        format!(
            "camera eye ({:.0}, {:.0}, {:.0}) forward ({:.2}, {:.2}, {:.2}), view scale {scale:.4}, {width}x{height}, tan half fov x {tan_x:.3} y {tan_y:.3} (horizontal {:.1} degrees), near {near:.2}; {}",
            self.eye.x,
            self.eye.y,
            self.eye.z,
            forward.x,
            forward.y,
            forward.z,
            2.0 * tan_x.atan().to_degrees(),
            self.convention(),
        )
    }

    /// The depth convention of the projection and how the frame's own clip
    /// matrix compares with the lens's, in words (for the log; it changes
    /// only when the game changes its lens).
    pub fn convention(&self) -> String {
        let (_, _, near) = self.lens();
        let p = self.clip_from_view;
        let reversed = ndc_depth(&p, 1.0) > ndc_depth(&p, 1000.0);
        // (Depth times distance is the near plane's all the way out when the far plane is at infinity.)
        let infinite = reversed && (ndc_depth(&p, 10.0) * 10.0 - ndc_depth(&p, 1000.0) * 1000.0).abs() <= 1e-3 * near.max(1e-6);
        format!(
            "depth {} (ndc depth 10 units ahead {:.4}, 1000 units {:.6}), the frame's own clip matrix {}",
            match (reversed, infinite) {
                (true, true) => "reversed, infinite far (as the host expects)",
                (true, false) => "reversed with a FINITE far plane (the host makes its own infinite one)",
                (false, _) => "FORWARD, 0 at the near plane (the host makes its own reversed one)",
            },
            ndc_depth(&p, 10.0),
            ndc_depth(&p, 1000.0),
            match self.clip_mismatch() {
                Some(m) if m < 1e-3 => "agrees with the lens's".to_owned(),
                Some(m) => format!("DIFFERS from the lens's by {m:.4} ndc (jitter, another far plane or a scale; the lens's is used)"),
                None => "is not given (the lens's is used)".to_owned(),
            },
        )
    }
}

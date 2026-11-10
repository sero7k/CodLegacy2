//! bo2mp: Black Ops III's AI navigation.
//!
//! [`hkx`] reads the Havok packfiles the map zone keeps its nav data in;
//! [`mesh`] is the nav mesh zombies and other ground AI walk, with paths
//! over it (A* and the funnel) and the engine's dynamic parts: traversal
//! links and blocked faces (doors). The nav volume (parasites, wasps) is
//! not read yet. See `docs/T7_ZOMBIES_PLAN.md`. Not part of upstream IW4L.
#![forbid(unsafe_code)]

pub mod hkx;
pub mod mesh;

pub use mesh::{Link, NavMesh, Waypoint};

#[cfg(test)]
mod tests {
    use super::mesh::*;

    /// An L-shaped corridor of three unit squares (scaled to 100 units):
    /// (0,0)-(1,1), (1,0)-(2,1), (1,1)-(2,2), counter-clockwise.
    fn l_mesh() -> NavMesh {
        // Built through the same fields the packfile reader fills.
        let v = |x: f32, y: f32| [x * 100.0, y * 100.0, 0.0];
        let verts = vec![v(0., 0.), v(1., 0.), v(2., 0.), v(0., 1.), v(1., 1.), v(2., 1.), v(1., 2.), v(2., 2.)];
        // Faces as vertex loops (ccw); edges get their twins below.
        let loops: [&[u32]; 3] = [&[0, 1, 4, 3], &[1, 2, 5, 4], &[4, 5, 7, 6]];
        NavMesh::from_loops(verts, &loops)
    }

    #[test]
    fn path_around_a_corner_bends_at_the_inside_corner() {
        let m = l_mesh();
        let p = m.find_path([50.0, 50.0, 0.0], [150.0, 150.0, 0.0]).expect("path");
        let pts: Vec<[f32; 3]> = p.iter().map(|w| w.point).collect();
        // Straight from (50,50) to (150,150) would cut through (100,100):
        // the inside corner, exactly on the way, so at most that corner.
        assert_eq!(pts.last(), Some(&[150.0, 150.0, 0.0]));
        assert!(pts.len() <= 2, "{pts:?}");
        let q = m.find_path([20.0, 80.0, 0.0], [180.0, 180.0, 0.0]).expect("path");
        let pts: Vec<[f32; 3]> = q.iter().map(|w| w.point).collect();
        assert_eq!(pts.len(), 2, "one corner: {pts:?}");
        // The inside corner, moved into the turn by the agent's radius.
        let c = pts[0];
        assert!(c[0] > 100.0 && c[1] < 100.0 && ((c[0] - 100.0).hypot(c[1] - 100.0) - AGENT_RADIUS).abs() < 0.01, "{c:?}");
    }

    #[test]
    fn blocked_faces_cut_the_path() {
        let mut m = l_mesh();
        assert_eq!(m.block_box([100.0, 0.0, -10.0], [200.0, 100.0, 10.0], true), 1);
        assert!(m.find_path([50.0, 50.0, 0.0], [150.0, 150.0, 0.0]).is_none());
    }
}

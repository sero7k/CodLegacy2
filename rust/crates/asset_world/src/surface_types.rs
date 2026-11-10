//! bo2mp: other games' surface types read in Black Ops II's list.
//!
//! Every Call of Duty here numbers its surfaces alike from `default` (0) to
//! `paintedmetal` (28). After that they part: Modern Warfare 2 and 3 have
//! `riotshield` (29) and `slush` (30); Black Ops and Black Ops II have
//! `player` (29), `tallgrass` (30) and `riotshield` (31); Call of Duty 4 and
//! World at War stop at 28. Under Black Ops II's rules every map's surface
//! flags are read in BO2's list (footsteps, impacts, penetration), so an
//! Infinity Ward map's last two are moved to BO2's: its riot shield to
//! BO2's `riotshield`, its slush (BO2 has none) to `snow`; read as they
//! were, slush walked as tall grass and a shield as a body.
//!
//! Ghosts and Modern Warfare Remastered part earlier: their list (measured
//! on the maps' collision materials and the impact tables' columns) has a
//! second metal, `metalgrate`, at 14, which moves every type from `mud` on
//! up by one: `rock` is 18 where Black Ops II's is 17. Read in BO2's list,
//! a Ghosts rock wall was sand (a dirt crater mark and a faint dust puff),
//! water was wood, wood asphalt, plastic rubber.

use crate::ClipCollision;

const SURF_TYPE_SHIFT: u32 = 20;
const SURF_TYPE_MASK: u32 = 0x1f << SURF_TYPE_SHIFT;

const IW_RIOTSHIELD: u32 = 29;
const IW_SLUSH: u32 = 30;
const T6_SNOW: u32 = 19;
const T6_RIOTSHIELD: u32 = 31;

/// Which surface list a map's collision flags are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceList {
    /// Black Ops II's own (Black Ops and World at War share it).
    T6,
    /// Modern Warfare 2 and 3's, Call of Duty 4's.
    InfinityWard,
    /// Ghosts' and Modern Warfare Remastered's.
    Iw6,
}

impl SurfaceList {
    /// The list of a map from the game named (`iw3`, `iw6`, `h1` ...); the
    /// games whose lists were not measured read as Black Ops II's.
    #[must_use]
    pub fn of_game(game: &str) -> Self {
        match game {
            "iw3" | "iw4" | "iw5" => Self::InfinityWard,
            "iw6" | "h1" => Self::Iw6,
            _ => Self::T6,
        }
    }

    /// A flag word of this list in Black Ops II's.
    #[must_use]
    pub fn to_t6(self, flags: u32) -> u32 {
        match self {
            Self::T6 => flags,
            Self::InfinityWard => iw_surface_flags_to_t6(flags),
            Self::Iw6 => iw6_surface_flags_to_t6(flags),
        }
    }
}

/// A Ghosts / Modern Warfare Remastered map's surface flags in Black Ops
/// II's surface list. Their list: 0..=13 as BO2's, `metalgrate` (14; BO2
/// has none: metal), then `mud` 15 .. `plastic` 25 one above BO2's,
/// `rubber` 26, `fruit` 27, `paintedmetal` 28, `riotshield` 29, `slush`
/// 30; 31 is no surface (default).
#[must_use]
pub fn iw6_surface_flags_to_t6(flags: u32) -> u32 {
    let ty = (flags & SURF_TYPE_MASK) >> SURF_TYPE_SHIFT;
    let to = match ty {
        0..=13 => return flags,
        14 => 13,
        15..=25 => ty - 1,
        26 => 25,
        27 | 28 => return flags,
        29 => T6_RIOTSHIELD,
        30 => T6_SNOW,
        _ => 0,
    };
    (flags & !SURF_TYPE_MASK) | (to << SURF_TYPE_SHIFT)
}

/// An Infinity Ward map's surface flags in Black Ops II's surface list.
#[must_use]
pub fn iw_surface_flags_to_t6(flags: u32) -> u32 {
    let ty = (flags & SURF_TYPE_MASK) >> SURF_TYPE_SHIFT;
    let to = match ty {
        IW_RIOTSHIELD => T6_RIOTSHIELD,
        IW_SLUSH => T6_SNOW,
        _ => return flags,
    };
    (flags & !SURF_TYPE_MASK) | (to << SURF_TYPE_SHIFT)
}

/// Every surface flag of the map's collision through `map` (brush sides,
/// the mesh's triangles, its materials, static models' collision); how many
/// changed.
pub fn remap_clip_surface_flags(clip: &mut ClipCollision, map: impl Fn(u32) -> u32) -> usize {
    let mut changed = 0usize;
    let mut apply = |f: &mut u32| {
        let to = map(*f);
        if to != *f {
            *f = to;
            changed += 1;
        }
    };
    for brush in &mut clip.brushes {
        brush.plane_surface_flags.iter_mut().for_each(&mut apply);
    }
    for material in &mut clip.materials {
        apply(&mut material.surface_flags);
    }
    let needs_mesh = clip.mesh.tri_surface_flags.iter().any(|&f| map(f) != f);
    if needs_mesh {
        std::sync::Arc::make_mut(&mut clip.mesh)
            .tri_surface_flags
            .iter_mut()
            .for_each(&mut apply);
    }
    for model in &mut clip.static_models {
        for surf in std::sync::Arc::make_mut(&mut model.model.coll.surfs).iter_mut() {
            apply(&mut surf.surf_flags);
        }
    }
    changed
}

/// How many of the map's collision surfaces carry each surface type (diag).
#[must_use]
pub fn clip_surface_type_census(clip: &ClipCollision) -> [usize; 32] {
    let mut out = [0usize; 32];
    let mut count = |f: u32| out[((f & SURF_TYPE_MASK) >> SURF_TYPE_SHIFT) as usize] += 1;
    clip.brushes
        .iter()
        .flat_map(|b| b.plane_surface_flags.iter())
        .for_each(|&f| count(f));
    clip.mesh.tri_surface_flags.iter().for_each(|&f| count(f));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_surfaces_stay() {
        for ty in 0..=28u32 {
            let f = (ty << 20) | 0x0200_0010;
            assert_eq!(iw_surface_flags_to_t6(f), f);
        }
    }

    #[test]
    fn ghosts_surfaces_move_to_black_ops_ii_names() {
        let other_bits = 0x0200_40a0;
        let t6 = |ty: u32| (iw6_surface_flags_to_t6((ty << 20) | other_bits) >> 20) & 0x1f;
        // Measured on mp_prisonbreak's collision materials.
        assert_eq!(t6(18), 17, "pbk_river_rock_wall: rock");
        assert_eq!(t6(21), 20, "pbk_water_stream_rolling: water");
        assert_eq!(t6(22), 21, "pbk_wood_planks_01: wood");
        assert_eq!(t6(23), 22, "pbk_asphalt_road_light_01: asphalt");
        assert_eq!(t6(25), 24, "pbk_plastic_gray: plastic");
        assert_eq!(t6(17), 16, "ug_wall_drywall_plain: plaster");
        assert_eq!(t6(16), 15, "pbk_cardboard_noshdw: paper");
        assert_eq!(t6(14), 13, "clip_grating_metal: metal");
        assert_eq!(t6(28), 28, "paintedmetal");
        for same in 0..=13 {
            assert_eq!(t6(same), same);
        }
        assert_eq!(t6(29), 31, "riotshield");
        assert_eq!(t6(30), 19, "slush: snow");
        assert_eq!(t6(31), 0);
        // Only the type moves.
        assert_eq!(
            iw6_surface_flags_to_t6((18 << 20) | other_bits) & !SURF_TYPE_MASK,
            other_bits
        );
        // Each name of Ghosts' list lands on the same name of Black Ops II's.
        let ghosts = [
            "default", "bark", "brick", "carpet", "cloth", "concrete", "dirt", "flesh", "foliage",
            "glass", "grass", "gravel", "ice", "metal", "metalgrate", "mud", "paper", "plaster",
            "rock", "sand", "snow", "water", "wood", "asphalt", "ceramic", "plastic", "rubber",
            "fruit", "paintedmetal",
        ];
        for (i, name) in ghosts.iter().enumerate() {
            let want = if *name == "metalgrate" { "metal" } else { name };
            assert_eq!(weapon_iw4::T6_SURFACE_TYPE_NAMES[t6(i as u32) as usize], want, "type {i}");
        }
    }

    #[test]
    fn lists_by_game() {
        assert_eq!(SurfaceList::of_game("iw6"), SurfaceList::Iw6);
        assert_eq!(SurfaceList::of_game("h1"), SurfaceList::Iw6);
        assert_eq!(SurfaceList::of_game("iw3"), SurfaceList::InfinityWard);
        assert_eq!(SurfaceList::of_game("t5"), SurfaceList::T6);
        assert_eq!(SurfaceList::T6.to_t6(18 << 20), 18 << 20);
        assert_eq!(SurfaceList::Iw6.to_t6(18 << 20), 17 << 20);
        assert_eq!(SurfaceList::InfinityWard.to_t6(30 << 20), 19 << 20);
    }

    #[test]
    fn infinity_ward_slush_and_shield_move_to_black_ops_ii_names() {
        let other_bits = 0x0400_0004;
        assert_eq!(iw_surface_flags_to_t6((30 << 20) | other_bits), (19 << 20) | other_bits);
        assert_eq!(iw_surface_flags_to_t6((29 << 20) | other_bits), (31 << 20) | other_bits);
    }
}

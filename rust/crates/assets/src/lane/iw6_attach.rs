//! bo2mp: Ghosts' attachment and scope selection as pure logic.
//!
//! A Ghosts weapon is one 32-bit word: the gun, a camo, the scope, the
//! under-barrel, the other attachments and the reticle. Everything an
//! attachment changes (sounds, effects, animations) is chosen by matching a
//! code per attachment slot against an override entry's two code fields; the
//! models an attachment adds come from per-camo arrays, and the reticle is a
//! model picked by index. The dual-view scope (a second, magnified scene
//! drawn into the lens) switches on and off by how far the aim has come.
//! Nothing here reads a file: it is given the arrays the walker already
//! keeps (`fastfile_iw6::walk::WeaponRec`, `AttachmentRec`).

/// Models per camo array.
pub(crate) const CAMOS: usize = 64;
/// Reticle models one attachment holds.
pub(crate) const RETICLES_PER_ATTACHMENT: usize = 16;
/// Reticle indices past the first sixteen come from three extra attachments
/// (named `reticlehack`, `reticlehack2`, `reticlehack3`) of sixteen each.
pub(crate) const RETICLE_EXTENSIONS: usize = 3;

/// The parts of a weapon word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WeaponWord {
    pub gun: u8,
    pub camo: u8,
    /// 1-based index into the gun's scopes; 0: none.
    pub scope: u8,
    /// 1-based index into the gun's under-barrels; 0: none.
    pub under_barrel: u8,
    /// Bit mask over the gun's other attachments.
    pub others: u8,
    /// 0-63.
    pub reticle: u8,
}

impl WeaponWord {
    pub(crate) fn decode(word: u32) -> Self {
        Self {
            gun: (word & 0xff) as u8,
            camo: ((word >> 8) & 0x3f) as u8,
            scope: ((word >> 14) & 0x7) as u8,
            under_barrel: ((word >> 17) & 0x3) as u8,
            others: ((word >> 19) & 0x7f) as u8,
            reticle: ((word >> 26) & 0x3f) as u8,
        }
    }

    pub(crate) fn encode(self) -> u32 {
        u32::from(self.gun)
            | (u32::from(self.camo & 0x3f) << 8)
            | (u32::from(self.scope & 0x7) << 14)
            | (u32::from(self.under_barrel & 0x3) << 17)
            | (u32::from(self.others & 0x7f) << 19)
            | (u32::from(self.reticle & 0x3f) << 26)
    }

    /// The match code of each active slot: scope, under-barrel, then the
    /// others in ascending bit order.
    pub(crate) fn slot_codes(self) -> Vec<u16> {
        let mut out = Vec::new();
        if self.scope != 0 {
            out.push(scope_code(self.scope));
        }
        if self.under_barrel != 0 {
            out.push(under_barrel_code(self.under_barrel));
        }
        out.extend((0..7).filter(|i| self.others >> i & 1 != 0).map(other_code));
        out
    }
}

/// A scope's code: its 1-based index.
pub(crate) fn scope_code(index: u8) -> u16 {
    u16::from(index)
}

/// An under-barrel's code: its 1-based index times eight.
pub(crate) fn under_barrel_code(index: u8) -> u16 {
    u16::from(index) * 8
}

/// The code of other attachment number `bit` (0-based): `0x20 << bit`.
pub(crate) fn other_code(bit: u32) -> u16 {
    0x20 << bit
}

/// An override entry's two code fields (sound, effect and animation
/// entries share this header).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OverrideKey {
    pub attachment1: u16,
    pub attachment2: u16,
}

/// How many of an entry's two fields equal some active slot's code (0-2).
/// `guard_zero`: a zero field matches nothing (sounds, effects and animation
/// times do this; the animation resolver does not, but a slot code is never
/// zero, so only a caller counting empty slots would see a difference).
pub(crate) fn override_score(key: OverrideKey, codes: &[u16], guard_zero: bool) -> u32 {
    [key.attachment1, key.attachment2]
        .into_iter()
        .filter(|&f| !(guard_zero && f == 0) && codes.contains(&f))
        .count() as u32
}

/// The entry that wins among `keys`: the highest score, the first on a tie,
/// and none when every score is zero.
pub(crate) fn best_override(keys: &[OverrideKey], codes: &[u16], guard_zero: bool) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (i, &k) in keys.iter().enumerate() {
        let s = override_score(k, codes, guard_zero);
        if s > 0 && best.is_none_or(|(_, b)| s > b) {
            best = Some((i, s));
        }
    }
    best.map(|(i, _)| i)
}

/// The reticle model for reticle index `index` (0-63) of a scope: 0-15 from
/// the scope's own reticle models, 16-63 from the extension attachments'
/// (`extensions[0]` is `reticlehack`, and so on).
pub(crate) fn reticle_model<T: Copy>(
    index: u8,
    scope_reticles: &[Option<T>],
    extensions: [&[Option<T>]; RETICLE_EXTENSIONS],
) -> Option<T> {
    let index = usize::from(index);
    if index >= RETICLES_PER_ATTACHMENT * (RETICLE_EXTENSIONS + 1) {
        return None;
    }
    let (list, at) = match index / RETICLES_PER_ATTACHMENT {
        0 => (scope_reticles, index),
        n => (extensions[n - 1], index % RETICLES_PER_ATTACHMENT),
    };
    list.get(at).copied().flatten()
}

/// One attachment's model arrays, as the walker keeps them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AttachmentModels<'a, T> {
    pub view: &'a [Option<T>],
    pub world: &'a [Option<T>],
    pub reticles: &'a [Option<T>],
}

fn camo_pick<T: Copy>(list: &[Option<T>], camo: u8) -> Option<T> {
    list.get(usize::from(camo).min(CAMOS - 1)).copied().flatten()
}

/// The models an attachment set adds to the gun, in the order they are
/// appended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttachmentModelPlan<T> {
    /// First person: the scope's, the reticle, the under-barrel's, the
    /// others' in ascending bit order.
    pub view: Vec<T>,
    /// Third person: the scope's, the under-barrel's, the others'.
    pub world: Vec<T>,
}

/// Picks the view and world model slots of the word's attachments.
/// `scope`, `under_barrel` and `others` are the attachments the word's
/// indices select (the caller resolves them; `others[i]` is bit `i`).
pub(crate) fn model_plan<T: Copy>(
    word: WeaponWord,
    scope: Option<AttachmentModels<'_, T>>,
    under_barrel: Option<AttachmentModels<'_, T>>,
    others: &[Option<AttachmentModels<'_, T>>],
    extensions: [&[Option<T>]; RETICLE_EXTENSIONS],
) -> AttachmentModelPlan<T> {
    let mut plan = AttachmentModelPlan { view: Vec::new(), world: Vec::new() };
    if let Some(s) = scope {
        plan.view.extend(camo_pick(s.view, word.camo));
        plan.view.extend(reticle_model(word.reticle, s.reticles, extensions));
        plan.world.extend(camo_pick(s.world, word.camo));
    }
    if let Some(u) = under_barrel {
        plan.view.extend(camo_pick(u.view, word.camo));
        plan.world.extend(camo_pick(u.world, word.camo));
    }
    for (bit, o) in others.iter().enumerate().take(7) {
        if word.others >> bit & 1 == 0 {
            continue;
        }
        if let Some(o) = o {
            plan.view.extend(camo_pick(o.view, word.camo));
            plan.world.extend(camo_pick(o.world, word.camo));
        }
    }
    plan
}

/// Aim progress above which the dual-view scope switches on going in.
pub(crate) const DUAL_VIEW_ON: f32 = 0.4;
/// Aim progress below which it switches off going out.
pub(crate) const DUAL_VIEW_OFF: f32 = 0.7;
/// Resolution of the second scene.
pub(crate) const DUAL_VIEW_SIZE: u32 = 512;

/// The dual-view scope's state for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DualView {
    pub active: bool,
    /// 0-1 inside the active range; 0 when off.
    pub t: f32,
    /// How far the lens fades while off (1 at no aim, 0 at the on point).
    pub fade: f32,
    /// Blur / depth-of-field strength of the second scene: `t * 14 / 3`.
    pub blur: f32,
    /// Field-of-view magnification of the second scene: `8 * t + 1`.
    pub zoom: f32,
}

/// Dual-view state from the aim progress `p` (0-1), whether the aim is
/// going out, and whether the scope has the dual-view flag. Going in it
/// switches on past 0.4; going out it stays on until below 0.7.
pub(crate) fn dual_view(p: f32, going_out: bool, use_dual_fov: bool) -> DualView {
    let off = DualView { active: false, t: 0.0, fade: ((DUAL_VIEW_ON - p) / DUAL_VIEW_ON).max(0.0), blur: 0.0, zoom: 1.0 };
    if !use_dual_fov {
        return DualView { fade: 0.0, ..off };
    }
    let (on, t) = if going_out {
        (p >= DUAL_VIEW_OFF, (p - DUAL_VIEW_OFF) / (1.0 - DUAL_VIEW_OFF))
    } else {
        (p > DUAL_VIEW_ON, (p - DUAL_VIEW_ON) / (1.0 - DUAL_VIEW_ON))
    };
    if !on {
        return off;
    }
    let t = t.clamp(0.0, 1.0);
    DualView { active: true, t, fade: 0.0, blur: t * 14.0 / 3.0, zoom: 8.0 * t + 1.0 }
}

/// A scope attachment's thermal and hybrid flags (`AttADSOverlay`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeFlags {
    pub thermal_scope: bool,
    /// The player can switch the thermal view on and off.
    pub thermal_toggle: bool,
    /// The player can switch between the two sight blocks.
    pub hybrid_toggle: bool,
    pub outline_enemies: bool,
    pub use_dual_fov: bool,
}

/// Whether the scope's thermal view shows: the scope is thermal, the aim
/// is up (dual view or overlay), it was not switched off by the toggle and,
/// for a hybrid, the hybrid scope is on its thermal side. Never under EMP.
pub(crate) fn thermal_active(
    flags: ScopeFlags,
    emp: bool,
    aim_view_up: bool,
    toggle_on: bool,
    hybrid_other_side: bool,
) -> bool {
    !emp
        && flags.thermal_scope
        && aim_view_up
        && (!flags.thermal_toggle || toggle_on)
        && (!flags.hybrid_toggle || !hybrid_other_side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_round_trips_and_gives_its_slot_codes() {
        let w = WeaponWord { gun: 9, camo: 7, scope: 2, under_barrel: 1, others: 0b101, reticle: 33 };
        assert_eq!(WeaponWord::decode(w.encode()), w);
        assert_eq!(w.slot_codes(), vec![2, 8, 0x20, 0x80]);
        assert_eq!(under_barrel_code(3), 24);
        assert_eq!(WeaponWord::default().slot_codes(), Vec::<u16>::new());
    }

    #[test]
    fn the_best_override_has_most_matches_first_on_a_tie() {
        let codes = [2, 8];
        let k = |a, b| OverrideKey { attachment1: a, attachment2: b };
        let keys = [k(8, 0), k(2, 8), k(2, 0), k(2, 8)];
        assert_eq!(best_override(&keys, &codes, true), Some(1));
        assert_eq!(best_override(&[k(0, 0), k(4, 0x20)], &codes, true), None);
        assert_eq!(override_score(k(0, 2), &codes, true), 1);
    }

    #[test]
    fn reticles_past_fifteen_come_from_the_extension_attachments() {
        let own: Vec<Option<u32>> = (0..16).map(Some).collect();
        let e1: Vec<Option<u32>> = (100..116).map(Some).collect();
        let e2: Vec<Option<u32>> = (200..216).map(Some).collect();
        let e3: Vec<Option<u32>> = (300..316).map(Some).collect();
        let ext = [&e1[..], &e2[..], &e3[..]];
        assert_eq!(reticle_model(5, &own, ext), Some(5));
        assert_eq!(reticle_model(16, &own, ext), Some(100));
        assert_eq!(reticle_model(47, &own, ext), Some(215));
        assert_eq!(reticle_model(63, &own, ext), Some(315));
        assert_eq!(reticle_model(5, &[], ext), None);
    }

    #[test]
    fn models_are_appended_scope_reticle_under_barrel_others() {
        let s_view = [Some(1), Some(2)];
        let s_ret = [Some(10), Some(11)];
        let u_view = [Some(3), Some(4)];
        let o_view = [Some(5), Some(6)];
        let none: [Option<i32>; 0] = [];
        let m = |view, reticles| AttachmentModels { view, world: &[], reticles };
        let w = WeaponWord { camo: 1, scope: 1, under_barrel: 1, others: 0b10, reticle: 1, ..Default::default() };
        let plan = model_plan(
            w,
            Some(m(&s_view, &s_ret)),
            Some(m(&u_view, &none)),
            &[None, Some(m(&o_view, &none))],
            [&none, &none, &none],
        );
        assert_eq!(plan.view, vec![2, 11, 4, 6]);
        assert!(plan.world.is_empty());
    }

    #[test]
    fn the_dual_view_switches_on_past_040_in_and_off_below_070_out() {
        assert!(!dual_view(0.4, false, true).active);
        let on = dual_view(0.7, false, true);
        assert!(on.active && (on.t - 0.5).abs() < 1e-6);
        assert!(dual_view(0.69, true, true).active == false);
        assert!(dual_view(0.7, true, true).active);
        assert!((dual_view(0.2, false, true).fade - 0.5).abs() < 1e-6);
        assert!(!dual_view(1.0, false, false).active);
        let full = dual_view(1.0, false, true);
        assert!((full.zoom - 9.0).abs() < 1e-6 && (full.blur - 14.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn thermal_needs_the_scope_the_aim_and_its_toggles() {
        let f = ScopeFlags { thermal_scope: true, thermal_toggle: true, ..Default::default() };
        assert!(thermal_active(f, false, true, true, false));
        assert!(!thermal_active(f, false, true, false, false));
        assert!(!thermal_active(f, true, true, true, false));
        assert!(!thermal_active(f, false, false, true, false));
        let h = ScopeFlags { thermal_scope: true, hybrid_toggle: true, ..Default::default() };
        assert!(!thermal_active(h, false, true, true, true));
    }
}

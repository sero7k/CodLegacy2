//! Black Ops II's per-frame weapon state machine for one hand: its timers,
//! its checks and its state handlers, in its own order and with its own
//! edge cases (a swap-back during a drop, the timer held at 1 ms with the
//! trigger down, a melee taken on the weapon delay rather than the weapon
//! time, the gun slot emptied under an offhand and the weapon asked for
//! raised quick after it, ...). Nothing here is tidied: the glitches the
//! game is played for come from these exact rules.

use crate::melee::{MeleeWeaponFacts, melee_charge_start, weapon_start_melee_uses_charge};
use crate::tick::{
    BUTTON_ATTACK, BUTTON_RELOAD, WeaponCmd, WeaponCombatFacts, WeaponHandState, WeaponTickEvent,
    get_weapon_fire_button,
};
use crate::weap_anim::{start_weapon_anim, weap_anim_event as anim};
use crate::weaponstate::WeaponState as S;
use playerstate_iw4::{buttons, pm_flags, weap_flags};

/// Black Ops II's weapon flags (kept per hand in `WeaponHandState::flags`).
pub mod flags {
    /// A reload asked for outside the reload button (a use/reload tap).
    pub const RELOAD_LATCH: u32 = 0x1;
    /// An offhand is in hand (its viewmodel shows).
    pub const OFFHAND: u32 = 0x2;
    /// The trigger was pulled on a burst (or other non-automatic, non-single)
    /// gun since its last shot.
    pub const BURST_LATCH: u32 = 0x400;
    /// The weapon is being put away for an offhand.
    pub const OFFHAND_PUTAWAY: u32 = 0x40000;
    /// An offhand ended with no weapon in hand: the next raise is quick.
    pub const AFTER_OFFHAND: u32 = 0x100000;
    /// The weapon being changed to is a riot shield.
    pub const SHIELD_CHANGE: u32 = 0x400000;
}

const FIRING_COUNTER_MAX: u8 = 250;
const SHOT_COUNT_MAX: u8 = 7;
const DRY_FIRE_MS: i32 = 500;
/// The weapon flag that keeps the weapon from firing or being changed.
const WEAPONS_DISABLED: u32 = 0x80;

/// Black Ops II's `weapType` values the state machine looks at.
const WEAPTYPE_BULLET: i32 = 0;
const WEAPTYPE_GRENADE: i32 = 1;
const WEAPTYPE_PROJECTILE: i32 = 2;
const WEAPTYPE_MELEE: i32 = 7;
const WEAPTYPE_RIOTSHIELD: i32 = 8;
const WEAPTYPE_MINE: i32 = 6;

/// The dive flags (movement_iw4::dive): in the air, sliding. Black Ops II
/// keeps one flag for both and tells them apart by its dive timer's sign.
const PMF_DIVE: u32 = 0x0100_0000;
const PMF_DIVE_SLIDE: u32 = 0x0200_0000;

/// Black Ops II gates its bolt-action reload start (a bolt gun that still
/// needs its bolt worked starts the reload in the bolt variant, and keeps
/// needing it when the reload is cut short by a weapon change) behind a
/// game setting whose name and default could not be read; on, as the DSR 50
/// plays in the game.
const BOLT_RELOAD_RECHAMBER: bool = true;

/// Up to this many events a hand can raise in one frame.
pub const MAX_EVENTS: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    list: [Option<WeaponTickEvent>; MAX_EVENTS],
    len: usize,
}

impl Events {
    fn push(&mut self, ev: WeaponTickEvent) {
        if self.len < MAX_EVENTS {
            self.list[self.len] = Some(ev);
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = WeaponTickEvent> + '_ {
        self.list[..self.len].iter().flatten().copied()
    }

    pub fn first(&self) -> Option<WeaponTickEvent> {
        self.list[0]
    }
}

struct Pm<'a> {
    h: &'a mut WeaponHandState,
    f: &'a WeaponCombatFacts,
    c: &'a mut WeaponCmd,
    ev: Events,
}

fn state_of(raw: i32) -> Option<S> {
    S::from_i32(raw).ok()
}

fn in_range(st: i32, lo: S, hi: S) -> bool {
    (lo as i32..=hi as i32).contains(&st)
}

/// Black Ops II's raw fire type: its own when the weapon is Black Ops II's,
/// else the remapped one (full auto, single, bursts).
fn fire_type_of(f: &WeaponCombatFacts) -> i32 {
    if f.t6_fire_type != 0 { f.t6_fire_type } else { f.fire_type }
}

fn is_burst_fire_type(t: i32) -> bool {
    (2..=5).contains(&t)
}

fn shot_limit_reached(f: &WeaponCombatFacts, shot_count: u8) -> bool {
    let n = i32::from(shot_count);
    match fire_type_of(f) {
        1 | 8 => n > 0,
        t @ 2..=5 => n >= t,
        6 => n >= f.clip_size,
        _ => false,
    }
}

fn is_grenade_type(f: &WeaponCombatFacts) -> bool {
    matches!(f.t6_weap_type, WEAPTYPE_GRENADE | WEAPTYPE_MINE)
}

fn is_reload_state(st: i32) -> bool {
    in_range(st, S::Reloading, S::ReloadQuickEmpty)
}

fn is_offhand_state(st: i32) -> bool {
    in_range(st, S::OffhandInit, S::OffhandDone)
}

fn is_melee_state(st: i32) -> bool {
    in_range(st, S::MeleeInit, S::MeleeEnd)
}

fn is_deploy_state(st: i32) -> bool {
    in_range(st, S::DeployInit, S::Deploy22)
}

fn is_dtp_state(st: i32) -> bool {
    in_range(st, S::DtpIn, S::DtpOut)
}

fn is_drop_state(st: i32) -> bool {
    in_range(st, S::Dropping, S::DroppingAltswitch)
}

fn is_raise_or_drop(st: i32) -> bool {
    in_range(st, S::Raising, S::DroppingAltswitch)
}

/// The frame's msec as the weapon timers count it: shortened by the reload
/// perk in the reload states and by Fast Hands in every raise and drop.
pub(crate) fn timer_msec(h: &WeaponHandState, f: &WeaponCombatFacts, c: &WeaponCmd) -> i32 {
    let st = h.weaponstate;
    let msec = c.msec;
    let scaled = |mult: f32| -> i32 {
        if mult == 0.0 {
            h.weapon_time.max(h.weapon_delay)
        } else {
            ((msec as f32 / mult) + 0.5) as i32
        }
    };
    if is_reload_state(st) && crate::tick::perk_fastreload_eligible(c.perks0, f.inherits_perks) {
        return scaled(c.perk_weap_reload_multiplier);
    }
    if crate::tick::perk_fastweaponswitch_applies(c.perks0, st) && f.offhand_slot != 4 {
        return scaled(crate::tick::PERK_WEAP_SWITCH_MULTIPLIER_DEFAULT);
    }
    msec
}

impl Pm<'_> {
    fn st(&self) -> i32 {
        self.h.weaponstate
    }

    fn set_st(&mut self, s: S) {
        self.h.weaponstate = s as i32;
    }

    fn offhand_facts(&self) -> WeaponCombatFacts {
        let w = self.c.offhand.off_hand_index.max(0) as u32;
        self.c
            .offhand
            .inventory
            .iter()
            .find(|r| r.weapon == w && w != 0)
            .map_or_else(WeaponCombatFacts::none, |r| r.combat)
    }

    fn fire_mask(&self) -> u32 {
        get_weapon_fire_button(self.c.last_weapon_hand, i32::from(self.h.hand_index))
    }

    fn attack(&self) -> bool {
        self.c.buttons & self.fire_mask() != 0
    }

    fn old_attack(&self) -> bool {
        self.c.old_buttons & self.fire_mask() != 0
    }

    fn btn(&self, b: u32) -> bool {
        self.c.buttons & b != 0
    }

    fn old_btn(&self, b: u32) -> bool {
        self.c.old_buttons & b != 0
    }

    fn anim(&mut self, a: u32) {
        if self.c.pm_type < 10 {
            start_weapon_anim(&mut self.h.weap_anim, a);
        }
    }

    fn anim_if_changed(&mut self, a: u32) {
        if self.c.pm_type < 10 && (self.h.weap_anim as u32 & !crate::WEAP_ANIM_RESTART_BIT) != a {
            start_weapon_anim(&mut self.h.weap_anim, a);
        }
    }

    fn weapons_disabled(&self) -> bool {
        self.c.weap_flags & WEAPONS_DISABLED != 0
    }

    fn clip_empty(&self) -> bool {
        !self.f.unlimited_ammo && self.h.clip == 0
    }

    fn ads_full(&self) -> bool {
        self.c.f_weapon_pos_frac == 1.0
    }

    fn ads_anim(&self) -> bool {
        self.c.f_weapon_pos_frac > 0.0
    }

    fn burst_in_progress(&self) -> bool {
        if self.h.weapon == 0 {
            return false;
        }
        let t = fire_type_of(self.f);
        if t == 0 || t == 7 || t == 9 || self.h.shot_count == 0 {
            return false;
        }
        if self.h.stock == 0 && self.h.clip == 0 {
            return false;
        }
        !shot_limit_reached(self.f, self.h.shot_count)
    }

    fn reset_to_ready(&mut self) {
        self.h.flags &= !flags::OFFHAND;
        self.c.pm_flags &= !pm_flags::PRONEMOVE_OVERRIDDEN;
        self.h.weapon_time = 0;
        self.h.weapon_delay = 0;
        self.set_st(S::Ready);
        self.anim(anim::IDLE);
    }

    fn timer_msec(&self) -> i32 {
        timer_msec(self.h, self.f, self.c)
    }

    fn should_hold_timer(&self) -> bool {
        let st = self.st();
        if is_offhand_state(st) || in_range(st, S::CrawlIn, S::CrawlLeft) || self.h.flags & flags::OFFHAND_PUTAWAY != 0 {
            return false;
        }
        let limit = self.h.flags & flags::BURST_LATCH == 0 && shot_limit_reached(self.f, self.h.shot_count);
        let grenade_hold = is_grenade_type(self.f) && self.f.hold_button_to_throw;
        if !(limit || grenade_hold) || !self.attack() {
            return false;
        }
        if self.h.weapon != u32::from(self.c.cmd_weapon)
            && (!self.c.alternate_switch || !self.f.alt_weapon_disable_switching)
        {
            return false;
        }
        self.h.clip != 0 || self.f.unlimited_ammo
    }

    fn finish_rechamber(&mut self) {
        if self.c.pm_type < 10 && (self.h.weap_anim as u32 & !crate::WEAP_ANIM_RESTART_BIT) != 0 {
            start_weapon_anim(&mut self.h.weap_anim, anim::IDLE);
        }
        self.h.rechamber_pending = false;
        self.set_st(S::Ready);
        if self.f.bolt_action && self.f.shots_before_rechamber != 0 {
            self.c.rechamber_shot_count = 0;
        }
    }

    /// The weapon timers; true on the frame the weapon delay runs out.
    fn update_timers(&mut self) -> bool {
        let elapsed = self.timer_msec();
        if self.h.weapon_time != 0 {
            self.h.weapon_time -= elapsed;
            if self.h.weapon_time > 0 {
                return self.update_delay(elapsed);
            }
            let st = self.st();
            if (st == S::Firing as i32 || st == S::ContFireLoop as i32)
                && self.h.weapon != 0
                && is_burst_fire_type(fire_type_of(self.f))
                && !self.burst_in_progress()
            {
                self.h.weapon_time = if self.f.burst_cooldown_ms == 0 { 1 } else { self.f.burst_cooldown_ms };
                self.anim_if_changed(anim::IDLE);
                self.set_st(S::Ready);
                return false;
            }
            if !self.should_hold_timer() {
                if (!self.attack() || self.h.flags & flags::BURST_LATCH != 0) && !self.burst_in_progress() {
                    self.h.shot_count = 0;
                }
                self.h.weapon_time = 0;
            } else {
                self.h.weapon_time = if st == S::RaisingAltswitch as i32 || st == S::DroppingAltswitch as i32 { 0 } else { 1 };
                if is_reload_state(st) {
                    self.h.weapon_time = 0;
                    self.h.shot_count = 0;
                } else if st == S::RechamberingBolted as i32 {
                    self.finish_rechamber();
                } else if st == S::Firing as i32
                    || st == S::Rechambering as i32
                    || st == S::ContFireLoop as i32
                    || is_melee_state(st)
                {
                    self.anim_if_changed(anim::IDLE);
                    self.set_st(S::Ready);
                }
            }
        }
        self.update_delay(elapsed)
    }

    fn update_delay(&mut self, elapsed: i32) -> bool {
        if self.h.weapon_delay != 0 {
            self.h.weapon_delay -= elapsed;
            if self.h.weapon_delay < 1 {
                self.h.weapon_delay = 0;
                return true;
            }
        }
        false
    }

    fn update_burst_latch(&mut self) {
        let t = fire_type_of(self.f);
        if t != 0 && t != 1 && self.attack() && !self.old_attack() {
            self.h.flags |= flags::BURST_LATCH;
        }
    }

    // --- sprint -----------------------------------------------------------

    fn check_for_sprint(&mut self) {
        let st = self.st();
        if self.c.cmd_weapon == 0
            || matches!(st, x if x == S::Firing as i32 || x == S::Rechambering as i32 || x == S::RechamberingBolted as i32 || x == S::ContFireLoop as i32)
            || is_melee_state(st)
            || is_raise_or_drop(st)
            || is_offhand_state(st)
            || is_dtp_state(st)
            || is_deploy_state(st)
            || st == S::Detonating as i32
        {
            return;
        }
        let sprinting = self.c.pm_flags & pm_flags::SPRINTING != 0;
        if sprinting && !in_range(st, S::SprintIn, S::SprintOut) {
            self.begin_sprint();
        } else if !sprinting && (st == S::SprintIn as i32 || st == S::SprintLoop as i32) {
            self.end_sprint();
        }
    }

    fn sprint_anim(&self, full: u32) -> u32 {
        if self.h.clip == 0 {
            full + (anim::SPRINT_IN_EMPTY - anim::SPRINT_IN)
        } else {
            full
        }
    }

    fn begin_sprint(&mut self) {
        self.set_st(S::SprintIn);
        self.h.weapon_time = self.f.sprint_raise_time_ms;
        self.h.weapon_delay = 0;
        let a = self.sprint_anim(anim::SPRINT_IN);
        self.anim(a);
    }

    fn end_sprint(&mut self) {
        self.set_st(S::SprintOut);
        let mut time = self.f.sprint_drop_time_ms;
        if self.c.perks0 & crate::sprint::PERK_SPRINTRECOVERY != 0 {
            time = (time as f32 * crate::sprint::SPRINT_RECOVERY_MULTIPLIER) as i32;
        }
        self.h.weapon_time = time;
        self.h.weapon_delay = 0;
        let a = self.sprint_anim(anim::SPRINT_OUT);
        self.anim(a);
    }

    fn sprint_loop(&mut self) {
        self.set_st(S::SprintLoop);
        self.h.weapon_time = 0;
        self.h.weapon_delay = 0;
        let a = self.sprint_anim(anim::SPRINT_LOOP);
        self.anim(a);
    }

    // --- weapon change ----------------------------------------------------

    fn is_quick_switch(&self, old_w: u32, old: &WeaponCombatFacts, new_w: u32, new: &WeaponCombatFacts, state: i32) -> bool {
        let mut q = state == S::DroppingQuick as i32;
        if old_w == 0 && new.t6_weap_type == WEAPTYPE_PROJECTILE {
            q = true;
        }
        if old.can_use_in_vehicle || new.can_use_in_vehicle {
            q = true;
        }
        if new.offhand_slot != 0 || old.offhand_slot != 0 {
            q = true;
        }
        if new_w != 0
            && (matches!(new.t6_weap_class, 4 | 11 | 13) || new.weap_class == crate::WEAPCLASS_PISTOL)
        {
            return true;
        }
        q
    }

    fn check_for_change_weapon(&mut self) {
        let st = self.st();
        if is_melee_state(st)
            || is_dtp_state(st)
            || st == S::NightVisionWear as i32
            || st == S::NightVisionRemove as i32
            || is_deploy_state(st)
            || self.weapons_disabled()
        {
            return;
        }
        if self.h.weapon_time != 0
            && !is_reload_state(st)
            && st != S::Rechambering as i32
            && st != S::RechamberingBolted as i32
        {
            if st == S::Firing as i32 || st == S::ContFireLoop as i32 || is_melee_state(st) {
                return;
            }
            if self.h.weapon_delay != 0 {
                return;
            }
        }
        if crate::weapon_change::traversal_forces_holster(self.c)
            || self.h.flags & flags::OFFHAND_PUTAWAY != 0
        {
            if self.h.weapon != 0 {
                self.begin_weapon_change(0, true);
            }
            return;
        }
        let new = u32::from(self.c.cmd_weapon);
        let blocked = (self.c.pm_flags & crate::weapon_change::PMF_CHANGE_BLOCK != 0 || self.c.mantle_weapon_up) && self.h.weapon != 0;
        let owned = new == 0 || self.c.cmd_weapon_owned;
        let offhand_holds = is_offhand_state(st)
            && !(self.offhand_facts().offhand_slot == 3 && self.btn(buttons::OFFHAND_HOLD_CANCEL));
        if self.h.weapon == new || st == S::Mounted as i32 || blocked || !owned || offhand_holds {
            if self.h.weapon == new && is_drop_state(st) {
                self.reset_to_ready();
                self.anim(anim::FORCE_IDLE);
                if st == S::DroppingAltswitch as i32
                    && self.f.alt_weapon_disable_switching
                    && self.should_hold_timer()
                {
                    self.h.weapon_time = 1;
                }
                return;
            }
            if self.h.weapon == 0 || self.c.held_weapon_owned {
                return;
            }
            let quick = self.is_quick_switch(self.h.weapon, self.f, new, &self.c.cmd_facts, st);
            self.begin_weapon_change(new, quick);
            return;
        }
        if st == S::Mounted as i32 || st == S::Mounting as i32 || st == S::Unmounting as i32 {
            return;
        }
        let quick = self.h.flags & flags::AFTER_OFFHAND != 0
            || self.is_quick_switch(self.h.weapon, self.f, new, &self.c.cmd_facts, st);
        self.begin_weapon_change(new, quick);
    }

    fn begin_weapon_change(&mut self, new: u32, quick: bool) {
        if new != 0 && !self.c.cmd_weapon_owned {
            return;
        }
        let st = self.st();
        if is_drop_state(st) {
            return;
        }
        if is_reload_state(st) {
            self.ev.push(WeaponTickEvent::ReloadCancelled { weaponstate: st });
        }
        self.h.flags &= !flags::AFTER_OFFHAND;
        self.h.weapon_delay = 0;
        let offhand = is_offhand_state(st);
        let (old_w, old) = if offhand {
            (self.c.offhand.off_hand_index.max(0) as u32, self.offhand_facts())
        } else {
            (self.h.weapon, *self.f)
        };
        let new_facts = if new != 0 { self.c.cmd_facts } else { WeaponCombatFacts::none() };
        if new_facts.t6_weap_type == WEAPTYPE_RIOTSHIELD {
            self.h.flags |= flags::SHIELD_CHANGE;
        } else {
            self.h.flags &= !flags::SHIELD_CHANGE;
        }
        if new_facts.can_use_in_vehicle && old.can_use_in_vehicle {
            return;
        }
        if old_w == 0 || old.offhand_slot != 3 {
            let old_held = old_w != 0 && (offhand || self.c.held_weapon_owned);
            if !old_held || self.c.offhand.grenade_time_left > 0 {
                self.h.flags &= !flags::OFFHAND;
                self.h.weapon_time = 0;
                self.c.offhand.grenade_time_left = 0;
                self.h.weaponstate = if quick { S::DroppingQuick } else { S::Dropping } as i32;
                return;
            }
        } else {
            self.h.flags &= !flags::OFFHAND;
        }
        let empty = self.h.weapon != 0 && self.clip_empty();
        let alt = !offhand && old_w == self.h.weapon && new != 0 && self.c.alternate_switch;
        self.c.offhand.grenade_time_left = 0;
        if self.h.flags & flags::OFFHAND_PUTAWAY == 0 {
            self.ev.push(WeaponTickEvent::SwitchStarted { weapon: self.h.weapon });
        }
        let cooking_equipment = st == S::Firing as i32 && old.t6_weap_type == WEAPTYPE_GRENADE && old.offhand_slot == 4;
        if !alt {
            if !cooking_equipment {
                self.ev.push(WeaponTickEvent::PutawayStarted);
                if !old.no_drops_or_raises {
                    let a = if !empty || (quick && !old.no_quick_drop_when_empty) {
                        if quick { anim::QUICK_DROP } else { anim::DROP }
                    } else {
                        anim::EMPTY_DROP
                    };
                    self.anim(a);
                }
            }
        } else {
            self.ev.push(WeaponTickEvent::AlternateStarted);
            self.anim(anim::ALT_DROP);
        }
        if BOLT_RELOAD_RECHAMBER && old.bolt_action && st == S::ReloadStartBolt as i32 {
            self.h.rechamber_pending = true;
        }
        self.h.weaponstate = if alt {
            S::DroppingAltswitch
        } else if quick {
            S::DroppingQuick
        } else {
            S::Dropping
        } as i32;
        self.h.weapon_time = if old.no_drops_or_raises {
            0
        } else if alt {
            old.alternate_drop_time_ms
        } else if quick {
            old.quick_drop_time_ms
        } else if empty {
            old.empty_drop_time_ms
        } else {
            old.drop_time_ms
        };
    }

    fn skip_raise_between(&self, old_w: u32, new_w: u32) -> bool {
        let old = self.f;
        let new = &self.c.cmd_facts;
        let partners = (old.alternate_weapon == new_w && new_w != 0) || (new.alternate_weapon == old_w && old_w != 0);
        partners && old.alt_weapon_ads_only && new.alt_weapon_ads_only
    }

    fn finish_weapon_change(&mut self, state: i32) {
        let mut new = u32::from(self.c.cmd_weapon);
        if self.h.flags & flags::OFFHAND_PUTAWAY != 0
            || self.c.pm_flags & pm_flags::LADDER != 0
            || crate::weapon_change::traversal_forces_holster(self.c)
            || !self.c.cmd_weapon_owned
        {
            new = 0;
        }
        let old = self.h.weapon;
        self.h.weapon = new;
        if self.h.flags & flags::OFFHAND_PUTAWAY != 0 {
            self.set_st(S::OffhandInit);
            return;
        }
        if old == new {
            self.set_st(S::Ready);
            self.anim(anim::IDLE);
            return;
        }
        let first = new != 0 && self.c.cmd_first_raise && self.c.pm_type != 9 && self.c.pm_type != 7;
        if new != 0 {
            self.c.first_raised = Some(new);
        }
        let new_facts = if new != 0 { self.c.cmd_facts } else { WeaponCombatFacts::none() };
        let alt = old != 0 && new != 0 && self.c.alternate_switch;
        let (a, time) = if !alt {
            let quick = self.is_quick_switch(old, self.f, new, &new_facts, state);
            let empty = !new_facts.unlimited_ammo && self.c.cmd_clip == 0;
            if new_facts.no_drops_or_raises {
                (anim::IDLE, 0)
            } else if empty {
                (anim::EMPTY_RAISE, new_facts.empty_raise_time_ms)
            } else if first {
                (anim::FIRST_RAISE, new_facts.first_raise_time_ms)
            } else if !quick {
                (anim::RAISE, new_facts.raise_time_ms)
            } else {
                (anim::QUICK_RAISE, new_facts.quick_raise_time_ms)
            }
        } else {
            if !self.attack() {
                self.h.shot_count = 0;
            }
            (anim::ALT_RAISE, self.c.switch_alternate_raise_time_ms)
        };
        if !new_facts.no_drops_or_raises {
            if !self.skip_raise_between(old, new) {
                self.h.weapon_time = time;
                self.set_st(if alt { S::RaisingAltswitch } else { S::Raising });
                self.anim(a);
            }
        } else {
            self.set_st(S::Ready);
            self.h.weapon_time = 0;
            self.anim(anim::IDLE);
        }
        self.ev.push(WeaponTickEvent::RaiseStarted);
    }

    fn finish_raise(&mut self) {
        self.set_st(S::Ready);
        self.anim(anim::IDLE);
        self.ev.push(WeaponTickEvent::RaiseFinished);
    }

    // --- reload -----------------------------------------------------------

    fn can_reload(&self) -> bool {
        crate::reload::weapon_allow_reload(self.h, self.f)
    }

    fn auto_reload_allowed(&self, st: i32) -> bool {
        if is_raise_or_drop(st)
            || st == S::Reloading as i32
            || st == S::ReloadingInterrupt as i32
            || st == S::ReloadEnd as i32
            || st == S::UnderwaterDrop as i32
            || in_range(st, S::ReloadStart, S::ReloadStartInterrupt)
        {
            return false;
        }
        st != S::Firing as i32
            && st != S::ContFireLoop as i32
            && !in_range(st, S::SprintIn, S::SprintOut)
            && !in_range(st, S::Mounted, S::Unmounting)
            && !self.f.unlimited_ammo
            && (!self.f.no_ads_auto_reload || !self.ads_full())
    }

    fn check_for_reload(&mut self) {
        let st = self.st();
        if st == S::MeleeEnd as i32 && self.clip_empty() && !self.f.has_bayonet {
            self.set_st(S::Raising);
            self.h.weapon_time = self.f.empty_raise_time_ms;
            self.anim(anim::EMPTY_RAISE);
            return;
        }
        if is_offhand_state(st)
            || is_melee_state(st)
            || is_dtp_state(st)
            || is_deploy_state(st)
            || self.c.pm_flags & pm_flags::MANTLE != 0
            || self.c.mantle_weapon_up
        {
            return;
        }
        let mut wants = self.h.flags & flags::RELOAD_LATCH != 0;
        self.h.flags &= !flags::RELOAD_LATCH;
        wants |= self.btn(BUTTON_RELOAD);
        if self.f.segmented_reload && self.attack() && !self.old_attack() {
            if (st == S::ReloadStart as i32 || st == S::ReloadStartBolt as i32) && self.f.reload_start_time_ms != 0 {
                let done = (self.f.reload_start_time_ms - self.h.weapon_time) as f32 / self.f.reload_start_time_ms as f32;
                if done > 0.0 {
                    self.set_st(S::ReloadStartInterrupt);
                }
            } else if st == S::Reloading as i32 {
                self.set_st(S::ReloadingInterrupt);
            }
        }
        let go = wants && self.can_reload();
        let st = self.st();
        if (self.h.clip == 0 && self.h.stock != 0 && self.auto_reload_allowed(st)) || go {
            self.begin_weapon_reload();
        }
    }

    fn begin_weapon_reload(&mut self) {
        let st = self.st();
        let allowed = st == S::Ready as i32
            || in_range(st, S::CrawlForward, S::CrawlLeft)
            || st == S::Firing as i32
            || st == S::ContFireLoop as i32
            || st == S::Rechambering as i32
            || st == S::RechamberingBolted as i32
            || in_range(st, S::SprintIn, S::SprintOut)
            || in_range(st, S::LowReadyIn, S::LowReadyOut);
        if !allowed || self.h.weapon == 0 || self.weapons_disabled() {
            return;
        }
        self.h.shot_count = 0;
        self.ev.push(WeaponTickEvent::ReloadStarted);
        if self.f.reload_start_time_ms == 0 || !self.f.segmented_reload {
            self.set_reloading_state();
        } else {
            self.anim(anim::RELOAD_START);
            self.h.weapon_time = self.f.reload_start_time_ms;
            self.set_st(if BOLT_RELOAD_RECHAMBER && self.f.bolt_action && self.h.rechamber_pending { S::ReloadStartBolt } else { S::ReloadStart });
            let t = self.h.weapon_time;
            crate::reload::weapon_arm_reload_add_delay(self.h, self.f, t);
        }
    }

    fn set_reloading_state(&mut self) {
        if self.f.unlimited_ammo {
            return;
        }
        let empty = self.h.clip == 0 && self.f.t6_weap_type == WEAPTYPE_BULLET;
        let (ms, a) = crate::reload::reload_segment(self.h, self.f, empty);
        self.anim(a);
        self.h.weapon_time = ms;
        let interrupt = self.st() == S::ReloadStartInterrupt as i32;
        self.set_st(if interrupt { S::ReloadingInterrupt } else { S::Reloading });
        crate::reload::weapon_arm_reload_add_delay(self.h, self.f, ms);
    }

    fn commit_reload_ammo(&mut self) {
        let out = crate::reload::weapon_reload_delayed_action(self.h, self.f, true);
        if out.rechamber_event {
            self.h.delayed_rechamber = true;
            self.ev.push(WeaponTickEvent::RechamberWeapon);
        }
        if out.shells > 0 {
            self.ev.push(WeaponTickEvent::ReloadAmmoAdded { shells: out.shells });
        }
    }

    fn reload_loop_end(&mut self, delay_expired: bool) {
        if delay_expired {
            self.commit_reload_ammo();
        }
        if self.h.weapon_time != 0 {
            return;
        }
        let segmented = self.f.segmented_reload;
        if segmented && self.attack() {
            self.set_st(S::ReloadingInterrupt);
        }
        self.h.rechamber_pending = false;
        if self.f.bolt_action && self.f.shots_before_rechamber != 0 {
            self.c.rechamber_shot_count = 0;
        }
        if !segmented {
            self.set_st(S::Ready);
            self.anim(anim::IDLE);
            return;
        }
        if self.st() != S::ReloadingInterrupt as i32 && self.can_reload() {
            self.set_reloading_state();
            self.ev.push(WeaponTickEvent::ReloadInsert);
            return;
        }
        if self.f.reload_end_time_ms == 0 {
            self.set_st(S::Ready);
            self.anim(anim::IDLE);
            return;
        }
        self.set_st(S::ReloadEnd);
        self.anim(anim::RELOAD_END);
        self.h.weapon_time = self.f.reload_end_time_ms;
        self.ev.push(WeaponTickEvent::ReloadEnded);
    }

    fn reload_start_end(&mut self, delay_expired: bool) {
        if delay_expired {
            self.commit_reload_ammo();
        }
        if self.h.weapon_time != 0 {
            return;
        }
        if self.attack() && self.f.segmented_reload {
            self.set_st(S::ReloadStartInterrupt);
        }
        if (self.st() != S::ReloadStartInterrupt as i32 || self.h.clip == 0) && self.can_reload() {
            self.set_reloading_state();
            self.ev.push(WeaponTickEvent::ReloadInsert);
            return;
        }
        self.h.rechamber_pending = false;
        if self.f.reload_end_time_ms == 0 {
            self.set_st(S::Ready);
            self.anim(anim::IDLE);
            return;
        }
        self.set_st(S::ReloadEnd);
        self.anim(anim::RELOAD_END);
        self.h.weapon_time = self.f.reload_end_time_ms;
        self.ev.push(WeaponTickEvent::ReloadEnded);
    }

    // --- melee ------------------------------------------------------------

    fn melee_facts(&self) -> MeleeWeaponFacts {
        if self.f.has_bayonet {
            MeleeWeaponFacts::from_combat(self.f)
        } else {
            self.c.knife.unwrap_or_else(|| MeleeWeaponFacts::from_combat(self.f))
        }
    }

    fn check_for_melee(&mut self, delay_expired: bool) {
        if self.h.hand_index != 0 {
            return;
        }
        if self.c.pm_flags & pm_flags::LADDER != 0 {
            return;
        }
        let st = self.st();
        if is_offhand_state(st)
            || st == S::NightVisionWear as i32
            || st == S::NightVisionRemove as i32
            || is_dtp_state(st)
            || is_deploy_state(st)
            || self.c.pm_flags & pm_flags::MANTLE != 0
        {
            return;
        }
        let mf = self.melee_facts();
        let mut pressed = self.btn(buttons::MELEE_CHARGE);
        if is_melee_state(st) && pressed {
            pressed = false;
        }
        let edge = self.btn(buttons::MELEE_CHARGE) && !self.old_btn(buttons::MELEE_CHARGE);
        let delay_ok = self.h.weapon_delay == 0 || is_reload_state(st);
        let scope_ok = self.c.f_weapon_pos_frac <= 0.0 || mf.overlay_reticle == 0;
        if mf.melee_damage != 0
            && !delay_expired
            && delay_ok
            && edge
            && scope_ok
            && !is_raise_or_drop(st)
            && pressed
        {
            melee_charge_start(
                &mut self.c.melee_charge,
                self.c.melee_charge_yaw,
                self.c.melee_charge_dist,
                self.c.is_in_air,
            );
            self.c.pm_flags = (self.c.pm_flags & !pm_flags::MELEE_CHARGE) | (self.c.melee_charge.pm_flags & pm_flags::MELEE_CHARGE);
            let charge = weapon_start_melee_uses_charge(&self.c.melee_charge, self.c.player_melee_range, &mf);
            self.begin_melee(&mf, charge);
        }
    }

    fn begin_melee(&mut self, mf: &MeleeWeaponFacts, charge: bool) {
        let empty = self.h.clip == 0;
        let (time, delay, a) = if charge {
            (mf.melee_charge_time_ms, mf.melee_charge_delay_ms, if empty { anim::MELEE_CHARGE_EMPTY } else { anim::MELEE_CHARGE })
        } else {
            (mf.melee_time_ms, mf.melee_delay_ms, if empty { anim::MELEE_EMPTY } else { anim::MELEE })
        };
        self.h.weapon_time = time;
        self.h.weapon_delay = delay;
        self.anim(a);
        self.set_st(S::MeleeInit);
        self.c.melee_started = Some(charge);
    }

    fn melee_end(&mut self) {
        if !self.f.has_bayonet {
            self.set_st(S::MeleeEnd);
            self.h.weapon_time = self.f.quick_raise_time_ms;
            self.h.weapon_delay = 0;
            self.anim(anim::QUICK_RAISE);
            return;
        }
        self.reset_to_ready();
    }

    // --- rechamber --------------------------------------------------------

    /// True: the rest of the frame is skipped.
    fn check_for_rechamber(&mut self, delay_expired: bool) -> bool {
        let st = self.st();
        if is_offhand_state(st) || is_dtp_state(st) || is_deploy_state(st) {
            return false;
        }
        if !self.f.bolt_action || !self.h.rechamber_pending {
            return false;
        }
        if st == S::Rechambering as i32 && delay_expired {
            self.set_st(S::RechamberingBolted);
            self.ev.push(WeaponTickEvent::EjectBrass);
            if self.h.weapon_time != 0 {
                return true;
            }
        } else if self.h.weapon_time != 0 {
            if st == S::Firing as i32
                || st == S::Rechambering as i32
                || st == S::RechamberingBolted as i32
                || st == S::ContFireLoop as i32
                || is_melee_state(st)
            {
                return false;
            }
            if self.h.weapon_delay != 0 {
                return false;
            }
        }
        let st = self.st();
        if st == S::RechamberingBolted as i32 {
            self.finish_rechamber();
            return false;
        }
        if st == S::Ready as i32 || in_range(st, S::CrawlIn, S::CrawlOut) {
            let a = if self.ads_anim() { anim::ADS_RECHAMBER } else { anim::RECHAMBER };
            self.anim(a);
            self.set_st(S::Rechambering);
            self.h.weapon_time = self.f.rechamber_time_ms;
            if self.f.shots_before_rechamber != 0 {
                self.c.rechamber_shot_count = 0;
            }
            let bolt = self.f.rechamber_bolt_delay_ms;
            self.h.weapon_delay = if bolt != 0 && bolt < self.f.rechamber_time_ms { bolt } else { 1 };
            self.h.delayed_rechamber = false;
            self.ev.push(WeaponTickEvent::RechamberWeapon);
        }
        false
    }

    // --- firing -----------------------------------------------------------

    /// Whether the weapon wants to fire; unless `peek`, a weapon that does
    /// not is put back to ready.
    fn should_fire(&mut self, delay_expired: bool, peek: bool) -> bool {
        if self.f.t6_weap_type == WEAPTYPE_RIOTSHIELD || self.weapons_disabled() {
            return false;
        }
        let mut fire = self.attack();
        if self.f.freeze_movement_when_firing && !self.c.on_ground {
            fire = false;
        }
        if self.f.can_use_in_vehicle {
            fire = false;
        }
        let mut held = delay_expired || self.burst_in_progress();
        if self.f.bolt_action && self.f.shots_before_rechamber != 0 && self.h.rechamber_pending {
            fire = false;
            held = false;
        }
        if fire || held {
            return true;
        }
        if peek {
            return false;
        }
        if self.st() == S::Firing as i32 && !self.weapons_disabled() {
            self.anim_if_changed(anim::IDLE);
        }
        self.set_st(S::Ready);
        false
    }

    fn is_last_round(&self) -> bool {
        let clip = self.h.clip;
        match fire_type_of(self.f) {
            2 => clip < 3,
            3 => clip < 4,
            4 => clip < 5,
            5 => clip < 6,
            _ => clip < 2,
        }
    }

    fn start_firing(&mut self, delay_expired: bool) {
        let st = self.st();
        if st != S::Firing as i32 {
            self.h.firing_counter = 0;
        }
        if is_grenade_type(self.f) {
            if !delay_expired {
                if self.h.clip != 0 {
                    self.c.offhand.grenade_time_left = self.f.fuse_time_ms;
                    self.anim(anim::HOLD_FIRE);
                }
                self.h.weapon_delay = self.f.hold_fire_time_ms;
                self.h.weapon_time = 0;
            }
        } else {
            let in_intro = self.f.intro_fire_length != 0 && i32::from(self.h.firing_counter) < self.f.intro_fire_length;
            self.h.weapon_time = if self.is_last_round() && self.f.last_fire_time_ms != 0 {
                self.f.last_fire_time_ms
            } else if self.f.intro_fire_time_ms != 0 && in_intro {
                self.f.intro_fire_time_ms
            } else {
                self.f.fire_time_ms
            };
            if self.f.bolt_action && self.f.shots_before_rechamber == 0 {
                self.h.rechamber_pending = true;
            }
            if st != S::Firing as i32 && st != S::ContFireLoop as i32 {
                self.h.weapon_delay = self.f.fire_delay_ms;
                let reduced = if self.c.f_weapon_pos_frac < 1.0 {
                    self.f.hip_gun_kick_reduced_kick_bullets
                } else {
                    self.f.ads_gun_kick_reduced_kick_bullets
                };
                self.h.weapon_restrict_kick_time = self.f.fire_time_ms * reduced + self.f.fire_delay_ms;
            }
            if self.f.ads_fire_only && fire_type_of(self.f) != 7 {
                let wait = crate::tick::ads_fire_only_delay_ms(self.c.f_weapon_pos_frac, self.f.ads_in_rate);
                self.h.weapon_delay = self.h.weapon_delay.max(wait);
            }
        }
        if !in_range(self.st(), S::ContFireIn, S::ContFireOut) {
            self.set_st(S::Firing);
        }
        if self.f.bolt_action && self.f.shots_before_rechamber != 0 {
            self.c.rechamber_shot_count = self.c.rechamber_shot_count.saturating_add(1);
            if i32::from(self.c.rechamber_shot_count) >= self.f.shots_before_rechamber {
                self.h.rechamber_pending = true;
            }
        }
        if self.h.weapon_delay == 0 && fire_type_of(self.f) != 0 {
            self.h.shot_count = (self.h.shot_count + 1).min(SHOT_COUNT_MAX);
        }
    }

    fn check_ammo_for_fire(&mut self) -> bool {
        if self.h.clip >= 1 {
            return true;
        }
        if !self.f.no_ads_auto_reload || !self.ads_full() {
            if self.h.stock >= 1 {
                self.begin_weapon_reload();
                return false;
            }
            if !is_grenade_type(self.f) {
                self.ev.push(WeaponTickEvent::EmptyClick);
            }
        }
        if !self.weapons_disabled() {
            self.h.rechamber_pending = false;
            self.anim_if_changed(anim::IDLE);
            if !is_grenade_type(self.f) {
                self.h.weapon_time += DRY_FIRE_MS;
            }
        }
        false
    }

    fn set_fire_anim(&mut self) {
        let empty = self.clip_empty();
        let in_intro = self.f.intro_fire_length != 0 && i32::from(self.h.firing_counter) <= self.f.intro_fire_length;
        let ads = self.ads_anim();
        if self.c.pm_type < 10 {
            crate::weap_anim::set_fps_fire_anim(&mut self.h.weap_anim, ads, empty, in_intro && !empty);
        }
    }

    fn fire_weapon(&mut self, delay_expired: bool) {
        if self.f.t6_weap_type == WEAPTYPE_MELEE {
            return;
        }
        if crate::fire_weapon_kind(self.f.weap_type, self.f.weap_class).is_none() && !is_grenade_type(self.f) {
            return;
        }
        if !self.f.unlimited_ammo && !self.check_ammo_for_fire() {
            return;
        }
        self.start_firing(delay_expired);
        if self.h.weapon_delay != 0 {
            return;
        }
        self.h.flags &= !flags::BURST_LATCH;
        let used = if self.f.unlimited_ammo || self.h.clip == 0 { 0 } else { 1.min(self.h.clip) };
        self.h.clip -= used;
        if is_grenade_type(self.f) {
            self.h.weapon_time = self.f.fire_time_ms;
        }
        self.h.firing_counter = self.h.firing_counter.saturating_add(1).min(FIRING_COUNTER_MAX);
        if !in_range(self.st(), S::ContFireIn, S::ContFireOut) {
            self.set_fire_anim();
        }
        self.ev.push(WeaponTickEvent::ShotAccepted { ammo_used: used });
    }

    // --- detonate ---------------------------------------------------------

    fn check_for_detonate(&mut self) {
        if self.h.weapon == 0 || !is_grenade_type(self.f) || !self.f.has_detonator {
            return;
        }
        let st = self.st();
        let busy = st == S::Detonating as i32
            || is_reload_state(st)
            || st == S::Firing as i32
            || st == S::Rechambering as i32
            || st == S::RechamberingBolted as i32
            || st == S::ContFireLoop as i32
            || is_melee_state(st)
            || is_raise_or_drop(st)
            || is_offhand_state(st)
            || is_deploy_state(st)
            || is_dtp_state(st);
        if busy || !self.btn(BUTTON_ATTACK) {
            return;
        }
        self.set_st(S::Detonating);
        self.h.weapon_time = self.f.detonate_time_ms;
        self.h.weapon_delay = self.f.detonate_delay_ms;
        self.anim(anim::DETONATE);
    }

    // --- offhand ----------------------------------------------------------

    fn check_grenade_timer(&mut self) -> bool {
        let (w, wf) = if self.h.flags & flags::OFFHAND != 0 {
            (self.c.offhand.off_hand_index.max(0) as u32, self.offhand_facts())
        } else if self.h.weapon == 0 {
            return false;
        } else {
            (self.h.weapon, *self.f)
        };
        if !is_grenade_type(&wf) || self.c.offhand.grenade_time_left <= 0 {
            return false;
        }
        if wf.cook_off_hold {
            self.c.offhand.grenade_time_left -= self.c.msec;
        }
        if self.c.offhand.grenade_time_left < 1 {
            self.c.offhand.grenade_time_left = -1;
            if w != 0 {
                self.ev.push(WeaponTickEvent::OffhandCookedOff { weapon: w });
            }
            return true;
        }
        false
    }

    fn check_for_offhand(&mut self) {
        let st = self.st();
        if (self.c.e_flags & 0xc00) != 0
            || (self.c.weap_flags & 0x80) != 0
            || (self.c.weap_flags & 0x1000) != 0
            || self.c.pm_flags & pm_flags::SPRINTING != 0
            || is_drop_state(st)
            || (is_offhand_state(st) && st != S::OffhandDone as i32)
            || is_dtp_state(st)
            || in_range(st, S::LowReadyIn, S::LowReadyOut)
            || is_deploy_state(st)
        {
            return;
        }
        if self.c.offhand.cmd_off_hand_owned && self.c.offhand.cmd_off_hand_index != 0 {
            self.c.offhand.off_hand_index = i32::from(self.c.offhand.cmd_off_hand_index);
        }
        let wanted = if self.btn(buttons::FRAG) && !self.old_btn(buttons::FRAG) {
            self.c.offhand.offhand_primary
        } else if self.btn(buttons::SMOKE) && !self.old_btn(buttons::SMOKE) {
            self.c.offhand.offhand_secondary
        } else {
            return;
        };
        let mut picked = crate::offhand::get_first_available_offhand(&self.c.offhand.inventory, wanted);
        if picked == 0 {
            picked = self
                .c
                .offhand
                .inventory
                .iter()
                .find(|row| row.weapon != 0 && row.offhand_class == wanted && row.has_detonator)
                .map_or(0, |row| row.weapon);
        }
        if picked == 0 {
            return;
        }
        self.c.offhand.off_hand_index = picked as i32;
        if self.c.offhand.cursor_hint_ent == crate::offhand::CURSOR_HINT_NONE
            && (self.h.weapon == 0 || st == S::OffhandDone as i32)
        {
            self.offhand_init();
        } else {
            self.begin_offhand_putaway();
        }
    }

    fn begin_offhand_putaway(&mut self) {
        self.h.flags &= !flags::OFFHAND;
        self.set_st(S::OffhandInit);
        self.h.weapon_delay = 0;
        if self.h.weapon != 0 {
            self.h.flags |= flags::OFFHAND_PUTAWAY;
        }
        self.c.pm_flags &= !pm_flags::ADS_INTENT;
    }

    fn offhand_init(&mut self) {
        if self.h.weapon != 0 {
            return;
        }
        let of = self.offhand_facts();
        let w = self.c.offhand.off_hand_index.max(0) as u32;
        self.set_st(S::OffhandPrepare);
        self.h.flags |= flags::OFFHAND;
        self.h.flags &= !flags::OFFHAND_PUTAWAY;
        self.h.weapon_time = of.hold_fire_time_ms;
        self.h.weapon_delay = 0;
        let ammo = self.c.offhand.inventory.iter().find(|r| r.weapon == w).map_or(0, |r| r.ammo);
        if !of.has_detonator || ammo != 0 {
            if w != 0 {
                self.ev.push(WeaponTickEvent::OffhandPrepare { weapon: w });
            }
            self.anim(anim::HOLD_FIRE);
        } else {
            self.set_st(S::OffhandStart);
            self.h.weapon_time = of.detonate_time_ms;
            self.h.weapon_delay = of.detonate_delay_ms;
            self.anim(anim::DETONATE);
        }
    }

    fn offhand_prepare(&mut self) {
        self.h.flags |= flags::OFFHAND;
        self.h.weapon_time = 0;
        self.h.weapon_delay = 0;
        self.set_st(S::Offhand);
        self.c.offhand.grenade_time_left = self.offhand_facts().fuse_time_ms;
    }

    fn offhand_fire(&mut self) {
        let of = self.offhand_facts();
        let held_before = self.old_btn(buttons::FRAG) || self.old_btn(buttons::SMOKE);
        let held_now = self.btn(buttons::FRAG) || self.btn(buttons::SMOKE);
        if !of.hold_button_to_throw && held_before && held_now {
            if of.offhand_hold_is_cancelable == Some(true) && self.btn(buttons::OFFHAND_HOLD_CANCEL) {
                self.offhand_end();
                return;
            }
            self.h.weapon_delay = 1;
            return;
        }
        if !of.is_hold_use_grenade {
            self.set_st(S::OffhandHold);
            self.h.weapon_time = of.fire_time_ms;
            self.h.weapon_delay = of.fire_delay_ms;
            self.h.flags |= flags::OFFHAND;
            self.anim(anim::FIRE);
            return;
        }
        self.offhand_finish();
    }

    fn offhand_hold(&mut self) {
        let w = self.c.offhand.off_hand_index.max(0) as u32;
        if w != 0 {
            let fuse = self.c.offhand.grenade_time_left;
            if let Some(row) = self.c.offhand.inventory.iter_mut().find(|r| r.weapon == w)
                && row.ammo > 0
            {
                row.ammo -= 1;
            }
            self.ev.push(WeaponTickEvent::OffhandUsed { weapon: w, remaining_fuse_ms: (fuse > 0).then_some(fuse) });
            self.c.offhand.grenade_time_left = 0;
        }
        self.h.flags |= flags::OFFHAND;
        self.set_st(S::OffhandEnd);
    }

    fn offhand_end(&mut self) {
        if self.h.weapon == 0 {
            self.h.flags |= flags::AFTER_OFFHAND;
            self.h.weapon_time = 0;
            self.h.weapon_delay = 1;
        } else {
            self.h.weapon_time = self.f.quick_raise_time_ms;
            self.h.weapon_delay = 0;
            let a = if self.clip_empty() { anim::EMPTY_RAISE } else { anim::QUICK_RAISE };
            self.anim(a);
        }
        self.h.flags &= !(flags::OFFHAND | flags::OFFHAND_PUTAWAY);
        self.c.pm_flags &= !pm_flags::PRONEMOVE_OVERRIDDEN;
        self.set_st(S::OffhandDone);
    }

    fn offhand_finish(&mut self) {
        self.h.weapon_time = self.offhand_facts().quick_drop_time_ms;
        self.h.weapon_delay = 0;
        self.anim(anim::DROP);
        self.h.flags |= flags::OFFHAND;
        self.set_st(S::OffhandEnd);
    }

    fn offhand_start(&mut self, delay_expired: bool) {
        if delay_expired {
            let w = if self.h.flags & flags::OFFHAND != 0 {
                self.c.offhand.off_hand_index.max(0) as u32
            } else {
                self.h.weapon
            };
            if w != 0 {
                self.ev.push(WeaponTickEvent::Detonated { weapon: w });
            }
            return;
        }
        self.reset_to_ready();
    }

    fn check_offhand_hold_cancel(&mut self) {
        let st = self.st();
        let of = self.offhand_facts();
        let cancelable = of.offhand_hold_is_cancelable == Some(true);
        let cancel = self.btn(buttons::OFFHAND_HOLD_CANCEL);
        if st == S::OffhandPrepare as i32 {
            if !of.hold_button_to_throw {
                if !cancelable || !cancel {
                    return;
                }
            } else if self.btn(buttons::FRAG) || self.btn(buttons::SMOKE) {
                return;
            }
            self.offhand_end();
        } else if st == S::OffhandInit as i32 && cancelable {
            if !cancel {
                return;
            }
            self.offhand_end();
            return;
        }
        let st = self.st();
        let wf = if is_offhand_state(st) { of } else { *self.f };
        let wf_cancelable = wf.offhand_hold_is_cancelable == Some(true);
        let firing = st == S::Firing as i32
            || st == S::Rechambering as i32
            || st == S::RechamberingBolted as i32
            || st == S::ContFireLoop as i32
            || is_melee_state(st);
        if is_grenade_type(&wf)
            && (wf_cancelable || (self.h.weapon != 0 && firing && wf.hold_button_to_throw))
            && !self.btn(BUTTON_ATTACK)
            && (!wf_cancelable || cancel)
            && wf.offhand_slot != 3
        {
            self.reset_to_ready();
            self.anim(anim::FORCE_IDLE);
        }
    }

    /// A grenade-type weapon in hand that is not thrown on release, with
    /// its trigger still down: the throw waits.
    fn holding_grenade_throw(&self) -> bool {
        self.h.weapon != 0
            && matches!(self.f.t6_weap_type, WEAPTYPE_GRENADE | WEAPTYPE_MINE)
            && !self.f.hold_button_to_throw
            && self.attack()
    }

    // --- riot shield ------------------------------------------------------

    fn check_for_shield_deploy(&mut self) {
        let st = self.st();
        if self.f.t6_weap_type != WEAPTYPE_RIOTSHIELD
            || self.c.weap_flags & (WEAPONS_DISABLED | 0x100) != 0
            || self.c.pm_flags & pm_flags::SPRINTING != 0
            || self.holding_grenade_throw()
            || is_drop_state(st)
            || in_range(st, S::DeployInit, S::Deploy21)
            || is_dtp_state(st)
            || in_range(st, S::LowReadyIn, S::LowReadyOut)
            || is_offhand_state(st)
            || !self.attack()
            || self.old_attack()
        {
            return;
        }
        self.set_st(S::DeployInit);
        self.h.weapon_time = self.f.hold_fire_time_ms;
        self.h.weapon_delay = 0;
        self.ev.push(WeaponTickEvent::ShieldRaised);
        self.anim(anim::HOLD_FIRE);
    }

    /// The raised shield is held while the trigger is; let go, it is
    /// planted if it has room, else put back.
    fn shield_deploy(&mut self) {
        if self.f.t6_weap_type != WEAPTYPE_RIOTSHIELD || self.h.weapon == 0 {
            self.shield_put_back();
            return;
        }
        if self.attack() && self.old_attack() {
            self.h.weapon_delay = 1;
            return;
        }
        if self.c.shield_can_deploy {
            self.set_st(S::DeployPrepare);
            self.h.weapon_time = self.f.fire_time_ms;
            self.h.weapon_delay = self.f.fire_delay_ms;
            self.anim(anim::FIRE);
        } else {
            self.shield_put_back();
        }
    }

    fn shield_put_back(&mut self) {
        self.set_st(S::Deploy22);
        self.h.weapon_time = self.f.reload_time_ms;
        self.h.weapon_delay = 0;
        self.anim(anim::RELOAD);
        self.ev.push(WeaponTickEvent::ShieldDeployed { planted: false });
    }

    // --- dive to prone ----------------------------------------------------

    fn check_dtp_lower(&mut self) {
        let st = self.st();
        if self.c.cmd_weapon == 0
            || st == S::Firing as i32
            || st == S::Rechambering as i32
            || st == S::RechamberingBolted as i32
            || st == S::ContFireLoop as i32
            || is_melee_state(st)
            || is_raise_or_drop(st)
            || is_offhand_state(st)
            || in_range(st, S::DeployInit, S::Deploy22)
            || is_dtp_state(st)
            || self.c.pm_flags & (PMF_DIVE | PMF_DIVE_SLIDE) == 0
        {
            return;
        }
        if self.c.pm_flags & PMF_DIVE_SLIDE != 0 {
            return;
        }
        self.set_st(S::DtpIn);
        self.h.weapon_time = self.f.dtp_in_time_ms;
        self.h.weapon_delay = 0;
        let a = if self.clip_empty() { anim::DIVE_IN_EMPTY } else { anim::DIVE_IN };
        self.anim_if_changed(a);
    }

    fn dtp_loop(&mut self) {
        // (The take-off animation runs on into the one in the air.)
        self.set_st(S::DtpLoop);
        self.h.weapon_time = 0;
        self.h.weapon_delay = 0;
    }

    fn dtp_out(&mut self) {
        self.set_st(S::DtpOut);
        self.h.weapon_time = self.f.dtp_out_time_ms;
        self.h.weapon_delay = 0;
        let a = if self.clip_empty() { anim::DIVE_OUT_EMPTY } else { anim::DIVE_OUT };
        self.anim_if_changed(a);
    }

    // --- prone crawl ------------------------------------------------------

    fn crawl_empty_offset(&self) -> u32 {
        if self.clip_empty() { anim::CRAWL_EMPTY_OFFSET } else { 0 }
    }

    /// The crawl state and animation for the move input: sideways unless
    /// forward is pushed more than half as hard (on no input, left).
    fn crawl_direction(&self) -> (S, u32) {
        let [f, r] = self.c.move_input.map(i32::from);
        let (st, a) = if 4 * f * f <= r * r {
            if r < 1 { (S::CrawlLeft, anim::CRAWL_LEFT) } else { (S::CrawlRight, anim::CRAWL_RIGHT) }
        } else if f < 1 {
            (S::CrawlBack, anim::CRAWL_BACK)
        } else {
            (S::CrawlForward, anim::CRAWL_FORWARD)
        };
        (st, a + self.crawl_empty_offset())
    }

    fn crawl_in(&mut self) {
        self.set_st(S::CrawlIn);
        self.h.weapon_time = self.f.crawl_in_time_ms;
        self.h.weapon_delay = 0;
        // (The viewmodel plays the crawl-in before the direction.)
        let (_, a) = self.crawl_direction();
        self.anim(a);
    }

    fn crawl_out(&mut self, fired: bool) {
        self.set_st(S::CrawlOut);
        self.h.weapon_time = if fired { self.f.crawl_out_fire_time_ms } else { self.f.crawl_out_time_ms };
        self.h.weapon_delay = 0;
        let a = anim::CRAWL_OUT + self.crawl_empty_offset();
        self.anim(a);
    }

    fn crawl_state(&mut self, delay_expired: bool) {
        if self.should_fire(delay_expired, true) && !shot_limit_reached(self.f, self.h.shot_count) {
            self.crawl_out(true);
            return;
        }
        let (st, a) = self.crawl_direction();
        if self.st() != st as i32 {
            self.set_st(st);
            self.h.weapon_time = 0;
            self.h.weapon_delay = 0;
            self.anim_if_changed(a);
        }
    }

    fn check_crawl_states(&mut self) {
        let moving = self.c.move_input != [0, 0];
        let st = self.st();
        if self.c.cmd_weapon == 0
            || st == S::Firing as i32
            || st == S::Rechambering as i32
            || st == S::RechamberingBolted as i32
            || st == S::ContFireLoop as i32
            || is_melee_state(st)
            || is_reload_state(st)
            || is_raise_or_drop(st)
            || is_offhand_state(st)
            || in_range(st, S::DeployInit, S::Deploy22)
            || is_dtp_state(st)
            || in_range(st, S::SprintIn, S::SprintOut)
            || st == S::Detonating as i32
            || self.c.pm_flags & (PMF_DIVE | PMF_DIVE_SLIDE) != 0
        {
            return;
        }
        let limit = shot_limit_reached(self.f, self.h.shot_count);
        let fire = self.should_fire(false, true);
        let crawling = in_range(st, S::CrawlIn, S::CrawlLeft);
        if fire && !limit && crawling {
            self.crawl_out(true);
            return;
        }
        let prone = self.c.pm_flags & pm_flags::PRONE != 0;
        if !prone && in_range(st, S::CrawlForward, S::CrawlLeft) {
            self.crawl_out(false);
            return;
        }
        if fire && (!limit || in_range(st, S::CrawlIn, S::CrawlOut)) {
            return;
        }
        if prone && (moving || self.c.dive_pause) {
            if moving && !in_range(st, S::CrawlIn, S::CrawlOut) {
                self.crawl_in();
            }
            return;
        }
        if crawling {
            self.crawl_out(false);
        }
    }

    // --- the frame --------------------------------------------------------

    fn run(&mut self) {
        let st = self.st();
        if !is_raise_or_drop(st) {
            self.h.flags &= !flags::SHIELD_CHANGE;
        }
        if self.h.hand_index == 0 && self.check_grenade_timer() {
            return;
        }
        self.update_burst_latch();
        let delay_expired = self.update_timers();
        if !self.burst_in_progress() {
            if self.h.hand_index == 0 {
                self.check_for_sprint();
                self.check_dtp_lower();
                self.check_for_offhand();
                self.check_for_shield_deploy();
                self.check_for_change_weapon();
            }
            self.check_for_reload();
            self.check_for_melee(delay_expired);
            if self.h.hand_index == 0 {
                self.check_for_detonate();
                self.check_offhand_hold_cancel();
                self.check_crawl_states();
            }
        }
        if self.check_for_rechamber(delay_expired) {
            return;
        }
        if !delay_expired && (self.h.weapon_time != 0 || self.h.weapon_delay != 0) {
            return;
        }
        self.dispatch(delay_expired);
    }

    fn dispatch(&mut self, delay_expired: bool) {
        let st = self.st();
        let Some(state) = state_of(st) else { return };
        match state {
            S::Raising | S::RaisingAltswitch => self.finish_raise(),
            S::Dropping | S::DroppingQuick | S::DroppingAltswitch => self.finish_weapon_change(st),
            S::Reloading | S::ReloadingInterrupt => self.reload_loop_end(delay_expired),
            S::ReloadStart | S::ReloadStartBolt | S::ReloadStartInterrupt => self.reload_start_end(delay_expired),
            S::ReloadEnd => {
                self.set_st(S::Ready);
                self.anim(anim::IDLE);
            }
            S::MeleeInit => {
                self.set_st(S::MeleeFire);
                self.ev.push(WeaponTickEvent::MeleeFired);
            }
            S::MeleeFire => self.melee_end(),
            S::MeleeEnd
            | S::OffhandDone
            | S::Deploy22
            | S::SprintOut
            | S::ContFireOut
            | S::DtpOut
            | S::Special34
            | S::Lockout
            | S::CrawlOut => self.reset_to_ready(),
            S::OffhandInit => self.offhand_init(),
            S::OffhandPrepare => self.offhand_prepare(),
            S::OffhandHold => self.offhand_hold(),
            S::OffhandStart | S::Detonating => self.offhand_start(delay_expired),
            S::Offhand => self.offhand_fire(),
            S::OffhandEnd => self.offhand_end(),
            S::OffhandFinish => self.offhand_finish(),
            S::DeployInit => {
                self.set_st(S::Deploying);
                self.h.weapon_time = 0;
                self.h.weapon_delay = 0;
            }
            S::DeployPrepare => {
                self.ev.push(WeaponTickEvent::ShieldDeployed { planted: true });
                self.set_st(S::Deploy22);
            }
            S::Deploying => self.shield_deploy(),
            S::SprintIn => self.sprint_loop(),
            S::SprintLoop | S::Mounting => {}
            S::NightVisionWear | S::NightVisionRemove => {
                if self.h.weapon_time == 0 {
                    self.set_st(S::Ready);
                    self.anim(anim::IDLE);
                }
            }
            S::Mounted => {}
            S::Unmounting => self.reset_to_ready(),
            S::DtpIn => self.dtp_loop(),
            S::DtpLoop => {
                if self.c.pm_flags & PMF_DIVE_SLIDE != 0 || self.c.pm_flags & (PMF_DIVE | PMF_DIVE_SLIDE) == 0 {
                    self.dtp_out();
                }
            }
            S::CrawlIn => {
                self.crawl_state(delay_expired);
            }
            S::CrawlForward | S::CrawlBack | S::CrawlRight | S::CrawlLeft => {
                if self.should_fire(delay_expired, true) && !shot_limit_reached(self.f, self.h.shot_count) {
                    self.crawl_out(true);
                } else {
                    self.crawl_state(delay_expired);
                }
            }
            _ => {
                if self.h.weapon != 0 && self.should_fire(delay_expired, false) {
                    if delay_expired && self.holding_grenade_throw() {
                        self.h.weapon_delay = 1;
                        return;
                    }
                    if st != S::Mounted as i32 {
                        self.fire_weapon(delay_expired);
                    }
                }
            }
        }
    }
}

/// One hand's frame of Black Ops II's weapon state machine.
pub fn pm_weapon(hand: &mut WeaponHandState, facts: &WeaponCombatFacts, cmd: &mut WeaponCmd) -> Events {
    let none = WeaponCombatFacts::none();
    let f = if hand.weapon == 0 { &none } else { facts };
    if hand.burst_latch {
        hand.flags |= flags::BURST_LATCH;
    } else {
        hand.flags &= !flags::BURST_LATCH;
    }
    let mut pm = Pm { h: hand, f, c: cmd, ev: Events::default() };
    pm.run();
    if pm.h.hand_index == 0 {
        if pm.h.flags & flags::OFFHAND != 0 {
            pm.c.weap_flags |= weap_flags::OFFHAND_VIEW;
        } else {
            pm.c.weap_flags &= !weap_flags::OFFHAND_VIEW;
        }
    }
    pm.h.burst_latch = pm.h.flags & flags::BURST_LATCH != 0;
    pm.ev
}

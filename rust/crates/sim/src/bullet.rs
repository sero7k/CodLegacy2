use weapon_iw4::WeaponCombatFacts;

pub fn bullet_damage_at_distance(facts: &WeaponCombatFacts, dist: f32) -> i32 {
    if let Some(d) = damage_over_points(&facts.damage_points, dist) {
        return d.max(0);
    }
    let base = facts.damage;
    let min = if facts.min_damage > 0 {
        facts.min_damage
    } else {
        base
    };
    let mut damage = base;
    let range = facts.min_damage_range - facts.max_damage_range;
    if base != min && range != 0.0 {
        if facts.max_damage_range <= dist {
            if facts.min_damage_range <= dist {
                damage = min;
            } else {
                let lerp = (dist - facts.max_damage_range) / range;
                let lerp = lerp.clamp(0.0, 1.0);
                damage = ((base as f32) * (1.0 - lerp) + (min as f32) * lerp) as i32;
            }
        }
    }
    damage.max(0)
}

/// Black Ops II's damage at a distance from its (range, damage) points
/// (points with no range are unused): the first point's damage up to its
/// range, straight between neighbouring points, the last's beyond. None
/// when the gun has no points (the other games' guns).
pub fn damage_over_points(points: &[[f32; 2]; 6], dist: f32) -> Option<i32> {
    let mut used = points.iter().filter(|p| p[0] > 0.0);
    let first = used.next()?;
    if dist <= first[0] {
        return Some(first[1] as i32);
    }
    let mut prev = *first;
    for p in used {
        if dist <= p[0] {
            let span = p[0] - prev[0];
            let t = if span > 0.0 { (dist - prev[0]) / span } else { 1.0 };
            return Some((prev[1] + (p[1] - prev[1]) * t) as i32);
        }
        prev = *p;
    }
    Some(prev[1] as i32)
}

pub fn angles_to_forward(angles: [f32; 3]) -> [f32; 3] {
    let yaw = angles[1].to_radians();
    let pitch = angles[0].to_radians();
    let cy = yaw.cos();
    let sy = yaw.sin();
    let sp = pitch.sin();
    let cp = pitch.cos();
    [cp * cy, cp * sy, -sp]
}

/// bo2mp: the damage a sniper hit that kills outright does, before the wall
/// it went through takes its share (never less than the victim's health).
pub const ONE_SHOT_SNIPER_DAMAGE: f32 = 1000.0;

/// Sniper rifles by their base names (Black Ops II files them as rifles,
/// so the class alone does not tell): every game's in the catalogue.
const SNIPER_BASES: &[&str] = &[
    "dsr50", "ballista", "svu", "as50", "cheytac", "barrett", "wa2000", "m21", "l96a1", "msr", "rsass",
    "dragunov", "psg1", "l118a", "m40a3", "m40", "r700", "remington700", "mosin", "kar98k", "springfield",
    "type99rifle", "ptrs41", "usr", "vks", "intervention", "m82", "aw50", "svd", "lynx", "l115",
    "xpr50", "mk14", "g28", "imbel", "m990", "mors", "thor", "gm6", "sniper",
];

fn base_name(script_name: &str) -> String {
    let name = script_name.rsplit(':').next().unwrap_or(script_name).to_ascii_lowercase();
    let name = name.split('+').next().unwrap_or("").to_string();
    let mut base = name.as_str();
    for p in ["iw5_", "iw6_", "iw7_", "iw9_", "jup_", "s1_", "h1_", "t5_", "t4_", "t7_"] {
        base = base.strip_prefix(p).unwrap_or(base);
    }
    base.to_string()
}

/// A sniper rifle: one of the names above, or a gun that looks down a scope
/// overlay (any sniper sight), not a pistol, shotgun or launcher.
pub fn is_sniper(facts: &WeaponCombatFacts, script_name: &str) -> bool {
    let base = base_name(script_name);
    let named = SNIPER_BASES.iter().any(|b| {
        base.strip_prefix(b).is_some_and(|rest| rest.is_empty() || rest.starts_with('_') || rest.starts_with("mp"))
    });
    // IW4 numbering after the remap: 0 rifle, 1 sniper.
    named || facts.weap_class == 1 || (facts.overlay_reticle != 0 && facts.weap_class == 0)
}

/// The owner's sniper rule (2026-10-26), per weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SniperRule {
    /// Bolt / pump / manual action: any head, neck or torso hit kills at
    /// any range; limb hits do the gun's normal (limb-scaled) damage.
    Always1Shot,
    /// Semi-automatic or burst: a head (helmet, head, neck) or chest
    /// (`torso_upper`) hit kills; stomach and limbs do normal damage.
    ChestOrHead1Shot,
    /// The SVU (BO2's SVU-AS, any game's SVU): a headshot kills, any other
    /// hit takes exactly half of the victim's health (two to kill).
    TwoShotUnlessHead,
}

impl SniperRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Always1Shot => "always1shot",
            Self::ChestOrHead1Shot => "chestorhead1shot",
            Self::TwoShotUnlessHead => "twoshotunlesshead",
        }
    }
}

/// The SVU by name: the only weapon the rule names (its fire mode alone
/// would make it a semi-automatic).
fn is_svu(script_name: &str) -> bool {
    let base = base_name(script_name);
    base.split('_').next() == Some("svu")
}

/// The rule a gun follows, from its class and fire mode in the weapon
/// data; None for a gun that is not a sniper rifle. Manual action is the
/// def's bolt flag, or (for guns whose files carry no bolt flag) a single
/// shot fire mode with at least half a second between shots.
pub fn sniper_damage_rule(facts: &WeaponCombatFacts, script_name: &str) -> Option<SniperRule> {
    if !is_sniper(facts, script_name) {
        return None;
    }
    if is_svu(script_name) {
        return Some(SniperRule::TwoShotUnlessHead);
    }
    let single = facts.fire_type == weapon_iw4::FireType::SingleShot as i32;
    let manual = facts.bolt_action || (single && facts.fire_time_ms >= 500);
    Some(if manual { SniperRule::Always1Shot } else { SniperRule::ChestOrHead1Shot })
}

const HL_HELMET: u8 = 1;
const HL_HEAD: u8 = 2;
const HL_NECK: u8 = 3;
const HL_TORSO_UPPER: u8 = 4;
const HL_TORSO_MID: u8 = 5;
const HL_TORSO_LOWER: u8 = 6;

/// What a sniper hit does under its rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SniperHit {
    /// The gun's normal damage, scaled by the hit location as usual.
    Normal,
    /// Kills outright.
    Kill,
    /// Half of the victim's max health, rounded up (two hits kill).
    Half,
}

pub fn sniper_hit(rule: SniperRule, hitloc: u8) -> SniperHit {
    // A hit the riot shield stopped is the shield's business.
    if usize::from(hitloc) == weapon_iw4::HITLOC_RIOTSHIELD {
        return SniperHit::Normal;
    }
    let head = matches!(hitloc, HL_HELMET | HL_HEAD);
    match rule {
        SniperRule::Always1Shot => {
            if matches!(hitloc, HL_HELMET | HL_HEAD | HL_NECK | HL_TORSO_UPPER | HL_TORSO_MID | HL_TORSO_LOWER) {
                SniperHit::Kill
            } else {
                SniperHit::Normal
            }
        }
        SniperRule::ChestOrHead1Shot => {
            if head || matches!(hitloc, HL_NECK | HL_TORSO_UPPER) {
                SniperHit::Kill
            } else {
                SniperHit::Normal
            }
        }
        SniperRule::TwoShotUnlessHead => {
            if head {
                SniperHit::Kill
            } else {
                SniperHit::Half
            }
        }
    }
}

/// The final damage of a sniper hit (hit location already counted), or
/// None when the hit does the gun's normal damage. `pen_mult` is the share
/// a wall left the bullet; a kill never drops below the victim's max health.
pub fn sniper_final_damage(rule: SniperRule, hitloc: u8, pen_mult: f32, max_health: i32) -> Option<i32> {
    match sniper_hit(rule, hitloc) {
        SniperHit::Normal => None,
        SniperHit::Kill => Some(sniper_kill_damage(pen_mult, max_health)),
        SniperHit::Half => Some(((max_health.max(1) + 1) / 2).max(1)),
    }
}

/// A hit that kills outright: the one-shot damage after the wall's share,
/// never less than the victim's max health.
pub fn sniper_kill_damage(pen_mult: f32, max_health: i32) -> i32 {
    ((ONE_SHOT_SNIPER_DAMAGE * pen_mult) as i32).max(max_health.max(1))
}

/// `snipers: name=rule,...` over a weapon table (the match-start log line).
pub fn sniper_roster<'a>(rows: impl IntoIterator<Item = (&'a str, &'a WeaponCombatFacts)>) -> String {
    let mut seen = std::collections::BTreeMap::new();
    for (name, facts) in rows {
        if name.is_empty() || !facts.is_usable() {
            continue;
        }
        if let Some(rule) = sniper_damage_rule(facts, name) {
            let base = name.split('+').next().unwrap_or(name);
            seen.entry(base.to_string()).or_insert(rule.label());
        }
    }
    let list: Vec<String> = seen.into_iter().map(|(n, r)| format!("{n}={r}")).collect();
    format!("snipers: {}", list.join(","))
}

#[cfg(test)]
mod tests {
    use super::damage_over_points;
    use super::*;
    use weapon_iw4::WeaponCombatFacts;

    fn gun(bolt: bool, fire_ms: i32, overlay: i32) -> WeaponCombatFacts {
        let mut f = WeaponCombatFacts::none();
        f.bolt_action = bolt;
        f.fire_time_ms = fire_ms;
        f.overlay_reticle = overlay;
        f
    }

    fn single(bolt: bool, fire_ms: i32) -> WeaponCombatFacts {
        let mut f = gun(bolt, fire_ms, 1);
        f.fire_type = weapon_iw4::FireType::SingleShot as i32;
        f
    }

    #[test]
    fn snipers_are_classified_by_fire_mode() {
        assert_eq!(sniper_damage_rule(&single(true, 1100), "dsr50_mp"), Some(SniperRule::Always1Shot));
        assert_eq!(sniper_damage_rule(&gun(true, 900, 0), "ballista_mp+acog"), Some(SniperRule::Always1Shot));
        assert_eq!(sniper_damage_rule(&single(true, 1200), "iw4:cheytac_mp"), Some(SniperRule::Always1Shot));
        assert_eq!(sniper_damage_rule(&single(false, 200), "as50_mp"), Some(SniperRule::ChestOrHead1Shot));
        assert_eq!(sniper_damage_rule(&single(false, 150), "iw4:barrett_mp"), Some(SniperRule::ChestOrHead1Shot));
        let mut burst = gun(false, 90, 1);
        burst.fire_type = weapon_iw4::FireType::BurstFire3 as i32;
        assert_eq!(sniper_damage_rule(&burst, "iw6_imbel_mp"), Some(SniperRule::ChestOrHead1Shot));
        assert_eq!(sniper_damage_rule(&single(false, 250), "svu_mp+ir"), Some(SniperRule::TwoShotUnlessHead));
        assert_eq!(sniper_damage_rule(&single(false, 250), "iw6:iw6_svu_mp"), Some(SniperRule::TwoShotUnlessHead));
        assert_eq!(sniper_damage_rule(&gun(true, 1000, 0), "remington870_mp"), None);
        assert!(!is_sniper(&gun(false, 100, 0), "an94_mp"));
    }

    #[test]
    fn bolt_torso_kills_at_long_range_limbs_do_normal_damage() {
        let r = SniperRule::Always1Shot;
        // Long range: the gun's own falloff is irrelevant to a torso hit.
        for loc in [1u8, 2, 3, 4, 5, 6] {
            assert!(sniper_final_damage(r, loc, 1.0, 100).unwrap() >= 100);
        }
        // Through a thin wall too.
        assert!(sniper_final_damage(r, 6, 0.05, 100).unwrap() >= 100);
        for loc in 7u8..=18 {
            assert_eq!(sniper_final_damage(r, loc, 1.0, 100), None);
        }
    }

    #[test]
    fn semi_kills_with_chest_or_head_only() {
        let r = SniperRule::ChestOrHead1Shot;
        for loc in [1u8, 2, 3, 4] {
            assert!(sniper_final_damage(r, loc, 1.0, 100).unwrap() >= 100);
        }
        for loc in [5u8, 6, 7, 13, 17] {
            assert_eq!(sniper_final_damage(r, loc, 1.0, 100), None);
        }
    }

    #[test]
    fn svu_two_shots_body_one_shots_head() {
        let r = SniperRule::TwoShotUnlessHead;
        for max in [100, 150, 101, 30] {
            for loc in [3u8, 4, 5, 6, 8, 15] {
                let d = sniper_final_damage(r, loc, 1.0, max).unwrap();
                assert!(d < max, "one body hit must not kill");
                assert!(d * 2 >= max, "two body hits must kill");
            }
            assert!(sniper_final_damage(r, 2, 1.0, max).unwrap() >= max);
            assert!(sniper_final_damage(r, 1, 1.0, max).unwrap() >= max);
            assert_eq!(sniper_final_damage(r, 20, 1.0, max), None);
        }
    }

    #[test]
    fn roster_lists_each_sniper_once() {
        let dsr = single(true, 1100);
        let svu = single(false, 250);
        let ar = gun(false, 100, 0);
        let line = sniper_roster([("dsr50_mp", &dsr), ("dsr50_mp+acog", &dsr), ("svu_mp", &svu), ("an94_mp", &ar)]);
        assert_eq!(line, "snipers: dsr50_mp=always1shot,svu_mp=twoshotunlesshead");
    }

    /// The MP7's points as Black Ops II's common_mp has them.
    const MP7: [[f32; 2]; 6] =
        [[800.0, 33.0], [801.0, 22.0], [1100.0, 22.0], [0.0, 0.0], [0.0, 0.0], [1100.0, 18.0]];
    /// The SWAT-556's (burst): five tiers.
    const SARITCH: [[f32; 2]; 6] =
        [[1000.0, 50.0], [1001.0, 50.0], [1500.0, 50.0], [1501.0, 49.0], [3000.0, 49.0], [3001.0, 40.0]];

    #[test]
    fn black_ops_ii_damage_steps_between_its_points() {
        assert_eq!(damage_over_points(&MP7, 10.0), Some(33));
        assert_eq!(damage_over_points(&MP7, 800.0), Some(33));
        assert_eq!(damage_over_points(&MP7, 900.0), Some(22));
        assert_eq!(damage_over_points(&MP7, 1099.0), Some(22));
        assert_eq!(damage_over_points(&MP7, 5000.0), Some(18));
        assert_eq!(damage_over_points(&SARITCH, 1400.0), Some(50));
        assert_eq!(damage_over_points(&SARITCH, 2000.0), Some(49));
        assert_eq!(damage_over_points(&SARITCH, 4000.0), Some(40));
        assert_eq!(damage_over_points(&[[0.0; 2]; 6], 100.0), None);
    }
}

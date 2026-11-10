//! bo2mp: a foreign map's ambient effects (Ghosts' createfx: fire, smoke,
//! steam, dust, sparks, ripples) played as Black Ops III's own where it has
//! a matching effect (`BO2MP_BO3_VFX_SUBST=0` turns the pass off).
//!
//! No list of Black Ops III effects is written down: a rule says what a
//! foreign effect's path is about (its words: `fire`, `smoke`, `steam`...,
//! and a size word) and which words the Black Ops III effect's name must
//! hold; the pass picks, among the effect names the install's shared zones
//! really define (`T7Map::effect_names`), the best one (the most specific
//! rule that finds a name, the shortest name, then the alphabetical first).
//! A foreign effect no rule names, or whose rule finds no Black Ops III
//! effect, keeps playing as the game's own (waterfalls, god rays, a map's
//! unique effects). The load report counts both and lists examples.

use std::collections::BTreeMap;

pub(super) fn enabled() -> bool {
    super::bo3_look::game_on("BO2MP_BO3_VFX_SUBST", true)
}

/// A rule: when every word in `ghosts` starts a word of the foreign effect's
/// path and none of `not` does, the Black Ops III effect is the first of
/// `bo3` (alternatives, most wanted first; each a list of substrings the
/// name must all hold) that some defined effect satisfies.
struct Rule {
    label: &'static str,
    ghosts: &'static [&'static str],
    not: &'static [&'static str],
    bo3: &'static [&'static [&'static str]],
}

/// Words that mark a Black Ops III effect as not an ambient one, whatever
/// else it says (a gun's, a body's, a vehicle's, a hit's).
const NOT_AMBIENT: &[&str] = &["impact", "muzzle", "shell", "blood", "weapon", "wpn", "player", "veh", "vehicle", "gib", "death", "hit", "tracer", "_view", "fp_", "zm_", "zombie", "ai_", "human", "robot", "wasp", "amws", "raps", "plyr", "killstreak", "destruct", "dest_", "dmg", "trail", "hero", "grenade", "sentinel", "_t6", "ability", "bounce", "footstep", "treadfx", "vtol", "heli", "mship", "care_package", "betty", "trophy", "agr_"];

/// Words in a foreign path that make it the game's own whatever else it says.
const OWN: &[&str] = &["waterfall", "godray", "lightshaft", "ray", "rays", "lensflare", "flare", "sunflare", "birds", "bird", "insects", "fish", "leaves", "leaf", "snowfall", "rain", "blizzard", "sandstorm", "wind"];

const RULES: &[Rule] = &[
    // (Filled from the effect names the install defines; see `choose`.)
    Rule { label: "ember", ghosts: &["ember"], not: &[], bo3: &[&["fx_", "ember"], &["ember"]] },
    Rule { label: "spark", ghosts: &["spark"], not: &["ember"], bo3: &[&["fx_", "spark", "ambient"], &["fx_", "sparks", "loop"], &["fx_", "spark"]] },
    Rule { label: "fire (small)", ghosts: &["fire", "small"], not: &[], bo3: &[&["fx_fire", "sm"], &["fire", "sm"]] },
    Rule { label: "fire (large)", ghosts: &["fire", "large"], not: &[], bo3: &[&["fx_fire", "lg"], &["fire", "lg"]] },
    Rule { label: "fire", ghosts: &["fire"], not: &["smoke"], bo3: &[&["fx_fire", "md"], &["fx_fire"], &["fire"]] },
    Rule { label: "steam", ghosts: &["steam"], not: &[], bo3: &[&["fx_steam"], &["steam"]] },
    Rule { label: "smoke", ghosts: &["smoke"], not: &["fire", "grenade"], bo3: &[&["fx_smk", "ambient"], &["fx_smoke", "ambient"], &["fx_smk"], &["fx_smoke"]] },
    Rule { label: "dust", ghosts: &["dust"], not: &[], bo3: &[&["fx_dust", "ambient"], &["fx_dust", "motes"], &["fx_dust"]] },
    Rule { label: "ripple", ghosts: &["ripple"], not: &[], bo3: &[&["water", "ripple"]] },
    Rule { label: "drip", ghosts: &["drip"], not: &[], bo3: &[&["fx_water", "drip"], &["drip"]] },
];

fn tokens(path: &str) -> Vec<String> {
    path.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// The rule a foreign effect's path names, if any (`None`: the game's own).
fn rule_for(path: &str) -> Option<&'static Rule> {
    let toks = tokens(path);
    if toks.iter().any(|t| OWN.contains(&t.as_str())) {
        return None;
    }
    let has = |w: &str| toks.iter().any(|t| t.starts_with(w));
    RULES.iter().find(|r| r.ghosts.iter().all(|w| has(w)) && !r.not.iter().any(|w| has(w)))
}

/// Whether a rule names this foreign path (before the install's effect names
/// are read).
pub(super) fn wants(path: &str) -> bool {
    rule_for(path).is_some()
}

/// The Black Ops III effect for a foreign path among `names` (`defined`),
/// and the rule's label.
pub(super) fn choose(path: &str, names: &[String]) -> Option<(String, &'static str)> {
    let rule = rule_for(path)?;
    let ok = |n: &&String| {
        let l = n.to_ascii_lowercase();
        !NOT_AMBIENT.iter().any(|w| l.contains(w))
    };
    for alt in rule.bo3 {
        let pick = names
            .iter()
            .filter(ok)
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                alt.iter().all(|w| l.contains(w))
            })
            .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        if let Some(p) = pick {
            return Some((p.clone(), rule.label));
        }
    }
    None
}

/// Which of its own axes a foreign effect rises along (Ghosts' effects are
/// authored either way: `docs/IW6.md`, "Placed effects' axis").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rise {
    /// Along the local forward axis (placed with forward up).
    Forward,
    /// Along the local up axis (placed upright).
    Up,
    /// Hardly moves, or falls: the placement's angles are kept.
    Still,
}

/// The axis a Ghosts effect rises along, from its elements' local velocity
/// samples (each 96 bytes: the local velocity x, y, z and its range, per
/// sample; the sample count minus one at record byte 183): the axis (forward
/// x or up z) with the larger mean upward speed (units per ms).
pub(super) fn rise_of(fx: &fastfile_iw6::fxview::OwnedFx) -> Rise {
    let f = |b: &[u8], at: usize| b.get(at..at + 4).map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let (mut x, mut z, mut n) = (0.0f32, 0.0f32, 0usize);
    for e in &fx.elems {
        let samples = e.record.get(183).map_or(0, |c| usize::from(*c) + 1);
        for s in 0..samples {
            let b = s * 96;
            // (The sample's middle: its value plus half its range.)
            x += f(&e.vel_samples, b) + 0.5 * f(&e.vel_samples, b + 12);
            z += f(&e.vel_samples, b + 8) + 0.5 * f(&e.vel_samples, b + 20);
            n += 1;
        }
    }
    if n == 0 {
        return Rise::Still;
    }
    let (x, z) = (x / n as f32, z / n as f32);
    // (A rise above 0.002 units a millisecond, about 2 a second: smoke and
    // fire; rain and drips fall and keep their angles.)
    match (x > 0.002, z > 0.002) {
        (true, false) => Rise::Forward,
        (false, true) => Rise::Up,
        (true, true) => if x >= z { Rise::Forward } else { Rise::Up },
        (false, false) => Rise::Still,
    }
}

/// Quake angles (pitch, yaw, roll degrees) to their forward and up vectors.
fn angle_vectors(a: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let [p, y, r] = a.map(f32::to_radians);
    let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
    ([cp * cy, cp * sy, -sp], [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp])
}

/// The angles a Black Ops III effect is placed at to rise where the foreign
/// one did. Black Ops III's ambient effects rise along their forward axis
/// (INFERRED from Black Ops II's createfx placing them at pitch 270; not
/// checked against a Black Ops III effect's own velocity samples): a foreign
/// effect that rose along its local forward keeps its angles, one that rose
/// along its local up gets its up vector as forward, and a still one keeps them.
pub(super) fn bo3_angles(angles: [f32; 3], rise: Rise) -> [f32; 3] {
    match rise {
        Rise::Forward | Rise::Still => angles,
        Rise::Up => {
            let (_, up) = angle_vectors(angles);
            let pitch = -up[2].clamp(-1.0, 1.0).asin().to_degrees();
            let yaw = if up[0].abs() + up[1].abs() < 1e-5 { angles[1] } else { up[1].atan2(up[0]).to_degrees() };
            [pitch.rem_euclid(360.0), yaw.rem_euclid(360.0), 0.0]
        }
    }
}

/// What a map's pass did, for its load report.
#[derive(Debug, Default)]
pub(super) struct Tally {
    /// Per foreign effect kept (no rule, or no Black Ops III effect).
    pub kept: BTreeMap<String, &'static str>,
    /// Per foreign effect matched: the Black Ops III effect and the rule.
    pub matched: BTreeMap<String, (String, &'static str)>,
    /// Placements that play a Black Ops III effect, and the game's own.
    pub placed_bo3: usize,
    pub placed_own: usize,
}

impl Tally {
    pub(super) fn line(&self, tag: &str, source: &str) -> String {
        let mut by_rule = BTreeMap::<&str, usize>::new();
        for (_, r) in self.matched.values() {
            *by_rule.entry(r).or_default() += 1;
        }
        let ex = |n: usize| self.matched.iter().take(n).map(|(g, (b, r))| format!("{g} -> {b} [{r}]")).collect::<Vec<_>>().join("; ");
        let kept: Vec<String> = self.kept.iter().take(6).map(|(g, why)| format!("{g} ({why})")).collect();
        format!(
            "{tag} vfx substitution: {} of {} effects played as Black Ops III's {by_rule:?} ({} placements; {} placements keep the game's own, {} effects: e.g. {kept:?}); {source}; e.g. {}",
            self.matched.len(),
            self.matched.len() + self.kept.len(),
            self.placed_bo3,
            self.placed_own,
            self.kept.len(),
            ex(5)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_rule_finds_the_shortest_matching_ambient_effect() {
        let n = names(&["fire/fx_fire_md_loop", "fire/fx_fire_md", "fire/fx_fire_sm", "weapon/fx_fire_impact_md", "smoke/fx_smk_ambient_lg"]);
        assert_eq!(choose("fx/fire/fire_medium", &n).map(|c| c.0), Some("fire/fx_fire_md".to_owned()));
        assert_eq!(choose("fx/fire/fire_small_a", &n).map(|c| c.0), Some("fire/fx_fire_sm".to_owned()));
        assert_eq!(choose("fx/smoke/smoke_stack", &n).map(|c| c.0), Some("smoke/fx_smk_ambient_lg".to_owned()));
    }

    #[test]
    fn the_games_own_and_unmatched_stay() {
        let n = names(&["fire/fx_fire_md"]);
        assert_eq!(choose("fx/water/waterfall_large", &n), None, "a waterfall is the game's own");
        assert_eq!(choose("fx/misc/godray_a", &n), None);
        assert_eq!(choose("fx/props/unique_machine", &n), None, "no rule names it");
        assert_eq!(choose("fx/steam/steam_vent", &n), None, "a rule, but the install has no steam effect");
    }

    #[test]
    fn effects_of_guns_and_hits_are_never_ambient() {
        let n = names(&["weapon/fx_fire_muzzle_md", "impacts/fx_fire_hit"]);
        assert_eq!(choose("fx/fire/fire_medium", &n), None);
    }

    fn fx_rising(local: [f32; 3]) -> fastfile_iw6::fxview::OwnedFx {
        let mut record = vec![0u8; 200];
        record[183] = 0;
        let mut vel = vec![0u8; 96];
        for (k, v) in local.iter().enumerate() {
            vel[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        fastfile_iw6::fxview::OwnedFx {
            name: "t".into(),
            header: Vec::new(),
            elems: vec![fastfile_iw6::fxview::OwnedElem { record, vel_samples: vel, ..Default::default() }],
        }
    }

    #[test]
    fn the_rise_axis_is_read_from_the_velocity() {
        assert_eq!(rise_of(&fx_rising([0.03, 0.0, 0.0])), Rise::Forward);
        assert_eq!(rise_of(&fx_rising([0.0, 0.0, 0.0147])), Rise::Up);
        assert_eq!(rise_of(&fx_rising([0.0, 0.0, -0.02])), Rise::Still, "rain falls");
        assert_eq!(rise_of(&fx_rising([0.0, 0.0, 0.0])), Rise::Still);
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 0.01 || ((x - y).abs() - 360.0).abs() < 0.01)
    }

    #[test]
    fn a_black_ops_iii_effect_is_placed_to_rise_where_the_games_did() {
        // Rose along forward: angles kept.
        assert_eq!(bo3_angles([283.0, 40.0, 0.0], Rise::Forward), [283.0, 40.0, 0.0]);
        // Rose along an upright effect's up: forward is made world up (pitch 270).
        assert!(near(bo3_angles([0.0, 90.0, 0.0], Rise::Up), [270.0, 90.0, 0.0]), "{:?}", bo3_angles([0.0, 90.0, 0.0], Rise::Up));
        // The new forward is the old up, whatever the angles.
        for a in [[10.0f32, 20.0, 30.0], [350.0, 200.0, 259.0], [30.0, 0.0, 0.0]] {
            let (_, up) = angle_vectors(a);
            let (f, _) = angle_vectors(bo3_angles(a, Rise::Up));
            assert!((0..3).all(|k| (up[k] - f[k]).abs() < 1e-3), "{a:?}: up {up:?} forward {f:?}");
        }
    }

    /// Reads the real Black Ops III effect names and runs sample Ghosts paths
    /// through the rules (needs the install: `CODLEGACY_GAMES=<Black Ops III
    /// folder> cargo test -p assets --lib bo3_vfx_exercise -- --ignored
    /// --nocapture`).
    #[test]
    #[ignore]
    fn bo3_vfx_exercise() {
        let dir = super::super::t7::bo3_zone_dir().expect("a Black Ops III install");
        let map = fastfile_t7::map::T7Map::shared_only(&dir).expect("core_common");
        let names = map.effect_names();
        println!("{} Black Ops III effects", names.len());
        let words = ["fire", "flame", "smoke", "smk", "steam", "dust", "ember", "spark", "ripple", "drip", "mist", "fog", "water", "ambient", "amb_", "loop"];
        for w in words {
            let hits: Vec<&String> = names.iter().filter(|n| n.to_ascii_lowercase().contains(w)).collect();
            println!("[{w}] {} names, first: {:?}", hits.len(), hits.iter().take(25).collect::<Vec<_>>());
        }
        for g in ["fx/fire/fire_medium", "fx/fire/fire_small_a", "fx/fire/fire_large", "fx/smoke/smoke_stack", "fx/smoke/smoke_thin", "fx/steam/steam_vent", "fx/dust/dust_falling", "fx/misc/embers_a", "fx/sparks/sparks_wire", "fx/water/ripple_a", "fx/water/drip_a", "fx/misc/godray_a", "fx/water/waterfall"] {
            println!("{g} -> {:?}", choose(g, &names));
        }
    }

    #[test]
    fn the_report_counts_both_kinds() {
        let mut t = Tally::default();
        t.matched.insert("fx/fire/a".into(), ("fire/fx_fire_md".into(), "fire"));
        t.kept.insert("fx/water/waterfall".into(), "no rule");
        t.placed_bo3 = 3;
        t.placed_own = 1;
        let l = t.line("iw6", "pool");
        assert!(l.contains("1 of 2 effects") && l.contains("3 placements") && l.contains("waterfall"), "{l}");
    }
}

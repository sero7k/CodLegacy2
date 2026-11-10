//! Look for Modern Warfare III's movement settings, to replace the
//! estimates in `movement_iw4::modern::MW3_2023`:
//! `jup_movement <install> [zone...]` (default: the zones below).
//!
//! For each zone: every plain-text name with a movement word in it
//! (sprint, slide, mantle, vault, swim, jump, fall, stance speeds...) with
//! its offset and the numbers stored just after it (as f32 and i32), and
//! where the hashes of the likely setting names are stored, with the
//! numbers around them. `JUP_WORDS=a,b` adds words, `JUP_NAMES=a,b` adds
//! names to hash.

use std::path::Path;

use fastfile_jup::hash;
use fastfile_jup::zone::HashIndex;

const ZONES: &[&str] = &["code_post_gfx", "common", "common_mp", "ingame_br", "ingame_base_mp", "ingame_shared_mp", "mp_jup_escape5"];

const WORDS: &[&str] = &[
    "sprint", "tactical", "slide", "mantle", "vault", "swim", "underwater", "jump", "fall", "crouch", "prone", "strafe", "backpedal", "movespeed", "speedscale", "dolphin", "dive", "ledge",
];

const NAMES: &[&str] = &[
    "player_sprintSpeedScale",
    "player_tacticalSprintSpeedScale",
    "player_sprintTime",
    "player_tacticalSprintTime",
    "player_tacticalSprintRechargeTime",
    "player_backSpeedScale",
    "player_strafeSpeedScale",
    "player_sprintStrafeSpeedScale",
    "player_crouchSpeedScale",
    "player_proneSpeedScale",
    "jump_height",
    "bg_fallDamageMinHeight",
    "bg_fallDamageMaxHeight",
    "slide_duration",
    "slide_speed",
    "slide_friction",
    "mantle_enable",
    "mantle_check_range",
    "mantle_max_height",
    "swim_speed",
    "swim_sprint_speed",
    "player_swimSpeed",
    "g_speed",
    "gamesettings/mp/movement",
    "gamesettings/br/movement",
];

fn list(var: &str, base: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = base.iter().map(|s| (*s).to_owned()).collect();
    if let Ok(v) = codlegacy_env::var(var) {
        out.extend(
            v.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
        );
    }
    out
}

/// The numbers stored right after `at`: four f32s and the same as i32s.
fn numbers(zone: &[u8], at: usize) -> String {
    let mut f = Vec::new();
    let mut i = Vec::new();
    for k in 0..4 {
        let o = at + 4 * k;
        let Some(b) = zone.get(o..o + 4) else { break };
        let b = [b[0], b[1], b[2], b[3]];
        let x = f32::from_le_bytes(b);
        f.push(if x.is_finite() && x.abs() < 1.0e7 { format!("{x}") } else { "-".to_owned() });
        i.push(i32::from_le_bytes(b).to_string());
    }
    format!("f32 [{}] i32 [{}]", f.join(" "), i.join(" "))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(install) = args.get(1) else {
        eprintln!("usage: jup_movement <install> [zone...]");
        std::process::exit(2);
    };
    let install = Path::new(install);
    let zones: Vec<String> = if args.len() > 2 { args[2..].to_vec() } else { ZONES.iter().map(|z| (*z).to_owned()).collect() };
    let words: Vec<String> = list("JUP_WORDS", WORDS).iter().map(|w| w.to_ascii_lowercase()).collect();
    let names = list("JUP_NAMES", NAMES);
    let oodle = fastfile_jup::oodle::load(install).expect("oodle");
    for zname in &zones {
        let (_, zone) = match fastfile_jup::read_zone(&install.join("zone").join(format!("{zname}.ff")), oodle.as_ref()) {
            Ok(z) => z,
            Err(e) => {
                println!("{zname}: {e}");
                continue;
            }
        };
        println!("== {zname} ({} bytes)", zone.len());
        // Plain text: runs of printable bytes, 4 or more long, ended by a 0.
        let mut start = 0usize;
        let mut shown = 0usize;
        for (at, &b) in zone.iter().enumerate() {
            if (0x20..0x7f).contains(&b) {
                continue;
            }
            let run = &zone[start..at];
            if b == 0 && run.len() >= 4 {
                let text = String::from_utf8_lossy(run);
                let lower = text.to_ascii_lowercase();
                if words.iter().any(|w| lower.contains(w.as_str())) && shown < 4000 {
                    println!("text @{start:#x} {text:?}  {}", numbers(&zone, at + 1));
                    shown += 1;
                }
            }
            start = at + 1;
        }
        let hashes: Vec<(u64, &String)> = names.iter().map(|n| (hash::name(n), n)).collect();
        let index = HashIndex::build(&zone, hashes.iter().map(|(h, _)| *h), 8);
        for (h, n) in &hashes {
            for &at in index.plain(*h) {
                println!("hash {n} ({h:#x}) @{at:#x}  {}", numbers(&zone, at + 8));
            }
        }
    }
}

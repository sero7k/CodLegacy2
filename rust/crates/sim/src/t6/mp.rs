//! bo2mp: Black Ops II multiplayer on the same runtime as Zombies.
//!
//! A multiplayer map (`mp_*`) runs its game type from `maps/mp/gametypes/`
//! (Zombies': `maps/mp/gametypes_zm/`). Its scripts call one script no PC
//! zone carries, `maps/mp/gametypes/_globallogic` (the match: start, clock,
//! limits, end, next round); the engine provides it as GSC source
//! ([`GLOBALLOGIC`], OUR CODE written from what the game's scripts call and
//! wait for), compiled with `gsc_t6::compile` and linked beside the zones'.

use bevy_ecs::prelude::World;
use gsc_t6::ScriptObject;

use super::Zm;

/// Our `maps/mp/gametypes/_globallogic`.
const GLOBALLOGIC: &str = include_str!("scripts/mp_globallogic.gsc");

static MOAB_KILLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(25);

/// bo2mp: the lobby's M.O.A.B. setting for the next match: the kills in
/// one life that earn it, 0 for none (native Black Ops II).
pub fn set_moab_kills(kills: u32) {
    MOAB_KILLS.store(kills, std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn moab_kills() -> u32 {
    MOAB_KILLS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Our Zombies bots (`bo2mp/zm_bots`, Custom Games' BOTS).
pub(super) const ZM_BOTS: &str = include_str!("scripts/zm_bots.gsc");

/// Our Mystery Box custom guns (`bo2mp/zm_guest_guns`, Custom Games'
/// CUSTOM WEAPONS).
pub(super) const ZM_GUEST_GUNS: &str = include_str!("scripts/zm_guest_guns.gsc");

/// Our Gun Game ladder fix (`bo2mp/gungame`, run before the game type starts).
pub(super) const GUNGAME: &str = include_str!("scripts/gungame.gsc");

/// Our optional M.O.A.B. (`bo2mp/moab`, off unless `scr_moab 1`).
const MOAB: &str = include_str!("scripts/moab.gsc");

/// bo2mp: Bot Warfare's bot behaviour (INeedGames', see NOTICE).
const BOT_WARFARE: &str = include_str!("scripts/bot_warfare.gsc");
pub(super) const BOT_WARFARE_NAME: &str = "bo2mp/bot_warfare";

fn bot_warfare_on() -> bool {
    std::env::var("BO2MP_BOTWARFARE").map_or(true, |v| v.trim() != "0")
}

/// The M.O.A.B.'s two visions, in the IW games' film form: a white-hot,
/// orange-lit flash, then a dim, desaturated, ash-brown aftermath. OUR
/// values, after the look of Modern Warfare 2's `mpnuke` and
/// `mpnuke_aftermath` (high contrast, half desaturated, warm light tint
/// for the flash; cold-dark, red-brown dark tint for the aftermath).
pub(super) fn register_moab_visions() {
    let flash = asset_world::FilmVision {
        enable: true,
        contrast: 1.6,
        brightness: 0.03,
        desaturation: 0.45,
        light_tint: [1.9, 1.3, 0.7],
        medium_tint: [1.2, 1.05, 0.9],
        dark_tint: [0.75, 0.9, 1.05],
        ..Default::default()
    };
    // The KEM's last seconds (Ghosts): the light swells and burns warm,
    // then all white, then the burnt dark after.
    let glare = asset_world::FilmVision {
        enable: true,
        contrast: 1.25,
        brightness: 0.16,
        desaturation: 0.35,
        light_tint: [1.7, 1.42, 0.95],
        medium_tint: [1.35, 1.18, 0.86],
        dark_tint: [1.0, 0.9, 0.72],
        glow_enable: true,
        glow_radius: 8.0,
        glow_bloom_cutoff: 0.35,
        glow_bloom_desaturation: 0.3,
        glow_bloom_intensity: 2.2,
        ..Default::default()
    };
    let white = asset_world::FilmVision {
        enable: true,
        contrast: 0.25,
        brightness: 0.85,
        desaturation: 1.0,
        light_tint: [2.0, 2.0, 1.95],
        medium_tint: [2.0, 2.0, 1.95],
        dark_tint: [1.8, 1.8, 1.75],
        glow_enable: true,
        glow_radius: 10.0,
        glow_bloom_cutoff: 0.1,
        glow_bloom_intensity: 4.0,
        ..Default::default()
    };
    let aftermath = asset_world::FilmVision {
        enable: true,
        contrast: 1.4,
        brightness: -0.06,
        desaturation: 0.65,
        light_tint: [0.85, 0.7, 0.5],
        medium_tint: [0.62, 0.46, 0.32],
        dark_tint: [0.36, 0.22, 0.12],
        ..Default::default()
    };
    // (A script's vision switches at once: the swell and the burn-down are
    // steps the script walks through, `kem_glare_1`..`_8` from the plain
    // look to the glare, `kem_burn_1`..`_8` from the white to the dark.)
    let plain = asset_world::FilmVision { enable: true, ..Default::default() };
    let mut list = vec![
        ("mpnuke".to_owned(), flash),
        ("kem_glare".to_owned(), glare.clone()),
        ("kem_white".to_owned(), white.clone()),
        ("mpnuke_aftermath".to_owned(), aftermath.clone()),
    ];
    for k in 1..=8 {
        let t = k as f32 / 8.0;
        list.push((format!("kem_glare_{k}"), lerp_film(&plain, &glare, t * t)));
        list.push((format!("kem_burn_{k}"), lerp_film(&white, &aftermath, 1.0 - (1.0 - t) * (1.0 - t))));
    }
    asset_world::register_film_visions(list);
}

/// A film vision between two (`t` 0: `a`, 1: `b`).
fn lerp_film(a: &asset_world::FilmVision, b: &asset_world::FilmVision, t: f32) -> asset_world::FilmVision {
    let l = |x: f32, y: f32| x + (y - x) * t;
    let v = |x: [f32; 3], y: [f32; 3]| [l(x[0], y[0]), l(x[1], y[1]), l(x[2], y[2])];
    asset_world::FilmVision {
        enable: true,
        contrast: l(a.contrast, b.contrast),
        brightness: l(a.brightness, b.brightness),
        desaturation: l(a.desaturation, b.desaturation),
        desaturation_dark: l(a.desaturation_dark, b.desaturation_dark),
        invert: false,
        light_tint: v(a.light_tint, b.light_tint),
        medium_tint: v(a.medium_tint, b.medium_tint),
        dark_tint: v(a.dark_tint, b.dark_tint),
        glow_enable: a.glow_enable || b.glow_enable,
        glow_radius: l(a.glow_radius, b.glow_radius),
        glow_bloom_cutoff: l(a.glow_bloom_cutoff, b.glow_bloom_cutoff),
        glow_bloom_desaturation: l(a.glow_bloom_desaturation, b.glow_bloom_desaturation),
        glow_bloom_intensity: l(a.glow_bloom_intensity, b.glow_bloom_intensity),
    }
}

/// A multiplayer map, by its zone name.
pub(super) fn is_mp(map: &str) -> bool {
    map.starts_with("mp_")
}

/// The scripts the engine provides for a multiplayer map, compiled.
/// BO2MP_DEBUG_GSC=<file>: a script of ours (`bo2mp/debug`) whose `main`
/// runs on the level once the match has started (a test aid: print what
/// the game's scripts hold, `println(...)` lands in the log).
pub(super) fn engine_scripts() -> Result<Vec<ScriptObject>, String> {
    let mut out = vec![
        gsc_t6::compile("maps/mp/gametypes/_globallogic", GLOBALLOGIC)?,
        gsc_t6::compile("bo2mp/moab", MOAB)?,
        gsc_t6::compile("bo2mp/gungame", GUNGAME)?,
        gsc_t6::compile("bo2mp/resurgence", super::br::SCRIPT)?,
    ];
    register_moab_visions();
    // bo2mp: Bot Warfare's bot behaviour (every multiplayer match;
    // BO2MP_BOTWARFARE=0 leaves Black Ops II's bots as they are).
    if bot_warfare_on() {
        out.push(gsc_t6::compile(BOT_WARFARE_NAME, BOT_WARFARE)?);
    }
    // bo2mp: his trickshot script (Custom Games' TRICKSHOT MODE).
    super::trickshot::set_active(super::trickshot::enabled());
    if super::trickshot::active() {
        out.push(gsc_t6::compile(super::trickshot::NAME, super::trickshot::SCRIPT)?);
    }
    if let Ok(path) = std::env::var("BO2MP_DEBUG_GSC") {
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        out.push(gsc_t6::compile("bo2mp/debug", &src)?);
    }
    Ok(out)
}

/// Where the game type's scripts live.
pub(super) fn gametypes(world: &World) -> &'static str {
    if world.get_resource::<Zm>().is_some_and(|z| z.mp) {
        "maps/mp/gametypes"
    } else {
        "maps/mp/gametypes_zm"
    }
}

/// The engine's callbacks script (`codecallback_*`; Black Ops III's are
/// in `callbacks_shared`, under the same names).
pub(super) fn callbacks(world: &World) -> String {
    if world.get_resource::<Zm>().is_some_and(|z| z.t7.is_some()) {
        return super::t7::CALLBACKS.to_owned();
    }
    format!("{}/_callbacksetup", gametypes(world))
}

#[cfg(test)]
mod globallogic_tests {
    #[test]
    fn our_globallogic_compiles() {
        let scripts = super::engine_scripts().expect("mp_globallogic.gsc compiles");
        assert!(!scripts.is_empty());
    }
}

#[cfg(test)]
mod gungame_tests {
    use gsc_t6::{Program, Strings, Value, Vm, compile};

    /// The test's catalog: a gun's variant it lacks is its base gun (as
    /// `resolve_weapon` falls back); `zzz*` is in no catalog.
    fn catalog(vm: &mut Vm<()>, _: &mut (), _: &Value, a: &[Value]) -> Result<Value, String> {
        let name = vm.to_text(&a[0]);
        let base = name.split('+').next().unwrap_or("").to_owned();
        Ok(vm.string(if base.starts_with("zzz") { "" } else { &base }))
    }

    /// Run a ladder (`,` between a level's names, `;` between levels)
    /// through `fixladder`; the ladder it leaves, in the same form.
    fn fixed(gametype: &str, ladder: &str) -> String {
        let adds: String = ladder
            .split(';')
            .map(|level| {
                let names: Vec<String> = level.split(',').enumerate().map(|(i, n)| format!("g.names[{i}] = \"{n}\";")).collect();
                format!("g = spawnstruct(); g.names = []; {} level.gunprogression[level.gunprogression.size] = g;", names.join(" "))
            })
            .collect();
        let harness = format!(
            "main() {{ level.gametype = \"{gametype}\"; level.gunprogression = []; {adds} \
             bo2mp/gungame::fixladder(); out = \"\"; \
             foreach ( g in level.gunprogression ) {{ foreach ( n in g.names ) out = out + n + \",\"; out = out + \";\"; }} \
             level.out = out; }}"
        );
        let objs = vec![
            compile("bo2mp/gungame", super::GUNGAME).expect("gungame.gsc compiles"),
            compile("test/gun", &harness).expect("harness compiles"),
        ];
        let mut strings = Strings::default();
        let program = Program::link(objs, &mut strings).expect("links");
        let mut vm = Vm::new(program, strings);
        gsc_t6::natives::bind_core(&mut vm);
        vm.bind("bo2mp_resolveweapon", false, catalog);
        let level = Value::Object(vm.level);
        vm.spawn_named(&mut (), "test/gun", "main", level, vec![]).expect("main exists");
        vm.run_frame(&mut ());
        let f = vm.intern("out");
        let out = vm.raw_field(vm.level, f);
        vm.to_text(&out)
    }

    /// Black Ops II names a level's gun with its attachments; a catalog that
    /// holds it under another name gets the ladder in that name, so a kill
    /// with the gun in hand is the level's gun (the promotion compares names).
    #[test]
    fn a_level_is_named_as_the_catalog_holds_its_gun() {
        assert_eq!(fixed("gun", "judge_mp+steadyaim;smaw_mp"), "judge_mp,;smaw_mp,;");
    }

    /// A level none of whose guns the catalog has is left out; the rest move up.
    #[test]
    fn a_level_with_no_gun_the_catalog_has_is_skipped() {
        assert_eq!(fixed("gun", "ksg_mp+fastads;zzz_mp+reflex;hk416_mp"), "ksg_mp,;hk416_mp,;");
    }

    /// A level's two names are kept once each; an unknown first name gives
    /// way to the second.
    #[test]
    fn a_levels_names_are_kept_once_each() {
        assert_eq!(fixed("gun", "dualoptic_mk48_mp+dualoptic,mk48_mp+dualoptic"), "dualoptic_mk48_mp,mk48_mp,;");
        assert_eq!(fixed("gun", "mk48_mp+dualoptic,mk48_mp+acog"), "mk48_mp,;");
        assert_eq!(fixed("gun", "zzz_mp,smaw_mp"), "smaw_mp,;");
    }

    /// A ladder the catalog has nothing of stays as the game type made it,
    /// and no other game type is touched.
    #[test]
    fn an_empty_result_or_another_game_type_leaves_the_ladder_alone() {
        assert_eq!(fixed("gun", "zzz_mp;zzz2_mp"), "zzz_mp,;zzz2_mp,;");
        assert_eq!(fixed("tdm", "judge_mp+steadyaim;zzz_mp"), "judge_mp+steadyaim,;zzz_mp,;");
    }
}

#[cfg(test)]
mod moab_tests {
    #[test]
    fn the_zombies_bots_compile() {
        let s = gsc_t6::compile("bo2mp/zm_bots", super::ZM_BOTS).expect("zm_bots.gsc compiles");
        for f in ["main", "bot_life", "bot_revive", "bot_fight", "bot_rebuild", "bot_spend", "bot_follow", "can_upgrade"] {
            assert!(s.functions.iter().any(|x| x.name == f), "{f}");
        }
    }

    #[test]
    fn the_zombies_custom_guns_compile() {
        let s = gsc_t6::compile("bo2mp/zm_guest_guns", super::ZM_GUEST_GUNS).expect("zm_guest_guns.gsc compiles");
        for f in ["main", "register_gun", "box_slots"] {
            assert!(s.functions.iter().any(|x| x.name == f), "{f}");
        }
        // (A gun is registered with its Pack-a-Punch upgrade.)
        assert!(s.functions.iter().any(|x| x.name == "register_gun" && x.params == 3));
    }

    #[test]
    fn the_moab_script_compiles_with_its_parts() {
        let s = gsc_t6::compile("bo2mp/moab", super::MOAB).expect("moab.gsc compiles");
        for f in ["main", "moab_watchstreak", "moab_launch", "moab_deaths", "moab_ticks"] {
            assert!(s.functions.iter().any(|x| x.name == f), "{f}");
        }
    }

    #[test]
    fn the_nuke_is_on_by_default_at_25_kills() {
        assert_eq!(super::moab_kills(), 25);
        assert!(super::MOAB.contains("getdvarint( \"scr_moab\" ) == 0"));
    }
}

#[cfg(test)]
mod trickshot_tests {
    /// His Plutonium trickshot script compiles as he wrote it (the ternary
    /// `?:` included) with its entry points and its menu.
    #[test]
    fn his_trickshot_script_compiles() {
        let s = gsc_t6::compile(super::super::trickshot::NAME, super::super::trickshot::SCRIPT)
            .expect("trickshot/main.gsc compiles");
        for f in ["init", "main", "onplayerconnect", "onplayerspawned", "add_menu", "add_option", "menuinit", "callback_playerdamagehook"] {
            assert!(s.functions.iter().any(|x| x.name == f), "{f}");
        }
        assert!(s.includes.iter().any(|i| i.contains("_hud_util")));
    }

    /// Brad's Black Ops III out-of-map spots: 21 maps' worth (96 spots), the
    /// script compiles with them, and every map the menu supports has a case
    /// and a function of its own.
    #[test]
    fn bo3_oom_spots_are_wired_for_every_map() {
        let src = super::super::trickshot::SCRIPT;
        gsc_t6::compile(super::super::trickshot::NAME, src).expect("compiles");
        // A Windows checkout (core.autocrlf) gives the script CRLF line ends;
        // the function splits below look for LF.
        let src = &src.replace("\r\n", "\n");
        let maps = [
            ("mp_spire", 4),
            ("mp_sector", 2),
            ("mp_biodome", 4),
            ("mp_apartments", 5),
            ("mp_veiled", 7),
            ("mp_havoc", 4),
            ("mp_ethiopia", 6),
            ("mp_metro", 4),
            ("mp_stronghold", 4),
            ("mp_nuketown_x", 5),
            ("mp_chinatown", 3),
            ("mp_redwood", 3),
            ("mp_infection", 4),
            ("mp_aerospace", 5),
            ("mp_rome", 4),
            ("mp_miniature", 5),
            ("mp_waterpark", 6),
            ("mp_conduit", 7),
            ("mp_rise", 6),
            ("mp_kung_fu", 4),
            ("mp_shrine", 4),
        ];
        assert!(src.contains("case \"mp_veiled_heyday\":"));
        let mut total = 0;
        for (map, n) in maps {
            assert!(src.contains(&format!("case \"{map}\":")), "{map}");
            let body = src.split(&format!("Bo3_{}()\n{{", &map[3..])).nth(1).unwrap_or_else(|| panic!("{map} function"));
            let body = body.split("\n}").next().unwrap();
            let spots = body.matches("Bo3Spot((").count();
            assert_eq!(spots, n, "{map}");
            total += spots;
        }
        assert_eq!(total, 96);
        // A spot's flag is on a real spawn, the OOM end 10 above the platform.
        assert!(src.contains("CreateFlag(inside, spot + (0, 0, 10), false, false)"));
        // BO3 maps are not in the BO2 case list, so they fall to the default.
        assert!(src.contains("default:\n            level thread Bo3Spots(mapname);"));
    }

    #[test]
    fn trickshot_mode_is_off_by_default() {
        if std::env::var_os("BO2MP_TRICKSHOT").is_none() {
            assert!(!super::super::trickshot::enabled());
        }
    }
}

use std::path::PathBuf;

use asset_audio::load_mp_sound_bank;
use asset_game::{load_mp_localized_strings, load_ui_menu_catalog};
use asset_transport::{LoadProgress, find_runtime_common_mp, find_zone_file, list_mp_map_packs};
use assets::{
    LoadingPreviewSource, LoadingScreen, MatchLoadRequest, NamespaceSoundIwd, NamespaceTrees,
};
use audio::{SoundBank, SoundIwd};
use bevy::prelude::*;
use bevy::window::PresentMode;
use render::diag::acceptance::{
    ACCEPTANCE_HEIGHT, ACCEPTANCE_PRESENT_MODE, ACCEPTANCE_WIDTH, AcceptanceRun,
};
use render::diag::capture::{CaptureQueue, CaptureRequest};
use render_frontend::prepare::scene::world::WorldScene;
use replay::{Playback, ReplayPlayback};
use session::StartupCommands;
use ui::{
    AppScreen, ClassLoadoutCatalog, LaunchIdentity, LaunchReport, MenuMapList, UiAssetRoot, UiDraw,
    UiLayer, UiLayers,
};

use crate::args::{AcceptanceLaunch, LaunchMode};
use crate::bench;
use crate::plugins::{add_runtime_plugins, add_runtime_plugins_with_role};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Listen,
    Dedicated,
    Client,
    Replay,
}

struct LaunchConfig {
    role: Role,
    zone: String,
    games_root: PathBuf,
    artifacts: PathBuf,
}

fn launch_report(
    zone: String,
    common_mp: Result<PathBuf, String>,
    zone_ff: Result<PathBuf, String>,
    zone_alias: Option<String>,
) -> LaunchReport {
    LaunchReport {
        zone,
        common_mp,
        zone_ff,
        zone_alias,
        sim_gap: "map loading has not reached clipmap yet",
        prediction_metrics: None,
        world_report: vec!["queued background zone load".into()],
    }
}

#[derive(Resource)]
struct ShellCommonTask {
    task: bevy::tasks::Task<assets::ShellCommon>,
    perk_table: Option<asset_game::CapturedStringTable>,
    started: std::time::Instant,
}

fn install_class_catalog(
    mut commands: Commands,
    shell: Option<ResMut<ShellCommonTask>>,
    mut strings: ResMut<asset_game::LocalizeCatalog>,
    menus: Res<asset_game::MenuCatalog>,
) {
    use bevy::tasks::futures_lite::future;
    let Some(mut shell) = shell else {
        return;
    };
    let Some(common) = future::block_on(future::poll_once(&mut shell.task)) else {
        return;
    };
    for line in &common.report {
        diag::info!(Launch, "{line}");
    }
    let mut class_catalog =
        ClassLoadoutCatalog::from_weapon_registry(std::sync::Arc::new(common.weapons))
            .with_weapon_tables(&common.tables);
    if let Some(table) = shell.perk_table.as_ref() {
        class_catalog = class_catalog.with_perk_table(table);
    }
    diag::info!(
        Launch,
        "CAC menu: primary={} secondary={} lethal={} tactical={} excluded={} ({:.0}ms after the menu started)",
        class_catalog.primary.len(),
        class_catalog.secondary.len(),
        class_catalog.lethal.len(),
        class_catalog.tactical.len(),
        class_catalog.excluded.len(),
        shell.started.elapsed().as_secs_f32() * 1000.0,
    );
    for (key, preview) in &mut class_catalog.previews {
        if key.contains('+') && menus.material_images.contains_key(&preview.image) {
            preview.image = format!("iw4:material/{}", preview.image);
        }
    }
    strings.absorb(common.strings);
    commands.insert_resource(class_catalog);
    commands.remove_resource::<ShellCommonTask>();
}

fn launch_identity(config: &LaunchConfig) -> LaunchIdentity {
    LaunchIdentity {
        role_label: format!("{:?}", config.role),
        games_root: config.games_root.clone(),
        artifacts: config.artifacts.clone(),
        zone: config.zone.clone(),
    }
}

fn fatal(msg: &str) -> ! {
    diag::exit_launch_error(msg);
}

pub fn launch(
    games: asset_transport::GamesRoot,
    artifacts: PathBuf,
    mode: LaunchMode,
    acceptance: Option<AcceptanceLaunch>,
) {
    diag::info!(Launch, "{}", asset_transport::games_root_report(&games));

    // bo2mp: with Black Ops II installed its own stock classes are the
    // defaults, so a Black Ops II-only install has classes it can spawn with.
    let bo2_installed = find_zone_file(&games, "t6:patch_mp").is_ok();
    frame::set_prefer_bo2_classes(bo2_installed);
    diag::info!(Launch, "default classes: {}", if bo2_installed { "Black Ops II stock" } else { "guest presets" });

    if let Some(plan) = crate::frame_owner::prefer_performance_cores() {
        assets::publish_process_cpus(plan.allowed.clone());
        diag::info!(
            Launch,
            "frame owner: cpus {:?} of {:?} allowed (performance cores {:?})",
            plan.chosen,
            plan.allowed,
            plan.performance
        );
    }
    match mode {
        LaunchMode::Menu => {
            if acceptance.is_some() {
                fatal(&format!(
                    "render acceptance requires `CodLegacy map <zone>` (not menu); maps: {}",
                    render::diag::acceptance::ACCEPTANCE_MAPS.join(", ")
                ));
            }
            run_menu(games, artifacts);
        }
        LaunchMode::FrontendT6Mp => {
            if acceptance.is_some() {
                fatal("render acceptance requires `CodLegacy map <zone>` (not frontend)");
            }
            run_frontend_t6mp(games, artifacts);
        }
        LaunchMode::CheckMaps => run_check_maps(&games, &artifacts),
        LaunchMode::Pack(dir) => run_pack(&games, &dir),
        LaunchMode::CheckGhosts(zone) => run_check_ghosts(&games, &artifacts, zone.as_deref()),
        LaunchMode::ExportMw3(dir) => match ui::export_mw3_small_copy(&dir) {
            Ok(line) => println!("{line}"),
            Err(e) => fatal(&format!("Modern Warfare III small copy: {e}")),
        },
        LaunchMode::ExportJup(dir) => match assets::export_jup(&dir, &mut |line| println!("{line}")) {
            Ok(line) => println!("{line}"),
            Err(e) => fatal(&format!("Modern Warfare III export: {e}")),
        },
        LaunchMode::CheckGuns => {
            let lines = assets::check_guest_guns();
            for line in &lines {
                println!("{line}");
            }
            let out = artifacts.join("gun-check.txt");
            let _ = std::fs::create_dir_all(&artifacts);
            match std::fs::write(&out, lines.join("\r\n")) {
                Ok(()) => println!("report: {}", out.display()),
                Err(e) => println!("report not written ({}): {e}", out.display()),
            }
        }
        LaunchMode::Map(zone) => {
            // bo2mp: the game is Black Ops II at its core: `CodLegacy map <zone>`
            // plays the map by Black Ops II's rules, as the front end's
            // START MATCH does, whichever game the map comes from.
            assets::set_t6_rules(true);
            register_t7_zombies(&zone);
            run_map(games, artifacts, zone, acceptance, Role::Listen, None)
        }
        LaunchMode::Serve(zone) => {
            assets::set_t6_rules(true);
            let rotation = session::server::ServerRotation::from_env(&zone).unwrap_or_else(|e| fatal(&e));
            let zone = rotation.current().zone.clone();
            session::server::configure(rotation);
            register_t7_zombies(&zone);
            run_map(games, artifacts, zone, None, Role::Dedicated, None)
        }
        LaunchMode::Frontend(zone) => {
            frame::set_bo2_frontend(&zone);
            run_map(games, artifacts, zone, None, Role::Listen, None)
        }
        LaunchMode::ExportGltf(zone) => {
            if acceptance.is_some() {
                fatal("render acceptance is not available for export-gltf");
            }
            run_export_gltf(games, artifacts, zone);
        }
        LaunchMode::Play {
            name,
            zone_override,
        } => run_play(games, artifacts, name, zone_override, acceptance),
    }
}

/// bo2mp: `map t7:zm_<map>` plays a Black Ops III Zombies map as Black Ops
/// II Zombies through the foreign map script (`sim::register_foreign_zm_map`;
/// the map's lane registers its side when it reads the map).
fn register_t7_zombies(zone: &str) {
    if let Some(stem) = zone.trim().to_ascii_lowercase().strip_prefix("t7:").filter(|s| s.starts_with("zm_")) {
        sim::register_foreign_zm_map(stem);
    }
}

/// bo2mp `CodLegacy check-maps`: every MW2 map the front end lists (MW2's own,
/// IW4x's packs, usermaps) read and checked (`assets::check_iw4_map`), then
/// Call of Duty 4's and World at War's (`assets::check_map`), one
/// line each, to the console and `<artifacts>/map-check.txt`.
fn run_check_maps(games: &asset_transport::GamesRoot, artifacts: &std::path::Path) {
    let mut maps: Vec<String> = asset_transport::list_mp_map_packs(games)
        .into_iter()
        .flat_map(|p| p.maps)
        .filter(|k| k.starts_with("iw4:"))
        .collect();
    maps.sort();
    maps.dedup();
    let mut lines = vec![format!("MW2 map check (with usermaps): {} maps under {}", maps.len(), games.0.display())];
    println!("{}", lines[0]);
    let (mut ok, mut bad) = (0, 0);
    for key in &maps {
        let started = std::time::Instant::now();
        let line = match find_zone_file(games, key) {
            Ok(found) => {
                let path = found.path.clone();
                let result = std::panic::catch_unwind(move || assets::check_iw4_map(&path))
                    .unwrap_or_else(|_| "FAIL the reader panicked".to_owned());
                format!("{key:<28} {result} [{:.1}s] {}", started.elapsed().as_secs_f32(), found.path.display())
            }
            Err(e) => format!("{key:<28} FAIL not found: {e}"),
        };
        if line.contains(" OK:") {
            ok += 1;
        } else {
            bad += 1;
        }
        println!("{line}");
        lines.push(line);
    }
    // bo2mp: Call of Duty 4's and World at War's maps, read by their own
    // readers (`assets::check_map`).
    let legacy = asset_transport::list_legacy_mp_map_files(games);
    let head = format!("CoD4 and World at War map check: {} maps", legacy.len());
    println!("{head}");
    lines.push(head);
    for (game, stem, path) in legacy {
        let started = std::time::Instant::now();
        let key = format!("{game}:{stem}");
        let at = path.clone();
        let result = std::panic::catch_unwind(move || assets::check_map(&game, &at))
            .unwrap_or_else(|_| "FAIL the reader panicked".to_owned());
        let line = format!("{key:<28} {result} [{:.1}s] {}", started.elapsed().as_secs_f32(), path.display());
        if line.contains(" OK:") {
            ok += 1;
        } else {
            bad += 1;
        }
        println!("{line}");
        lines.push(line);
    }
    lines.push(format!("{ok} OK, {bad} with problems"));
    println!("{}", lines.last().map_or("", String::as_str));
    let out = artifacts.join("map-check.txt");
    let _ = std::fs::create_dir_all(artifacts);
    match std::fs::write(&out, lines.join("\r\n")) {
        Ok(()) => println!("report: {}", out.display()),
        Err(e) => println!("report not written ({}): {e}", out.display()),
    }
}

/// bo2mp `CodLegacy pack <folder>`: every multiplayer map from the other games
/// read as a match reads it and its gameplay half written to
/// `<folder>/<game>/<map>.iw4lmap`, one line each and a total; a dedicated
/// server given the folder (`CODLEGACY_SERVER_PACKS`) hosts them without their
/// games installed. Black Ops II's own maps are not packed (the server reads
/// them from Black Ops II, which it has for the rules).
fn run_pack(games: &asset_transport::GamesRoot, dir: &std::path::Path) {
    let mut maps: Vec<(String, String, PathBuf)> = asset_transport::list_mp_maps(games)
        .into_iter()
        .filter_map(|key| {
            let (game, map) = server_pack::split_key(&key);
            if game == "t6" {
                return None;
            }
            let found = find_zone_file(games, &key).ok()?;
            Some((game, map, found.path))
        })
        .collect();
    maps.extend(asset_transport::list_legacy_mp_map_files(games));
    maps.sort();
    maps.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    println!("server packs: {} maps into {}", maps.len(), dir.display());
    let (mut ok, mut bad, mut total) = (0usize, 0usize, 0usize);
    for (game, map, path) in maps {
        let started = std::time::Instant::now();
        let (g, m, at) = (game.clone(), map.clone(), path.clone());
        let built = std::panic::catch_unwind(move || assets::build_map_pack(&g, &m, &at))
            .unwrap_or_else(|_| Err("the reader panicked".to_owned()));
        let line = match built.and_then(|pack| pack.write(&server_pack::pack_path(dir, &game, &map)).map(|n| (pack, n))) {
            Ok((pack, bytes)) => {
                ok += 1;
                total += bytes;
                let brushes = pack.collision.as_ref().map_or(0, |c| c.brushes.len());
                format!(
                    "{game}:{map:<24} OK {:.2} MB ({brushes} brushes, {} spawns) [{:.1}s]",
                    bytes as f64 / 1_048_576.0,
                    pack.spawns.len(),
                    started.elapsed().as_secs_f32()
                )
            }
            Err(e) => {
                bad += 1;
                format!("{game}:{map:<24} FAIL {e}")
            }
        };
        println!("{line}");
    }
    println!("{ok} packed ({:.1} MB in all), {bad} failed", total as f64 / 1_048_576.0);
}

/// `CodLegacy check-ghosts [zone]`: see `audio::music::ghosts_check_report`;
/// to the console and `<artifacts>/ghosts-check.txt`.
fn run_check_ghosts(games: &asset_transport::GamesRoot, artifacts: &std::path::Path, zone: Option<&str>) {
    let lines = audio::music::ghosts_check_report(Some(&games.0), zone);
    for line in &lines {
        println!("{line}");
    }
    let out = artifacts.join("ghosts-check.txt");
    let _ = std::fs::create_dir_all(artifacts);
    match std::fs::write(&out, lines.join("\r\n")) {
        Ok(()) => println!("report: {}", out.display()),
        Err(e) => println!("report not written ({}): {e}", out.display()),
    }
}

fn run_export_gltf(games: asset_transport::GamesRoot, artifacts: PathBuf, zone_arg: String) {
    let found = find_zone_file(&games, &zone_arg)
        .unwrap_or_else(|error| fatal(&format!("export-gltf: {error}")));
    let game = asset_transport::zone_game_for_path(&found.path)
        .unwrap_or_else(|| fatal("export-gltf: source game could not be identified"));
    if game != asset_core::ZoneGame::Iw4 {
        fatal(&format!(
            "export-gltf P1a supports native IW4 only; {} is {}",
            found.zone_name,
            game.prefix()
        ));
    }
    let common_mp = find_runtime_common_mp(&games, &found.path).map(|zone| zone.path);

    let prepared = match bevy::tasks::futures_lite::future::block_on(assets::load_prepared_match(
        Ok(found.path),
        common_mp,
        asset_transport::LoadProgress::default(),
    )) {
        assets::MatchLoadOutcome::Ready(prepared) => prepared,
        assets::MatchLoadOutcome::Canceled => fatal("export-gltf: map walk canceled"),
    };
    let summary = assets::export_prepared_world_gltf(
        &artifacts,
        &found.zone_name,
        prepared.world,
        &prepared.materials,
    )
    .unwrap_or_else(|error| fatal(&format!("export-gltf: {error}")));
    diag::announce_stdout(&summary.scene.display().to_string());
    diag::announce_stdout(&summary.report_line());
}

fn run_menu(games: asset_transport::GamesRoot, artifacts: PathBuf) {
    start_perf(None, "menu");
    let ui_games = asset_game::ui_games_root(&games).unwrap_or_else(|error| {
        let content = asset_transport::games_content_report(&games).join("\n");
        fatal(&format!(
            "Cannot start the IW4 menu: base MW2 Multiplayer assets were not found.\n\n\
             {content}\n\n\
             The menu requires common_mp.ff with IW4 envelope version 0x114. \
             Point CODLEGACY_GAMES or a shortcut beside iw4launcher.exe to the folder containing \
             the base MW2 Multiplayer files. If this is the intended folder, restore its \
             missing base files; DLC maps alone are insufficient.\n\nSearch details: {error}"
        ))
    });
    let shell_common = assets::load_pool().spawn(assets::load_shell_common(games.clone()));
    assets::prefetch_match_common(games.clone());
    let (mut menus, menu_report) = load_ui_menu_catalog(&ui_games);
    ui::install_frontend_menus(&mut menus).unwrap_or_else(|error| fatal(&error));
    for line in &menu_report {
        diag::info!(Launch, "{line}");
    }
    let missing = ["main", "main_text"]
        .into_iter()
        .filter(|name| menus.get(name).is_none())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        fatal(&format!(
            "Cannot start the IW4 menu: {} not loaded from {}.\n\n\
             A missing menu can mean an absent file or a FastFile decoding failure. \
             Include the catalog details below when reporting this error.\n\n{}",
            missing.join(", "),
            ui_games.0.display(),
            menu_report.join("\n")
        ));
    }
    let maps = list_mp_map_packs(&games);
    diag::info!(
        Launch,
        "menu: {} maps in {} packs under {}",
        maps.iter().map(|pack| pack.maps.len()).sum::<usize>(),
        maps.len(),
        games.0.display()
    );
    let config = LaunchConfig {
        role: Role::Listen,
        zone: String::new(),
        games_root: games.0.clone(),
        artifacts,
    };
    let mut app = App::new();
    let namespace_trees = NamespaceTrees::discover(&games);
    let have = content_flags(&namespace_trees, &games);
    let master_intent = if have.0 & net::CONTENT_IW4 == 0 {
        diag::warn!(
            Net,
            "master browser disabled: IW4 common_mp.ff is not installed"
        );
        net::MasterLaunchIntent::disabled()
    } else {
        net::MasterLaunchIntent::browser_from_env(have).unwrap_or_else(|error| {
            diag::warn!(Net, "master browser disabled: {error}");
            net::MasterLaunchIntent::disabled()
        })
    };
    app.insert_resource(master_intent);
    app.add_plugins(crate::plugins::default_plugins_with_quiet_log(
        WindowPlugin {
            primary_window: Some(Window {
                title: "Call of Duty: Legacy".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        },
    ));
    app.insert_resource(ShellCommonTask {
        task: shell_common,
        perk_table: menus.string_table("mp/perkTable.csv").cloned(),
        started: std::time::Instant::now(),
    })
    .add_systems(Update, install_class_catalog);
    match load_mp_localized_strings(&ui_games, "iw4:code_post_gfx_mp") {
        Ok(loc) => {
            diag::info!(Launch, "menu: {} localize keys", loc.len());
            app.insert_resource(loc);
        }
        Err(error) => diag::warn!(Launch, "menu: localize: {error}"),
    }
    app.insert_resource(menus);
    let menu_bank = match load_mp_sound_bank(&ui_games, "iw4:code_post_gfx_mp") {
        Ok(loaded) => {
            diag::info!(Launch, "menu: sound bank ready");
            for line in loaded.gap_lines() {
                diag::warn!(Launch, "menu: {line}");
            }
            let bank = std::sync::Arc::new(loaded.catalog);
            app.insert_resource(SoundBank(std::sync::Arc::clone(&bank)));
            Some(bank)
        }
        Err(error) => {
            diag::warn!(Launch, "menu: sound bank: {error}");
            None
        }
    };

    let (menu_iwd, lines) = NamespaceSoundIwd::open(&namespace_trees);
    for line in lines {
        diag::info!(Launch, "menu: {line}");
    }
    let menu_iwd = std::sync::Arc::new(menu_iwd);
    app.insert_resource(SoundIwd(std::sync::Arc::clone(&menu_iwd)));
    if let Some(bank) = menu_bank {
        app.insert_resource(audio::ClipStore::start(
            std::sync::Arc::clone(&bank),
            Some(std::sync::Arc::clone(&menu_iwd)),
        ));
        app.insert_resource(audio::FrontendAudio {
            bank,
            iwd: menu_iwd,
        });
    }
    app.insert_resource(launch_identity(&config))
        .insert_resource(MenuMapList(maps))
        .insert_resource(AppScreen::MainMenu)
        .insert_resource(UiAssetRoot(Some(ui_games.0)))
        .insert_resource(StartupCommands {
            lines: console::startup_commands(),
        })
        .insert_resource(WorldScene::default())
        .insert_resource(ClearColor(Color::srgb(0.04, 0.045, 0.06)))
        .insert_resource({
            let mut layers = UiLayers::default();
            layers.show_only([UiLayer::Shell, UiLayer::Overlay]);
            layers
        });
    add_runtime_plugins(&mut app);
    if let Some(capture) = CaptureRequest::from_env() {
        queue_launch_capture(
            &mut app,
            CaptureRequest {
                exit_after_capture: true,
                ..capture
            },
        );
    }
    log_exit(app.run());
    let _ = flush_perf();
}

/// bo2mp lane C: Black Ops II's multiplayer front end and no map: his MP UI
/// zones (scripts, pictures, fonts, tables, English text) read here, then
/// the menu screen runs BO2's own menus (`ui::Bo2mpFrontend`, drawn by the
/// LUI HUD on the shell layer). No MW2 menu catalog, sounds or master
/// browser. START MATCH swaps to `t6:<map>` like the `map` command.
fn run_frontend_t6mp(games: asset_transport::GamesRoot, artifacts: PathBuf) {
    start_perf(None, "frontend");
    // Every match this front end starts plays by Black Ops II's rules,
    // whichever game its map comes from.
    assets::set_t6_rules(true);
    let mut boot = crate::boot_steps::BootSteps::start();
    assets::prefetch_own_image_at_launch();
    // The first launch asks where each game is installed.
    crate::game_folders::first_run();
    boot.step("game folders");
    // The menu music's guest catalogs and the sound archives (half a minute
    // of BO1 zone reads and archive indexes) need nothing from the front end,
    // so they are read on their own thread. On a hard drive it starts once
    // the front end's zones are read (beside the window and renderer): read
    // together, the head sought between them (front end 43.8 s and these
    // 72 s on a cold boot, 1.1 s and 1 s warm). On a solid-state drive, where
    // a seek costs next to nothing, it starts with the boot.
    // `CODLEGACY_MENU_SOUNDS_AT_BOOT=1` starts it with the boot on any drive.
    let spawn_menu_sounds = {
        let games = games.clone();
        move || {
            let games = games.clone();
            std::thread::Builder::new()
                .name("menu-sounds".into())
                .spawn(move || {
                    let span = ThreadSpan::start();
                    let sources = menu_sound_sources(&games);
                    span.end("menu-sounds");
                    sources
                })
                .ok()
        }
    };
    let patch = find_zone_file(&games, "t6:patch_mp").unwrap_or_else(|error| {
        fatal(&format!(
            "Cannot start the Black Ops II multiplayer front end: its files were not found.\n\nSet Black Ops II's folder in game_paths.cfg beside the game, or delete that file to be asked again.\n\n{error}"
        ))
    });
    let zone_all = patch.path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let seeks = drive_seeks_cost(&zone_all);
    let at_boot = codlegacy_env::var("CODLEGACY_MENU_SOUNDS_AT_BOOT").is_ok_and(|v| v.trim() == "1") || !seeks;
    // The online panel's content flags walk every game's folders (tens of
    // seconds on a cold disk) on their own thread. On a hard drive the walk
    // reads at background priority, so the front end's reads go first and
    // the walk fills the gaps between them (the front end's pictures took
    // 36.9 s with 1.1 GB read beside them). `CODLEGACY_SCAN_BG_IO=0` reads it
    // at normal priority.
    let scan_in_background = seeks && codlegacy_env::var("CODLEGACY_SCAN_BG_IO").map_or(true, |v| v.trim() != "0");
    // Load speed: on a hard drive the walk starts once the front end's zones
    // are read, so the disk serves one reader fewer (on a first launch the
    // HUD-icon bundle is written then: 62 s of pictures with the walk's 2473
    // reads beside them on a cold boot). `CODLEGACY_SCAN_AFTER_BUNDLE=0` walks
    // beside the zone reads.
    let scan_after_front = seeks && codlegacy_env::var("CODLEGACY_SCAN_AFTER_BUNDLE").map_or(true, |v| v.trim() != "0");
    let (front_read_tx, front_read_rx) = std::sync::mpsc::channel::<()>();
    let content_scan = {
        let games = games.clone();
        std::thread::Builder::new()
            .name("content-scan".into())
            .spawn(move || {
                if scan_after_front {
                    // (A read that stopped drops the sender, which lets it go too.)
                    let _ = front_read_rx.recv();
                }
                let span = ThreadSpan::start();
                if scan_in_background {
                    asset_audio::background_io(true);
                }
                let have = content_flags(&NamespaceTrees::discover(&games), &games);
                asset_transport::save_zone_versions();
                if scan_in_background {
                    asset_audio::background_io(false);
                }
                span.end(if scan_in_background { "content-scan (background priority)" } else { "content-scan" });
                have
            })
            .ok()
    };
    // The map list (every game's map zones opened for their headers) is
    // walked beside the front end's zone reads, at the scan's priority, so
    // the console's startup takes it ready (190 s on the main thread on a
    // cold, busy hard drive). `CODLEGACY_MAP_LIST_EARLY=0` = walked there.
    asset_transport::start_map_pack_listing(&games, move |on| {
        if scan_in_background {
            asset_audio::background_io(on);
        }
    });
    diag::info!(
        Launch,
        "boot: menu sounds read {}",
        if at_boot { "with the front end" } else { "after the front end's zones (a hard drive)" }
    );
    let early_sounds = if at_boot { spawn_menu_sounds() } else { None };
    // Load speed: the front end's zones are read on their own thread while
    // this one sets up the window and renderer (on a cold boot 69.5 s of
    // zones, then 16.4 s of window and renderer, one after the other); neither
    // needs the other, and the window opens once both are done.
    // `CODLEGACY_FRONTEND_BESIDE_RENDERER=0` reads them here first.
    let beside_renderer = codlegacy_env::var("CODLEGACY_FRONTEND_BESIDE_RENDERER").map_or(true, |v| v.trim() != "0");
    let read_front = {
        let zone_all = zone_all.clone();
        move || {
            let started = std::time::Instant::now();
            // The first map's slow disk reads, done while he is in the menus.
            assets::prewarm_t6_mp_shared(&zone_all);
            let front = assets::load_t6_frontend(&zone_all);
            let _ = front_read_tx.send(());
            let late_sounds = if at_boot { None } else { spawn_menu_sounds() };
            (front, late_sounds, started.elapsed())
        }
    };
    let (front_thread, read_here) = if beside_renderer {
        let thread = std::thread::Builder::new().name("front-end-zones".into()).spawn(move || {
            let span = ThreadSpan::start();
            let read = read_front();
            span.end("front-end-zones");
            read
        });
        (thread.ok(), None)
    } else {
        (None, Some(read_front()))
    };
    boot.step(if read_here.is_some() { "front end zones read" } else { "Black Ops II found, front end zones being read" });
    let config = LaunchConfig {
        role: Role::Listen,
        zone: String::new(),
        games_root: games.0.clone(),
        artifacts: artifacts.clone(),
    };
    let mut app = App::new();
    // One line per wait inside app.run before the menu's first frame.
    app.add_plugins(crate::boot_steps::BootMarksFirst);
    app.add_plugins(crate::plugins::default_plugins_with_quiet_log(
        WindowPlugin {
            primary_window: Some(Window {
                title: "Call of Duty: Legacy".into(),
                resolution: (1280, 720).into(),
                position: test_window_position().unwrap_or_default(),
                focused: test_window_position().is_none() && !test_window_hidden(),
                visible: !test_window_hidden(),
                ..default()
            }),
            ..default()
        },
    ));
    boot.step("window and renderer set up");
    // (A thread that did not start or stopped: the zones are read here, and
    // the menu sounds where they are used.)
    let (front, late_sounds, took) = read_here.unwrap_or_else(|| {
        let read = front_thread.and_then(|thread| thread.join().ok()).unwrap_or_else(|| {
            diag::warn!(Launch, "boot: the front end's zones were not read on their thread; reading them here");
            let started = std::time::Instant::now();
            (assets::load_t6_frontend(&zone_all), None, started.elapsed())
        });
        boot.step("front end zones read (beside the window and renderer)");
        read
    });
    let menu_sounds = early_sounds.or(late_sounds);
    for line in &front.report {
        diag::info!(Launch, "{line}");
    }
    diag::info!(
        Launch,
        "frontend t6mp: read {} in {:.1} s",
        zone_all.display(),
        took.as_secs_f64()
    );
    // (Taken here, after the window and renderer, so a walk that waits for
    // the front end's zones does not hold them up; the net plugin reads it.)
    // The front end's online panel counts the players on the master server
    // (CODLEGACY_MASTER_ADDR), when one is set.
    // Launch speed: the scan reads at background priority and a busy drive can
    // starve it (128.7 s waited here in a hidden run); the menu comes up
    // without it and its flags reach the intent when it ends (they only matter
    // once he hosts or joins). `CODLEGACY_CONTENT_SCAN_WAIT=1` waits here (old way).
    let wait_for_scan = codlegacy_env::var("CODLEGACY_CONTENT_SCAN_WAIT").is_ok_and(|v| v.trim() == "1");
    let have = match content_scan {
        Some(scan) if wait_for_scan || scan.is_finished() => scan
            .join()
            .ok()
            .unwrap_or_else(|| content_flags(&NamespaceTrees::discover(&games), &games)),
        Some(scan) => {
            diag::info!(Launch, "content scan: not done, the menu goes on without it");
            app.insert_resource(PendingContentScan(Some(scan)))
                .add_systems(Update, apply_content_scan);
            master_protocol::ContentFlags::default()
        }
        None => content_flags(&NamespaceTrees::discover(&games), &games),
    };
    app.insert_resource(net::MasterLaunchIntent::browser_from_env(have).unwrap_or_else(|error| {
        diag::warn!(Net, "front end online panel: no master server ({error})");
        net::MasterLaunchIntent::disabled()
    }));
    boot.step("game folders scanned");
    let fe = ui::Bo2mpFrontend {
        // (bo2mp: his one stats file, the match's card's too.)
        stats_path: ui::bo2mp_stats_file(Some(&artifacts)),
    };
    // BO2's menu sounds and music: the front end's own bank (also put back
    // when a match ends).
    match front.sound {
        Ok(mut catalog) => {
            // The menu music rotates through BO1's, MW2's and MW3's main-menu
            // tracks too: their catalogs join BO2's, and their games' sound
            // archives (BO1 from the games folder, MW2/MW3 from the
            // voice-only one) are opened for the streamed tracks.
            // Busy PC: those reads (BO1's zones and every game's sound
            // archive index) are a long wait on a hard drive other programs
            // are using, and only the music needs them. When they are not
            // done the menu comes up with Black Ops II's own sounds, the
            // music waits (`audio::MenuMusicWaits`), and the whole bank goes
            // in when the thread ends (`apply_menu_sounds`).
            // `CODLEGACY_MENU_SOUNDS_WAIT=1` waits here (the old way).
            let wait_for_sounds = codlegacy_env::var("CODLEGACY_MENU_SOUNDS_WAIT").is_ok_and(|v| v.trim() == "1");
            match menu_sounds {
                Some(read) if !wait_for_sounds && !read.is_finished() => {
                    diag::info!(Launch, "menu sounds: not read yet, the menu goes on with Black Ops II's own");
                    let base = catalog.clone();
                    catalog.finalize();
                    let bank = std::sync::Arc::new(catalog);
                    let iwd = std::sync::Arc::new(NamespaceSoundIwd::default());
                    app.insert_resource(SoundBank(std::sync::Arc::clone(&bank)));
                    app.insert_resource(SoundIwd(std::sync::Arc::clone(&iwd)));
                    app.insert_resource(audio::ClipStore::start(std::sync::Arc::clone(&bank), Some(std::sync::Arc::clone(&iwd))));
                    app.insert_resource(audio::FrontendAudio { bank, iwd });
                    app.insert_resource(audio::MenuMusicWaits);
                    app.insert_resource(PendingMenuSounds(Some((read, base))));
                    app.add_systems(Update, apply_menu_sounds);
                }
                read => {
                    let sources = read
                        .and_then(|read| read.join().ok())
                        .unwrap_or_else(|| menu_sound_sources(&games));
                    let (bank, iwd) = whole_menu_bank(catalog, sources);
                    app.insert_resource(SoundBank(std::sync::Arc::clone(&bank)));
                    app.insert_resource(SoundIwd(std::sync::Arc::clone(&iwd)));
                    app.insert_resource(audio::ClipStore::start(std::sync::Arc::clone(&bank), Some(std::sync::Arc::clone(&iwd))));
                    app.insert_resource(audio::FrontendAudio { bank, iwd });
                }
            }
        }
        Err(error) => diag::warn!(Launch, "frontend t6mp: no menu sounds: {error}"),
    }
    boot.step("menu music and sound archives opened");
    app.insert_resource(ui::Bo2mpFrontendAssets {
        ui: front.ui.clone(),
        icons: front.icons.clone(),
        fonts: front.fonts.clone(),
        front: fe.clone(),
    })
    .insert_resource(front.ui)
        .insert_resource(front.icons)
        .insert_resource(front.fonts)
        .insert_resource(fe)
        .insert_resource(asset_game::MenuCatalog::default())
        .insert_resource(launch_identity(&config))
        .insert_resource(MenuMapList(Vec::new()))
        .insert_resource(AppScreen::MainMenu)
        .insert_resource(UiAssetRoot(None))
        .insert_resource(StartupCommands {
            lines: console::startup_commands(),
        })
        .insert_resource(WorldScene::default())
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource({
            let mut layers = UiLayers::default();
            layers.show_only([UiLayer::Shell, UiLayer::Overlay]);
            layers
        });
    add_runtime_plugins(&mut app);
    app.add_plugins(crate::boot_steps::BootMarksLast::default());
    app.add_systems(Update, crate::boot_steps::log_menu_up);
    boot.step("game systems added");
    // lane B (B5): the hang sampler (CODLEGACY_HANG_SAMPLER=1) starts on the
    // first beat; beating here lets it name what the menu's first frame
    // waits on too, not only the stalls after it.
    diag::hang::beat();
    if let Some(capture) = CaptureRequest::from_env() {
        queue_launch_capture(
            &mut app,
            CaptureRequest {
                exit_after_capture: true,
                ..capture
            },
        );
    }
    log_exit(app.run());
    let _ = flush_perf();
}

fn run_play(
    games: asset_transport::GamesRoot,
    artifacts: PathBuf,
    name: String,
    zone_override: Option<String>,
    acceptance: Option<AcceptanceLaunch>,
) {
    if acceptance.is_some() {
        fatal("render acceptance requires `CodLegacy map <zone>` (not play)");
    }
    let playback = Playback::open(&artifacts, &name).unwrap_or_else(|e| {
        fatal(&format!("play: {e}"));
    });
    let zone = zone_override
        .or_else(|| playback.identity().zone_name().map(str::to_owned))
        .unwrap_or_else(|| {
            fatal(&format!(
                "play: recording {} has no zone — re-record on this build, or: make play {name} ZONE=<map>",
                playback.path().display()
            ));
        });
    diag::info!(Launch, "play: {} zone={zone}", playback.path().display());
    let session = ReplayPlayback::new(playback);
    run_map(games, artifacts, zone, None, Role::Replay, Some(session));
}

fn run_map(
    games: asset_transport::GamesRoot,
    artifacts: PathBuf,
    zone_arg: String,
    acceptance: Option<AcceptanceLaunch>,
    role: Role,
    playback: Option<ReplayPlayback>,
) {
    let found = find_zone_file(&games, &zone_arg);
    let zone_alias = found.as_ref().ok().and_then(|z| z.alias_note.clone());
    let zone_arg_lc = zone_arg.trim().to_ascii_lowercase();
    let (parsed_game, requested_stem) = asset_transport::split_zone_key(&zone_arg_lc);
    let zone = found
        .as_ref()
        .ok()
        .map(|z| z.zone_name.clone())
        // bo2mp: a map served from its server pack keeps its game's key.
        .unwrap_or_else(|| match server_pack::find_pack(&zone_arg_lc) {
            Some(_) => zone_arg_lc.clone(),
            None => requested_stem.to_owned(),
        });
    let zone_ff = found
        .as_ref()
        .map(|z| z.path.clone())
        .map_err(|e| e.clone());
    let common_mp = match &zone_ff {
        Ok(path) => find_runtime_common_mp(&games, path).map(|z| z.path),
        Err(e) => Err(e.clone()),
    };
    let probe = launch_report(zone.clone(), common_mp.clone(), zone_ff.clone(), zone_alias);
    let game = parsed_game.or_else(|| {
        found
            .as_ref()
            .ok()
            .and_then(|z| asset_transport::zone_game_for_path(&z.path))
    });
    let loading_title = asset_transport::map_load_title(&zone_arg_lc, game);
    let namespace_trees = NamespaceTrees::discover(&games);
    let have = content_flags(&namespace_trees, &games);
    let requires = net::content_required_by_map(&zone_arg_lc)
        .unwrap_or_else(|error| fatal(&format!("master content: {error}")));

    let master_intent = if role == Role::Replay {
        net::MasterLaunchIntent::disabled()
    } else if have.0 & net::CONTENT_IW4 == 0
        // bo2zm: a Black Ops II map needs only Black Ops II (co-op).
        && !(requires.0 & net::CONTENT_T6 != 0 && have.0 & net::CONTENT_T6 != 0)
    {
        diag::warn!(
            Net,
            "master launch disabled: IW4 common_mp.ff is not installed"
        );
        net::MasterLaunchIntent::disabled()
    } else {
        net::MasterLaunchIntent::from_env_for_map(&zone, have, requires).unwrap_or_else(|error| {
            diag::warn!(Net, "master launch disabled: {error}");
            net::MasterLaunchIntent::disabled()
        })
    };
    // bo2mp: a dedicated server's room runs its rotation's first mode.
    let master_intent = match role {
        Role::Dedicated => master_intent.with_host_mode(session::server::first_gametype().unwrap_or_else(|| "tdm".to_owned())),
        _ => master_intent,
    };

    let config = LaunchConfig {
        role: if master_intent.is_join() {
            Role::Client
        } else {
            role
        },
        zone: zone.clone(),
        games_root: games.0.clone(),
        artifacts,
    };
    start_perf(Some(zone.clone()), role_name(config.role));
    let (mut menus, menu_report) = load_ui_menu_catalog(&games);
    if let Err(error) = ui::install_frontend_menus(&mut menus) {
        // bo2zm: a Black Ops II map runs without MW2's menu files, which the
        // built-in front-end menus extend.
        if game == Some(asset_transport::ZoneGame::T6) {
            diag::warn!(Launch, "front-end menus left out for a T6 map: {error}");
        } else {
            fatal(&error);
        }
    }
    for line in &menu_report {
        diag::info!(Launch, "{line}");
    }
    let progress = LoadProgress::default();
    let acceptance_run = acceptance
        .map(|a| AcceptanceRun {
            artifact_dir: a.dir,
            map: zone.clone(),
        })
        .or_else(|| AcceptanceRun::from_env(zone.clone()));
    let present_mode = launch_present_mode(acceptance_run.is_some());
    let mut app = App::new();
    if acceptance_run.is_some() || codlegacy_env::var_os("CODLEGACY_PRESENT_MODE").is_some() {
        app.insert_resource(ui::PresentModeOverride(present_mode));
    }
    app.insert_resource(master_intent);
    let dedicated = config.role == Role::Dedicated;
    if dedicated {
        app.insert_resource(bevy::winit::WinitSettings::continuous())
            .insert_resource(frame::Headless);
    }
    if frame::Headless::requested() {
        // CODLEGACY_HEADLESS=1: nothing waits for the world to be drawn or heard.
        diag::info!(
            Launch,
            "headless: no window, no graphics device, no sound device (CODLEGACY_HEADLESS=1)"
        );
        app.insert_resource(frame::Headless);
    }
    app.add_plugins(crate::plugins::default_plugins_with_quiet_log(
        WindowPlugin {
            exit_condition: if dedicated {
                bevy::window::ExitCondition::DontExit
            } else {
                bevy::window::ExitCondition::OnAllClosed
            },
            primary_window: (!dedicated).then(|| Window {
                title: match config.role {
                    Role::Replay => format!("CodLegacy — play {}", config.zone),
                    Role::Listen | Role::Dedicated => format!("CodLegacy — {}", config.zone),
                    Role::Client => format!("CodLegacy — join {}", config.zone),
                },
                resolution: (ACCEPTANCE_WIDTH, ACCEPTANCE_HEIGHT).into(),
                present_mode,
                // CODLEGACY_WINDOW_POS=x,y places the window (test runs on another
                // monitor) and opens it without taking the keyboard.
                position: test_window_position().unwrap_or_default(),
                focused: test_window_position().is_none() && !test_window_hidden(),
                // CODLEGACY_WINDOW_HIDDEN=1: a test run with no window to see
                // (log-only tests while he plays something else).
                visible: !test_window_hidden(),
                ..default()
            }),
            ..default()
        },
    ));
    // bo2zm M4: `CodLegacy frontend <zone>` opens on Black Ops II's own menu
    // (its menu files read on a worker); Start loads the map.
    let frontend = frame::bo2_frontend().is_some();
    if !frontend && let Ok(path) = &zone_ff {
        app.insert_resource(LoadingPreviewSource {
            path: path.clone(),
            map_name: zone.clone(),
            request_id: 0,
        });
    }
    let ui_games_root = asset_game::ui_games_root(&games).ok().map(|root| root.0);
    let mut identity = launch_identity(&config);
    if frontend {
        identity.zone.clear();
    }
    app.insert_resource(identity)
        .insert_resource(probe)
        .insert_resource(menus)
        .insert_resource(UiAssetRoot(ui_games_root))
        .insert_resource(StartupCommands {
            lines: console::startup_commands(),
        })
        .insert_resource(WorldScene::default())
        .insert_resource(ClearColor(Color::BLACK));
    if frontend {
        let dir = zone_ff
            .as_ref()
            .ok()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| fatal(&format!("frontend: `{zone}` was not found")));
        let task = assets::load_pool().spawn(async move { assets::load_t6_zm_frontend(&dir) });
        app.insert_resource(T6FrontendTask(task, std::time::Instant::now()))
            .add_systems(Update, install_t6_frontend)
            .insert_resource(AppScreen::MainMenu)
            .insert_resource({
                let mut layers = UiLayers::default();
                layers.show_only([UiLayer::Shell, UiLayer::Overlay]);
                layers
            });
    } else {
        app.insert_resource(MatchLoadRequest {
            request_id: 0,
            load_key: Default::default(),
            zone: zone.clone(),
            zone_ff,
            common_mp,
            progress: progress.clone(),
        })
        .insert_resource(LoadingScreen::new(
            progress.clone(),
            loading_title,
            sim::host_game_mode_kind().display_name().to_owned(),
        ))
        .insert_resource(AppScreen::Loading)
        .insert_resource({
            let mut layers = UiLayers::default();
            layers.show_only([UiLayer::Loading, UiLayer::Overlay]);
            layers
        });
    }
    if let Some(run) = acceptance_run {
        diag::info!(
            Launch,
            "render acceptance: dir={} map={} {}x{} {:?}",
            run.artifact_dir.display(),
            run.map,
            ACCEPTANCE_WIDTH,
            ACCEPTANCE_HEIGHT,
            present_mode
        );

        app.insert_resource(UiDraw(false));
        app.insert_resource(run);
    }
    match config.role {
        Role::Replay => add_runtime_plugins_with_role(&mut app, frame::RuntimeRole::Replay),
        Role::Listen | Role::Dedicated => add_runtime_plugins(&mut app),
        Role::Client => add_runtime_plugins_with_role(&mut app, frame::RuntimeRole::Client),
    }
    // The demo, before the playback moves into the world: it names the workload
    // in the bench manifest, and "same demo" is what makes two runs comparable
    // at all. The whole path, not a stem — a clip is `clips/<id>/clip.iw4ldemo`,
    // whose stem is `clip` for every clip ever recorded.
    let demo = playback
        .as_ref()
        .map(|playback| playback.path().display().to_string());
    if let Some(playback) = playback {
        app.insert_resource(playback);
    }
    if let Some(capture) = CaptureRequest::from_env() {
        queue_launch_capture(&mut app, capture);
    }
    bench::announce_runtime(&mut app);
    let bench = bench::enabled();
    if bench {
        bench::insert(
            &mut app,
            &zone,
            demo.as_deref(),
            role_name(config.role),
            &config.artifacts,
            progress.clone(),
        );
    }
    log_exit(app.run());
    let trace = flush_perf();
    if bench {
        bench::finish(&config.artifacts, trace);
    }
}

/// bo2zm M4: Black Ops II's front end being read (and since when).
#[derive(Resource)]
struct T6FrontendTask(bevy::tasks::Task<assets::T6ZmFrontend>, std::time::Instant);

/// bo2zm M4: the front end, once read: its scripts, pictures and fonts for
/// the menu (the HUD's fonts and pictures until a map brings its own).
fn install_t6_frontend(mut commands: Commands, task: Option<ResMut<T6FrontendTask>>) {
    let Some(mut task) = task else {
        return;
    };
    let Some(front) = bevy::tasks::futures_lite::future::block_on(
        bevy::tasks::futures_lite::future::poll_once(&mut task.0),
    ) else {
        return;
    };
    for line in &front.report {
        diag::info!(Launch, "front end: {line}");
    }
    diag::info!(
        Launch,
        "front end: read in {:.0} ms",
        task.1.elapsed().as_secs_f64() * 1000.0
    );
    commands.remove_resource::<T6FrontendTask>();
    commands.insert_resource(front.icons.clone());
    commands.insert_resource(front.fonts.clone());
    // Its sounds: the menu's bank now, and again whenever a game ends.
    if let Some(bank) = front.sound.clone() {
        let iwd = std::sync::Arc::new(NamespaceSoundIwd::default());
        commands.insert_resource(SoundBank(std::sync::Arc::clone(&bank)));
        commands.insert_resource(SoundIwd(std::sync::Arc::clone(&iwd)));
        commands.insert_resource(audio::ClipStore::start(
            std::sync::Arc::clone(&bank),
            Some(std::sync::Arc::clone(&iwd)),
        ));
        commands.insert_resource(audio::FrontendAudio { bank, iwd });
    }
    commands.insert_resource(front);
}

/// What [`menu_sound_sources`] reads: the guest catalogs, the sound
/// archives, the log lines.
type MenuSoundSources = (Vec<asset_audio::SoundCatalog>, asset_transport::NamespaceSoundIwd, Vec<String>);

/// Black Ops II's own menu bank with the guest music joined in.
fn whole_menu_bank(
    mut catalog: asset_audio::SoundCatalog,
    (guests, iwd, lines): MenuSoundSources,
) -> (std::sync::Arc<asset_audio::SoundCatalog>, std::sync::Arc<NamespaceSoundIwd>) {
    for line in lines {
        diag::info!(Launch, "{line}");
    }
    for guest in guests {
        catalog.absorb_missing_aliases_unresolved(guest);
    }
    catalog.finalize();
    (std::sync::Arc::new(catalog), std::sync::Arc::new(iwd))
}

/// The menu-sounds thread the menu did not wait for, and Black Ops II's own
/// bank (not finalized) its guests join.
#[derive(Resource)]
struct PendingMenuSounds(Option<(std::thread::JoinHandle<MenuSoundSources>, asset_audio::SoundCatalog)>);

fn apply_menu_sounds(
    mut pending: ResMut<PendingMenuSounds>,
    frontend: Option<Res<audio::FrontendAudio>>,
    live: Option<Res<SoundBank>>,
    mut commands: Commands,
) {
    if !pending.0.as_ref().is_some_and(|(read, _)| read.is_finished()) {
        return;
    }
    let Some((read, base)) = pending.0.take() else { return };
    let sources = match read.join() {
        Ok(sources) => sources,
        Err(_) => {
            diag::warn!(Launch, "menu sounds: their thread stopped; the menu keeps Black Ops II's own");
            (Vec::new(), NamespaceSoundIwd::default(), Vec::new())
        }
    };
    let (bank, iwd) = whole_menu_bank(base, sources);
    diag::info!(Launch, "menu sounds: read after the menu came up; the music may start");
    audio::swap_frontend_bank(&mut commands, bank, iwd, frontend.as_deref(), live.as_deref());
}

/// The front end's menu-music guest catalogs (BO1's, MW2's and MW3's
/// main-menu tracks) and the sound archives their streamed tracks play from:
/// BO1 from the games folder, MW2/MW3 from the voice-only one.
fn menu_sound_sources(
    games: &asset_transport::GamesRoot,
) -> MenuSoundSources {
    let (guests, mut lines) = asset_audio::menu_music_guests(games);
    let mut trees = asset_transport::NamespaceTrees::discover(games);
    if let Some(voice) = asset_audio::voice_games_root() {
        let voice = asset_transport::NamespaceTrees::discover(&voice);
        for ns in [asset_core::AssetNamespace::Iw4, asset_core::AssetNamespace::Iw5] {
            if trees.get(ns).is_none()
                && let Some(tree) = voice.get(ns)
            {
                trees.adopt_zone(&tree.anchor);
            }
        }
    }
    let (iwd, iwd_lines) = asset_transport::NamespaceSoundIwd::open(&trees);
    lines.extend(iwd_lines);
    (guests, iwd, lines)
}

/// Whether the drive holding `path` pays for each seek (a spinning hard
/// drive), as Windows reports it. False when it cannot tell.
/// Load speed: when a boot thread ran and what the program read
/// meanwhile, to set beside the front end's lines ("pictures in 36.9 s,
/// 1140 MB" counted every thread's reads).
struct ThreadSpan {
    from: f64,
    reads: Option<(u64, u64)>,
}

impl ThreadSpan {
    fn start() -> Self {
        Self {
            from: diag::process_elapsed_ns() as f64 / 1e9,
            reads: diag::process_reads(),
        }
    }

    /// "boot: menu-sounds thread from 2.1 s to 30.4 s, 1203 reads, 512.0 MB read (whole program)".
    fn end(self, name: &str) {
        let to = diag::process_elapsed_ns() as f64 / 1e9;
        let read = match (self.reads, diag::process_reads()) {
            (Some((ops, bytes)), Some((ops_now, bytes_now))) => format!(
                ", {} reads, {:.1} MB read (whole program)",
                ops_now.saturating_sub(ops),
                bytes_now.saturating_sub(bytes) as f64 / 1_048_576.0
            ),
            _ => String::new(),
        };
        diag::info!(Launch, "boot: {name} thread from {:.1} s to {to:.1} s{read}", self.from);
    }
}

fn drive_seeks_cost(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        unsafe extern "system" {
            fn DeviceIoControl(
                device: *mut std::ffi::c_void,
                code: u32,
                input: *const std::ffi::c_void,
                input_len: u32,
                output: *mut std::ffi::c_void,
                output_len: u32,
                returned: *mut u32,
                overlapped: *mut std::ffi::c_void,
            ) -> i32;
        }
        // IOCTL_STORAGE_QUERY_PROPERTY, asking StorageDeviceSeekPenaltyProperty.
        const QUERY_PROPERTY: u32 = 0x002d_1400;
        const SEEK_PENALTY_PROPERTY: u32 = 7;
        let Some(name) = volume_name(path) else {
            return false;
        };
        // No access asked: a volume's properties need no administrator.
        let volume = match std::fs::OpenOptions::new().access_mode(0).share_mode(3).open(&name) {
            Ok(volume) => volume,
            Err(e) => {
                diag::info!(Launch, "boot: drive of {} not asked ({name}: {e})", path.display());
                return false;
            }
        };
        // PropertyId, QueryType (standard), one byte of extra parameters.
        let query: [u32; 3] = [SEEK_PENALTY_PROPERTY, 0, 0];
        // Version, Size, IncursSeekPenalty (a byte).
        let mut answer = [0u32; 3];
        let mut returned = 0u32;
        // SAFETY: both buffers outlive the call and their lengths are theirs.
        let ok = unsafe {
            DeviceIoControl(
                volume.as_raw_handle(),
                QUERY_PROPERTY,
                query.as_ptr().cast(),
                std::mem::size_of_val(&query) as u32,
                answer.as_mut_ptr().cast(),
                std::mem::size_of_val(&answer) as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        ok != 0 && returned >= 9 && answer[2] & 0xff != 0
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// The volume `path` is on, as Windows opens it ("\\.\D:"; "\.\D:" with
/// one backslash is a relative path, and the drive was never asked).
#[cfg_attr(not(windows), allow(dead_code))]
fn volume_name(path: &std::path::Path) -> Option<String> {
    let path = std::path::absolute(path).ok()?;
    let Some(std::path::Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        std::path::Prefix::Disk(l) | std::path::Prefix::VerbatimDisk(l) => Some(format!(r"\\.\{}:", l as char)),
        _ => None,
    }
}

#[cfg(all(test, windows))]
mod volume_tests {
    use super::*;

    #[test]
    fn volume_names_are_device_paths() {
        assert_eq!(volume_name(std::path::Path::new(r"D:\games\zone")).as_deref(), Some(r"\\.\D:"));
        assert_eq!(volume_name(std::path::Path::new(r"\\?\C:\x")).as_deref(), Some(r"\\.\C:"));
        assert_eq!(volume_name(std::path::Path::new(r"\\server\share\x")), None);
    }

    #[test]
    fn the_programs_own_drive_opens() {
        use std::os::windows::fs::OpenOptionsExt;
        let exe = std::env::current_exe().unwrap();
        let name = volume_name(&exe).unwrap();
        std::fs::OpenOptions::new().access_mode(0).share_mode(3).open(&name).unwrap();
    }
}

/// The installed-content scan still running when the menu came up.
#[derive(Resource)]
struct PendingContentScan(Option<std::thread::JoinHandle<master_protocol::ContentFlags>>);

/// Hands the scan's flags to the master intent once its thread ends.
/// (The emptied resource stays: removing it made this system's next run fail
/// its parameter check and the game stopped, 2 s after the menu, whenever the
/// scan ended after the menu came up.)
fn apply_content_scan(pending: Option<ResMut<PendingContentScan>>, intent: Option<ResMut<net::MasterLaunchIntent>>) {
    let Some(mut pending) = pending else { return };
    if !pending.0.as_ref().is_some_and(std::thread::JoinHandle::is_finished) {
        return;
    }
    let Some(scan) = pending.0.take() else { return };
    match scan.join() {
        Ok(have) => {
            diag::info!(Launch, "content scan: done after the menu ({have:?})");
            if let Some(mut intent) = intent {
                intent.set_have(have);
            }
        }
        Err(_) => diag::warn!(Launch, "content scan: its thread stopped; no game content flags"),
    }
}

fn content_flags(trees: &NamespaceTrees, games: &asset_transport::GamesRoot) -> master_protocol::ContentFlags {
    let mut flags = net::content_inventory(
        trees.get(asset_core::AssetNamespace::Iw4).is_some(),
        trees.get(asset_core::AssetNamespace::Iw5).is_some(),
        trees.get(asset_core::AssetNamespace::T5).is_some(),
    );
    // bo2zm: Black Ops II.
    if trees.get(asset_core::AssetNamespace::T6).is_some() {
        flags.0 |= net::CONTENT_T6;
    }
    // bo2mp: Call of Duty 4 and World at War, when any of their maps is in.
    for (game, _) in asset_transport::list_legacy_mp_maps(games) {
        flags.0 |= match game.as_str() {
            "iw3" => net::CONTENT_IW3,
            "t4" => net::CONTENT_T4,
            _ => 0,
        };
    }
    flags
}

fn start_perf(zone: Option<String>, role: &str) {
    if let Err(error) = perf::start(perf::RunMetadata {
        zone,
        role: role.to_owned(),
        focus: codlegacy_env::var("CODLEGACY_PERF_FOCUS").ok(),
    }) {
        diag::error!(Launch, "perf: start failed: {error}");
    }
}

fn flush_perf() -> Option<PathBuf> {
    match perf::flush() {
        Ok(path) => path,
        Err(error) => {
            diag::error!(Launch, "perf: flush failed: {error}");
            None
        }
    }
}

fn queue_launch_capture(app: &mut App, request: CaptureRequest) {
    app.world_mut()
        .get_resource_mut::<CaptureQueue>()
        .expect("CaptureQueue: RenderPlugin must be added before a launch capture is queued")
        .push(request);
}

const fn role_name(role: Role) -> &'static str {
    match role {
        Role::Listen => "listen",
        Role::Dedicated => "dedicated",
        Role::Client => "client",
        Role::Replay => "replay",
    }
}

fn launch_present_mode(acceptance: bool) -> PresentMode {
    if acceptance {
        return ACCEPTANCE_PRESENT_MODE;
    }
    match codlegacy_env::var("CODLEGACY_PRESENT_MODE").ok().as_deref() {
        Some("AutoNoVsync") => PresentMode::AutoNoVsync,
        Some("Immediate") => PresentMode::Immediate,
        Some("Mailbox") => PresentMode::Mailbox,
        Some("Fifo") => PresentMode::Fifo,
        Some(other) => {
            diag::warn!(Launch, "unknown CODLEGACY_PRESENT_MODE={other}; using Fifo");
            PresentMode::default()
        }
        None => PresentMode::default(),
    }
}

/// `CODLEGACY_WINDOW_POS=x,y`: where a test run's window opens.
fn test_window_position() -> Option<bevy::window::WindowPosition> {
    let v = codlegacy_env::var("CODLEGACY_WINDOW_POS").ok()?;
    let (x, y) = v.split_once(',')?;
    Some(bevy::window::WindowPosition::At(bevy::math::IVec2::new(
        x.trim().parse().ok()?,
        y.trim().parse().ok()?,
    )))
}

/// `CODLEGACY_WINDOW_HIDDEN=1`: a test run's window stays hidden.
pub(crate) fn test_window_hidden() -> bool {
    codlegacy_env::var("CODLEGACY_WINDOW_HIDDEN").is_ok_and(|v| v == "1")
}

/// How the game's loop ended, in the run log (bevy quits with an error exit
/// on a fatal render error and says so only on stderr).
fn log_exit(exit: AppExit) {
    if exit.is_error() {
        diag::error!(Launch, "the game ended with an error exit ({exit:?}); the lines above say why");
    } else {
        diag::info!(Launch, "the game ended normally");
    }
}

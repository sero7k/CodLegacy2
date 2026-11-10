//! bo2zm M2: a Black Ops II map's guns, arms, animations, effects and
//! sound, from every zone the map loads.
//!
//! A Zombies map loads shared zones before its own (`code_post_gfx_zm`,
//! `common_zm`, `patch_zm`, ...) and its patch after. Each is walked once
//! and captured; later zones win a name they share with an earlier one
//! (the patch's weapons replace the map's). Comma-named assets are
//! references to another zone's and are skipped.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use asset_anim::XAnimBuild;
use asset_game::{FxCatalog, OwnedFxImpactTable, WeaponBuild, WeaponCatalog};
use asset_material::MaterialCatalog;
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use asset_t6::{PackSet, ZoneCapture};

/// The arms a Zombies map gives its players, by map (from the map's own
/// script: `zm_nuked.gsc` sets `c_zom_suit_viewhands` for the CIA player and
/// `c_zom_hazmat_viewhands_light` for CDC).
pub const T6_PLAYER_ARMS: &str = "c_zom_suit_viewhands";
const T6_OTHER_ARMS: [&str; 1] = ["c_zom_hazmat_viewhands_light"];

/// Zones a Zombies map loads before its own, in order.
const T6_ZM_PRE_ZONES: [&str; 4] = ["code_post_gfx_zm", "common_zm", "patch_zm", "patch_ui_zm"];

/// bo2mp: zones a multiplayer map loads before its own, in order (later
/// zones win a name: `common_patch_mp`'s guns replace `common_mp`'s, the
/// patch's scripts and tables everything before it). Then the map's two
/// factions (`faction_<name>_mp`, from `mp/mapstable.csv`).
pub(super) const T6_MP_PRE_ZONES: [&str; 6] = [
    "code_post_gfx_mp",
    "common_mp",
    "common_patch_mp",
    "patch_mp",
    "ui_mp",
    "patch_ui_mp",
];

/// bo2mp: the English text and voice zones a multiplayer map reads
/// (`zone/english/en_<zone>.ff`), the factions' and map's added after.
const T6_MP_LANG_ZONES: [&str; 5] = [
    "code_post_gfx_mp",
    "common_mp",
    "patch_mp",
    "ui_mp",
    "patch_ui_mp",
];

/// bo2mp: a Black Ops II map is multiplayer (`mp_*`) or Zombies (`zm_*`).
pub(crate) fn is_mp_map(map: &str) -> bool {
    map.starts_with("mp_")
}

pub(crate) struct T6Combat {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world_weapons: WorldWeaponBuild,
    pub xanims: XAnimBuild,
    pub fx: FxCatalog,
    /// The models effects draw (shell casings, debris).
    pub fx_models: asset_game::FxModelCatalog,
    /// The tracers every zone defines (each weapon names its own).
    pub tracers: asset_game::TracerCatalog,
    pub impact_fx: Option<OwnedFxImpactTable>,
    pub sound: Result<asset_audio::SoundCatalog, String>,
    /// The map's footstep tables (first person first), alias ids per
    /// surface and step kind.
    pub footsteps: Vec<asset_t6::FootstepTableRef>,
    /// The default ambient room's tone, from the map's ambience script.
    pub room_tone: Option<String>,
    /// bo2zm M3: every zone's compiled scripts, string tables and entity
    /// strings, in load order.
    pub scripts: crate::T6ScriptSet,
    /// bo2zm M3: the Zombies HUD's icons.
    pub hud_icons: crate::T6HudIcons,
    /// bo2zm M3: the Zombies HUD's fonts.
    pub hud_fonts: crate::T6HudFonts,
    /// bo2zm M4: Black Ops II's UI scripts and what they read.
    pub ui: crate::T6Ui,
    /// bo2zm: the zones' bullet penetration table.
    pub pen_table: Option<weapon_iw4::PenetrationDepthTable>,
    /// bo2zm: every zone's vision files (`vision/<name>.vision`), later
    /// zones replacing earlier ones: the visions a script turns on
    /// (Mob's Afterlife, `zm_afterlife` in its mode zone).
    pub visions: Vec<(String, Vec<u8>)>,
    /// bo2zm: the full-screen filters the scripts turn on (Mob's Afterlife).
    pub screen_filters: crate::T6ScreenFilters,
    /// bo2mp: the camos the walk installed, kept so a cache hit installs
    /// the same ones without the walk.
    pub camos: T6Camos,
    pub report: Vec<String>,
}

/// bo2mp: what the combat walk hands `asset_core::weapon_camos`: the
/// map's camo table, Black Ops' and the player's own pattern camos, and
/// the Pack-a-Punch camo (catalog indices, as installed).
#[derive(Default)]
pub(crate) struct T6Camos {
    pub table: Option<asset_core::weapon_camos::CamoTable>,
    pub extra: HashMap<u32, asset_core::weapon_camos::PatternCamo>,
    pub pap: Option<(u32, asset_core::weapon_camos::MaterialCamo)>,
}

impl T6Camos {
    /// Installs them for the draw (the live walk and a cache hit alike).
    pub fn install(&self) {
        asset_core::weapon_camos::install(self.table.clone());
        asset_core::weapon_camos::install_extra_patterns(self.extra.clone());
        asset_core::weapon_camos::install_pap(self.pap);
    }
}

/// The Zombies HUD's icons: the perks a Nuketown machine sells and the
/// power-ups that last a while (their materials' colour maps).
const HUD_ICONS: [&str; 51] = [
    "specialty_juggernaut_zombies",
    "specialty_quickrevive_zombies",
    "specialty_fastreload_zombies",
    "specialty_doubletap_zombies",
    "specialty_instakill_zombies",
    "specialty_doublepoints_zombies",
    "specialty_firesale_zombies",
    // The round's chalk marks, rounds 1 to 5 (hudroundstatuszombie.lua).
    "hud_chalk_1",
    "hud_chalk_2",
    "hud_chalk_3",
    "hud_chalk_4",
    "hud_chalk_5",
    // The grenades and mines he carries, one icon each (offhandicons.lua):
    // frag, Semtex, monkey, Claymore.
    "hud_us_grenade",
    "hud_icon_sticky_grenade",
    "hud_cymbal_monkey",
    "hud_icon_claymore",
    // The red screen-edge overlay the health script fades in when he is hit
    // (_zm_playerhealth's healthoverlay).
    "overlay_low_health",
    // The guns' hip-fire crosshair: its four lines and the centre pieces
    // some guns name (weapon defs' reticleSide / reticleCenter).
    "reticle_side_small",
    "reticle_center_cross",
    "reticle_flechette",
    // The orange YOU pin the killcam draws over the player who was killed.
    "headiconyouinkillcam",
    // Mob's Afterlife hands' centre mark (its weapon's reticleCenter, in
    // zm_prison.ff).
    "afterlife_reticle",
    // The scoreboard's team badges ("faction_" .. GetFactionForTeam): the
    // CDC and CIA (Nuketown's teamset).
    "faction_cdc",
    "faction_cia",
    // Nuketown's desert map behind its start locations: GameMapZombie
    // builds the name ("menu_" .. GetUIMapName() .. "_map", and "_blur"),
    // so no script names it whole.
    "menu_zm_nuked_map",
    "menu_zm_nuked_map_blur",
    // BO2's Xbox button pictures (code_post_gfx_zm): the pad's prompts in
    // its menus and hints (frame::BO2_XBOX_GLYPHS).
    "xenonbutton_a",
    "xenonbutton_b",
    "xenonbutton_x",
    "xenonbutton_y",
    "xenonbutton_lb",
    "xenonbutton_rb",
    "xenonbutton_lt",
    "xenonbutton_rt",
    "xenonbutton_back",
    "xenonbutton_start",
    "xenonbutton_ls",
    "xenonbutton_rs",
    "xenonbutton_dpad_all",
    "xenonbutton_dpad_ud",
    "xenonbutton_dpad_rl",
    "xenonbutton_dpad_up",
    "xenonbutton_dpad_down",
    "xenonbutton_dpad_left",
    "xenonbutton_dpad_right",
    // BO2 on PC: its menu tabs' arrows (frame::bo2_cycle_glyph).
    "ui_arrow_left",
    "ui_arrow_right",
    // The lobby rows' speaker (the engine's VoipImage, code_post_gfx_zm):
    // muted, and talking (its left half is the quiet one).
    "voice_off",
    "voice_on",
    // The build progress bar (_hud_util createbar: its back and its fill;
    // Buried's buildables, any table map).
    "progress_bar_bg",
    "progress_bar_fill",
];

/// The HUD's fonts by the Lua scripts' names and their files, as
/// `ui/t6/codbase.lua` registers them for English.
const HUD_FONTS: [(&str, &str); 7] = [
    ("Default", "fonts/720/normalFont"),
    ("Condensed", "fonts/720/smallFont"),
    ("Big", "fonts/720/bigFont"),
    ("Morris", "fonts/720/extraBigFont"),
    ("ExtraSmall", "fonts/720/extraSmallFont"),
    ("Italic", "fonts/720/italicFont"),
    ("SmallItalic", "fonts/720/smallItalicFont"),
];

pub(super) fn hud_fonts(
    captures: &[ZoneCapture],
    packs: Option<&PackSet>,
    report: &mut Vec<String>,
) -> crate::T6HudFonts {
    let mut out = crate::T6HudFonts::default();
    let mut sheet_material = None;
    for (name, file) in HUD_FONTS {
        let Some(font) = captures
            .iter()
            .rev()
            .flat_map(|c| c.fonts.iter())
            .find(|f| f.name.eq_ignore_ascii_case(file))
        else {
            report.push(format!("t6 hud font {name}: {file} missing"));
            continue;
        };
        sheet_material.get_or_insert_with(|| font.material.clone());
        out.fonts.push(crate::T6HudFont {
            name: name.to_owned(),
            pixel_height: font.pixel_height as f32,
            glyphs: font
                .glyphs
                .iter()
                .map(|g| {
                    (
                        g.letter,
                        [
                            i16::from(g.x0),
                            i16::from(g.y0),
                            i16::from(g.dx),
                            i16::from(g.pixel_width),
                            i16::from(g.pixel_height),
                        ],
                        g.st,
                    )
                })
                .collect(),
        });
    }
    // Every one of them draws from the same sheet (`gamefonts_pc`).
    if let (Some(material), Some(packs)) = (sheet_material, packs) {
        let image = captures.iter().find_map(|c| {
            let m = c.materials.iter().find(|m| m.name == material)?;
            let t = m.textures.first()?;
            c.images.get(t.image?.index)
        });
        match image.map(|i| super::t6_materials::decode(packs, i)) {
            Some(Ok(img)) => out.sheet = Some(std::sync::Arc::new(img)),
            Some(Err(e)) => report.push(format!("t6 hud font sheet {material}: {e}")),
            None => report.push(format!("t6 hud font sheet {material}: no image")),
        }
    }
    report.push(format!(
        "t6 hud fonts: {} ({})",
        out.fonts.len(),
        out.fonts
            .iter()
            .map(|f| format!("{} {}px {} glyphs", f.name, f.pixel_height, f.glyphs.len()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out
}

/// bo2mp: the zones' config files (`*.cfg`) as text, by name (a later
/// zone's wins).
pub(super) fn ui_configs(captures: &[&ZoneCapture]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, bytes) in captures.iter().flat_map(|c| c.raw_files.iter()) {
        if !name.to_ascii_lowercase().ends_with(".cfg") {
            continue;
        }
        let text = String::from_utf8_lossy(bytes).trim_end_matches('\0').to_owned();
        out.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        out.push((name.clone(), text));
    }
    out
}

/// bo2zm M4: the zones Black Ops II's Zombies front end reads (main menu,
/// lobby, globe and map picker), in load order; each map pack's load zone
/// adds its map pictures and loading screen (Nuketown, then the four DLC
/// packs: every Zombies map is on the globe).
const T6_ZM_FRONTEND_ZONES: [&str; 10] = [
    "code_post_gfx_zm",
    "common_zm",
    "patch_zm",
    "ui_zm",
    "patch_ui_zm",
    "dlczm0_load_zm",
    "dlc1_load_zm",
    "dlc2_load_zm",
    "dlc3_load_zm",
    "dlc4_load_zm",
];

/// bo2zm M4: Black Ops II's Zombies front end without a map: its UI
/// scripts, the pictures they name, the fonts, English strings and string
/// tables. Kept for the whole run: the menu comes back after each game.
#[derive(Clone, bevy::prelude::Resource)]
pub struct T6Frontend {
    pub ui: crate::T6Ui,
    pub icons: crate::T6HudIcons,
    pub fonts: crate::T6HudFonts,
    /// Its sounds: the menus' clicks and music (`zmb_code_post_gfx`).
    pub sound: Option<std::sync::Arc<asset_audio::SoundCatalog>>,
    pub report: Vec<String>,
}

/// bo2zm M4: read the front end from the zones in `zone_dir`
/// (`zone/all`; the English zones are in `zone/english` beside it).
pub fn load_t6_frontend(zone_dir: &Path) -> T6Frontend {
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let capture_all = |dir: &Path, prefix: &str, report: &mut Vec<String>| -> Vec<ZoneCapture> {
        T6_ZM_FRONTEND_ZONES
            .iter()
            .filter_map(|z| {
                let path = dir.join(format!("{prefix}{z}.ff"));
                if !path.is_file() {
                    return None;
                }
                match asset_t6::capture_zone(&path) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        report.push(format!("t6 front end zone gap: {e}"));
                        None
                    }
                }
            })
            .collect()
    };
    let caps = capture_all(zone_dir, "", &mut report);
    // A Black Ops map pack in an extra folder (`CODLEGACY_T6_EXTRA`): its load
    // zone carries those maps' globe rows, words and pictures. Only those
    // are taken; its own menu scripts are not (BO2's globe stays BO2's).
    let pack_load = asset_transport::t6_extra::extra_file("mod_load.ff").and_then(|path| {
        asset_t6::capture_zone(&path)
            .map_err(|e| report.push(format!("t6 front end map pack gap: {e}")))
            .ok()
    });
    let lang_dir = zone_dir.parent().map(|d| d.join("english"));
    let lang_caps = lang_dir
        .as_deref()
        .map(|d| capture_all(d, "en_", &mut report))
        .unwrap_or_default();
    let captures: Vec<&ZoneCapture> = caps.iter().collect();
    report.push(format!(
        "t6 front end zones: {}; English: {}",
        captures
            .iter()
            .map(|c| c.zone.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        lang_caps
            .iter()
            .map(|c| c.zone.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let packs = match PackSet::open_dir(zone_dir) {
        Ok(p) => Some(p),
        Err(e) => {
            report.push(format!("t6 front end image packs: {e}"));
            None
        }
    };
    let mut strings: HashMap<String, String> = HashMap::new();
    for c in &lang_caps {
        for (k, v) in &c.localize {
            strings.insert(k.clone(), v.clone());
        }
    }
    let mut tables = Vec::new();
    for c in &captures {
        for t in &c.string_tables {
            if real(&t.name) {
                tables.push((t.name.clone(), t.columns, t.rows, t.cells.clone()));
            }
        }
    }
    if let Some(pack) = &pack_load {
        // Its words where BO2 has none (the maps' names and starts).
        for (k, v) in &pack.localize {
            strings.entry(k.clone()).or_insert_with(|| v.clone());
        }
        let added = add_pack_maps(&mut tables, pack);
        report.push(format!("t6 front end map pack maps: {}", added.join(" ")));
    }
    let read_ms = started.elapsed().as_millis();
    let scripts = ui_scripts(&captures);
    let mut names = ui_strings(&scripts);
    // Every picture the front-end zones hold: the menus also name them
    // from tables and joined strings (`menu_` .. map .. `_map`, the maps
    // table's signposts), which no script constant spells out.
    for c in captures
        .iter()
        .filter(|c| matches!(c.zone.as_str(), "ui_zm" | "patch_ui_zm") || c.zone.ends_with("_load_zm"))
    {
        names.extend(
            c.materials
                .iter()
                .filter(|m| real(&m.name))
                .map(|m| m.name.clone()),
        );
    }
    // TranZit's loading pictures (each start and mode) sit in
    // code_post_gfx_zm; BO2's loading script joins their names.
    for c in captures.iter().filter(|c| c.zone == "code_post_gfx_zm") {
        names.extend(
            c.materials
                .iter()
                .filter(|m| real(&m.name) && m.name.starts_with("loadscreen_"))
                .map(|m| m.name.clone()),
        );
    }
    // And the pictures string tables name (rank icons, map signposts).
    for (_, _, _, cells) in &tables {
        names.extend(
            cells
                .iter()
                .filter(|c| !c.is_empty() && c.len() < 64)
                .cloned(),
        );
    }
    // The pack's pictures (signposts, map pictures, loading screens).
    let mut icon_caps = captures.clone();
    if let Some(pack) = &pack_load {
        names.extend(
            pack.materials
                .iter()
                .filter(|m| real(&m.name))
                .map(|m| m.name.clone()),
        );
        icon_caps.push(pack);
    }
    let mut icons = hud_icons(&icon_caps, packs.as_ref(), &names, &mut report, IconShare::Off);
    let icons_ms = started.elapsed().as_millis();
    let lang_packs = lang_dir.as_deref().and_then(|d| PackSet::open_dir(d).ok());
    // Pictures with English words on them (the globe's green "NEW" tag)
    // are in the English zones; their pixels in either folder's packs.
    let lang_refs: Vec<&ZoneCapture> = lang_caps.iter().collect();
    let mut found_lang = Vec::new();
    let mut lang_report = Vec::new();
    for p in [packs.as_ref(), lang_packs.as_ref()].into_iter().flatten() {
        let have: std::collections::BTreeSet<String> =
            icons.0.iter().map(|(n, _)| n.clone()).collect();
        let lang_names: std::collections::BTreeSet<String> =
            names.iter().filter(|n| !have.contains(*n)).cloned().collect();
        lang_report.clear();
        let more = hud_icons(&lang_refs, Some(p), &lang_names, &mut lang_report, IconShare::Off);
        for (n, img) in more.0 {
            if !have.contains(&n) {
                found_lang.push(n.clone());
                icons.0.push((n, img));
            }
        }
    }
    report.push(format!("t6 front end English pictures: {}", found_lang.join(", ")));
    report.extend(lang_report.into_iter().map(|l| format!("English: {l}")));
    let fonts = hud_fonts(
        &lang_caps,
        lang_packs.as_ref().or(packs.as_ref()),
        &mut report,
    );
    // The menus' sounds, from the same banks a map reads them from.
    let sound_dir = zone_dir
        .parent()
        .and_then(Path::parent)
        .map(|install| install.join("sound"));
    let sound = sound_dir.filter(|d| d.is_dir()).map(|dir| {
        let bases = bank_bases(&captures, "");
        let base_refs: Vec<&str> = bases.iter().map(String::as_str).collect();
        let (index, bank_report) = asset_audio::T6BankIndex::open(&dir, &base_refs);
        report.extend(bank_report);
        let banks: Vec<&asset_t6::SndBankRef> = captures
            .iter()
            .copied()
            .chain(lang_caps.iter())
            .flat_map(|c| c.sound_banks.iter())
            .collect();
        let globals = captures.iter().find_map(|c| c.snd_globals.as_ref());
        let (catalog, census) = asset_audio::build_t6_sound_catalog(
            &banks,
            globals,
            &index,
            asset_core::ZoneOwner::intern("frontend"),
        );
        report.push(format!(
            "t6 front end sound: {} aliases, {} with a clip",
            census.aliases, census.with_clip
        ));
        std::sync::Arc::new(catalog)
    });
    report.push(format!(
        "t6 front end: {} scripts, {} strings, {} string tables; zones read in {read_ms} ms, pictures by {icons_ms} ms, all in {} ms",
        scripts.len(),
        strings.len(),
        tables.len(),
        started.elapsed().as_millis()
    ));
    T6Frontend {
        ui: crate::T6Ui {
            scripts,
            strings: std::sync::Arc::new(strings.into_iter().collect()),
            tables: std::sync::Arc::new(tables),
            configs: std::sync::Arc::new(ui_configs(&captures)),
            game_settings: std::sync::Arc::new(zm_game_settings(&captures)),
        },
        icons,
        fonts,
        sound,
        report,
    }
}

/// bo2zm: the maps a map pack's load zone adds to the globe: its maps table
/// rows for maps BO2's table lacks, and those maps' start locations and
/// modes from its gametypes table (renumbered after BO2's own). Returns the
/// maps added.
fn add_pack_maps(
    tables: &mut [(String, usize, usize, Vec<String>)],
    pack: &ZoneCapture,
) -> Vec<String> {
    let pack_rows = |name: &str| -> Vec<Vec<String>> {
        pack.string_tables
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.cells.chunks(t.columns.max(1)).map(<[String]>::to_vec).collect())
            .unwrap_or_default()
    };
    fn cell(r: &[String], i: usize) -> &str {
        r.get(i).map_or("", String::as_str)
    }
    let mut added = Vec::new();
    // Append rows (fitted to the table's columns) to a table.
    let append = |table: &mut (String, usize, usize, Vec<String>), rows: Vec<Vec<String>>| {
        let cols = table.1.max(1);
        for mut r in rows {
            r.resize(cols, String::new());
            table.3.extend(r);
            table.2 += 1;
        }
    };
    let Some(maps) = tables.iter().position(|t| t.0 == "zm/mapstable.csv") else {
        return added;
    };
    let have: BTreeSet<String> = tables[maps]
        .3
        .chunks(tables[maps].1.max(1))
        .map(|r| cell(r, 0).to_owned())
        .collect();
    let new_maps: Vec<Vec<String>> = pack_rows("zm/mapstable.csv")
        .into_iter()
        .filter(|r| cell(r, 0).starts_with("zm_") && !have.contains(cell(r, 0)))
        .collect();
    added.extend(new_maps.iter().map(|r| cell(r, 0).to_owned()));
    append(&mut tables[maps], new_maps);
    if let Some(gt) = tables.iter().position(|t| t.0 == "zm/gametypestable.csv") {
        let cols = tables[gt].1.max(1);
        let next = |cat: &str| {
            tables[gt]
                .3
                .chunks(cols)
                .filter(|r| cell(r, 0) == cat)
                .filter_map(|r| cell(r, 1).parse::<i64>().ok())
                .max()
                .map_or(0, |m| m + 1)
        };
        let (mut loc_id, mut mode_id) = (next("5"), next("6"));
        let rows: Vec<Vec<String>> = pack_rows("zm/gametypestable.csv")
            .into_iter()
            .filter(|r| matches!(cell(r, 0), "5" | "6") && added.iter().any(|m| m == cell(r, 2)))
            .map(|mut r| {
                let id = if r[0] == "5" { &mut loc_id } else { &mut mode_id };
                r[1] = id.to_string();
                *id += 1;
                r
            })
            .collect();
        append(&mut tables[gt], rows);
    }
    added
}

/// bo2zm M4: Zombies' game settings defaults, from the zones' rawfiles
/// (`gametype_setting <name> <value>` in the default settings file, then
/// the standard mode's).
fn zm_game_settings(captures: &[&ZoneCapture]) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = Vec::new();
    for file in ["gamesettings_default.cfg", "gamesettings_zstandard.cfg"] {
        let Some(bytes) = captures
            .iter()
            .rev()
            .flat_map(|c| c.raw_files.iter())
            .find(|(n, _)| {
                n.to_ascii_lowercase().ends_with(file) && n.to_ascii_lowercase().contains("zm")
            })
            .map(|(_, b)| b.clone())
        else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let mut w = line.split_whitespace();
            if w.next() != Some("gametype_setting") {
                continue;
            }
            let (Some(name), Some(value)) =
                (w.next(), w.next().and_then(|v| v.parse::<f32>().ok()))
            else {
                continue;
            };
            out.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
            out.push((name.to_owned(), value));
        }
    }
    out
}

/// bo2zm M4: every UI script (`.lua` rawfile) the zones hold, in load
/// order.
pub(super) fn ui_scripts(captures: &[&ZoneCapture]) -> Vec<(String, std::sync::Arc<[u8]>)> {
    captures
        .iter()
        .flat_map(|c| c.raw_files.iter())
        .filter(|(name, _)| name.to_ascii_lowercase().ends_with(".lua"))
        .map(|(name, bytes)| (name.clone(), std::sync::Arc::from(bytes.as_slice())))
        .collect()
}

/// bo2zm M4: the material names the UI scripts may draw: every string
/// constant in them (`RegisterMaterial("hud_chalk_1")`).
pub(super) fn ui_strings(scripts: &[(String, std::sync::Arc<[u8]>)]) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (_, bytes) in scripts {
        let Ok(chunk) = hks_t6::parse(bytes) else {
            continue;
        };
        let mut stack = vec![&chunk.main];
        while let Some(p) = stack.pop() {
            for k in &p.consts {
                if let hks_t6::Const::String(s) = k
                    && s.len() < 64
                {
                    out.insert(String::from_utf8_lossy(s).into_owned());
                }
            }
            stack.extend(p.protos.iter().map(|c| &**c));
        }
    }
    out
}

/// bo2zm: every `generic_filter_*` material the zones hold (patch zones
/// first: their copy wins), its pictures decoded and its constants by name.
fn screen_filters(
    captures: &[&ZoneCapture],
    packs: Option<&PackSet>,
    report: &mut Vec<String>,
) -> crate::T6ScreenFilters {
    let mut out: Vec<crate::T6ScreenFilter> = Vec::new();
    let Some(packs) = packs else {
        return crate::T6ScreenFilters(out);
    };
    let patched = captures.iter().filter(|c| c.zone.starts_with("patch_"));
    let rest = captures.iter().filter(|c| !c.zone.starts_with("patch_"));
    for c in patched.chain(rest) {
        for m in &c.materials {
            if !m.name.starts_with("generic_filter_") || out.iter().any(|f| f.material == m.name) {
                continue;
            }
            let mut images = Vec::new();
            for t in &m.textures {
                let Some(image) = t.image.and_then(|k| c.images.get(k.index)) else {
                    continue;
                };
                match super::t6_materials::decode(packs, image) {
                    Ok(img) => images.push((image.name.clone(), std::sync::Arc::new(img))),
                    Err(e) => report.push(format!("t6 screen filter {}: {}: {e}", m.name, image.name)),
                }
            }
            out.push(crate::T6ScreenFilter {
                material: m.name.clone(),
                images,
                constants: m.constants.iter().map(|(_, n, v)| (n.clone(), *v)).collect(),
            });
        }
    }
    report.push(format!(
        "t6 screen filters: {}",
        out.iter()
            .map(|f| format!("{} ({} pictures)", f.material, f.images.len()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    crate::T6ScreenFilters(out)
}

/// bo2mp: the pictures' bytes read once in disk order, the head sweeping
/// each pack, so the decodes after it (in name order) find them in the
/// system's file cache. Read in name order, his hard drive sought for most
/// of a cold front end's 83 s.
fn warm_in_disk_order(captures: &[&ZoneCapture], packs: &PackSet, names: &[String], report: &mut Vec<String>) -> usize {
    // (The real index entry, key and all: the cache files each entry under
    // its key, so a copy made under key 0 was one no read ever asked for.)
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = names
        .iter()
        .filter_map(|name| {
            let patched = captures.iter().filter(|c| c.zone.starts_with("patch_"));
            let rest = captures.iter().filter(|c| !c.zone.starts_with("patch_"));
            let image = patched.chain(rest).find_map(|c| {
                let m = c.materials.iter().find(|m| m.name == *name)?;
                let t = super::t6_materials::colour_texture(m, c)?;
                c.images.get(t.image?.index)
            })?;
            match packs.locate(image) {
                Some(asset_t6::ImageSource::Pack(i, e)) => Some((i, e)),
                _ => None,
            }
        })
        .collect();
    entries.sort_unstable_by_key(|&(i, e)| (i, e.offset, e.size));
    entries.dedup_by_key(|&mut (i, e)| (i, e.offset, e.size));
    // A front end's many pictures as one bundle file (named after its zones).
    if entries.len() >= 64 {
        let zones = captures.iter().map(|c| c.zone.as_str()).collect::<Vec<_>>().join(",");
        let label = format!("{:016x}", asset_transport::fnv1a64(zones.as_bytes()));
        let started = std::time::Instant::now();
        let bundled = packs.preload(&label, &entries);
        if bundled == 0 {
            // Load speed: the bundle written now (or the cache off), its reads
            // with the gaps they read and dropped, in both read plans.
            let (reads, read) = packs.bundle_reads(&entries, false);
            let (exact_reads, exact_read) = packs.bundle_reads(&entries, true);
            let needed: u64 = entries.iter().map(|(_, e)| e.size as u64).sum();
            report.push(format!(
                "t6 hud icons: {} pictures not from a bundle, {} of them taken from an earlier bundle file and the rest read from the packs (all of them, shared plan: {reads} reads, {:.1} MB; own plan: {exact_reads} reads, {:.1} MB; read with the {} plan; for {:.1} MB), in {:.1} s",
                entries.len(),
                asset_t6::images::bundle_entries_reused(),
                read as f64 / 1_048_576.0,
                exact_read as f64 / 1_048_576.0,
                if asset_t6::images::bundle_exact_read_on() { "own" } else { "shared" },
                needed as f64 / 1_048_576.0,
                started.elapsed().as_secs_f64()
            ));
        }
        return bundled;
    }
    for (i, e) in entries {
        let _ = packs.packs[i].warm(e);
    }
    0
}

/// How many threads decode the front end's pictures: up to 8, or
/// `CODLEGACY_T6_ICON_THREADS` (1 = all on the load thread, the old way).
fn icon_threads(pictures: usize) -> usize {
    if pictures < 64 {
        return 1;
    }
    match std::env::var("CODLEGACY_T6_ICON_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(n) => n.clamp(1, 16),
        None => std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8),
    }
}

/// One front-end picture, decoded: its entries for `T6HudIcons` (the
/// picture and any copies made from it), its report lines, and why it has
/// no picture.
#[derive(Default, Clone)]
struct HudIconOut {
    icons: Vec<(String, std::sync::Arc<bevy::prelude::Image>)>,
    report: Vec<String>,
    failed: Option<String>,
    additive: bool,
}

fn hud_icon(captures: &[&ZoneCapture], packs: &PackSet, name: &str) -> HudIconOut {
    let mut out = HudIconOut::default();
    let mut add = false;
    let mut add_map = None;
    // A patch zone's copy replaces the one it patches (`patch_ui_zm`'s
    // Theater frame is silver with light cracks, `ui_zm`'s dull).
    let patched = captures.iter().filter(|c| c.zone.starts_with("patch_"));
    let rest = captures.iter().filter(|c| !c.zone.starts_with("patch_"));
    let found = patched.chain(rest).find_map(|c| {
        let m = c.materials.iter().find(|m| m.name == name)?;
        add = super::t6_materials::is_additive(m);
        add_map = super::t6_materials::add_map(m, c);
        // BO2MP_ICON_STATES=<substring>: print the matching pictures' draw states (debugging aid).
        if let Ok(sub) = std::env::var("BO2MP_ICON_STATES")
            && name.contains(sub.as_str())
        {
            for t in 0..36 {
                if let Some(d) = m.draw_state(t) {
                    eprintln!("icon-state {name} tech {t}: src {} dst {} op {} atest {:?} rgb {} a {}", d.src_blend, d.dst_blend, d.blend_op, d.alpha_test, d.color_write_rgb, d.color_write_alpha);
                }
            }
        }
        let t = super::t6_materials::colour_texture(m, c)?;
        c.images.get(t.image?.index)
    });
    let Some(image) = found else {
        if HUD_ICONS.contains(&name) {
            out.report.push(format!("t6 hud icon {name}: no material"));
        }
        out.failed = Some(format!("{name} (no colour map)"));
        return out;
    };
    // A `,name` image is a reference to one another zone defines (the
    // Create-a-Class perk pictures `perk_*_256` in ui_mp): its pixels
    // are found in the packs by the name alone.
    let decoded = super::t6_materials::decode(packs, image).or_else(|e| {
        if !image.name.starts_with(',') {
            return Err(e);
        }
        let (i, entry) = packs.locate_name(&image.name).ok_or(e)?;
        let bytes = packs.packs[i].read(entry)?;
        let iwi = ipak_t6::parse_iwi(&bytes).map_err(|e| e.to_string())?;
        super::t6_materials::gpu_image(&iwi)
    });
    match decoded {
        Ok(img) => {
            // BO2MP_ICON_DUMP=<dir>:<substring> writes the matching
            // pictures there as PNGs, as stored (a debugging aid).
            if let Ok(spec) = std::env::var("BO2MP_ICON_DUMP")
                && let Some((dir, sub)) = spec.rsplit_once('|')
                && name.contains(sub)
                && let Some(plain) = super::t6_materials::to_rgba8(&img)
                && let Some(data) = plain.data.as_ref()
                && let Ok(file) = std::fs::File::create(format!("{dir}/{name}{}.png", if add { "_ADD" } else { "" }))
            {
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), plain.width(), plain.height());
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                if let Ok(mut w) = enc.write_header() {
                    let _ = w.write_image_data(data);
                }
            }
            // bo2mp: the HUD adds these onto the scene (true additive
            // blending), so keep the pictures as stored under
            // "additive:<name>" next to the see-through copy.
            // bo2mp vehicle screens: a picture with an AddMap (BO2's
            // `sw4_2d_color_add` pixel shader: the colour map's alpha
            // times the element's colour, plus the AddMap's colour, all
            // times its alpha) is kept packed (AddMap colour, colour
            // map alpha) under "additive:" and marked "coloradd:".
            let packed = add_map
                .and_then(|i| super::t6_materials::decode(packs, i).ok())
                .and_then(|added| super::t6_materials::pack_color_add(&img, &added));
            let img = match packed {
                Some((packed, flat)) => {
                    out.icons.push((format!("coloradd:{name}"), std::sync::Arc::new(packed.clone())));
                    if add {
                        out.icons.push((format!("additive:{name}"), std::sync::Arc::new(packed)));
                    }
                    flat
                }
                None => {
                    if add && let Some(orig) = super::t6_materials::to_rgba8(&img) {
                        out.icons.push((format!("additive:{name}"), std::sync::Arc::new(orig)));
                    }
                    img
                }
            };
            // Additive pictures (menu brackets, glows) get their
            // brightness as alpha: the 2D layer only alpha-blends.
            let img = match add.then(|| super::t6_materials::additive_to_alpha(&img)).flatten() {
                Some(a) => {
                    out.additive = true;
                    a
                }
                // bo2mp: the minimap's picture as plain pixels (the
                // HUD turns it about him on the CPU).
                None if name.starts_with("compass_map_") => super::t6_materials::to_rgba8(&img).unwrap_or(img),
                None => img,
            };
            // The lobby's quiet speaker: BO2 draws `voice_on` without
            // its sound waves while nobody talks.
            if name == "voice_on"
                && let Some(quiet) = super::t6_materials::left_part(&img, 0.5)
            {
                out.icons.push(("voice_quiet".to_owned(), std::sync::Arc::new(quiet)));
            }
            // bo2mp: the default emblem on a player's card (BO2 draws the
            // rank's emblem layer, `em_rank_pvt_full`, in the rank's
            // green): the PFC chevron as a white silhouette the
            // card tints.
            if name == "rank_pfc"
                && let Some(mut sil) = super::t6_materials::to_rgba8(&img)
                && let Some(data) = sil.data.as_mut()
            {
                for px in data.chunks_exact_mut(4) {
                    px[0] = 255;
                    px[1] = 255;
                    px[2] = 255;
                }
                out.icons.push(("em_rank_pvt_full".to_owned(), std::sync::Arc::new(sil)));
            }
            out.icons.push((name.to_owned(), std::sync::Arc::new(img)));
        }
        Err(e) => {
            if HUD_ICONS.contains(&name) {
                out.report.push(format!("t6 hud icon {name}: {e}"));
            }
            out.failed = Some(format!("{name} ({e})"));
        }
    }
    out
}

/// Whether a `hud_icons` call keeps its decoded pictures for the next map
/// (the front end), takes the ones kept (a map), or neither.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IconShare {
    Keep,
    Take,
    Off,
}

/// A HUD picture's material name, its colour picture's, whether it adds,
/// and its AddMap's picture.
type IconKey = (String, String, bool, String);

/// Map load speed and memory: the front end's decoded HUD pictures, by
/// material name and the picture it draws. A map's census asked for about
/// 2,500 HUD pictures and read and decoded every one again (1.4 s of a cold
/// Village's load), the same pictures the menu had decoded: now each one
/// the menu has is the menu's own (shared, not a second copy).
/// `BO2MP_T6_SHARE_HUD_ICONS=1` turns it on (off by default until measured).
fn shared_icons() -> &'static Mutex<HashMap<IconKey, HudIconOut>> {
    static SHARED: OnceLock<Mutex<HashMap<IconKey, HudIconOut>>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `share`, unless `BO2MP_T6_SHARE_HUD_ICONS=1` is not set (or a picture
/// dump is asked for, which decodes each one).
fn icon_share(share: IconShare) -> IconShare {
    let on = codlegacy_env::var("BO2MP_T6_SHARE_HUD_ICONS").is_ok_and(|v| v.trim() == "1");
    if on && std::env::var_os("BO2MP_ICON_DUMP").is_none() { share } else { IconShare::Off }
}

/// What `hud_icon` would decode for `name`: the material's name, its
/// colour picture's, whether it adds, and its AddMap's picture (the same
/// zone order `hud_icon` takes).
fn icon_key(captures: &[&ZoneCapture], name: &str) -> Option<IconKey> {
    let patched = captures.iter().filter(|c| c.zone.starts_with("patch_"));
    let rest = captures.iter().filter(|c| !c.zone.starts_with("patch_"));
    patched.chain(rest).find_map(|c| {
        let m = c.materials.iter().find(|m| m.name == name)?;
        let add = super::t6_materials::is_additive(m);
        let add_map = super::t6_materials::add_map(m, c).map_or_else(String::new, |i| i.name.clone());
        let t = super::t6_materials::colour_texture(m, c)?;
        let image = c.images.get(t.image?.index)?;
        Some((name.to_owned(), image.name.clone(), add, add_map))
    })
}

pub(super) fn hud_icons(
    captures: &[&ZoneCapture],
    packs: Option<&PackSet>,
    ui_names: &std::collections::BTreeSet<String>,
    report: &mut Vec<String>,
    share: IconShare,
) -> crate::T6HudIcons {
    let share = icon_share(share);
    let mut icons = Vec::new();
    let Some(packs) = packs else {
        return crate::T6HudIcons(icons);
    };
    // The materials the UI scripts name, beside the hand-picked ones.
    let mut wanted: Vec<String> = HUD_ICONS.iter().map(|s| (*s).to_owned()).collect();
    let mut seen: std::collections::BTreeSet<String> = wanted.iter().cloned().collect();
    for c in captures {
        for m in &c.materials {
            // Origins, Mob and Buried build their ammo corner's names at run
            // time (`hud_zm_num_tomb_` + digit), so no script names them.
            let map_hud = m.name.starts_with("hud_zm_");
            if (ui_names.contains(&m.name) || map_hud) && seen.insert(m.name.clone()) {
                wanted.push(m.name.clone());
            }
        }
    }
    // bo2mp: the objective markers' pictures and the carried flag's: BO2's
    // game scripts name them, built from parts ("waypoint_capture" + "_a"),
    // so no UI script holds the whole name.
    let ui_wanted = wanted.len();
    for c in captures {
        for m in &c.materials {
            if (m.name.starts_with("waypoint_") || m.name.starts_with("hudicon_")) && seen.insert(m.name.clone()) {
                wanted.push(m.name.clone());
            }
        }
    }
    report.push(format!("t6 hud icons the game scripts name: {}", wanted.len() - ui_wanted));
    // The ones the front end decoded already (`IconShare::Take`), by place
    // in `wanted`; only the rest are read and decoded.
    let keys: Vec<Option<IconKey>> = if share == IconShare::Off {
        Vec::new()
    } else {
        wanted.iter().map(|name| icon_key(captures, name)).collect()
    };
    let mut kept: Vec<Option<HudIconOut>> = vec![None; wanted.len()];
    if share == IconShare::Take
        && let Ok(shared) = shared_icons().lock()
    {
        for (slot, key) in kept.iter_mut().zip(&keys) {
            *slot = key.as_ref().and_then(|k| shared.get(k)).cloned();
        }
    }
    let all_wanted = wanted;
    let wanted: Vec<String> =
        all_wanted.iter().zip(&kept).filter(|(_, k)| k.is_none()).map(|(n, _)| n.clone()).collect();
    if share == IconShare::Take {
        report.push(format!(
            "t6 hud icons: {} of {} the front end's own (not read or decoded again)",
            all_wanted.len() - wanted.len(),
            all_wanted.len()
        ));
    }
    let read_ahead = std::time::Instant::now();
    let bundled = warm_in_disk_order(captures, packs, &wanted, report);
    if bundled > 0 {
        report.push(format!("t6 hud icons: {bundled} pictures from one bundle file"));
    }
    let read_ahead = read_ahead.elapsed().as_secs_f64();
    // Load speed: the pictures decoded on several threads (each from its
    // bytes in memory once the bundle is read), then taken in name order, so
    // the list and its report are what one thread made.
    let decodes = std::time::Instant::now();
    let threads = icon_threads(wanted.len());
    let outs: Vec<HudIconOut> = if threads <= 1 {
        wanted.iter().map(|name| hud_icon(captures, packs, name)).collect()
    } else {
        std::thread::scope(|s| {
            let parts: Vec<_> = wanted
                .chunks(wanted.len().div_ceil(threads))
                .map(|names| {
                    std::thread::Builder::new()
                        .name("t6 hud icons".into())
                        .spawn_scoped(s, move || names.iter().map(|name| hud_icon(captures, packs, name)).collect::<Vec<_>>())
                })
                .collect();
            parts
                .into_iter()
                .flat_map(|part| match part {
                    Ok(part) => part.join().unwrap_or_else(|e| std::panic::resume_unwind(e)),
                    // (No thread: the names it would have had are done here.)
                    Err(_) => Vec::new(),
                })
                .collect()
        })
    };
    // A thread that would not start leaves its names out; they are done here.
    let outs = if outs.len() == wanted.len() {
        outs
    } else {
        wanted.iter().map(|name| hud_icon(captures, packs, name)).collect()
    };
    // Back in `all_wanted`'s order: the kept ones where they were.
    let mut decoded = outs.into_iter();
    let outs: Vec<HudIconOut> = kept.into_iter().filter_map(|k| k.or_else(|| decoded.next())).collect();
    if share == IconShare::Keep
        && let Ok(mut shared) = shared_icons().lock()
    {
        for (key, out) in keys.iter().zip(&outs) {
            if let Some(key) = key
                && out.failed.is_none()
            {
                shared.insert(key.clone(), out.clone());
            }
        }
    }
    let wanted = all_wanted;
    let mut failed = 0usize;
    let mut failed_names: Vec<String> = Vec::new();
    let mut additive = 0usize;
    for out in outs {
        icons.extend(out.icons);
        report.extend(out.report);
        if let Some(why) = out.failed {
            failed_names.push(why);
            failed += 1;
        }
        additive += usize::from(out.additive);
    }
    report.push(format!(
        "t6 hud icons: read ahead in {read_ahead:.1} s, {} decoded in {:.1} s on {threads} threads",
        wanted.len(),
        decodes.elapsed().as_secs_f64()
    ));
    // bo2mp: pictures the scripts name that no material defines; BO2 finds
    // their pixels in the packs by the name alone (the green "NEW" badge on
    // a Create-a-Class / Barracks tile).
    for name in ["menu_mp_lobby_new_small"] {
        if icons.iter().any(|(k, _)| k == name) || !ui_names.contains(name) {
            continue;
        }
        let Some((i, entry)) = packs.locate_name(name) else { continue };
        let Ok(bytes) = packs.packs[i].read(entry) else { continue };
        let Ok(iwi) = ipak_t6::parse_iwi(&bytes) else { continue };
        if let Ok(img) = super::t6_materials::gpu_image(&iwi) {
            icons.push((name.to_owned(), std::sync::Arc::new(img)));
        }
    }
    // bo2mp: each weapon's kill icon (BO2's kill feed draws it between the
    // two names), as "killicon:<weapon>:<ratio>" (WeaponDef killIcon and
    // killIconRatio: 0 square, 1 twice as wide, 2 four times).
    let mut kill_icons = 0usize;
    for c in captures {
        for w in &c.weapons {
            let Some((_, material)) = w.materials.iter().find(|(off, _)| *off == 1632) else { continue };
            let base = w.name.split('+').next().unwrap_or(&w.name).trim_end_matches("_mp").to_owned();
            let prefix = format!("killicon:{base}:");
            if base.is_empty() || icons.iter().any(|(k, _)| k.starts_with(&prefix)) {
                continue;
            }
            let ratio = w.def.get(1636..1640).map_or(0, |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            let mut add = false;
            let found = captures.iter().find_map(|c| {
                let m = c.materials.iter().find(|m| m.name == *material)?;
                add = super::t6_materials::is_additive(m);
                let t = super::t6_materials::colour_texture(m, c)?;
                c.images.get(t.image?.index)
            });
            let Some(image) = found else { continue };
            if let Ok(img) = super::t6_materials::decode(packs, image) {
                let img = add.then(|| super::t6_materials::additive_to_alpha(&img)).flatten().unwrap_or(img);
                icons.push((format!("{prefix}{ratio}"), std::sync::Arc::new(img)));
                kill_icons += 1;
            }
        }
    }
    // The pictures the engine draws when no gun's icon applies
    // (`hud_obit_death_suicide`, `_falling`,
    // `_crush`, `_grenade_round`, `hud_obit_knife`, `killicondied`,
    // `killiconheadshot`), each as a square icon under its own name.
    let mut obit_names: Vec<String> = Vec::new();
    for c in captures {
        for m in &c.materials {
            if !(m.name.starts_with("hud_obit_") || m.name.starts_with("killicon")) || obit_names.contains(&m.name) {
                continue;
            }
            obit_names.push(m.name.clone());
            let key = format!("killicon:{}:0", m.name);
            if icons.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let add = super::t6_materials::is_additive(m);
            let Some(t) = super::t6_materials::colour_texture(m, c) else { continue };
            let Some(image) = t.image.and_then(|i| c.images.get(i.index)) else { continue };
            if let Ok(img) = super::t6_materials::decode(packs, image) {
                let img = add.then(|| super::t6_materials::additive_to_alpha(&img)).flatten().unwrap_or(img);
                icons.push((key, std::sync::Arc::new(img)));
                kill_icons += 1;
            }
        }
    }
    report.push(format!("t6 kill icons: {kill_icons}"));
    report.push(format!("t6 obituary icon materials: {}", obit_names.join(" ")));
    // bo2mp: each gun's scope picture (its variant's overlayMaterial, e.g.
    // `scope_overlay_svu`), as plain pixels keyed "overlay:<material>": the
    // HUD draws it over the zoomed view and samples its black frame for the
    // bars beside it.
    let mut overlays = 0usize;
    for c in captures {
        let unique_overlays = c
            .attachment_uniques
            .iter()
            .flat_map(|u| [&u.overlay_material, &u.overlay_material_low]);
        for material in c
            .weapons
            .iter()
            .flat_map(|w| [&w.overlay_material, &w.overlay_material_low])
            .chain(unique_overlays)
        {
            let key = format!("overlay:{material}");
            if material.is_empty() || icons.iter().any(|(k, _)| *k == key) {
                continue;
            }
            // Every zone's copy of the material in turn, until one's
            // picture decodes (a zone may hold it as a reference to
            // another's, its picture not there).
            let mut decoded = Err("no picture".to_owned());
            for image in captures.iter().filter_map(|c| {
                let m = c
                    .materials
                    .iter()
                    .find(|m| m.name.trim_start_matches(',') == *material)?;
                let t = super::t6_materials::colour_texture(m, c)?;
                c.images.get(t.image?.index)
            }) {
                decoded = super::t6_materials::decode(packs, image);
                if decoded.is_ok() {
                    break;
                }
            }
            match decoded {
                Ok(img) => {
                    let img = super::t6_materials::to_rgba8(&img).unwrap_or(img);
                    icons.push((key, std::sync::Arc::new(img)));
                    overlays += 1;
                }
                Err(e) => report.push(format!("t6 scope overlay {material}: {e}")),
            }
        }
    }
    report.push(format!("t6 scope overlays: {overlays}"));
    // bo2mp: BO2's hit blood (`overlay_low_health_splat`, technique
    // sw4_2d_blood) is two pictures the HUD mixes: the blood's colour
    // (iw2_blood_color) and the reveal map (iw2_blood_reveal: how far in the
    // blood shows); both as plain pixels, by their image names.
    if let Some((m, c)) = captures
        .iter()
        .find_map(|c| c.materials.iter().find(|m| m.name == "overlay_low_health_splat").map(|m| (m, c)))
    {
        for t in &m.textures {
            let Some(image) = t.image.and_then(|k| c.images.get(k.index)) else { continue };
            if let Ok(img) = super::t6_materials::decode(packs, image)
                && let Some(img) = super::t6_materials::to_rgba8(&img)
            {
                icons.push((image.name.clone(), std::sync::Arc::new(img)));
            }
        }
    }
    if !failed_names.is_empty() {
        report.push(format!(
            "t6 hud icons without a picture: {}",
            failed_names.join(", ")
        ));
    }
    report.push(format!(
        "t6 hud icons: {} of {} ({} hand-picked, the rest the UI scripts name; {failed} without a picture; {additive} additive made see-through)",
        icons.len(),
        wanted.len(),
        HUD_ICONS.len()
    ));
    packs.drop_preloaded();
    crate::T6HudIcons(icons)
}

/// Nuketown's weapons: every base gun the box and walls give, the starting
/// pistol, knives and equipment.
const NUKETOWN_WEAPONS: [&str; 40] = [
    "m1911_zm",
    "beretta93r_zm",
    "fiveseven_zm",
    "fivesevendw_zm",
    "kard_zm",
    "python_zm",
    "judge_zm",
    "ak74u_zm",
    "mp5k_zm",
    "qcw05_zm",
    "870mcs_zm",
    "rottweil72_zm",
    "saiga12_zm",
    "srm1216_zm",
    "m14_zm",
    "m16_zm",
    "fnfal_zm",
    "galil_zm",
    "hk416_zm",
    "saritch_zm",
    "tar21_zm",
    "type95_zm",
    "xm8_zm",
    "hamr_zm",
    "lsat_zm",
    "rpd_zm",
    "barretm82_zm",
    "dsr50_zm",
    "ray_gun_zm",
    "raygun_mark2_zm",
    "knife_ballistic_zm",
    "m32_zm",
    "usrpg_zm",
    "knife_zm",
    "bowie_knife_zm",
    "tazer_knuckles_zm",
    "frag_grenade_zm",
    "sticky_grenade_zm",
    "claymore_zm",
    "cymbal_monkey_zm",
];

/// The M2 census: every Nuketown weapon complete (first-person and world
/// models, launcher and melee models, magazine and scope attachments,
/// animations), and nothing the weapons,
/// their animations, the impact table or the map scripts name missing from
/// the effects or the sound bank. A sound BO2's own banks lack (an
/// animation's cue Nuketown never shipped) is silent in BO2 too: counted
/// apart, as is an alias BO2 ships without audio.
#[allow(clippy::too_many_arguments)]
fn census(
    captures: &[&ZoneCapture],
    names: &[String],
    weapon_refs: &HashMap<String, &asset_t6::WeaponRef>,
    models_missing: &[String],
    xanims: &XAnimBuild,
    fx: &FxCatalog,
    impact: Option<&OwnedFxImpactTable>,
    map_fx: &asset_audio::ScriptedMapFx,
    room_tone: Option<&str>,
    sound: Option<&asset_audio::SoundCatalog>,
) -> Vec<String> {
    use fastfile_t6::layout::WeaponDef as d;
    let ns = asset_core::AssetNamespace::T6;
    let mut missing_weapons: Vec<String> = Vec::new();
    let mut missing_fx: Vec<String> = Vec::new();
    let mut missing_sounds: Vec<String> = Vec::new();
    let mut absent_in_bo2: Vec<String> = Vec::new();
    let mut silent: usize = 0;
    let (mut fx_n, mut snd_n) = (0usize, 0usize);
    let anim_notifies: HashMap<String, &asset_t6::XAnimRef> = captures
        .iter()
        .flat_map(|c| c.xanims.iter())
        .filter(|a| !a.name.starts_with(','))
        .map(|a| (a.name.to_ascii_lowercase(), a))
        .collect();
    let aliases: HashMap<String, &asset_audio::CapturedSound> = sound
        .map(|cat| {
            cat.sounds
                .iter()
                .map(|s| (s.name.to_ascii_lowercase(), s))
                .collect()
        })
        .unwrap_or_default();
    let mut check_fx = |name: &str, what: &str, missing: &mut Vec<String>| {
        if name.is_empty() {
            return;
        }
        fx_n += 1;
        if fx.get_in(ns, name.trim_start_matches(',')).is_none() {
            missing.push(format!("{what}: {name}"));
        }
    };
    // A sound named by the data: in the bank with audio (or shipped silent),
    // else absent from BO2's own banks (silent in BO2 too). Missing means
    // in the bank but no audio found: a failure here.
    let mut check_sound = |name: &str, _from_anim: bool, what: &str| {
        if name.is_empty() {
            return;
        }
        snd_n += 1;
        match aliases.get(&name.to_ascii_lowercase()) {
            None => absent_in_bo2.push(name.to_owned()),
            Some(sound) => {
                let audible = sound.aliases.iter().any(|row| row.streamed.is_some());
                let shipped_silent = sound.aliases.iter().all(|row| {
                    row.streamed.is_none() && row.file_name.as_deref().unwrap_or("").is_empty()
                });
                if !audible {
                    if shipped_silent {
                        silent += 1;
                    } else {
                        missing_sounds.push(format!("{what}: {name} (no audio)"));
                    }
                }
            }
        }
    };
    for name in names.iter().map(String::as_str) {
        let Some(w) = weapon_refs.get(name) else {
            missing_weapons.push(format!("{name}: not in the zones"));
            continue;
        };
        let mut gaps = Vec::new();
        for model in w
            .gun_models
            .first()
            .into_iter()
            .chain(w.world_models.first())
            .map(String::as_str)
            .chain(
                [d::rocketModel, d::projectileModel, d::additionalMeleeModel]
                    .map(|f| w.def_model(f)),
            )
            .chain(w.attach_view_models.iter().map(|a| a.1.as_str()))
            .filter(|m| !m.is_empty() && !m.starts_with(','))
        {
            if models_missing.iter().any(|m| m == model) {
                gaps.push(format!("model {model}"));
            }
        }
        for anim in w.xanims.iter().filter(|a| !a.is_empty()) {
            let key = anim.trim_start_matches(',').to_ascii_lowercase();
            if xanims.get(ns, &key).is_none() {
                gaps.push(format!("anim {anim}"));
            }
            if let Some(a) = anim_notifies.get(&key) {
                for (note, _) in &a.notifies {
                    if let Some(alias) = note.strip_prefix("sndnt#") {
                        check_sound(alias, true, name);
                    }
                }
            }
        }
        if !gaps.is_empty() {
            missing_weapons.push(format!("{name}: {}", gaps.join(", ")));
        }
        for field in [
            d::viewFlashEffect,
            d::worldFlashEffect,
            d::viewShellEjectEffect,
            d::worldShellEjectEffect,
            d::viewLastShotEjectEffect,
            d::worldLastShotEjectEffect,
            d::projExplosionEffect,
            d::projTrailEffect,
            d::projIgnitionEffect,
        ] {
            check_fx(w.def_fx(field), name, &mut missing_fx);
        }
        for field in [
            d::fireSound,
            d::fireSoundPlayer,
            d::emptyFireSound,
            d::emptyFireSoundPlayer,
            d::reloadSound,
            d::reloadSoundPlayer,
            d::reloadEmptySound,
            d::reloadEmptySoundPlayer,
            d::raiseSound,
            d::raiseSoundPlayer,
            d::putawaySound,
            d::putawaySoundPlayer,
            d::projExplosionSound,
            d::meleeSwipeSoundPlayer,
            d::meleeSwipeSound,
            d::meleeHitSound,
            d::meleeMissSound,
            d::pullbackSoundPlayer,
        ] {
            check_sound(w.def_string(field), false, name);
        }
        for alias in &w.bounce_sounds {
            check_sound(alias, false, name);
        }
        // A map value may name the player's alias then the others'.
        for (_, value) in &w.notetrack_sounds {
            for alias in value.split_whitespace() {
                check_sound(alias, false, name);
            }
        }
    }
    if let Some(table) = impact {
        for row in 0..table.row_count() {
            for surf in 0..40 {
                if let Some(n) = table.effect_name(row, surf, Some(0)) {
                    check_fx(n.name, "impact table", &mut missing_fx);
                }
            }
            for flesh in 0..8 {
                if let Some(n) = table.effect_name(row, 7, Some(flesh)) {
                    check_fx(n.name, "impact table", &mut missing_fx);
                }
            }
        }
    }
    for shot in &map_fx.oneshots {
        check_fx(&shot.fxid, "map", &mut missing_fx);
    }
    for l in &map_fx.loop_sounds {
        check_sound(&l.soundalias, false, "map loop");
    }
    for r in &map_fx.random_sounds {
        check_sound(&r.soundalias, false, "map");
    }
    if let Some(tone) = room_tone {
        check_sound(tone, false, "room tone");
    }
    for gait in [
        "sprint",
        "run",
        "walk",
        "prone",
        "crouch_run",
        "crouch_walk",
    ] {
        for surf in [
            "default", "asphalt", "concrete", "dirt", "metal", "wood", "grass", "gravel",
        ] {
            check_sound(&format!("fly_step_{gait}_plr_{surf}"), false, "footsteps");
        }
    }
    for surf in [
        "default", "asphalt", "concrete", "dirt", "metal", "wood", "grass", "gravel",
    ] {
        check_sound(&format!("fly_land_plr_{surf}"), false, "landings");
    }
    missing_fx.sort();
    missing_fx.dedup();
    missing_sounds.sort();
    missing_sounds.dedup();
    absent_in_bo2.sort();
    absent_in_bo2.dedup();
    vec![
        format!(
            "t6 census: {} missing weapons of {} (models, first person, animations){}",
            missing_weapons.len(),
            names.len(),
            if missing_weapons.is_empty() {
                String::new()
            } else {
                format!(": {missing_weapons:?}")
            }
        ),
        format!(
            "t6 census: {} missing FX of {fx_n} named (weapons, impacts, map){}",
            missing_fx.len(),
            if missing_fx.is_empty() {
                String::new()
            } else {
                format!(": {:?}", &missing_fx[..missing_fx.len().min(12)])
            }
        ),
        format!(
            "t6 census: {} missing sounds of {snd_n} named (weapons, animations, footsteps, landings, map){}; {} names BO2's own banks lack (silent in BO2 too) {:?}; {silent} shipped silent",
            missing_sounds.len(),
            if missing_sounds.is_empty() {
                String::new()
            } else {
                format!(": {:?}", &missing_sounds[..missing_sounds.len().min(12)])
            },
            absent_in_bo2.len(),
            &absent_in_bo2[..absent_in_bo2.len().min(8)],
        ),
    ]
}

/// The map's compiled client scripts, read for its placed effects and
/// ambience (later zones' copies win: the patch zone fixes the map's).
fn map_scripts(
    captures: &[&ZoneCapture],
    map: &str,
    report: &mut Vec<String>,
) -> (asset_audio::ScriptedMapFx, Option<String>) {
    let facts = asset_t6::MapScriptFacts::read(map, |name| {
        let want = format!("script:{name}");
        captures.iter().rev().find_map(|c| {
            c.raw_files
                .iter()
                .find(|(n, _)| *n == want)
                .map(|(_, bytes)| bytes.clone())
        })
    });
    let shown: Vec<&asset_t6::PlacedFx> = facts
        .placed
        .iter()
        .filter(|p| p.exploder.is_none() && p.kind != "exploder")
        .collect();
    // CODLEGACY_T6_PLACEDLOG=1: each shown placed effect (test aid).
    if codlegacy_env::var_os("CODLEGACY_T6_PLACEDLOG").is_some() {
        for p in &shown {
            report.push(format!(
                "t6 placed fx {} = {} {} at {:?} angles {:?} delay {}",
                p.fxid,
                facts.effects.get(&p.fxid).map_or("?", String::as_str),
                p.kind,
                p.origin,
                p.angles,
                p.delay
            ));
        }
    }
    let oneshots: Vec<asset_audio::CreateFxOneshot> = shown
        .iter()
        .map(|p| asset_audio::CreateFxOneshot {
            fxid: facts
                .effects
                .get(&p.fxid)
                .cloned()
                .unwrap_or_else(|| p.fxid.clone()),
            origin_inches: p.origin,
            angles_deg: p.angles,
            // A loop effect's delay is its repeat time, not a pre-roll.
            delay: if p.kind == "loopfx" { 0.0 } else { p.delay },
            repeat_secs: 0.0,
        })
        .collect();
    let mut loop_sounds: Vec<asset_audio::CreateFxLoopSound> = Vec::new();
    for (fxid, alias, off) in &facts.sounds_on_fx {
        for p in shown.iter().filter(|p| &p.fxid == fxid) {
            loop_sounds.push(asset_audio::CreateFxLoopSound {
                soundalias: alias.clone(),
                origin_inches: [0, 1, 2].map(|a| p.origin[a] + off[a]),
                line_end_inches: None,
            });
        }
    }
    for (alias, origin) in &facts.loops_at {
        loop_sounds.push(asset_audio::CreateFxLoopSound {
            soundalias: alias.clone(),
            origin_inches: *origin,
            line_end_inches: None,
        });
    }
    let mut random_sounds: Vec<asset_audio::RandomPointSound> = facts
        .random_sounds
        .iter()
        .map(|(alias, wait, places)| asset_audio::RandomPointSound {
            soundalias: alias.clone(),
            wait_secs: *wait,
            places_inches: places.clone(),
        })
        .collect();
    let (structs_random, structs_loop) =
        map_sound_structs(captures, &mut random_sounds, &mut loop_sounds);
    let unresolved = shown
        .iter()
        .filter(|p| !facts.effects.contains_key(&p.fxid))
        .count();
    report.push(format!(
        "t6 map sound structs: {structs_random} now-and-then (wind gusts, creaks), {structs_loop} loops (power lines, wind lines, arcs)"
    ));
    report.push(format!(
        "t6 map scripts: {} placed effects ({} shown at load, {} exploders wait for the game, {unresolved} without an effect table entry); {} loop sounds ({} on effects, {} at points); {} now-and-then sounds; room tone {}",
        facts.placed.len(),
        oneshots.len(),
        facts.placed.len() - shown.len(),
        loop_sounds.len(),
        loop_sounds.len() - facts.loops_at.len(),
        facts.loops_at.len(),
        random_sounds.len(),
        facts.room_tone.as_deref().unwrap_or("none"),
    ));
    report.push(format!("t6 map rooms (room, echo, dry, wet): {:?}", facts.room_echoes));
    (
        asset_audio::ScriptedMapFx {
            oneshots,
            loop_sounds,
            random_sounds,
            rooms: facts.room_echoes.clone(),
            room_tones: Vec::new(),
            room_volumes: Vec::new(),
        },
        facts.room_tone,
    )
}

/// bo2mp: each destructible definition's model, by definition name.
fn destructible_models(captures: &[&ZoneCapture]) -> HashMap<String, String> {
    let mut defs = HashMap::new();
    for c in captures {
        for d in &c.destructibles {
            if real(&d.name) && real(&d.model) {
                defs.insert(d.name.to_ascii_lowercase(), d.model.clone());
            }
        }
    }
    defs
}

/// bo2mp: each destructible definition the server breaks (pieces, stages,
/// what each break plays and tells the scripts), later zones replacing
/// earlier; client-only ones are left to the clients.
fn destructible_defs(captures: &[&ZoneCapture]) -> Vec<std::sync::Arc<xmodel_runtime::T5DestructibleDef>> {
    let text = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    let mut defs: Vec<std::sync::Arc<xmodel_runtime::T5DestructibleDef>> = Vec::new();
    for c in captures {
        for d in &c.destructibles {
            if !real(&d.name) || !real(&d.model) || d.pieces.is_empty() || d.client_only {
                continue;
            }
            let pieces = d
                .pieces
                .iter()
                .map(|p| xmodel_runtime::T5DestructiblePiece {
                    stages: p.stages.clone().map(|st| xmodel_runtime::T5DestructibleStage {
                        spawn_presets: st.spawn_models.clone().map(|m| debris_preset(captures, &m)),
                        show_bone: text(&st.show_bone),
                        break_health: st.break_health,
                        max_time: st.max_time,
                        flags: st.flags,
                        break_effect: text(&st.break_effect),
                        break_sound: text(&st.break_sound),
                        break_notify: text(&st.break_notify),
                        loop_sound: text(&st.loop_sound),
                        has_phys_preset: st.phys_preset,
                        spawn_models: st.spawn_models.clone().map(|m| text(&m)),
                    }),
                    parent_piece: p.parent_piece,
                    parent_damage_percent: p.parent_damage_percent,
                    bullet_damage_scale: p.bullet_damage_scale,
                    explosive_damage_scale: p.explosive_damage_scale,
                    melee_damage_scale: p.melee_damage_scale,
                    health: p.health,
                    hide_bones: p.hide_bones,
                })
                .collect();
            defs.retain(|x| !x.name.eq_ignore_ascii_case(&d.name));
            defs.push(std::sync::Arc::new(xmodel_runtime::T5DestructibleDef {
                name: d.name.clone(),
                model: d.model.clone(),
                pieces,
                client_only: false,
            }));
        }
    }
    defs
}

/// codmp: Black Ops II's loose and breakable props (the clipmap's
/// `dynEntDefList`: bottles, cones, crates, TVs, lamps), which the engine
/// keeps outside the entity string. Each becomes a `script_model` with a
/// `destructibledef` of its own (`dyn_<i>`): a clutter prop (kind 1) comes
/// loose and flies off with its physics preset the first time it is hit; a
/// destruct prop (kind 2) takes its health, then plays its destroy effect
/// and turns into its destroyed model (`codmp_dyn_destroyed`), or is gone.
fn dyn_ent_props(
    c: &ZoneCapture,
    captures: &[&ZoneCapture],
) -> (String, Vec<std::sync::Arc<xmodel_runtime::T5DestructibleDef>>) {
    use xmodel_runtime::{DebrisPreset, T5DestructibleDef, T5DestructiblePiece, T5DestructibleStage};
    let mut text = String::new();
    let mut defs = Vec::new();
    let Some(clip) = &c.clip else {
        return (text, defs);
    };
    let preset = |name: &str, model: &str| -> Option<DebrisPreset> {
        captures
            .iter()
            .rev()
            .find_map(|z| z.phys_presets.iter().find(|p| !name.is_empty() && p.name == name))
            .filter(|p| p.mass > 0.0)
            .map(|p| DebrisPreset {
                mass: p.mass,
                bounce: p.bounce,
                friction: p.friction,
                bullet_force_scale: p.bullet_force_scale,
                explosive_force_scale: p.explosive_force_scale,
                gravity_scale: p.gravity_scale,
            })
            .or_else(|| debris_preset(captures, model))
    };
    for (i, d) in clip.dyn_ents.iter().enumerate() {
        let model = d.model.trim_start_matches(',');
        if !real(model) || !(d.kind == 1 || d.kind == 2) {
            continue;
        }
        let destroyed = d.destroyed_model.trim_start_matches(',');
        let name = format!("dyn_{i}");
        let clutter = d.kind == 1;
        let health = if clutter { 1 } else { d.health.clamp(1, i32::from(i16::MAX)) };
        // (A stage with no part to show counts as already broken, so the
        // whole prop names its origin, as the guest props do.)
        let mut whole = T5DestructibleStage {
            show_bone: Some("tag_origin".to_owned()),
            break_health: 1.0 / health as f32,
            break_effect: (!d.destroy_fx.is_empty()).then(|| d.destroy_fx.clone()),
            ..Default::default()
        };
        if clutter {
            whole.has_phys_preset = true;
            whole.spawn_models[0] = Some(model.to_owned());
            whole.spawn_presets[0] = preset(&d.phys_preset, model);
        }
        defs.push(std::sync::Arc::new(T5DestructibleDef {
            name: name.clone(),
            model: model.to_owned(),
            pieces: vec![T5DestructiblePiece {
                stages: [
                    whole,
                    T5DestructibleStage {
                        break_health: -1.0,
                        ..Default::default()
                    },
                    T5DestructibleStage::default(),
                    T5DestructibleStage::default(),
                    T5DestructibleStage::default(),
                ],
                parent_piece: u8::MAX,
                parent_damage_percent: 0.0,
                bullet_damage_scale: 1.0,
                explosive_damage_scale: 1.0,
                melee_damage_scale: 1.0,
                health,
                hide_bones: [0; 5],
            }],
            client_only: false,
        }));
        let angles = quat_angles(d.quat);
        let [x, y, z] = d.origin;
        text.push_str(&format!(
            "{{\n\"classname\" \"script_model\"\n\"model\" \"{model}\"\n\"origin\" \"{x} {y} {z}\"\n\"angles\" \"{} {} {}\"\n\"destructibledef\" \"{name}\"\n",
            angles[0], angles[1], angles[2]
        ));
        if real(destroyed) {
            text.push_str(&format!("\"codmp_dyn_destroyed\" \"{destroyed}\"\n"));
        }
        text.push_str("}\n");
    }
    (text, defs)
}

/// Pitch, yaw and roll (degrees) of a placement's quaternion (x, y, z, w).
fn quat_angles([x, y, z, w]: [f32; 4]) -> [f32; 3] {
    let fwd = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + w * z),
        2.0 * (x * z - w * y),
    ];
    let left_z = 2.0 * (y * z + w * x);
    let up_z = 1.0 - 2.0 * (x * x + y * y);
    let pitch = (-fwd[2]).atan2(fwd[0].hypot(fwd[1])).to_degrees();
    let yaw = fwd[1].atan2(fwd[0]).to_degrees();
    let roll = left_z.atan2(up_z).to_degrees();
    [pitch, yaw, roll]
}

/// bo2mp: how a destructible's spawn model flies once it breaks off: its
/// model's `physPreset`, the latest zone's copy of the model winning.
fn debris_preset(captures: &[&ZoneCapture], model: &str) -> Option<xmodel_runtime::DebrisPreset> {
    if model.is_empty() {
        return None;
    }
    captures.iter().rev().find_map(|c| {
        let p = c
            .xmodels
            .iter()
            .filter(|x| x.name.trim_start_matches(',') == model)
            .find_map(|x| x.phys_preset)
            .and_then(|k| c.phys_presets.get(k.index))?;
        (p.mass > 0.0).then_some(xmodel_runtime::DebrisPreset {
            mass: p.mass,
            bounce: p.bounce,
            friction: p.friction,
            bullet_force_scale: p.bullet_force_scale,
            explosive_force_scale: p.explosive_force_scale,
            gravity_scale: p.gravity_scale,
        })
    })
}

/// bo2mp: a map's entity text with each destructible built from its
/// definition. An entity with a `destructibledef` carries the editor's
/// stand-in in `model` (`veh_t6_police_car`, no such model in any zone);
/// the engine builds it from the definition's model, so that is what the
/// entity is given here.
fn destructible_entities(text: &str, defs: &HashMap<String, String>) -> String {
    if defs.is_empty() || !text.contains("\"destructibledef\"") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut block: Vec<&str> = Vec::new();
    let flush = |block: &mut Vec<&str>, out: &mut String| {
        let model = block
            .iter()
            .find_map(|l| {
                let mut q = l.split('"').skip(1).step_by(2);
                (q.next()?.eq_ignore_ascii_case("destructibledef")).then(|| q.next())?
            })
            .and_then(|d| defs.get(&d.to_ascii_lowercase()));
        for l in block.drain(..) {
            let is_model = l.split('"').nth(1).is_some_and(|k| k.eq_ignore_ascii_case("model"));
            match model {
                Some(m) if is_model => {
                    out.push_str(&format!("\"model\" \"{m}\""));
                }
                _ => out.push_str(l),
            }
            out.push_str("\n");
        }
    };
    for line in text.lines() {
        block.push(line);
        if line.trim() == "}" {
            flush(&mut block, &mut out);
        }
    }
    flush(&mut block, &mut out);
    out
}

/// The map's sound structs, which every map's client script
/// (clientscripts/mp/_audio.csc) starts: a "random" one plays its sound at
/// its place every `randomfloatrange(script_wait_min, script_wait_max)` s
/// (1 to 3 by default), a "looper" loops it there, a "line_emitter" loops
/// it along the line to its target. Returns (random, loops) added.
fn map_sound_structs(
    captures: &[&ZoneCapture],
    random: &mut Vec<asset_audio::RandomPointSound>,
    loops: &mut Vec<asset_audio::CreateFxLoopSound>,
) -> (usize, usize) {
    let Some(ents) = captures.iter().rev().find(|c| !c.map_ents.is_empty()) else {
        return (0, 0);
    };
    let ents: Vec<_> = ents
        .map_ents
        .iter()
        .flat_map(|e| asset_t6::parse_entities(e))
        .collect();
    let (mut nr, mut nl) = (0, 0);
    for e in &ents {
        if e.classname() != "script_struct" {
            continue;
        }
        let (Some(alias), Some(origin)) = (e.get("script_sound"), e.vec3("origin")) else {
            continue;
        };
        let alias = alias.to_owned();
        match e.get("script_label") {
            Some("random") => {
                let f = |k: &str, d: f32| e.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
                random.push(asset_audio::RandomPointSound {
                    soundalias: alias,
                    wait_secs: [f("script_wait_min", 1.0), f("script_wait_max", 3.0)],
                    places_inches: vec![origin],
                });
                nr += 1;
            }
            Some("looper") => {
                loops.push(asset_audio::CreateFxLoopSound {
                    soundalias: alias,
                    origin_inches: origin,
                    line_end_inches: None,
                });
                nl += 1;
            }
            Some("line_emitter") => {
                let end = e.get("target").and_then(|t| {
                    ents.iter()
                        .find(|o| o.get("targetname") == Some(t))
                        .and_then(|o| o.vec3("origin"))
                });
                loops.push(asset_audio::CreateFxLoopSound {
                    soundalias: alias,
                    origin_inches: origin,
                    line_end_inches: end,
                });
                nl += 1;
            }
            _ => {}
        }
    }
    (nr, nl)
}

/// What a bullet shows on a model's bone boxes: its paint's surface (a
/// mannequin's is plastic), the one most of its surfaces use; 0 (blood) when
/// none has one.
fn model_surface_flags(capture: &ZoneCapture, model: &asset_t6::XModelRef) -> u32 {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for srf in &model.lod0 {
        let flags = srf
            .material
            .and_then(|k| capture.material(k))
            .map_or(0, |m| m.surface_flags);
        if flags != 0 {
            *counts.entry(flags).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map_or(0, |(flags, _)| flags)
}

/// bo2zm M3: every XModel the zones carry whose name a server script
/// (as a string constant), a map entity (`"model"`) or a destructible's
/// broken-off piece names.
fn script_model_names(
    captures: &[&ZoneCapture],
    destructibles: &HashMap<String, String>,
) -> BTreeSet<String> {
    let models: BTreeSet<&str> = captures
        .iter()
        .flat_map(|c| c.xmodels.iter().map(|m| m.name.as_str()))
        .filter(|n| real(n))
        .collect();
    // bo2mp: every collision model (`collision_clip_64x64x10`...): small,
    // never drawn, and any script may stand one up (the engine's own
    // scripts, his trickshot script's platforms, are not in a zone).
    let mut named: BTreeSet<String> = models
        .iter()
        .filter(|n| n.starts_with("collision_"))
        .map(|n| (*n).to_owned())
        .collect();
    let mut take = |s: &str| {
        if models.contains(s) {
            named.insert(s.to_owned());
        }
    };
    for c in captures {
        for (name, bytes) in &c.raw_files {
            if !name.starts_with("script:") || !name.ends_with(".gsc") {
                continue;
            }
            for token in bytes.split(|&b| b == 0) {
                if (3..96).contains(&token.len()) && token.iter().all(|b| b.is_ascii_graphic()) {
                    take(&String::from_utf8_lossy(token));
                }
            }
        }
        // Every gun's world model (the box's floating gun, bought wall guns).
        for w in &c.weapons {
            for m in &w.world_models {
                take(m);
            }
            // And the parts it wears there (magazine, scope, the RPG's
            // rocket): BO2 hangs them on the gun (`attachWorldModel`).
            for (_, m, ..) in &w.attach_world_models {
                take(m);
            }
        }
        // The pieces a breakable prop throws (a mannequin's head and arms).
        for d in &c.destructibles {
            for p in &d.pieces {
                for st in &p.stages {
                    for m in st.spawn_models.iter().filter(|m| !m.is_empty()) {
                        take(m);
                    }
                }
            }
        }
        for d in &c.destructibles {
            for st in d.pieces.iter().flat_map(|p| &p.stages) {
                for m in &st.spawn_models {
                    take(m);
                }
            }
        }
        // codmp: the clipmap's loose and breakable props (`dyn_ent_props`).
        for d in c.clip.iter().flat_map(|clip| &clip.dyn_ents) {
            take(d.model.trim_start_matches(','));
            take(d.destroyed_model.trim_start_matches(','));
        }
        for ents in &c.map_ents {
            for e in asset_t6::parse_entities(&destructible_entities(ents, destructibles)) {
                if let Some(m) = e.get("model").filter(|m| !m.starts_with('*')) {
                    take(m);
                }
                // A zbarrier's pieces (the magic box) name their models.
                for i in 1..=8 {
                    if let Some(m) = e.get(&format!("zbarrierboardmodel{i}")) {
                        take(m);
                    }
                }
            }
        }
    }
    named
}

/// bo2mp: the mod's zone made ready for the match (`CODLEGACY_T6_MOD`): what
/// would change the game left out (`asset_t6::t6_mod::sanitize_capture`),
/// and the animations its guns name that neither it nor the zones before
/// it (`earlier`) carry read from its archive's raw files (`xanim/<name>`).
pub(crate) fn complete_mod_capture(c: &mut ZoneCapture, earlier: &[ZoneCapture], report: &mut Vec<String>) {
    let info = asset_transport::t6_mod::info().unwrap_or_default();
    let left_out = asset_t6::t6_mod::sanitize_capture(c);
    report.push(format!(
        "bo2mp mod {} {} by {} ({}): {} weapons, {} attachments, {} camos, {} models, {} materials, {} pictures, {} sound banks; left out: {}",
        info.name,
        info.version,
        info.author,
        asset_transport::t6_mod::root().map_or_else(String::new, |r| r.display().to_string()),
        c.weapons.len(),
        c.attachment_uniques.len(),
        c.weapon_camos.len(),
        c.xmodels.len(),
        c.materials.len(),
        c.images.len(),
        c.sound_banks.len(),
        if left_out.is_empty() { "nothing".to_owned() } else { left_out.join("; ") }
    ));
    let have: std::collections::HashSet<String> =
        earlier.iter().flat_map(|e| e.xanims.iter()).map(|x| x.name.to_ascii_lowercase()).collect();
    let missing = asset_t6::t6_mod::missing_xanims(c, &have);
    let (mut read, mut gaps) = (0usize, Vec::new());
    for name in &missing {
        let Some(bytes) = asset_transport::t6_mod::xanim(name) else {
            // (Black Ops II's own, from a zone after this one, or none.)
            continue;
        };
        match asset_t6::t6_mod::raw_xanim(name, &bytes) {
            Ok(a) => {
                c.xanims.push(a);
                read += 1;
            }
            Err(e) => gaps.push(e),
        }
    }
    report.push(format!(
        "bo2mp mod animations: {} in its zone, {read} of {} more read from its archive{}",
        c.xanims.len() - read,
        missing.len(),
        if gaps.is_empty() { String::new() } else { format!("; gaps: {}", gaps.join("; ")) }
    ));
}

/// The zones around the map zone, in load order, that exist beside it.
fn zone_paths(map_path: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    // bo2zm: a map from an extra folder (`CODLEGACY_T6_EXTRA`) still loads the
    // install's common zones.
    let dir = asset_transport::t6_extra::install_zone_dir(map_path);
    let dir = dir.as_path();
    let stem = map_path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    // bo2mp: a multiplayer map loads multiplayer's shared zones.
    let shared: &[&str] = if is_mp_map(&stem) {
        &T6_MP_PRE_ZONES
    } else {
        &T6_ZM_PRE_ZONES
    };
    let mut pre: Vec<PathBuf> = shared
        .iter()
        .map(|z| dir.join(format!("{z}.ff")))
        .collect();
    // Nuketown is the first Zombies DLC map: its load zone carries the
    // intro's sound bank.
    if stem == "zm_nuked" {
        pre.push(dir.join("dlczm0_load_zm.ff"));
    }
    // bo2mp: the mod's zone (`CODLEGACY_T6_MOD`: its guns, their models,
    // pictures, animations and sounds), after Black Ops II's shared zones,
    // winning a name it shares with them.
    if let Some(zone) = asset_transport::t6_mod::zone() {
        pre.push(zone);
    }
    // A map from an extra folder (a map pack) loads its mod's
    // zones (weapons, string tables, menus) before the map, as its game does.
    if map_path.parent() != Some(dir) {
        pre.extend(
            ["mod.ff", "mod_load.ff", "mod_patch.ff"]
                .iter()
                .filter_map(|z| asset_transport::t6_extra::extra_file(z)),
        );
    }
    // bo2mp: a multiplayer map has no mode zone nor patch zone (its fixes
    // are in patch_mp).
    // bo2zm: the mode zone (`so_zclassic_zm_buried.ff`: the map's zombie
    // types, their characters and animstatedefs, the mode's extra
    // entities), then the patch. Nuketown plays zstandard and has none. A
    // survival start (Tranzit's Town: zstandard) takes the zsurvival zone,
    // Grief and Turned zencounter (Borough's zombie hands and melee are only
    // there).
    let post = if is_mp_map(&stem) {
        Vec::new()
    } else {
        vec![
            asset_transport::t6_extra::file_in(dir, &format!("{}.ff", mode_zone(&stem))),
            asset_transport::t6_extra::file_in(dir, &format!("{stem}_patch.ff")),
        ]
    };
    let exists = |v: Vec<PathBuf>| v.into_iter().filter(|p| p.is_file()).collect();
    (exists(pre), exists(post))
}

/// bo2mp: a multiplayer map's two factions' zones, from `mp/mapstable.csv`
/// in the zones read so far: the map's row names the allies' and axis'
/// team sets (columns 1 and 2) and, on the snow, wet and sand maps, the
/// look in its last column (16): Downhill = `faction_seals_snow_mp` +
/// `faction_pmc_snow_mp`. A team set without that look loads its own zone.
const MAPSTABLE_TEAMS: [usize; 2] = [1, 2];
const MAPSTABLE_LOOK: usize = 16;

fn mp_faction_zones(
    captures: &[ZoneCapture],
    map: &str,
    dir: &Path,
    report: &mut Vec<String>,
) -> Vec<PathBuf> {
    let Some(t) = captures
        .iter()
        .rev()
        .flat_map(|c| c.string_tables.iter())
        .find(|t| t.name.eq_ignore_ascii_case("mp/mapstable.csv"))
    else {
        report.push("t6 mp factions: no mp/mapstable.csv".to_owned());
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for r in 0..t.rows {
        let row = &t.cells[r * t.columns..((r + 1) * t.columns).min(t.cells.len())];
        if !row.first().is_some_and(|c| c.eq_ignore_ascii_case(map)) {
            continue;
        }
        let look = row
            .get(MAPSTABLE_LOOK)
            .filter(|c| !c.is_empty())
            .map(|c| format!("_{}", c.to_ascii_lowercase()))
            .unwrap_or_default();
        for team in MAPSTABLE_TEAMS.iter().filter_map(|&i| row.get(i)) {
            let team = team.to_ascii_lowercase();
            if team.is_empty() || team.len() >= 32 {
                continue;
            }
            let path = [
                format!("faction_{team}{look}_mp.ff"),
                format!("faction_{team}_mp.ff"),
            ]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file());
            if let Some(path) = path.filter(|p| !out.contains(p)) {
                out.push(path);
            }
        }
    }
    // bo2mp: a map from another game has no row: Aftermath's pairing.
    if out.is_empty() && map != "mp_la" {
        report.push(format!("t6 mp factions for {map}: none listed, Aftermath's taken"));
        return mp_faction_zones(captures, "mp_la", dir, report);
    }
    report.push(format!(
        "t6 mp factions for {map}: {}",
        out.iter()
            .filter_map(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out
}

/// bo2mp: a material drawn by BO2's sight reticle shader
/// (`sw4_3d_reticle_dynamic`: the reflex's dot, the EOTech's ring, the
/// ACOG's chevron), whose picture is projected to infinity along the sight;
/// (its capture, index) where it is defined.
fn reticle_material(
    captures: &[&ZoneCapture],
    homes: &HashMap<String, (usize, usize)>,
    ci: usize,
    key: asset_t6::AssetKey,
) -> Option<(usize, usize)> {
    let m = captures[ci].materials.get(key.index)?;
    let (hc, hm) = if m.name.starts_with(',') {
        *homes.get(m.name.trim_start_matches(','))?
    } else {
        (ci, key.index)
    };
    let def = captures[hc].materials.get(hm)?;
    let set = def
        .technique_set
        .and_then(|k| captures[hc].technique_sets.get(k.index))?;
    set.name.contains("reticle_dynamic").then_some((hc, hm))
}

/// bo2mp: a sight model's reticle as Black Ops II's shader draws it (fxc
/// disassembly of `sw4_3d_reticle_dynamic`): its picture's red channel is
/// the dot or ring in `Reticle_Color`, its green the glow around it raised
/// to `Glow_Falloff` in `Glow_Color`, both times `Emissive_Amount`, added on
/// (the picture is that colour, for the HUD to add onto the scene). Returns the picture and `Color_Map_Scale`
/// (the picture spans 1 / scale in tangent of the view angle).
fn sight_reticle(
    captures: &[&ZoneCapture],
    homes: &HashMap<String, (usize, usize)>,
    packs: &PackSet,
    model: &str,
    top: bool,
) -> Option<(f32, bevy::prelude::Image)> {
    let (ci, m) = captures
        .iter()
        .enumerate()
        .rev()
        .find_map(|(ci, c)| c.xmodels.iter().find(|m| m.name == model).map(|m| (ci, m)))?;
    // The Hybrid's two reticles: the scope's (`_down`) and the little dot
    // above it (`_up`), one for each of its two modes (`top`: the dot's).
    let found: Vec<(usize, usize)> = m
        .lod0
        .iter()
        .filter_map(|s| s.material)
        .chain(m.materials.iter().flatten().copied())
        .filter_map(|k| reticle_material(captures, homes, ci, k))
        .collect();
    let (hc, hm) = found
        .iter()
        .copied()
        .find(|&(hc, hm)| captures[hc].materials[hm].name.ends_with("_up") == top)
        .or_else(|| found.first().copied())?;
    let c = captures[hc];
    let mat = &c.materials[hm];
    let constant = |prefix: &str| {
        mat.constants
            .iter()
            .find(|(_, name, _)| name.starts_with(prefix))
            .map(|(_, _, v)| *v)
    };
    let scale = constant("Color_Map_Sc")?[0];
    let core = constant("Reticle_Colo").unwrap_or([1.0; 4]);
    let glow = constant("Glow_Color").unwrap_or([1.0, 0.0, 0.0, 1.0]);
    let falloff = constant("Glow_Falloff").map_or(1.0, |v| v[0]);
    let emissive = constant("Emissive_Amo").map_or(1.0, |v| v[0]);
    let noise = constant("Color_Map_No").map_or(0.0, |v| v[0]);
    // Its picture (slot 0xb607c0fe; the noise map beside it is another
    // zone's and empty here).
    let image = mat
        .textures
        .iter()
        .filter_map(|t| Some((t.name_hash, c.images.get(t.image?.index)?)))
        .filter(|(_, img)| img.width > 0)
        .max_by_key(|(hash, img)| (*hash == 0xb607_c0fe, u32::from(img.width) * u32::from(img.height)))
        .map(|(_, img)| img)?;
    let img = super::t6_materials::to_rgba8(&super::t6_materials::decode(packs, image).ok()?)?;
    let mut rgba = img.data.clone()?;
    // A colour map is read as sRGB (the engine's own rule: `use_srgb_reads`
    // for every colour map), so the shader's picture values are the linear
    // ones.
    let linear = |byte: u8| {
        let v = f32::from(byte) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    for px in rgba.chunks_exact_mut(4) {
        let r = linear(px[0]);
        let g = linear(px[1]).powf(falloff);
        // The shader's noise map (another zone's, not held here) shimmers
        // the glow; its mean, one half, stands in for it. The shader's
        // output is HDR (`Emissive_Amount` runs to 9), which the game
        // squeezes before it shows: 1 - e^(-c * EXPOSURE) (a fitted soft
        // knee, no data behind it). The material adds that onto the scene
        // (blend ONE / ONE or ONE / 1 - alpha with a zero alpha), so the
        // HUD draws it additively: the picture is the colour added.
        const NOISE_MEAN: f32 = 0.5;
        const EXPOSURE: f32 = 0.35;
        for i in 0..3 {
            let c = (r * core[i] * (1.0 - noise * NOISE_MEAN) + g * glow[i] * NOISE_MEAN) * emissive;
            px[i] = ((1.0 - (-c * EXPOSURE).exp()) * 255.0).round() as u8;
        }
        px[3] = 255;
    }
    let mut out = bevy::prelude::Image::new(
        img.texture_descriptor.size,
        bevy::render::render_resource::TextureDimension::D2,
        rgba,
        bevy::render::render_resource::TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::default(),
    );
    out.sampler = img.sampler.clone();
    Some((scale, out))
}

/// A camo pack's picture (`asset_core::camo_pack`): a PNG as it is, or an
/// animation's frames (one PNG each, or the cells of a sheet) stacked into
/// the filmstrip the shader plays.
fn read_pack_picture(source: &asset_core::camo_pack::Source) -> Option<(u32, u32, Vec<u8>)> {
    use asset_core::camo_pack::{Source, filmstrip, slice_sheet};
    match source {
        Source::Image(path) => read_user_camo(path),
        Source::Embedded(bytes) => decode_camo_png(std::io::Cursor::new(*bytes)),
        Source::Frames(paths) => {
            let frames = paths.iter().map(|p| read_user_camo(p)).collect::<Option<Vec<_>>>()?;
            filmstrip(&frames)
        }
        Source::Sheet { path, cols, rows, count } => {
            let (w, h, rgba) = read_user_camo(path)?;
            filmstrip(&slice_sheet(w, h, &rgba, *cols, *rows, *count)?)
        }
    }
}

/// A player's own camo: (width, height, RGBA8) from a PNG up to 4096 x 4096
/// (any colour type, 8 or 16 bits).
fn read_user_camo(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    decode_camo_png(std::io::BufReader::new(file))
}

/// `read_user_camo`'s decode, from any reader (a built-in camo's bytes too).
fn decode_camo_png<R: std::io::BufRead + std::io::Seek>(reader: R) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(reader);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    buf.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == (w * h * 4) as usize).then_some((w, h, rgba))
}

/// The mode zone's name for a map (`so_zclassic_zm_transit`).
fn mode_zone(stem: &str) -> String {
    let mode = match frame::bo2_zm_start(stem).map(|(_, mode)| mode).as_deref() {
        Some("zstandard") => "zsurvival",
        Some("zgrief" | "zcleansed" | "zmeat") => "zencounter",
        _ if stem == "zm_nuked" => "zstandard",
        _ => "zclassic",
    };
    format!("so_{mode}_{stem}")
}

fn real(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(',')
}

/// Where each material is really defined: name -> (capture, index), the
/// last zone that defines it (not a comma-named reference) winning.
fn material_homes(captures: &[&ZoneCapture]) -> HashMap<String, (usize, usize)> {
    let mut homes = HashMap::new();
    for (ci, c) in captures.iter().enumerate() {
        for (mi, m) in c.materials.iter().enumerate() {
            if real(&m.name) {
                homes.insert(m.name.clone(), (ci, mi));
            }
        }
    }
    homes
}

/// For every capture, the material indices (into that capture) the wanted
/// models and effects draw with, each taken from the zone that defines it.
/// bo2zm M3: Pack-a-Punch's look. The scripts give an upgraded gun camo
/// 39 on this map (`get_pack_a_punch_weapon_options`; 40 on Mob of the
/// Dead, 45 on Origins); `mp/weaponoptions_zm.csv`'s row for it says it
/// swaps materials (a 1 in column 3) and which of the gun's camo material
/// sets (column 4, counted from 1: the M1911's camo1 -> the zombies camo).
/// Each upgraded gun's view model gets a copy with those swaps,
/// `<model>+camo<n>`.
#[derive(Default)]
struct PapCamo {
    /// Copy -> (the gun model, swaps base material -> camo material, and
    /// the camo's texture tiling: the swap's constants 0 and 1, which BO2's
    /// 3D camo shader multiplies the texture coordinates by, 2 x 2 for the
    /// zombies camo).
    models: BTreeMap<String, (String, Vec<(String, String, [f32; 2])>)>,
    /// Upgraded weapon -> its gun model's copy.
    weapon_models: HashMap<String, String>,
}

/// A float as a half (IEEE binary16), rounded down; texture coordinates.
fn f32_to_half(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x007f_ffff;
    if exp <= 0 {
        return sign;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | ((exp as u16) << 10) | ((mant >> 13) as u16)
}

/// bo2mp: Black Ops II's pattern camos for the match: per camo option
/// (`mp/weaponoptions.csv` column 3 = 0, its pattern set counted from 1 in
/// column 4), every gun's camo-ready materials (those its material camos
/// replace) with that gun's pattern picture and repeat for the set.
fn mp_pattern_camos(captures: &[&ZoneCapture]) -> BTreeMap<u32, Vec<(String, String, f32)>> {
    let mut out: BTreeMap<u32, Vec<(String, String, f32)>> = BTreeMap::new();
    let Some(t) = captures
        .iter()
        .rev()
        .flat_map(|c| c.string_tables.iter())
        .find(|t| t.name.eq_ignore_ascii_case("mp/weaponoptions.csv"))
    else {
        return out;
    };
    let sets: Vec<(u32, usize)> = (0..t.rows)
        .filter_map(|r| {
            let cell = |k: usize| t.cells.get(r * t.columns + k).map_or("", String::as_str);
            (cell(1) == "camo" && cell(3) == "0")
                .then(|| Some((cell(0).parse::<u32>().ok()?, cell(4).parse::<usize>().ok()?)))
                .flatten()
        })
        .filter(|(o, set)| *o > 0 && *set > 0)
        .collect();
    let camos: Vec<&asset_t6::WeaponCamoRef> = captures.iter().flat_map(|c| c.weapon_camos.iter()).collect();
    for (option, set) in sets {
        let list = out.entry(option).or_default();
        for camo in &camos {
            let Some(pattern) = camo.sets.get(set - 1).filter(|p| real(&p.pattern)) else {
                continue;
            };
            let ready: BTreeSet<&String> = camo.material_sets.iter().flatten().flat_map(|m| m.base.iter()).filter(|b| real(b)).collect();
            for base in ready {
                if !list.iter().any(|(b, _, _)| b == base) {
                    list.push((base.clone(), pattern.pattern.clone(), pattern.scale));
                }
            }
        }
    }
    out.retain(|_, l| !l.is_empty());
    out
}

/// bo2mp: Black Ops II's material camos for the match: per camo option
/// (`mp/weaponoptions.csv`: index, "camo", name, 1 = swaps materials, its
/// material set counted from 1), every gun's (camo-ready material, the
/// camo's material, its tiling) from the guns' `WeaponCamo` material
/// sets.
fn mp_material_camos(captures: &[&ZoneCapture]) -> BTreeMap<u32, Vec<(String, String, f32)>> {
    let mut out: BTreeMap<u32, Vec<(String, String, f32)>> = BTreeMap::new();
    let Some(t) = captures
        .iter()
        .rev()
        .flat_map(|c| c.string_tables.iter())
        .find(|t| t.name.eq_ignore_ascii_case("mp/weaponoptions.csv"))
    else {
        return out;
    };
    let sets: Vec<(u32, usize)> = (0..t.rows)
        .filter_map(|r| {
            let cell = |k: usize| t.cells.get(r * t.columns + k).map_or("", String::as_str);
            (cell(1) == "camo" && cell(3) == "1")
                .then(|| Some((cell(0).parse::<u32>().ok()?, cell(4).parse::<usize>().ok()?)))
                .flatten()
        })
        .filter(|(o, set)| *o > 0 && *set > 0)
        .collect();
    let camos: Vec<&asset_t6::WeaponCamoRef> = captures.iter().flat_map(|c| c.weapon_camos.iter()).collect();
    for (option, set) in sets {
        let list = out.entry(option).or_default();
        for camo in &camos {
            let Some(materials) = camo.material_sets.get(set - 1) else {
                continue;
            };
            for m in materials {
                let tile = if m.consts[0] > 0.0 { m.consts[0] } else { 1.0 };
                for (b, c) in m.base.iter().zip(&m.camo) {
                    if real(b) && real(c) && !list.iter().any(|(x, _, _)| x == b) {
                        list.push((b.clone(), c.clone(), tile));
                    }
                }
            }
        }
    }
    out.retain(|_, l| !l.is_empty());
    out
}

fn pap_camo(
    captures: &[&ZoneCapture],
    weapon_refs: &HashMap<String, &asset_t6::WeaponRef>,
    camo_index: u32,
) -> PapCamo {
    let mut out = PapCamo::default();
    let want = camo_index.to_string();
    let set = captures
        .iter()
        .rev()
        .flat_map(|c| c.string_tables.iter())
        .find(|t| t.name == "mp/weaponoptions_zm.csv")
        .and_then(|t| {
            (0..t.rows).find_map(|r| {
                let cell = |k: usize| t.cells.get(r * t.columns + k).map_or("", String::as_str);
                (cell(0) == want && cell(1) == "camo" && cell(3) == "1")
                    .then(|| cell(4).parse::<usize>().ok())
                    .flatten()
            })
        })
        .filter(|set| *set > 0);
    let Some(set) = set else {
        return out;
    };
    let camos: HashMap<&str, &asset_t6::WeaponCamoRef> = captures
        .iter()
        .flat_map(|c| c.weapon_camos.iter())
        .map(|c| (c.name.as_str(), c))
        .collect();
    for (name, w) in weapon_refs {
        if !name.contains("_upgraded") {
            continue;
        }
        let Some(list) = camos
            .get(w.camo.as_str())
            .and_then(|camo| camo.material_sets.get(set - 1))
        else {
            continue;
        };
        let swaps: Vec<(String, String, [f32; 2])> = list
            .iter()
            .flat_map(|m| {
                let tile = [m.consts[0], m.consts[1]].map(|t| if t > 0.0 { t } else { 1.0 });
                m.base
                    .iter()
                    .cloned()
                    .zip(m.camo.iter().cloned())
                    .map(move |(b, c)| (b, c, tile))
            })
            .filter(|(base, camo, _)| real(base) && real(camo))
            .collect();
        let Some(gun) = w.gun_models.first().filter(|n| real(n)) else {
            continue;
        };
        if swaps.is_empty() {
            continue;
        }
        let copy = format!("{gun}+camo{camo_index}");
        out.models
            .entry(copy.clone())
            .or_insert((gun.clone(), swaps));
        out.weapon_models.insert(name.clone(), copy);
    }
    out
}

fn wanted_materials(
    captures: &[&ZoneCapture],
    model_names: &BTreeSet<String>,
    homes: &HashMap<String, (usize, usize)>,
) -> Vec<BTreeSet<usize>> {
    let mut wanted: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); captures.len()];
    let mut want = |name: &str| {
        if let Some(&(ci, mi)) = homes.get(name.trim_start_matches(',')) {
            wanted[ci].insert(mi);
        }
    };
    // Models: from the last capture that defines each.
    for name in model_names {
        if let Some((ci, model)) = captures
            .iter()
            .enumerate()
            .rev()
            .find_map(|(ci, c)| c.xmodels.iter().find(|m| &m.name == name).map(|m| (ci, m)))
        {
            for srf in &model.lod0 {
                if let Some(k) = srf.material
                    && let Some(m) = captures[ci].materials.get(k.index)
                {
                    want(&m.name);
                }
            }
        }
    }
    // Effects and tracers: their material names.
    for c in captures {
        for e in c.fx.iter().filter(|e| real(&e.name)) {
            for el in &e.elems {
                for v in &el.visuals {
                    match v {
                        asset_t6::FxVisualRef::Material(n) => want(n),
                        asset_t6::FxVisualRef::Mark([a, b]) => {
                            want(a);
                            want(b);
                        }
                        _ => {}
                    }
                }
            }
        }
        for t in c.tracers.iter().filter(|t| real(&t.name)) {
            want(&t.material);
        }
    }
    wanted
}

/// The sound bank files a map's banks read: each bank asset's own files,
/// the map's location banks (`zmb_<location>*`), and every file those
/// name as a dependency.
pub(super) fn bank_bases(captures: &[&ZoneCapture], map: &str) -> Vec<String> {
    let mut bases: Vec<String> = Vec::new();
    let mut push = |b: String| {
        if !b.is_empty() && !bases.contains(&b) {
            bases.push(b);
        }
    };
    if let Some(location) = map.strip_prefix("zm_") {
        push(format!("zmb_{location}"));
        push(format!("zmb_{location}_real"));
        push(format!("zmb_{location}_real_intro"));
    }
    for c in captures {
        for b in &c.sound_banks {
            let base = b
                .name
                .rsplit_once('.')
                .map_or(b.name.as_str(), |(base, _)| base);
            push(base.to_owned());
        }
    }
    let deps: [&str; 3] = if is_mp_map(map) {
        ["mpl_common", "mpl_code_post_gfx", "cmn_root"]
    } else {
        ["zmb_common", "zmb_code_post_gfx", "cmn_root"]
    };
    for dep in deps {
        push(dep.to_owned());
    }
    bases
}

/// Load speed: the zones the combat stage reads besides the map's own (the
/// shared and faction zones before it, the mode zones after it, the English
/// ones), each read and parsed on its own thread. The map lane starts this on
/// a thread of its own before the map's textures decode, so the disk and the
/// cores do both at once.
pub(crate) struct T6CombatZones {
    pre: Vec<ZoneCapture>,
    post: Vec<ZoneCapture>,
    lang: Vec<ZoneCapture>,
    report: Vec<String>,
}

pub(crate) fn read_combat_zones(map_path: &Path) -> T6CombatZones {
    let mut report = Vec::new();
    let (pre, post) = zone_paths(map_path);
    let zone_dir = asset_transport::t6_extra::install_zone_dir(map_path);
    let started = std::time::Instant::now();
    // Every path read on a thread of its own; results come back in path order.
    let capture_each = |paths: &[PathBuf]| -> Vec<Result<ZoneCapture, String>> {
        std::thread::scope(|scope| {
            let threads: Vec<_> = paths
                .iter()
                .map(|path| scope.spawn(move || asset_t6::capture_zone(path)))
                .collect();
            threads
                .into_iter()
                .map(|t| t.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
                .collect()
        })
    };
    let capture_all = |paths: &[PathBuf], gap: &str, report: &mut Vec<String>| -> Vec<ZoneCapture> {
        capture_each(paths)
            .into_iter()
            .filter_map(|result| result.map_err(|e| report.push(format!("{gap}: {e}"))).ok())
            .collect()
    };
    let mod_zone = asset_transport::t6_mod::zone();
    let (pre_results, (post_caps, post_report)) = std::thread::scope(|scope| {
        let post_thread = scope.spawn(|| {
            let mut post_report = Vec::new();
            let caps = capture_all(&post, "t6 zone gap", &mut post_report);
            (caps, post_report)
        });
        let pre_results = capture_each(&pre);
        (
            pre_results,
            post_thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        )
    });
    let mut pre_caps: Vec<ZoneCapture> = Vec::with_capacity(pre.len());
    for (path, result) in pre.iter().zip(pre_results) {
        let mut c = match result {
            Ok(c) => c,
            Err(e) => {
                report.push(format!("t6 zone gap: {e}"));
                continue;
            }
        };
        // bo2mp: the mod's zone without what would change the game (its
        // string tables and scripts), with the animations its archive holds.
        if mod_zone.as_ref() == Some(path) {
            complete_mod_capture(&mut c, &pre_caps, &mut report);
        }
        pre_caps.push(c);
    }
    report.extend(post_report);
    let map = map_path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    // bo2mp: a multiplayer map's factions (bodies, arms, team voices).
    if is_mp_map(&map) {
        let dir = zone_dir.as_path();
        let factions = mp_faction_zones(&pre_caps, &map, dir, &mut report);
        pre_caps.extend(capture_all(&factions, "t6 zone gap", &mut report));
    }
    // The language zones beside zone/all (zone/english/en_*.ff): the
    // English strings (hints, perk and power-up names) and the voices'
    // aliases (the announcer's, the players'). bo2zm: and the mode zone's
    // (`en_so_zclassic_zm_transit.ff`: TranZit's player and bus driver
    // lines; its `en_zm_transit.ff` is empty, so the driver never spoke).
    let lang_dir = Some(zone_dir.as_path())
        .and_then(Path::parent)
        .map(|d| d.join("english"));
    let lang_zones: Vec<String> = if is_mp_map(&map) {
        // bo2mp: the shared zones', then the factions' (team voices), then
        // the map's.
        T6_MP_LANG_ZONES
            .iter()
            .map(|z| (*z).to_owned())
            .chain(
                pre_caps
                    .iter()
                    .chain(post_caps.iter())
                    .filter(|c| c.zone.starts_with("faction_"))
                    .map(|c| c.zone.clone()),
            )
            .chain(std::iter::once(map.clone()))
            .collect()
    } else {
        let mode = mode_zone(&map);
        [
            "code_post_gfx_zm",
            "common_zm",
            "patch_zm",
            "ui_zm",
            "patch_ui_zm",
            map.as_str(),
            mode.as_str(),
        ]
        .iter()
        .map(|z| (*z).to_owned())
        .collect()
    };
    let lang_paths: Vec<PathBuf> = lang_zones
        .iter()
        .filter_map(|z| {
            let path = asset_transport::t6_extra::file_in(lang_dir.as_ref()?, &format!("en_{z}.ff"));
            path.is_file().then_some(path)
        })
        .collect();
    let lang = capture_all(&lang_paths, "t6 language zone gap", &mut report);
    diag::info!(
        World,
        "t6 combat: {} zones read and parsed in {:.0} ms",
        pre_caps.len() + post_caps.len() + lang.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    T6CombatZones {
        pre: pre_caps,
        post: post_caps,
        lang,
        report,
    }
}

/// `zones`: the ones `read_combat_zones` read ahead (None: read them now).
/// A later start with the same zone files reads the finished combat load
/// back (`t6_combat_cache`), guns included, instead of walking the zones.
pub(crate) fn load_t6_combat(
    map_path: &Path,
    map_capture: &ZoneCapture,
    materials: &mut MaterialCatalog,
    packs: Option<&PackSet>,
    zones: Option<T6CombatZones>,
) -> T6Combat {
    let zone_dir = asset_transport::t6_extra::install_zone_dir(map_path);
    super::t6_materials::note_card_dir(&zone_dir);
    let files = t6_cache_files(map_path);
    // The player's own camos folder is part of what the file holds (its
    // pictures and the camo numbers), so a changed folder makes it stale.
    let note = format!("{}|camos:{:016x}", capture_note(map_capture), camos_folder_key());
    let code = [
        include_str!("finished_gun_cache.rs"),
        include_str!("t6_combat_cache.rs"),
        include_str!("t6_m2.rs"),
    ];
    let stem = map_path
        .file_stem()
        .map_or_else(|| "map".to_owned(), |s| s.to_string_lossy().into_owned());
    let names = vec![stem.clone()];
    let src = super::finished_gun_cache::Sources {
        lane: "t6",
        name: &stem,
        files: &files,
        caps: &[],
        code: &code,
        zones: &names,
        note: &note,
        missing_ok: true,
    };
    let at = super::finished_gun_cache::MatAt::of(materials);
    let started = std::time::Instant::now();
    if let Some(body) = super::finished_gun_cache::read_body(&src) {
        let read_ms = started.elapsed().as_secs_f32() * 1000.0;
        if let Some(combat) = super::t6_combat_cache::unpack(&body, materials) {
            replay_visions(&stem, &combat);
            combat.camos.install();
            if let Some(dir) = map_bake::map_dir("t6", &stem) {
                diag::info!(
                    World,
                    "t6 guest guns: {stem} reads {} ({:.1} MB) in {:.0} ms (read {read_ms:.0} ms)",
                    dir.join("finished_guns.pak").display(),
                    body.len() as f64 / (1024.0 * 1024.0),
                    started.elapsed().as_secs_f32() * 1000.0
                );
            }
            return combat;
        }
        diag::warn!(World, "t6 guest guns: {stem} cache unreadable, built again");
    }
    let combat = load_t6_combat_now(map_path, map_capture, materials, packs, zones);
    if !super::finished_gun_cache::enabled() {
        // (Packing takes 1.7 s on Stronghold: not for a file nobody writes.)
        return combat;
    }
    let packing = std::time::Instant::now();
    match super::t6_combat_cache::pack(&combat, materials, at) {
        Ok(bytes) => {
            let pack_ms = packing.elapsed().as_secs_f32() * 1000.0;
            let writing = std::time::Instant::now();
            super::finished_gun_cache::write_logged(&src, &bytes);
            diag::info!(
                World,
                "t6 guest guns: {stem} packed in {pack_ms:.0} ms, written in {:.0} ms",
                writing.elapsed().as_secs_f32() * 1000.0
            );
        }
        Err(why) => diag::warn!(World, "t6 guest guns: not cached ({why})"),
    }
    combat
}

/// A key over the player's camos folder (every file's name, size and time):
/// its camos are baked into the finished-gun file.
fn camos_folder_key() -> u64 {
    let dir = asset_core::weapon_camos::user_camos_dir();
    let mut key = map_bake::Key::new();
    key.str(&dir.to_string_lossy().to_ascii_lowercase());
    let mut files: Vec<(PathBuf, Option<std::fs::Metadata>)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.path(), e.metadata().ok()))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    key.u64(files.len() as u64);
    for (path, meta) in files {
        key.str(&path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_ascii_lowercase()));
        let len = meta.as_ref().map_or(u64::MAX, std::fs::Metadata::len);
        let at = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        key.u64(len).bytes(&at.to_le_bytes());
    }
    key.value()
}

fn t6_cache_files(map_path: &Path) -> Vec<PathBuf> {
    let (pre, post) = zone_paths(map_path);
    let mut files = pre;
    files.push(map_path.to_path_buf());
    files.extend(post);
    let zone_dir = asset_transport::t6_extra::install_zone_dir(map_path);
    if let Ok(read) = std::fs::read_dir(&zone_dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".ff") {
                files.push(entry.path());
            }
        }
    }
    if let Some(english) = zone_dir.parent().map(|d| d.join("english")) {
        if let Ok(read) = std::fs::read_dir(english) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("en_") && name.ends_with(".ff") {
                    files.push(entry.path());
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn capture_note(capture: &ZoneCapture) -> String {
    let mut names: Vec<&str> = capture.weapons.iter().map(|w| w.name.as_str()).collect();
    names.extend(capture.fx.iter().map(|fx| fx.name.as_str()));
    names.sort_unstable();
    format!("{}|{}", capture.zone, names.join(","))
}

fn replay_visions(map: &str, combat: &T6Combat) {
    if !is_mp_map(map) {
        return;
    }
    let visions = combat
        .visions
        .iter()
        .filter_map(|(name, bytes)| {
            let name = name.strip_prefix("vision/")?.strip_suffix(".vision")?.to_owned();
            Some((name, asset_world::parse_t6_vision(&String::from_utf8_lossy(bytes))?))
        })
        .collect();
    asset_world::register_t6_visions(visions);
}

fn load_t6_combat_now(
    map_path: &Path,
    map_capture: &ZoneCapture,
    materials: &mut MaterialCatalog,
    packs: Option<&PackSet>,
    zones: Option<T6CombatZones>,
) -> T6Combat {
    let zone_dir = asset_transport::t6_extra::install_zone_dir(map_path);
    super::t6_materials::note_card_dir(&zone_dir);
    let T6CombatZones {
        pre: pre_caps,
        post: post_caps,
        lang: lang_caps,
        mut report,
    } = zones.unwrap_or_else(|| read_combat_zones(map_path));
    let mut parts = CombatParts::start();
    let map_stem = map_path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let captures: Vec<&ZoneCapture> = pre_caps
        .iter()
        .chain(std::iter::once(map_capture))
        .chain(post_caps.iter())
        .collect();
    report.push(format!(
        "t6 combat zones: {}",
        captures
            .iter()
            .map(|c| c.zone.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    // bo2mp: the bullet penetration depths every gun's penetrate type
    // reads (`BULLET_PEN_TABLE`, keyed by Black Ops II's 32 surfaces).
    let mut pen_table = None;
    for (name, bytes) in captures.iter().flat_map(|c| c.raw_files.iter()) {
        if name.replace('\\', "/") != "info/bullet_penetration_mp" {
            continue;
        }
        match asset_game::parse_pen_table_t6(bytes) {
            Ok(table) => pen_table = Some(table),
            Err(e) => report.push(format!("t6 bullet penetration table: {e:?}")),
        }
    }
    match pen_table {
        Some(table) => report.push(format!(
            "t6 bullet penetration table: flesh small {} medium {} large {}, wood large {}",
            table.depth(1, 7),
            table.depth(2, 7),
            table.depth(3, 7),
            table.depth(3, 21)
        )),
        None => report.push("t6 bullet penetration table: no info/bullet_penetration_mp".to_owned()),
    }

    // bo2mp vehicle screens: the match zones' named visions
    // (`vision/<name>.vision`, later zones winning a name) for the scripts'
    // `visionsetnaked` and `setvisionsetforplayer` (the VTOL Warship's
    // `remote_mortar_enhanced`, _helicopter_gunner.gsc).
    if is_mp_map(&map_stem) {
        let visions: Vec<(String, asset_world::T6Vision)> = captures
            .iter()
            .flat_map(|c| c.raw_files.iter())
            .filter_map(|(name, bytes)| {
                let n = name.replace('\\', "/");
                let n = n.strip_prefix("vision/")?.strip_suffix(".vision")?.to_owned();
                Some((n, asset_world::parse_t6_vision(&String::from_utf8_lossy(bytes))?))
            })
            .collect();
        report.push(format!(
            "t6 match visions: {} ({})",
            visions.len(),
            visions.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ));
        asset_world::register_t6_visions(visions);
    }

    // Weapons: later zones replace earlier ones of the same name.
    let mut weapon_refs: HashMap<String, &asset_t6::WeaponRef> = HashMap::new();
    let mut weapon_order: Vec<String> = Vec::new();
    // bo2mp: the scope overlay of each weapon, from the latest of its
    // zones' copies that names one: a map zone's copy of a gun can leave it
    // unset (its reference to the shared zone's material not kept), and the
    // gun would aim unscoped on that map only.
    let mut overlay_names: HashMap<String, String> = HashMap::new();
    for c in &captures {
        for w in c.weapons.iter().filter(|w| real(&w.name)) {
            if weapon_refs.insert(w.name.clone(), w).is_none() {
                weapon_order.push(w.name.clone());
            }
            if let Some(m) = [&w.overlay_material, &w.overlay_material_low]
                .into_iter()
                .find(|m| !m.is_empty())
            {
                overlay_names.insert(w.name.clone(), m.clone());
            }
        }
    }
    // bo2mp: each multiplayer gun's sight attachments (`au_<gun>_<sight>`
    // with its `<sight>` attachment), later zones winning a name.
    let mut attachments: HashMap<&str, &asset_t6::AttachmentRef> = HashMap::new();
    let mut uniques: HashMap<&str, &asset_t6::AttachmentUniqueRef> = HashMap::new();
    let mut unique_overlays: HashMap<&str, &str> = HashMap::new();
    for c in &captures {
        for a in c.attachments.iter().filter(|a| real(&a.name)) {
            attachments.insert(a.name.as_str(), a);
        }
        for u in c.attachment_uniques.iter().filter(|u| real(&u.name)) {
            uniques.insert(u.name.as_str(), u);
            if let Some(m) = [&u.overlay_material, &u.overlay_material_low]
                .into_iter()
                .find(|m| !m.is_empty())
            {
                unique_overlays.insert(u.name.as_str(), m.as_str());
            }
        }
    }
    let mut sights: Vec<(String, &asset_t6::AttachmentRef, &asset_t6::AttachmentUniqueRef)> = Vec::new();
    for name in &weapon_order {
        let Some(base) = name.strip_suffix("_mp") else { continue };
        for sight in asset_game::T6_SIGHT_ATTACHMENTS {
            if let (Some(a), Some(u)) = (
                attachments.get(sight),
                uniques.get(format!("au_{base}_{sight}").as_str()),
            ) {
                sights.push((name.clone(), a, u));
            }
        }
    }
    let pap = pap_camo(&captures, &weapon_refs, 39);
    report.push(format!(
        "t6 Pack-a-Punch camo: {} gun models, {} upgraded weapons",
        pap.models.len(),
        pap.weapon_models.len()
    ));
    let mut model_names: BTreeSet<String> = BTreeSet::new();
    for w in weapon_refs.values() {
        if let Some(gun) = w.gun_models.first().filter(|n| real(n)) {
            model_names.insert(gun.clone());
        }
        let hands = w.def_model(fastfile_t6::layout::WeaponDef::handXModel);
        if real(hands) {
            model_names.insert(hands.to_owned());
        }
        // The magazine and (snipers) the scope, attached to the gun.
        for (_, model, ..) in &w.attach_view_models {
            if real(model) {
                model_names.insert(model.clone());
            }
        }
        // What a launcher shows loaded and fires: the RPG's rocket, the
        // ballistic knife's blade, the knife a gun's melee swings.
        for field in [
            fastfile_t6::layout::WeaponDef::rocketModel,
            fastfile_t6::layout::WeaponDef::projectileModel,
            fastfile_t6::layout::WeaponDef::additionalMeleeModel,
        ] {
            let model = w.def_model(field);
            if real(model) {
                model_names.insert(model.to_owned());
            }
        }
    }
    // bo2mp: the sight attachments' models.
    for (_, _, u) in &sights {
        for model in [&u.view_model, &u.view_model_additional, &u.view_model_ads] {
            if real(model) {
                model_names.insert(model.clone());
            }
        }
    }
    // A multiplayer map's arms come with its factions' class scripts
    // (`mpbody/class_*`), named below with the script models.
    if !is_mp_map(&map_stem) {
        model_names.insert(T6_PLAYER_ARMS.to_owned());
        for arms in T6_OTHER_ARMS {
            model_names.insert(arms.to_owned());
        }
        // Every other Zombies map's player arms (`c_zom_reporter_viewhands`
        // in Buried's mode zone): whatever viewhands its zones carry.
        for c in &captures {
            for m in c.xmodels.iter() {
                if real(&m.name) && m.name.contains("_viewhands") {
                    model_names.insert(m.name.clone());
                }
            }
        }
    }
    // bo2zm M3: models the map's own scripts and entities name (perk
    // machines, the box, power-ups, zombie bodies and heads...), drawn as
    // script models and actors from the same catalog.
    let destructibles = destructible_models(&captures);
    let script_models = script_model_names(&captures, &destructibles);
    report.push(format!(
        "t6 script models: {} named by the map's scripts and entities",
        script_models.len()
    ));
    model_names.extend(script_models);

    // Third-person gun models too (a weapon cannot be given without one).
    let mut world_names: BTreeSet<String> = BTreeSet::new();
    for w in weapon_refs.values() {
        if let Some(world) = w.world_models.first().filter(|n| real(n)) {
            world_names.insert(world.clone());
        }
    }
    // Effect models: shell casings, debris.
    let mut fx_model_names: BTreeSet<String> = BTreeSet::new();
    for c in &captures {
        for e in c.fx.iter().filter(|e| real(&e.name)) {
            for el in &e.elems {
                for v in &el.visuals {
                    if let asset_t6::FxVisualRef::Model(n) = v
                        && real(n)
                    {
                        fx_model_names.insert(n.clone());
                    }
                }
            }
        }
    }
    let all_models: BTreeSet<String> = model_names
        .union(&world_names)
        .cloned()
        .chain(fx_model_names.iter().cloned())
        .collect();

    // Materials the guns, arms and effects draw with, linked per zone.
    let homes = material_homes(&captures);
    let mut wanted = wanted_materials(&captures, &all_models, &homes);
    // bo2mp: the material camos' materials (gold, diamond, ...), and the
    // guns' camo-ready ones they replace.
    let camo_plan = mp_material_camos(&captures);
    let pattern_plan = mp_pattern_camos(&captures);
    for (base, camo, _) in camo_plan.values().flatten() {
        for name in [base, camo] {
            if let Some(&(ci, mi)) = homes.get(name.trim_start_matches(',')) {
                wanted[ci].insert(mi);
            }
        }
    }
    // The map's brush models with surfaces (doors), drawn as models.
    let map_ci = pre_caps.len();
    let brush_models = map_capture
        .world
        .as_ref()
        .map(brush_model_refs)
        .unwrap_or_default();
    for srf in brush_models.iter().flat_map(|m| m.lod0.iter()) {
        if let Some(k) = srf.material {
            wanted[map_ci].insert(k.index);
        }
    }
    for (_, swaps) in pap.models.values() {
        for (_, camo, _) in swaps {
            if let Some(&(ci, mi)) = homes.get(camo.trim_start_matches(',')) {
                wanted[ci].insert(mi);
            }
        }
    }
    // Images by name, from the zone that holds their pixels.
    let mut image_homes: HashMap<String, asset_t6::ImageRef> = HashMap::new();
    for c in &captures {
        for img in &c.images {
            if real(&img.name) {
                image_homes.insert(img.name.clone(), img.clone());
            }
        }
    }
    // lane B: every zone's pictures for the stage (and the earlier-zone
    // colour maps and camo patterns below) read ahead in one disk-order
    // sweep; each zone's link below then finds them cached.
    if let Some(packs) = packs {
        let t = std::time::Instant::now();
        let flat = materials.images.iter().filter_map(|img| {
            let name = img.name.as_str().strip_suffix("#flat")?.strip_prefix(',')?;
            image_homes.get(name)
        });
        let camos = pattern_plan
            .values()
            .flatten()
            .filter_map(|(_, image, _)| image_homes.get(image.as_str()));
        let (n, bytes, cold) = super::t6_materials::warm_captures(
            packs,
            &captures,
            &wanted,
            Some(&image_homes),
            flat.chain(camos),
        );
        report.push(format!(
            "t6 combat pictures read ahead: {n} pack entries, {} MB in {} ms; cache {}, {} MB from the packs",
            bytes >> 20,
            t.elapsed().as_millis(),
            if cold == 0 { "hit" } else if cold == bytes { "miss" } else { "partial" },
            cold >> 20
        ));
    }
    // bo2mp: a world colour map named `,<image>` is a picture an earlier
    // zone holds (Carrier's parked jets and drone); the world, linked before
    // those zones were read, drew it flat. Give it its pixels now.
    if let Some(packs) = packs {
        let (mut given, mut left) = (0usize, Vec::new());
        for img in materials.images.iter_mut() {
            let Some(name) = img
                .name
                .as_str()
                .strip_suffix("#flat")
                .and_then(|n| n.strip_prefix(','))
            else {
                continue;
            };
            match image_homes
                .get(name)
                .map(|real| (real, super::t6_materials::decode(packs, real)))
            {
                Some((real, Ok(gpu))) => {
                    img.decoded = Some(std::sync::Arc::new(gpu));
                    img.map_type = real.map_type;
                    img.width = real.width;
                    img.height = real.height;
                    img.depth = real.depth;
                    img.level_count = real.level_count;
                    given += 1;
                }
                _ => left.push(name.to_owned()),
            }
        }
        report.push(format!(
            "t6 world colour maps from earlier zones: {given} given pixels, {} not held anywhere {left:?}",
            left.len()
        ));
    }
    let mut local: Vec<HashMap<usize, usize>> = Vec::with_capacity(captures.len());
    for (c, want) in captures.iter().zip(&wanted) {
        let linked = super::t6_materials::link_materials_with(
            materials,
            c,
            packs,
            want.iter().copied(),
            &c.zone,
            Some(&image_homes),
        );
        report.extend(
            linked
                .report
                .into_iter()
                .map(|line| format!("{} ({}): {line}", "t6 combat", c.zone)),
        );
        local.push(linked.local);
    }
    // bo2mp: the camos by catalog material, for the draw.
    let camos = {
        let catalog_of = |name: &str| {
            let &(ci, mi) = homes.get(name.trim_start_matches(','))?;
            local.get(ci)?.get(&mi).map(|&i| i as u32)
        };
        let mut table = asset_core::weapon_camos::CamoTable::default();
        for (&option, list) in &camo_plan {
            for (base, camo, tile) in list {
                if let (Some(b), Some(c)) = (catalog_of(base), catalog_of(camo)) {
                    table.swaps.insert((option, b), asset_core::weapon_camos::MaterialCamo { material: c, tile: *tile });
                }
            }
            if list.iter().any(|(b, c, _)| catalog_of(b).is_some() && catalog_of(c).is_some()) {
                table.options.insert(option, String::new());
            }
        }
        // The pattern pictures (no material draws them): each linked once,
        // its texture slot for the draw.
        let mut pictures: HashMap<String, Option<u32>> = HashMap::new();
        let image_refs: HashMap<&str, &asset_t6::ImageRef> = captures
            .iter()
            .flat_map(|c| c.images.iter())
            .filter(|i| i.name.starts_with("t6_camo_"))
            .map(|i| (i.name.as_str(), i))
            .collect();
        for (&option, list) in &pattern_plan {
            for (base, image, scale) in list {
                let Some(b) = catalog_of(base) else { continue };
                let slot = *pictures.entry(image.clone()).or_insert_with(|| {
                    let r = image_refs.get(image.as_str())?;
                    let gpu = super::t6_materials::decode(packs?, r).ok()?;
                    Some(materials.link_image(asset_material::AuthoredImage {
                        namespace: asset_core::AssetNamespace::T6,
                        name: asset_core::AssetRef::Real(r.name.clone()),
                        map_type: r.map_type,
                        semantic: asset_material::TS_COLOR_MAP,
                        category: 0,
                        use_srgb_reads: true,
                        width: r.width,
                        height: r.height,
                        depth: r.depth,
                        level_count: r.level_count,
                        format: 0,
                        payload: std::sync::Arc::new(Vec::new()),
                        decoded: Some(std::sync::Arc::new(gpu)),
                        common_owned: false,
                        decoded_variant: None,
                        decoded_by: None,
                        pending_decode: None,
                    }) as u32)
                });
                if let Some(image) = slot {
                    table.patterns.insert((option, b), asset_core::weapon_camos::PatternCamo::still(image, *scale, false));
                }
            }
        }
        report.push(format!(
            "t6 camos: {} pattern camo pictures, {} gun materials patterned",
            pictures.values().filter(|s| s.is_some()).count(),
            table.patterns.len()
        ));
        report.push(format!(
            "t6 camos: {} material camos ({} gun materials swapped) of {} planned",
            table.options.len(),
            table.swaps.len(),
            camo_plan.len()
        ));
        let table = (!table.swaps.is_empty() || !table.patterns.is_empty()).then_some(table);
        // bo2mp: Black Ops' camos, its camo pictures from its IWDs, each a
        // pattern over a whole gun material of every game's guns.
        let mut extra = HashMap::new();
        for (n, _, image, _) in asset_game::weapon_camo_models::BO1_CAMOS {
            let Some(img) = asset_t6::legacy_images::legacy_iwd_mains()
                .iter()
                .find_map(|main| asset_material::decode_iwd_colour_image(main, image).and_then(Result::ok))
            else {
                continue;
            };
            let (width, height) = (img.width() as u16, img.height() as u16);
            let slot = materials.link_image(asset_material::AuthoredImage {
                namespace: asset_core::AssetNamespace::T5,
                name: asset_core::AssetRef::Real(format!("t5:{image}")),
                map_type: 0,
                semantic: asset_material::TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width,
                height,
                depth: 1,
                level_count: img.texture_descriptor.mip_level_count as u8,
                format: 0,
                payload: std::sync::Arc::new(Vec::new()),
                decoded: Some(std::sync::Arc::new(img)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            }) as u32;
            extra.insert(
                asset_core::weapon_camos::BO1_FIRST + u32::from(n),
                asset_core::weapon_camos::PatternCamo::still(slot, 2.0, true),
            );
        }
        report.push(format!("bo1 camos: {} of {} pictures read", extra.len(), asset_game::weapon_camo_models::BO1_CAMOS.len()));
        // bo2mp: the player's own camos (the `camos` folder: PNGs and camo
        // packs, `asset_core::camo_pack`), each a pattern over a whole gun
        // material of every game's guns, as Black Ops' are; they move by
        // their pack's say (frames, a scroll, a shader look).
        let mut own = 0usize;
        for u in asset_core::weapon_camos::user_camos() {
            for note in &u.notes {
                report.push(format!("own camo {}: {note}", u.name));
            }
            let Some((width, height, rgba)) = read_pack_picture(&u.source) else {
                report.push(format!("own camo {}: could not be read (PNGs up to 4096 x 4096; a sheet no smaller than its grid)", u.name));
                continue;
            };
            let mut img = bevy::prelude::Image::new(
                bevy::render::render_resource::Extent3d { width, height, depth_or_array_layers: 1 },
                bevy::render::render_resource::TextureDimension::D2,
                rgba,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::default(),
            );
            img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                address_mode_u: bevy::image::ImageAddressMode::Repeat,
                address_mode_v: bevy::image::ImageAddressMode::Repeat,
                ..bevy::image::ImageSamplerDescriptor::linear()
            });
            let slot = materials.link_image(asset_material::AuthoredImage {
                namespace: asset_core::AssetNamespace::T5,
                name: asset_core::AssetRef::Real(format!("user:{}:{}", u.number, u.name)),
                map_type: 0,
                semantic: asset_material::TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width: width as u16,
                height: height as u16,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: std::sync::Arc::new(Vec::new()),
                decoded: Some(std::sync::Arc::new(img)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            }) as u32;
            extra.insert(u.number, asset_core::weapon_camos::PatternCamo { image: slot, scale: u.scale, full: true, style: u.style });
            own += 1;
        }
        if own > 0 {
            report.push(format!("own camos: {own} from the camos folder"));
        }
        // bo2mp: the guest guns' Pack-a-Punch upgrades wear the map's
        // Pack-a-Punch camo: the camo material its upgraded guns' camo
        // set swaps in most, at its tiling.
        let mut counts: BTreeMap<(&str, [u32; 2]), usize> = BTreeMap::new();
        for (_, swaps) in pap.models.values() {
            for (_, camo, tile) in swaps {
                *counts.entry((camo.trim_start_matches(','), tile.map(f32::to_bits))).or_default() += 1;
            }
        }
        let guest_pap = counts
            .iter()
            .max_by_key(|(_, n)| **n)
            .and_then(|((camo, tile), _)| {
                let material = catalog_of(camo)?;
                Some((
                    bo2_profile::guest::ZM_PAP_CAMO_OPTION,
                    asset_core::weapon_camos::MaterialCamo { material, tile: f32::from_bits(tile[0]) },
                ))
            });
        report.push(format!(
            "t6 Pack-a-Punch camo for guest guns: {}",
            guest_pap.map_or_else(|| "none".to_owned(), |(_, c)| format!("material {} tile {}", c.material, c.tile))
        ));
        T6Camos { table, extra, pap: guest_pap }
    };
    camos.install();

    // A model surface's material, in the catalog: the capture's own row
    // when it linked it, else the row of the zone that defines it.
    let catalog_material = |ci: usize, key: asset_t6::AssetKey| -> Option<usize> {
        if let Some(&i) = local[ci].get(&key.index) {
            return Some(i);
        }
        let name = captures[ci]
            .materials
            .get(key.index)?
            .name
            .trim_start_matches(',');
        let &(hc, hm) = homes.get(name)?;
        local[hc].get(&hm).copied()
    };
    let find_model = |name: &str| {
        captures
            .iter()
            .enumerate()
            .rev()
            .find_map(|(ci, c)| c.xmodels.iter().find(|m| m.name == name).map(|m| (ci, m)))
    };

    parts.mark("gun materials and camos");
    // First-person meshes: every gun and the players' arms.
    let mut fpv = FpvMeshBuild::default();
    fpv.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut fpv_missing = Vec::new();
    for name in &model_names {
        let Some((ci, model)) = find_model(name) else {
            fpv_missing.push(name.clone());
            continue;
        };
        // bo2mp: a sight's reticle surface is drawn by the HUD at
        // infinity (its shader's projection), not as a picture on the glass.
        let resolve = |k: asset_t6::AssetKey| {
            if reticle_material(&captures, &homes, ci, k).is_some() {
                return None;
            }
            catalog_material(ci, k)
        };
        match asset_model::capture_model_skel_t6(model, resolve) {
            Some(mut skel) => {
                skel.surface_flags = model_surface_flags(captures[ci], model);
                // bo2mp: what a script model wearing it is solid as.
                skel.collmaps = captures[ci]
                    .xmodel_collmaps
                    .get(&model.name)
                    .map(|c| {
                        c.iter()
                            .map(|b| xmodel_runtime::CollmapBox { mins: b.mins, maxs: b.maxs, contents: b.contents })
                            .collect()
                    })
                    .unwrap_or_default();
                fpv.insert_in(asset_core::AssetNamespace::T6, skel, Some(materials))
            }
            None => fpv_missing.push(format!("{name} (unreadable)")),
        }
    }
    // bo2mp: Modern Warfare III's (2023) operators as the soldiers, on a
    // Black Ops II soldier body's skeleton (`super::jup_ops`).
    super::jup_ops::add_operators(
        &mut fpv,
        materials,
        super::t5_soldiers::soldier_skeleton(captures.iter().flat_map(|c| c.xmodels.iter())),
        &mut report,
    );
    // bo2mp: or Black Ops' soldiers (`super::t5_soldiers`).
    super::t5_soldiers::add_soldiers(
        &mut fpv,
        materials,
        super::t5_soldiers::soldier_skeleton(captures.iter().flat_map(|c| c.xmodels.iter())),
        &mut report,
    );
    for model in &brush_models {
        match asset_model::capture_model_skel_t6(model, |k| catalog_material(map_ci, k)) {
            Some(skel) => fpv.insert_in(asset_core::AssetNamespace::T6, skel, Some(materials)),
            None => fpv_missing.push(format!("{} (brush model)", model.name)),
        }
    }
    if !brush_models.is_empty() {
        report.push(format!(
            "t6 brush models drawn: {}",
            brush_models
                .iter()
                .map(|m| format!(
                    "{} ({} surfaces [{}], {:.0} {:.0} {:.0} .. {:.0} {:.0} {:.0})",
                    m.name,
                    m.lod0.len(),
                    m.materials
                        .iter()
                        .flatten()
                        .filter_map(|k| map_capture.materials.get(k.index))
                        .map(|mat| mat.name.trim_start_matches(','))
                        .collect::<Vec<_>>()
                        .join(" "),
                    m.mins[0],
                    m.mins[1],
                    m.mins[2],
                    m.maxs[0],
                    m.maxs[1],
                    m.maxs[2]
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    // The upgraded guns' copies, their camo surfaces swapped.
    for (copy, (gun, swaps)) in &pap.models {
        let Some((ci, model)) = find_model(gun) else {
            continue;
        };
        // Each surface's camo tiling, in the order the surfaces are read.
        let tiles: std::cell::RefCell<Vec<[f32; 2]>> = std::cell::RefCell::new(Vec::new());
        let swapped = |k: asset_t6::AssetKey| -> Option<usize> {
            let name = captures[ci]
                .materials
                .get(k.index)
                .map_or("", |m| m.name.trim_start_matches(','));
            match swaps
                .iter()
                .find(|(base, _, _)| base.trim_start_matches(',') == name)
            {
                Some((_, camo, tile)) => {
                    tiles.borrow_mut().push(*tile);
                    let &(hc, hm) = homes.get(camo.trim_start_matches(','))?;
                    local[hc].get(&hm).copied()
                }
                None => {
                    tiles.borrow_mut().push([1.0, 1.0]);
                    catalog_material(ci, k)
                }
            }
        };
        if let Some(mut skel) = asset_model::capture_model_skel_t6(model, swapped) {
            skel.name = copy.clone();
            // The camo surfaces' texture coordinates times the camo's
            // tiling (BO2's camo shader scales them; ours samples as given).
            let tiles = tiles.into_inner();
            if tiles.len() == skel.surface_vertex_ranges.len() {
                for (&(start, count), tile) in skel.surface_vertex_ranges.iter().zip(&tiles) {
                    if *tile == [1.0, 1.0] {
                        continue;
                    }
                    for uv in skel.uvs.iter_mut().skip(start).take(count) {
                        uv[0] *= tile[0];
                        uv[1] *= tile[1];
                    }
                    // The packed rows the gun draws from: two halves, v low
                    // and u high (T6's u-low word turned a half round).
                    for row in skel.packed_vertices.iter_mut().skip(start).take(count) {
                        let tex = u32::from_le_bytes([row[20], row[21], row[22], row[23]]);
                        let v = asset_model::half_to_f32(tex as u16) * tile[1];
                        let u = asset_model::half_to_f32((tex >> 16) as u16) * tile[0];
                        let tex = u32::from(f32_to_half(v)) | (u32::from(f32_to_half(u)) << 16);
                        row[20..24].copy_from_slice(&tex.to_le_bytes());
                    }
                }
            }
            fpv.insert_in(asset_core::AssetNamespace::T6, skel, Some(materials));
        }
    }
    // Third-person gun models.
    let mut world_weapons = WorldWeaponBuild::default();
    world_weapons.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut world_missing = Vec::new();
    for name in &world_names {
        let Some((ci, model)) = find_model(name) else {
            world_missing.push(name.clone());
            continue;
        };
        match asset_model::capture_model_skel_t6(model, |k| catalog_material(ci, k)) {
            Some(skel) => {
                world_weapons.insert_in(asset_core::AssetNamespace::T6, skel, Some(materials))
            }
            None => world_missing.push(format!("{name} (unreadable)")),
        }
    }
    report.push(format!(
        "t6 world gun models: {} built, {} missing {:?}",
        world_weapons.len(),
        world_missing.len(),
        world_missing.iter().take(8).collect::<Vec<_>>()
    ));
    report.push(format!(
        "t6 first-person meshes: {} built, {} missing {:?}",
        fpv.len(),
        fpv_missing.len(),
        fpv_missing.iter().take(8).collect::<Vec<_>>()
    ));

    parts.mark("first-person, soldier and world gun meshes");
    // Animations: every one, later zones win.
    let mut xanims = XAnimBuild::default();
    xanims.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut anim_n = 0usize;
    for c in &captures {
        for a in &c.xanims {
            anim_n += usize::from(xanims.insert_t6(a));
        }
    }
    report.push(format!(
        "t6 animations: {anim_n} captured, {} kept, {} gaps",
        xanims.len(),
        xanims.capture_gaps
    ));

    parts.mark("animations");
    // The weapon catalog.
    let mut catalog = WeaponCatalog::default();
    catalog.set_capture_ns(asset_core::AssetNamespace::T6);
    for name in &weapon_order {
        let w = weapon_refs[name];
        // A dual-wield weapon's left-hand weapon (its gun and dw_left_*
        // animations join the right one's viewmodel).
        let left = weapon_refs
            .get(w.def_string(fastfile_t6::layout::WeaponDef::szDualWieldWeaponName))
            .copied();
        catalog.capture_t6(w, left, &pap.weapon_models);
        if let Some(m) = overlay_names.get(name) {
            catalog.keep_t6_overlay(name, m);
        }
    }
    // bo2mp: every gun with each of its sight attachments.
    let mut sight_n = 0usize;
    let mut reticle_pictures: HashMap<(String, bool), (f32, std::sync::Arc<bevy::prelude::Image>)> =
        HashMap::new();
    let mut sight_reticles: Vec<(String, std::sync::Arc<bevy::prelude::Image>)> = Vec::new();
    for (name, a, u) in &sights {
        let optic = if real(&u.view_model) { u.view_model.as_str() } else { "" };
        let hybrid = a.name == "dualoptic" && !u.alt_weapon_name.is_empty();
        // The picture of one of the sight model's reticles (`top`: the
        // little dot above a Hybrid's scope, which its first mode aims
        // through; the other mode, and every other sight, the first below).
        let mut picture = |top: bool| match (packs, optic) {
            (_, "") | (None, _) => None,
            (Some(packs), optic) => match reticle_pictures.get(&(optic.to_owned(), top)) {
                Some(found) => Some(found.clone()),
                None => sight_reticle(&captures, &homes, packs, optic, top).map(|(scale, img)| {
                    let found = (scale, std::sync::Arc::new(img));
                    reticle_pictures.insert((optic.to_owned(), top), found.clone());
                    found
                }),
            },
        };
        let reticle = picture(hybrid);
        let anim_ms = |name: &str| {
            captures.iter().flat_map(|c| c.xanims.iter()).find(|a| a.name.eq_ignore_ascii_case(name)).and_then(|a| {
                (a.framerate > 0.0).then(|| (f64::from(a.numframes) / f64::from(a.framerate) * 1000.0).round() as i32)
            })
        };
        let scale = reticle.as_ref().map_or(0.0, |(scale, _)| *scale);
        if catalog.capture_t6_attachment(weapon_refs[name], a, u, scale, None, &anim_ms) {
            if let Some(m) = unique_overlays.get(u.name.as_str()) {
                catalog.keep_t6_overlay(&format!("{name}+{}", a.name), m);
            }
            // Hybrid Optic: its other mode is a weapon of its own, with the
            // other reticle.
            if hybrid && let Some(alt) = weapon_refs.get(u.alt_weapon_name.as_str()) {
                let other = picture(false);
                let other_scale = other.as_ref().map_or(0.0, |(scale, _)| *scale);
                if catalog.capture_t6_attachment(alt, a, u, other_scale, Some(&format!("{name}+{}", a.name)), &anim_ms) {
                    if let Some((_, img)) = other {
                        sight_reticles.push((format!("reticle:{}+{}", u.alt_weapon_name, a.name), img));
                    }
                }
            }
            sight_n += 1;
            if let Some((_, img)) = reticle {
                sight_reticles.push((format!("reticle:{name}+{}", a.name), img));
            }
        }
    }
    // bo2mp: Select Fire (`sf`): each gun with an `au_<gun>_sf` unique
    // naming its alternate (`sf_an94_mp`), plain and with each sight.
    let mut sf_n = 0usize;
    for name in &weapon_order {
        let Some(base) = name.strip_suffix("_mp") else { continue };
        let Some(u) = uniques.get(format!("au_{base}_sf").as_str()) else { continue };
        let Some(sf) = weapon_refs.get(u.alt_weapon_name.as_str()) else { continue };
        let mut froms = vec![name.clone()];
        froms.extend(sights.iter().filter(|(n, a, _)| n == name && a.name != "dualoptic").map(|(n, a, _)| format!("{n}+{}", a.name)));
        let anim_ms = |name: &str| {
            captures.iter().flat_map(|c| c.xanims.iter()).find(|a| a.name.eq_ignore_ascii_case(name)).and_then(|a| {
                (a.framerate > 0.0).then(|| (f64::from(a.numframes) / f64::from(a.framerate) * 1000.0).round() as i32)
            })
        };
        for from in froms {
            sf_n += usize::from(catalog.capture_t6_select_fire(&from, sf, u, &anim_ms).is_some());
        }
    }
    report.push(format!(
        "t6 select fire: {sf_n} guns and sight variants with Select Fire"
    ));
    // bo2mp: FMJ (an attachment that changes how deep bullets go): each gun
    // with an `au_<gun>_<attachment>` unique, plain and with each sight.
    let mut pen_n = 0usize;
    let mut piercing: Vec<&asset_t6::AttachmentRef> = attachments
        .values()
        .copied()
        .filter(|a| asset_game::t6_attachment_penetrates(a))
        .collect();
    piercing.sort_by(|a, b| a.name.cmp(&b.name));
    for name in &weapon_order {
        let Some(base) = name.strip_suffix("_mp") else {
            continue;
        };
        for a in &piercing {
            if !uniques.contains_key(format!("au_{base}_{}", a.name).as_str()) {
                continue;
            }
            let mut froms = vec![name.clone()];
            froms.extend(
                sights
                    .iter()
                    .filter(|(n, s, _)| n == name && s.name != "dualoptic")
                    .map(|(n, s, _)| format!("{n}+{}", s.name)),
            );
            for from in froms {
                pen_n += usize::from(catalog.capture_t6_penetration(&from, a).is_some());
            }
        }
    }
    report.push(format!(
        "t6 penetration attachments: {pen_n} guns and sight variants ({:?})",
        piercing.iter().map(|a| a.name.as_str()).collect::<Vec<_>>()
    ));
    report.push(format!(
        "t6 sight reticles: {} sight models with a reticle ({:?})",
        reticle_pictures.len(),
        reticle_pictures.iter().map(|((m, top), (s, _))| format!("{m}{} {s}", if *top { " (top)" } else { "" })).collect::<Vec<_>>()
    ));
    report.push(format!(
        "t6 sight attachments: {sight_n} guns with a sight ({} attachments, {} uniques read)",
        attachments.len(),
        uniques.len()
    ));
    // bo2zm fix list 2: a rolling grenade rests and rolls on its body: the
    // nearest face of its projectile model's bounds to the model's origin
    // (the frag: a ball of about 1.4 units around it, the fuse above).
    let mut rolling = Vec::new();
    for name in &weapon_order {
        let w = weapon_refs[name];
        if w.def_i32(fastfile_t6::layout::WeaponDef::isRollingGrenade) == 0 {
            continue;
        }
        let model = w.def_model(fastfile_t6::layout::WeaponDef::projectileModel);
        if let Some((_, m)) = find_model(model) {
            let radius = (0..3)
                .flat_map(|a| [m.mins[a].abs(), m.maxs[a].abs()])
                .fold(f32::MAX, f32::min);
            if radius.is_finite() && radius > 0.1 {
                catalog.set_t6_rolling_radius(name, radius);
                rolling.push(format!("{name} {radius:.2}"));
            }
        }
    }
    report.push(format!("t6 rolling grenades: {}", rolling.join(", ")));
    let captured = catalog.len();
    let mut weapons = catalog.into_build();
    weapons.stamp_namespace(asset_core::AssetNamespace::T6);
    // bo2mp: Black Ops II's attachment numbers, for guest guns' variants.
    weapons.set_bo2_attachment_stats(
        attachments
            .iter()
            .map(|(name, a)| ((*name).to_owned(), asset_game::Bo2AttachmentStats::from_t6(a)))
            .collect(),
    );
    // (And its suppressors' quiet shots, `an94_silencer`.)
    weapons.set_bo2_attachment_sounds(
        uniques
            .iter()
            .filter_map(|(name, u)| {
                let key = name.strip_prefix("au_").filter(|k| !k.contains('+') && k.contains("_silencer"))?;
                use fastfile_t6::layout::WeaponAttachmentUnique as f;
                use asset_game::WeaponSoundSlot as S;
                // (Its last shot is the same quiet shot.)
                let sounds: Vec<(S, String)> = u
                    .fire_sounds
                    .iter()
                    .flat_map(|(off, alias)| {
                        let slots: &[S] = match *off {
                            f::fireSound => &[S::Fire, S::FireLast],
                            f::fireSoundPlayer => &[S::FirePlayer, S::FireLastPlayer],
                            _ => &[],
                        };
                        slots.iter().map(move |slot| (*slot, format!("t6:{alias}")))
                    })
                    .collect();
                (!sounds.is_empty()).then(|| (key.to_owned(), sounds))
            })
            .collect(),
    );
    report.push(format!(
        "t6 weapons: {captured} captured, {} in the catalog",
        weapons.len()
    ));
    // BO2MP_EQUIP_AUDIT (docs/EQUIPMENT_AUDIT.md): which client scripts the
    // zones hold for the equipment whose lights and effects they add.
    if codlegacy_env::var_os("BO2MP_EQUIP_AUDIT").is_some() {
        for line in asset_game::equipment_audit::client_script_report(
            captures
                .iter()
                .flat_map(|c| c.raw_files.iter())
                .filter_map(|(name, _)| name.strip_prefix("script:")),
        ) {
            report.push(format!("equip audit: {line}"));
        }
    }

    parts.mark("weapon catalog");
    // Effects: every one, later zones win.
    let mut fx = FxCatalog::default();
    fx.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut fx_n = 0usize;
    for c in &captures {
        for e in &c.fx {
            fx_n += usize::from(fx.capture_t6(e));
        }
    }
    report.push(format!("t6 effects: {fx_n} captured, {} kept", fx.len()));
    // bo2zm fix list 3: a gun whose first-person flash is the flash other
    // players see (it names the same effect for both) gets a first-person
    // copy at `T6_FIRST_PERSON_FLASH_SCALE` (our choice; see there). Bullet
    // guns only: the launchers' flashes are launch smoke or already small.
    // A flash made for the first person keeps its own size even without
    // parts drawn with the gun (the Ray Gun's).
    let mut first_person = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for name in &weapon_order {
        let w = weapon_refs[name];
        let view = w.def_fx(fastfile_t6::layout::WeaponDef::viewFlashEffect);
        let world = w.def_fx(fastfile_t6::layout::WeaponDef::worldFlashEffect);
        let bullet = w.def_i32(fastfile_t6::layout::WeaponDef::weapType) == 0;
        if !bullet || view.is_empty() || view != world || !seen.insert(view) {
            continue;
        }
        if let Some((before, after)) = fx.add_t6_first_person_flash(view) {
            first_person.push(format!("{view} {before:.1}->{after:.1}"));
        }
    }
    report.push(format!(
        "t6 first-person flashes made from far-view ones (x{}): {}",
        asset_game::T6_FIRST_PERSON_FLASH_SCALE,
        first_person.join(", ")
    ));
    let mut fx_models = asset_game::FxModelCatalog::default();
    fx_models.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut fx_model_missing = Vec::new();
    for name in &fx_model_names {
        match find_model(name)
            .and_then(|(ci, m)| asset_model::capture_model_skel_t6(m, |k| catalog_material(ci, k)))
        {
            Some(skel) => fx_models.insert_skel(skel, materials),
            None => fx_model_missing.push(name.clone()),
        }
    }
    report.push(format!(
        "t6 effect models: {} built, {} missing {:?}",
        fx_models.len(),
        fx_model_missing.len(),
        &fx_model_missing[..fx_model_missing.len().min(8)]
    ));
    // Tracers: every one, later zones win.
    let mut tracers = asset_game::TracerCatalog::default();
    tracers.set_capture_ns(asset_core::AssetNamespace::T6);
    let mut tracer_n = 0usize;
    for c in &captures {
        for t in &c.tracers {
            tracer_n += usize::from(tracers.capture_t6(t));
        }
    }
    report.push(format!(
        "t6 tracers: {tracer_n} captured, {} kept",
        tracers.len()
    ));
    let impact_fx = captures
        .iter()
        .rev()
        .find_map(|c| c.impact_tables.first())
        .map(OwnedFxImpactTable::from_t6);
    report.push(format!(
        "t6 impact table: {}",
        impact_fx.as_ref().map_or_else(
            || "none".to_owned(),
            |t| format!("{} ({} rows)", t.name, t.row_count())
        )
    ));

    parts.mark("effects and tracers");
    // Sound: every bank's aliases, the clips in the bank files beside the
    // zones (`<install>/sound`).
    let map = map_path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let sound_dir = Some(zone_dir.as_path())
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|install| install.join("sound"));
    let (map_fx, room_tone) = map_scripts(&captures, &map, &mut report);
    // bo2mp: each alias's length in ms (its longest variant's clip), for
    // the scripts' soundgetplaybacktime (the announcer waits one line out).
    let mut sound_lengths: Vec<(String, u32)> = Vec::new();
    let sound = match sound_dir {
        Some(dir) if dir.is_dir() => {
            let bases = bank_bases(&captures, &map);
            let base_refs: Vec<&str> = bases.iter().map(String::as_str).collect();
            let (index, bank_report) = asset_audio::T6BankIndex::open_for_map(&dir, &base_refs, &map);
            report.extend(bank_report);
            let banks: Vec<&asset_t6::SndBankRef> = captures
                .iter()
                .copied()
                .chain(lang_caps.iter())
                .flat_map(|c| c.sound_banks.iter())
                .collect();
            let globals = captures.iter().find_map(|c| c.snd_globals.as_ref());
            let (mut catalog, census) = asset_audio::build_t6_sound_catalog(
                &banks,
                globals,
                &index,
                asset_core::ZoneOwner::intern(&map),
            );
            report.push(format!(
                "t6 sound: {} aliases, {} variants ({} with a clip, {} without); {} falloff curves",
                census.aliases,
                census.variants,
                census.with_clip,
                census.without_clip,
                globals.map_or(0, |g| g.curves.len())
            ));
            for s in &catalog.sounds {
                let ms = s
                    .aliases
                    .iter()
                    .filter_map(|a| a.streamed.as_ref())
                    .filter_map(|(_, id)| u32::from_str_radix(id, 16).ok())
                    .filter_map(|id| index.get(id))
                    .map(|(_, e)| (u64::from(e.frame_count) * 1000 / u64::from(e.rate().max(1))) as u32)
                    .max();
                if let Some(ms) = ms {
                    sound_lengths.push((s.name.to_ascii_lowercase(), ms));
                }
            }
            catalog.scripted_map_fx = Some(map_fx);
            let mut echoes: Vec<String> = catalog
                .radverbs
                .iter()
                .map(|(name, v)| format!("{name} {v:?}"))
                .collect();
            echoes.sort();
            report.push(format!("t6 sound: {} room echoes: {}", echoes.len(), echoes.join("; ")));
            Ok(catalog)
        }
        _ => Err("Black Ops II sound folder not found beside the zones".to_owned()),
    };
    let models_missing: Vec<String> = fpv_missing
        .iter()
        .chain(world_missing.iter())
        .cloned()
        .collect();
    parts.mark("sound");
    // The guns checked: Nuketown Zombies' list, or (bo2mp) every
    // multiplayer gun the zones carry.
    let census_names: Vec<String> = if is_mp_map(&map) {
        let mut v: Vec<String> = weapon_order
            .iter()
            .filter(|n| n.ends_with("_mp"))
            .cloned()
            .collect();
        v.sort();
        v
    } else {
        NUKETOWN_WEAPONS.iter().map(|n| (*n).to_owned()).collect()
    };
    // (Where this part's time goes, for the cold first map's line.)
    let census_started = std::time::Instant::now();
    let census_lines = census(
        &captures,
        &census_names,
        &weapon_refs,
        &models_missing,
        &xanims,
        &fx,
        impact_fx.as_ref(),
        sound
            .as_ref()
            .ok()
            .and_then(|c| c.scripted_map_fx.as_ref())
            .unwrap_or(&asset_audio::ScriptedMapFx::default()),
        room_tone.as_deref(),
        sound.as_ref().ok(),
    );
    for line in &census_lines {
        diag::info!(World, "{line}");
    }
    report.extend(census_lines);
    let footsteps = captures
        .iter()
        .rev()
        .find(|c| !c.footstep_tables.is_empty())
        .map(|c| c.footstep_tables.clone())
        .unwrap_or_default();
    let mut scripts = crate::T6ScriptSet::default();
    let (mut playeranim, mut playeranim_types) = (None, None);
    for c in &captures {
        for (name, bytes) in &c.raw_files {
            if let Some(n) = name.strip_prefix("script:")
                && !n.starts_with(',')
                && n.ends_with(".gsc")
            {
                scripts.objects.push(bytes.clone());
            }
        }
        for t in &c.string_tables {
            if real(&t.name) {
                scripts
                    .tables
                    .push((t.name.clone(), t.columns, t.rows, t.cells.clone()));
            }
        }
        for v in &c.vehicles {
            scripts.vehicles.retain(|n| n.name != v.name);
            scripts.vehicles.push(v.clone());
        }
        scripts.entities.extend(
            c.map_ents
                .iter()
                .map(|t| destructible_entities(t, &destructibles)),
        );
        // codmp: the clipmap's loose and breakable props, as script models
        // that break by a definition of their own.
        let (text, defs) = dyn_ent_props(c, &captures);
        if !defs.is_empty() {
            report.push(format!("codmp t6 dynents: {} props as breakables", defs.len()));
            scripts.entities.push(text);
            for d in defs {
                scripts.destructibles.retain(|x| x.name != d.name);
                scripts.destructibles.push(d);
            }
        }
        // bo2mp: every destructible the map's entities can name.
        for d in destructible_defs(&[*c]) {
            scripts.destructibles.retain(|x| x.name != d.name);
            scripts.destructibles.push(d);
        }
        for z in &c.zbarriers {
            scripts.zbarriers.retain(|o| !o.name.eq_ignore_ascii_case(&z.name));
            scripts.zbarriers.push(z.clone());
        }
        if !c.path_nodes.is_empty() {
            scripts.path_nodes = c.path_nodes.clone();
            scripts.path_tables = c.path_tables.clone();
        }
        for (name, bytes) in &c.raw_files {
            let lower = name.to_ascii_lowercase();
            if let Some(shock) = lower.strip_prefix("shock/").and_then(|n| n.strip_suffix(".shock")) {
                scripts.shocks.retain(|(n, _)| n != shock);
                scripts
                    .shocks
                    .push((shock.to_owned(), String::from_utf8_lossy(bytes).into_owned()));
            }
            if name.starts_with("animstatedefs/") && name.ends_with(".asd") {
                let text = String::from_utf8_lossy(bytes).into_owned();
                scripts.animstatedefs.retain(|(n, _)| n != name);
                scripts.animstatedefs.push((name.clone(), text));
            }
            // bo2mp: the shellshocks (`shock/flashbang.shock`, ...).
            let lower = name.replace('\\', "/").to_ascii_lowercase();
            if let Some(shock) = lower
                .strip_prefix("shock/")
                .and_then(|n| n.strip_suffix(".shock"))
            {
                let text = String::from_utf8_lossy(bytes).into_owned();
                scripts.shocks.retain(|(n, _)| n != shock);
                scripts.shocks.push((shock.to_owned(), text));
            }
            if name.contains("/gamesettings_") && name.ends_with(".cfg") {
                let text = String::from_utf8_lossy(bytes).into_owned();
                scripts.gamesettings.retain(|(n, _)| n != name);
                scripts.gamesettings.push((name.clone(), text));
            }
            // bo2mp: how a dead body goes limp.
            if lower == "ragdoll.cfg" {
                scripts.ragdoll = Some(String::from_utf8_lossy(bytes).into_owned());
            }
            // bo2mp: how players look to others (third-person animations).
            if name == "mp/playeranim.script" {
                playeranim = Some(String::from_utf8_lossy(bytes).into_owned());
            }
            if name == "mp/playeranimtypes.txt" {
                playeranim_types = Some(String::from_utf8_lossy(bytes).into_owned());
            }
        }
    }
    scripts.playeranim = playeranim.zip(playeranim_types);
    // Every animation's timing, root motion and notetracks; later zones win.
    let mut anim_facts: HashMap<String, crate::T6AnimFacts> = HashMap::new();
    for c in &captures {
        for a in c.xanims.iter().filter(|a| real(&a.name)) {
            anim_facts.insert(
                a.name.clone(),
                crate::T6AnimFacts {
                    name: a.name.clone(),
                    numframes: a.numframes,
                    framerate: a.framerate,
                    looping: a.looping,
                    delta_trans: a.delta_trans.clone(),
                    delta_quat: a.delta_quat.clone(),
                    notifies: a.notifies.clone(),
                },
            );
        }
    }
    scripts.anims = anim_facts.into_values().collect();
    let mut strings: HashMap<String, String> = HashMap::new();
    for c in &lang_caps {
        for (k, v) in &c.localize {
            strings.insert(k.clone(), v.clone());
        }
    }
    // bo2mp: the map's name as players know it (`mp/mapstable.csv` column
    // 3: `MPUI_LA` = "AFTERMATH").
    if is_mp_map(&map) {
        let key = captures
            .iter()
            .rev()
            .flat_map(|c| c.string_tables.iter())
            .find(|t| t.name.eq_ignore_ascii_case("mp/mapstable.csv"))
            .and_then(|t| {
                t.cells
                    .chunks(t.columns.max(1))
                    .find(|r| r.first().is_some_and(|c| c.eq_ignore_ascii_case(&map)))
                    .and_then(|r| r.get(3).cloned())
            });
        if let Some(key) = key {
            let name = strings
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&key))
                .map_or("-", |(_, v)| v.as_str());
            report.push(format!("t6 mp map name: {key} = {name}"));
        }
    }
    // Strings a mod zone carries (a map pack's mod_load), where
    // no language zone has the key.
    for c in &captures {
        for (k, v) in &c.localize {
            strings.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    scripts.strings = strings.into_iter().collect();
    scripts.sound_lengths = sound_lengths;
    // bo2mp: weapons a player can pick back up once placed or thrown (their
    // weapon file's bRetrievable: the hatchet, claymore, Bouncing Betty...),
    // as the engine's getretrievableweapons lists them.
    let mut retrievable: Vec<String> = weapon_refs
        .iter()
        .filter(|(_, w)| w.def_u8(fastfile_t6::layout::WeaponDef::bRetrievable) != 0)
        .map(|(n, _)| n.to_ascii_lowercase())
        .collect();
    retrievable.sort();
    report.push(format!("t6 retrievable weapons: {}", retrievable.join(", ")));
    // bo2mp: each weapon's own melee damage (iMeleeDamage), to check the
    // knife against BO2 (a melee hit kills in one).
    let mut melee: Vec<String> = weapon_refs
        .iter()
        .filter(|(n, _)| ["knife_mp", "knife_held_mp", "mp7_mp", "an94_mp", "dsr50_mp", "fiveseven_mp", "riotshield_mp", "knife_ballistic_mp"].contains(&n.as_str()))
        .map(|(n, w)| format!("{n}={}", w.def_i32(fastfile_t6::layout::WeaponDef::iMeleeDamage)))
        .collect();
    melee.sort();
    report.push(format!("t6 melee damage: {}", melee.join(" ")));
    scripts.retrievable_weapons = retrievable;
    // Every sound alias the banks hold (the scripts' soundexists).
    if let Ok(cat) = &sound {
        scripts.sound_aliases = cat
            .sounds
            .iter()
            .map(|s| s.name.to_ascii_lowercase())
            .collect();
    }
    report.push(format!(
        "t6 scripts: {} server script objects, {} string tables, {} entity strings, {} path nodes, {} animations, {} animstatedefs, {} English strings, {} barrier types",
        scripts.objects.len(),
        scripts.tables.len(),
        scripts.entities.len(),
        scripts.path_nodes.len(),
        scripts.anims.len(),
        scripts.animstatedefs.len(),
        scripts.strings.len(),
        scripts.zbarriers.len()
    ));
    let ui_script_list = ui_scripts(&captures);
    let mut ui_names = ui_strings(&ui_script_list);
    // bo2mp lane C: a multiplayer map's menus (the class menu) also draw
    // pictures named by table cells (weapons, perks).
    if map.starts_with("mp_") {
        ui_names.extend(super::t6_frontend::table_picture_names(&captures));
        // The minimap the engine draws: the map's picture, the pings
        // (him, his team), its overlay; the scripts' hit marker, the
        // outcome screen's team icons; his damage feedback (blood, the
        // low-health edge, the hit's direction).
        for c in &captures {
            for m in &c.materials {
                let n = m.name.as_str();
                if n.starts_with("compass_map_")
                    || n.starts_with("compassping_")
                    || n == "mp_minimap_overlay"
                    || n == "compass_radarline"
                    || n.starts_with("damage_feedback")
                    || n.starts_with("faction_")
                    || n.starts_with("overlay_low_health")
                    || n == "hit_direction"
                    || (n.starts_with("map_") && n.contains("selector"))
                    // The scorestreak earned popup's big icon (the item's
                    // picture and "_big", notificationpopups.lua) and the
                    // column's small ones (rewardselection.lua).
                    || n.starts_with("hud_ks_")
                    // The equipment pips beside his ammo (offhandicons.lua).
                    || matches!(
                        n,
                        "hud_us_grenade"
                            | "hud_grenadeicon"
                            // The grenade warning around his crosshair
                            // (cgMedia: the pointer, the throw-back hand).
                            | "hud_grenadepointer"
                            | "hud_grenadethrowback"
                            | "hud_indicator_arrow"
                            | "hud_icon_sticky_grenade"
                            | "hud_hatchet"
                            | "hud_icon_claymore"
                            | "hud_icon_satchelcharge"
                            | "hud_bounce_betty"
                            | "hud_us_flashgrenade"
                            | "hud_us_stungrenade"
                            | "hud_willy_pete"
                            | "hud_empgrenade"
                            | "hud_trophy_system"
                            | "hud_tact_insert"
                    )
                {
                    ui_names.insert(n.to_owned());
                }
            }
        }
    }
    let icons_started = std::time::Instant::now();
    let mut hud_icons = hud_icons(&captures, packs, &ui_names, &mut report, IconShare::Take);
    let icons_ms = icons_started.elapsed().as_secs_f32() * 1000.0;
    let before_icons_ms = icons_started.duration_since(census_started).as_secs_f32() * 1000.0;
    let icon_count = hud_icons.0.len();
    hud_icons.0.extend(sight_reticles);
    let rest_started = std::time::Instant::now();
    let screen_filters = screen_filters(&captures, packs, &mut report);
    report.push(format!("t6 ui: {} scripts", ui_script_list.len()));
    let ui = crate::T6Ui {
        scripts: ui_script_list,
        strings: std::sync::Arc::new(scripts.strings.clone()),
        tables: std::sync::Arc::new(scripts.tables.clone()),
        configs: std::sync::Arc::new(ui_configs(&captures)),
        game_settings: std::sync::Arc::new(zm_game_settings(&captures)),
    };
    // The font sheet's pixels are in the language's own image packs.
    let lang_packs = Some(zone_dir.as_path())
        .and_then(Path::parent)
        .map(|d| d.join("english"))
        .and_then(|d| asset_t6::PackSet::open_dir(&d).ok());
    let hud_fonts = hud_fonts(&lang_caps, lang_packs.as_ref().or(packs), &mut report);
    // bo2zm: the bullet penetration table (patch_zm's `info/bullet_penetration_mp`).
    let pen_table = captures
        .iter()
        .flat_map(|c| c.raw_files.iter())
        .filter_map(|(name, bytes)| asset_game::capture_pen_table(name, bytes, false))
        .last();
    report.push(format!(
        "t6 bullet penetration table: {}",
        if pen_table.is_some() {
            "loaded"
        } else {
            "missing (bullets stop at the first hit)"
        }
    ));
    diag::info!(
        World,
        "t6 combat part: census, strings and fonts: census and scripts {before_icons_ms:.1}ms, hud icons {icons_ms:.1}ms ({icon_count} pictures), filters, fonts and tables {:.1}ms",
        rest_started.elapsed().as_secs_f32() * 1000.0
    );
    parts.mark("census, strings and fonts");
    T6Combat {
        weapons,
        fpv,
        world_weapons,
        xanims,
        fx,
        fx_models,
        tracers,
        impact_fx,
        sound,
        footsteps,
        room_tone,
        scripts,
        hud_icons,
        hud_fonts,
        ui,
        pen_table,
        visions: script_visions(&captures),
        screen_filters,
        camos,
        report,
    }
}

fn script_visions(captures: &[&ZoneCapture]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, bytes) in captures.iter().flat_map(|c| c.raw_files.iter()) {
        let name = name.to_ascii_lowercase();
        if !(name.starts_with("vision/") && name.ends_with(".vision")) {
            continue;
        }
        out.retain(|(n, _)| *n != name);
        out.push((name, bytes.clone()));
    }
    out
}

/// bo2zm: a map's brush models that carry surfaces of their own (Zombies
/// Nacht der Untoten: the first room's door `*11` is a
/// `script_brushmodel` drawn from the world's surfaces, where BO2's own maps
/// use a script model), each as a one-bone model named `*N` so the entity
/// draws where the scripts move it. The world vertex's first 32 bytes are
/// the model vertex (`GfxPackedVertex`); the surfaces are authored about
/// the entity's origin, as the submodel's brushes are.
fn brush_model_refs(world: &asset_t6::WorldRef) -> Vec<asset_t6::XModelRef> {
    const T6_WORLD_VERTEX: usize = 36;
    let mut out = Vec::new();
    for (n, &(first, count, bounds)) in world.brush_models.iter().enumerate().skip(1) {
        let mut lod0 = Vec::new();
        for surf in world
            .surfaces
            .iter()
            .skip(first as usize)
            .take(count as usize)
        {
            let start = surf.base_index.max(0) as usize;
            let Some(tris) = world
                .indices
                .get(start..start + usize::from(surf.tri_count) * 3)
            else {
                continue;
            };
            let Some(&top) = tris.iter().max() else {
                continue;
            };
            let verts = usize::from(top) + 1;
            let at = surf.vertex_data_offset0.max(0) as usize;
            let Some(block) = world.vd0.get(at..at + verts * T6_WORLD_VERTEX) else {
                continue;
            };
            lod0.push(asset_t6::XSurfaceRef {
                // The world's vertex colour (per surface: magenta, green
                // on Nacht's door) tints a model: draw it white.
                verts: block
                    .chunks_exact(T6_WORLD_VERTEX)
                    .flat_map(|v| {
                        let mut m = [0u8; 32];
                        m.copy_from_slice(&v[..32]);
                        m[16..20].fill(0xff);
                        m
                    })
                    .collect(),
                vert_count: verts as u16,
                indices: tris.to_vec(),
                vert_lists: vec![(0, verts as u16, 0, surf.tri_count)],
                material: surf.material,
                ..Default::default()
            });
        }
        if lod0.is_empty() {
            continue;
        }
        out.push(asset_t6::XModelRef {
            phys_preset: None,
            name: format!("*{n}"),
            numsurfs: lod0.len() as u8,
            materials: lod0.iter().map(|s| s.material).collect(),
            num_bones: 1,
            num_lods: 1,
            mins: bounds[0],
            maxs: bounds[1],
            radius: 0.0,
            lighting_origin_offset: [0.0; 3],
            lighting_origin_range: 0.0,
            lod0,
            bone_names: vec!["tag_origin".to_owned()],
            num_root_bones: 1,
            parent_list: Vec::new(),
            quats: Vec::new(),
            trans: Vec::new(),
            base_mat: vec![([0.0, 0.0, 0.0, 1.0], [0.0; 3])],
            part_classification: vec![0],
            bone_info: vec![None],
            lods: vec![(0.0, 0, 0)],
            contents: 0,
            coll_lod: -1,
            coll_surfs: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod camo_plan_tests {
    /// Black Ops II's material camos from its zones, when they are here
    /// (`CODLEGACY_TEST_T6_ZONES`: the folder holding common_mp.ff and patch_mp.ff).
    #[test]
    fn material_camos_from_bo2_zones() {
        let Ok(dir) = codlegacy_env::var("CODLEGACY_TEST_T6_ZONES") else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        let caps: Vec<asset_t6::ZoneCapture> = ["common_mp", "patch_mp"]
            .iter()
            .map(|z| asset_t6::capture_zone(&dir.join(format!("{z}.ff"))).unwrap())
            .collect();
        let refs: Vec<&asset_t6::ZoneCapture> = caps.iter().collect();
        let plan = super::mp_material_camos(&refs);
        // Gold (15), carbon fibre (10), diamond (16) and the DLC material camos.
        for option in [10, 15, 16] {
            assert!(plan.get(&option).is_some_and(|l| !l.is_empty()), "camo {option} missing: {:?}", plan.keys().collect::<Vec<_>>());
        }
        let gold = &plan[&15];
        assert!(gold.iter().any(|(b, c, t)| b.contains("mp7") && c == "mc/mtl_weapon_camo_gold" && *t == 10.0), "{gold:?}");
        println!("{} material camos: {:?}", plan.len(), plan.iter().map(|(o, l)| (o, l.len())).collect::<Vec<_>>());
        // The pattern camos: DEVGRU (1) on the MP7 with its own repeat.
        let patterns = super::mp_pattern_camos(&refs);
        let devgru = patterns.get(&1).expect("devgru");
        assert!(devgru.iter().any(|(b, img, scale)| b.contains("mp7") && img == "t6_camo_devgru_pattern" && (*scale - 1.77).abs() < 0.01), "{devgru:?}");
        println!("{} pattern camos", patterns.len());
    }
}

/// bo2mp memory: what each part of the combat load added (the stage's own
/// line gives only the sum, a gigabyte on every multiplayer map).
struct CombatParts {
    at: std::time::Instant,
    rss: Option<u64>,
    heap: Option<u64>,
}

impl CombatParts {
    fn start() -> Self {
        Self {
            at: std::time::Instant::now(),
            rss: asset_transport::process_resident_bytes(),
            heap: diag::process_live_heap_bytes(),
        }
    }

    fn mark(&mut self, part: &str) {
        let next = Self::start();
        let mib = |name: &str, was: Option<u64>, now: Option<u64>| match (was, now) {
            (Some(was), Some(now)) => {
                format!(" {name}={:+}MiB→{}MiB", (now as i64 - was as i64) >> 20, now >> 20)
            }
            _ => String::new(),
        };
        diag::info!(
            World,
            "t6 combat part: {part} {:.1}ms{}{}",
            next.at.duration_since(self.at).as_secs_f32() * 1000.0,
            mib("rss", self.rss, next.rss),
            mib("heap", self.heap, next.heap)
        );
        *self = next;
    }
}

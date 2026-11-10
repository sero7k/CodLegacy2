//! Starting the map's script: the script set, the host's entities, the engine's level fields,
//! then `maps/mp/<map>::main`. World-independent (the sim glue and the tests both use it).

use gsc_iw6_vm::{file_name, ArrRef, Key, Name, ScriptSet, Value, Vm};

use super::host::{struct_from, Iw6Host, Table};

/// What the map load handed over (`asset_world::Iw6ScriptPack`, by reference).
pub struct BootData<'a> {
    pub map: &'a str,
    pub scripts: &'a [(String, Vec<u8>, Vec<u8>)],
    pub tables: &'a [(String, usize, usize, Vec<String>)],
    pub entities: &'a [Vec<(u32, String)>],
}

/// Facts about the boot, for the log and the tests.
#[derive(Debug, Default)]
pub struct BootReport {
    pub scripts: usize,
    pub script_errors: Vec<String>,
    pub entities: usize,
    pub structs: usize,
    pub unresolved_includes: usize,
    pub main_found: bool,
}

/// `level` fields the engine fills before the map script runs (INFERRED from what the scripts
/// read without checking: the map name, the console flag (this is the PC build), the bots'
/// bookkeeping).
const F_LEVEL_SCRIPT: u16 = 502;
const F_LEVEL_CONSOLE: u16 = 7879;
const F_LEVEL_BOTS: u16 = 5594;
/// `level.gametype`, which the gametype script sets before any map script runs (the engine loads the
/// gametype first); mp_snow's main reads it at once.
const F_LEVEL_GAMETYPE: u16 = 14086;
/// `level.rankedmatch`, which the game's init sets from the `onlinegame`, `xblive_privatematch` and
/// `force_ranking` dvars (false here: this is not an online game); Favela's main reads it at once.
const F_LEVEL_RANKED: u16 = 25418;
/// `level.gameended`, which the game's init sets to zero (the teleport and nuke scripts read it).
const F_LEVEL_GAME_ENDED: u16 = 21806;
/// `level.<field>`, the table of the level-event flags (`maps/mp/_utility`'s getter reads it); the game's
/// init makes `post_game_level_event_active` false before a map's event looks at it.
const F_LEVEL_EVENT_FLAGS: u16 = 19894;
/// A level flag the game-type init sets to 0 and the map scripts read.
const F_LEVEL_FLAG_14062: u16 = 14062;
/// The chopper boss's path starts; the teleport code walks them and the boss script is not run.
const F_LEVEL_CHOPPER_PATHS: [u16; 7] = [2658, 2651, 23303, 34657, 17442, 34099, 25810];
/// `level.onstartgame`: the game's start hook (functions chained by the map's scripts).
const F_LEVEL_ON_START_GAME: u16 = 22905;
/// The teleport system's zones (by name) and the name of the zone the players are in.
const F_LEVEL_TELEPORT_ZONES: u16 = 32797;
const F_LEVEL_TELEPORT_ZONE: u16 = 32796;
/// The map's level-killstreak setup function (`level.<field>`), set in a map's main.
const F_LEVEL_KILLSTREAK_SETUP: u16 = 20636;
/// The shared function the gametype scripts call first to set the console flags (`level.ps3`,
/// `level.xb3`, `level.ps4`) from the dvars (a function of `maps/mp/_utility`, file token 1385).
const FN_CONSOLE_FLAGS: u16 = 28260;
/// `maps/mp/_utility`'s game flags (file token 1385, function tokens): `flag_init(name, value)`,
/// `flag_set(name)` (sets and notifies on level). The gametype's init makes `prematch_done`.
pub const FN_FLAG_INIT: u16 = 14067;
pub const FN_FLAG_SET: u16 = 14068;
/// The flags the gametype script makes before any map script runs, with their first value.
pub const GAMETYPE_FLAGS: [&str; 1] = ["prematch_done"];

/// A function of `maps/mp/_utility` by its token.
pub fn utility_fn(vm: &Vm<Iw6Host>, token: u16) -> Option<gsc_iw6_vm::FuncRef> {
    vm.resolve(&Name::Id(1385), &Name::Id(token))
}

/// `level.players`.
pub const F_LEVEL_PLAYERS: u16 = 6922;
/// `level.participants`: the players and the agents.
pub const F_LEVEL_PARTICIPANTS: u16 = 24528;

pub fn boot(data: &BootData<'_>) -> Result<(Vm<Iw6Host>, BootReport), String> {
    let mut rep = BootReport::default();
    let mut set = ScriptSet::new();
    for (name, stream, code) in data.scripts {
        if let Err(e) = set.add_bytes(file_name(name), stream, code) {
            rep.script_errors.push(format!("{name}: {e}"));
        }
    }
    rep.scripts = set.len();
    rep.unresolved_includes = set.unresolved_includes().len();
    let mut host = Iw6Host::default();
    for (name, columns, rows, cells) in data.tables {
        host.tables.insert(name.to_ascii_lowercase(), Table { columns: *columns, rows: *rows, cells: cells.clone() });
    }
    host.set_dvar("mapname", data.map);
    host.set_dvar("g_gametype", "war");
    host.set_dvar("ui_gametype", "war");
    host.set_dvar("createfx", "");
    // The client plays the map's createfx effects (the exploders are activated by number).
    host.set_dvar("clientSideEffects", "1");
    host.set_dvar("r_reflectionprobegenerate", "0");
    host.set_dvar("scr_requiredmapaspectratio", "1");
    let structs = host.load_blocks(data.entities);
    rep.entities = host.ents.len();
    rep.structs = structs.len();
    let mut vm = Vm::new(set, host);
    // The structs the engine makes from the map's `script_struct` entities.
    let list = ArrRef::new();
    for (_, fields) in &structs {
        list.push(Value::Object(struct_from(fields)));
    }
    vm.level_obj().set(Name::Id(super::host::F_STRUCT_LIST), Value::Array(list));
    vm.level_obj().set(Name::Id(F_LEVEL_BOTS), Value::Array(ArrRef::new()));
    vm.level_obj().set(Name::Id(F_LEVEL_PLAYERS), Value::Array(ArrRef::new()));
    vm.level_obj().set(Name::Id(F_LEVEL_PARTICIPANTS), Value::Array(ArrRef::new()));
    vm.level_obj().set(Name::Id(F_LEVEL_CONSOLE), Value::Int(0));
    vm.level_obj().set(Name::Id(F_LEVEL_SCRIPT), Value::str(data.map));
    vm.level_obj().set(Name::Id(F_LEVEL_GAMETYPE), Value::str("war"));
    vm.level_obj().set(Name::Id(F_LEVEL_RANKED), Value::Int(0));
    vm.level_obj().set(Name::Id(F_LEVEL_FLAG_14062), Value::Int(0));
    for f in F_LEVEL_CHOPPER_PATHS {
        vm.level_obj().set(Name::Id(f), Value::Array(ArrRef::new()));
    }
    vm.level_obj().set(Name::Id(F_LEVEL_GAME_ENDED), Value::Int(0));
    {
        let flags = ArrRef::new();
        flags.set(Key::Str("post_game_level_event_active".into()), Value::Int(0));
        vm.level_obj().set(Name::Id(F_LEVEL_EVENT_FLAGS), Value::Array(flags));
    }
    // The engine runs the gametype's main before the map's: that is where the console flags are set.
    if let Some(flags) = vm.resolve(&Name::Id(1385), &Name::Id(FN_CONSOLE_FLAGS)) {
        vm.start_thread(flags, Vec::new());
    }
    if let Some(init) = utility_fn(&vm, FN_FLAG_INIT) {
        for f in GAMETYPE_FLAGS {
            vm.start_thread(init, vec![Value::str(f), Value::Int(0)]);
        }
    }
    // The game's killstreak init runs the nuke's own init (`maps/mp/killstreaks/_nuke`, file token
    // 1553): its timer, state and effects, which a map's nuke hook or event reads.
    if let Some(init) = vm.resolve(&Name::Id(1553), &Name::Id(17631)) {
        vm.start_thread(init, Vec::new());
    }
    // The game-start hook begins as a function that does nothing (the game type's own is not run);
    // the teleport system keeps it and wraps it.
    if let Some(noop) = vm.resolve(&Name::Id(927), &Name::Id(12842)) {
        vm.level_obj().set(Name::Id(F_LEVEL_ON_START_GAME), Value::Function(noop));
    }
    let file = Name::Text(format!("maps/mp/{}", data.map));
    let main = vm.resolve(&file, &Name::Id(20445));
    rep.main_found = main.is_some();
    if let Some(main) = main {
        vm.start_thread(main, Vec::new());
    }
    // The match's start then calls the game-start hook (`level.<field 22905>`; the teleport system
    // wraps it with its own, which finds the map's teleport origins and zones).
    if let Some(Value::Function(start)) = vm.level_obj().get(&Name::Id(F_LEVEL_ON_START_GAME)) {
        vm.start_thread(start, Vec::new());
    }
    // A map with teleport zones begins in its "start" zone (the teleport system names the zone
    // players are in; the first move away from it reads the name).
    if vm.level_obj().get(&Name::Id(F_LEVEL_TELEPORT_ZONES)).is_some() && vm.level_obj().get(&Name::Id(F_LEVEL_TELEPORT_ZONE)).is_none() {
        vm.level_obj().set(Name::Id(F_LEVEL_TELEPORT_ZONE), Value::str("start"));
    }
    // The game's killstreak init then runs the map's level-killstreak setup (`level.<field 20636>`,
    // a function a map such as Strikezone sets in its main: it decides whether the map's own nuke
    // event can run, `level.nukeoverride` reads that).
    if let Some(Value::Function(setup)) = vm.level_obj().get(&Name::Id(F_LEVEL_KILLSTREAK_SETUP)) {
        vm.start_thread(setup, Vec::new());
    }
    Ok((vm, rep))
}

//! bo2mp shell: the client's own front end.
//!
//! Every front-end screen is drawn here - the main menu, Multiplayer with
//! its online panel, the Custom Game lobby (LOBBY / LOADOUTS / SETTINGS),
//! the map and mode pickers - over a full-screen map picture that fades
//! into the dark on the side the menus sit. BO2's Global (the globe and its
//! activity screens) is gone: the online panel says how many players are on
//! the master server instead. The loading screen (`loading`) and the in-game
//! HUD (`hud_modern`) share the look (`theme`).
//!
//! BO2's scripts still run underneath (`bridge`): the shell reads its maps,
//! modes, stats and classes from them and starts the match through them.
//!
//! ZOMBIES opens the same screens in red (`zombies`, `zombies_view`): a
//! map, a start and mode, then START runs the Zombies rebuild
//! (DaffyDabz/bo2-zombies-rebuild, built beside this game) and comes back
//! here when it closes.
//!
//! WARZONE opens Multiplayer's screens again for Resurgence on Rebirth
//! Island in its own grey and orange (`warzone`, `wz_view`, `wz_theme`).
//!
//! `BO2_SHELL=off` brings BO2's own menus back.

mod anim; // bo2mp: the front end's slides, focus bands and pulses
mod background;
mod ghosts; // bo2mp: the Ghosts-style parts (rows, cards, lists, stats page)
pub(crate) mod bridge;
mod cac;
mod coming; // bo2mp: Cold War and Vanguard: banners and placeholders in Create-a-Class
pub(crate) mod cards;
pub(crate) mod classes;
pub(crate) mod ingame;
pub(crate) mod zm_record;
pub(crate) mod zm_board;
mod loading;
mod friends;
mod lobbies;
pub(crate) mod lobby_rules; // bo2mp: Custom Game bots: none to start, no friendly without teams, 18 players at most
mod matchmaking;
mod pregame;
mod prepare; // bo2mp: PREPARE ALL MAPS' bar on the menus (Esc stops it)
mod room;
mod looks;
mod online;
mod p2p; // bo2mp: custom games with friends over the internet, no server
pub(crate) mod options;
mod opt_view; // bo2mp: the front end's OPTIONS page and its sub-pages, as Ghosts'
mod keys; // bo2mp: OPTIONS > MOUSE & KEYBOARD > KEY BINDINGS
mod pad_diagram; // bo2mp: OPTIONS > CONTROLLER's pad
pub(crate) mod pics; // (hud_match reuses the picture resolver)
#[doc(hidden)]
pub mod preview;
pub(crate) mod theme; // (hud_match reuses the colours and fonts)
mod view;
mod warzone;
pub(crate) mod wz_theme; // bo2mp: WARZONE's colours and words (the Warzone HUD reads them too)
mod wz_view;
pub(crate) mod zombies;
mod zombies_view;

use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frame::ClientSet;

pub(crate) use bridge::{covers_loading, covers_lui, enabled, lui_side};

use bridge::{Catalog, Request, StartSpec};

/// The screen he is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Screen {
    #[default]
    Main,
    Multiplayer,
    Lobby,
    MapPick,
    ModePick,
    /// Create-a-Class: his classes, one class's slots, a slot's items.
    Classes,
    ClassEdit,
    ItemPick,
    /// Options (its tab is `page`), Barracks.
    Options,
    Barracks,
    /// Barracks' calling card and emblem pickers (a page of them, `page`).
    CardPick,
    EmblemPick,
    /// His friend code and his friends (`friends`).
    Friends,
    /// The server's lobbies (`lobbies`).
    Lobbies,
    /// FIND A MATCH: the playlist filter (`matchmaking`; its tab is `page`).
    MatchFilter,
    /// The room he joined, until its host starts (`room`).
    Room,
    /// Zombies: its lobby (map, start, starting round, START), its map
    /// picker (a page per game, `page`) and the map's starts and modes.
    Zombies,
    ZmMapPick,
    ZmStartPick,
    /// Zombies' game lobby (Solo, Custom or a public game: `zm_kind`) and
    /// its combat record (rank and stats).
    ZmLobby,
    ZmRecord,
    /// WARZONE: Multiplayer's screens again for Resurgence (`warzone`,
    /// `wz_view`, `wz_theme`): its menu, the private match's squad lobby
    /// (tabs: `tab`) and the playlist picker.
    WzMenu,
    WzLobby,
    WzModePick,
}

impl Screen {
    /// The red Zombies screens.
    pub(crate) fn is_zombies(self) -> bool {
        matches!(self, Screen::Zombies | Screen::ZmMapPick | Screen::ZmStartPick | Screen::ZmLobby | Screen::ZmRecord)
    }

    /// WARZONE's own screens.
    pub(crate) fn is_warzone(self) -> bool {
        matches!(self, Screen::WzMenu | Screen::WzLobby | Screen::WzModePick)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Tab {
    #[default]
    Lobby,
    Loadouts,
    Settings,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Lobby, Tab::Loadouts, Tab::Settings];

    fn label(self) -> &'static str {
        match self {
            Tab::Lobby => "LOBBY",
            Tab::Loadouts => "LOADOUTS",
            Tab::Settings => "SETTINGS",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Setting {
    Mode,
    Friends,
    Enemies,
    Difficulty,
    /// The optional M.O.A.B. (off, or its kill count).
    Moab,
    /// Custom Games: his trickshot script (`bo2mp/trickshot`).
    Trickshot,
    LethalCount,
    TacticalCount,
    /// Zombies' starting round, and its bots (Custom Games).
    ZmRound,
    ZmBots,
    /// Custom Games: Multiplayer's custom guns in the Mystery Box.
    ZmCustomWeapons,
    /// An Options row (of the tab he is on).
    Option(u8),
    /// WARZONE: the playlist, the squad size and the number of squads.
    BrMode,
    BrSquadSize,
    BrSquads,
}

/// What a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Act {
    Multiplayer,
    Zombies,
    Options,
    Friends,
    Barracks,
    Prestige,
    Quit,
    CustomGame,
    Start,
    PickMap,
    PickMode,
    Cycle(Setting),
    /// A map tile / mode row (its index in the list shown).
    Choose(usize),
    /// Create-a-Class: his classes, a class, a slot of it (`cac::EDIT_SLOTS`).
    Classes,
    EditClass(i32),
    EditSlot(usize),
    /// Zombies: start the game, pick the map, pick the start and mode.
    ZmPlay,
    ZmPickMap,
    ZmPickStart,
    /// Zombies' menu: a lobby of a kind (Solo, Custom, public), the record.
    ZmLobby(zombies::ZmKind),
    ZmRecord,
    /// Options > ACCOUNT: type a new name.
    TypeName,
    /// Options > GAME FOLDERS: pick a game's folder (its index in
    /// `game_paths::GAMES`).
    PickFolder(usize),
    /// Options > GAME FOLDERS: SCAN FOR GAMES (the game scan again).
    ScanGames,
    /// Options > GAME FOLDERS: SCAN A FOLDER (every game in the folder or
    /// drive he picks).
    ScanFolder,
    /// Options > GAME FOLDERS: CLEAR CACHE (every cache, after a second press).
    ClearCache,
    /// Options > GAME FOLDERS: BAKE ALL (every map's fast-load copy now; a
    /// press while it runs cancels it).
    BakeAll,
    /// Barracks: open the calling card or emblem picker.
    Looks(cards::Kind),
    /// A row that only shows something (Options > ACCOUNT's friend code).
    Info,
    /// Options: open a page of its own (VIDEO OPTIONS > ADVANCED VIDEO:
    /// `options::ADVANCED_PAGE`).
    SubPage(usize),
    /// Options > KEY BINDINGS: press a new key for an action (its index in
    /// `keys::ACTIONS`); Black Ops II's keys back.
    Rebind(usize),
    ResetKeys,
    /// FRIENDS: type a code to add; a friend (by code).
    AddFriend,
    Friend(u32),
    /// Multiplayer: FIND A LOBBY; a lobby on the server.
    Lobbies,
    Lobby(net::master_protocol::AdvertId),
    /// FIND A MATCH: open the filter; tick a mode (by catalog index); search;
    /// host one when the search finds none.
    FindMatch,
    FilterMode(usize),
    /// FIND A MATCH: the BRAINROT 24/7 playlist (Shipment only), on or off.
    Brainrot,
    SearchMatches,
    HostMatch,
    /// The matchmaking lobby: vote for one of its two maps; the lobby's
    /// players by rank.
    Vote(usize),
    Leaderboard,
    /// The main menu's modes that are not built yet: a notice, nothing else.
    Soon(Soon),
    /// WARZONE: its menu, and DEPLOY.
    Warzone,
    BrStart,
    /// WARZONE: QUICK PLAY (a playlist, then straight in), PRIVATE MATCH
    /// (the squad lobby), and the lobby's CHANGE MODE.
    WzQuickPlay,
    WzPrivate,
    WzPickMode,
    /// bo2mp: the custom lobby online with friends (`p2p`): open it, copy
    /// its invite code, close it; join a friend's by his code.
    P2pHost,
    P2pCopy,
    P2pClose,
    P2pJoin,
}

/// The main menu's modes still to come (SURVIVAL).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Soon {
    SpecOps,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub label: String,
    pub sub: String,
    /// A selector's value (drawn `< value >`).
    pub value: Option<String>,
    pub act: Act,
}

impl Row {
    fn new(label: &str, sub: &str, act: Act) -> Row {
        Row {
            label: label.to_owned(),
            sub: sub.to_owned(),
            value: None,
            act,
        }
    }
}

/// The Tactical Nuke's kill counts: off, Modern Warfare 2's 25 (the default), then others.
pub(crate) const MOAB_KILLS: [u32; 5] = [0, 25, 20, 15, 30];

pub(crate) const DIFFICULTY: [&str; 4] = ["RECRUIT", "REGULAR", "HARDENED", "VETERAN"];
/// Maps a picker page holds (6 by 4: a page fits on the screen, no
/// scrolling; a game with more gets more tabs).
pub(crate) const PAGE: usize = 24;
pub(crate) const COLUMNS: usize = 6;
/// The columns the map / item grid has on this screen, and its first row
/// in view (the view sets them; moving uses them).
pub(crate) static GRID_COLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(COLUMNS);
pub(crate) static GRID_FIRST_ROW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// OPTIONS' list: its first row in view, and the rows it shows and has (the
/// view sets them; the wheel and the scroll bar move the first).
pub(crate) static OPT_FIRST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static OPT_SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static OPT_ROWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Resource, Debug, Clone)]
pub(crate) struct Shell {
    pub screen: Screen,
    pub focus: usize,
    pub tab: Tab,
    pub map: String,
    pub mode: String,
    pub friends: u32,
    pub enemies: u32,
    pub difficulty: u32,
    /// The optional M.O.A.B.: an index into `MOAB_KILLS` (0: off).
    pub moab: u32,
    /// Custom Games' TRICKSHOT MODE (his Plutonium trickshot script); off.
    pub trickshot: bool,
    /// The map picker's page (a group, split by `PAGE`).
    pub page: usize,
    pub note: Option<(String, f64)>,
    /// He started a match from the lobby (coming back lands there).
    pub played: bool,
    /// Create-a-Class: the class, the slot (`cac::EDIT_SLOTS`), and the
    /// screen it was opened from.
    pub class: i32,
    pub slot: usize,
    pub cac_from: Screen,
    /// The clock (seconds) as of his last input, for notes.
    pub now: f64,
    /// His new name while he types it (Options > ACCOUNT).
    pub typing: Option<String>,
    /// bo2mp: Options: the 1-10 row (its tab and row number) whose exact
    /// value `typing` holds.
    pub typing_value: Option<(usize, u8)>,
    /// bo2mp: the calling card or emblem being named (R in the pickers)
    /// while `typing` holds its new name.
    pub typing_card: Option<String>,
    /// FRIENDS > ADD FRIEND: the code being typed.
    pub code_entry: Option<String>,
    /// Zombies: the map (`zm_transit`), its start (an index into its
    /// starts) and the starting round (an index into `zombies::ROUNDS`).
    pub zm_map: String,
    pub zm_start: usize,
    pub zm_round: usize,
    /// Zombies: the lobby's kind and Custom Games' bots (0-4).
    pub zm_kind: zombies::ZmKind,
    pub zm_bots: u32,
    /// Custom Games: Multiplayer's custom guns in the Mystery Box (on).
    pub zm_custom_weapons: bool,
    /// FIND A MATCH: the modes ticked (none: every mode), and whether the
    /// lobby list on screen is that search's.
    pub filter: Vec<String>,
    pub searching: bool,
    /// FIND A MATCH: BRAINROT 24/7 (Shipment, nothing else) is on.
    pub brainrot: bool,
    /// FIND A MATCH: when the search began, and its last join (attempt, when).
    pub search_started: f64,
    pub join_at: Option<(usize, f64)>,
    /// The matchmaking lobby's two maps to vote between (catalog
    /// `ui_name`s), his vote, and whether its leaderboard is showing.
    pub vote_maps: Vec<String>,
    pub vote: Option<usize>,
    pub leaderboard: bool,
    /// The public room's vote tally (in `vote_maps`' order), and whether he,
    /// its host, has opened the vote and started its winner.
    pub vote_counts: Vec<u8>,
    pub pregame: (bool, bool),
    /// bo2mp: he is setting up a public match (FIND A MATCH: searching or
    /// hosting for searchers), not a Custom Game: the mod's Zombies weapons
    /// are not listed and its START is a public match.
    pub public: bool,
    /// A yes / no question up, and YES (0) or NO (1) in focus.
    pub ask: Option<Ask>,
    pub ask_focus: usize,
    /// WARZONE: players a squad (1-4) and squads in the match.
    pub br_squad_size: u8,
    pub br_squads: u32,
    /// WARZONE: he is in its menus (Create-a-Class, Barracks and Options
    /// opened from them are drawn in its palette), its playlist (an index
    /// into `warzone::MODES`), and whether the playlist picker is QUICK
    /// PLAY's (a pick deploys).
    pub wz: bool,
    pub wz_mode: usize,
    pub wz_quick: bool,
    /// The calling card / emblem picker's category (an index into the
    /// categories its kind has, `cards::category_tabs`).
    pub look_cat: usize,
    /// The class editor opened in a match (`ingame`): no camos.
    pub in_game: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Shell {
            screen: Screen::Main,
            focus: 0,
            tab: Tab::Lobby,
            map: "mp_la".to_owned(),
            mode: "tdm".to_owned(),
            // bo2mp: Custom Game starts with no bots (`lobby_rules`).
            friends: 0,
            enemies: 0,
            difficulty: 1,
            moab: 1,
            trickshot: false,
            page: 0,
            note: None,
            played: false,
            class: 0,
            slot: 0,
            cac_from: Screen::Multiplayer,
            now: 0.0,
            typing: None,
            typing_value: None,
            typing_card: None,
            code_entry: None,
            zm_map: zombies::MAPS[0].map.to_owned(),
            zm_start: 0,
            zm_round: 0,
            zm_kind: zombies::ZmKind::Solo,
            zm_bots: 0,
            zm_custom_weapons: true,
            filter: Vec::new(),
            searching: false,
            brainrot: false,
            search_started: 0.0,
            join_at: None,
            vote_maps: Vec::new(),
            vote_counts: Vec::new(),
            pregame: (false, false),
            vote: None,
            leaderboard: false,
            public: false,
            ask: None,
            ask_focus: 1,
            br_squad_size: gamemode_iw4::resurgence::REBIRTH.squad_size,
            br_squads: 4,
            wz: false,
            wz_mode: 0,
            wz_quick: false,
            look_cat: 0,
            in_game: false,
        }
    }
}

/// The map picker's pages: (title, maps by index into the catalog).
pub(crate) fn pages(c: &Catalog) -> Vec<(String, Vec<usize>)> {
    let mut out = Vec::new();
    for g in c.groups() {
        let all: Vec<usize> = c.maps.iter().enumerate().filter(|(_, m)| m.group == g).map(|(i, _)| i).collect();
        let n = all.len().div_ceil(PAGE);
        for (k, chunk) in all.chunks(PAGE).enumerate() {
            let title = if n > 1 { format!("{g} {}", k + 1) } else { g.clone() };
            out.push((title, chunk.to_vec()));
        }
    }
    out
}

static CLASSIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the main menu is drawn in the Classic layout (Options: MENU STYLE).
pub(crate) fn classic() -> bool {
    CLASSIC.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_classic(on: bool) {
    CLASSIC.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A main-menu row's place (for coming back to it).
fn main_row(act: Act) -> usize {
    rows(&Shell::default(), &Catalog::default()).iter().position(|r| r.act == act).unwrap_or(0)
}

fn rows_len(s: &Shell, c: &Catalog) -> usize {
    rows(s, c).len()
}

/// The rows the screen shows.
pub(crate) fn rows(s: &Shell, c: &Catalog) -> Vec<Row> {
    match s.screen {
        Screen::Main if classic() => vec![
            Row::new("MULTIPLAYER", "Find a match, or a custom game with bots on every map.", Act::Multiplayer),
            Row::new("ZOMBIES", "Hold out against the undead, round after round.", Act::Zombies),
            Row::new("WARZONE", "Resurgence on Rebirth Island: squads, the gas, redeploys.", Act::Warzone),
            Row::new("SPEC OPS", "Coming soon.", Act::Soon(Soon::SpecOps)),
            Row::new("OPTIONS", "Graphics, sound and controls.", Act::Options),
            Row::new("FRIENDS", "Add friends by their code and see where they are.", Act::Friends),
            Row::new("QUIT", "Back to the desktop.", Act::Quit),
        ],
        Screen::Main => vec![
            Row::new("MULTIPLAYER", "Find a match, or a custom game with bots on every map.", Act::Multiplayer),
            Row::new("SURVIVAL", "Zombies: hold out against the undead, round after round.", Act::Zombies),
            Row::new("DROP ZONE", "Resurgence on Rebirth Island: squads, the gas, redeploys.", Act::Warzone),
            Row::new("OPTIONS", "Graphics, sound and controls.", Act::Options),
            Row::new("FRIENDS", "Add friends by their code and see where they are.", Act::Friends),
            Row::new("QUIT", "Back to the desktop.", Act::Quit),
        ],
        Screen::Multiplayer => vec![
            Row::new("FIND A MATCH", "Public matches by playlist, and the server's active lobbies.", Act::FindMatch),
            Row::new("CUSTOM GAME", "Pick the map, the mode and the bots, then play.", Act::CustomGame),
            p2p::join_row(),
            Row::new("CREATE A CLASS", "Build your loadouts.", Act::Classes),
            Row::new("BARRACKS", "Your combat record, calling card, emblem and prestige.", Act::Barracks),
            Row::new("OPTIONS", "Graphics, sound and controls.", Act::Options),
        ],
        Screen::Lobby => match s.tab {
            Tab::Lobby => {
                let mut r = vec![
                    Row::new("START MATCH", "Everyone in, the match begins.", Act::Start),
                    Row::new("CHANGE MAP", "Every map from every game installed.", Act::PickMap),
                    Row::new("CHANGE MODE", "Team Deathmatch, Domination, Search & Destroy...", Act::PickMode),
                ];
                if !s.public {
                    r.extend(p2p::lobby_rows());
                }
                r
            }
            Tab::Loadouts => vec![
                Row::new("CREATE A CLASS", "Build your loadouts.", Act::Classes),
                Row::new("BARRACKS", "Your combat record, calling card, emblem and prestige.", Act::Barracks),
            ],
            Tab::Settings => {
                let mode = c.mode(&s.mode).map_or_else(|| s.mode.to_uppercase(), |m| m.title.clone());
                let mut r = vec![
                    Row::new("GAME MODE", "", Act::Cycle(Setting::Mode)),
                    Row::new("FRIENDLY BOTS", "", Act::Cycle(Setting::Friends)),
                    Row::new("ENEMY BOTS", "", Act::Cycle(Setting::Enemies)),
                    Row::new("BOT DIFFICULTY", "", Act::Cycle(Setting::Difficulty)),
                    Row::new("MAP", "", Act::PickMap),
                    Row::new(
                        "TACTICAL NUKE",
                        "Optional, not Black Ops II: Modern Warfare 2's Tactical Nuke for a kill streak in one life, called in with Modern Warfare 2 Remastered's voice. It kills everyone and ends the match.",
                        Act::Cycle(Setting::Moab),
                    ),
                ];
                r[0].value = Some(mode);
                // bo2mp: each bot row with its live maximum (18 players in
                // all, `lobby_rules`); none friendly in a mode without teams.
                let (humans, teams) = (lobby_rules::humans(), lobby_rules::has_teams(&s.mode));
                r[1].value = Some(if teams {
                    format!("{}  /  {}", s.friends, lobby_rules::row_max(s.enemies, humans, teams, true))
                } else {
                    "NONE (NO TEAMS)".to_owned()
                });
                r[2].value = Some(format!("{}  /  {}", s.enemies, lobby_rules::row_max(s.friends, humans, teams, false)));
                if !teams {
                    r[2].label = "OPPONENT BOTS".to_owned();
                }
                r[3].value = Some(DIFFICULTY[s.difficulty.min(3) as usize].to_owned());
                r[4].value = Some(c.map(&s.map).map_or_else(|| s.map.clone(), |m| m.title.clone()));
                r[5].value = Some(match MOAB_KILLS.get(s.moab as usize).copied().unwrap_or(0) {
                    0 => "OFF".to_owned(),
                    n => format!("{n} KILLS"),
                });
                // Custom Games only: never in a public match.
                if !s.public {
                    let mut t = Row::new(
                        "TRICKSHOT MODE",
                        "Custom games only: your Plutonium trickshot script (its menu, binds, teleport flags, crates and platforms) runs in the match.",
                        Act::Cycle(Setting::Trickshot),
                    );
                    t.value = Some(if s.trickshot { "ON" } else { "OFF" }.to_owned());
                    r.push(t);
                }
                r
            }
        },
        Screen::ModePick => c
            .modes
            .iter()
            .enumerate()
            .map(|(i, m)| Row::new(&m.title, &m.desc, Act::Choose(i)))
            .collect(),
        Screen::Classes | Screen::ClassEdit | Screen::ItemPick => classes::book().map(|b| cac::rows(s, &b)).unwrap_or_default(),
        Screen::Friends => friends::rows(s),
        Screen::Lobbies => lobbies::rows(s, c),
        Screen::MatchFilter => matchmaking::rows(s, c),
        Screen::Room => room::rows(s, c),
        Screen::Options => {
            let mut r = options::page_rows(s.page, &options::snapshot());
            if let Some(t) = &s.typing {
                let typed = match s.typing_value {
                    Some((_, k)) => Act::Cycle(Setting::Option(k)),
                    None => Act::TypeName,
                };
                if let Some(row) = r.iter_mut().find(|r| r.act == typed) {
                    row.value = Some(format!("{t}_"));
                }
            }
            r
        }
        Screen::Barracks => {
            let mut r = vec![Row::new("COMBAT RECORD", "Your kills, wins and time played.", Act::Barracks)];
            for (label, sub, kind) in [
                ("CALLING CARD", "Your calling card, from Modern Warfare 2, 3 and III.", cards::Kind::Card),
                ("EMBLEM", "Your emblem, from Modern Warfare 2, 3 and III.", cards::Kind::Emblem),
            ] {
                let stat = if kind == cards::Kind::Card { &c.player.card } else { &c.player.emblem };
                let mut row = Row::new(label, sub, Act::Looks(kind));
                row.value = Some(cards::title(&cards::mine(kind, stat)));
                r.push(row);
            }
            if c.player.can_prestige {
                r.push(Row::new("ENTER PRESTIGE", "Back to level 1 with the next prestige icon.", Act::Prestige));
            } else {
                r.push(Row::new("PRESTIGE", "Reach level 155 to enter the next prestige.", Act::Prestige));
            }
            r
        }
        Screen::Zombies | Screen::ZmMapPick | Screen::ZmStartPick | Screen::ZmLobby | Screen::ZmRecord => zombies::rows(s),
        Screen::WzMenu | Screen::WzLobby | Screen::WzModePick => warzone::rows(s, c),
        Screen::CardPick | Screen::EmblemPick => {
            let kind = look_kind(s.screen);
            let list = cards::shown(kind, s.look_cat);
            cards::page_items(kind, &list, s.page).iter().enumerate().map(|(k, n)| Row::new(&cards::title(n), &cards::label(n), Act::Choose(k))).collect()
        }
        Screen::MapPick => {
            let p = pages(c);
            p.get(s.page.min(p.len().saturating_sub(1)))
                .map(|(_, maps)| {
                    maps.iter()
                        .enumerate()
                        .map(|(k, &i)| Row::new(&c.maps[i].title, &c.maps[i].origin, Act::Choose(k)))
                        .collect()
                })
                .unwrap_or_default()
        }
    }
}

impl Shell {
    /// bo2mp: he is in the Zombies front end: on a Zombies screen, or on a
    /// shared one (Options, Barracks and its card / emblem pickers,
    /// Create-a-Class) opened from one (`cac_from`, the screen those were
    /// opened from). Everything there shows Zombies' pictures and plays
    /// Zombies' menu music only (`background`, `music_screen`).
    pub(crate) fn in_zombies(&self) -> bool {
        self.screen.is_zombies()
            || (matches!(
                self.screen,
                Screen::Options
                    | Screen::Barracks
                    | Screen::CardPick
                    | Screen::EmblemPick
                    | Screen::Classes
                    | Screen::ClassEdit
                    | Screen::ItemPick
            ) && self.cac_from.is_zombies())
    }

    /// The calling card / emblem picker shows another category, from its
    /// first page.
    pub(crate) fn set_look_cat(&mut self, cat: usize) {
        if cat != self.look_cat {
            self.look_cat = cat;
            self.page = 0;
            self.focus = 0;
        }
    }
}

/// The kind a picker screen picks.
pub(crate) fn look_kind(screen: Screen) -> cards::Kind {
    if screen == Screen::EmblemPick { cards::Kind::Emblem } else { cards::Kind::Card }
}

/// A yes / no question over a screen (NO first in focus).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ask {
    Prestige,
    /// A notice with OK alone: a main-menu mode that is not built yet.
    Soon(Soon),
    /// bo2mp: the notice he comes back to the front end with when his game
    /// ended under him (the host ended it or left, the session closed, the
    /// join failed, the map did not load): NOTICE and why (`ended_text`), OK alone. It replaces
    /// the front end's `codlegacy_connection_error` / BO2's
    /// `popup_xboxlive_partyended`, which the shell covers.
    Ended,
}

/// bo2mp: the words of the game-ended notice up or due (`Ask::Ended`).
static ENDED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// ... a notice queued from BO2's side (its party ended for him), shown
/// when nothing else is asked.
static ENDED_QUEUED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// ... he closed it (OK or back): the menu dvars it came from are cleared.
static ENDED_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ended_text() -> String {
    ENDED.lock().ok().and_then(|e| e.clone()).unwrap_or_default()
}

/// bo2mp: his game ended under him (BO2's party ended, he was removed
/// from it): the shell's notice says `why` on the front end.
pub(crate) fn game_ended_notice(why: &str) {
    if let Ok(mut q) = ENDED_QUEUED.lock() {
        *q = Some(why.to_owned());
    }
}

impl Ask {
    pub(crate) fn text(self) -> (&'static str, String) {
        match self {
            Ask::Prestige => ("ENTER PRESTIGE?", "You go back to level 1 with the next prestige icon. Your unlocks start over.".to_owned()),
            Ask::Soon(Soon::SpecOps) => (if classic() { "SPEC OPS" } else { "SURVIVAL" }, "Coming soon.".to_owned()),
            // (The front end's notice was titled `@MENU_NOTICE`.)
            Ask::Ended => ("NOTICE", ended_text()),
        }
    }

    /// Its buttons: YES / NO, or OK for a notice.
    pub(crate) fn buttons(self) -> &'static [&'static str] {
        match self {
            Ask::Prestige => &["YES", "NO"],
            Ask::Soon(_) | Ask::Ended => &["OK"],
        }
    }
}

impl Shell {
    /// YES to a question.
    fn answer(&mut self, ask: Ask, now: f64) {
        match ask {
            Ask::Prestige => {
                bridge::request(Request::Prestige);
                self.note("Prestige entered.", now);
            }
            Ask::Soon(_) | Ask::Ended => {}
        }
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.focus = 0;
    }

    fn note(&mut self, text: &str, now: f64) {
        self.note = Some((text.to_owned(), now + 3.5));
    }

    /// bo2mp: the bots held to the lobby's rules (`lobby_rules::fit`) with
    /// `humans` real players in it; whether they changed.
    pub(crate) fn fit_bots(&mut self, humans: u32) -> bool {
        let fitted = lobby_rules::fit(self.friends, self.enemies, humans, lobby_rules::has_teams(&self.mode));
        let changed = fitted != (self.friends, self.enemies);
        (self.friends, self.enemies) = fitted;
        changed
    }

    /// bo2mp: FRIENDLY / ENEMY BOTS stepped by `by`, never past the cap.
    fn step_bots(&mut self, friendly: bool, by: i32, humans: u32) {
        self.fit_bots(humans);
        let teams = lobby_rules::has_teams(&self.mode);
        let (v, other) = if friendly { (self.friends, self.enemies) } else { (self.enemies, self.friends) };
        let max = lobby_rules::row_max(other, humans, teams, friendly);
        match lobby_rules::step(v, by, max) {
            Some(n) if friendly => self.friends = n,
            Some(n) => self.enemies = n,
            None if friendly && !teams => self.note("No friendly bots: this mode has no teams.", self.now),
            None if by > 0 => self.note(&format!("{} players at most: {max} {} bots with {humans} in the lobby.", lobby_rules::MAX_PLAYERS, if friendly { "friendly" } else { "enemy" }), self.now),
            None => {}
        }
    }

    /// A note that stays up longer (a long job's end).
    fn note_long(&mut self, text: &str, now: f64) {
        self.note = Some((text.to_owned(), now + 15.0));
    }

    fn back(&mut self) {
        match self.screen {
            // (ADVANCED VIDEO: back to VIDEO OPTIONS, on its row.)
            Screen::Options if self.page == options::ADVANCED_PAGE => {
                self.page = 0;
                self.focus = options::VIDEO_MAIN;
            }
            // (PREPARE MAPS: back to GAME FOLDERS, on its row.)
            Screen::Options if self.page == options::PREPARE_PAGE => {
                self.page = options::FOLDERS_TAB;
                self.focus = options::prepare_row();
            }
            // (KEY BINDINGS: back to MOUSE & KEYBOARD, on its row.)
            Screen::Options if self.page == keys::KEYS_PAGE => {
                self.page = 2;
                self.focus = options::KEYS_ROW;
            }
            Screen::Options | Screen::Barracks => {
                let from = self.cac_from;
                let was = self.screen;
                self.go(from);
                self.page = 0;
                self.focus = match (from, was) {
                    (Screen::Main, _) => main_row(Act::Options),
                    (Screen::Multiplayer, Screen::Barracks) => 3,
                    (Screen::Multiplayer, _) => 4,
                    (Screen::WzMenu, Screen::Barracks) => 3,
                    (Screen::WzMenu, _) => 4,
                    (Screen::Zombies, _) => 4,
                    (Screen::Lobby | Screen::WzLobby, _) => 1,
                    _ => 0,
                };
            }
            Screen::Classes => {
                let from = self.cac_from;
                self.go(from);
                self.focus = if matches!(from, Screen::Multiplayer | Screen::WzMenu) { 2 } else { 0 };
            }
            Screen::ClassEdit => {
                let n = self.class.max(0) as usize;
                self.go(Screen::Classes);
                self.focus = n;
            }
            Screen::ItemPick => {
                let slot = self.slot;
                self.screen = Screen::ClassEdit;
                self.page = 0;
                // (Back on the row of the slot he picked for.)
                self.focus = classes::book()
                    .and_then(|b| {
                        let c = b.classes.get(self.class as usize)?;
                        cac::rows(self, &b).iter().position(|r| r.act == Act::EditSlot(slot)).filter(|_| c.n >= 0)
                    })
                    .unwrap_or(0);
            }
            Screen::CardPick | Screen::EmblemPick => {
                let from = self.screen;
                self.go(Screen::Barracks);
                self.page = 0;
                self.focus = if from == Screen::CardPick { 1 } else { 2 };
            }
            Screen::Main => {}
            Screen::Friends => {
                self.go(Screen::Main);
                self.focus = main_row(Act::Friends);
            }
            Screen::Lobbies if self.searching => {
                self.searching = false;
                self.go(Screen::MatchFilter);
                self.focus = rows_len(self, &bridge::catalog().unwrap_or_default()).saturating_sub(1);
            }
            Screen::Lobbies => {
                self.go(Screen::MatchFilter);
                self.page = matchmaking::ACTIVE_LOBBIES;
            }
            Screen::MatchFilter => {
                self.go(Screen::Multiplayer);
                self.page = 0;
                self.focus = 0;
            }
            // (The shell leaves the room screen once he is out of it.)
            Screen::Room => crate::online_match::queue_leave(),
            Screen::Multiplayer => self.go(Screen::Main),
            Screen::WzMenu => {
                self.wz = false;
                self.go(Screen::Main);
                self.focus = main_row(Act::Warzone);
            }
            Screen::WzLobby => {
                if bridge::starting_in().is_some() {
                    bridge::request(Request::CancelStart);
                } else {
                    self.go(Screen::WzMenu);
                    self.focus = 1;
                }
            }
            Screen::WzModePick if self.wz_quick => {
                self.wz_quick = false;
                self.go(Screen::WzMenu);
                self.focus = 0;
            }
            Screen::WzModePick => {
                self.go(Screen::WzLobby);
                self.focus = if self.tab == Tab::Settings { 0 } else { 1 };
            }
            Screen::Zombies => {
                self.go(Screen::Main);
                self.focus = main_row(Act::Zombies);
            }
            Screen::ZmMapPick | Screen::ZmStartPick => {
                let from = self.screen;
                self.go(Screen::ZmLobby);
                self.page = 0;
                self.focus = if from == Screen::ZmMapPick { 1 } else { 2 };
            }
            Screen::ZmLobby => {
                let k = self.zm_kind;
                self.go(Screen::Zombies);
                self.focus = k.menu_row();
            }
            Screen::ZmRecord => {
                self.go(Screen::Zombies);
                self.focus = 3;
            }
            Screen::Lobby => {
                if bridge::starting_in().is_some() {
                    bridge::request(Request::CancelStart);
                } else {
                    if p2p::hosting() {
                        crate::online_match::queue_leave();
                    }
                    self.go(Screen::Multiplayer);
                    self.focus = 1;
                }
            }
            Screen::MapPick | Screen::ModePick => {
                let from = self.screen;
                self.go(Screen::Lobby);
                self.focus = match (self.tab, from) {
                    (Tab::Settings, Screen::MapPick) => 4,
                    (Tab::Settings, _) => 0,
                    (_, Screen::MapPick) => 1,
                    _ => 2,
                };
            }
        }
    }

    fn cycle(&mut self, what: Setting, by: i32, c: &Catalog) {
        let step = |v: u32, max: u32| ((v as i32 + by).rem_euclid(max as i32 + 1)) as u32;
        match what {
            Setting::Friends | Setting::Enemies => self.step_bots(what == Setting::Friends, by, lobby_rules::humans()),
            Setting::Difficulty => self.difficulty = step(self.difficulty, 3),
            Setting::Moab => self.moab = step(self.moab, MOAB_KILLS.len() as u32 - 1),
            Setting::Trickshot => self.trickshot = !self.trickshot,
            Setting::ZmRound => self.zm_round = step(self.zm_round as u32, zombies::ROUNDS.len() as u32 - 1) as usize,
            // bo2mp: four players at most in Zombies: bots up to 4 - real players.
            Setting::ZmBots => self.zm_bots = step(self.zm_bots.min(zombies::bot_cap(zombies::CUSTOM_HUMANS)), zombies::bot_cap(zombies::CUSTOM_HUMANS)),
            Setting::ZmCustomWeapons => self.zm_custom_weapons = !self.zm_custom_weapons,
            Setting::BrMode | Setting::BrSquadSize | Setting::BrSquads => warzone::cycle(self, what, by),
            Setting::Option(k) => options::queue(options::tab_of(self.page), k, by),
            Setting::LethalCount | Setting::TacticalCount => {
                if let Some(b) = classes::book() {
                    cac::count(self, &b, what == Setting::LethalCount, by, self.now);
                }
            }
            Setting::Mode => {
                if !c.modes.is_empty() {
                    let i = c.modes.iter().position(|m| m.key == self.mode).unwrap_or(0) as i32;
                    let n = c.modes.len() as i32;
                    self.mode = c.modes[(i + by).rem_euclid(n) as usize].key.clone();
                    self.fit_bots(lobby_rules::humans());
                }
            }
        }
    }

    fn activate(&mut self, act: Act, c: &Catalog, now: f64, exit: &mut MessageWriter<AppExit>) {
        match act {
            Act::P2pHost | Act::P2pCopy | Act::P2pClose | Act::P2pJoin => p2p::activate(self, &act, now),
            Act::Multiplayer => {
                self.wz = false;
                self.go(Screen::Multiplayer);
            }
            Act::Warzone => {
                self.wz = true;
                // (Resurgence's sounds read ahead, off the match's load.)
                audio::prefetch_warzone_sounds();
                self.go(Screen::WzMenu);
            }
            Act::WzQuickPlay => {
                self.wz_quick = true;
                self.go(Screen::WzModePick);
                self.focus = self.wz_mode;
            }
            Act::WzPrivate => {
                self.go(Screen::WzLobby);
                self.tab = Tab::Lobby;
            }
            Act::WzPickMode => {
                self.wz_quick = false;
                self.go(Screen::WzModePick);
                self.focus = self.wz_mode;
            }
            Act::BrStart => {
                if let Err(e) = warzone::start(self, c) {
                    self.note(&e, now);
                }
            }
            Act::Zombies => {
                self.wz = false;
                zombies::refresh();
                self.go(Screen::Zombies);
            }
            Act::ZmPlay => match zombies::start(self) {
                Ok(()) => {}
                Err(e) => self.note(&e, now),
            },
            Act::ZmPickMap => {
                zombies::open_map_pick(self);
            }
            Act::ZmPickStart => {
                self.go(Screen::ZmStartPick);
                self.focus = self.zm_start;
            }
            Act::ZmLobby(kind) => {
                self.zm_kind = kind;
                self.go(Screen::ZmLobby);
            }
            Act::ZmRecord => self.go(Screen::ZmRecord),
            Act::Options => {
                self.cac_from = self.screen;
                self.go(Screen::Options);
                self.page = 0;
            }
            Act::Friends => self.go(Screen::Friends),
            Act::AddFriend => self.code_entry = Some(String::new()),
            Act::Friend(code) => self.note(&friends::friend_note(code), now),
            Act::Lobbies => {
                self.searching = false;
                self.go(Screen::Lobbies);
            }
            Act::FindMatch => {
                self.public = true;
                self.go(Screen::MatchFilter);
                self.page = 0;
            }
            Act::FilterMode(i) => matchmaking::toggle(self, c, i),
            Act::Brainrot => self.brainrot = !self.brainrot,
            Act::SearchMatches => {
                self.public = true;
                self.searching = true;
                self.search_started = now;
                self.join_at = None;
                self.vote_maps = matchmaking::vote_maps(self, c, now);
                self.vote = None;
                self.leaderboard = false;
                self.go(Screen::Lobbies);
                if !crate::online_match::online_available() {
                    self.note("No server set: fill in online.cfg next to run-bo2.cmd (see docs\\ONLINE.md).", now);
                }
            }
            Act::HostMatch => {
                // (His own lobby in the mode searched for: START opens it on
                // the server, where the next search finds it.)
                self.public = true;
                self.mode = matchmaking::host_mode(self);
                if self.brainrot
                    && let Some(m) = matchmaking::shipment(c)
                {
                    self.map = m.ui_name.clone();
                }
                self.searching = false;
                self.go(Screen::Lobby);
                self.tab = Tab::Lobby;
                self.note("Hosting: press START MATCH to open your lobby to players searching.", now);
            }
            Act::Lobby(id) => self.note(&lobbies::lobby_note(id), now),
            Act::Vote(k) => {
                if k < self.vote_maps.len() && self.vote != Some(k) {
                    self.vote = Some(k);
                    if crate::online_match::room().is_some_and(|r| r.vote.is_some()) {
                        crate::online_match::cast_vote(k);
                    }
                }
            }
            Act::Leaderboard => self.leaderboard = !self.leaderboard,
            Act::Barracks if self.screen != Screen::Barracks => {
                self.cac_from = self.screen;
                self.go(Screen::Barracks);
            }
            Act::Barracks | Act::Info => {}
            Act::Looks(kind) => {
                // The page and place of the one he has.
                let stat = if kind == cards::Kind::Card { &c.player.card } else { &c.player.emblem };
                let now = cards::mine(kind, stat);
                // (In the category of the one he has.)
                let cat = cards::category_of_in(&cards::list(kind), &now);
                let list = cards::shown(kind, cat);
                let at = list.iter().position(|n| *n == now).unwrap_or(0);
                let per = cards::per_page(kind);
                self.go(if kind == cards::Kind::Card { Screen::CardPick } else { Screen::EmblemPick });
                self.look_cat = cat;
                self.page = at / per;
                self.focus = at % per;
            }
            Act::Prestige => {
                if !c.player.can_prestige {
                    self.note("Reach level 155 to enter the next prestige.", now);
                } else {
                    self.ask = Some(Ask::Prestige);
                    self.ask_focus = 1;
                }
            }
            Act::Soon(which) => {
                self.ask = Some(Ask::Soon(which));
                self.ask_focus = 0;
            }
            Act::Quit => {
                exit.write(AppExit::Success);
            }
            Act::CustomGame => {
                self.public = false;
                self.go(Screen::Lobby);
                self.tab = Tab::Lobby;
            }
            Act::Start => {
                if bridge::starting_in().is_some() {
                    return;
                }
                self.played = true;
                sim::set_moab_kills(MOAB_KILLS.get(self.moab as usize).copied().unwrap_or(0));
                // (Never in a public match: the bridge turns it off for one.)
                // bo2mp: a Tricky mode plays its mode with his trickshot
                // script; Tricky Search and Destroy with two bots only.
                let tricky = bridge::tricky_base(&self.mode).filter(|_| !self.public);
                // bo2mp: JIGGY FREE FOR ALL plays Free For All with the Jiggy
                // mod menu's scripts (Custom Games only), not his trickshot
                // script (both would take `_development_dvars`).
                let jiggy = bridge::jiggy_base(&self.mode).filter(|_| !self.public);
                sim::set_jiggy(jiggy.is_some());
                sim::set_trickshot(!self.public && jiggy.is_none() && (self.trickshot || tricky.is_some()));
                // bo2mp: the bots the lobby shows, held to its rules (18
                // players at most, none friendly without teams).
                self.fit_bots(lobby_rules::humans());
                let (bot_friends, bot_enemies) = if tricky == Some("sd") { bridge::TRICKY_SD_BOTS } else { (self.friends, self.enemies) };
                bridge::request(Request::Start(StartSpec {
                    map: self.map.clone(),
                    gametype: tricky.or(jiggy).map_or_else(|| self.mode.clone(), str::to_owned),
                    bot_friends,
                    bot_enemies,
                    bot_difficulty: self.difficulty,
                    public: self.public,
                    br: None,
                }));
            }
            Act::PickMap => {
                // The page and tile of the map he has.
                let p = pages(c);
                let at = c.maps.iter().position(|m| m.ui_name == self.map);
                let (page, k) = at
                    .and_then(|i| p.iter().enumerate().find_map(|(pi, (_, maps))| maps.iter().position(|&m| m == i).map(|k| (pi, k))))
                    .unwrap_or((0, 0));
                self.screen = Screen::MapPick;
                self.page = page;
                self.focus = k;
            }
            Act::PickMode => {
                self.screen = Screen::ModePick;
                self.focus = c.modes.iter().position(|m| m.key == self.mode).unwrap_or(0);
            }
            // (His name is typed, not stepped.)
            Act::TypeName => {
                self.typing = Some(options::snapshot().player_name);
            }
            Act::ScanGames => {
                let line = crate::game_folders::start_scan();
                self.note(&line, now);
            }
            Act::ScanFolder => {
                let line = crate::game_folders::start_folder_scan();
                self.note(&line, now);
            }
            Act::ClearCache => {
                let line = crate::game_folders::clear_cache();
                self.note_long(&line, now);
            }
            Act::BakeAll => {
                let line = crate::game_folders::bake_all(&options::snapshot().prepare_games_off);
                self.note_long(&line, now);
            }
            Act::PickFolder(i) => {
                if let Some((key, title, _)) = asset_transport::game_paths::GAMES.get(i) {
                    match crate::game_folders::start_pick(key, title) {
                        Some(line) => self.note(&line, now),
                        None => self.note("A folder window is already open.", now),
                    }
                }
            }
            // bo2mp: a 1-10 sensitivity row: Enter or a click types its value.
            Act::Cycle(Setting::Option(k)) if self.screen == Screen::Options && options::scale_range(options::tab_of(self.page), k).is_some() => {
                let tab = options::tab_of(self.page);
                let s = options::snapshot();
                let shown = options::page_rows(self.page, &s).into_iter().find(|r| r.act == act).and_then(|r| r.value).unwrap_or_default();
                self.typing = Some(shown);
                self.typing_value = Some((tab, k));
            }
            Act::Cycle(what) => self.cycle(what, 1, c),
            Act::SubPage(page) => {
                self.page = page;
                self.focus = 0;
            }
            Act::Rebind(i) => keys::listen(i, now),
            Act::ResetKeys => {
                keys::reset();
                self.note("Every action is back on Black Ops II's own keys.", now);
            }
            Act::Classes => {
                self.cac_from = self.screen;
                self.go(Screen::Classes);
            }
            Act::EditClass(n) => {
                self.class = n;
                self.go(Screen::ClassEdit);
            }
            Act::EditSlot(i) => {
                if let Some(b) = classes::book() {
                    cac::open_pick(self, &b, i);
                }
            }
            Act::Choose(i) => match self.screen {
                Screen::ModePick => {
                    if let Some(m) = c.modes.get(i) {
                        self.mode = m.key.clone();
                        self.fit_bots(lobby_rules::humans());
                    }
                    self.back();
                }
                Screen::MapPick => {
                    let p = pages(c);
                    if let Some(&m) = p.get(self.page).and_then(|(_, maps)| maps.get(i)) {
                        self.map = c.maps[m].ui_name.clone();
                    }
                    self.back();
                }
                Screen::ItemPick => {
                    if let Some(b) = classes::book()
                        && cac::choose(self, &b, i, now)
                    {
                        self.back();
                    }
                }
                Screen::ZmMapPick => {
                    zombies::choose_map(self, i);
                    self.back();
                }
                Screen::ZmStartPick => {
                    self.zm_start = i;
                    self.back();
                }
                Screen::WzModePick => {
                    warzone::set_mode(self, i);
                    if self.wz_quick {
                        // QUICK PLAY: straight in (from the squad lobby, where
                        // the match comes back to; it stays up if it cannot
                        // start).
                        self.wz_quick = false;
                        self.go(Screen::WzLobby);
                        self.tab = Tab::Lobby;
                        if let Err(e) = warzone::start(self, c) {
                            self.note(&e, now);
                        }
                    } else {
                        self.back();
                    }
                }
                Screen::CardPick | Screen::EmblemPick => {
                    let kind = look_kind(self.screen);
                    let list = cards::shown(kind, self.look_cat);
                    if let Some(name) = cards::page_items(kind, &list, self.page).get(i) {
                        cards::set(kind, name);
                        self.note(&format!("{} equipped.", cards::title(name)), now);
                    }
                    self.back();
                }
                _ => {}
            },
        }
        let _ = now;
    }
}

/// A clickable part of a screen.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) enum Hit {
    Row(usize),
    Tab(usize),
    Page(usize),
    /// A selector's arrow: the row, -1 or +1.
    Step(usize, i32),
    /// A picker's page arrow: -1 or +1.
    PageStep(i32),
    /// The calling card / emblem picker's category tab.
    LookCat(usize),
    Back,
    /// The weapon picker's strip arrows (`‹` / `›`): the gun to focus.
    Strip(usize),
    /// A yes / no question's YES (0) or NO (1).
    Ask(usize),
}

#[derive(Default)]
struct PadRepeat {
    held: Option<(i32, i32)>,
    next_at: f64,
}

#[allow(clippy::too_many_arguments)]
fn shell_input(
    mut shell: ResMut<Shell>,
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    hits: Query<(&Hit, &Interaction)>,
    hud_keys: Option<Res<frame::HudInputView>>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
    mut repeat: Local<PadRepeat>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut sounds: MessageWriter<frame::UiPlaySound>,
    mut heard: Local<Option<(Screen, usize, Tab, usize)>>,
    mut cursor: MessageReader<bevy::window::CursorMoved>,
    mut mouse_leads: Local<bool>,
) {
    // bo2mp: the mouse leads focus only once it has moved (or clicked) since
    // the last key or pad press: an idle cursor resting on a map tile took
    // the focus back every frame, so the pad could not move on the grid.
    if cursor.read().count() > 0 || mouse.get_just_pressed().next().is_some() {
        *mouse_leads = true;
    }
    if !enabled() || front.is_none() || crate::game_folders::is_open() || zombies::running() {
        typed.clear();
        return;
    }
    let now_s = time.elapsed_secs_f64();
    // bo2mp: PREPARE MAPS AT LAUNCH, once a start, as soon as his settings
    // are read (never on a proof run or in the menus' preview pictures).
    {
        static CHECKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if options::loaded() && !CHECKED.swap(true, std::sync::atomic::Ordering::Relaxed) && options::snapshot().prepare_maps_at_launch && std::env::var_os("BO2MP_SHELL_PROOF_MAP").is_none()
            && !preview::active()
            && let Some(line) = crate::game_folders::prepare_at_launch(&options::snapshot().prepare_games_off)
        {
            shell.note_long(&line, now_s);
        }
    }
    // bo2mp: OPTIONS > KEY BINDINGS: while it waits for a key (PRESS A KEY)
    // the console has the keys, and the menu waits until they are let go.
    if keys::capturing(now_s, keys.get_pressed().next().is_some() || mouse.get_pressed().next().is_some()) {
        typed.clear();
        return;
    }
    if let Some(line) = keys::take_note() {
        shell.note(&line, now_s);
    }
    if shell.screen == Screen::Options
        && shell.page == keys::KEYS_PAGE
        && shell.ask.is_none()
        && keys.just_pressed(KeyCode::Delete)
        && let Some(label) = keys::clear(shell.focus)
    {
        shell.note(&format!("{label}: UNBOUND."), now_s);
        sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
    }
    // bo2mp: a player joining his Custom Game past 18 in all trims the bots
    // (enemies first, then friendly: `lobby_rules`).
    if matches!(shell.screen, Screen::Lobby | Screen::ModePick | Screen::MapPick) {
        let humans = lobby_rules::humans();
        let fitted = lobby_rules::fit(shell.friends, shell.enemies, humans, lobby_rules::has_teams(&shell.mode));
        if fitted != (shell.friends, shell.enemies) && shell.fit_bots(humans) {
            shell.note(&format!("{} players at most: bots taken out to make room.", lobby_rules::MAX_PLAYERS), now_s);
        }
        // Load speed: the map he has chosen, or the tile under the cursor
        // on the map grid, is read ahead now, not in START MATCH's countdown
        // (`CODLEGACY_T6_PREWARM_PICK=0`: the old way).
        let focused = (shell.screen == Screen::MapPick)
            .then(bridge::catalog)
            .flatten()
            .and_then(|c| pages(&c).get(shell.page).and_then(|(_, maps)| maps.get(shell.focus)).map(|&m| c.maps[m].ui_name.clone()));
        assets::prewarm_t6_mp_pick(focused.as_deref().unwrap_or(&shell.map));
    }
    // bo2mp: F10 opens (and closes) Options > GAME FOLDERS; a folder picked
    // in its window comes back here.
    if crate::game_folders::take_open_request() && shell.ask.is_none() && shell.typing.is_none() && shell.code_entry.is_none() {
        if shell.screen == Screen::Options && shell.page == options::FOLDERS_TAB {
            shell.back();
        } else {
            if shell.screen != Screen::Options {
                shell.cac_from = shell.screen;
            }
            shell.go(Screen::Options);
            shell.page = options::FOLDERS_TAB;
        }
    }
    if let Some(line) = crate::game_folders::take_picked() {
        shell.note(&line, now_s);
    }
    // bo2mp: F5 on GAME FOLDERS scans for the games (as its SCAN FOR GAMES
    // row); what it found comes back here.
    if shell.screen == Screen::Options
        && shell.page == options::FOLDERS_TAB
        && shell.typing.is_none()
        && keys.just_pressed(KeyCode::F5)
    {
        let line = crate::game_folders::start_scan();
        shell.note(&line, now_s);
    }
    if let Some(line) = crate::game_folders::take_scanned() {
        shell.note_long(&line, now_s);
    }
    if let Some(line) = assets::bake_all::take_finished() {
        shell.note_long(&line, now_s);
    }
    if shell.screen == Screen::Options
        && shell.page == options::FOLDERS_TAB
        && !crate::game_folders::picking()
        && (keys.just_pressed(KeyCode::Delete) || pads.iter().any(|p| p.just_pressed(GamepadButton::West)))
        && let Some((key, title, _)) = asset_transport::game_paths::GAMES.get(shell.focus)
    {
        let line = crate::game_folders::clear(key, title);
        shell.note(&line, now_s);
    }
    // bo2mp: X on Modern Warfare III's row makes a small copy of its guns,
    // gun sounds, calling cards and emblems (`jup_pack`); its progress shows
    // in the notes.
    if shell.screen == Screen::Options
        && shell.page == options::FOLDERS_TAB
        && shell.typing.is_none()
        && (keys.just_pressed(KeyCode::KeyX) || pads.iter().any(|p| p.just_pressed(GamepadButton::North)))
        && asset_transport::game_paths::GAMES.get(shell.focus).is_some_and(|(k, _, _)| *k == crate::jup_cards::PREFIX)
    {
        let line = crate::jup_pack::start();
        shell.note(&line, now_s);
    }
    for (line, last) in crate::jup_pack::take_notes() {
        if last {
            shell.note_long(&line, now_s);
        } else {
            shell.note(&line, now_s);
        }
    }
    // Online: the room he joined shows until its host starts the match (its
    // menus open over it: Create a Class, Barracks, Options).
    let over_room = matches!(
        shell.screen,
        Screen::Room | Screen::Classes | Screen::ClassEdit | Screen::ItemPick | Screen::Barracks | Screen::Options | Screen::CardPick | Screen::EmblemPick
    );
    match (crate::online_match::room().is_some(), shell.screen == Screen::Room) {
        (true, false) if !over_room => {
            shell.go(Screen::Room);
            shell.focus = 0;
        }
        (false, true) if shell.searching => {
            // (Out of the room: back to the search, which looks again.)
            shell.go(Screen::Lobbies);
            shell.search_started = now_s;
            shell.join_at = None;
        }
        (false, true) => {
            shell.go(Screen::MatchFilter);
            shell.page = matchmaking::ACTIVE_LOBBIES;
            shell.focus = 0;
        }
        _ => {}
    }
    matchmaking::tick(&mut shell, now_s);
    if let Some(c) = bridge::catalog() {
        pregame::tick(&mut shell, &c, now_s);
    }
    if let Some(alias) = matchmaking::countdown_tick(&shell) {
        sounds.write(frame::UiPlaySound { alias: alias.to_owned() });
    }
    // FRIENDS > ADD FRIEND: digits (8 at most); Enter adds the friend,
    // Esc leaves the list as it was.
    if let Some(mut t) = shell.code_entry.clone() {
        use bevy::input::keyboard::Key;
        for ev in typed.read() {
            if !ev.state.is_pressed() {
                continue;
            }
            match &ev.logical_key {
                Key::Enter => {
                    let code: u32 = t.parse().unwrap_or(0);
                    if frame::GameSettings::is_friend_code(code) {
                        friends::queue_add(code);
                        shell.note(&format!("Friend {} added.", frame::GameSettings::friend_code_text(code)), now_s);
                        sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                    } else {
                        shell.note("A friend code is 8 digits.", now_s);
                    }
                    shell.code_entry = None;
                    return;
                }
                Key::Escape => {
                    shell.code_entry = None;
                    sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
                    return;
                }
                Key::Backspace => {
                    t.pop();
                }
                Key::Character(s) => {
                    for ch in s.chars().filter(char::is_ascii_digit) {
                        if t.len() < 8 {
                            t.push(ch);
                        }
                    }
                }
                _ => {}
            }
        }
        if shell.code_entry.as_ref() != Some(&t) {
            shell.code_entry = Some(t);
        }
        return;
    }
    // FRIENDS: Del removes the friend in focus.
    if shell.screen == Screen::Friends {
        use bevy::input::keyboard::Key;
        let del = typed.read().any(|ev| ev.state.is_pressed() && ev.logical_key == Key::Delete);
        if del
            && let Some(cat) = bridge::catalog()
            && let Some(Act::Friend(code)) = rows(&shell, &cat).get(shell.focus).map(|r| r.act)
        {
            friends::queue_remove(code);
            shell.note(&format!("Friend {} removed.", frame::GameSettings::friend_code_text(code)), now_s);
            sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
            shell.focus = shell.focus.saturating_sub(1);
            return;
        }
    }
    // His new name, typed: letters, digits and a few marks; Enter keeps it,
    // Esc leaves it as it was.
    // bo2mp: R in a calling card or emblem picker names the one in focus.
    if shell.typing.is_none()
        && matches!(shell.screen, Screen::CardPick | Screen::EmblemPick)
        && keys.just_pressed(KeyCode::KeyR)
    {
        let kind = look_kind(shell.screen);
        let list = cards::shown(kind, shell.look_cat);
        let pages = cards::pages(kind, list.len());
        if let Some(name) = cards::page_items(kind, &list, shell.page.min(pages.saturating_sub(1))).get(shell.focus).cloned() {
            shell.typing = Some(cards::title(&name));
            shell.typing_card = Some(name);
            typed.clear();
            return;
        }
    }
    if let Some(mut t) = shell.typing.clone() {
        use bevy::input::keyboard::Key;
        // (A card's name may be longer than his own.)
        let limit = if shell.typing_card.is_some() {
            32
        } else if shell.typing_value.is_some() {
            6
        } else {
            16
        };
        let number = shell.typing_value.is_some();
        for ev in typed.read() {
            if !ev.state.is_pressed() {
                continue;
            }
            match &ev.logical_key {
                Key::Enter => {
                    let name = t.trim().to_owned();
                    if let Some((tab, k)) = shell.typing_value.take() {
                        // (Not a number: left as it was.)
                        if let Some(v) = options::parse_scale(&name) {
                            options::queue_typed(tab, k, v);
                            sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                        }
                    } else if let Some(card) = shell.typing_card.take() {
                        // (Empty: the card's own name back.)
                        cards::set_custom_title(&card, &name);
                        sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                    } else if !name.is_empty() {
                        options::set_name(name);
                        sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                    }
                    shell.typing = None;
                    return;
                }
                Key::Escape => {
                    shell.typing = None;
                    shell.typing_card = None;
                    shell.typing_value = None;
                    sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
                    return;
                }
                Key::Backspace => {
                    t.pop();
                }
                Key::Character(s) => {
                    for ch in s.chars() {
                        let ok = if number { ch.is_ascii_digit() || ".,".contains(ch) } else { ch.is_ascii_alphanumeric() || " _-.[]'!&".contains(ch) };
                        if t.chars().count() < limit && ok {
                            t.push(ch);
                        }
                    }
                }
                Key::Space if !number && t.chars().count() < limit => t.push(' '),
                _ => {}
            }
        }
        if shell.typing.as_ref() != Some(&t) {
            shell.typing = Some(t);
        }
        return;
    }
    typed.clear();
    // The menu sounds: a move, a new tab or page, a new screen.
    let state = (shell.screen, shell.focus, shell.tab, shell.page);
    let sound_for = |old: (Screen, usize, Tab, usize), new: (Screen, usize, Tab, usize)| -> Option<&'static str> {
        if old.0 != new.0 {
            None
        } else if old.2 != new.2 || old.3 != new.3 {
            Some("uin_main_nav")
        } else if old.1 != new.1 {
            Some("uin_navigation_click")
        } else {
            None
        }
    };
    if let Some(old) = *heard
        && let Some(alias) = sound_for(old, state)
    {
        sounds.write(frame::UiPlaySound { alias: alias.to_owned() });
    }
    *heard = Some(state);
    if hud_keys.as_deref().is_some_and(|k| k.console_open) {
        return;
    }
    let Some(c) = bridge::catalog() else { return };
    let now = time.elapsed_secs_f64();
    shell.now = now;
    classes::want(matches!(shell.screen, Screen::Classes | Screen::ClassEdit | Screen::ItemPick));
    if shell.note.as_ref().is_some_and(|(_, until)| now > *until) {
        shell.note = None;
    }
    // Keep the lobby's map and mode ones the catalog has.
    if !c.maps.is_empty() && c.mp_map(&shell.map).is_none() {
        shell.map = c.maps[0].ui_name.clone();
    }
    if !c.modes.is_empty() && c.mode(&shell.mode).is_none() {
        shell.mode = c.modes[0].key.clone();
    }
    let rows = rows(&shell, &c);
    let n = rows.len();
    if n > 0 && shell.focus >= n {
        shell.focus = n - 1;
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    // The left stick and the d-pad move, repeating while held.
    let stick = pads.iter().find_map(|p| {
        let x = p.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let y = p.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        let dx = if x > 0.6 { 1 } else if x < -0.6 { -1 } else { 0 };
        let dy = if y > 0.6 { -1 } else if y < -0.6 { 1 } else { 0 };
        (dx != 0 || dy != 0).then_some((dx, dy))
    });
    let mut dir = (0, 0);
    match stick {
        Some(d) if repeat.held != Some(d) => {
            repeat.held = Some(d);
            repeat.next_at = now + 0.38;
            dir = d;
        }
        Some(d) if now >= repeat.next_at => {
            repeat.next_at = now + 0.12;
            dir = d;
        }
        Some(_) => {}
        None => repeat.held = None,
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) || pad(GamepadButton::DPadUp) {
        dir.1 = -1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) || pad(GamepadButton::DPadDown) {
        dir.1 = 1;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) || pad(GamepadButton::DPadLeft) {
        dir.0 = -1;
    }
    if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) || pad(GamepadButton::DPadRight) {
        dir.0 = 1;
    }
    if dir != (0, 0) || keys.get_just_pressed().next().is_some() || pads.iter().any(|p| p.get_just_pressed().next().is_some()) {
        *mouse_leads = false;
    }
    let confirm = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::NumpadEnter) || pad(GamepadButton::South);
    // bo2mp: Esc while maps are being prepared stops that first (the bar
    // says so); the next Esc goes back as before.
    if keys.just_pressed(KeyCode::Escape) && assets::bake_all::running() && shell.ask.is_none() && shell.typing.is_none() && shell.code_entry.is_none() {
        let line = crate::game_folders::stop_preparing();
        shell.note_long(&line, now_s);
        return;
    }
    let back = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::East);
    // A yes / no question is up: it has the keys until it is answered.
    if let Some(kind) = shell.ask {
        let n = kind.buttons().len();
        if dir.1 != 0 && n > 1 {
            shell.ask_focus ^= 1;
        }
        let mut pick = confirm;
        for (hit, i) in &hits {
            if let Hit::Ask(k) = *hit
                && *i != Interaction::None
            {
                if shell.ask_focus != k {
                    shell.ask_focus = k;
                }
                if mouse.just_pressed(MouseButton::Left) {
                    pick = true;
                }
            }
        }
        if kind == Ask::Ended && (back || pick || mouse.just_pressed(MouseButton::Right)) {
            ENDED_SEEN.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if back || mouse.just_pressed(MouseButton::Right) {
            shell.ask = None;
            sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
        } else if pick {
            let yes = shell.ask_focus == 0;
            shell.ask = None;
            sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
            if yes {
                shell.answer(kind, now);
            }
        }
        return;
    }
    let prev_tab = keys.just_pressed(KeyCode::KeyQ) || pad(GamepadButton::LeftTrigger);
    let next_tab = keys.just_pressed(KeyCode::KeyE) || pad(GamepadButton::RightTrigger);
    // Tabs / pages.
    let tabbed = |s: &mut Shell, by: i32| match s.screen {
        Screen::Lobby | Screen::WzLobby => {
            let i = Tab::ALL.iter().position(|t| *t == s.tab).unwrap_or(0) as i32;
            s.tab = Tab::ALL[(i + by).rem_euclid(3) as usize];
            s.focus = 0;
        }
        Screen::MapPick => {
            let np = pages(&c).len().max(1) as i32;
            s.page = (s.page as i32 + by).rem_euclid(np) as usize;
            s.focus = 0;
        }
        Screen::Options => {
            s.page = (options::tab_of(s.page) as i32 + by).rem_euclid(options::TABS.len() as i32) as usize;
            s.focus = 0;
        }
        Screen::MatchFilter => {
            s.page = (s.page as i32 + by).rem_euclid(matchmaking::TABS.len() as i32) as usize;
            s.focus = 0;
        }
        Screen::ItemPick => {
            let np = classes::book().map_or(1, |b| cac::picks(s, &b).0.len().max(1)) as i32;
            s.page = (s.page as i32 + by).rem_euclid(np) as usize;
            s.focus = 0;
        }
        Screen::ZmMapPick => {
            let np = zombies::pages(zombies::state().bo1()).len().max(1) as i32;
            s.page = (s.page as i32 + by).rem_euclid(np) as usize;
            s.focus = 0;
        }
        Screen::CardPick | Screen::EmblemPick => {
            let kind = look_kind(s.screen);
            let np = cards::pages(kind, cards::shown(kind, s.look_cat).len()) as i32;
            s.page = (s.page as i32 + by).rem_euclid(np) as usize;
            s.focus = 0;
        }
        _ => {}
    };
    // (The calling card and emblem pickers' categories: Z / C, the
    // triggers.)
    if matches!(shell.screen, Screen::CardPick | Screen::EmblemPick) {
        let by = i32::from(keys.just_pressed(KeyCode::KeyC) || pad(GamepadButton::RightTrigger2)) - i32::from(keys.just_pressed(KeyCode::KeyZ) || pad(GamepadButton::LeftTrigger2));
        if by != 0 {
            let n = cards::category_tabs(look_kind(shell.screen)).len().max(1) as i32;
            let cat = (shell.look_cat as i32 + by).rem_euclid(n) as usize;
            shell.set_look_cat(cat);
        }
    }
    if prev_tab {
        tabbed(&mut shell, -1);
    }
    if next_tab {
        tabbed(&mut shell, 1);
    }
    // Moving.
    if n > 0 && dir != (0, 0) {
        // bo2mp: the weapon picker goes a category at a time (left / right
        // its guns, up / down the categories), as Black Ops II's.
        let weapon_move = (shell.screen == Screen::ItemPick && cac::weapon_picker(&shell))
            .then(|| classes::book().and_then(|b| cac::weapon_step(&shell, &b, dir)))
            .flatten();
        if let Some(k) = weapon_move {
            shell.focus = k;
        } else if matches!(shell.screen, Screen::MapPick | Screen::ItemPick | Screen::ZmMapPick | Screen::CardPick | Screen::EmblemPick) {
            let f = shell.focus as i32;
            let picker = matches!(shell.screen, Screen::CardPick | Screen::EmblemPick);
            let cols = if picker {
                cards::columns(look_kind(shell.screen)) as i32
            } else {
                GRID_COLS.load(std::sync::atomic::Ordering::Relaxed).max(1) as i32
            };
            // (A picker's page turns off its left or right edge, onto the
            // same row of the next page.)
            let edge = picker && dir.0 != 0 && dir.1 == 0 && ((dir.0 < 0 && f % cols == 0) || (dir.0 > 0 && (f % cols == cols - 1 || f + 1 >= n as i32)));
            let mut t = f + dir.0 + dir.1 * cols;
            if edge {
                let row = f / cols;
                tabbed(&mut shell, dir.0);
                let m = rows_len(&shell, &c) as i32;
                t = (row * cols + if dir.0 > 0 { 0 } else { cols - 1 }).min(m - 1);
                shell.focus = t.max(0) as usize;
            } else {
                if t < 0 || t >= n as i32 {
                    // Off the page's side: the next page.
                    if dir.0 != 0 && dir.1 == 0 {
                        tabbed(&mut shell, dir.0);
                        t = 0;
                    } else {
                        t = f;
                    }
                }
                shell.focus = t.clamp(0, n as i32 - 1) as usize;
            }
        } else {
            if dir.1 != 0 {
                shell.focus = ((shell.focus as i32 + dir.1).rem_euclid(n as i32)) as usize;
            }
            if dir.0 != 0
                && let Some(Act::Cycle(what)) = rows.get(shell.focus).map(|r| r.act)
            {
                shell.cycle(what, dir.0, &c);
            }
        }
    }
    // The mouse: the row under it is in focus; a click works it.
    let clicked = mouse.just_pressed(MouseButton::Left);
    for (hit, interaction) in &hits {
        if *interaction == Interaction::None {
            continue;
        }
        match *hit {
            Hit::Row(i) if i < n => {
                if *mouse_leads && shell.focus != i {
                    shell.focus = i;
                }
                if clicked {
                    let act = rows[i].act;
                    sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                    shell.activate(act, &c, now, &mut exit);
                    return;
                }
            }
            Hit::Tab(i) if clicked => {
                shell.tab = Tab::ALL[i.min(2)];
                shell.focus = 0;
                return;
            }
            Hit::Strip(k) if clicked => {
                shell.focus = k;
                return;
            }
            Hit::Page(i) if clicked => {
                shell.page = i;
                shell.focus = 0;
                return;
            }
            Hit::Step(i, by) if clicked => {
                if let Some(Act::Cycle(what)) = rows.get(i).map(|r| r.act) {
                    shell.focus = i;
                    shell.cycle(what, by, &c);
                }
                return;
            }
            Hit::PageStep(by) if clicked => {
                tabbed(&mut shell, by);
                return;
            }
            Hit::LookCat(i) if clicked => {
                shell.set_look_cat(i);
                return;
            }
            // (The main menu's EXIT quits.)
            Hit::Back if clicked && shell.screen == Screen::Main => {
                sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
                shell.activate(Act::Quit, &c, now, &mut exit);
                return;
            }
            Hit::Back if clicked => {
                shell.back();
                return;
            }
            _ => {}
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        shell.back();
        return;
    }
    if confirm && let Some(r) = rows.get(shell.focus) {
        let act = r.act;
        sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
        shell.activate(act, &c, now, &mut exit);
    } else if back && shell.screen != Screen::Main {
        sounds.write(frame::UiPlaySound { alias: "cac_cmn_backout".to_owned() });
        shell.back();
    }
    // (Moves made this frame - keys, the stick, the mouse over a row - are
    // heard next frame against `heard`; a new screen makes no move sound.)
}

/// The screens are up while the front end is (`covers_lui`), and the
/// front end comes back from a match to the lobby he started it from:
/// bo2mp: a Zombies game (any map that plays as Zombies, however it was
/// started) back to the Zombies lobby its START GAME is on, a Warzone match
/// (however it was joined) to the Warzone screen, as Warzone does, a
/// public match to its pre-game lobby (`pregame`), a Custom Game to its
/// lobby.
fn shell_shown(
    mut shell: ResMut<Shell>,
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    identity: Option<Res<frame::LaunchIdentity>>,
    mut was: Local<bool>,
    mut last_zm: Local<bool>,
    mut last_wz: Local<bool>,
    time: Res<Time>,
) {
    let on = enabled() && front.is_some();
    bridge::set_shown(on);
    if !on {
        // The game being played: Zombies if the shell's Zombies START began
        // it (until the map is in), then by the map.
        if *was {
            *last_zm = zombies::loading().is_some();
        }
        if let Some(zone) = identity.as_deref().map(|i| i.zone.as_str()).filter(|z| !z.is_empty()) {
            *last_zm = crate::lui_hud::is_zombies_zone(zone);
            *last_wz |= hks_t6::mapvote::is_warzone_map(zone);
        }
    }
    if on && !*was && *last_zm {
        *last_zm = false;
        shell.screen = Screen::ZmLobby;
        shell.focus = 0;
    } else if on && !*was && (*last_wz || (shell.played && shell.wz)) {
        // (A Warzone match: back to the Warzone screen, squad kept.)
        shell.wz = true;
        shell.screen = Screen::WzMenu;
        shell.focus = 0;
    } else if on && !*was && shell.played && bridge::public_start() {
        // (A public match: the matchmaking lobby and a new vote; in a room,
        // its pre-game lobby takes him from there.)
        pregame::back_from_public(&mut shell, time.elapsed_secs_f64());
    } else if on && !*was && shell.played {
        shell.screen = Screen::Lobby;
        shell.tab = Tab::Lobby;
        shell.focus = 0;
    }
    if on && !*was {
        *last_wz = false;
        let stage = if shell.played { "menu after a match" } else { "menu" };
        if shell.played {
            // The match just let go of its map; hand that memory back to Windows.
            diag::release_freed_heap();
        }
        diag::info!(Ui, "{}", diag::memory_line(stage));
    }
    *was = on;
}

pub(crate) fn window_scale(windows: &Query<&Window, With<PrimaryWindow>>) -> (Vec2, f32) {
    windows.single().map_or((Vec2::new(1920.0, 1080.0), 1.0), |w| {
        let size = Vec2::new(w.width(), w.height());
        (size, (size.y / 1080.0).max(0.3))
    })
}

/// The map picker's pictures, asked for a few a frame from the moment the
/// front end is up (they are slow to read from the games' archives).
fn prefetch_pictures(
    mut pics: ResMut<pics::ShellPics>,
    icons: Option<Res<assets::T6HudIcons>>,
    mut images: ResMut<Assets<Image>>,
    mut cursor: Local<usize>,
) {
    if !bridge::covers_lui() {
        return;
    }
    let Some(c) = bridge::catalog() else { return };
    crate::foreign_maps::warm_map_pictures();
    cards::warm();
    pics.prefetch(&mut cursor, &c, icons.as_deref(), &mut images);
}

/// bo2mp: the game-ended notice. The session leaves `partyend_reason` and
/// `ui_connection_error` in the menu dvars when his game ended under him
/// (`session::lifecycle`); the front end's menu that showed them is not
/// drawn under the shell, so the shell asks it in its own look, and OK
/// clears them as that menu's close did. BO2's own (its party ended,
/// `lui_hud_zm`) comes queued (`game_ended_notice`).
fn ended_notice(mut shell: ResMut<Shell>, mut dvars: Option<ResMut<frame::UiMenuDvars>>) {
    use std::sync::atomic::Ordering;
    if !bridge::covers_lui() {
        return;
    }
    if ENDED_SEEN.swap(false, Ordering::Relaxed) {
        if let Some(d) = dvars.as_deref_mut()
            && d.get("ui_connection_error") == Some("1")
        {
            d.set("ui_connection_error", "0");
            d.set("partyend_reason", "");
        }
        if let Ok(mut e) = ENDED.lock() {
            *e = None;
        }
    }
    if shell.ask.is_some() {
        return;
    }
    let why = dvars
        .as_deref()
        .filter(|d| d.get("ui_connection_error") == Some("1"))
        .map(|d| d.get("partyend_reason").unwrap_or_default().to_owned())
        .or_else(|| ENDED_QUEUED.lock().ok().and_then(|mut q| q.take()));
    if let Some(why) = why {
        diag::info!(Ui, "bo2mp shell: game ended notice: {why}");
        if let Ok(mut e) = ENDED.lock() {
            *e = Some(why);
        }
        shell.ask = Some(Ask::Ended);
        shell.ask_focus = 0;
    }
}

pub(crate) fn register_shell(app: &mut App) {
    // (Zombies' START writes its `map` command; the frame registers this
    // too, the shell preview does not.)
    app.add_message::<frame::UiExecCommand>();
    app.add_message::<frame::UiBindEdit>();
    app.init_resource::<Shell>()
        .init_resource::<pics::ShellPics>()
        .add_systems(Startup, theme::load_fonts)
        .add_systems(
            Update,
            (
                shell_shown,
                online::poll_online,
                p2p::tick,
                keys::sync_keys,
                shell_input,
                options::sync_settings,
                background::shell_background,
                prefetch_pictures,
                view::shell_view,
                view::turn_flipbooks,
                anim::animate,
                loading::shell_loading,
                loading::shell_loading_music,
                ingame::ingame_input,
                ingame::ingame_view,
                ingame::tag_hits,
                zombies::zombies_watch,
                zombies::zombies_exec,
                zm_record::watch_games,
                zm_board::zm_board,
            )
                .chain()
                .in_set(ClientSet::Ui),
        );
    app.add_message::<frame::MapLoadFailed>();
    app.add_systems(Update, map_failed_notice.before(ended_notice).in_set(ClientSet::Ui));
    app.add_systems(Update, ended_notice.after(shell_shown).before(shell_input).in_set(ClientSet::Ui));
    app.add_systems(Update, opt_view::options_scroll.after(shell_input).before(view::shell_view).in_set(ClientSet::Ui));
}

/// bo2mp: a map that failed to load (he is back on the front end) says why
/// in the shell's notice, a Steam Workshop map by its title.
fn map_failed_notice(mut failed: MessageReader<frame::MapLoadFailed>) {
    for fact in failed.read() {
        let text = map_failed_text(&fact.zone, &fact.error);
        diag::warn!(Ui, "bo2mp shell: map failed notice: {text}");
        game_ended_notice(&text);
    }
}

/// The words of the map-failed notice.
fn map_failed_text(zone: &str, error: &str) -> String {
    let stem = zone.rsplit(':').next().unwrap_or(zone);
    let name = match asset_transport::workshop::bo3_map(stem) {
        Some(m) if m.source == asset_transport::workshop::MapSource::Workshop => format!("{} (Steam Workshop item {})", m.title, m.id),
        Some(m) => format!("{} (BO3 usermap)", m.title),
        None => stem.to_owned(),
    };
    format!("{name} could not be loaded. {error}")
}

#[cfg(test)]
mod map_failed_notice_tests {
    #[test]
    fn notice_names_the_map_and_why() {
        // (No Workshop map by this name: the zone name.)
        assert_eq!(
            super::map_failed_text("t7:mp_not_a_workshop_map_x", "Required collision geometry is missing"),
            "mp_not_a_workshop_map_x could not be loaded. Required collision geometry is missing"
        );
    }
}

//! The music director's Bevy side: builds the track table and playlists from
//! `music.cfg`, the game tracks and the user's music folders; works out the
//! context each frame (menu, lobby, match, or silence while loading); carries
//! out the [`Core`]'s actions (prepare, start, stop); sets every music voice's
//! sink volume from its fade, its context's level, `music_volume` and
//! `master_volume`; and keeps `<artifacts>/audio_manifest.csv` current.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use asset_core::AssetNamespace;
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, futures_lite::future};
use frame::{
    AppScreen, ClientSet, GameSettings, LaunchIdentity, MapLoadApproved, MenuMusicScreen, UiPlayMusic, UiStopMusic,
};

use super::config::{
    self, Entry, GAME_TRACKS, MusicConfig, game_label, is_music_file, scan_dir, unreadable_game,
    unwired_game,
};
use super::ghosts;
use super::logic::{Action, Core, MusicContext, Playlist, TrackId};
use super::map_intro::{self, IntroTable};
use super::manifest::{self, ManifestRow};
use super::stream::{Probe, StreamedMusic, probe};
use crate::clip_store::{ClipKey, ClipStore, clip_keys_for_alias};
use crate::frontend::FrontendAudio;
use crate::pcm::PcmAudio;

/// The biggest music file read (they are held in memory while they play).
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// A voice whose sink runs dry this soon after starting never really played.
/// How long a new voice may go without its entity or sink before it counts
/// as gone.
const SINK_GRACE_S: f64 = 5.0;
const DEAD_ON_ARRIVAL_S: f64 = 1.0;
/// The manifest is rewritten at most this often.
const MANIFEST_EVERY_S: f64 = 2.0;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Source {
    Game(AssetNamespace, String),
    /// A music file read straight from a game's IWDs (`sound/<path>`):
    /// Black Ops' and World at War's Zombies menu music.
    Iwd(AssetNamespace, String),
    /// A Ghosts music alias, streamed from its sound pak (`ghosts.rs`).
    Ghosts(String),
    File(PathBuf),
    /// A clip of a Black Ops II sound bank in the install's `sound` folder
    /// (bank name, entry id, the alias that plays it): Black Ops II's
    /// Zombies menu music, which is not in the multiplayer front end's bank.
    T6Bank(String, u32, String),
}

#[derive(Clone, Debug)]
struct Track {
    source: Source,
    intended: &'static str,
    origin: String,
    format: String,
    length: Option<f64>,
    playable: Option<bool>,
    reason: String,
}

impl Track {
    fn label(&self) -> String {
        match &self.source {
            Source::Game(ns, alias) => format!("{} `{alias}`", game_label(*ns)),
            Source::Iwd(ns, path) => format!("{} `{}`", game_label(*ns), iwd_stem(path)),
            Source::Ghosts(alias) => format!("{} `{alias}`", ghosts::GAME),
            Source::T6Bank(bank, _, alias) => format!("{} `{alias}` ({bank})", game_label(AssetNamespace::T6)),
            Source::File(path) => format!(
                "`{}`",
                path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
            ),
        }
    }
}

/// An IWD sound path's file name without its extension (its alias).
fn iwd_stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}

enum Prepared {
    Pcm(Handle<PcmAudio>),
    Stream(Handle<StreamedMusic>),
}

enum Preparing {
    File {
        track: TrackId,
        task: Task<Result<(Arc<[u8]>, Probe), String>>,
    },
    Game {
        track: TrackId,
        key: ClipKey,
    },
    /// An IWD music file being read and decoded.
    Pcm {
        track: TrackId,
        task: Task<Result<PcmAudio, String>>,
    },
}

/// Each game's Zombies menu music looked for (its known track, then the
/// fallback search `config::choose_zombies_track`), off the main thread at
/// startup: Black Ops II's bank, Black Ops' and World at War's IWDs.
#[derive(Default)]
struct IwdScan {
    /// Fallback tracks chosen (a game whose own track is not there).
    found: Vec<(AssetNamespace, String)>,
    /// Black Ops II's bank files by bank name (`zmb_code_post_gfx` ->
    /// its `.sabs`), when the known entry is in it.
    t6_banks: Vec<(String, PathBuf)>,
    /// Black Ops II's known track is not in the install: the front end's
    /// bank is searched by name instead (`check_bank`).
    t6_fallback: bool,
    indexes: Vec<(AssetNamespace, Arc<asset_transport::iwd::IwdSoundIndex>)>,
    /// Games not installed (their IWD tracks cannot play).
    missing: Vec<AssetNamespace>,
    lines: Vec<String>,
}

/// A music voice (its director voice id, for inspection).
#[derive(Component, Debug)]
pub struct MusicVoice {
    pub id: u64,
}

struct LiveVoice {
    entity: Entity,
    track: TrackId,
    started: f64,
    had_sink: bool,
}

#[derive(Resource)]
pub(crate) struct MusicDirector {
    core: Core,
    cfg: MusicConfig,
    tracks: Vec<Track>,
    by_source: HashMap<Source, TrackId>,
    extras: Vec<ManifestRow>,
    clock: f64,
    voices: HashMap<u64, LiveVoice>,
    prepared: HashMap<TrackId, Prepared>,
    preparing: Option<Preparing>,
    probes: Option<Task<Vec<(TrackId, Result<Probe, String>)>>>,
    /// The Ghosts install, when there is one.
    ghosts: Option<PathBuf>,
    game_store: Option<(Arc<asset_audio::SoundCatalog>, ClipStore)>,
    bank_checked: bool,
    manifest_path: Option<PathBuf>,
    manifest_dirty: bool,
    manifest_written_at: f64,
    /// Map intro: the table, the user's `music/intro` files, the zone of the
    /// load seen last (and its `game:map` key when an approval named one) and
    /// the join being resolved.
    intro_table: IntroTable,
    intro_files: Vec<PathBuf>,
    games_root: Option<asset_transport::GamesRoot>,
    seen_request: Option<u64>,
    last_zone: String,
    last_zone_key: Option<String>,
    intro_join: Option<IntroJoin>,
    /// The zombies playlist takes its defaults (no `zombies =` line, or one
    /// listing `defaults`): the Zombies menu music found later (the bank,
    /// the IWDs) joins it.
    zombies_defaults: bool,
    iwd_scan: Option<Task<IwdScan>>,
    iwd_indexes: HashMap<AssetNamespace, Arc<asset_transport::iwd::IwdSoundIndex>>,
    /// Black Ops II bank files found by the scan (bank name -> `.sabs`).
    t6_banks: HashMap<String, PathBuf>,
    /// Black Ops II's own Zombies menu track: None while the scan runs,
    /// Some(false) when it is not installed (then the front end's bank is
    /// searched by name: `t6_alias_hits`).
    t6_zombies_known: Option<bool>,
    t6_alias_hits: Vec<String>,
    t6_fallback_done: bool,
}

/// A player joining a match, whose map's game may still be being looked up.
struct IntroJoin {
    stem: String,
    game: Option<String>,
    task: Option<Task<Option<String>>>,
}

impl MusicDirector {
    fn add_track(&mut self, source: Source, intended: &'static str) -> TrackId {
        if let Some(&id) = self.by_source.get(&source) {
            return id;
        }
        let (origin, format) = match &source {
            Source::Game(..) | Source::Ghosts(..) => (String::new(), String::new()),
            Source::T6Bank(bank, entry, _) => (format!("sound/{bank}.all.sabs #{entry:08x}"), "flac".to_owned()),
            Source::Iwd(_, path) => (
                format!("sound/{path}"),
                path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default(),
            ),
            Source::File(path) => (
                path.display().to_string(),
                path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(),
            ),
        };
        let id = self.tracks.len();
        self.tracks.push(Track {
            source: source.clone(),
            intended,
            origin,
            format,
            length: None,
            playable: None,
            reason: "not checked yet".to_owned(),
        });
        self.by_source.insert(source, id);
        id
    }

    fn mark(&mut self, track: TrackId, playable: bool, reason: impl Into<String>) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.playable = Some(playable);
            t.reason = reason.into();
            self.manifest_dirty = true;
        }
    }

    fn fail(&mut self, track: TrackId, reason: &str) {
        let label = self.tracks.get(track).map(Track::label).unwrap_or_default();
        diag::warn!(Audio, "audio: music: {label} is unplayable ({reason}); skipped");
        self.mark(track, false, reason);
        self.core.failed(track);
        self.prepared.remove(&track);
    }

    fn rows(&self) -> Vec<ManifestRow> {
        let mut rows: Vec<ManifestRow> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(id, t)| {
                let lists: Vec<&str> = MusicContext::PLAYING
                    .iter()
                    .filter(|c| self.core.playlist(**c).is_some_and(|p| p.contains(id)))
                    .map(|c| c.label())
                    .collect();
                let (game, alias, source) = match &t.source {
                    Source::Game(ns, alias) => (game_label(*ns).to_owned(), alias.clone(), t.origin.clone()),
                    Source::Iwd(ns, path) => (game_label(*ns).to_owned(), iwd_stem(path).to_owned(), t.origin.clone()),
                    Source::Ghosts(alias) => (ghosts::GAME.to_owned(), alias.clone(), t.origin.clone()),
                    Source::T6Bank(_, _, alias) => (game_label(AssetNamespace::T6).to_owned(), alias.clone(), t.origin.clone()),
                    Source::File(_) => ("user file".to_owned(), String::new(), t.origin.clone()),
                };
                ManifestRow {
                    game,
                    source,
                    alias,
                    format: t.format.clone(),
                    duration_s: t.length,
                    intended_use: t.intended.to_owned(),
                    playable: match t.playable {
                        Some(true) => "yes",
                        Some(false) => "no",
                        None => "unknown",
                    }
                    .to_owned(),
                    reason: t.reason.clone(),
                    playlists: if self.cfg.enabled { lists.join("|") } else { String::new() },
                }
            })
            .collect();
        rows.extend(self.extras.iter().cloned());
        rows
    }

    fn write_manifest(&mut self, why: &str) {
        self.manifest_dirty = false;
        self.manifest_written_at = self.clock;
        let Some(path) = self.manifest_path.clone() else {
            return;
        };
        let rows = self.rows();
        match manifest::write(&path, &rows) {
            Ok(()) => diag::info!(
                Audio,
                "audio: music: manifest {} ({why}): {}",
                path.display(),
                manifest::summary(&rows)
            ),
            Err(error) => diag::warn!(Audio, "audio: music: manifest {} not written ({error})", path.display()),
        }
    }
}

/// The context for this frame.
pub(crate) fn resolve_context(
    enabled: bool,
    screen: Option<AppScreen>,
    loading: bool,
    menu: MenuMusicScreen,
    match_music: bool,
) -> MusicContext {
    if !enabled {
        return MusicContext::Silent;
    }
    match screen {
        Some(AppScreen::InGame | AppScreen::ClassSelect) if match_music => MusicContext::Match,
        Some(AppScreen::MainMenu) if !loading => match menu {
            MenuMusicScreen::Main => MusicContext::Menu,
            MenuMusicScreen::Lobby => MusicContext::Lobby,
            // (Its own playlist only: never the menu's tracks.)
            MenuMusicScreen::Zombies => MusicContext::Zombies,
        },
        _ => MusicContext::Silent,
    }
}

fn games_folder_rows(roots: &[PathBuf]) -> Vec<ManifestRow> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut visit = |dir: &Path, rows: &mut Vec<ManifestRow>| {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if !path.is_dir() || !seen.insert(path.clone()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(game) = unreadable_game(&name) {
                rows.push(ManifestRow {
                    game: game.to_owned(),
                    source: path.display().to_string(),
                    intended_use: "other".to_owned(),
                    playable: "no".to_owned(),
                    reason: "not readable by this build (unsupported game format)".to_owned(),
                    ..ManifestRow::default()
                });
            } else if let Some(game) = unwired_game(&name) {
                rows.push(ManifestRow {
                    game: game.to_owned(),
                    source: path.display().to_string(),
                    intended_use: "other".to_owned(),
                    playable: "no".to_owned(),
                    reason: "game installed, but the music director has no music loader for it".to_owned(),
                    ..ManifestRow::default()
                });
            }
        }
    };
    for root in roots {
        visit(root, &mut rows);
        // One level down too (`<root>/<store>/<game>`).
        if let Ok(read) = std::fs::read_dir(root) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && unreadable_game(&entry.file_name().to_string_lossy()).is_none()
                    && unwired_game(&entry.file_name().to_string_lossy()).is_none()
                {
                    visit(&p, &mut rows);
                }
            }
        }
    }
    rows
}

fn build_director(identity: Option<&LaunchIdentity>) -> MusicDirector {
    let exe_dir = config::exe_dir();
    let root = config::music_root(&exe_dir);
    for sub in ["menu", "lobby", "match", "zombies", "intro"] {
        let dir = root.join(sub);
        if !dir.is_dir() {
            match std::fs::create_dir_all(&dir) {
                Ok(()) => diag::info!(Audio, "audio: music: made {} for your music", dir.display()),
                Err(error) => diag::info!(Audio, "audio: music: no {} ({error})", dir.display()),
            }
        }
    }
    let (cfg_path, exists) = config::config_path(&exe_dir, &root);
    let mut cfg = if exists {
        match std::fs::read_to_string(&cfg_path) {
            Ok(text) => {
                let mut cfg = MusicConfig::parse(&text, &exe_dir);
                cfg.source = Some(cfg_path.clone());
                cfg
            }
            Err(error) => {
                diag::warn!(Audio, "audio: music: {} not read ({error}); defaults", cfg_path.display());
                MusicConfig::default()
            }
        }
    } else {
        match std::fs::write(&cfg_path, config::TEMPLATE) {
            Ok(()) => diag::info!(Audio, "audio: music: wrote a template {}", cfg_path.display()),
            Err(error) => diag::info!(Audio, "audio: music: no {} ({error}); defaults", cfg_path.display()),
        }
        MusicConfig::default()
    };
    for warning in cfg.warnings.drain(..) {
        diag::warn!(Audio, "audio: music: music.cfg {warning}");
    }

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64);
    let mut director = MusicDirector {
        core: Core::new(cfg.timings, Default::default(), seed),
        cfg: cfg.clone(),
        tracks: Vec::new(),
        by_source: HashMap::new(),
        extras: Vec::new(),
        clock: 0.0,
        voices: HashMap::new(),
        prepared: HashMap::new(),
        preparing: None,
        probes: None,
        ghosts: ghosts::install(identity.map(|i| i.games_root.as_path())),
        game_store: None,
        bank_checked: false,
        manifest_path: identity
            .filter(|i| !i.artifacts.as_os_str().is_empty())
            .map(|i| i.artifacts.join("audio_manifest.csv")),
        manifest_dirty: true,
        manifest_written_at: f64::NEG_INFINITY,
        intro_table: IntroTable::builtin(),
        intro_files: Vec::new(),
        games_root: identity
            .map(|i| i.games_root.clone())
            .filter(|p| !p.as_os_str().is_empty())
            .map(asset_transport::GamesRoot),
        seen_request: None,
        last_zone: identity.map(|i| i.zone.clone()).unwrap_or_default(),
        last_zone_key: None,
        intro_join: None,
        zombies_defaults: cfg.playlists[3].as_ref().is_none_or(|e| e.contains(&Entry::Defaults)),
        iwd_scan: None,
        iwd_indexes: HashMap::new(),
        t6_banks: HashMap::new(),
        t6_zombies_known: None,
        t6_alias_hits: Vec::new(),
        t6_fallback_done: false,
    };
    for warning in std::mem::take(&mut director.intro_table.warnings) {
        diag::warn!(Audio, "audio: music: map_intro.cfg {warning}");
    }
    for (game, stem, entries) in &cfg.intro_maps {
        director.intro_table.set_map(game, stem, entries.clone());
    }
    director.intro_files = scan_dir(&root.join("intro")).0;

    // The defaults: the games' menu tracks, then each folder's files.
    let game: Vec<TrackId> = GAME_TRACKS
        .iter()
        .map(|(ns, alias, _)| director.add_track(Source::Game(*ns, (*alias).to_owned()), "menu"))
        .collect();
    // Ghosts' main-menu track, only when Ghosts is installed.
    let ghost: Vec<TrackId> = if director.ghosts.is_some() {
        ghosts::DEFAULT_TRACKS
            .iter()
            .map(|alias| director.add_track(Source::Ghosts((*alias).to_owned()), "menu"))
            .collect()
    } else {
        Vec::new()
    };
    let folder = |director: &mut MusicDirector, sub: &str, intended: &'static str| -> Vec<TrackId> {
        let dir = root.join(sub);
        let (music, other) = scan_dir(&dir);
        for path in other {
            director.extras.push(ManifestRow {
                game: "user file".to_owned(),
                source: path.display().to_string(),
                format: path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(),
                intended_use: intended.to_owned(),
                playable: "no".to_owned(),
                reason: "not a music file this build reads (.mp3 .ogg .wav .flac)".to_owned(),
                ..ManifestRow::default()
            });
        }
        music
            .into_iter()
            .map(|p| director.add_track(Source::File(p), intended))
            .collect()
    };
    let menu_files = folder(&mut director, "menu", "menu");
    let lobby_files = folder(&mut director, "lobby", "lobby");
    let match_files = folder(&mut director, "match", "ambience");
    let zombies_files = folder(&mut director, "zombies", "zombies");
    let menu_default: Vec<TrackId> = game.iter().chain(&ghost).chain(&menu_files).copied().collect();
    let lobby_default = if lobby_files.is_empty() { menu_default.clone() } else { lobby_files };
    // (No menu tracks in a match: only his own match music, if any.)
    let match_default = match_files;
    // Zombies: each game's Zombies main-menu music (known by name) and
    // music/zombies, never the menu's tracks; a game whose own track is not
    // installed drops out, and a fallback found by name may join once the
    // games have been searched.
    let zombies_default: Vec<TrackId> = config::ZOMBIES_MENU_TRACKS
        .iter()
        .map(|t| director.add_track(zombies_source(t), "zombies"))
        .chain(zombies_files)
        .collect();
    let defaults = [menu_default, lobby_default, match_default, zombies_default];

    let mut lists: [Vec<TrackId>; 4] = Default::default();
    for slot in 0..4 {
        let Some(entries) = cfg.playlists[slot].clone() else {
            lists[slot] = defaults[slot].clone();
            continue;
        };
        for entry in entries {
            match entry {
                Entry::Defaults => lists[slot].extend(&defaults[slot]),
                Entry::None => {}
                Entry::Game(ns, alias) => lists[slot].push(director.add_track(Source::Game(ns, alias), "custom")),
                Entry::Ghosts(alias) => lists[slot].push(director.add_track(Source::Ghosts(alias), "custom")),
                Entry::Path(path) if path.is_dir() => {
                    let (music, _) = scan_dir(&path);
                    for p in music {
                        lists[slot].push(director.add_track(Source::File(p), "custom"));
                    }
                }
                Entry::Path(path) => {
                    let id = director.add_track(Source::File(path.clone()), "custom");
                    if !path.is_file() {
                        director.mark(id, false, "file not found");
                    } else if !is_music_file(&path) {
                        director.mark(id, false, "not a music file this build reads (.mp3 .ogg .wav .flac)");
                    }
                    lists[slot].push(id);
                }
            }
        }
    }
    let shuffle = cfg.shuffle;
    let [menu, lobby, matc, zombies] = lists;
    director.core = Core::new(
        cfg.timings,
        [
            Playlist::new(menu, shuffle),
            Playlist::new(lobby, shuffle),
            Playlist::new(matc, shuffle),
            Playlist::new(zombies, shuffle),
        ],
        seed,
    );
    for id in 0..director.tracks.len() {
        if director.tracks[id].playable == Some(false) {
            director.core.failed(id);
        }
    }

    // Game folders this build cannot read, for the manifest.
    let mut roots: Vec<PathBuf> = identity
        .map(|i| i.games_root.clone())
        .filter(|p| !p.as_os_str().is_empty())
        .into_iter()
        .collect();
    if let Some(voice) = asset_audio::voice_games_root() {
        roots.push(voice.0);
    }
    director.extras.extend(games_folder_rows(&roots));

    // Probe every file (and Ghosts track) in the background: format,
    // length, playable. Ghosts' zones are read there too, once.
    let files: Vec<(TrackId, Source)> = director
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.playable.is_none())
        .filter(|(_, t)| matches!(t.source, Source::File(_) | Source::Ghosts(_)))
        .map(|(id, t)| (id, t.source.clone()))
        .collect();
    if !files.is_empty() || director.ghosts.is_some() {
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
        let ghosts_dir = director.ghosts.clone();
        director.probes = Some(pool.spawn(async move {
            if let Some(dir) = &ghosts_dir {
                ghosts::tracks(dir);
            }
            files
                .into_iter()
                .map(|(id, source)| (id, read_and_probe_source(&source, ghosts_dir.as_deref()).map(|(_, p)| p)))
                .collect()
        }));
    }

    // Black Ops' and World at War's IWDs, searched for Zombies menu music.
    if cfg.enabled {
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
        let games = director.games_root.clone();
        director.iwd_scan = Some(pool.spawn(async move { scan_zombies_iwds(games) }));
    }

    let count = |c: MusicContext| director.core.playlist(c).map_or(0, |p| p.tracks().len());
    diag::info!(
        Audio,
        "audio: music: director up — config {}, music folder {}, playlists menu {} / lobby {} / match {} / zombies {} tracks, shuffle {}, fades in {:.1} s / out {:.1} s / cross {:.1} s{}",
        cfg.source.as_ref().map_or_else(|| "defaults".to_owned(), |p| p.display().to_string()),
        root.display(),
        count(MusicContext::Menu),
        count(MusicContext::Lobby),
        count(MusicContext::Match),
        count(MusicContext::Zombies),
        if shuffle { "on" } else { "off" },
        cfg.timings.fade_in,
        cfg.timings.fade_out,
        cfg.timings.crossfade,
        if cfg.enabled { "" } else { " — DISABLED by music.cfg" },
    );
    director
}

fn read_and_probe_source(source: &Source, ghosts_dir: Option<&Path>) -> Result<(Arc<[u8]>, Probe), String> {
    match source {
        Source::File(path) => read_and_probe(path),
        Source::Ghosts(alias) => ghosts::read_and_probe(ghosts_dir, alias),
        Source::Game(..) => Err("game bank tracks are prepared from the front end's bank".to_owned()),
        Source::Iwd(..) => Err("IWD tracks are read when they play".to_owned()),
        Source::T6Bank(..) => Err("bank tracks are read when they play".to_owned()),
    }
}

/// The director's source for a game's known Zombies menu track.
fn zombies_source(t: &config::ZombiesMenuTrack) -> Source {
    match t.file {
        config::ZombiesFile::T6Bank { bank, entry } => Source::T6Bank(bank.to_owned(), entry, t.alias.to_owned()),
        config::ZombiesFile::Iwd(path) => Source::Iwd(t.ns, path.to_owned()),
    }
}

/// Black Ops II's `sound` folder: beside `zone/` of the install whose
/// Zombies front-end zone (`code_post_gfx_zm`) the games folder holds.
fn t6_sound_dir(games: &asset_transport::GamesRoot) -> Option<PathBuf> {
    let zone = asset_transport::find_zone_file_version(
        games,
        "code_post_gfx_zm",
        asset_transport::zone_version(asset_transport::ZoneGame::T6),
    )
    .ok()?;
    zone.path.parent()?.parent()?.parent().map(|install| install.join("sound")).filter(|d| d.is_dir())
}

/// Read and decode one Black Ops II bank clip.
fn read_t6_bank_music(bank: Option<PathBuf>, entry: u32) -> Result<PcmAudio, String> {
    let bank = bank.ok_or_else(|| "Black Ops II's Zombies sound bank is not installed".to_owned())?;
    let pcm = asset_audio::decode_t6_clip(&bank.to_string_lossy(), &format!("{entry:08x}"))?;
    let samples: Arc<[f32]> = pcm.samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
    PcmAudio::from_prepared(samples, pcm.channels, pcm.rate).ok_or_else(|| "decoded to nothing".to_owned())
}

/// The IWD sound index of `ns`'s game (Black Ops, World at War), opened.
fn open_iwd_index(
    ns: AssetNamespace,
    games: &asset_transport::GamesRoot,
) -> Option<Result<asset_transport::iwd::IwdSoundIndex, String>> {
    let main = match ns {
        AssetNamespace::T4 => asset_audio::waw_main(games)?,
        _ => asset_transport::NamespaceTrees::discover(games).main_for(ns)?.to_path_buf(),
    };
    Some(asset_transport::iwd::IwdSoundIndex::open(&main))
}

/// Each game's Zombies menu music: its known track looked for in its files
/// (Black Ops II's bank, Black Ops' and World at War's IWDs), the pattern
/// search only when that is not there. One log line per game.
fn scan_zombies_iwds(games: Option<asset_transport::GamesRoot>) -> IwdScan {
    let mut scan = IwdScan::default();
    let Some(games) = games.or_else(|| asset_transport::games_root_from_env().ok()) else {
        scan.lines.push("no games folder: no game's Zombies menu music searched".to_owned());
        scan.missing = vec![AssetNamespace::T6, AssetNamespace::T5, AssetNamespace::T4];
        return scan;
    };
    for track in &config::ZOMBIES_MENU_TRACKS {
        let ns = track.ns;
        let label = game_label(ns);
        match track.file {
            config::ZombiesFile::T6Bank { bank, entry } => {
                let Some(dir) = t6_sound_dir(&games) else {
                    scan.lines.push(format!("{label}: not installed (no code_post_gfx_zm zone / sound folder) — none"));
                    scan.missing.push(ns);
                    continue;
                };
                let (index, _) = asset_audio::T6BankIndex::open(&dir, &[bank]);
                match index.get(entry) {
                    Some((file, e)) => {
                        scan.lines.push(format!(
                            "{label}: `{}` ({bank}, entry {entry:08x}, {:.0} s{}) — {}",
                            track.alias,
                            f64::from(e.frame_count) / f64::from(e.rate().max(1)),
                            if e.looping { " loop" } else { "" },
                            track.why
                        ));
                        scan.t6_banks.push((bank.to_owned(), file.path.clone()));
                    }
                    None => {
                        scan.lines.push(format!(
                            "{label}: `{}` not in {}/{bank}.all.sabs — the front end's bank is searched by name instead",
                            track.alias,
                            dir.display()
                        ));
                        scan.missing.push(ns);
                        scan.t6_fallback = true;
                    }
                }
            }
            config::ZombiesFile::Iwd(path) => match open_iwd_index(ns, &games) {
                None => {
                    scan.lines.push(format!("{label}: not installed — no Zombies menu music from it"));
                    scan.missing.push(ns);
                }
                Some(Err(error)) => {
                    scan.lines.push(format!("{label}: IWDs not read ({error}) — none"));
                    scan.missing.push(ns);
                }
                Some(Ok(index)) => {
                    let known = index.sound_paths().any(|p| p.eq_ignore_ascii_case(path));
                    let hits: Vec<String> = if known {
                        Vec::new()
                    } else {
                        index.sound_paths().map(str::to_owned).collect()
                    };
                    match config::choose_zombies_track(known, &hits) {
                        config::ZombiesChoice::Known => {
                            scan.lines.push(format!("{label}: `{}` ({path}) — {}", track.alias, track.why));
                        }
                        config::ZombiesChoice::Fallback(found) => {
                            scan.lines.push(format!(
                                "{label}: `{}` ({path}) not in its IWDs; `{}` instead (found by name: Zombies menu music, no stinger)",
                                track.alias,
                                iwd_stem(&found)
                            ));
                            scan.found.push((ns, found));
                        }
                        config::ZombiesChoice::Nothing => scan.lines.push(format!(
                            "{label}: `{}` ({path}) not in its IWDs, and no other Zombies menu music there — none",
                            track.alias
                        )),
                    }
                    scan.indexes.push((ns, Arc::new(index)));
                }
            },
        }
    }
    scan
}

/// The IWD search is done: what it found joins the zombies playlist (when it
/// takes its defaults); IWD tracks of games not installed, or not in their
/// IWDs, are out.
fn finish_iwd_scan(director: &mut MusicDirector, scan: IwdScan) {
    for line in &scan.lines {
        diag::info!(Audio, "audio: music: zombies: {line}");
    }
    for (ns, index) in scan.indexes {
        director.iwd_indexes.insert(ns, index);
    }
    director.t6_banks.extend(scan.t6_banks);
    director.t6_zombies_known = Some(!scan.t6_fallback && !scan.missing.contains(&AssetNamespace::T6));
    for id in 0..director.tracks.len() {
        if let Source::T6Bank(bank, ..) = director.tracks[id].source.clone()
            && director.tracks[id].playable.is_none()
        {
            if director.t6_banks.contains_key(&bank) {
                director.tracks[id].reason = "found in Black Ops II's sound bank; not decoded yet".to_owned();
                director.manifest_dirty = true;
            } else {
                director.fail(id, "not in Black Ops II's sound banks (or Black Ops II not installed)");
            }
        }
    }
    if scan.t6_fallback {
        t6_zombies_fallback(director);
    }
    for (ns, path) in scan.found {
        let id = director.add_track(Source::Iwd(ns, path), "zombies");
        if director.zombies_defaults && director.core.add_to_playlist(MusicContext::Zombies, id) {
            diag::info!(Audio, "audio: music: zombies: {} joins the zombies playlist", director.tracks[id].label());
        }
        director.manifest_dirty = true;
    }
    for id in 0..director.tracks.len() {
        let Source::Iwd(ns, path) = director.tracks[id].source.clone() else {
            continue;
        };
        if director.tracks[id].playable.is_some() {
            continue;
        }
        if scan.missing.contains(&ns) {
            director.fail(id, &format!("{} is not installed", game_label(ns)));
        } else if director
            .iwd_indexes
            .get(&ns)
            .is_some_and(|i| !i.sound_paths().any(|p| p.eq_ignore_ascii_case(&path)))
        {
            director.fail(id, &format!("not in {}'s IWDs", game_label(ns)));
        } else {
            director.tracks[id].reason = "found in the game's IWDs; not decoded yet".to_owned();
            director.manifest_dirty = true;
        }
    }
}

/// The zombies playlist's tracks and what became of each, for the "nothing
/// to play" warning.
fn zombies_report(director: &MusicDirector) -> String {
    let Some(list) = director.core.playlist(MusicContext::Zombies) else {
        return String::new();
    };
    if list.tracks().is_empty() {
        return "none".to_owned();
    }
    list.tracks()
        .iter()
        .filter_map(|&id| director.tracks.get(id))
        .map(|t| format!("{} ({})", t.label(), t.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The manifest rows for Ghosts' music that is in no playlist.
fn ghosts_extras(director: &MusicDirector) -> Vec<ManifestRow> {
    let Some(dir) = director.ghosts.as_deref() else {
        return Vec::new();
    };
    let all = ghosts::tracks(dir);
    let Ok(list) = all.as_ref() else {
        return Vec::new();
    };
    list.iter()
        .filter(|t| !director.by_source.contains_key(&Source::Ghosts(t.alias.clone())))
        .map(|t| ManifestRow {
            game: ghosts::GAME.to_owned(),
            source: ghosts::origin(t),
            alias: t.alias.clone(),
            format: "streamed flac".to_owned(),
            duration_s: Some(t.seconds()),
            intended_use: if t.alias.contains("mainmenu") { "menu" } else { "other" }.to_owned(),
            playable: "unknown".to_owned(),
            reason: format!("in Ghosts' menu zones, not in a playlist (add `ghosts:{}` to music.cfg)", t.alias),
            ..ManifestRow::default()
        })
        .collect()
}

fn read_and_probe(path: &Path) -> Result<(Arc<[u8]>, Probe), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("not readable ({e})"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("too big ({} MB)", meta.len() / (1024 * 1024)));
    }
    let bytes: Arc<[u8]> = std::fs::read(path).map_err(|e| format!("not readable ({e})"))?.into();
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    let probed = probe(Arc::clone(&bytes), ext.as_deref())?;
    Ok((bytes, probed))
}

/// The front end's bank was resident: check every game track against it and
/// list the other music it holds.
fn check_bank(director: &mut MusicDirector, frontend: &FrontendAudio) {
    director.bank_checked = true;
    let bank = &frontend.bank;
    for id in 0..director.tracks.len() {
        let Source::Game(ns, alias) = director.tracks[id].source.clone() else {
            continue;
        };
        let keys = clip_keys_for_alias(bank, ns, &alias);
        let zone = bank
            .index_in(ns, &alias)
            .map(|i| bank.zone_of_alias(i).as_str().to_owned())
            .unwrap_or_default();
        let Some(key) = keys.into_iter().next() else {
            let reason = if bank.index_in(ns, &alias).is_some() {
                "alias has no sound file"
            } else {
                "not installed (alias not in the front end's sound bank)"
            };
            diag::info!(Audio, "audio: music: {} — {reason}; left out", director.tracks[id].label());
            director.mark(id, false, reason);
            director.core.failed(id);
            continue;
        };
        let (origin, format) = describe_key(bank, &key);
        let t = &mut director.tracks[id];
        t.origin = if zone.is_empty() { origin } else { format!("{zone}: {origin}") };
        t.format = format;
        t.reason = "found in the front end's sound bank; not decoded yet".to_owned();
        director.manifest_dirty = true;
    }
    // Every other music alias the bank holds, for the manifest.
    for (i, sound) in bank.sounds.iter().enumerate() {
        let lower = sound.name.to_ascii_lowercase();
        if !(lower.starts_with("mus_") || lower.starts_with("music_")) {
            continue;
        }
        let ns = bank.namespace_of_alias(i);
        if director.by_source.contains_key(&Source::Game(ns, sound.name.clone())) {
            continue;
        }
        // Zombies menu music found by name: only a fallback for Black Ops
        // II's own track (`t6_zombies_fallback`).
        if ns == AssetNamespace::T6
            && config::is_zombies_menu_alias(&sound.name)
            && !clip_keys_for_alias(bank, ns, &sound.name).is_empty()
        {
            director.t6_alias_hits.push(sound.name.clone());
            continue;
        }
        let intended = if lower.contains("frontend") || lower.contains("mainmenu") || lower.contains("menu") {
            "menu"
        } else if lower.contains("lobby") {
            "lobby"
        } else if lower.contains("amb") {
            "ambience"
        } else {
            "other"
        };
        let key = clip_keys_for_alias(bank, ns, &sound.name).into_iter().next();
        let (origin, format) = key.as_ref().map(|k| describe_key(bank, k)).unwrap_or_default();
        director.extras.push(ManifestRow {
            game: game_label(ns).to_owned(),
            source: format!("{}: {origin}", sound.zone.as_str()),
            alias: sound.name.clone(),
            format,
            intended_use: intended.to_owned(),
            playable: if key.is_some() { "unknown" } else { "no" }.to_owned(),
            reason: if key.is_some() {
                format!("in the front end's bank, not in a playlist (add `{}:{}` to music.cfg)", ns.as_str(), sound.name)
            } else {
                "alias has no sound file".to_owned()
            },
            ..ManifestRow::default()
        });
    }
    director.manifest_dirty = true;
    t6_zombies_fallback(director);
}

/// Black Ops II's own Zombies menu track is not installed: the first Zombies
/// menu alias found by name in the front end's bank joins instead (once both
/// the scan and the bank check are done).
fn t6_zombies_fallback(director: &mut MusicDirector) {
    if director.t6_fallback_done || director.t6_zombies_known != Some(false) || !director.bank_checked {
        return;
    }
    director.t6_fallback_done = true;
    match config::choose_zombies_track(false, &director.t6_alias_hits) {
        config::ZombiesChoice::Fallback(alias) => {
            let id = director.add_track(Source::Game(AssetNamespace::T6, alias), "zombies");
            diag::info!(
                Audio,
                "audio: music: zombies: Black Ops II: {} instead (found by name in the front end's bank)",
                director.tracks[id].label()
            );
            if director.zombies_defaults && director.core.add_to_playlist(MusicContext::Zombies, id) {
                director.manifest_dirty = true;
            }
        }
        _ => diag::info!(Audio, "audio: music: zombies: Black Ops II: no Zombies menu alias in the front end's bank either — none"),
    }
}

fn describe_key(bank: &asset_audio::SoundCatalog, key: &ClipKey) -> (String, String) {
    match key {
        ClipKey::Loaded(idx) => {
            let name = bank.loaded.get(*idx).map(|l| l.name.clone()).unwrap_or_default();
            let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
            (format!("loaded {name}"), format!("loaded {}", ext.unwrap_or_else(|| "pcm".into())))
        }
        ClipKey::Streamed { dir, name, .. } => {
            let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
            (format!("streamed {dir}/{name}"), format!("streamed {}", ext.unwrap_or_default()))
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct ContextInputs<'w> {
    screen: Option<Res<'w, AppScreen>>,
    menu: Option<Res<'w, MenuMusicScreen>>,
    settings: Option<Res<'w, GameSettings>>,
    request: Option<Res<'w, assets::MatchLoadRequest>>,
    accepted: Option<Res<'w, assets::MatchLoadAccepted>>,
    busy: Option<Res<'w, assets::MatchLoadBusy>>,
    identity: Option<Res<'w, LaunchIdentity>>,
}

#[derive(SystemParam)]
pub(crate) struct MusicAssets<'w> {
    pcm: ResMut<'w, Assets<PcmAudio>>,
    streams: ResMut<'w, Assets<StreamedMusic>>,
}

pub(crate) fn init_director(
    director: Option<Res<MusicDirector>>,
    identity: Option<Res<LaunchIdentity>>,
    mut commands: Commands,
) {
    if director.is_some() {
        return;
    }
    let mut built = build_director(identity.as_deref());
    built.write_manifest("startup");
    commands.insert_resource(built);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_director(
    mut director: Option<ResMut<MusicDirector>>,
    time: Res<Time<Real>>,
    inputs: ContextInputs,
    frontend: Option<Res<FrontendAudio>>,
    waits: Option<Res<crate::frontend::MenuMusicWaits>>,
    mut assets: MusicAssets,
    mut play_bed: MessageReader<UiPlayMusic>,
    mut stop_bed: MessageReader<UiStopMusic>,
    mut approved: MessageReader<MapLoadApproved>,
    mut sinks: Query<Option<&mut AudioSink>, With<MusicVoice>>,
    mut commands: Commands,
) {
    let Some(director) = director.as_deref_mut() else {
        play_bed.clear();
        stop_bed.clear();
        approved.clear();
        return;
    };
    director.clock += time.delta_secs_f64();
    let now = director.clock;

    // (The menu's guest music still being read: nothing is checked against,
    // or started from, the bank without it.)
    let waits = waits.is_some();
    if !director.bank_checked
        && !waits
        && let Some(frontend) = frontend.as_deref()
    {
        check_bank(director, frontend);
    }

    // A script menu's music bed joins the menu playlist instead of playing
    // over the director; its stop is the director's to decide.
    stop_bed.clear();
    for bed in play_bed.read() {
        if bed.alias.is_empty() {
            continue;
        }
        let ns = frontend
            .as_deref()
            .and_then(|f| {
                [AssetNamespace::Iw4, AssetNamespace::T6, AssetNamespace::T5, AssetNamespace::Iw5]
                    .into_iter()
                    .find(|ns| f.bank.index_in(*ns, &bed.alias).is_some())
            })
            .unwrap_or(AssetNamespace::Iw4);
        let id = director.add_track(Source::Game(ns, bed.alias.clone()), "menu");
        if director.core.add_to_playlist(MusicContext::Menu, id) {
            director.manifest_dirty = true;
            diag::info!(Audio, "audio: music: menu bed `{}` joins the menu playlist", bed.alias);
        }
    }

    // The IWD search for Zombies menu music.
    if let Some(task) = director.iwd_scan.as_mut()
        && let Some(scan) = future::block_on(future::poll_once(task))
    {
        director.iwd_scan = None;
        finish_iwd_scan(director, scan);
    }

    // The file probes from startup.
    if let Some(task) = director.probes.as_mut()
        && let Some(results) = future::block_on(future::poll_once(task))
    {
        director.probes = None;
        let extras = ghosts_extras(director);
        director.extras.extend(extras);
        for (id, result) in results {
            if let (Source::Ghosts(alias), Some(dir)) = (director.tracks[id].source.clone(), director.ghosts.clone())
                && let Ok(track) = ghosts::find_track(&dir, &alias)
            {
                director.tracks[id].origin = ghosts::origin(&track);
            }
            match result {
                Ok(p) => {
                    let t = &mut director.tracks[id];
                    t.format = format!("{} ({} ch, {} Hz)", p.codec, p.channels, p.rate);
                    t.length = p.length;
                    diag::info!(
                        Audio,
                        "audio: music: track {} — {}, {}",
                        t.label(),
                        t.format,
                        p.length.map_or_else(|| "length unknown".to_owned(), |l| format!("{l:.1} s"))
                    );
                    director.mark(id, true, "decodes");
                }
                Err(reason) => director.fail(id, &reason),
            }
        }
    }

    // The map being loaded, for the intro: the approval names `game:map`.
    for event in approved.read() {
        director.last_zone_key = Some(event.zone.clone());
    }
    if let Some((id, zone)) = inputs
        .request
        .as_deref()
        .map(|r| (r.request_id, r.zone.as_str()))
        .or_else(|| inputs.accepted.as_deref().map(|a| (a.request_id, a.zone.as_str())))
        && director.seen_request != Some(id)
    {
        director.seen_request = Some(id);
        director.last_zone = zone.to_owned();
    }

    // The context.
    let settings = inputs.settings.as_deref().cloned().unwrap_or_default();
    let loading = inputs.request.is_some()
        || inputs.accepted.is_some()
        || inputs.busy.as_deref().is_some_and(|b| b.0);
    let ambient = settings.match_music && director.cfg.levels[2] > 0.0;
    let intro_on =
        settings.map_intro_music && director.cfg.intro_level > 0.0 && director.cfg.intro_seconds > 0.0;
    let context = resolve_context(
        director.cfg.enabled,
        inputs.screen.as_deref().copied(),
        loading,
        inputs.menu.as_deref().copied().unwrap_or_default(),
        ambient || intro_on,
    );
    director.core.set_match_ambient(ambient);
    let was = director.core.context();
    if context != was {
        diag::info!(Audio, "audio: music: context {} -> {}", was.label(), context.label());
        director.core.set_context(context, now);
        if context == MusicContext::Match && intro_on {
            begin_intro_join(director, inputs.identity.as_deref());
        }
    }
    if context != MusicContext::Match {
        director.intro_join = None;
    }
    poll_intro_join(director, frontend.is_some());

    poll_preparing(director, &mut assets);

    let actions = if waits { Vec::new() } else { director.core.tick(now) };
    for action in actions {
        match action {
            Action::Prepare { track, context } => start_preparing(director, frontend.as_deref(), track, context),
            Action::Start { voice, track, context, fade } => {
                let Some(prepared) = director.prepared.remove(&track) else {
                    director.fail(track, "prepared clip went missing");
                    continue;
                };
                let settings = PlaybackSettings::ONCE.with_volume(Volume::Linear(0.0));
                let entity = match prepared {
                    Prepared::Pcm(handle) => commands.spawn((AudioPlayer(handle), settings, MusicVoice { id: voice })).id(),
                    Prepared::Stream(handle) => {
                        commands.spawn((AudioPlayer(handle), settings, MusicVoice { id: voice })).id()
                    }
                };
                director.voices.insert(voice, LiveVoice { entity, track, started: now, had_sink: false });
                let t = &director.tracks[track];
                diag::info!(
                    Audio,
                    "audio: music: {} plays {} ({}, fade-in {fade:.1} s)",
                    context.label(),
                    t.label(),
                    t.length.map_or_else(|| "length unknown".to_owned(), |l| format!("{l:.0} s")),
                );
            }
            Action::Stop { voice, track } => {
                if let Some(live) = director.voices.remove(&voice) {
                    commands.entity(live.entity).try_despawn();
                }
                diag::info!(Audio, "audio: music: {} faded out", director.tracks[track].label());
            }
            Action::TimedOut { track } => {
                director.fail(track, "did not prepare in time");
                director.preparing = None;
            }
            Action::IntroUnavailable => diag::info!(
                Audio,
                "audio: music: no map intro track could play (none installed or all failed) — the match goes without"
            ),
            // (Zombies never falls back to the menu's tracks: silence, and
            // what was looked for.)
            Action::Empty { context: MusicContext::Zombies } => diag::warn!(
                Audio,
                "audio: music: no Zombies menu music to play — silent (the menu's tracks are not used in Zombies){}. Looked for: {}. Zombies playlist: {}",
                if director.iwd_scan.is_some() || !director.bank_checked { "; still searching" } else { "" },
                config::ZOMBIES_WANTED,
                zombies_report(director)
            ),
            Action::Empty { context } => diag::info!(
                Audio,
                "audio: music: the {} playlist has nothing playable — silent",
                context.label()
            ),
        }
    }

    // Volumes; voices whose sink ran dry.
    let master = settings.master_volume.clamp(0.0, 1.0);
    let music = settings.music_volume.clamp(0.0, 1.0);
    let mut ended = Vec::new();
    for (&id, live) in &mut director.voices {
        let Ok(sink) = sinks.get_mut(live.entity) else {
            // (Spawned this frame: its entity only exists once the
            // commands run, so it is not missing yet.)
            if !live.had_sink && now - live.started < SINK_GRACE_S {
                continue;
            }
            // Despawned behind the director's back (a teardown).
            ended.push((id, live.track, now - live.started));
            continue;
        };
        let Some(mut sink) = sink else {
            continue;
        };
        live.had_sink = true;
        if sink.empty() {
            ended.push((id, live.track, now - live.started));
            continue;
        }
        let (voice_ctx, intro) = director
            .core
            .voices()
            .iter()
            .find(|v| v.id == id)
            .map_or((MusicContext::Silent, false), |v| (v.context, v.is_intro()));
        // (A map intro has its own level; the match ambience's is far lower.)
        let level = if intro { director.cfg.intro_level } else { director.cfg.level(voice_ctx) };
        // In-match music (the match playlist and the map intro) sits at its
        // own base gain, far under the menus'.
        let base = if intro || voice_ctx == MusicContext::Match {
            crate::bus_gain::MATCH_MUSIC_GAIN
        } else {
            crate::bus_gain::MENU_MUSIC_GAIN
        };
        let level = level * base;
        let gain = director.core.gain(id, now) * level * music * master;
        if (sink.volume().to_linear() - gain).abs() > 1e-4 {
            sink.set_volume(Volume::Linear(gain));
        }
    }
    for (id, track, played) in ended {
        if let Some(live) = director.voices.remove(&id) {
            commands.entity(live.entity).try_despawn();
        }
        director.core.voice_ended(id);
        if played < DEAD_ON_ARRIVAL_S {
            director.fail(track, "stopped as soon as it started");
        } else {
            diag::info!(Audio, "audio: music: {} ended ({played:.0} s)", director.tracks[track].label());
        }
    }

    if director.manifest_dirty && now - director.manifest_written_at >= MANIFEST_EVERY_S {
        director.write_manifest("update");
    }
}

/// A player has joined a match: find the map's game (its zone key, or a look
/// at the game folders for a bare zone name) off the main thread.
fn begin_intro_join(director: &mut MusicDirector, identity: Option<&LaunchIdentity>) {
    let plain = if director.last_zone.is_empty() {
        identity.map(|i| i.zone.clone()).unwrap_or_default()
    } else {
        director.last_zone.clone()
    };
    let (_, stem) = map_intro::split_zone(&plain);
    // The approval's `game:map` key, when it is for this map.
    let key = director
        .last_zone_key
        .clone()
        .filter(|k| stem.is_empty() || map_intro::split_zone(k).1 == stem);
    let (game, stem) = match key {
        Some(key) => map_intro::split_zone(&key),
        None => map_intro::split_zone(&plain),
    };
    if stem.is_empty() {
        diag::info!(Audio, "audio: music: map intro — the match's map is not known; none");
        return;
    }
    if map_intro::is_zombies(&stem) {
        diag::info!(Audio, "audio: music: map intro — `{stem}` is a Zombies map (it has its own music); none");
        return;
    }
    let task = match (&game, director.games_root.clone()) {
        (None, Some(root)) => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            let name = stem.clone();
            Some(pool.spawn(async move { game_of_zone(&root, &name) }))
        }
        _ => None,
    };
    director.intro_join = Some(IntroJoin { stem, game, task });
}

/// The zone prefix of the game whose `zone` file the loader would open.
fn game_of_zone(root: &asset_transport::GamesRoot, zone: &str) -> Option<String> {
    let found = asset_transport::find_zone_file(root, zone).ok()?;
    if let Some(game) = asset_transport::legacy_game_for_path(&found.path) {
        return Some(game.to_owned());
    }
    Some(
        match asset_transport::zone_game_for_path(&found.path)? {
            asset_transport::ZoneGame::T6 => "t6",
            asset_transport::ZoneGame::T5 => "t5",
            asset_transport::ZoneGame::Iw4 => "iw4",
            asset_transport::ZoneGame::Iw5 => "iw5",
        }
        .to_owned(),
    )
}

fn poll_intro_join(director: &mut MusicDirector, frontend_present: bool) {
    let Some(join) = director.intro_join.as_mut() else {
        return;
    };
    if let Some(task) = join.task.as_mut() {
        let Some(game) = future::block_on(future::poll_once(task)) else {
            return;
        };
        join.task = None;
        join.game = game;
    }
    let Some(join) = director.intro_join.take() else {
        return;
    };
    plan_intro(director, join.game.as_deref(), &join.stem, frontend_present);
}

/// Choose the map's intro: the user's own file named after the map, else the
/// table's tracks (the map's own pick first, the others as fallbacks).
fn plan_intro(director: &mut MusicDirector, game: Option<&str>, stem: &str, frontend_present: bool) {
    let name = match game {
        Some(game) => format!("{game}:{stem}"),
        None => stem.to_owned(),
    };
    let mut sources: Vec<Source> = Vec::new();
    for file in &director.intro_files {
        let file_stem = file.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let by_game = game.map(|g| format!("{g}_{stem}"));
        if file_stem == stem || by_game.as_deref() == Some(file_stem.as_str()) {
            sources.push(Source::File(file.clone()));
        }
    }
    let own_files = sources.len();
    let (entries, found) = director.intro_table.entries(game, stem);
    let mut from_table: Vec<Source> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Game(ns, alias) => from_table.push(Source::Game(*ns, alias.clone())),
            Entry::Ghosts(alias) => from_table.push(Source::Ghosts(alias.clone())),
            Entry::Path(path) if path.is_dir() => {
                from_table.extend(scan_dir(path).0.into_iter().map(Source::File));
            }
            Entry::Path(path) => from_table.push(Source::File(path.clone())),
            Entry::Defaults | Entry::None => {}
        }
    }
    // What cannot be there: the games' own tracks need the front end's sound
    // bank, Ghosts' need its install.
    from_table.retain(|s| match s {
        Source::Game(..) => frontend_present,
        Source::Ghosts(..) => director.ghosts.is_some(),
        Source::Iwd(ns, _) => director.iwd_indexes.contains_key(ns),
        Source::T6Bank(bank, ..) => director.t6_banks.contains_key(bank),
        Source::File(path) => path.is_file(),
    });
    let from_table = map_intro::ordered(&from_table, &name);
    sources.extend(from_table);
    let mut seen = std::collections::HashSet::new();
    sources.retain(|s| seen.insert(s.clone()));
    let mut candidates: Vec<TrackId> = Vec::new();
    for source in sources {
        let id = director.add_track(source, "intro");
        if !director.core.is_dead(id) {
            candidates.push(id);
        }
    }
    if candidates.is_empty() {
        diag::info!(Audio, "audio: music: map intro for `{name}` — no track of its is installed; none");
        return;
    }
    let first = director.tracks[candidates[0]].label();
    let seconds = director.cfg.intro_seconds;
    if director.core.arm_intro(candidates, seconds) {
        diag::info!(
            Audio,
            "audio: music: map intro for `{name}` — {first} ({}; up to {seconds:.0} s)",
            if own_files > 0 { "your file".to_owned() } else { format!("{found:?}") }
        );
        director.manifest_dirty = true;
    }
}

fn start_preparing(director: &mut MusicDirector, frontend: Option<&FrontendAudio>, track: TrackId, context: MusicContext) {
    director.preparing = None;
    let label = director.tracks[track].label();
    diag::info!(Audio, "audio: music: {} prepares {label}", context.label());
    match director.tracks[track].source.clone() {
        Source::File(path) => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            let task = pool.spawn(async move { read_and_probe(&path) });
            director.preparing = Some(Preparing::File { track, task });
        }
        Source::Iwd(ns, path) => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            let index = director.iwd_indexes.get(&ns).cloned();
            let games = director.games_root.clone();
            let task = pool.spawn(async move { read_iwd_music(ns, &path, index, games) });
            director.preparing = Some(Preparing::Pcm { track, task });
        }
        Source::T6Bank(bank, entry, _) => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            let path = director.t6_banks.get(&bank).cloned();
            let task = pool.spawn(async move { read_t6_bank_music(path, entry) });
            director.preparing = Some(Preparing::Pcm { track, task });
        }
        Source::Ghosts(alias) => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            let dir = director.ghosts.clone();
            let task = pool.spawn(async move { ghosts::read_and_probe(dir.as_deref(), &alias) });
            director.preparing = Some(Preparing::File { track, task });
        }
        Source::Game(ns, alias) => {
            let Some(frontend) = frontend else {
                director.fail(track, "no front end sound bank in this run");
                return;
            };
            let Some(key) = clip_keys_for_alias(&frontend.bank, ns, &alias).into_iter().next() else {
                director.fail(track, "not installed (alias not in the front end's sound bank)");
                return;
            };
            let fresh = director
                .game_store
                .as_ref()
                .is_none_or(|(bank, _)| !Arc::ptr_eq(bank, &frontend.bank));
            if fresh {
                director.game_store = Some((
                    Arc::clone(&frontend.bank),
                    ClipStore::start(Arc::clone(&frontend.bank), Some(Arc::clone(&frontend.iwd))),
                ));
            }
            if let Some((_, store)) = director.game_store.as_mut() {
                store.request(key.clone());
            }
            director.preparing = Some(Preparing::Game { track, key });
        }
    }
}

fn poll_preparing(director: &mut MusicDirector, assets: &mut MusicAssets) {
    let Some(preparing) = director.preparing.as_mut() else {
        return;
    };
    match preparing {
        Preparing::File { track, task } => {
            let track = *track;
            let Some(result) = future::block_on(future::poll_once(task)) else {
                return;
            };
            director.preparing = None;
            match result {
                Ok((bytes, p)) => {
                    let ext = match &director.tracks[track].source {
                        Source::File(path) => path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()),
                        Source::Ghosts(..) => Some("flac".to_owned()),
                        Source::Game(..) | Source::Iwd(..) | Source::T6Bank(..) => None,
                    };
                    let length = p.length;
                    let handle = assets.streams.add(StreamedMusic::new(bytes, ext, &p));
                    if director.core.ready(track, length) {
                        director.prepared.insert(track, Prepared::Stream(handle));
                        let t = &mut director.tracks[track];
                        t.length = length.or(t.length);
                        director.mark(track, true, "decodes");
                    }
                }
                Err(reason) => director.fail(track, &reason),
            }
        }
        Preparing::Game { track, key } => {
            let (track, key) = (*track, key.clone());
            let Some((_, store)) = director.game_store.as_mut() else {
                director.preparing = None;
                director.fail(track, "no clip store");
                return;
            };
            let Some(result) = store.ready(&key) else {
                return;
            };
            store.forget(&key);
            director.preparing = None;
            match result {
                Ok(pcm) => pcm_ready(director, assets, track, pcm),
                Err(error) => director.fail(track, &format!("did not decode ({error:?})")),
            }
        }
        Preparing::Pcm { track, task } => {
            let track = *track;
            let Some(result) = future::block_on(future::poll_once(task)) else {
                return;
            };
            director.preparing = None;
            match result {
                Ok(pcm) => pcm_ready(director, assets, track, pcm),
                Err(reason) => director.fail(track, &reason),
            }
        }
    }
}

/// A decoded clip is ready to play.
fn pcm_ready(director: &mut MusicDirector, assets: &mut MusicAssets, track: TrackId, pcm: PcmAudio) {
    let frames = pcm.samples().len() / usize::from(pcm.channel_count().max(1));
    let length = frames as f64 / f64::from(pcm.rate().max(1));
    let format_note = format!("{} ch, {} Hz", pcm.channel_count(), pcm.rate());
    let handle = assets.pcm.add(pcm);
    let t = &mut director.tracks[track];
    t.length = Some(length);
    if !t.format.contains("Hz") {
        t.format = if t.format.is_empty() { format_note } else { format!("{} ({format_note})", t.format) };
    }
    director.mark(track, true, "decodes");
    if director.core.ready(track, Some(length)) {
        director.prepared.insert(track, Prepared::Pcm(handle));
    }
}

/// An IWD music file (Black Ops' own stream format, or a plain wave),
/// read and decoded.
fn read_iwd_music(
    ns: AssetNamespace,
    path: &str,
    index: Option<Arc<asset_transport::iwd::IwdSoundIndex>>,
    games: Option<asset_transport::GamesRoot>,
) -> Result<PcmAudio, String> {
    let index = match index {
        Some(index) => index,
        None => {
            let games = games
                .or_else(|| asset_transport::games_root_from_env().ok())
                .ok_or_else(|| "no games folder".to_owned())?;
            Arc::new(open_iwd_index(ns, &games).ok_or_else(|| format!("{} is not installed", game_label(ns)))??)
        }
    };
    let bytes = index
        .read_sound(path)
        .ok_or_else(|| format!("not in {}'s IWDs", game_label(ns)))?
        .map_err(|e| format!("not readable ({e})"))?;
    crate::pcm::decode_t5_stream(&bytes)
        .or_else(|| crate::pcm::decode_audio_bytes(&bytes))
        .ok_or_else(|| "did not decode".to_owned())
}

/// Map changes and returns need nothing here (the context does it), but the
/// app closing stops every voice at once and writes the final manifest.
pub(crate) fn shutdown_director(
    mut exit: MessageReader<AppExit>,
    director: Option<ResMut<MusicDirector>>,
    mut commands: Commands,
) {
    if exit.read().count() == 0 {
        return;
    }
    let Some(mut director) = director else {
        return;
    };
    for action in director.core.stop_all() {
        if let Action::Stop { voice, .. } = action
            && let Some(live) = director.voices.remove(&voice)
        {
            commands.entity(live.entity).try_despawn();
        }
    }
    director.preparing = None;
    director.prepared.clear();
    director.game_store = None;
    director.write_manifest("shutdown");
    diag::info!(Audio, "audio: music: shut down");
}

pub(crate) fn register(app: &mut App) {
    if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
        bevy::audio::AddAudioSource::add_audio_source::<StreamedMusic>(app);
    } else {
        app.init_asset::<StreamedMusic>();
    }
    app.init_resource::<MenuMusicScreen>().add_systems(
        Update,
        (init_director, update_director, shutdown_director)
            .chain()
            .in_set(ClientSet::Effects),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With `CODLEGACY_GHOSTS_DIR` (a Ghosts install), `CODLEGACY_MUSIC_DIR` and
    /// `CODLEGACY_MUSIC_CFG` pointing at scratch places: Ghosts' main-menu track is
    /// in the default menu playlist and its startup probe decodes it.
    #[test]
    #[ignore]
    fn ghosts_track_joins_the_menu_playlist() {
        let mut d = build_director(None);
        if d.ghosts.is_none() {
            eprintln!("no Ghosts install");
            return;
        }
        let id = *d.by_source.get(&Source::Ghosts("music_mainmenu_mp".into())).expect("Ghosts track");
        assert!(d.core.playlist(MusicContext::Menu).is_some_and(|p| p.contains(id)));
        let results = future::block_on(d.probes.take().expect("probe task"));
        let (_, probed) = results.iter().find(|(t, _)| *t == id).expect("probed");
        let p = probed.as_ref().expect("decodes");
        assert_eq!((p.codec.as_str(), p.channels, p.rate), ("flac", 2, 44_100));
        assert!((p.length.unwrap() - 131.095).abs() < 0.05);
        let extras = ghosts_extras(&d);
        assert_eq!(extras.len(), 5, "{extras:?}");
    }

    #[test]
    fn zombies_screens_resolve_to_the_zombies_context() {
        let ctx = |screen, loading, menu| resolve_context(true, screen, loading, menu, true);
        use MenuMusicScreen::{Main, Zombies};
        assert_eq!(ctx(Some(AppScreen::MainMenu), false, Zombies), MusicContext::Zombies);
        // Back on the main menu: the menu playlist.
        assert_eq!(ctx(Some(AppScreen::MainMenu), false, Main), MusicContext::Menu);
        // Loading a Zombies map, and in it: as before (silence, then the match).
        assert_eq!(ctx(Some(AppScreen::MainMenu), true, Zombies), MusicContext::Silent);
        assert_eq!(ctx(Some(AppScreen::Loading), false, Zombies), MusicContext::Silent);
        assert_eq!(ctx(Some(AppScreen::InGame), false, Zombies), MusicContext::Match);
        assert_eq!(resolve_context(false, Some(AppScreen::MainMenu), false, Zombies, true), MusicContext::Silent);
        assert_eq!(MusicContext::Zombies.slot(), Some(3));
        assert_eq!(MusicContext::Zombies.label(), "zombies");
    }

    #[test]
    fn iwd_stems() {
        assert_eq!(iwd_stem("mus/zombie/zombie_global/mus_zombie_splash_screen.wav"), "mus_zombie_splash_screen");
        assert_eq!(iwd_stem("x"), "x");
    }

    #[test]
    fn contexts() {
        let ctx = |screen, loading, menu, mm| resolve_context(true, screen, loading, menu, mm);
        use MenuMusicScreen::{Lobby, Main};
        assert_eq!(ctx(Some(AppScreen::MainMenu), false, Main, true), MusicContext::Menu);
        assert_eq!(ctx(Some(AppScreen::MainMenu), false, Lobby, true), MusicContext::Lobby);
        // A load asked for from the lobby: silence at once.
        assert_eq!(ctx(Some(AppScreen::MainMenu), true, Lobby, true), MusicContext::Silent);
        assert_eq!(ctx(Some(AppScreen::Loading), false, Main, true), MusicContext::Silent);
        assert_eq!(ctx(Some(AppScreen::InGame), true, Main, true), MusicContext::Match);
        assert_eq!(ctx(Some(AppScreen::ClassSelect), false, Lobby, true), MusicContext::Match);
        assert_eq!(ctx(Some(AppScreen::InGame), false, Main, false), MusicContext::Silent);
        assert_eq!(ctx(None, false, Main, true), MusicContext::Silent);
        assert_eq!(
            resolve_context(false, Some(AppScreen::MainMenu), false, Main, true),
            MusicContext::Silent
        );
    }
}

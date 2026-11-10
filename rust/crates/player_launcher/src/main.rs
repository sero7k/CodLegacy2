#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! The CodLegacy launcher window. All behaviour is in the library; this is layout and threads.
//!
//! Layout follows the ROTK launcher's idea (a nav bar, news on the left, account and Play on
//! the right) with CodLegacy's own look.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use codlegacy_launcher::api::{Client, Me, NewsItem, ServerRow};
use codlegacy_launcher::manifest::Manifest;
use codlegacy_launcher::settings::{self, Settings};
use codlegacy_launcher::{Error, game, selfupdate, update};
use call_of_claude::pack;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;

const BG: Color32 = Color32::from_rgb(0x0e, 0x11, 0x13);
const PANEL: Color32 = Color32::from_rgb(0x15, 0x1a, 0x1d);
const LINE: Color32 = Color32::from_rgb(0x26, 0x2d, 0x31);
const TRACER: Color32 = Color32::from_rgb(0xf2, 0xb6, 0x32);
const TRACER_INK: Color32 = Color32::from_rgb(0x1a, 0x13, 0x04);
const DIM: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xa6);
const OK: Color32 = Color32::from_rgb(0x6f, 0xcf, 0x97);
const BAD: Color32 = Color32::from_rgb(0xe5, 0x64, 0x5a);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 620.0]).with_min_inner_size([760.0, 520.0]).with_title("CodLegacy"),
        ..Default::default()
    };
    eframe::run_native("CodLegacy", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

enum Msg {
    Account(Result<Me, Error>),
    News(Result<Vec<NewsItem>, Error>),
    Servers(Result<Vec<ServerRow>, Error>),
    Build(Result<Manifest, Error>),
    Game(Result<std::path::PathBuf, Error>),
    Content(Vec<game::GameStatus>),
    Status(String),
    Progress(usize, usize),
    Done(Result<String, Error>),
    PackPlan(Vec<pack::GamePlan>),
    PackProgress(u64, u64, String),
    PackDone(Result<PackOutcome, String>),
    PackUsed(Result<game::ExistingPack, Error>, String),
    LauncherNew(Manifest),
    LauncherDone(Result<(), Error>),
}

/// What building the Call of Claude folder produced.
struct PackOutcome {
    dest: String,
    files: usize,
    copied: u64,
    failed: Vec<String>,
    bad: Vec<String>,
    adopt_error: Option<String>,
    /// (game title, its folder, bytes in that folder) for the games it is now safe to remove.
    removable: Vec<(String, std::path::PathBuf, u64)>,
}

fn gb(n: u64) -> String {
    format!("{:.1} GB", n as f64 / 1e9)
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Servers,
    Games,
    Settings,
}

struct App {
    tab: Tab,
    s: Settings,
    draft: Settings,
    show_key: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    me: Option<Result<Me, Error>>,
    news: Option<Result<Vec<NewsItem>, Error>>,
    servers: Option<Result<Vec<ServerRow>, Error>>,
    build: Option<Result<Manifest, Error>>,
    game: Option<Result<std::path::PathBuf, Error>>,
    content: Option<Vec<game::GameStatus>>,
    include: HashMap<String, bool>,
    pack_dest: String,
    pack_plan: Option<Vec<pack::GamePlan>>,
    pack_busy: bool,
    pack_prog: (u64, u64, String),
    pack_out: Option<Result<PackOutcome, String>>,
    own_pack: String,
    own_pack_note: Option<Result<String, Error>>,
    busy: bool,
    status: String,
    progress: Option<(usize, usize)>,
    notice: Option<Result<String, Error>>,
    launcher_new: Option<Manifest>,
    ctx: egui::Context,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
        v.selection.bg_fill = TRACER;
        v.hyperlink_color = TRACER;
        ctx.set_visuals(v);
        let (tx, rx) = channel();
        let s = Settings::load();
        let mut app = App {
            tab: Tab::Home,
            draft: s.clone(),
            s,
            show_key: false,
            tx,
            rx,
            me: None,
            news: None,
            servers: None,
            build: None,
            game: None,
            content: None,
            include: HashMap::new(),
            pack_dest: String::new(),
            pack_plan: None,
            pack_busy: false,
            pack_prog: (0, 0, String::new()),
            pack_out: None,
            own_pack: String::new(),
            own_pack_note: None,
            busy: false,
            status: String::new(),
            progress: None,
            notice: None,
            launcher_new: None,
            ctx: ctx.clone(),
        };
        app.pack_dest = if app.s.pack_dir.trim().is_empty() { settings::default_pack_dir().display().to_string() } else { app.s.pack_dir.clone() };
        if let Ok(exe) = std::env::current_exe() {
            selfupdate::cleanup_old(&exe);
        }
        app.refresh();
        app.check_launcher();
        app
    }

    fn spawn(&self, f: impl FnOnce(Sender<Msg>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            f(tx);
            ctx.request_repaint();
        });
    }

    /// Looks up everything the home page shows.
    fn refresh(&mut self) {
        self.me = None;
        self.news = None;
        self.servers = None;
        self.build = None;
        self.game = None;
        self.content = None;
        let s = self.s.clone();
        self.spawn(move |tx| {
            let client = match Client::new(&s) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Msg::Account(Err(e.clone())));
                    let _ = tx.send(Msg::News(Err(e.clone())));
                    let _ = tx.send(Msg::Servers(Err(e.clone())));
                    let _ = tx.send(Msg::Build(Err(e)));
                    return;
                }
            };
            let _ = tx.send(Msg::Account(s.check_key().and_then(|()| client.me())));
            let _ = tx.send(Msg::News(client.news()));
            let _ = tx.send(Msg::Servers(client.servers()));
            let _ = tx.send(Msg::Build(client.signed_manifest("manifest")));
        });
        let s = self.s.clone();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Game(game::find_bo2(&s)));
            let _ = tx.send(Msg::Content(game::content_check(&s)));
        });
    }

    /// Asks whether a newer launcher has been published (silently ignores failures).
    fn check_launcher(&mut self) {
        let s = self.s.clone();
        self.spawn(move |tx| {
            if let Ok(client) = Client::new(&s)
                && let Ok(m) = client.signed_manifest("launcher")
                && selfupdate::available(&m, selfupdate::CURRENT).is_some()
            {
                let _ = tx.send(Msg::LauncherNew(m));
            }
        });
    }

    fn pack_sources(&self) -> Vec<(String, std::path::PathBuf)> {
        let Some(list) = &self.content else { return Vec::new() };
        let dest = std::path::PathBuf::from(self.pack_dest.trim());
        let inc = self.include.clone();
        // Black Ops II is always included: the game needs it.
        game::pack_sources(list, &dest, &|k| k == "bo2" || inc.get(k).copied().unwrap_or(true))
    }

    fn preview_pack(&mut self) {
        self.pack_plan = None;
        self.pack_out = None;
        let sources = self.pack_sources();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::PackPlan(pack::plan(&sources)));
        });
    }

    fn build_pack(&mut self) {
        let sources = self.pack_sources();
        let dest = std::path::PathBuf::from(self.pack_dest.trim());
        if sources.is_empty() || self.pack_dest.trim().is_empty() {
            self.pack_out = Some(Err("Nothing to copy: Black Ops II was not found. Pick its folder in Settings.".into()));
            return;
        }
        let titles: HashMap<&'static str, String> = self.content.iter().flatten().map(|g| (g.key, g.title.clone())).collect();
        let mut s = self.s.clone();
        self.pack_busy = true;
        self.pack_out = None;
        self.pack_prog = (0, 0, String::new());
        self.spawn(move |tx| {
            let r = (|| -> Result<PackOutcome, String> {
                let plan = pack::plan(&sources);
                let mut last = std::time::Instant::now();
                let tx2 = tx.clone();
                let built = pack::build(&plan, &dest, &mut |p| {
                    if last.elapsed() >= std::time::Duration::from_millis(150) || p.done_bytes == p.total_bytes {
                        last = std::time::Instant::now();
                        let _ = tx2.send(Msg::PackProgress(p.done_bytes, p.total_bytes, format!("{}: {}", p.game, p.file)));
                    }
                })?;
                let _ = tx.send(Msg::PackProgress(0, 1, "Checking every copied file…".into()));
                let tx3 = tx.clone();
                let bad = pack::verify(&dest, &mut |d, t| {
                    let _ = tx3.send(Msg::PackProgress(d as u64, t.max(1) as u64, "Checking every copied file…".into()));
                })?;
                let mut adopt_error = None;
                if built.failed.is_empty() && bad.is_empty() {
                    match asset_transport::call_of_claude::adopt(&dest) {
                        Ok(_) => {
                            s.pack_dir = dest.display().to_string();
                            if let Err(e) = s.save() {
                                adopt_error = Some(e.to_string());
                            }
                        }
                        Err(e) => adopt_error = Some(e),
                    }
                }
                let ok = built.failed.is_empty() && bad.is_empty() && adopt_error.is_none();
                let removable = if ok {
                    plan.iter().map(|g| (titles.get(g.key.as_str()).cloned().unwrap_or_else(|| g.key.clone()), g.source.clone(), g.keep_bytes + g.left_bytes)).collect()
                } else {
                    Vec::new()
                };
                Ok(PackOutcome { dest: dest.display().to_string(), files: built.files, copied: built.copied_bytes, failed: built.failed, bad, adopt_error, removable })
            })();
            let _ = tx.send(Msg::PackDone(r));
        });
    }

    /// Uses a trimmed Call of Claude folder the player already has, in place of building one.
    fn use_own_pack(&mut self) {
        let dir = self.own_pack.trim().trim_matches('"').to_string();
        if dir.is_empty() {
            return;
        }
        self.pack_busy = true;
        self.pack_prog = (0, 1, "Checking the folder…".into());
        self.own_pack_note = None;
        self.spawn(move |tx| {
            let tx2 = tx.clone();
            let r = game::use_existing_pack(std::path::Path::new(&dir), &mut |d, t| {
                let _ = tx2.send(Msg::PackProgress(d as u64, t.max(1) as u64, "Checking every file against its manifest…".into()));
            });
            let _ = tx.send(Msg::PackUsed(r, dir));
        });
    }

    fn update_launcher(&mut self) {
        let Some(m) = self.launcher_new.clone() else { return };
        self.busy = true;
        let s = self.s.clone();
        self.spawn(move |tx| {
            let r = (|| -> Result<(), Error> {
                let client = Client::new(&s)?;
                let exe = std::env::current_exe().map_err(|e| Error::other(e.to_string()))?;
                selfupdate::install(&m, selfupdate::CURRENT, &exe, &|u, max| client.download(u, max))?;
                std::process::Command::new(&exe).spawn().map_err(|e| Error::other(format!("Updated, but could not restart: {e}. Start the launcher again.")))?;
                Ok(())
            })();
            let _ = tx.send(Msg::LauncherDone(r));
        });
    }

    fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Account(r) => self.me = Some(r),
                Msg::News(r) => self.news = Some(r),
                Msg::Servers(r) => self.servers = Some(r),
                Msg::Build(r) => self.build = Some(r),
                Msg::Game(r) => self.game = Some(r),
                Msg::Content(c) => self.content = Some(c),
                Msg::PackPlan(p) => self.pack_plan = Some(p),
                Msg::PackProgress(d, t, f) => self.pack_prog = (d, t, f),
                Msg::PackDone(r) => {
                    self.pack_busy = false;
                    if let Ok(o) = &r
                        && o.failed.is_empty()
                        && o.bad.is_empty()
                        && o.adopt_error.is_none()
                    {
                        self.s.pack_dir = o.dest.clone();
                        self.draft.pack_dir = o.dest.clone();
                        self.refresh();
                    }
                    self.pack_out = Some(r);
                }
                Msg::PackUsed(r, dir) => {
                    self.pack_busy = false;
                    self.own_pack_note = Some(r.map(|p| {
                        self.s.pack_dir = dir.clone();
                        self.draft.pack_dir = dir;
                        let _ = self.s.save();
                        self.refresh();
                        let mut t = format!("Using it. Games: {}.", p.games.join(", "));
                        if !p.checked {
                            t.push_str(" It has no manifest, so its files could not be checked.");
                        }
                        if !p.ignored.is_empty() {
                            t.push_str(&format!(" Ignored (not used by CodLegacy): {}.", p.ignored.join(", ")));
                        }
                        t
                    }));
                }
                Msg::LauncherNew(m) => self.launcher_new = Some(m),
                Msg::LauncherDone(Ok(())) => std::process::exit(0),
                Msg::LauncherDone(Err(e)) => {
                    self.busy = false;
                    self.notice = Some(Err(e));
                }
                Msg::Status(t) => self.status = t,
                Msg::Progress(d, t) => self.progress = Some((d, t)),
                Msg::Done(r) => {
                    self.busy = false;
                    self.progress = None;
                    self.status.clear();
                    self.notice = Some(r);
                    self.ctx.request_repaint();
                }
            }
        }
    }

    fn installed(&self) -> Option<String> {
        update::installed_version(&self.s.install_path())
    }

    fn needs_update(&self) -> bool {
        match &self.build {
            Some(Ok(m)) => !update::stale(m, &self.s.install_path()).is_empty(),
            _ => false,
        }
    }

    /// Update (if asked or needed), then ticket, then start.
    fn run(&mut self, play: bool) {
        self.busy = true;
        self.notice = None;
        let s = self.s.clone();
        self.spawn(move |tx| {
            let result = (|| -> Result<String, Error> {
                let client = Client::new(&s)?;
                let install = s.install_path();
                let _ = tx.send(Msg::Status("Checking for a new build…".into()));
                let build = client.signed_manifest("manifest");
                let mut updated = 0;
                match &build {
                    Ok(m) => {
                        let tx2 = tx.clone();
                        updated = update::apply(m, &install, &|u, max| client.download(u, max), &mut |d, t| {
                            let _ = tx2.send(Msg::Progress(d, t));
                            let _ = tx2.send(Msg::Status(format!("Downloading… {d} of {t} files")));
                        })?;
                    }
                    Err(e) if play && install.join(game::GAME_EXE).is_file() => {
                        // Offline updates are optional when a build is already installed.
                        let _ = tx.send(Msg::Status(format!("Skipping update: {e}")));
                    }
                    Err(e) => return Err(e.clone()),
                }
                if !play {
                    return Ok(if updated == 0 { "CodLegacy is up to date.".into() } else { format!("Updated {updated} files.") });
                }
                s.check_key()?;
                game::link_supplied_mods(&install);
                let _ = tx.send(Msg::Status("Finding Black Ops II…".into()));
                let bo2 = game::find_bo2(&s)?;
                let _ = tx.send(Msg::Status("Getting your join ticket…".into()));
                let ticket = client.join_ticket()?;
                let _ = tx.send(Msg::Status("Starting CodLegacy…".into()));
                game::start(&install, &bo2, client.base(), &ticket.ticket)?;
                Ok("CodLegacy started.".into())
            })();
            let _ = tx.send(Msg::Done(result));
        });
    }

    fn save_settings(&mut self) {
        let mut d = self.draft.clone();
        d.key = settings::clean_key(&d.key);
        d.game_dir = d.game_dir.trim().to_string();
        d.install_dir = d.install_dir.trim().to_string();
        let checked = settings::normalise_api(&d.api_url).and_then(|u| {
            d.api_url = u;
            if !d.key.is_empty() && !settings::key_is_well_formed(&d.key) {
                return Err(Error::new(codlegacy_launcher::Fault::KeyMissing, "That does not look like a launcher key (32 characters, 0-9 and a-f)."));
            }
            if !d.game_dir.is_empty() {
                game::check_bo2_folder(std::path::Path::new(&d.game_dir))?;
            }
            d.save()
        });
        match checked {
            Ok(()) => {
                self.s = d.clone();
                self.draft = d;
                self.notice = Some(Ok("Saved.".into()));
                self.refresh();
            }
            Err(e) => self.notice = Some(Err(e)),
        }
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(6).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn state_line(ui: &mut egui::Ui, label: &str, value: Result<String, String>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).color(DIM));
        match value {
            Ok(v) => ui.label(RichText::new(v).color(OK)),
            Err(v) => ui.label(RichText::new(v).color(BAD)),
        };
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        egui::Panel::top("nav").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 10))).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("COD").strong().size(20.0).color(Color32::WHITE));
                ui.label(RichText::new("LEGACY").strong().size(20.0).color(TRACER));
                ui.add_space(24.0);
                for (t, name) in [(Tab::Home, "Home"), (Tab::Servers, "Servers"), (Tab::Games, "Games"), (Tab::Settings, "Settings")] {
                    if ui.selectable_label(self.tab == t, RichText::new(name).size(15.0)).clicked() {
                        self.tab = t;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &self.me {
                    Some(Ok(me)) => {
                        ui.label(RichText::new(&me.callsign).strong().color(OK));
                    }
                    Some(Err(_)) => {
                        ui.label(RichText::new("Not signed in").color(BAD));
                    }
                    None => {
                        ui.spinner();
                    }
                });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(m) = self.launcher_new.clone() {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("A new launcher ({}) is available.", m.version)).color(TRACER));
                        if ui.add_enabled(!self.busy, egui::Button::new("Update and restart")).clicked() {
                            self.update_launcher();
                        }
                    });
                });
                ui.add_space(6.0);
            }
            if let Some(n) = self.notice.clone() {
                match n {
                    Ok(t) => ui.label(RichText::new(t).color(OK)),
                    Err(e) => ui.label(RichText::new(e.to_string()).color(BAD)),
                };
                ui.add_space(6.0);
            }
            match self.tab {
                Tab::Home => self.home(ui),
                Tab::Servers => self.servers_tab(ui),
                Tab::Games => self.games_tab(ui),
                Tab::Settings => self.settings_tab(ui),
            }
        });
    }
}

impl App {
    fn home(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            // News.
            cols[0].label(RichText::new("News").heading().color(Color32::WHITE));
            egui::ScrollArea::vertical().id_salt("news").show(&mut cols[0], |ui| match &self.news {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e.to_string()).color(BAD));
                }
                Some(Ok(items)) if items.is_empty() => {
                    ui.label(RichText::new("Nothing yet.").color(DIM));
                }
                Some(Ok(items)) => {
                    for n in items {
                        card(ui, |ui| {
                            ui.label(RichText::new(format!("{}  ·  {}", n.date, n.kind.to_uppercase())).small().color(TRACER));
                            ui.label(RichText::new(&n.title).strong().size(16.0));
                            ui.label(RichText::new(&n.summary).color(DIM));
                            ui.hyperlink_to("Read more", format!("{}/updates/{}", self.s.api_url.trim_end_matches('/'), n.slug));
                        });
                        ui.add_space(8.0);
                    }
                }
            });

            // Account, game and Play.
            let ui = &mut cols[1];
            card(ui, |ui| {
                ui.label(RichText::new("Ready check").heading().color(Color32::WHITE));
                match &self.me {
                    None => state_line(ui, "Account:", Ok("checking…".into())),
                    Some(Ok(me)) => state_line(ui, "Account:", Ok(format!("signed in as {}", me.callsign))),
                    Some(Err(e)) => state_line(ui, "Account:", Err(e.to_string())),
                }
                match &self.game {
                    None => state_line(ui, "Black Ops II:", Ok("looking…".into())),
                    Some(Ok(p)) => state_line(ui, "Black Ops II:", Ok(p.display().to_string())),
                    Some(Err(e)) => state_line(ui, "Black Ops II:", Err(e.to_string())),
                }
                let have = self.installed();
                match (&self.build, have) {
                    (None, _) => state_line(ui, "CodLegacy:", Ok("checking…".into())),
                    (Some(Ok(m)), Some(v)) if v == m.version && !self.needs_update() => state_line(ui, "CodLegacy:", Ok(format!("version {v}, up to date"))),
                    (Some(Ok(m)), _) => state_line(ui, "CodLegacy:", Err(format!("update to {} available", m.version))),
                    (Some(Err(e)), Some(v)) => state_line(ui, "CodLegacy:", Ok(format!("version {v} ({e})"))),
                    (Some(Err(e)), None) => state_line(ui, "CodLegacy:", Err(e.to_string())),
                }
            });
            ui.add_space(10.0);
            card(ui, |ui| {
                ui.label(RichText::new("Your games").heading().color(Color32::WHITE));
                match &self.content {
                    None => {
                        ui.spinner();
                    }
                    Some(list) => {
                        for g in list {
                            let (mark, color, note) = match (&g.path, g.steam) {
                                (Some(_), true) => ("✔", OK, "installed through Steam"),
                                (Some(_), false) => ("✔", OK, "found on this PC"),
                                (None, _) => ("–", DIM, "not found"),
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(mark).color(color));
                                ui.label(&g.title);
                                ui.label(RichText::new(note).small().color(DIM));
                            });
                        }
                        ui.label(RichText::new("Maps and guns from a game unlock only when its files are on this PC. Your files are never uploaded or copied.").small().color(DIM));
                    }
                }
            });
            ui.add_space(10.0);
            if self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.status);
                });
                if let Some((d, t)) = self.progress
                    && t > 0
                {
                    ui.add(egui::ProgressBar::new(d as f32 / t as f32).fill(TRACER));
                }
            } else {
                let play = egui::Button::new(RichText::new("PLAY").size(24.0).strong().color(TRACER_INK)).fill(TRACER).min_size(egui::vec2(ui.available_width(), 56.0));
                if ui.add(play).clicked() {
                    self.run(true);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.needs_update() || self.installed().is_none(), egui::Button::new("Update")).clicked() {
                        self.run(false);
                    }
                    if ui.button("Re-check").clicked() {
                        self.refresh();
                    }
                    if ui.button("Get my launcher key").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!("{}/account", self.s.api_url.trim_end_matches('/'))));
                    }
                });
            }
        });
    }

    fn servers_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Servers").heading().color(Color32::WHITE));
            if ui.button("Refresh").clicked() {
                self.servers = None;
                let s = self.s.clone();
                self.spawn(move |tx| {
                    let _ = tx.send(Msg::Servers(Client::new(&s).and_then(|c| c.servers())));
                });
            }
        });
        match &self.servers {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e.to_string()).color(BAD));
            }
            Some(Ok(rows)) if rows.is_empty() => {
                ui.label(RichText::new("No servers are online right now.").color(DIM));
            }
            Some(Ok(rows)) => {
                egui::Grid::new("servers").striped(true).num_columns(5).spacing([24.0, 8.0]).show(ui, |ui| {
                    for h in ["Server", "Region", "Mode", "Map", "Players"] {
                        ui.label(RichText::new(h).color(DIM));
                    }
                    ui.end_row();
                    for r in rows {
                        ui.label(&r.name);
                        ui.label(&r.region);
                        ui.label(&r.mode);
                        ui.label(&r.map);
                        ui.label(format!("{}/{}", r.players, r.max_players));
                        ui.end_row();
                    }
                });
            }
        }
    }

    fn games_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Games").heading().color(Color32::WHITE));
        ui.label(RichText::new("CodLegacy needs only part of each game: the multiplayer maps, guns and sounds, in English. The launcher copies just those files from your own installed games into one folder on this PC, checks every copy, and then tells you which originals you can remove. Nothing is uploaded and nothing is deleted for you.").color(DIM));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().id_salt("games").show(ui, |ui| {
            card(ui, |ui| {
                ui.label(RichText::new("Already have a trimmed game folder?").strong());
                ui.label(RichText::new("If you already have a Call of Claude folder (made from your own copies of the games), use it instead of copying again. It is checked against its manifest when it has one.").small().color(DIM));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.own_pack).desired_width(420.0).hint_text("D:\\Call of Claude"));
                    if ui.add_enabled(!self.pack_busy, egui::Button::new("Use this folder")).clicked() {
                        self.use_own_pack();
                    }
                });
                match &self.own_pack_note {
                    Some(Ok(t)) => ui.label(RichText::new(t).color(OK)),
                    Some(Err(e)) => ui.label(RichText::new(e.to_string()).color(BAD)),
                    None => ui.label(""),
                };
            });
            ui.add_space(8.0);
            card(ui, |ui| {
                let Some(list) = self.content.clone() else {
                    ui.spinner();
                    return;
                };
                for g in &list {
                    ui.horizontal(|ui| {
                        let mut on = g.key == "bo2" || self.include.get(g.key).copied().unwrap_or(true);
                        let found = g.path.is_some();
                        if ui.add_enabled(found && g.key != "bo2", egui::Checkbox::new(&mut on, "")).changed() {
                            self.include.insert(g.key.to_string(), on);
                            self.pack_plan = None;
                        }
                        ui.label(&g.title);
                        match &g.path {
                            Some(p) => ui.label(RichText::new(p.display().to_string()).small().color(DIM)),
                            None => ui.label(RichText::new("not found").small().color(DIM)),
                        };
                    });
                }
                ui.add_space(8.0);
                ui.label("Put the copy here (a drive with room; it can be tens of GB):");
                if ui.add(egui::TextEdit::singleline(&mut self.pack_dest).desired_width(420.0)).changed() {
                    self.pack_plan = None;
                }
            });
            ui.add_space(8.0);
            if let Some(plan) = &self.pack_plan {
                card(ui, |ui| {
                    let (mut keep, mut all) = (0u64, 0u64);
                    for g in plan {
                        keep += g.keep_bytes;
                        all += g.keep_bytes + g.left_bytes;
                        ui.label(format!("{}: copy {} of {}", g.key.to_uppercase(), gb(g.keep_bytes), gb(g.keep_bytes + g.left_bytes)));
                    }
                    ui.label(RichText::new(format!("In total {} is copied out of {}.", gb(keep), gb(all))).strong());
                });
                ui.add_space(8.0);
            }
            if self.pack_busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.pack_prog.2);
                });
                let (d, t, _) = &self.pack_prog;
                if *t > 0 {
                    ui.add(egui::ProgressBar::new(*d as f32 / *t as f32).fill(TRACER).show_percentage());
                }
            } else {
                ui.horizontal(|ui| {
                    if ui.button("Preview sizes").clicked() {
                        self.preview_pack();
                    }
                    if ui.add(egui::Button::new(RichText::new("Copy what CodLegacy needs").color(TRACER_INK).strong()).fill(TRACER)).clicked() {
                        self.build_pack();
                    }
                });
            }
            ui.add_space(8.0);
            match &self.pack_out {
                None => {}
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(BAD));
                }
                Some(Ok(o)) => {
                    if !o.failed.is_empty() || !o.bad.is_empty() || o.adopt_error.is_some() {
                        ui.label(RichText::new("The copy is not complete, so nothing is safe to remove yet.").strong().color(BAD));
                        for f in o.failed.iter().chain(o.bad.iter()).take(8) {
                            ui.label(RichText::new(f).small().color(BAD));
                        }
                        if let Some(e) = &o.adopt_error {
                            ui.label(RichText::new(e).small().color(BAD));
                        }
                        ui.label(RichText::new("Run it again: files already copied are kept and only the rest is redone.").small().color(DIM));
                    } else {
                        ui.label(RichText::new(format!("Done: {} files ({} newly copied), every one checked. CodLegacy now reads them from {}.", o.files, gb(o.copied), o.dest)).color(OK));
                        ui.add_space(6.0);
                        card(ui, |ui| {
                            ui.label(RichText::new("You can now remove the originals you don't want").heading().color(Color32::WHITE));
                            ui.label(RichText::new("CodLegacy no longer needs these folders. Keep any you still play on their own. For Steam games, use Steam's Uninstall: it frees the space and you keep owning the game.").color(DIM));
                            for (title, path, bytes) in &o.removable {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{title} ({} on disk)", gb(*bytes)));
                                    if ui.small_button("Show folder").clicked() {
                                        let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
                                        let _ = std::process::Command::new(opener).arg(path).spawn();
                                    }
                                });
                            }
                        });
                    }
                }
            }
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Settings").heading().color(Color32::WHITE));
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.label("Launcher key (make one on your account page after signing in with Steam)");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.draft.key).password(!self.show_key).desired_width(340.0).hint_text("32 characters"));
                ui.checkbox(&mut self.show_key, "Show");
            });
            ui.add_space(8.0);
            ui.label("Account service address");
            ui.add(egui::TextEdit::singleline(&mut self.draft.api_url).desired_width(340.0));
            ui.add_space(8.0);
            ui.label("Black Ops II folder (leave empty to find it automatically)");
            ui.add(egui::TextEdit::singleline(&mut self.draft.game_dir).desired_width(340.0));
            ui.add_space(8.0);
            ui.label("Where CodLegacy is installed (leave empty for the default)");
            ui.add(egui::TextEdit::singleline(&mut self.draft.install_dir).desired_width(340.0).hint_text(settings::default_install_dir().display().to_string()));
            ui.add_space(12.0);
            if ui.add(egui::Button::new(RichText::new("Save").color(TRACER_INK).strong()).fill(TRACER)).clicked() {
                self.save_settings();
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Your key is stored on this PC only. Never share it: anyone with it can play as you.").small().color(DIM));
    }
}

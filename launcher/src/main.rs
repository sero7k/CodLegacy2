#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Cod Legacy Launcher: a small window beside "CoD Legacy.exe" with one job,
//! starting multiplayer. The look follows the CodLegacy client's launcher
//! (nav bar, cards, one accent) in the front end's blue.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};

const GAME_EXE: &str = "CoD Legacy.exe";

const BG: Color32 = Color32::from_rgb(0x07, 0x0a, 0x10);
const PANEL: Color32 = Color32::from_rgb(0x0d, 0x12, 0x1a);
const LINE: Color32 = Color32::from_rgb(0x22, 0x2b, 0x38);
const ACCENT: Color32 = Color32::from_rgb(0x42, 0x9e, 0xf2);
const ACCENT_INK: Color32 = Color32::from_rgb(0x03, 0x0a, 0x14);
const DIM: Color32 = Color32::from_rgb(0x9a, 0xa6, 0xb6);
const OK: Color32 = Color32::from_rgb(0x6f, 0xcf, 0x97);
const BAD: Color32 = Color32::from_rgb(0xe5, 0x64, 0x5a);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 420.0])
            .with_min_inner_size([620.0, 360.0])
            .with_title("Cod Legacy Launcher"),
        ..Default::default()
    };
    eframe::run_native("Cod Legacy Launcher", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

/// The folder this launcher sits in (where the game is).
fn here() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Settings {
    /// Extra command-line arguments for the game, split on spaces.
    args: String,
    /// Close the launcher once the game has started.
    close_on_play: bool,
}

impl Settings {
    fn path() -> PathBuf {
        here().join("cod-legacy-launcher.json")
    }
    fn load() -> Settings {
        let loaded = std::fs::read_to_string(Settings::path()).ok().and_then(|t| serde_json::from_str::<Settings>(&t).ok());
        loaded.unwrap_or(Settings { args: String::new(), close_on_play: true })
    }
    fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Settings::path(), text).map_err(|e| e.to_string())
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Settings,
}

struct App {
    tab: Tab,
    s: Settings,
    draft: Settings,
    notice: Option<Result<String, String>>,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
        v.selection.bg_fill = ACCENT;
        v.hyperlink_color = ACCENT;
        ctx.set_visuals(v);
        let s = Settings::load();
        App { tab: Tab::Home, draft: s.clone(), s, notice: None }
    }

    fn game_path(&self) -> PathBuf {
        here().join(GAME_EXE)
    }

    fn play(&mut self, ctx: &egui::Context) {
        let exe = self.game_path();
        if !exe.is_file() {
            self.notice = Some(Err(format!("{GAME_EXE} is not next to the launcher.")));
            return;
        }
        let mut command = std::process::Command::new(&exe);
        command.current_dir(here());
        for arg in self.s.args.split_whitespace() {
            command.arg(arg);
        }
        match command.spawn() {
            Ok(_) => {
                self.notice = Some(Ok("Cod Legacy started.".into()));
                if self.s.close_on_play {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(e) => self.notice = Some(Err(format!("Could not start the game: {e}"))),
        }
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(6).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("nav").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 10))).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("COD").strong().size(20.0).color(Color32::WHITE));
                ui.label(RichText::new("LEGACY").strong().size(20.0).color(ACCENT));
                ui.add_space(24.0);
                for (t, name) in [(Tab::Home, "Home"), (Tab::Settings, "Settings")] {
                    if ui.selectable_label(self.tab == t, RichText::new(name).size(15.0)).clicked() {
                        self.tab = t;
                    }
                }
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(n) = self.notice.clone() {
                match n {
                    Ok(t) => ui.label(RichText::new(t).color(OK)),
                    Err(e) => ui.label(RichText::new(e).color(BAD)),
                };
                ui.add_space(6.0);
            }
            match self.tab {
                Tab::Home => self.home(ui, &ctx),
                Tab::Settings => self.settings_tab(ui),
            }
        });
    }
}

impl App {
    fn home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.label(RichText::new("Multiplayer").heading().color(Color32::WHITE));
            ui.add_space(4.0);
            ui.label(RichText::new("Find a match, or a custom game with bots on every map.").color(DIM));
            ui.add_space(14.0);
            let ready = self.game_path().is_file();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Game").color(DIM));
                if ready {
                    ui.label(RichText::new("ready").color(OK));
                } else {
                    ui.label(RichText::new(format!("{GAME_EXE} not found next to the launcher")).color(BAD));
                }
            });
            ui.add_space(14.0);
            let play = egui::Button::new(RichText::new("PLAY").strong().size(18.0).color(ACCENT_INK)).fill(ACCENT).min_size(egui::vec2(180.0, 42.0));
            if ui.add_enabled(ready, play).clicked() {
                self.play(ctx);
            }
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Settings").heading().color(Color32::WHITE));
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.label(RichText::new("Launch options").color(DIM));
            ui.add(egui::TextEdit::singleline(&mut self.draft.args).desired_width(f32::INFINITY));
            ui.add_space(8.0);
            ui.checkbox(&mut self.draft.close_on_play, "Close the launcher when the game starts");
            ui.add_space(10.0);
            if ui.button("Save").clicked() {
                match self.draft.save() {
                    Ok(()) => {
                        self.s = self.draft.clone();
                        self.notice = Some(Ok("Saved.".into()));
                    }
                    Err(e) => self.notice = Some(Err(format!("Could not save: {e}"))),
                }
            }
        });
    }
}

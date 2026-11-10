//! bo2mp shell: the menus in a match - the pause menu (Esc / Start) and
//! Choose Class (the pause menu's, and the one the server opens when he
//! spawns), in the shell's look, instead of BO2's. Choosing a class answers
//! the server as BO2's menu did (`changeclass` `custom<n>`); END GAME is
//! BO2's `popup_leavegame` `endround`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::classes::{self, POINTS};
use super::pics::ShellPics;
use super::theme::*;
use super::ghosts::{self, Look};
use super::view::{Ctx, abs, header, hint, menu_rows_window};
use super::{Act, Hit, Row, Screen, Setting, Shell, bridge, cac, window_scale};

/// What is up: 0 nothing, 1 the pause menu, 2 Choose Class, 3 Options,
/// 4 a yes / no question over the pause menu (`ASK`), 5 the class editor
/// opened from Choose Class (`EDITOR`: the front end's Create-a-Class).
static UP: AtomicU8 = AtomicU8::new(0);
/// The server asked for Choose Class / for its menus closed.
static SERVER_OPEN: AtomicBool = AtomicBool::new(false);
static SERVER_CLOSE: AtomicBool = AtomicBool::new(false);
static RESPONSES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// A menu of the shell is up in the match (it holds his keys and mouse).
pub(crate) fn open() -> bool {
    bridge::enabled() && UP.load(Ordering::Relaxed) != 0
}

fn menu_name(up: u8) -> &'static str {
    match up {
        1 => "pause menu",
        2 => "choose class",
        3 => "options",
        4 => "question",
        5 => "class editor",
        _ => "menu",
    }
}

/// What is up now: kept for the view, and said to the game's input (the
/// gameplay keys, mouse look and fire stop; the pointer is freed and shown
/// by the cursor grab) directly through `frame::set_shell_menu_open`. (It
/// used to go only through `frame::LuiMenus` -> `hud::ScriptMenus::lui_open`,
/// but the system copying it, `hud::menus::update_script_menus`, runs only
/// under MW2's HUD (`run_if(mw2_hud)`), never under Black Ops II's rules: the
/// game kept his keys, look, fire and the locked, hidden pointer.)
fn set_up(up: u8) -> bool {
    let was = UP.swap(up, Ordering::Relaxed);
    // (Held one frame more after it closes: the click or Enter that closed
    // it is not a shot or a jump.)
    let hold = bridge::enabled() && (up != 0 || was != 0);
    frame::set_shell_menu_open(hold);
    if was != up {
        match (was, up) {
            (0, _) => diag::info!(Ui, "bo2mp shell: {}: open, input captured, cursor shown", menu_name(up)),
            (_, 0) => diag::info!(Ui, "bo2mp shell: {}: closed, input back to the game, cursor locked", menu_name(was)),
            _ => diag::info!(Ui, "bo2mp shell: in-match menu: {} -> {}", menu_name(was), menu_name(up)),
        }
    }
    hold
}

/// The server opened a menu on his screen (`openmenu`): Choose Class for
/// the class menu; true when the shell shows it (BO2's is not opened).
pub(crate) fn server_menu(menu: &str) -> bool {
    if !bridge::enabled() {
        return false;
    }
    if menu.contains("changeclass") || menu.eq_ignore_ascii_case("class") {
        SERVER_OPEN.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// The server closed his menus.
pub(crate) fn server_close() {
    SERVER_CLOSE.store(true, Ordering::Relaxed);
}

/// His answers for the server, taken by the match HUD.
pub(crate) fn take_responses() -> Vec<(String, String)> {
    std::mem::take(&mut *RESPONSES.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

fn respond(menu: &str, response: &str) {
    diag::info!(Ui, "bo2mp shell: menu response {menu} {response}");
    RESPONSES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((menu.to_owned(), response.to_owned()));
}

#[derive(Default)]
pub(crate) struct InGame {
    focus: usize,
    /// The row the pointer was over last frame: the pointer moves the
    /// focus only when it comes onto another row, so a still pointer
    /// does not undo Up / Down every frame.
    hover: Option<usize>,
    held: Option<i32>,
    next_at: f64,
    /// The editor's stick, both ways, and its repeat.
    held2: Option<(i32, i32)>,
    next2: f64,
}

/// The class editor's state while it is up: the front end's own `Shell`
/// (its Create-a-Class screens and rows), opened on the class, with no
/// camos (`in_game`). It writes through `classes::edit`, the one place the
/// front end writes his classes, so the stats file is the same.
static EDITOR: Mutex<Option<Shell>> = Mutex::new(None);

fn editor_lock() -> std::sync::MutexGuard<'static, Option<Shell>> {
    EDITOR.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Choose Class's EDIT CLASS button (clickable, as its key F).
#[derive(Component)]
pub(crate) struct EditHit;

/// Open the editor on class `n` (the class the Choose Class row names).
fn open_editor(n: i32) {
    let mut sh = Shell { in_game: true, class: n, screen: Screen::ClassEdit, focus: 0, ..Shell::default() };
    sh.cac_from = Screen::ClassEdit;
    *editor_lock() = Some(sh);
}

/// The editor's input for a frame (its keys, pad and mouse: the front end's
/// Create-a-Class moves and picks). Returns (close, sound): close when it
/// was backed out of from the class's own screen.
fn editor_input(
    st: &mut InGame,
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    pads: &Query<&Gamepad>,
    hits: &Query<(&Hit, &Interaction)>,
    now: f64,
    esc: bool,
) -> (bool, Option<&'static str>) {
    let mut guard = editor_lock();
    let Some(sh) = guard.as_mut() else { return (true, None) };
    let Some(book) = classes::book() else { return (false, None) };
    sh.now = now;
    if sh.note.as_ref().is_some_and(|(_, until)| now > *until) {
        sh.note = None;
    }
    let before = (sh.screen, sh.focus, sh.page);
    let rows = cac::rows(sh, &book);
    let n = rows.len();
    if n > 0 && sh.focus >= n {
        sh.focus = n - 1;
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    // Moving: the stick (repeating while held), the d-pad, the keys.
    let stick = pads.iter().find_map(|p| {
        let x = p.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let y = p.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        let dx = if x > 0.6 { 1 } else if x < -0.6 { -1 } else { 0 };
        let dy = if y > 0.6 { -1 } else if y < -0.6 { 1 } else { 0 };
        (dx != 0 || dy != 0).then_some((dx, dy))
    });
    let mut dir = (0, 0);
    match stick {
        Some(d) if st.held2 != Some(d) => {
            st.held2 = Some(d);
            st.next2 = now + 0.38;
            dir = d;
        }
        Some(d) if now >= st.next2 => {
            st.next2 = now + 0.12;
            dir = d;
        }
        Some(_) => {}
        None => st.held2 = None,
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
    // The picker's tabs (the games / categories).
    let tabbed = |sh: &mut Shell, by: i32| {
        if sh.screen == Screen::ItemPick {
            let np = cac::picks(sh, &book).0.len().max(1) as i32;
            sh.page = (sh.page as i32 + by).rem_euclid(np) as usize;
            sh.focus = 0;
        }
    };
    if keys.just_pressed(KeyCode::KeyQ) || pad(GamepadButton::LeftTrigger) {
        tabbed(sh, -1);
    }
    if keys.just_pressed(KeyCode::KeyE) || pad(GamepadButton::RightTrigger) {
        tabbed(sh, 1);
    }
    if n > 0 && dir != (0, 0) {
        let weapon_move = (sh.screen == Screen::ItemPick && cac::weapon_picker(sh)).then(|| cac::weapon_step(sh, &book, dir)).flatten();
        if let Some(k) = weapon_move {
            sh.focus = k;
        } else if sh.screen == Screen::ItemPick {
            let f = sh.focus as i32;
            let cols = super::GRID_COLS.load(Ordering::Relaxed).max(1) as i32;
            let mut t = f + dir.0 + dir.1 * cols;
            if t < 0 || t >= n as i32 {
                // (Off the page's side: the next page.)
                if dir.0 != 0 && dir.1 == 0 {
                    tabbed(sh, dir.0);
                    t = 0;
                } else {
                    t = f;
                }
            }
            sh.focus = t.clamp(0, n as i32 - 1) as usize;
        } else {
            if dir.1 != 0 {
                sh.focus = ((sh.focus as i32 + dir.1).rem_euclid(n as i32)) as usize;
            }
            if dir.0 != 0
                && let Some(Act::Cycle(what @ (Setting::LethalCount | Setting::TacticalCount))) = rows.get(sh.focus).map(|r| r.act)
            {
                cac::count(sh, &book, what == Setting::LethalCount, dir.0, now);
            }
        }
    }
    let rows = cac::rows(sh, &book);
    let n = rows.len();
    let mut sound: Option<&'static str> = None;
    let clicked = mouse.just_pressed(MouseButton::Left);
    let mut back = esc || keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::East) || mouse.just_pressed(MouseButton::Right);
    let mut act: Option<Act> = None;
    let mut hovered = None;
    for (hit, i) in hits.iter() {
        if *i == Interaction::None {
            continue;
        }
        match *hit {
            Hit::Row(r) if r < n => {
                hovered = Some(r);
                if clicked {
                    act = Some(rows[r].act);
                }
            }
            Hit::Page(p) if clicked => {
                sh.page = p;
                sh.focus = 0;
                sound = Some("uin_navigation_click");
            }
            Hit::PageStep(by) if clicked => tabbed(sh, by),
            Hit::Step(r, by) if clicked => {
                if let Some(Act::Cycle(what @ (Setting::LethalCount | Setting::TacticalCount))) = rows.get(r).map(|x| x.act) {
                    sh.focus = r;
                    cac::count(sh, &book, what == Setting::LethalCount, by, now);
                }
            }
            Hit::Back if clicked => back = true,
            _ => {}
        }
    }
    if let Some(r) = hovered
        && (st.hover != Some(r) || clicked)
        && sh.focus != r
    {
        sh.focus = r;
    }
    st.hover = hovered;
    if act.is_none() && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::NumpadEnter) || pad(GamepadButton::South)) {
        act = rows.get(sh.focus).map(|r| r.act);
    }
    if let Some(a) = act {
        sound = Some("uin_navigation_select_main");
        match a {
            Act::EditSlot(i) => cac::open_pick(sh, &book, i),
            Act::Choose(k) if sh.screen == Screen::ItemPick => {
                if cac::choose(sh, &book, k, now) {
                    sh.back();
                }
            }
            Act::Cycle(what @ (Setting::LethalCount | Setting::TacticalCount)) => cac::count(sh, &book, what == Setting::LethalCount, 1, now),
            _ => {}
        }
    } else if back {
        sound = Some("cac_cmn_backout");
        if sh.screen == Screen::ItemPick {
            sh.back();
        } else {
            return (true, sound);
        }
    }
    if sound.is_none() && before != (sh.screen, sh.focus, sh.page) && before.0 == sh.screen {
        sound = Some("uin_navigation_click");
    }
    (false, sound)
}

/// The question the yes / no box asks: END GAME or RESTART LEVEL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    EndGame,
    Restart,
    Leave,
}

static ASK: AtomicU8 = AtomicU8::new(0);

fn ask() -> Ask {
    match ASK.load(Ordering::Relaxed) {
        1 => Ask::Restart,
        2 => Ask::Leave,
        _ => Ask::EndGame,
    }
}

/// He hosts the game (a listen server): only the host ends it or restarts
/// it; the others can only leave.
static HOST: AtomicBool = AtomicBool::new(true);

fn host() -> bool {
    HOST.load(Ordering::Relaxed)
}

/// A Zombies game is being played (its pause menu is red, with RESTART
/// LEVEL in place of CHOOSE CLASS).
static ZM: AtomicBool = AtomicBool::new(false);

pub(crate) fn zombies() -> bool {
    ZM.load(Ordering::Relaxed)
}

/// The pause menu's rows' actions.
const RESUME: usize = 0;
const CLASS: usize = 1;
const END: usize = 2;
const OPTIONS: usize = 3;
const RESTART: usize = 4;
const YES: usize = 5;
const NO: usize = 6;
const LEAVE: usize = 7;

pub(super) fn rows(up: u8) -> Vec<Row> {
    let row = |label: &str, sub: &str, act: Act| Row { label: label.to_owned(), sub: sub.to_owned(), value: None, act };
    match up {
        1 if zombies() && host() => vec![
            row("RESUME GAME", "Back to the game.", Act::Choose(RESUME)),
            row("RESTART LEVEL", "The map again from the first round.", Act::Choose(RESTART)),
            row("OPTIONS", "Graphics, sound and controls.", Act::Choose(OPTIONS)),
            row("END GAME", "Ends the game for everyone.", Act::Choose(END)),
        ],
        1 if zombies() => vec![
            row("RESUME GAME", "Back to the game.", Act::Choose(RESUME)),
            row("OPTIONS", "Graphics, sound and controls.", Act::Choose(OPTIONS)),
            row("LEAVE GAME", "You leave; the game goes on for the others.", Act::Choose(LEAVE)),
        ],
        1 => vec![
            row("RESUME", "Back to the match.", Act::Choose(RESUME)),
            row("CHOOSE CLASS", "Your next life spawns with it.", Act::Choose(CLASS)),
            row("OPTIONS", "Graphics, sound and controls.", Act::Choose(OPTIONS)),
            if host() {
                row("END GAME", "Ends the match for everyone and goes back to the lobby.", Act::Choose(END))
            } else {
                row("LEAVE GAME", "You leave; the match goes on for the others.", Act::Choose(LEAVE))
            },
        ],
        4 => vec![row("YES", "", Act::Choose(YES)), row("NO", "", Act::Choose(NO))],
        2 => classes::book()
            .map(|b| {
                b.classes
                    .iter()
                    .map(|c| row(&c.name.to_uppercase(), &format!("{} / {POINTS} points", c.spent), Act::Choose(c.n as usize)))
                    .collect()
            })
            .unwrap_or_default(),
        3 => super::options::rows(OPT_TAB.load(Ordering::Relaxed), &super::options::snapshot()),
        _ => Vec::new(),
    }
}

/// How many Options rows the in-match list shows at once.
const OPT_ROWS: usize = 10;

/// The Options tab he is on in a match.
static OPT_TAB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// In a match: no front end, no load, a match set up (or a Zombies map
/// played).
fn in_match(front: bool, loading: bool, setup: bool) -> bool {
    !front && !loading && (setup || zombies())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn ingame_input(
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    loading: Option<Res<assets::LoadingScreen>>,
    setup: Option<Res<session::Bo2mpMatchSetup>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    hits: Query<(&GameHit, &Interaction)>,
    // (Options' tabs and a selector's arrows, clicked.)
    parts: Query<(&Hit, &Interaction), Without<GameHit>>,
    hud_keys: Option<Res<frame::HudInputView>>,
    time: Res<Time>,
    mut st: Local<InGame>,
    // (The class editor's parts, and Choose Class's EDIT CLASS button.)
    (editor_hits, edit_btn): (Query<(&Hit, &Interaction)>, Query<&Interaction, With<EditHit>>),
    mut sounds: MessageWriter<frame::UiPlaySound>,
    (identity, mut exec, role): (Option<Res<frame::LaunchIdentity>>, MessageWriter<frame::UiExecCommand>, Option<Res<frame::RuntimeRole>>),
) {
    HOST.store(role.as_deref().is_none_or(|r| r.runs_authority()), Ordering::Relaxed);
    ZM.store(
        setup.is_none() && identity.as_deref().is_some_and(|i| crate::lui_hud::is_zombies_zone(&i.zone)),
        Ordering::Relaxed,
    );
    if !bridge::enabled() || !in_match(front.is_some(), loading.is_some(), setup.is_some()) {
        set_up(0);
        SERVER_OPEN.store(false, Ordering::Relaxed);
        SERVER_CLOSE.store(false, Ordering::Relaxed);
        return;
    }
    let now = time.elapsed_secs_f64();
    let mut up = UP.load(Ordering::Relaxed);
    if SERVER_OPEN.swap(false, Ordering::Relaxed) {
        up = 2;
        st.focus = 0;
    }
    if SERVER_CLOSE.swap(false, Ordering::Relaxed) && (up == 2 || up == 5) {
        up = 0;
    }
    if up != 5 {
        // (An editor left open by a closed menu: gone.)
        let mut e = editor_lock();
        if e.is_some() {
            *e = None;
        }
    }
    classes::want(up == 2 || up == 5);
    if hud_keys.as_deref().is_some_and(|k| k.console_open) {
        publish(&st);
        set_up(up);
        return;
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let esc = keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::Start);
    if up == 0 {
        if esc {
            up = 1;
            st.focus = 0;
            sounds.write(frame::UiPlaySound { alias: "uin_main_pause".to_owned() });
        }
        publish(&st);
        set_up(up);
        return;
    }
    // The class editor: the front end's Create-a-Class over the running
    // match (it is not paused); it holds his keys and mouse as the menus do.
    if up == 5 {
        let (close, sound) = editor_input(&mut st, &keys, &mouse, &pads, &editor_hits, now, esc);
        if close {
            let class = editor_lock().take().map_or(0, |s| s.class);
            up = 2;
            st.hover = None;
            st.focus = classes::book().and_then(|b| b.classes.iter().position(|c| c.n == class)).unwrap_or(0);
        }
        if let Some(alias) = sound {
            sounds.write(frame::UiPlaySound { alias: alias.to_owned() });
        }
        publish(&st);
        set_up(up);
        return;
    }
    // The menu sounds: a move (focus, tab, a setting changed), a choice, a
    // step back; the last one this frame is heard.
    let mut sound: Option<&'static str> = None;
    let rows = rows(up);
    let n = rows.len().max(1);
    st.focus = st.focus.min(n - 1);
    let stick = pads.iter().find_map(|p| {
        let y = p.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        if y > 0.6 { Some(-1) } else if y < -0.6 { Some(1) } else { None }
    });
    let mut dir = 0;
    match stick {
        Some(d) if st.held != Some(d) => {
            st.held = Some(d);
            st.next_at = now + 0.38;
            dir = d;
        }
        Some(d) if now >= st.next_at => {
            st.next_at = now + 0.12;
            dir = d;
        }
        Some(_) => {}
        None => st.held = None,
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) || pad(GamepadButton::DPadUp) {
        dir = -1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) || pad(GamepadButton::DPadDown) {
        dir = 1;
    }
    if dir != 0 {
        let was = st.focus;
        st.focus = ((st.focus as i32 + dir).rem_euclid(n as i32)) as usize;
        if st.focus != was {
            sound = Some("uin_navigation_click");
        }
    }
    if up == 3 {
        let tabs = super::options::MATCH_TABS as i32;
        let mut tab = OPT_TAB.load(Ordering::Relaxed) as i32;
        if keys.just_pressed(KeyCode::KeyQ) || pad(GamepadButton::LeftTrigger) {
            tab = (tab - 1).rem_euclid(tabs);
            st.focus = 0;
            sound = Some("uin_navigation_click");
        }
        if keys.just_pressed(KeyCode::KeyE) || pad(GamepadButton::RightTrigger) {
            tab = (tab + 1).rem_euclid(tabs);
            st.focus = 0;
            sound = Some("uin_navigation_click");
        }
        let mut clicked_side = 0;
        if mouse.just_pressed(MouseButton::Left) {
            for (hit, i) in &parts {
                if *i == Interaction::None {
                    continue;
                }
                match *hit {
                    Hit::Page(p) => {
                        tab = (p as i32).clamp(0, tabs - 1);
                        st.focus = 0;
                        sound = Some("uin_navigation_click");
                    }
                    Hit::Step(row, by) => {
                        st.focus = row.min(n - 1);
                        clicked_side = by;
                    }
                    _ => {}
                }
            }
        }
        OPT_TAB.store(tab as usize, Ordering::Relaxed);
        let side = if clicked_side != 0 {
            clicked_side
        } else if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) || pad(GamepadButton::DPadLeft) {
            -1
        } else if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) || pad(GamepadButton::DPadRight) {
            1
        } else {
            0
        };
        if side != 0
            && let Some(Act::Cycle(super::Setting::Option(k))) = rows.get(st.focus).map(|r| r.act)
        {
            super::options::queue(tab as usize, k, side);
            sound = Some("uin_navigation_click");
        }
    }
    // Choose Class: F (X on a pad, or a click on EDIT CLASS) edits the class
    // in focus.
    if up == 2
        && (keys.just_pressed(KeyCode::KeyF)
            || pad(GamepadButton::West)
            || (mouse.just_pressed(MouseButton::Left) && edit_btn.iter().any(|i| *i != Interaction::None)))
        && let Some(Act::Choose(k)) = rows.get(st.focus).map(|r| r.act)
        && classes::book().is_some_and(|b| b.class(k as i32).is_some())
    {
        open_editor(k as i32);
        st.hover = None;
        sounds.write(frame::UiPlaySound { alias: "uin_navigation_select_main".to_owned() });
        publish(&st);
        set_up(5);
        return;
    }
    let mut pick = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(GamepadButton::South);
    let hovered = hits.iter().find(|(_, i)| **i != Interaction::None).map(|(h, _)| h.0);
    if let Some(h) = hovered
        && (st.hover != Some(h) || mouse.just_pressed(MouseButton::Left))
        && st.focus != h
    {
        st.focus = h;
        sound = Some("uin_navigation_click");
    }
    if hovered.is_some() && mouse.just_pressed(MouseButton::Left) {
        pick = true;
    }
    st.hover = hovered;
    let back = esc || keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::East) || mouse.just_pressed(MouseButton::Right);
    if pick && up == 3 {
        if let Some(Act::Cycle(super::Setting::Option(k))) = rows.get(st.focus).map(|r| r.act) {
            super::options::queue(OPT_TAB.load(Ordering::Relaxed), k, 1);
            sound = Some("uin_navigation_select_main");
        }
    } else if pick && let Some(r) = rows.get(st.focus) {
        let Act::Choose(k) = r.act else {
            if let Some(alias) = sound {
                sounds.write(frame::UiPlaySound { alias: alias.to_owned() });
            }
            return;
        };
        sound = Some("uin_navigation_select_main");
        match up {
            1 => match k {
                RESUME => up = 0,
                CLASS => {
                    up = 2;
                    st.focus = 0;
                }
                OPTIONS => {
                    up = 3;
                    st.focus = 0;
                    OPT_TAB.store(0, Ordering::Relaxed);
                }
                // (A question first, NO in focus.)
                RESTART | END | LEAVE => {
                    ASK.store(
                        match k {
                            RESTART => 1,
                            LEAVE => 2,
                            _ => 0,
                        },
                        Ordering::Relaxed,
                    );
                    up = 4;
                    st.focus = 1;
                }
                _ => {}
            },
            4 => {
                let asked = ask();
                up = 1;
                st.focus = asked_row(asked);
                if k == YES {
                    up = 0;
                    match asked {
                        Ask::EndGame => respond("popup_leavegame", "endround"),
                        Ask::Restart => {
                            diag::info!(Ui, "bo2mp shell: restart level");
                            exec.write(frame::UiExecCommand { text: "map_restart".to_owned() });
                        }
                        Ask::Leave => {
                            diag::info!(Ui, "bo2mp shell: leaving the game");
                            exec.write(frame::UiExecCommand { text: "disconnect".to_owned() });
                        }
                    }
                }
            }
            _ => {
                respond("changeclass", &format!("custom{k}"));
                up = 0;
            }
        }
    } else if back {
        let was = up;
        up = if up == 1 { 0 } else { 1 };
        st.focus = match was {
            3 => 2,
            4 => asked_row(ask()),
            _ if up == 1 && !zombies() => 1,
            _ => 0,
        };
        sound = Some("cac_cmn_backout");
    }
    if let Some(alias) = sound {
        sounds.write(frame::UiPlaySound { alias: alias.to_owned() });
    }
    publish(&st);
    set_up(up);
}

#[derive(Component)]
pub(crate) struct GameHit(usize);

#[derive(Component)]
pub(crate) struct GameRoot;

#[allow(clippy::too_many_arguments)]
pub(crate) fn ingame_view(
    mut commands: Commands,
    fonts: Option<Res<ShellFonts>>,
    mut pics: ResMut<ShellPics>,
    icons: Option<Res<assets::T6HudIcons>>,
    mut images: ResMut<Assets<Image>>,
    roots: Query<Entity, With<GameRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    hits: Query<(&GameHit, &Interaction)>,
    time: Res<Time>,
    mut drawn: Local<String>,
) {
    let up = if open() { UP.load(Ordering::Relaxed) } else { 0 };
    let Some(fonts) = fonts.filter(|_| up != 0) else {
        for e in &roots {
            commands.entity(e).despawn();
        }
        drawn.clear();
        return;
    };
    // (The row under the mouse is the one in focus: input set it.)
    let focus = hits.iter().find(|(_, i)| **i != Interaction::None).map(|(h, _)| h.0);
    let st_rows = rows(up);
    let zm = zombies();
    let book = classes::book();
    let (area, s) = window_scale(&windows);
    let pad = crate::pad_glyphs::pad_active();
    let editing = if up == 5 { format!("{:?}", editor_lock().as_ref()) } else { String::new() };
    let key = format!(
        "{up}|{zm}|{editing}|{}|{focus:?}|{}|{area:?}|{pad}|{}|{}|{}|{}",
        ASK.load(Ordering::Relaxed),
        FOCUS.load(Ordering::Relaxed),
        OPT_TAB.load(Ordering::Relaxed),
        super::options::snapshot().revision,
        book.as_ref().map_or(0, |b| b.rev),
        pics.count()
    );
    if *drawn == key && !roots.is_empty() {
        return;
    }
    *drawn = key;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let focus = FOCUS.load(Ordering::Relaxed) as usize;
    // (The slides restart when a menu or tab opens, the focus band when the
    // focus moves, as in the front end.)
    super::anim::note(&format!("ingame/{up}/{}", OPT_TAB.load(Ordering::Relaxed)), focus, time.elapsed_secs());
    // (The class editor is drawn in the front end's title-safe frame, as its
    // Create-a-Class screens are.)
    let safe = ghosts::safe_area(area, s);
    let mut cx = if up == 5 {
        Ctx { s: safe.s, w: safe.w, h: safe.h, fonts: &fonts, pics: &mut pics, icons: icons.as_deref(), images: &mut images, pad, pal: Palette::MULTIPLAYER, still: false, bleed: safe.bleed }
    } else {
        Ctx { s, w: area.x / s, h: area.y / s, fonts: &fonts, pics: &mut pics, icons: icons.as_deref(), images: &mut images, pad, pal: if zm { Palette::ZOMBIES } else { Palette::MULTIPLAYER }, still: false, bleed: Vec2::ZERO }
    };
    let [r, g, b] = cx.pal.shade;
    commands
        .spawn((
            GameRoot,
            GlobalZIndex(15_000),
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(Color::srgba(r, g, b, 0.94), Val::Percent(0.0)),
                ColorStop::new(Color::srgba(r, g, b, 0.72), Val::Percent(45.0)),
                ColorStop::new(Color::srgba(r, g, b, if up == 4 { 0.80 } else { 0.45 }), Val::Percent(100.0)),
            ])),
        ))
        .with_children(|c| {
            if up == 5 {
                editor_view(c, &mut cx, safe.inset);
                return;
            }
            header(c, &cx, &[match up {
                1 | 4 if zm => "OUTBREAK",
                1 | 4 => "PAUSED",
                3 => "OPTIONS",
                _ => "CHOOSE CLASS",
            }]);
            if up == 3 {
                let labels: Vec<String> = super::options::TABS.iter().take(super::options::MATCH_TABS).map(|t| (*t).to_owned()).collect();
                super::view::tabs(c, &mut cx, &labels, OPT_TAB.load(Ordering::Relaxed), Hit::Page);
            }
            c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(196.0), None, None, &cx) }).with_children(|col| {
                // (Under the question: the pause menu, dimmed, not clickable.)
                if up == 4 {
                    let asked = asked_row(ask());
                    for (i, r) in rows(1).iter().enumerate() {
                        ghosts::band(col, &cx, &r.label, i == asked, None, Look::list(600.0), 0.0, true);
                    }
                    return;
                }
                let mut rows = st_rows.clone();
                if up == 3 {
                    for r in &mut rows {
                        r.sub.clear();
                    }
                }
                // (Rows are hit by `GameHit`, not the front end's `Hit`.)
                // (OPTIONS keeps its own rows: it is being redone apart.)
                if up == 3 {
                    // (The window follows the focus, as the front end's.)
                    let (first, last) = super::options::scroll_window(rows.len(), focus, OPT_ROWS);
                    col.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(8.0), margin: UiRect::left(cx.px(72.0)), ..default() })
                        .with_children(|o| menu_rows_window(o, &cx, &rows, focus, 760.0, false, first, last));
                } else {
                    ghosts::rows(col, &cx, &rows, focus, Look::list(600.0));
                }
            });
            if up == 4 {
                question(c, &mut cx, &st_rows, focus);
            }
            if up == 2
                && let Some(b) = book.as_ref()
                && let Some(class) = b.classes.get(focus)
            {
                super::cac::class_card(c, &mut cx, b, class, 640.0, 176.0, 1208.0);
            }
            // The foot, as the front end's: a dark band, the keys at the left.
            c.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: cx.px(72.0),
                    padding: UiRect::horizontal(cx.px(ghosts::INSET)),
                    column_gap: cx.px(28.0),
                    align_items: AlignItems::Center,
                    border: UiRect::top(cx.px(1.0)),
                    ..abs(Some(0.0), None, None, Some(0.0), &cx)
                },
                BorderColor::all(cx.pal.line),
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.72), Val::Percent(0.0)),
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.94), Val::Percent(100.0)),
                ])),
            ))
            .with_children(|f| {
                let hints: &[(&str, &str, &str)] = if up == 3 {
                    &[("ESC", "xenonbutton_b", "BACK"), ("A / D", "xenonbutton_dpad_rl", "CHANGE"), ("Q / E", "xenonbutton_lb", "TABS")]
                } else {
                    &[("ESC", "xenonbutton_b", if up == 1 { "RESUME" } else { "BACK" }), ("ENTER", "xenonbutton_a", "SELECT")]
                };
                for &(k, pad, what) in hints {
                    hint(f, &mut cx, k, pad, what);
                }
                // Choose Class: EDIT CLASS, a key and a button.
                if up == 2 {
                    f.spawn((Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..default() }, EditHit, Button))
                        .with_children(|b| hint(b, &mut cx, "F", "xenonbutton_x", "EDIT CLASS"));
                }
            });
        });
}

/// The class editor, drawn as Choose Class is (the same translucent
/// ground over the running match) with the front end's own Create-a-Class
/// screens (`cac::view`) in it: the class's slots, the weapon / item pickers.
fn editor_view(c: &mut super::view::Kids, cx: &mut Ctx, inset: Vec2) {
    let Some(sh) = editor_lock().clone() else { return };
    let Some(book) = classes::book() else { return };
    let rows = cac::rows(&sh, &book);
    let weapons = sh.screen == Screen::ItemPick && cac::weapon_picker(&sh);
    c.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(inset.x), right: Val::Px(inset.x), top: Val::Px(inset.y), bottom: Val::Px(inset.y), ..default() })
        .with_children(|v| {
            cac::view(v, cx, &sh, &book, &rows);
            super::view::notice_bar(v, cx);
            if let Some((note, _)) = &sh.note {
                super::view::notice(v, cx, note);
            }
            v.spawn((
                Node {
                    padding: UiRect::new(cx.px(150.0 + cx.bleed.x), cx.px(150.0 + cx.bleed.x), Val::Px(0.0), cx.px(cx.bleed.y)),
                    align_items: AlignItems::Center,
                    border: UiRect::top(cx.px(1.0)),
                    ..ghosts::foot_shade(cx, 72.0)
                },
                BorderColor::all(cx.pal.line),
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.72), Val::Percent(0.0)),
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.94), Val::Percent(100.0)),
                ])),
            ))
            .with_children(|f| {
                f.spawn((Node { column_gap: cx.px(28.0), align_items: AlignItems::Center, ..default() }, Hit::Back, Button)).with_children(|l| {
                    hint(l, cx, "ESC", "xenonbutton_b", "BACK");
                    hint(l, cx, "ENTER", "xenonbutton_a", "SELECT");
                    if weapons {
                        hint(l, cx, "Q / E", "xenonbutton_lb", "GAMES");
                        hint(l, cx, "W / S", "xenonbutton_dpad_ud", "CATEGORIES");
                    } else if sh.screen == Screen::ItemPick {
                        hint(l, cx, "Q / E", "xenonbutton_lb", "CATEGORIES");
                    } else {
                        hint(l, cx, "A / D", "xenonbutton_dpad_rl", "CHANGE");
                    }
                });
            });
        });
}

/// The pause menu's row a question was asked from.
fn asked_row(a: Ask) -> usize {
    rows(1).iter().position(|r| r.act == Act::Choose(match a {
        Ask::Restart => RESTART,
        Ask::Leave => LEAVE,
        Ask::EndGame => END,
    })).unwrap_or(0)
}

/// The yes / no box over the pause menu: the question, what it means, and
/// YES / NO (NO first in focus), in the screen's colours.
fn question(c: &mut super::view::Kids, cx: &mut Ctx, rows: &[Row], focus: usize) {
    let (title, body) = match ask() {
        Ask::Restart => ("RESTART LEVEL?", "The map starts over from the first round. Everyone's progress in this game is lost."),
        Ask::EndGame if zombies() => ("END GAME?", "The game ends for everyone in it."),
        Ask::EndGame => ("END GAME?", "The match ends for everyone and goes back to the lobby."),
        Ask::Leave => ("LEAVE GAME?", "You leave the game and go back to the menu. It goes on for the others."),
    };
    // (As the front end asks: `ghosts::popup`; YES / NO are the rows, read
    // as `GameHit`.)
    ghosts::popup(c, cx, title, body, 15_090, |col, cx| ghosts::rows(col, cx, rows, focus, Look { width: ghosts::INSET + 320.0, h: 50.0, px: 28.0, sub: None }));
}

/// The row in focus, as input left it (for the view).
static FOCUS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The rows' hits are the front end's `Hit::Row`; in a match they are read
/// as `GameHit` (added beside them).
pub(crate) fn tag_hits(mut commands: Commands, rows: Query<(Entity, &Hit), (Added<Hit>, Without<GameHit>)>) {
    if !open() {
        return;
    }
    for (e, hit) in &rows {
        if let Hit::Row(i) = hit {
            commands.entity(e).insert(GameHit(*i));
        }
    }
}

pub(crate) fn publish(st: &InGame) {
    FOCUS.store(st.focus, Ordering::Relaxed);
}

/// The preview: a menu up as if he had opened it.
pub(crate) fn preview_open(up: u8) {
    set_up(up);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Esc's pause menu holds the game's input (gameplay keys, look, fire,
    /// the cursor grab read `frame::shell_menu_open`), one frame more after
    /// it closes, then gives it back.
    #[test]
    fn open_menu_captures_input() {
        if !bridge::enabled() {
            return;
        }
        set_up(0);
        set_up(0);
        assert!(!frame::shell_menu_open());
        for up in [1, 2, 3, 4, 5] {
            set_up(up);
            assert!(frame::shell_menu_open(), "{} up, input not captured", menu_name(up));
            assert!(open());
        }
        set_up(0);
        assert!(frame::shell_menu_open(), "the closing frame still holds the input");
        set_up(0);
        assert!(!frame::shell_menu_open());
        assert!(!open());
    }

    /// Every row of every in-match Options tab is reachable: the window
    /// holds the focus, and its rows number from the list's own.
    #[test]
    fn match_options_window_reaches_every_row() {
        let s = super::super::options::snapshot();
        for tab in 0..super::super::options::MATCH_TABS {
            let n = super::super::options::rows(tab, &s).len();
            for f in 0..n {
                let (a, b) = super::super::options::scroll_window(n, f, OPT_ROWS);
                assert!(a <= f && f < b && b <= n && b - a <= OPT_ROWS, "tab {tab} focus {f}");
            }
        }
    }

    /// The pause menu's rows (and Choose Class's, the same `ghosts::rows`)
    /// are clickable: each a `Hit::Row` with an `Interaction`.
    #[test]
    fn pause_rows_have_hit_targets() {
        let mut world = World::new();
        let fonts = ShellFonts { regular: Handle::default(), bold: Handle::default(), light: Handle::default(), black: Handle::default() };
        let mut pics = ShellPics::default();
        let mut images = Assets::<Image>::default();
        let cx = Ctx { s: 1.0, w: 1920.0, h: 1080.0, fonts: &fonts, pics: &mut pics, icons: None, images: &mut images, pad: false, pal: Palette::MULTIPLAYER, still: false, bleed: Vec2::ZERO };
        let pause = rows(1);
        assert!(pause.len() >= 3);
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            commands.spawn(Node::default()).with_children(|c| ghosts::rows(c, &cx, &pause, 0, Look::list(600.0)));
        }
        queue.apply(&mut world);
        let mut hits: Vec<usize> = world
            .query::<(&Hit, &Interaction)>()
            .iter(&world)
            .filter_map(|(h, _)| if let Hit::Row(i) = h { Some(*i) } else { None })
            .collect();
        hits.sort_unstable();
        assert_eq!(hits, (0..pause.len()).collect::<Vec<_>>());
    }
}

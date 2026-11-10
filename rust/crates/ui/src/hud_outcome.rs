//! bo2mp: the free-for-all end screen in the modern HUD, after Modern
//! Warfare Remastered's: the world dimmed, VICTORY or DEFEAT in big red (or
//! cool blue) capitals over a thin rule, the end reason under it ("Score
//! limit reached"), then the top three, each row a step further right: its
//! place in a diamond ("1st", the winner's with chevrons after it), the
//! name, the score at the right, a thin line under it. The title, names and
//! scores decode in as Black Ops' end screen's text does (`Decode`): hidden
//! behind random digits that lock into the real letters, in an order drawn
//! at random each match end. His own row follows the three when
//! he placed lower. Gun Game shows the gun reached; One in the Chamber,
//! Sharpshooter and the rest their points.
//!
//! The scripts publish `bo2mp_outcome` as the match ends (mp_globallogic.gsc
//! `bo2mp_ffaoutcome`); the screen shows until the final killcam starts and
//! does not come back after it (the scoreboard follows).
//!
//! Resurgence (`br`, each row's score its squad's place) has Warzone's own
//! (Modern Warfare III's, in WARZONE's colours and words,
//! `shell::wz_theme`): VICTORY, or ELIMINATED and the squad's place, under
//! its picture from the install, "SQUAD PLACED #n OF m", then the squads in
//! place order, each its place and its players' names, his own lit.

use bevy::prelude::*;
use net::{LocalPresentClient, PresentedSnapshot};

use crate::layers::GameUiFont;
use crate::shell::theme::ShellFonts;

/// One player's line of the result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub num: i32,
    pub score: i32,
    pub name: String,
}

/// The result as the scripts publish it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Outcome {
    /// The winner's entity number; -1 none (a draw), -2 a team game's
    /// side (`winners`).
    pub winner: i32,
    /// A team game's winning side (its players' entity numbers).
    pub winners: Vec<i32>,
    pub gametype: String,
    pub reason: String,
    pub placed: Vec<Placed>,
}

/// `"<winner>|<gametype>|<reason>|<num>,<score>,<name>;..."` (a name may
/// hold commas; it is the row's last field). A team game the host ended
/// (`bo2mp_hostendedoutcome`) has "tie" or "team:<num>/<num>..." for the
/// winner.
pub(crate) fn parse_outcome(s: &str) -> Option<Outcome> {
    let mut parts = s.splitn(4, '|');
    let w = parts.next()?.trim();
    let (winner, winners) = if w == "tie" {
        (-1, Vec::new())
    } else if let Some(side) = w.strip_prefix("team:") {
        (-2, side.split('/').filter_map(|n| n.trim().parse().ok()).collect())
    } else {
        (w.parse().ok()?, Vec::new())
    };
    let gametype = parts.next()?.trim().to_owned();
    let reason = parts.next()?.trim().to_owned();
    let placed = parts
        .next()
        .unwrap_or("")
        .split(';')
        .filter_map(|row| {
            let mut f = row.splitn(3, ',');
            let num = f.next()?.trim().parse().ok()?;
            let score = f.next()?.trim().parse::<f32>().ok()? as i32;
            Some(Placed { num, score, name: f.next().unwrap_or("").to_owned() })
        })
        .collect();
    Some(Outcome { winner, winners, gametype, reason, placed })
}

/// The end reason's words from its localized key (`MP_SCORE_LIMIT_REACHED`
/// -> "Score limit reached").
pub(crate) fn reason_text(key: &str) -> String {
    let k = key.trim().trim_start_matches('&').to_ascii_uppercase();
    let known = [
        ("SCORE_LIMIT_REACHED", "Score limit reached"),
        ("TIME_LIMIT_REACHED", "Time limit reached"),
        ("HOST_ENDED_GAME", "Host ended game"),
        ("ENDED_GAME", "Game ended"),
        ("ROUND_LIMIT_REACHED", "Round limit reached"),
        ("ENEMIES_ELIMINATED", "Enemies eliminated"),
        ("TARGET_DESTROYED", "Target destroyed"),
        ("PLAYERS_FORFEITED", "Players forfeited"),
    ];
    if let Some((_, t)) = known.iter().find(|(id, _)| k.ends_with(id)) {
        return (*t).to_owned();
    }
    if k.is_empty() || !k.contains('_') {
        return key.trim().to_owned();
    }
    // Another key: its words after the game's prefix, sentence case.
    let words = k.split('_').skip_while(|w| matches!(*w, "MP" | "MPUI" | "WAGER" | "GAME")).collect::<Vec<_>>().join(" ").to_lowercase();
    let mut c = words.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// "1st", "2nd", "3rd", "4th", ... "11th", "21st".
pub(crate) fn place_label(place: usize) -> String {
    let suffix = match (place % 10, place % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{place}{suffix}")
}

/// The score as the row shows it.
pub(crate) fn score_text(gametype: &str, score: i32) -> String {
    match gametype {
        "gun" => format!("GUN {score}"),
        // (Resurgence: the squad's place.)
        "br" => format!("#{score}"),
        _ => score.to_string(),
    }
}

/// The rows shown: (place, row), the top three and then his own when he
/// placed lower.
pub(crate) fn shown_rows(o: &Outcome, me: i32) -> Vec<(usize, Placed)> {
    let mut rows: Vec<(usize, Placed)> = o.placed.iter().take(3).cloned().enumerate().map(|(i, p)| (i + 1, p)).collect();
    if let Some(at) = o.placed.iter().position(|p| p.num == me)
        && at >= 3
    {
        rows.push((at + 1, o.placed[at].clone()));
    }
    rows
}

/// How long the title, the names and the scores take to decode (seconds):
/// Black Ops' end screen takes about a second for VICTORY and a little
/// longer for the standings under it.
const TITLE_DECODE_S: f32 = 1.2;
const NAME_DECODE_S: f32 = 1.5;
const SCORE_DECODE_S: f32 = 1.2;
/// How many times a second the hidden characters change.
const SCRAMBLE_HZ: f32 = 20.0;

/// The sound a decoding line starts with: a name for Black Ops' text-effect
/// sound, routed in `audio::ui_sounds` (the UI crate does not depend on
/// that one). UNVERIFIED alias: no Black Ops data was at hand when this was
/// written, so the route's candidates are guesses (see there); when none
/// resolves in the bank the sound is skipped without a word.
pub(crate) const DECODE_SOUND: &str = "bo2mp_bo1_decode";

/// A random-number source for the effect: xorshift64* seeded from the
/// operating system's entropy (`getrandom`), so no two match ends read the
/// same; the clock only stands in if the system gives none.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn from_entropy() -> Rng {
        let mut b = [0u8; 8];
        let seed = if getrandom::fill(&mut b).is_ok() {
            u64::from_le_bytes(b)
        } else {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
        };
        Rng(seed | 1)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0 up to (not including) `n`.
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        ((self.next_u64() >> 32) * u64::from(n.max(1)) >> 32) as u32
    }

    /// 0 up to (not including) 1.
    pub(crate) fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// When each letter of a `chars`-letter line locks in (seconds after the
/// line starts) and how many extra hidden characters trail it at the start:
/// every letter's time is drawn at random between a tenth of `full` and
/// `full`, one of them exactly at `full`, so the order is different each
/// time and the line is whole at `full`.
pub(crate) fn plan_decode(chars: usize, full: f32, rng: &mut Rng) -> (Vec<f32>, usize) {
    let mut lock: Vec<f32> = (0..chars).map(|_| full * (0.1 + 0.9 * rng.unit())).collect();
    if !lock.is_empty() {
        let last = rng.below(chars as u32) as usize;
        lock[last] = full;
    }
    // (Black Ops' hidden text starts longer than the words and shrinks to
    // them as the letters lock.)
    let extra = 4 + rng.below(7) as usize;
    (lock, extra)
}

/// What a decoding line reads `t` seconds in: the letters locked so far in
/// their places, every other place a hidden digit (`glyph`, called afresh
/// for each), and after them the extra hidden characters, fewer as more
/// letters lock; the real line, whole, from `full` on.
pub(crate) fn decoded(target: &str, t: f32, full: f32, lock: &[f32], extra: usize, glyph: &mut impl FnMut() -> char) -> String {
    if t >= full {
        return target.to_owned();
    }
    let n = target.chars().count();
    let locked = lock.iter().filter(|l| t >= **l).count();
    let tail = if n == 0 { 0 } else { ((extra * (n - locked.min(n))) as f32 / n as f32).ceil() as usize };
    let mut out = String::new();
    for (i, ch) in target.chars().enumerate() {
        if lock.get(i).is_some_and(|l| t >= *l) {
            out.push(ch);
        } else {
            out.push(glyph());
        }
    }
    for _ in 0..tail {
        out.push(glyph());
    }
    out
}

/// A line that decodes in (the Black Ops end screen's effect): its words,
/// when it starts (real seconds), how long it takes, its own random
/// plan and random source (`plan_decode`, `Rng`).
#[derive(Component)]
pub(crate) struct Decode {
    target: String,
    start: f64,
    full: f32,
    lock: Vec<f32>,
    extra: usize,
    rng: Rng,
    /// The hidden characters as last drawn and the tick they were drawn on.
    hidden: Vec<char>,
    tick: i64,
}

impl Decode {
    pub(crate) fn new(target: String, start: f64, full: f32) -> Decode {
        let mut rng = Rng::from_entropy();
        let (lock, extra) = plan_decode(target.chars().count(), full, &mut rng);
        Decode { target, start, full, lock, extra, rng, hidden: Vec::new(), tick: i64::MIN }
    }

    /// The line `t` seconds after it started. Its hidden characters change
    /// `SCRAMBLE_HZ` times a second, each a fresh random digit.
    pub(crate) fn read(&mut self, t: f32) -> String {
        let tick = (t.max(0.0) * SCRAMBLE_HZ).floor() as i64;
        let need = self.target.chars().count() + self.extra;
        if tick != self.tick || self.hidden.len() != need {
            self.tick = tick;
            self.hidden = (0..need).map(|_| char::from(b'0' + self.rng.below(10) as u8)).collect();
        }
        let mut at = 0;
        let hidden = &self.hidden;
        decoded(&self.target, t, self.full, &self.lock, self.extra, &mut || {
            at += 1;
            hidden[(at - 1) % hidden.len()]
        })
    }
}

#[derive(Component)]
struct OutcomeRoot;


#[derive(Default)]
struct State {
    /// The result being shown (or shown already) and when it was first seen.
    shown: Option<(String, f64)>,
    /// A killcam began after it: the screen is done.
    done: bool,
    /// When each decoding line's sound is due (real seconds), soonest last.
    sounds: Vec<f64>,
}

const DEFEAT_RED: Color = Color::srgb(0.86, 0.30, 0.18);
const VICTORY_BLUE: Color = Color::srgb(0.50, 0.82, 1.0);
const DRAW_GREY: Color = Color::srgb(0.80, 0.80, 0.82);

/// bo2mp: the end screen is up: it alone shows (VICTORY / DEFEAT / DRAW and
/// the places); the HUD, scoreboard and the rest are hidden meanwhile
/// (`screen::layers_for_screen`).
static OUTCOME_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn outcome_up() -> bool {
    OUTCOME_UP.load(std::sync::atomic::Ordering::Relaxed)
}

#[allow(clippy::too_many_arguments)]
fn outcome_screen(
    mut commands: Commands,
    presented: Option<Res<PresentedSnapshot>>,
    local: Option<Res<LocalPresentClient>>,
    (shell_fonts, game_font): (Option<Res<ShellFonts>>, Option<Res<GameUiFont>>),
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<OutcomeRoot>>,
    mut lines: Query<(&mut Text, &mut Decode)>,
    time: Res<Time<Real>>,
    mut state: Local<State>,
    mut sounds: MessageWriter<frame::UiPlaySound>,
    (mut cache, mut images): (Local<std::collections::HashMap<String, Option<Handle<Image>>>>, ResMut<Assets<Image>>),
) {
    let now = time.elapsed_secs_f64();
    let seen = match (presented.as_deref().and_then(PresentedSnapshot::snapshot), local.as_deref()) {
        (Some(snap), Some(l)) if crate::hud_modern::modern_hud_enabled() => {
            let d = snap.meta.script_dvars(l.0);
            let outcome = d.string("bo2mp_outcome").unwrap_or_default().to_owned();
            let killcam = d.string("bo2mp_killcam").unwrap_or_default();
            Some((outcome, killcam.starts_with("1|") || killcam.starts_with("2|"), l.0.0 as i32))
        }
        _ => None,
    };
    let clear = |commands: &mut Commands| {
        OUTCOME_UP.store(false, std::sync::atomic::Ordering::Relaxed);
        for e in &roots {
            commands.entity(e).despawn();
        }
    };
    let Some((raw, in_killcam, me)) = seen else {
        *state = State::default();
        clear(&mut commands);
        return;
    };
    if raw.is_empty() {
        // (A new match: ready for its result.)
        *state = State::default();
        clear(&mut commands);
        return;
    }
    if state.shown.as_ref().is_some_and(|(r, _)| *r != raw) {
        *state = State::default();
        clear(&mut commands);
    }
    if in_killcam && state.shown.is_some() {
        state.done = true;
    }
    if state.done || in_killcam {
        clear(&mut commands);
        return;
    }
    let Some(outcome) = parse_outcome(&raw) else { return };
    if state.shown.is_none() {
        state.shown = Some((raw.clone(), now));
    }
    let start = state.shown.as_ref().map_or(now, |(_, t)| *t);
    OUTCOME_UP.store(true, std::sync::atomic::Ordering::Relaxed);
    if !roots.is_empty() {
        for (mut text, mut d) in &mut lines {
            let t = (now - d.start) as f32;
            let v = d.read(t);
            if text.0 != v {
                text.0 = v;
            }
        }
        // (Each line's sound as it starts to decode.)
        while state.sounds.last().is_some_and(|t| *t <= now) {
            state.sounds.pop();
            sounds.write(frame::UiPlaySound { alias: DECODE_SOUND.to_owned() });
        }
        return;
    }
    let Some(faces) = crate::hud_match::Faces::of(shell_fonts.as_deref(), game_font.as_deref()) else { return };
    let title_face = shell_fonts.as_deref().map_or_else(|| faces.bold.clone(), |s| s.black.clone());
    let h = window.single().map_or(1080.0, Window::height);
    if outcome.gametype == "br" {
        let won = br_won(&outcome, me);
        let pic = crate::hud_warzone::picture(&mut cache, &mut images, if won { "squad" } else { "eliminated" });
        spawn_br_screen(&mut commands, &faces, &title_face, (h / 1080.0).max(0.4), &outcome, me, start, pic);
        return;
    }
    // The title's decode and each shown row's, as `spawn_screen` starts them.
    let rows = shown_rows(&outcome, me).len();
    state.sounds = std::iter::once(start).chain((0..rows).map(|i| start + 0.25 + i as f64 * 0.18)).rev().collect();
    if host_ended(&outcome.reason) {
        spawn_host_ended(&mut commands, &faces, (h / 1080.0).max(0.4), &outcome, me, start);
        return;
    }
    spawn_screen(&mut commands, &faces, &title_face, (h / 1080.0).max(0.4), &outcome, me, start);
}

/// Resurgence: his squad won (it placed first).
pub(crate) fn br_won(o: &Outcome, me: i32) -> bool {
    o.winner == me || o.placed.iter().any(|p| p.num == me && p.score == 1)
}

/// Resurgence's squads in place order: (place, its players' rows).
pub(crate) fn br_squads(o: &Outcome) -> Vec<(i32, Vec<Placed>)> {
    let mut out: Vec<(i32, Vec<Placed>)> = Vec::new();
    for p in &o.placed {
        match out.iter_mut().find(|(place, _)| *place == p.score) {
            Some((_, rows)) => rows.push(p.clone()),
            None => out.push((p.score, vec![p.clone()])),
        }
    }
    out.sort_by_key(|(place, _)| *place);
    out
}

/// The squads shown: the top three, then his own when it placed lower.
pub(crate) fn br_shown(o: &Outcome, me: i32) -> Vec<(i32, Vec<Placed>)> {
    let all = br_squads(o);
    let mine = all.iter().position(|(_, rows)| rows.iter().any(|p| p.num == me));
    let mut out: Vec<_> = all.iter().take(3).cloned().collect();
    if let Some(at) = mine
        && at >= 3
    {
        out.push(all[at].clone());
    }
    out
}

/// The title: VICTORY, or ELIMINATED and his squad's place.
pub(crate) fn br_title(o: &Outcome, me: i32) -> String {
    use crate::shell::wz_theme::HUD_WORDS;
    if br_won(o, me) {
        return HUD_WORDS.victory.to_owned();
    }
    match o.placed.iter().find(|p| p.num == me) {
        Some(p) => format!("{} \u{2014} #{}", HUD_WORDS.eliminated, p.score),
        None => HUD_WORDS.eliminated.to_owned(),
    }
}

/// Resurgence's end screen (see the module's notes).
#[allow(clippy::too_many_arguments)]
fn spawn_br_screen(
    commands: &mut Commands,
    faces: &crate::hud_match::Faces,
    title_face: &Handle<Font>,
    k: f32,
    outcome: &Outcome,
    me: i32,
    start: f64,
    pic: Option<Handle<Image>>,
) {
    use crate::shell::wz_theme::{HUD, HUD_WORDS};
    let px = |v: f32| Val::Px(v * k);
    let font = |f: &Handle<Font>, size: f32| crate::shell::theme::font(f, size * k);
    let shade = |size: f32| TextShadow { offset: Vec2::splat((size * k / 16.0).max(1.0)), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() };
    let nowrap = || TextLayout::linebreak(bevy::text::LineBreak::NoWrap);
    let won = br_won(outcome, me);
    let colour = if won { HUD.accent } else { HUD.down };
    let squads = br_squads(outcome);
    let placed_line = match outcome.placed.iter().find(|p| p.num == me) {
        Some(p) => format!("{} #{} {} {}", HUD_WORDS.squad_placed, p.score, HUD_WORDS.of, squads.len().max(p.score.max(0) as usize)),
        None => String::new(),
    };
    let reason = reason_text(&outcome.reason);
    let shown = br_shown(outcome, me);
    commands
        .spawn((
            OutcomeRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.66)),
            GlobalZIndex(40),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Percent(13.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|col| {
                // The picture (a plain diamond when the install lacks it).
                match pic {
                    Some(image) => {
                        col.spawn((ImageNode { image, color: colour, ..default() }, Node { width: px(96.0), height: px(96.0), ..default() }));
                    }
                    None => {
                        col.spawn((
                            Node { width: px(56.0), height: px(56.0), margin: UiRect::all(px(20.0)), border: UiRect::all(px(3.0)), ..default() },
                            BorderColor::all(colour),
                            UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
                        ));
                    }
                }
                col.spawn((
                    Text::new(br_title(outcome, me)),
                    font(title_face, 66.0),
                    TextColor(colour),
                    shade(66.0),
                    nowrap(),
                    Node { margin: UiRect::top(px(6.0)), ..default() },
                ));
                col.spawn((Node { width: px(520.0), height: px(3.0), margin: UiRect::vertical(px(8.0)), ..default() }, BackgroundColor(colour.with_alpha(0.85))));
                if !placed_line.is_empty() {
                    col.spawn((Text::new(placed_line), font(&faces.bold, 25.0), TextColor(HUD.text), shade(25.0), nowrap()));
                }
                col.spawn((
                    Text::new(reason),
                    font(&faces.regular, 19.0),
                    TextColor(HUD.dim),
                    shade(19.0),
                    nowrap(),
                    Node { margin: UiRect::top(px(4.0)), ..default() },
                ));
                col.spawn(Node { height: px(26.0), ..default() });
                for (i, (place, rows)) in shown.iter().enumerate() {
                    let is_mine = rows.iter().any(|p| p.num == me);
                    let first = *place == 1;
                    let row_start = start + 0.25 + i as f64 * 0.18;
                    let names = rows.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("   ");
                    col.spawn((
                        Node {
                            width: px(720.0),
                            height: px(52.0),
                            // (His own squad after the three, a gap before it.)
                            margin: UiRect::top(px(if i == 3 { 16.0 } else { 4.0 })),
                            align_items: AlignItems::Center,
                            border: UiRect::left(px(4.0)),
                            ..default()
                        },
                        BorderColor::all(if is_mine { HUD.accent } else { HUD.track }),
                        BackgroundColor(if is_mine { HUD.accent.with_alpha(0.16) } else { HUD.panel }),
                    ))
                    .with_children(|r| {
                        // The place in its box.
                        r.spawn((
                            Node {
                                width: px(64.0),
                                height: Val::Percent(100.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                margin: UiRect::right(px(14.0)),
                                ..default()
                            },
                            BackgroundColor(if first { HUD.accent } else { HUD.track }),
                        ))
                        .with_children(|b| {
                            b.spawn((Text::new(format!("#{place}")), font(&faces.bold, 22.0), TextColor(if first { Color::BLACK } else { HUD.text }), nowrap()));
                        });
                        r.spawn((
                            Text::new(""),
                            Decode::new(names, row_start, NAME_DECODE_S),
                            font(&faces.regular, if first { 24.0 } else { 21.0 }),
                            TextColor(if is_mine { HUD.text } else { HUD.text.with_alpha(0.85) }),
                            shade(22.0),
                            nowrap(),
                            Node { flex_grow: 1.0, ..default() },
                        ));
                    });
                }
            });
        });
}

/// The screen, drawn for `outcome` as player `me` sees it (its lines start
/// decoding at `start`, real seconds).
fn spawn_screen(commands: &mut Commands, faces: &crate::hud_match::Faces, title_face: &Handle<Font>, k: f32, outcome: &Outcome, me: i32, start: f64) {
    let px = |v: f32| Val::Px(v * k);
    let font = |f: &Handle<Font>, size: f32| crate::shell::theme::font(f, size * k);
    let shade = |size: f32| TextShadow { offset: Vec2::splat((size * k / 16.0).max(1.0)), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() };
    let (title, colour) = match title_of(outcome, me) {
        Result::Draw => ("DRAW", DRAW_GREY),
        Result::Victory => ("VICTORY", VICTORY_BLUE),
        Result::Defeat => ("DEFEAT", DEFEAT_RED),
    };
    let rows = shown_rows(&outcome, me);
    let reason = reason_text(&outcome.reason);
    commands
        .spawn((
            OutcomeRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
            GlobalZIndex(40),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Percent(21.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new(""),
                    Decode::new(title.to_owned(), start, TITLE_DECODE_S),
                    font(title_face, 64.0),
                    TextColor(colour),
                    shade(64.0),
                    TextLayout::linebreak(bevy::text::LineBreak::NoWrap),
                ));
                col.spawn((Node { width: px(470.0), height: px(1.5), margin: UiRect::vertical(px(8.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.35))));
                col.spawn((Text::new(reason), font(&faces.regular, 23.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)), shade(23.0), TextLayout::linebreak(bevy::text::LineBreak::NoWrap)));
                col.spawn(Node { height: px(26.0), ..default() });
                for (i, (place, p)) in rows.iter().enumerate() {
                    let mine = p.num == me;
                    let first = *place == 1;
                    // Each row a step further right, its own row after a gap.
                    let step = i.min(2) as f32 * 26.0;
                    let row_start = start + 0.25 + i as f64 * 0.18;
                    col.spawn(Node {
                        width: px(860.0 - step * 2.0),
                        height: px(if first { 66.0 } else { 58.0 }),
                        margin: UiRect { left: px(step * 2.0), top: px(if *place > 3 { 14.0 } else { 0.0 }), ..default() },
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(px(1.0)),
                        ..default()
                    })
                    .insert(BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.18)))
                    .with_children(|r| {
                        // The place in its diamond.
                        r.spawn(Node { width: px(78.0), height: px(58.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() })
                            .with_children(|d| {
                                d.spawn((
                                    Node { position_type: PositionType::Absolute, width: px(40.0), height: px(40.0), border: UiRect::all(px(1.5)), ..default() },
                                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, if first { 0.75 } else { 0.45 })),
                                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                                    UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
                                ));
                                d.spawn((Text::new(place_label(*place)), font(&faces.bold, 16.0), TextColor(Color::WHITE), shade(16.0), TextLayout::linebreak(bevy::text::LineBreak::NoWrap)));
                            });
                        // The winner's chevrons.
                        r.spawn((
                            Text::new(if first { "\u{203A}\u{203A}" } else { "" }),
                            font(&faces.regular, 30.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.45)),
                            Node { width: px(54.0), margin: UiRect::left(px(6.0)), ..default() },
                        ));
                        let name_colour = if mine { colour } else { Color::srgba(1.0, 1.0, 1.0, if first { 1.0 } else { 0.85 }) };
                        r.spawn((
                            Text::new(""),
                            Decode::new(p.name.clone(), row_start, NAME_DECODE_S),
                            font(&faces.regular, if first { 34.0 } else { 27.0 }),
                            TextColor(name_colour),
                            shade(30.0),
                            TextLayout::linebreak(bevy::text::LineBreak::NoWrap),
                            Node { flex_grow: 1.0, ..default() },
                        ));
                        r.spawn((
                            Text::new(""),
                            Decode::new(score_text(&outcome.gametype, p.score), row_start + 0.1, SCORE_DECODE_S),
                            font(&faces.bold, if first { 26.0 } else { 23.0 }),
                            TextColor(Color::WHITE),
                            shade(24.0),
                            TextLayout::linebreak(bevy::text::LineBreak::NoWrap),
                            Node { margin: UiRect::right(px(12.0)), ..default() },
                        ));
                    });
                }
            });
        });
}

/// How the match went for him.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Result {
    Victory,
    Defeat,
    Draw,
}

pub(crate) fn title_of(o: &Outcome, me: i32) -> Result {
    match o.winner {
        -1 => Result::Draw,
        -2 if o.winners.contains(&me) => Result::Victory,
        w if w == me => Result::Victory,
        _ if o.gametype == "br" && o.placed.iter().any(|p| p.num == me && p.score == 1) => Result::Victory,
        _ => Result::Defeat,
    }
}

/// The host ended the game (`MP_HOST_ENDED_GAME`, or its poor-connection
/// `MP_HOST_SUCKS`): the end screen is the shell's Ghosts look
/// (`spawn_host_ended`), not the Modern Warfare one.
pub(crate) fn host_ended(reason: &str) -> bool {
    let k = reason.trim().trim_start_matches('&').to_ascii_uppercase();
    k.ends_with("HOST_ENDED_GAME") || k.ends_with("HOST_SUCKS")
}

/// The end screen when the host ended the game, in the shell's Ghosts look
/// (`shell::ghosts::popup` and `rows`, Multiplayer blue; the in-match HUD
/// has no shell `Ctx`, so the parts are drawn here from `shell::theme`):
/// the world in the palette's shade, a dark band right across with an
/// accent rule along its top; in it VICTORY / DEFEAT / DRAW behind an
/// accent bar (red for DEFEAT, grey for DRAW) and the reason under it
/// ("Host ended game"); under the band the same standings as the other
/// screen as ruled bands from the screen's left edge, the place at the
/// left, the name and score pushed right, each ending in a cut corner, his
/// own in the accent. The words, the decode and its timing, and when the
/// screen shows and goes are the other screen's.
fn spawn_host_ended(commands: &mut Commands, faces: &crate::hud_match::Faces, k: f32, outcome: &Outcome, me: i32, start: f64) {
    use crate::shell::theme::{ENEMY, LINE, Palette, TEXT, WHITE, fade_right, font};
    const INSET: f32 = 150.0;
    let pal = Palette::MULTIPLAYER;
    let acc = pal.accent;
    let px = |v: f32| Val::Px(v * k);
    let nowrap = || TextLayout::linebreak(bevy::text::LineBreak::NoWrap);
    let (title, bar) = match title_of(outcome, me) {
        Result::Draw => ("DRAW", DRAW_GREY),
        Result::Victory => ("VICTORY", acc),
        Result::Defeat => ("DEFEAT", ENEMY),
    };
    let rows = shown_rows(outcome, me);
    let reason = reason_text(&outcome.reason);
    let [r, g, b] = pal.shade;
    let p = pal.panel.to_srgba();
    let end = Color::srgba(p.red, p.green, p.blue, 0.66);
    // A cut corner at a band's right end (`shell::ghosts::bevel`).
    let bevel = |c: &mut ChildSpawnerCommands, h: f32, color: Color| {
        let d = h * std::f32::consts::SQRT_2;
        c.spawn(Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), width: px(h * 0.5), height: px(h), overflow: Overflow::clip(), ..default() }).with_child((
            Node { position_type: PositionType::Absolute, left: px(-d * 0.5), top: px(-d * 0.5), width: px(d), height: px(d), ..default() },
            BackgroundColor(color),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
        ));
    };
    commands
        .spawn((
            OutcomeRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, ..default() },
            BackgroundColor(Color::srgba(r, g, b, 0.72)),
            GlobalZIndex(40),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(Node { height: Val::Percent(22.0), ..default() });
            // The band: title and reason.
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(px(INSET), px(INSET), px(30.0), px(30.0)),
                    row_gap: px(10.0),
                    border: UiRect::vertical(px(1.0)).with_top(px(2.0)),
                    ..default()
                },
                fade_right(Color::srgba(p.red, p.green, p.blue, 0.98), Color::srgba(p.red, p.green, p.blue, 0.86)),
                BorderColor { top: acc.with_alpha(0.75), ..BorderColor::all(LINE) },
            ))
            .with_children(|band| {
                band.spawn(Node { column_gap: px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|t| {
                    t.spawn((Node { width: px(7.0), height: px(52.0), ..default() }, BackgroundColor(bar)));
                    t.spawn((Text::new(""), Decode::new(title.to_owned(), start, TITLE_DECODE_S), font(&faces.bold, 62.0 * k), TextColor(WHITE), nowrap()));
                });
                band.spawn((Text::new(reason), font(&faces.regular, 26.0 * k), TextColor(TEXT), nowrap()));
            });
            root.spawn(Node { height: px(22.0), ..default() });
            // The standings: ruled bands from the left edge.
            for (i, (place, pl)) in rows.iter().enumerate() {
                let mine = pl.num == me;
                let first = *place == 1;
                let row_start = start + 0.25 + i as f64 * 0.18;
                let h = if first { 56.0 } else { 48.0 };
                root.spawn(Node { width: px(INSET + 860.0), height: px(h), flex_shrink: 0.0, margin: UiRect { bottom: px(4.0), top: px(if *place > 3 { 12.0 } else { 0.0 }), ..default() }, ..default() }).with_children(|row| {
                    row.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            right: px(h * 0.5),
                            align_items: AlignItems::Center,
                            padding: UiRect::new(px(INSET), px(36.0), Val::Px(0.0), Val::Px(0.0)),
                            column_gap: px(18.0),
                            border: UiRect::top(px(1.0)),
                            ..default()
                        },
                        BorderColor::all(if mine { crate::shell::theme::ACCENT_SOFT } else { LINE }),
                        if mine { fade_right(acc.with_alpha(0.0), acc.with_alpha(0.85)) } else { fade_right(end.with_alpha(0.0), end) },
                    ))
                    .with_children(|r| {
                        r.spawn((Text::new(place_label(*place)), font(&faces.bold, 22.0 * k), TextColor(if first || mine { WHITE } else { TEXT }), Node { width: px(60.0), ..default() }, nowrap()));
                        r.spawn((
                            Text::new(""),
                            Decode::new(pl.name.clone(), row_start, NAME_DECODE_S),
                            font(if first || mine { &faces.bold } else { &faces.regular }, (if first { 30.0 } else { 26.0 }) * k),
                            TextColor(if mine || first { WHITE } else { TEXT.with_alpha(0.80) }),
                            nowrap(),
                            Node { flex_grow: 1.0, ..default() },
                        ));
                        r.spawn((
                            Text::new(""),
                            Decode::new(score_text(&outcome.gametype, pl.score), row_start + 0.1, SCORE_DECODE_S),
                            font(&faces.bold, (if first { 26.0 } else { 23.0 }) * k),
                            TextColor(WHITE),
                            nowrap(),
                        ));
                    });
                    bevel(row, h, if mine { acc.with_alpha(0.85) } else { end });
                });
            }
        });
}

pub(crate) fn register_outcome_hud(app: &mut App) {
    app.add_systems(Update, outcome_screen.in_set(frame::ClientSet::Ui));
}

pub mod preview {
    //! The end screen with stand-in results over a picture, saved as
    //! pictures (`cargo run -p ui --example hud_preview -- <out dir>
    //! outcome [size] [background.png]`).

    use std::path::PathBuf;

    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};

    use super::*;

    #[derive(Resource)]
    struct Plan {
        out: PathBuf,
        background: Option<PathBuf>,
        step: usize,
        frames: u32,
        at: f64,
    }

    #[derive(Component)]
    struct Shot;

    /// (file name, published result, who is looking).
    fn case(i: usize) -> Option<(&'static str, &'static str, i32)> {
        Some(match i {
            0 => ("01_ffa_defeat", "3|dm|MP_SCORE_LIMIT_REACHED|3,1500,simon;1,1450,egs;0,1200,reaaL;2,900,Jimbo;", 1),
            1 => ("02_ffa_victory", "3|dm|MP_SCORE_LIMIT_REACHED|3,1500,simon;1,1450,egs;0,1200,reaaL;2,900,Jimbo;", 3),
            2 => ("03_gun_game_fifth", "4|gun|MP_SCORE_LIMIT_REACHED|4,20,Sass;1,18,egs;0,17,reaaL;2,15,Jimbo;3,12,simon;", 3),
            3 => ("04_oic_time", "1|oic|MP_TIME_LIMIT_REACHED|1,9,egs;3,7,simon;0,4,JTAG;", 3),
            4 => ("05_br_victory", "0|br|SQUAD 1 WINS|0,1,smart elegant;1,1,EricTheTrigger;4,2,xXSharkXx;5,2,iiChiefii;8,3,Kevin;9,3,Pamer;", 0),
            6 => ("07_host_ended_ffa", "3|dm|MP_HOST_ENDED_GAME|3,1500,simon;1,1450,egs;0,1200,reaaL;2,900,Jimbo;", 1),
            7 => ("08_host_ended_team", "team:0/2|tdm|MP_HOST_ENDED_GAME|3,1500,simon;0,1450,egs;1,1200,reaaL;2,900,Jimbo;", 0),
            5 => (
                "06_br_eliminated",
                "4|br|SQUAD 2 WINS|4,1,xXSharkXx;5,1,iiChiefii;8,2,Kevin;9,2,Pamer;2,3,Crazy;3,3,Frosty;0,4,smart elegant;1,4,EricTheTrigger;",
                0,
            ),
            _ => return None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn step(
        mut commands: Commands,
        mut plan: ResMut<Plan>,
        fonts: Option<Res<ShellFonts>>,
        window: Query<&Window, With<bevy::window::PrimaryWindow>>,
        shots: Query<Entity, With<Shot>>,
        roots: Query<Entity, With<OutcomeRoot>>,
        mut lines: Query<(&mut Text, &mut Decode)>,
        mut images: ResMut<Assets<Image>>,
        time: Res<Time<Real>>,
        mut exit: MessageWriter<AppExit>,
    ) {
        let Some(fonts) = fonts else { return };
        let now = time.elapsed_secs_f64();
        // (The lines decode on the real clock; the shots wait them out.)
        for (mut text, mut d) in &mut lines {
            let t = (now - d.start) as f32;
            text.0 = d.read(t);
        }
        plan.frames += 1;
        if plan.frames < 40 || now - plan.at < 1.6 {
            return;
        }
        plan.frames = 0;
        let i = plan.step / 2;
        let Some((name, raw, me)) = case(i) else {
            exit.write(AppExit::Success);
            return;
        };
        let height = window.single().map_or(1080.0, Window::height);
        if plan.step % 2 == 1 {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.out.join(format!("{name}_{}p.png", height as u32))));
            plan.step += 1;
            return;
        }
        for e in shots.iter().chain(roots.iter()) {
            commands.entity(e).despawn();
        }
        if let Some(path) = &plan.background
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(img) = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::Default, Default::default())
        {
            commands.spawn((
                Shot,
                ImageNode::new(images.add(img)),
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                GlobalZIndex(0),
            ));
        }
        let faces = crate::hud_match::Faces { regular: fonts.regular.clone(), bold: fonts.bold.clone() };
        let outcome = parse_outcome(raw).expect("case");
        if outcome.gametype == "br" {
            spawn_br_screen(&mut commands, &faces, &fonts.black, (height / 1080.0).max(0.4), &outcome, me, now, None);
        } else if host_ended(&outcome.reason) {
            spawn_host_ended(&mut commands, &faces, (height / 1080.0).max(0.4), &outcome, me, now);
        } else {
            spawn_screen(&mut commands, &faces, &fonts.black, (height / 1080.0).max(0.4), &outcome, me, now);
        }
        plan.at = now;
        plan.step += 1;
    }

    pub fn install(app: &mut App, out: PathBuf, background: Option<PathBuf>) {
        app.add_systems(Startup, crate::shell::theme::load_fonts);
        app.insert_resource(Plan { out, background, step: 0, frames: 0, at: 0.0 });
        app.add_systems(Update, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_published_result() {
        let o = parse_outcome("3|dm|MP_SCORE_LIMIT_REACHED|3,1500,simon;1,1450,egs;0,1200,rea,aL;").unwrap();
        assert_eq!(o.winner, 3);
        assert_eq!(o.gametype, "dm");
        assert_eq!(o.placed.len(), 3);
        assert_eq!(o.placed[2], Placed { num: 0, score: 1200, name: "rea,aL".to_owned() });
        assert_eq!(reason_text(&o.reason), "Score limit reached");
        assert!(parse_outcome("").is_none());
    }

    #[test]
    fn host_ended_team_result_reads_by_side() {
        let o = parse_outcome("team:0/2|tdm|MP_HOST_ENDED_GAME|3,1500,simon;0,1450,egs;").unwrap();
        assert_eq!(o.winner, -2);
        assert_eq!(o.winners, vec![0, 2]);
        assert_eq!(title_of(&o, 2), Result::Victory);
        assert_eq!(title_of(&o, 3), Result::Defeat);
        assert_eq!(title_of(&parse_outcome("tie|tdm|MP_HOST_ENDED_GAME|").unwrap(), 3), Result::Draw);
        assert!(host_ended(&o.reason));
        assert!(!host_ended("MP_SCORE_LIMIT_REACHED"));
        assert_eq!(reason_text(&o.reason), "Host ended game");
        assert_eq!(title_of(&parse_outcome("3|dm|x|3,1,a;").unwrap(), 3), Result::Victory);
    }

    #[test]
    fn places_and_scores_read_as_words() {
        assert_eq!(place_label(1), "1st");
        assert_eq!(place_label(2), "2nd");
        assert_eq!(place_label(3), "3rd");
        assert_eq!(place_label(11), "11th");
        assert_eq!(place_label(22), "22nd");
        assert_eq!(score_text("gun", 18), "GUN 18");
        assert_eq!(score_text("oic", 9), "9");
        assert_eq!(reason_text("MP_TIME_LIMIT_REACHED"), "Time limit reached");
        assert_eq!(reason_text("MP_SOMETHING_ELSE"), "Something else");
    }

    #[test]
    fn his_row_follows_the_top_three() {
        let o = parse_outcome("1|dm|x|1,9,a;2,8,b;3,7,c;4,6,d;5,5,e;").unwrap();
        let rows = shown_rows(&o, 5);
        assert_eq!(rows.iter().map(|(p, r)| (*p, r.num)).collect::<Vec<_>>(), vec![(1, 1), (2, 2), (3, 3), (5, 5)]);
        assert_eq!(shown_rows(&o, 2).len(), 3);
    }

    #[test]
    fn resurgence_reads_by_squad() {
        let o = parse_outcome("4|br|SQUAD 2 WINS|4,1,a;5,1,b;8,2,c;9,2,d;2,3,e;3,3,f;0,4,me;1,4,g;").unwrap();
        assert!(!br_won(&o, 0));
        assert!(br_won(&o, 5));
        assert_eq!(br_title(&o, 0), "ELIMINATED \u{2014} #4");
        assert_eq!(br_title(&o, 5), "VICTORY");
        let shown = br_shown(&o, 0);
        assert_eq!(shown.iter().map(|(p, r)| (*p, r.len())).collect::<Vec<_>>(), vec![(1, 2), (2, 2), (3, 2), (4, 2)]);
        assert_eq!(br_shown(&o, 8).len(), 3);
    }

    #[test]
    fn lines_decode_in() {
        let mut rng = Rng::from_entropy();
        let (lock, extra) = plan_decode(5, 0.5, &mut rng);
        let mut g = || '7';
        // Whole at `full`, nothing but hidden digits at the start.
        assert_eq!(decoded("simon", 0.5, 0.5, &lock, extra, &mut g), "simon");
        let first = decoded("simon", 0.0, 0.5, &lock, extra, &mut g);
        assert!(first.chars().count() > 5 && first.chars().all(|c| c == '7'));
        // The line never reads shorter than its words, shrinks as letters lock.
        let mut last = usize::MAX;
        for k in 0..=50 {
            let s = decoded("simon", k as f32 * 0.01, 0.5, &lock, extra, &mut g);
            assert!(s.chars().count() >= 5);
            assert!(s.chars().count() <= last);
            last = s.chars().count();
        }
        assert_eq!(last, 5);
        // A locked letter is the real one, in its own place.
        let mid = decoded("simon", lock[2], 0.5, &lock, extra, &mut g);
        assert_eq!(mid.chars().nth(2), Some('m'));
        assert_eq!(decoded("", 0.1, 0.5, &[], 6, &mut g).chars().count(), 0);
    }

    #[test]
    fn the_order_is_random_each_time() {
        let mut rng = Rng::from_entropy();
        let plans: Vec<Vec<f32>> = (0..4).map(|_| plan_decode(16, 1.5, &mut rng).0).collect();
        assert!(plans.windows(2).all(|w| w[0] != w[1]));
        for plan in &plans {
            assert!(plan.iter().all(|t| (0.15..=1.5).contains(t)));
            assert!(plan.iter().any(|t| *t == 1.5));
        }
        // Two sources, two streams.
        let (mut a, mut b) = (Rng::from_entropy(), Rng::from_entropy());
        assert_ne!((0..4).map(|_| a.next_u64()).collect::<Vec<_>>(), (0..4).map(|_| b.next_u64()).collect::<Vec<_>>());
        // Two lines of the same words plan alike only by a 1-in-billions chance.
        let (x, y) = (Decode::new("Jimbo99".into(), 0.0, 1.5), Decode::new("Jimbo99".into(), 0.0, 1.5));
        assert!(x.lock != y.lock);
    }

    #[test]
    fn hidden_characters_change_and_are_digits() {
        let mut d = Decode::new("abcdefgh".into(), 0.0, 1.5);
        let a = d.read(0.0);
        assert_eq!(d.read(0.01), a, "same tick, same characters");
        let seen: std::collections::HashSet<String> = (0..20).map(|k| d.read(k as f32 / SCRAMBLE_HZ)).collect();
        assert!(seen.len() > 10);
        assert!(a.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(d.read(1.5), "abcdefgh");
    }
}

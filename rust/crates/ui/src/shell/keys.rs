//! bo2mp shell: OPTIONS > MOUSE & KEYBOARD > KEY BINDINGS: Black Ops II's
//! PC controls page in the Ghosts look. A ruled row an action (grouped
//! under MOVEMENT, COMBAT and INTERFACE), its keys at the right; Enter or a
//! click listens for a key (PRESS A KEY: Esc cancels, Delete clears), Delete
//! on a row clears it, RESET TO DEFAULT brings Black Ops II's own keys back.
//!
//! His keys are the console's `KeyBinds` (the same ones `bind` and the old
//! LUI menus change, saved with his settings in `settings.cfg`): the page
//! only asks (`frame::UiBindEdit`, carried out by
//! `console::user_settings::consume_menu_binding`) and reads them back
//! through `crate::BindingView` (`sync_keys`, the one system that touches
//! either). A key another action had is taken from it; when that leaves it
//! none it gets the old key in its place (a swap), and a note says so.

use bevy::prelude::*;

use super::{Act, Row};

/// The page's number (past the tabs and ADVANCED VIDEO).
pub(crate) const KEYS_PAGE: usize = super::options::ADVANCED_PAGE + 1;
pub(crate) const KEYS_TITLE: &str = "KEY BINDINGS";

/// The sections, in order.
pub(crate) const SECTIONS: [&str; 3] = ["MOVEMENT", "COMBAT", "INTERFACE"];

/// An action: its section (in `SECTIONS`), its name, its command, a line
/// about it.
pub(crate) struct Action {
    pub section: usize,
    pub label: &'static str,
    pub command: &'static str,
    pub sub: &'static str,
}

const fn a(section: usize, label: &'static str, command: &'static str, sub: &'static str) -> Action {
    Action { section, label, command, sub }
}

/// The actions the page lists (the commands of `default_controls.cfg`, Black
/// Ops II's PC keys, and a few more he may want on a key).
pub(crate) const ACTIONS: [Action; 27] = [
    a(0, "FORWARD", "+forward", "Move forward."),
    a(0, "BACKPEDAL", "+back", "Move backward."),
    a(0, "MOVE LEFT", "+moveleft", "Strafe left."),
    a(0, "MOVE RIGHT", "+moveright", "Strafe right."),
    a(0, "JUMP / STAND", "+gostand", "Jump, or stand up from crouch or prone."),
    a(0, "CHANGE STANCE", "+stance", "Tap to crouch, hold to go prone."),
    a(0, "CROUCH", "togglecrouch", "Crouch, or stand back up."),
    a(0, "PRONE", "toggleprone", "Go prone, or get back up."),
    a(0, "SPRINT / HOLD BREATH", "+breath_sprint", "Sprint; while aiming a scope, steady it."),
    a(1, "FIRE WEAPON", "+attack", "Fire."),
    a(1, "AIM DOWN THE SIGHT", "+speed_throw", "Aim while held."),
    a(1, "TOGGLE AIM DOWN THE SIGHT", "+toggleads_throw", "Aim on one press, stop on the next."),
    a(1, "MELEE ATTACK", "+melee", "Knife."),
    a(1, "RELOAD", "+reload", "Reload."),
    a(1, "USE", "+activate", "Pick up, plant, defuse, open, buy."),
    a(1, "SWITCH WEAPON", "+weapnext_inventory", "Tap: the next weapon. Hold: the inventory weapon."),
    a(1, "PREVIOUS WEAPON", "weapprev", "The weapon before."),
    a(1, "LETHAL", "+frag", "Throw or use the lethal equipment."),
    a(1, "TACTICAL", "+smoke", "Throw or use the tactical equipment."),
    a(1, "ACTION SLOT 1", "+actionslot 1", "The d-pad's up slot: scorestreaks and equipment as the class has them."),
    a(1, "ACTION SLOT 2", "+actionslot 2", "The d-pad's down slot."),
    a(1, "ACTION SLOT 3", "+actionslot 3", "The d-pad's left slot."),
    a(1, "ACTION SLOT 4", "+actionslot 4", "The d-pad's right slot."),
    a(2, "SCOREBOARD", "+scores", "Show the scores while held."),
    a(2, "CHAT", "chatmodepublic", "Say something to everyone."),
    a(2, "TEAM CHAT", "chatmodeteam", "Say something to your team."),
    a(2, "VOICE CHAT", "+talk", "Talk while held."),
];

/// What a row with no key shows.
pub(crate) const UNBOUND: &str = "UNBOUND";
pub(crate) const PRESS: &str = "PRESS A KEY";

/// His keys as the page shows them, as of this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Keys {
    /// Command -> its keyboard and mouse keys (`w OR UPARROW`).
    pub keys: std::collections::BTreeMap<String, String>,
    /// The command listening for a key.
    pub listening: Option<String>,
}

static NOW: std::sync::Mutex<Option<Keys>> = std::sync::Mutex::new(None);
static EDITS: std::sync::Mutex<Vec<frame::UiBindEdit>> = std::sync::Mutex::new(Vec::new());
/// A note the page owes him (an action whose key was taken).
static NOTE: std::sync::Mutex<Option<(String, bool)>> = std::sync::Mutex::new(None);

/// While he presses a key for a row: since when, and whether the console
/// has begun listening.
#[derive(Clone, Debug)]
struct Capture {
    since: f64,
    seen: bool,
}
static CAPTURE: std::sync::Mutex<Option<Capture>> = std::sync::Mutex::new(None);

pub(crate) fn snapshot() -> Keys {
    NOW.lock().ok().and_then(|k| k.clone()).unwrap_or_default()
}

fn push(edit: frame::UiBindEdit) {
    if let Ok(mut q) = EDITS.lock() {
        q.push(edit);
    }
}

/// A key's name as the page shows it (upper case, the console's names
/// otherwise: `w OR UPARROW` -> `W OR UPARROW`).
pub(crate) fn shown(keys: &str) -> String {
    keys.to_uppercase()
}

/// The page's rows: an action a row (its keys, or UNBOUND, or PRESS A KEY
/// while it listens), then RESET TO DEFAULT.
pub(crate) fn rows(k: &Keys) -> Vec<Row> {
    let mut r: Vec<Row> = ACTIONS
        .iter()
        .enumerate()
        .map(|(i, act)| {
            let value = if k.listening.as_deref() == Some(act.command) {
                PRESS.to_owned()
            } else {
                k.keys.get(act.command).map_or_else(|| UNBOUND.to_owned(), |v| shown(v))
            };
            Row {
                label: act.label.to_owned(),
                sub: format!("{}\n\nEnter or click: press a new key (Esc cancels, Delete clears). Delete: clear it.", act.sub),
                value: Some(value),
                act: Act::Rebind(i),
            }
        })
        .collect();
    r.push(Row {
        label: "RESET TO DEFAULT".to_owned(),
        sub: "Black Ops II's own PC keys back for every action (the controller's buttons stay as they are).".to_owned(),
        value: None,
        act: Act::ResetKeys,
    });
    r
}

/// The section a row is under (none for RESET TO DEFAULT).
pub(crate) fn section_of(row: usize) -> Option<usize> {
    ACTIONS.get(row).map(|a| a.section)
}

/// Listen for a key for an action (Enter or a click on its row).
pub(crate) fn listen(i: usize, now: f64) {
    let Some(act) = ACTIONS.get(i) else { return };
    push(frame::UiBindEdit::Listen(act.command.to_owned()));
    if let Ok(mut l) = LAST_LISTEN.lock() {
        *l = Some(act.command.to_owned());
    }
    if let Ok(mut c) = CAPTURE.lock() {
        *c = Some(Capture { since: now, seen: false });
    }
}

/// Clear an action's keys (Delete on its row).
pub(crate) fn clear(i: usize) -> Option<&'static str> {
    let act = ACTIONS.get(i)?;
    push(frame::UiBindEdit::Clear(act.command.to_owned()));
    Some(act.label)
}

pub(crate) fn reset() {
    push(frame::UiBindEdit::ResetKeys);
}

/// A note the page owes him, once (asked for once the key is let go, by
/// when his keys as shown have caught up).
pub(crate) fn take_note() -> Option<String> {
    let (command, swapped) = NOTE.lock().ok().and_then(|mut n| n.take())?;
    Some(taken_note(&command, swapped, &snapshot()))
}

/// The note for a key taken from another action (`swapped`: it got the old
/// key in its place).
pub(crate) fn taken_note(command: &str, swapped: bool, keys: &Keys) -> String {
    let name = ACTIONS.iter().find(|a| a.command == command).map_or_else(|| command.to_uppercase(), |a| a.label.to_owned());
    if swapped {
        let now = keys.keys.get(command).map_or_else(|| UNBOUND.to_owned(), |v| shown(v));
        format!("Swapped: {name} is now on {now}.")
    } else if keys.keys.contains_key(command) {
        format!("Key taken from {name} (it keeps its other key).")
    } else {
        format!("Key taken from {name}: it is UNBOUND now.")
    }
}

/// Whether the page has the keys this frame: from Enter or a click on a row
/// until the console has a key (or Esc), and then until every key and button
/// he held is let go (so the key he bound does not also work the menu).
/// `held`: any key or mouse button down.
pub(crate) fn capturing(now: f64, held: bool) -> bool {
    let listening = snapshot().listening.is_some();
    let Ok(mut c) = CAPTURE.lock() else { return false };
    let Some(cap) = c.as_mut() else { return false };
    if listening {
        cap.seen = true;
        return true;
    }
    let busy = if cap.seen { held } else { now - cap.since < 1.0 };
    if !busy {
        *c = None;
    }
    busy
}

/// The one system that talks to the console's binds: sends the page's
/// edits, and reads his keys back for the page.
pub(crate) fn sync_keys(view: Option<ResMut<crate::BindingView>>, mut edits: MessageWriter<frame::UiBindEdit>) {
    for edit in EDITS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default() {
        edits.write(edit);
    }
    let Some(mut view) = view else { return };
    // (The command listening, as the page asked for it: the view holds its
    // number only; another menu's listening is not the page's.)
    let ours = CAPTURE.lock().is_ok_and(|c| c.is_some());
    let listening = if view.listening.is_some() && ours { LAST_LISTEN.lock().ok().and_then(|l| l.clone()) } else { None };
    let keys = Keys { keys: view.keys.clone(), listening };
    if view.taken.is_some()
        && let Some((command, swapped)) = view.taken.take()
        && let Ok(mut n) = NOTE.lock()
    {
        *n = Some((command, swapped));
    }
    if let Ok(mut g) = NOW.lock()
        && g.as_ref() != Some(&keys)
    {
        *g = Some(keys);
    }
}

/// The command the page last asked a key for.
static LAST_LISTEN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_show_keys_and_listening() {
        let mut k = Keys::default();
        k.keys.insert("+forward".to_owned(), "w OR UPARROW".to_owned());
        let r = rows(&k);
        assert_eq!(r.len(), ACTIONS.len() + 1);
        assert_eq!(r[0].value.as_deref(), Some("W OR UPARROW"));
        assert_eq!(r[1].value.as_deref(), Some(UNBOUND));
        assert_eq!(r.last().map(|r| r.act), Some(Act::ResetKeys));
        k.listening = Some("+back".to_owned());
        assert_eq!(rows(&k)[1].value.as_deref(), Some(PRESS));
        assert_eq!(section_of(0), Some(0));
        assert_eq!(section_of(ACTIONS.len()), None);
    }

    /// The sections run in order (each a single run of rows), every command
    /// once.
    #[test]
    fn sections_in_order() {
        for w in ACTIONS.windows(2) {
            assert!(w[0].section <= w[1].section, "{} after {}", w[1].label, w[0].label);
        }
        assert!(ACTIONS.iter().all(|a| a.section < SECTIONS.len()));
        let mut seen = std::collections::HashSet::new();
        assert!(ACTIONS.iter().all(|a| seen.insert(a.command)));
    }

    #[test]
    fn notes_for_taken_keys() {
        let mut k = Keys::default();
        k.keys.insert("+frag".to_owned(), "g".to_owned());
        assert_eq!(taken_note("+frag", true, &k), "Swapped: LETHAL is now on G.");
        assert_eq!(taken_note("+frag", false, &k), "Key taken from LETHAL (it keeps its other key).");
        assert_eq!(taken_note("+smoke", false, &k), "Key taken from TACTICAL: it is UNBOUND now.");
    }
}

//! bo2mp shell: a custom game with friends over the internet and no server.
//! PLAY ONLINE WITH FRIENDS opens his lobby on his own PC
//! (`net::DirectHost`) and puts its invite code on the clipboard; a friend
//! copies the code and picks JOIN WITH CODE, and waits in the host's room
//! until he starts the match. START MATCH plays it with everyone in.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;

use super::{Act, Row, Screen, Shell};
use crate::online_match::{self, P2pView};

static COPY: Mutex<Option<String>> = Mutex::new(None);
static PASTE: AtomicBool = AtomicBool::new(false);

/// He hosts a peer-to-peer custom game now.
pub(crate) fn hosting() -> bool {
    online_match::p2p_view().is_some()
}

/// The custom lobby's online rows, under START MATCH.
pub(crate) fn lobby_rows() -> Vec<Row> {
    match online_match::p2p_view() {
        None => vec![Row::new(
            "PLAY ONLINE WITH FRIENDS",
            "Opens this lobby to your friends over the internet, no server needed. You get an invite code to send them.",
            Act::P2pHost,
        )],
        Some(v) => vec![
            Row::new("COPY INVITE CODE", &status(&v), Act::P2pCopy),
            Row::new("CLOSE ONLINE LOBBY", "Your friends leave; the lobby is yours alone again.", Act::P2pClose),
        ],
    }
}

/// JOIN WITH CODE, on the Multiplayer menu.
pub(crate) fn join_row() -> Row {
    Row::new(
        "JOIN WITH CODE",
        "Copy a friend's invite code (from his PLAY ONLINE WITH FRIENDS), then pick this to join his custom game.",
        Act::P2pJoin,
    )
}

fn status(v: &P2pView) -> String {
    let Some(code) = &v.invite else {
        return "Getting your invite code...".to_owned();
    };
    let reach = match (v.opened, v.no_public) {
        (_, true) => format!(
            "Your public address could not be found: friends on your network can join; others may need UDP port {} forwarded.",
            v.port
        ),
        (Some(how), false) => format!("Your router opened UDP port {} ({how}).", v.port),
        (None, false) => format!("If a friend cannot get in, forward UDP port {} on your router.", v.port),
    };
    let friends = if v.friends.is_empty() { "No friends in yet.".to_owned() } else { format!("In: {}.", v.friends.join(", ")) };
    format!("{code}  ·  {friends} {reach}")
}

pub(crate) fn activate(shell: &mut Shell, act: &Act, now: f64) {
    match act {
        Act::P2pHost => {
            online_match::p2p_host(shell.map.clone(), shell.mode.clone());
            if let Ok(mut c) = COPY.lock() {
                *c = Some(String::new());
            }
            shell.note("Opening your lobby: the invite code goes on your clipboard.", now);
        }
        Act::P2pCopy => match online_match::p2p_view().and_then(|v| v.invite) {
            Some(code) => {
                if let Ok(mut c) = COPY.lock() {
                    *c = Some(code);
                }
                shell.note("Invite code copied: paste it to your friends.", now);
            }
            None => shell.note("Still getting your invite code...", now),
        },
        Act::P2pClose => {
            online_match::queue_leave();
            shell.note("Online lobby closed.", now);
        }
        Act::P2pJoin => PASTE.store(true, Ordering::Relaxed),
        _ => {}
    }
}

/// The clipboard, the lobby's map and mode sent to his room, and notes.
pub(crate) fn tick(
    mut shell: ResMut<Shell>,
    clipboard: Option<ResMut<bevy::prelude::Clipboard>>,
    time: Res<Time>,
    mut sent: Local<Option<(String, String)>>,
) {
    let now = time.elapsed_secs_f64();
    if let Some(why) = online_match::take_p2p_note() {
        shell.note(&why, now);
    }
    let view = online_match::p2p_view();
    // (The code goes on the clipboard once it is ready.)
    if let Some(mut clipboard) = clipboard {
        let want = COPY.lock().ok().and_then(|c| c.clone());
        match (want, view.as_ref().and_then(|v| v.invite.clone())) {
            (Some(w), _) if !w.is_empty() => {
                if clipboard.set_text(w).is_err() {
                    shell.note("The clipboard could not be written.", now);
                }
                if let Ok(mut c) = COPY.lock() {
                    *c = None;
                }
            }
            (Some(_), Some(code)) => {
                let _ = clipboard.set_text(code);
                shell.note("Lobby open: your invite code is on the clipboard.", now);
                if let Ok(mut c) = COPY.lock() {
                    *c = None;
                }
            }
            _ => {}
        }
        if PASTE.swap(false, Ordering::Relaxed) {
            let mut read = clipboard.fetch_text();
            match read.poll_result() {
                Some(Ok(text)) => match net::Invite::decode(&text) {
                    Ok(_) => {
                        online_match::p2p_join(text);
                        shell.note("Connecting to your friend...", now);
                    }
                    Err(e) => {
                        let e = e.strip_prefix("that ").unwrap_or(e);
                        shell.note(&format!("The clipboard holds no invite code ({e})."), now);
                    }
                },
                _ => shell.note("Copy your friend's invite code first.", now),
            }
        }
    }
    // (His room follows the lobby's map and mode.)
    match &view {
        Some(_) if shell.screen == Screen::Lobby => {
            let now_pick = (shell.map.clone(), shell.mode.clone());
            if sent.as_ref() != Some(&now_pick) {
                if sent.is_some() {
                    online_match::p2p_update(now_pick.0.clone(), now_pick.1.clone());
                }
                *sent = Some(now_pick);
            }
        }
        Some(_) => {}
        None => *sent = None,
    }
}

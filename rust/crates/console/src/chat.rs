//! bo2mp: text chat's typing line. `chatmodepublic` (T) / `chatmodeteam` (Y)
//! open it (`plugin::publish_client_action_input` sees the bound key); while
//! it is open the keyboard types into it and the game takes no keys. Enter
//! sends it to the server as client commands (`say` / `say_team`, longer
//! lines in `say+` pieces: a command carries 48 bytes), Escape drops it.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use frame::{CHAT_TYPED_MAX, ChatTyping};
use net::{ClientActionInbox, LocalPresentClient};
use sim::ClientAction;

use crate::ConsoleState;

/// Typing into the open chat line, and sending it.
pub(crate) fn handle_chat_input(
    mut events: MessageReader<KeyboardInput>,
    console: Res<ConsoleState>,
    mut chat: ResMut<ChatTyping>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    inbox: Option<ResMut<ClientActionInbox>>,
    seq: Option<ResMut<net::ActionRequestIds>>,
    local: Option<Res<LocalPresentClient>>,
) {
    let pressed: Vec<KeyboardInput> = events.read().filter(|e| e.state.is_pressed()).cloned().collect();
    if console.open && chat.open {
        close(&mut chat);
    }
    if !chat.open {
        return;
    }
    let mut send = false;
    for e in &pressed {
        match &e.logical_key {
            Key::Escape => {
                close(&mut chat);
                break;
            }
            Key::Enter => {
                send = true;
                break;
            }
            Key::Backspace => {
                chat.text.pop();
            }
            _ => {
                if let Some(t) = &e.text {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if chat.text.chars().count() < CHAT_TYPED_MAX {
                            chat.text.push(c);
                        }
                    }
                }
            }
        }
    }
    // (Nothing else hears the keys typed: the pause menu's Escape, the
    // Enter, shortcuts.)
    keys.reset_all();
    if !send {
        return;
    }
    let text = chat.text.trim().to_owned();
    let team = chat.team;
    close(&mut chat);
    if text.is_empty() {
        return;
    }
    let (Some(mut inbox), Some(mut seq), Some(local)) = (inbox, seq, local) else {
        return;
    };
    let Some(menu) = sim::menu_response_field(sim::CLIENT_COMMAND_MENU) else {
        return;
    };
    for line in frame::chat_commands(&text, team, sim::MENU_RESPONSE_BYTES) {
        let Some(response) = sim::menu_response_field(&line) else {
            continue;
        };
        let request_id = seq.allocate();
        if let Err(error) = inbox.push(local.0, ClientAction::MenuResponse { request_id, menu, response }) {
            diag::warn!(Console, "chat: {error}");
            return;
        }
    }
}

fn close(chat: &mut ChatTyping) {
    chat.open = false;
    chat.text.clear();
}

/// The command ids of `chatmodepublic` and `chatmodeteam`.
pub(crate) fn chat_mode_ids() -> (u32, u32) {
    (
        input_iw4::command_id_lookup("chatmodepublic").unwrap_or(u32::MAX),
        input_iw4::command_id_lookup("chatmodeteam").unwrap_or(u32::MAX),
    )
}

//! bo2mp: text chat shared between the input side (the typing line) and the
//! screen / speech side (the lines received).

use bevy::prelude::*;

/// The chat line being typed (`chatmodepublic` / `chatmodeteam`): while
/// `open` the keyboard types into `text` and the game takes no keys.
#[derive(Resource, Clone, Debug, Default)]
pub struct ChatTyping {
    pub open: bool,
    /// `say_team` rather than `say`.
    pub team: bool,
    pub text: String,
    /// The frame it opened (that frame's key press is not typed).
    pub opened_frame: u32,
}

/// The longest chat line typed (characters; the server keeps the same).
pub const CHAT_TYPED_MAX: usize = 150;

/// A chat line has reached this client (written once per line, as it is
/// first seen). Read with `MessageReader<ChatLineReceived>`.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct ChatLineReceived {
    /// The sender's name as shown.
    pub sender: String,
    pub text: String,
    pub team_only: bool,
    pub from_local_player: bool,
}

/// Registers the chat's resource and message once, however many plugins
/// ask (it is called directly, never added as a plugin: a function added
/// twice as a plugin is an error that stops the game at start).
pub fn register_chat(app: &mut App) {
    if app.world().contains_resource::<ChatTyping>() {
        return;
    }
    app.init_resource::<ChatTyping>().add_message::<ChatLineReceived>();
}

/// The commands that send `text` to the server, each at most `max` bytes:
/// leading `say+ <piece>` commands for a line too long for one, then
/// `say <rest>` / `say_team <rest>`.
pub fn chat_commands(text: &str, team: bool, max: usize) -> Vec<String> {
    let last = if team { "say_team " } else { "say " };
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() + last.len() > max {
        let budget = max - "say+ ".len();
        let mut cut = budget.min(rest.len());
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(format!("say+ {}", &rest[..cut]));
        rest = &rest[cut..];
    }
    out.push(format!("{last}{rest}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_line_is_one_command() {
        assert_eq!(chat_commands("hello there", false, 48), vec!["say hello there"]);
        assert_eq!(chat_commands("gg", true, 48), vec!["say_team gg"]);
    }

    #[test]
    fn a_long_line_is_split_into_pieces_that_join_back() {
        let text = "é".repeat(40) + " and some more words to go past the limit";
        let cmds = chat_commands(&text, true, 48);
        assert!(cmds.len() > 1);
        assert!(cmds.iter().all(|c| c.len() <= 48));
        let joined: String = cmds
            .iter()
            .map(|c| c.strip_prefix("say+ ").or_else(|| c.strip_prefix("say_team ")).unwrap())
            .collect();
        assert_eq!(joined, text);
        assert!(cmds.last().unwrap().starts_with("say_team "));
    }
}

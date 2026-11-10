use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct UiBindingCapture {
    pub command: Option<String>,
    pub consumed_input: bool,
}

#[derive(Message, Clone, Debug)]
pub struct UiBindRequest {
    pub command: String,
}

/// bo2mp: the front end's OPTIONS > MOUSE & KEYBOARD > KEY BINDINGS page
/// (`console::user_settings::consume_menu_binding` carries it out on his
/// `KeyBinds`, saved with his settings).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum UiBindEdit {
    /// Listen for a keyboard or mouse key for this command (Esc cancels,
    /// Delete clears; a key another command had is taken from it, and that
    /// command gets this one's old key when it would be left with none).
    Listen(String),
    /// Clear the command's keyboard and mouse keys.
    Clear(String),
    /// Black Ops II's own PC keys back (`default_controls.cfg`); the pad's
    /// buttons are left as they are.
    ResetKeys,
}

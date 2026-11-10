//! bo2mp: stand-in for the small copy of Modern Warfare III (2023) (its
//! guns, their sounds and the cards) when the game is built without the
//! `jup` feature (owner, 5 Nov 2026: MW3's guns and maps are left out; its
//! emblems and calling cards stay, `jup_cards`).

pub(crate) mod jup_pack {
    use std::path::Path;

    const LEFT_OUT: &str = "Modern Warfare III's (2023) guns are not in this build";

    pub(crate) fn take_notes() -> Vec<(String, bool)> {
        Vec::new()
    }
    pub(crate) fn running() -> bool {
        false
    }
    pub(crate) fn start() -> String {
        LEFT_OUT.to_owned()
    }
    pub fn export_from_command_line(_: &Path) -> Result<String, String> {
        Err(LEFT_OUT.to_owned())
    }
}

//! bo2mp: tells the audio crate's music director which front-end playlist
//! the shell's screen wants — Zombies' (every screen of the Zombies front
//! end, the shared ones opened from it included: `Shell::in_zombies`), the
//! custom-game lobby (the lobby, its map and mode pickers, and
//! Create-a-Class opened from there) or the main screens.
//! Read-only over the shell; the shell itself is not touched.

use bevy::prelude::*;
use frame::{ClientSet, MenuMusicScreen};

use crate::shell::{Screen, Shell};

fn music_screen_of(shell: &Shell) -> MenuMusicScreen {
    let lobby = |s: Screen| matches!(s, Screen::Lobby | Screen::MapPick | Screen::ModePick);
    if shell.in_zombies() {
        return MenuMusicScreen::Zombies;
    }
    match shell.screen {
        s if lobby(s) => MenuMusicScreen::Lobby,
        Screen::Classes | Screen::ClassEdit | Screen::ItemPick if lobby(shell.cac_from) => {
            MenuMusicScreen::Lobby
        }
        _ => MenuMusicScreen::Main,
    }
}

fn publish_music_screen(shell: Option<Res<Shell>>, mut out: ResMut<MenuMusicScreen>) {
    let next = shell.map_or(MenuMusicScreen::Main, |shell| music_screen_of(&shell));
    if *out != next {
        *out = next;
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<MenuMusicScreen>()
        .add_systems(Update, publish_music_screen.in_set(ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(screen: Screen, from: Screen) -> MenuMusicScreen {
        music_screen_of(&Shell {
            screen,
            cac_from: from,
            ..Shell::default()
        })
    }

    #[test]
    fn the_zombies_front_end_asks_for_zombies_music() {
        for s in [Screen::Zombies, Screen::ZmMapPick, Screen::ZmStartPick, Screen::ZmLobby, Screen::ZmRecord] {
            assert_eq!(at(s, Screen::Main), MenuMusicScreen::Zombies, "{s:?}");
        }
        for s in [Screen::Options, Screen::Barracks, Screen::CardPick, Screen::EmblemPick, Screen::Classes, Screen::ClassEdit] {
            assert_eq!(at(s, Screen::Zombies), MenuMusicScreen::Zombies, "{s:?} from Zombies");
            assert_eq!(at(s, Screen::Main), MenuMusicScreen::Main, "{s:?} from the main menu");
        }
        // Back on the main menu: the menu playlist again.
        assert_eq!(at(Screen::Main, Screen::Zombies), MenuMusicScreen::Main);
        assert_eq!(at(Screen::Lobby, Screen::Zombies), MenuMusicScreen::Lobby);
        assert_eq!(at(Screen::Classes, Screen::Lobby), MenuMusicScreen::Lobby);
    }
}

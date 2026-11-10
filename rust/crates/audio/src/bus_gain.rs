//! bo2zm: Black Ops II's Sound sliders as gains on its sound buses. Each
//! alias names its bus in flags bits 11-14 (measured on Nuketown's banks:
//! 1 game, 2 voice, 4 hdr effects, 5 menu, 6 music; 7 is the movie bus):
//! music and voice have their own sliders, the movie bus the cinematics
//! one, everything else is effects. The master slider stays the global
//! volume.

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;

/// Music, effects, voice, cinematics (f32 bits; 1.0 until set).
static GAINS: [AtomicU32; 4] = [const { AtomicU32::new(0x3f80_0000) }; 4];
const MUSIC: usize = 0;
const SFX: usize = 1;
const VOICE: usize = 2;
const MOVIE: usize = 3;

fn gain(i: usize) -> f32 {
    f32::from_bits(GAINS[i].load(Ordering::Relaxed))
}

/// The music slider.
pub(crate) fn music() -> f32 {
    gain(MUSIC)
}

/// The effects slider.
pub(crate) fn sfx() -> f32 {
    gain(SFX)
}

/// The menu music's base gain (the front end, lobby and Zombies front end):
/// the reference every other music level sits under.
pub const MENU_MUSIC_GAIN: f32 = 1.0;

/// In-match music's base gain, of the menu music's: Black Ops II's match
/// music is quiet background under gunfire, footsteps and voices. It goes
/// on every in-match music voice (the director's match playlist and map
/// intro, every game's match music aliases: BO2's music bus, Black Ops'
/// `mus_*`, Modern Warfare's `*_music` team cues, Zombies' bed), under the
/// music slider and master, which still scale it.
pub const MATCH_MUSIC_GAIN: f32 = 0.2;

/// The music slider for in-match music.
pub(crate) fn match_music() -> f32 {
    music() * MATCH_MUSIC_GAIN
}

/// Whether an alias played in a match is music (any game). BO2's own names
/// its bus in its flags (6 = music); the others go by their names: Black
/// Ops' `mus_*`/`music_*`, Modern Warfare's `<team>spawn_music` etc.
pub(crate) fn is_music_alias(t6_flags: Option<u32>, name: &str) -> bool {
    if let Some(f) = t6_flags {
        return (f >> 11) & 0xf == 6;
    }
    let lower = name.to_ascii_lowercase();
    lower.starts_with("mus_") || lower.starts_with("music_") || lower.ends_with("_music") || lower.contains("_mus_")
}

/// A Black Ops II alias's slider, from its flags.
pub(crate) fn t6_alias(flags: Option<u32>) -> f32 {
    match flags.map(|f| (f >> 11) & 0xf) {
        Some(6) => gain(MUSIC),
        Some(2) => gain(VOICE),
        Some(7) => gain(MOVIE),
        Some(_) => gain(SFX),
        None => 1.0,
    }
}

/// His settings into the gains, every frame (cheap).
pub(crate) fn sync(settings: Option<Res<frame::GameSettings>>) {
    let Some(s) = settings else {
        return;
    };
    for (i, v) in [s.music_volume, s.sfx_volume, s.voice_volume, s.cinematic_volume]
        .into_iter()
        .enumerate()
    {
        GAINS[i].store(v.to_bits(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_music_sits_well_under_the_menu_music() {
        const { assert!(MATCH_MUSIC_GAIN > 0.0 && MATCH_MUSIC_GAIN < MENU_MUSIC_GAIN) };
        let share = MATCH_MUSIC_GAIN / MENU_MUSIC_GAIN;
        assert!((0.15..=0.25).contains(&share), "{share}");
        // The music slider still scales it (1.0 until set).
        assert!((match_music() - MATCH_MUSIC_GAIN * music()).abs() < 1e-6);
    }

    #[test]
    fn music_aliases_of_every_game_are_found() {
        assert!(is_music_alias(Some(6 << 11), "mus_mp_spawn"));
        assert!(!is_music_alias(Some(1 << 11), "mus_looking_name"));
        assert!(is_music_alias(None, "mus_mp_victory"));
        assert!(is_music_alias(None, "US_spawn_music"));
        assert!(is_music_alias(None, "music_mainmenu_mp"));
        assert!(!is_music_alias(None, "weap_ak47_fire_plr"));
        assert!(!is_music_alias(None, "US_mp_cmd_suppressfire"));
    }
}

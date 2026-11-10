//! bo2mp: the players' calling cards and emblems in the match HUD (the kill
//! card, the Killed By card, the killcam's name card). Black Ops II's
//! banners and emblems are not shown: each player has a Modern Warfare 2
//! or 3 calling card (`cardtitle_*`) and emblem (`cardicon_*`), read from
//! that game's own files (`iw4:<material>` / `iw5:<material>`), or a Modern
//! Warfare III (2023) one (`jup:<name>`, its first frame here). His own are
//! the ones his stats hold (`bo2mp.callingcard` / `bo2mp.emblem`, picked in
//! the shell's Barracks); anyone else's follow from the name (the shell's
//! `cards::for_name`), so a player keeps his for good. A picture the games
//! do not have leaves the plain drawn card (never Black Ops II's).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::shell::cards::{self, Kind};

/// His calling card and emblem as his stats hold them (empty: none).
pub(crate) fn his_look_stats(profile: &bo2_profile::Profile) -> (String, String) {
    let read = |key: &str| match profile.stat(key) {
        Some(bo2_profile::StatValue::Str(t)) => t.trim().to_owned(),
        _ => String::new(),
    };
    (read(cards::CARD_STAT), read(cards::EMBLEM_STAT))
}

/// A player's calling card and emblem (`iw4:cardtitle_*`, `iw4:cardicon_*`):
/// `mine` holds his own stats' values when the player is him.
pub(crate) fn look_of(name: &str, mine: Option<&(String, String)>) -> (String, String) {
    let look = match mine {
        Some((card, emblem)) => (cards::mine(Kind::Card, card), cards::mine(Kind::Emblem, emblem)),
        None => (cards::for_name(Kind::Card, name), cards::for_name(Kind::Emblem, name)),
    };
    // (Once per player and look: which materials he got.)
    static SAID: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let line = format!("{name} -> card {} emblem {}{}", look.0, look.1, if mine.is_some() { " (his own)" } else { "" });
    if let Ok(mut said) = SAID.lock()
        && said.get_or_insert_with(HashSet::new).insert(line.clone())
    {
        diag::info!(Ui, "bo2mp hud: look: {line}");
    }
    look
}

/// A calling card's or emblem's picture (`iw4:` / `iw5:` / `t5:` / `t6:` material), read
/// off the main thread from its game: `None` while it is read, and for good
/// when the game lacks it (the reader logs which).
pub(crate) fn picture(material: &str) -> Option<Arc<Image>> {
    let (game, name) = material.split_once(':')?;
    if !is_look(material) {
        return None;
    }
    let (game, name) = (game.to_owned(), name.to_owned());
    let img = crate::foreign_maps::picture_async(material, true, move || {
        let img = crate::foreign_maps::read_game_image(&game, &[name])?;
        Some(if game == crate::bo_cards::BO1 || game == crate::bo_cards::BO2 { crate::bo_cards::shape_to_cover(img) } else { img })
    })?;
    // (Once per material: the first time it is in.)
    static READ: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    if let Ok(mut read) = READ.lock()
        && read.get_or_insert_with(HashSet::new).insert(material.to_owned())
    {
        diag::info!(Ui, "bo2mp hud: {material} read ({}x{})", img.width(), img.height());
    }
    Some(img)
}

/// Whether a material is one of these (a calling card or an emblem).
pub(crate) fn is_look(material: &str) -> bool {
    material.split_once(':').is_some_and(|(g, n)| matches!(g, "iw4" | "iw5") && (n.starts_with("cardtitle_") || n.starts_with("cardicon_")))
        || crate::jup_cards::is_look(material)
        || crate::bo_cards::is_look(material)
        || crate::custom_cards::is_look(material)
}

// ---------------------------------------------------------------------------
// bo2mp: animated calling cards and emblems in a match (the kill card, the
// Killed By card, the killcam's card, the score bar's emblems): the menus
// turn theirs with `shell::view::Flipbook`; here each picture is marked with
// its look and turned by `turn_look_flips` once its frames are read.

/// Calling cards and emblems move (Options > GRAPHICS > ANIMATED CARDS).
static ANIMATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub(crate) fn set_animated(on: bool) {
    ANIMATED.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn animated() -> bool {
    ANIMATED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Frames a second a card turns at (as the menus').
pub(crate) const LOOK_FPS: f32 = 12.0;

/// A calling card's or emblem's picture on screen: its look (`jup:<name>`,
/// `t6:<name>`) and the still picture it shows when it does not move.
#[derive(Component, Clone, Debug)]
pub(crate) struct LookFlip {
    pub look: String,
    pub poster: Handle<Image>,
}

impl LookFlip {
    pub(crate) fn new(look: &str, poster: &Handle<Image>) -> Self {
        Self { look: look.to_owned(), poster: poster.clone() }
    }
}

/// Each look's frames as they are read (kept while it is on screen).
#[derive(Resource, Default)]
struct LookFrames(std::collections::HashMap<String, Vec<Option<Handle<Image>>>>);

/// Looks kept at most (their frames are a few MB each).
const KEEP_LOOKS: usize = 24;

fn turn_look_flips(time: Res<Time<Real>>, mut frames: ResMut<LookFrames>, mut images: ResMut<Assets<Image>>, mut q: Query<(&mut ImageNode, &LookFlip)>) {
    let on = animated();
    let at = (time.elapsed_secs() * LOOK_FPS) as usize;
    let mut seen = HashSet::new();
    for (mut img, flip) in &mut q {
        seen.insert(flip.look.clone());
        let n = cards::frames(&flip.look) as usize;
        if !on || n < 2 {
            if img.image != flip.poster {
                img.image = flip.poster.clone();
            }
            continue;
        }
        let book = frames.0.entry(flip.look.clone()).or_insert_with(|| vec![None; n]);
        if book.len() != n {
            *book = vec![None; n];
        }
        for (i, slot) in book.iter_mut().enumerate() {
            if slot.is_none()
                && let Some(pic) = picture(&cards::frame_name(&flip.look, i as u32))
            {
                let mut pic = (*pic).clone();
                pic.sampler = bevy::image::ImageSampler::linear();
                *slot = Some(images.add(pic));
            }
        }
        // (Turned once every frame is in; the still picture till then.)
        let frame = book.iter().all(Option::is_some).then(|| book[at % n].clone()).flatten();
        let want = frame.unwrap_or_else(|| flip.poster.clone());
        if img.image != want {
            img.image = want;
        }
    }
    if frames.0.len() > KEEP_LOOKS {
        frames.0.retain(|look, _| seen.contains(look));
    }
}

pub(crate) fn register_look_flips(app: &mut App) {
    app.init_resource::<LookFrames>().add_systems(Update, turn_look_flips.in_set(frame::ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_steady_and_from_the_games() {
        let a = look_of("WildBacon13", None);
        assert_eq!(a, look_of("WildBacon13", None));
        assert!(a.0.contains(":cardtitle_") && a.1.contains(":cardicon_"));
        assert!(is_look(&a.0) && is_look(&a.1));
        assert!(!is_look("emblem_bg_default") && !is_look("em_rank_pvt_full"));
        assert!(is_look("jup:ui_playercard_100") && is_look("jup:ui_emblem_005"));
        // His own: what his stats hold when the games have it.
        let list = cards::list(Kind::Card);
        let mine = look_of("sero", Some(&(list[list.len() - 1].clone(), String::new())));
        assert_eq!(mine.0, list[list.len() - 1]);
    }
}

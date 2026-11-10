//! bo2mp: the owner's own calling cards and emblem (the Barracks pickers'
//! CUSTOM tab), built into the game.
//!
//! Each is a flipbook: its frames stacked top to bottom in one palette PNG
//! (`assets/custom_cards/`), a card 512 x 128 (4:1, as the others), an
//! emblem 256 x 256, at the menus' 12 frames a second. They were cut from
//! the owner's GIFs; a still card (`card_mural`) is one frame. `custom:<name>#<n>` is frame `n`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) const PREFIX: &str = "custom";

struct Item {
    name: &'static str,
    title: &'static str,
    card: bool,
    png: &'static [u8],
}

const ITEMS: &[Item] = &[
    Item { name: "card_above_the_clouds", title: "Above the Clouds", card: true, png: include_bytes!("../assets/custom_cards/card_above_the_clouds.png") },
    Item { name: "card_freedom", title: "Freedom", card: true, png: include_bytes!("../assets/custom_cards/card_freedom.png") },
    Item { name: "card_awakened_eye", title: "Awakened Eye", card: true, png: include_bytes!("../assets/custom_cards/card_awakened_eye.png") },
    Item { name: "card_the_founder", title: "The Founder", card: true, png: include_bytes!("../assets/custom_cards/card_the_founder.png") },
    Item { name: "card_dread", title: "Dread", card: true, png: include_bytes!("../assets/custom_cards/card_dread.png") },
    Item { name: "card_the_rumbling", title: "The Rumbling", card: true, png: include_bytes!("../assets/custom_cards/card_the_rumbling.png") },
    Item { name: "card_third_impact", title: "Third Impact", card: true, png: include_bytes!("../assets/custom_cards/card_third_impact.png") },
    Item { name: "card_frieren_himmel", title: "The Promise", card: true, png: include_bytes!("../assets/custom_cards/card_frieren_himmel.png") },
    Item { name: "card_himmel", title: "Himmel", card: true, png: include_bytes!("../assets/custom_cards/card_himmel.png") },
    Item { name: "card_stark", title: "Stark", card: true, png: include_bytes!("../assets/custom_cards/card_stark.png") },
    Item { name: "card_campfire", title: "Campfire", card: true, png: include_bytes!("../assets/custom_cards/card_campfire.png") },
    Item { name: "card_quiet_forest", title: "Quiet Forest", card: true, png: include_bytes!("../assets/custom_cards/card_quiet_forest.png") },
    Item { name: "card_frieren", title: "Frieren", card: true, png: include_bytes!("../assets/custom_cards/card_frieren.png") },
    Item { name: "card_kittens_look_up", title: "Stripe", card: true, png: include_bytes!("../assets/custom_cards/card_kittens_look_up.png") },
    Item { name: "card_sleepy_paws", title: "Sleepy Paws", card: true, png: include_bytes!("../assets/custom_cards/card_sleepy_paws.png") },
    Item { name: "card_snuggle", title: "Snuggle", card: true, png: include_bytes!("../assets/custom_cards/card_snuggle.png") },
    Item { name: "card_mural", title: "Mural", card: true, png: include_bytes!("../assets/custom_cards/card_mural.png") },
    Item { name: "emblem_berserk", title: "Berserk", card: false, png: include_bytes!("../assets/custom_cards/emblem_berserk.png") },
];

/// (cards, emblems), as `custom:<name>`.
pub(crate) fn names() -> (Vec<String>, Vec<String>) {
    let tagged = |card: bool| ITEMS.iter().filter(|i| i.card == card).map(|i| format!("{PREFIX}:{}", i.name)).collect();
    (tagged(true), tagged(false))
}

/// `name#3` -> (`name`, 3); `name` -> (`name`, 0).
fn split_frame(name: &str) -> (&str, u32) {
    match name.rsplit_once('#') {
        Some((n, f)) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => (n, f.parse().unwrap_or(0)),
        _ => (name, 0),
    }
}

fn item(shell_name: &str) -> Option<&'static Item> {
    let (game, n) = shell_name.split_once(':')?;
    let n = split_frame(n).0;
    (game == PREFIX).then(|| ITEMS.iter().find(|i| i.name == n)).flatten()
}

/// Whether a shell name is one of these (any frame).
pub(crate) fn is_look(material: &str) -> bool {
    item(material).is_some()
}

/// A frame's size.
fn frame_size(i: &Item) -> (u32, u32) {
    if i.card { (512, 128) } else { (256, 256) }
}

/// The strip's height, from its PNG header (no decode).
fn strip_height(png: &[u8]) -> u32 {
    png.get(20..24).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// How many frames a name's flipbook has (1 for anything else).
pub(crate) fn frames(shell_name: &str) -> u32 {
    item(shell_name).map_or(1, |i| (strip_height(i.png) / frame_size(i).1).max(1))
}

/// Its title in the pickers.
pub(crate) fn title(shell_name: &str) -> Option<&'static str> {
    item(shell_name).map(|i| i.title)
}

type Strip = Arc<(u32, Vec<u8>)>;

/// A strip decoded to RGBA (kept: they are small).
fn strip(i: &Item) -> Option<Strip> {
    static DECODED: Mutex<Option<HashMap<&'static str, Strip>>> = Mutex::new(None);
    if let Some(s) = DECODED.lock().ok()?.as_ref().and_then(|m| m.get(i.name).cloned()) {
        return Some(s);
    }
    let img = image::load_from_memory_with_format(i.png, image::ImageFormat::Png).ok()?.to_rgba8();
    let s: Strip = Arc::new((img.width(), img.into_raw()));
    DECODED.lock().ok()?.get_or_insert_with(HashMap::new).insert(i.name, s.clone());
    Some(s)
}

/// A frame (`custom:<name>#<n>`; the first without one) as RGBA.
pub(crate) fn read_picture(names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    names.iter().find_map(|n| {
        let shell = if n.contains(':') { n.clone() } else { format!("{PREFIX}:{n}") };
        let i = item(&shell)?;
        let f = split_frame(shell.split_once(':')?.1).1.min(frames(&shell) - 1);
        let (w, h) = frame_size(i);
        let s = strip(i)?;
        let row = (s.0 * 4) as usize;
        let start = (f * h) as usize * row;
        let end = start + h as usize * row;
        if s.0 != w {
            return None;
        }
        Some((w, h, s.1.get(start..end)?.to_vec()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flipbook_reads_whole_frames() {
        let (cards, emblems) = names();
        assert_eq!(
            (cards.len(), emblems.len()),
            (ITEMS.iter().filter(|i| i.card).count(), ITEMS.iter().filter(|i| !i.card).count())
        );
        for n in cards.iter().chain(&emblems) {
            let i = item(n).unwrap();
            let (w, h) = frame_size(i);
            assert_eq!(strip_height(i.png) % h, 0, "{n}");
            let f = frames(n);
            assert!(f >= 1, "{n}");
            let last = read_picture(&[format!("{n}#{}", f - 1)]).unwrap();
            assert_eq!((last.0, last.1, last.2.len()), (w, h, (w * h * 4) as usize));
            assert!(is_look(&format!("{n}#3")));
            assert!(title(n).is_some());
        }
        // (The pickers ask for them by the shell's picture routing.)
        assert_eq!(bo2_profile::progression::foreign_picture(&format!("{}#2", cards[0])).map(|(g, _)| g), Some("custom"));
        assert!(!is_look("custom:nope") && !is_look("t6:card_freedom"));
    }
}

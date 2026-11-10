//! bo2mp shell: Create-a-Class - his five classes, a class's slots, and the
//! items (or attachments) a slot takes, in the shell's own look. Pick 10:
//! a class spends at most ten points; the wildcards open the extra slots
//! (Primary / Secondary Gunfighter, the Perk Greeds, Danger Close,
//! Overkill), as in Black Ops II.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use super::classes::{self, ClassBook, ClassItem, ClassView, Edit, POINTS, group_label, slot_kind};
use super::theme::*;
use super::anim;
use super::ghosts;
use super::view::{Ctx, Kids, abs, header_ex, lift, picture_contain};
use super::{Act, Hit, Row, Screen, Setting, Shell};

/// A class's slots as the edit screen lists them: (slot, label).
pub(crate) const EDIT_SLOTS: [(&str, &str); 23] = [
    ("primary", "PRIMARY"),
    ("primaryattachment1", "ATTACHMENT 1"),
    ("primaryattachment2", "ATTACHMENT 2"),
    ("primaryattachment3", "ATTACHMENT 3"),
    ("primarycamo", "CAMO"),
    ("secondary", "SECONDARY"),
    ("secondaryattachment1", "ATTACHMENT"),
    ("secondaryattachment2", "ATTACHMENT 2"),
    ("secondarycamo", "CAMO"),
    ("primarygrenade", "LETHAL"),
    ("specialgrenade", "TACTICAL"),
    ("specialty1", "PERK 1"),
    ("specialty4", "PERK 1 · GREED"),
    ("specialty2", "PERK 2"),
    ("specialty5", "PERK 2 · GREED"),
    ("specialty3", "PERK 3"),
    ("specialty6", "PERK 3 · GREED"),
    ("bonuscard1", "WILDCARD 1"),
    ("bonuscard2", "WILDCARD 2"),
    ("bonuscard3", "WILDCARD 3"),
    ("killstreak1", "SCORESTREAK 1"),
    ("killstreak2", "SCORESTREAK 2"),
    ("killstreak3", "SCORESTREAK 3"),
];

/// bo2mp: a value built for one key and kept until the key changes (the
/// pickers' lists, the camo tabs: they were built again several times a
/// frame - the input reads the rows every frame, the view builds them for
/// every box it draws).
pub(crate) struct Memo<K, V> {
    kept: Option<(K, Arc<V>)>,
    /// How many times it was built (the tests count them).
    pub builds: u64,
}

impl<K: PartialEq, V> Memo<K, V> {
    pub const fn new() -> Self {
        Self { kept: None, builds: 0 }
    }

    /// The value for `key`: the kept one when the key is the same, else
    /// `build`'s (kept for next time).
    pub fn get(&mut self, key: K, build: impl FnOnce() -> V) -> Arc<V> {
        if let Some((k, v)) = &self.kept
            && *k == key
        {
            return v.clone();
        }
        let v = Arc::new(build());
        self.builds += 1;
        self.kept = Some((key, v.clone()));
        v
    }
}

/// A cache key for one class book: the book's own revision (`ClassBook::
/// rev`, stamped when it was put up: any edit, rename or read-back that
/// changed a class is a new one) and where it is (a test's own book).
/// bo2mp: it was the generation *now* and the address, read when the
/// cache was asked; the revision is the book's, whenever it is asked.
type BookKey = (u64, usize);

fn book_key(book: &ClassBook) -> BookKey {
    (book.rev, std::ptr::from_ref(book) as usize)
}

/// The class list's row in focus as his class (`Act::EditClass`'s number).
pub(crate) fn focused_class<'a>(book: &'a ClassBook, rows: &[Row], focus: usize) -> Option<&'a ClassView> {
    match rows.get(focus)?.act {
        Act::EditClass(n) => book.class(n),
        _ => None,
    }
}

fn memo<K: PartialEq, V>(m: &Mutex<Memo<K, V>>, key: K, build: impl FnOnce() -> V) -> Arc<V> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(key, build)
}

/// A wildcard of this class whose reference has `part`.
fn has_card(book: &ClassBook, c: &ClassView, part: &str) -> bool {
    (1..=3).any(|k| book.item(c.get(&format!("bonuscard{k}"))).is_some_and(|i| i.reference.contains(part)))
}

/// A camo in the picker: (number, name, picture).
type CamoChoice = (i32, String, String);

/// The camo tabs: (tab, [(number, name, picture)]).
type CamoTabs = Vec<(&'static str, Vec<CamoChoice>)>;

static CAMO_TABS: Mutex<Memo<(BookKey, bool), CamoTabs>> = Mutex::new(Memo::new());

/// [`camo_tabs_built`], built once per book.
fn camo_tabs(book: &ClassBook) -> Arc<CamoTabs> {
    memo(&CAMO_TABS, (book_key(book), bo2_profile::guest::available("t7")), || camo_tabs_built(book))
}

/// bo2mp: every game's camos, which every gun takes, a tab per game: Black
/// Ops II's (its weapon options), Modern Warfare 3's, 2's, Black Ops',
/// Black Ops III's, Modern Warfare III's and II's (their camo numbers,
/// `asset_core::weapon_camos`); a name twice in a tab kept once (each
/// game's Gold, Woodland, ... are its own pictures, so all are listed);
/// (tab, [(number, name, picture)]).
fn camo_tabs_built(book: &ClassBook) -> CamoTabs {
    use asset_core::weapon_camos::{BO1_FIRST, IW9_FIRST, JUP_FIRST, MW2_FIRST, MW3_FIRST, T7_FIRST};
    use asset_game::weapon_camo_models::{BO1_CAMOS, IW9_CAMO_NAMES, IW9_CAMOS, JUP_CAMO_NAMES, JUP_CAMOS, MW2_CAMOS, MW3_CAMOS, T7_CAMOS};
    let tab = |list: Vec<CamoChoice>| {
        let mut seen = std::collections::HashSet::new();
        list.into_iter().filter(|(_, name, _)| seen.insert(camo_key(name))).collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    out.push(("BLACK OPS II", tab(book.camos.iter().map(|k| (k.index, k.name.to_uppercase(), k.image.clone())).collect())));
    for (title, game, first, list) in [("MODERN WARFARE 3", "iw5", MW3_FIRST, &MW3_CAMOS[..]), ("MODERN WARFARE 2", "iw4", MW2_FIRST, &MW2_CAMOS[..])] {
        let v = list.iter().map(|(n, name, pic)| ((first + u32::from(*n)) as i32, name.to_uppercase(), format!("{game}:{pic}"))).collect();
        out.push((title, tab(v)));
    }
    let bo1 = BO1_CAMOS.iter().map(|(n, name, _, pic)| ((BO1_FIRST + u32::from(*n)) as i32, name.to_uppercase(), format!("t5:{pic}"))).collect();
    out.push(("BLACK OPS", tab(bo1)));
    // bo2mp: the player's own camos (the `camos` folder next to the game).
    let own: Vec<CamoChoice> = asset_core::weapon_camos::user_camos()
        .into_iter()
        .map(|u| (u.number as i32, u.name.to_uppercase(), String::new()))
        .collect();
    if !own.is_empty() {
        out.push(("CUSTOM", tab(own)));
    }
    // bo2mp: Black Ops III's (its swatch, else the picture drawn on the
    // gun: `foreign_maps::read_game_image`'s `t7` pictures).
    if bo2_profile::guest::available("t7") {
        let bo3 = T7_CAMOS
            .iter()
            .map(|(n, name, swatch, _, pic)| ((T7_FIRST + u32::from(*n)) as i32, name.to_uppercase(), format!("t7:{swatch}|{pic}")))
            .collect();
        out.push(("BLACK OPS III", tab(bo3)));
    }
    // bo2mp: Modern Warfare III's (2023) and II's (2022) camos, named by
    // their categories (`asset_game::weapon_camo_models::JUP_CAMO_NAMES`).
    for (title, first, list, names) in [
        ("MODERN WARFARE III", JUP_FIRST, &JUP_CAMOS[..], &JUP_CAMO_NAMES[..]),
        ("MODERN WARFARE II", IW9_FIRST, &IW9_CAMOS[..], &IW9_CAMO_NAMES[..]),
    ] {
        let v = list.iter().zip(names).enumerate().map(|(n, (pic, name))| ((first + n as u32) as i32, (*name).to_owned(), format!("jup:{pic}"))).collect();
        out.push((title, tab(v)));
    }
    // bo2mp: the special camos (`asset_core::weapon_camos::EFFECT_CAMOS`:
    // Ghost, the moving ones), each with its source camo's swatch when that
    // game's camos are listed above.
    let special = asset_core::weapon_camos::EFFECT_CAMOS
        .iter()
        .enumerate()
        .map(|(i, fx)| {
            let image = fx
                .source
                .and_then(|src| out.iter().flat_map(|(_, v)| v.iter()).find(|(n, ..)| *n == src as i32).map(|(_, _, p)| p.clone()))
                .unwrap_or_default();
            ((asset_core::weapon_camos::FX_FIRST + i as u32) as i32, fx.name.to_uppercase(), image)
        })
        .collect();
    out.push(("SPECIAL", tab(special)));
    out.retain(|(_, v)| !v.is_empty());
    out
}

/// A camo's name as one key across games (`Blue Tiger` and `Tiger Blue`,
/// `Jungle Tiger` and `Tiger` alike).
fn camo_key(name: &str) -> String {
    let mut words: Vec<String> = name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "camo" && *w != "jungle")
        .map(str::to_owned)
        .collect();
    words.sort();
    words.concat()
}

/// bo2mp: whether a gun takes camos (every game's, see `camo_tabs`): any
/// gun, when there are any.
fn takes_camo(book: &ClassBook, gun: i32) -> bool {
    gun > 0 && camo_tabs(book).iter().any(|(_, v)| !v.is_empty())
}

/// The camo numbered `value` on a gun: (number, name, picture).
fn camo_of(book: &ClassBook, gun: i32, value: i32) -> Option<CamoChoice> {
    if !takes_camo(book, gun) {
        return None;
    }
    camo_tabs(book).iter().flat_map(|(_, v)| v.iter()).find(|(n, ..)| *n == value).cloned()
}

/// Whether the edit screen lists this slot for the class.
fn shown(book: &ClassBook, c: &ClassView, slot: &str) -> bool {
    match slot {
        "primaryattachment3" => has_card(book, c, "primary_gunfighter"),
        "secondaryattachment2" => has_card(book, c, "secondary_gunfighter"),
        "specialty4" => has_card(book, c, "perk_1_greed"),
        "specialty5" => has_card(book, c, "perk_2_greed"),
        "specialty6" => has_card(book, c, "perk_3_greed"),
        // bo2mp: Black Ops II's camos, on its own guns.
        "primarycamo" | "secondarycamo" => takes_camo(book, c.get(weapon_of(slot))),
        s if s.starts_with("primaryattachment") => book.item(c.get("primary")).is_some_and(|i| !i.attachments.is_empty()),
        s if s.starts_with("secondaryattachment") => book.item(c.get("secondary")).is_some_and(|i| !i.attachments.is_empty()),
        _ => true,
    }
}

/// The slots the edit screen lists for the class, by index into
/// `EDIT_SLOTS`.
pub(crate) fn edit_slots(book: &ClassBook, c: &ClassView) -> Vec<usize> {
    (0..EDIT_SLOTS.len()).filter(|&i| shown(book, c, EDIT_SLOTS[i].0)).collect()
}

fn weapon_of(slot: &str) -> &str {
    if slot.starts_with("secondary") { "secondary" } else { "primary" }
}

/// What a slot holds, as text.
pub(crate) fn slot_value(book: &ClassBook, c: &ClassView, slot: &str) -> String {
    let v = c.get(slot);
    if slot.ends_with("camo") {
        return camo_of(book, c.get(weapon_of(slot)), v).map_or_else(|| "NONE".to_owned(), |(_, name, _)| name);
    }
    if slot.contains("attachment") {
        let weapon = book.item(c.get(weapon_of(slot)));
        return weapon
            .and_then(|w| w.attachments.iter().find(|(n, ..)| *n == v))
            .map_or_else(|| "NONE".to_owned(), |(_, name, _)| name.to_uppercase());
    }
    match book.item(v) {
        Some(i) if slot.ends_with("grenade") => {
            let count = c.get(&format!("{slot}count")).max(1);
            if count > 1 { format!("{} x{count}", i.name.to_uppercase()) } else { i.name.to_uppercase() }
        }
        Some(i) => i.name.to_uppercase(),
        None => "EMPTY".to_owned(),
    }
}

/// What a Create-a-Class screen's rows and picks depend on besides the
/// book: the screen, the class, the slot, the tab, a public match's list.
type ScreenKey = (BookKey, Screen, i32, usize, usize, (bool, bool));

fn screen_key(s: &Shell, book: &ClassBook) -> ScreenKey {
    (book_key(book), s.screen, s.class, s.slot, s.page, (s.public, s.in_game))
}

static ROWS: Mutex<Memo<ScreenKey, Vec<Row>>> = Mutex::new(Memo::new());

/// The screen's rows (the input reads them every frame: built once per
/// change, `rows_built`).
pub(crate) fn rows(s: &Shell, book: &ClassBook) -> Vec<Row> {
    // (The class list's rows do not depend on the class or slot.)
    let key = match s.screen {
        Screen::Classes => (book_key(book), s.screen, 0, 0, 0, (false, false)),
        _ => screen_key(s, book),
    };
    memo(&ROWS, key, || rows_built(s, book)).as_ref().clone()
}

fn rows_built(s: &Shell, book: &ClassBook) -> Vec<Row> {
    match s.screen {
        Screen::Classes => book
            .classes
            .iter()
            .map(|c| Row {
                label: c.name.to_uppercase(),
                sub: format!("{} / {POINTS} points", c.spent),
                value: None,
                act: Act::EditClass(c.n),
            })
            .collect(),
        Screen::ClassEdit => {
            let Some(c) = book.class(s.class) else { return Vec::new() };
            let mut out = Vec::new();
            // (In a match no camo is chosen: the editor lists none.)
            for i in edit_slots(book, c).into_iter().filter(|&i| !(s.in_game && EDIT_SLOTS[i].0.ends_with("camo"))) {
                let (slot, label) = EDIT_SLOTS[i];
                out.push(Row { label: label.to_owned(), sub: String::new(), value: Some(slot_value(book, c, slot)), act: Act::EditSlot(i) });
                // A grenade's count right after it.
                if slot.ends_with("grenade") && c.get(slot) > 0 {
                    let lethal = slot == "primarygrenade";
                    out.push(Row {
                        label: if lethal { "LETHAL COUNT" } else { "TACTICAL COUNT" }.to_owned(),
                        sub: String::new(),
                        value: Some(c.get(&format!("{slot}count")).max(1).to_string()),
                        act: Act::Cycle(if lethal { Setting::LethalCount } else { Setting::TacticalCount }),
                    });
                }
            }
            out
        }
        Screen::ItemPick => picks_arc(s, book)
            .1
            .iter()
            .enumerate()
            .map(|(k, p)| Row { label: p.name.clone(), sub: p.desc.clone(), value: None, act: Act::Choose(k) })
            .collect(),
        _ => Vec::new(),
    }
}

/// One choice in the item picker.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pick {
    pub value: i32,
    pub name: String,
    pub desc: String,
    pub image: String,
    pub cost: i32,
    pub locked: bool,
    pub level: i32,
    /// A weapon's kind (ASSAULT RIFLES...), shown on its tile.
    pub group: String,
}

/// The game a weapon comes from, as its tab in the picker: (order, name).
pub(crate) fn game_tab(index: i32) -> (usize, &'static str) {
    if index < bo2_profile::guest::FIRST_INDEX {
        return (0, "BO2");
    }
    // (A Horizon MW gun on the tab of the game it is a gun of: MWR's
    // versions stand for Modern Warfare 2's, 3's and Call of Duty 4's.)
    match bo2_profile::guest::tab_game_for_index(index) {
        Some("t5") => (1, "BO1"),
        Some("t7") => (2, "BO3"),
        Some("iw3" | "h1") => (3, "COD4"),
        Some("t4") => (4, "WAW"),
        Some("iw4") => (5, "MW2"),
        Some("iw5") => (6, "MW3"),
        Some("iw6") => (7, "GHOSTS"),
        Some("s1") => (8, "AW"),
        Some("jup") => (9, "MWIII"),
        // (Modern Warfare II (2022)'s guns, from the MWIII install.)
        Some("iw9") => (9, "MWII"),
        // (Infinite Warfare's guns, its classic guns among them.)
        Some("iw7") => (11, "IW"),
        // (Black Ops III Workshop weapon packs: COLD WAR, VANGUARD, MW 2019,
        // COD 2, CODOL, CODOL 2.)
        Some(g) if let Some(k) = bo2_profile::guest::WORKSHOP_PACKS.iter().position(|p| p.game == g) => {
            // (The two CoD Online packs share one tab.)
            let k = if g == "t7ol2" { bo2_profile::guest::WORKSHOP_PACKS.iter().position(|p| p.game == "t7ol").unwrap_or(k) } else { k };
            (12 + k, bo2_profile::guest::WORKSHOP_PACKS[k].tab)
        }
        Some(bo2_profile::guest::MOD_GAME) => (12 + bo2_profile::guest::WORKSHOP_PACKS.len(), "MOD"),
        _ => (13 + bo2_profile::guest::WORKSHOP_PACKS.len(), "OTHER"),
    }
}

/// The game a weapon comes from, in full (the picker's GAME line).
fn game_full(index: i32) -> &'static str {
    match game_tab(index).1 {
        "BO2" => "BLACK OPS II",
        "BO1" => "BLACK OPS",
        "BO3" => "BLACK OPS III",
        "COD4" => "CALL OF DUTY 4",
        "WAW" => "WORLD AT WAR",
        "MW2" => "MODERN WARFARE 2",
        "MW3" => "MODERN WARFARE 3",
        "GHOSTS" => "GHOSTS",
        "AW" => "ADVANCED WARFARE",
        "MWIII" => "MODERN WARFARE III",
        "MWII" => "MODERN WARFARE II (2022)",
        "IW" => "INFINITE WARFARE",
        "COLD WAR" => "BLACK OPS COLD WAR",
        "VANGUARD" => "VANGUARD",
        "MW 2019" => "MODERN WARFARE (2019)",
        "COD 2" => "CALL OF DUTY 2",
        "CODOL" => "CALL OF DUTY ONLINE",
        "CODOL 2" => "CALL OF DUTY ONLINE (SECOND PACK)",
        other => other,
    }
}

/// The picker's tabs and choices (`picks_arc`), as a copy.
pub(crate) fn picks(s: &Shell, book: &ClassBook) -> (Vec<String>, Vec<Pick>) {
    picks_arc(s, book).as_ref().clone()
}

/// The picker's tabs and the choices on the tab he is on.
type Picks = (Vec<String>, Vec<Pick>);

static PICKS: Mutex<Memo<ScreenKey, Picks>> = Mutex::new(Memo::new());

/// [`picks_built`], built once per change of class, slot or tab (it lists
/// every game's guns: the view and the input asked for it many times a
/// frame).
pub(crate) fn picks_arc(s: &Shell, book: &ClassBook) -> Arc<Picks> {
    memo(&PICKS, picks_key(s, book), || picks_built(s, book))
}

/// What the picker's list depends on (not the tile in focus: moving the
/// cursor builds nothing).
fn picks_key(s: &Shell, book: &ClassBook) -> ScreenKey {
    (book_key(book), Screen::ItemPick, s.class, s.slot, s.page, (s.public, s.in_game))
}

/// The picker's tabs (weapon groups) and the choices on the tab he is on.
fn picks_built(s: &Shell, book: &ClassBook) -> (Vec<String>, Vec<Pick>) {
    let Some(c) = book.class(s.class) else { return (Vec::new(), Vec::new()) };
    let slot = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].0;
    // bo2mp: Black Ops II's camos for the gun (Personalize Weapon's list).
    if slot.ends_with("camo") {
        if c.get(weapon_of(slot)) <= 0 {
            return (vec!["CAMOS".to_owned()], Vec::new());
        }
        let tabs = camo_tabs(book);
        let tab = s.page.min(tabs.len().saturating_sub(1));
        let mut v = Vec::new();
        // (NONE on the first tab.)
        if tab == 0 {
            v.push(Pick { value: 0, name: "NONE".to_owned(), desc: "The gun as it comes.".to_owned(), image: String::new(), cost: 0, locked: false, level: 1, group: String::new() });
        }
        let game = tabs.get(tab).map_or("", |(t, _)| *t);
        for (n, name, image) in tabs.get(tab).map_or_else(Vec::new, |(_, l)| l.clone()) {
            // (Where it comes from: the only camo facts the code has - no
            // camo here has an unlock challenge.)
            let from = match game {
                "SPECIAL" => "A special camo: it moves on the gun".to_owned(),
                "CUSTOM" => "Your own camo, from the camos folder".to_owned(),
                "BLACK OPS II" if book.camo(n).is_some_and(|k| k.material) => "A Black Ops II material camo".to_owned(),
                "BLACK OPS II" if book.camo(n).is_some_and(|k| !k.drawn) => "A Black Ops II DLC camo (its picture comes with its pack)".to_owned(),
                g => format!("A {} camo", title_case(g)),
            };
            v.push(Pick {
                value: n,
                name,
                desc: format!("{from}. Worn on the gun in matches, first and third person."),
                image,
                cost: 0,
                locked: false,
                level: 1,
                group: String::new(),
            });
        }
        let names = if tabs.is_empty() { vec!["CAMOS".to_owned()] } else { tabs.iter().map(|(t, _)| (*t).to_owned()).collect() };
        return (names, v);
    }
    if slot.contains("attachment") {
        let mut v = vec![Pick { value: 0, name: "NONE".to_owned(), desc: "No attachment.".to_owned(), image: String::new(), cost: 0, locked: false, level: 1, group: String::new() }];
        let gun = c.get(weapon_of(slot));
        if let Some(w) = book.item(gun) {
            // bo2mp: by kind (OPTICS, BARREL, ...: `attachment_group`), each
            // kind's in the table's order, so a kind is one run of the list
            // (`categories`, `weapon_step`).
            let mut list: Vec<Pick> = w
                .attachments
                .iter()
                .map(|(n, name, image)| {
                    let (reference, desc) = book.att_info.get(&(gun, *n)).cloned().unwrap_or_default();
                    Pick { value: *n, name: name.to_uppercase(), desc, image: image.clone(), cost: 1, locked: false, level: 1, group: attachment_group(&reference, name).to_owned() }
                })
                .collect();
            list.sort_by_key(|p| ATTACHMENT_GROUPS.iter().position(|g| *g == p.group).unwrap_or(ATTACHMENT_GROUPS.len()));
            v.extend(list);
        }
        return (vec!["ATTACHMENTS".to_owned()], v);
    }
    let mut items: Vec<&ClassItem> = book.for_slot(slot);
    // bo2mp: the mod's Zombies weapons are Custom Games only: not listed
    // while he sets up a public match (FIND A MATCH).
    if s.public {
        items.retain(|i| !bo2_profile::guest::is_custom_only_index(i.index));
    }
    // Overkill: a second primary as the secondary.
    if slot == "secondary" && has_card(book, c, "overkill") {
        items.extend(book.for_slot("primary"));
    }
    let weapons = matches!(slot_kind(slot).as_str(), "primary" | "secondary");
    // bo2mp: weapons by the game they come from (a tab each, Black Ops II's
    // own first), as Modern Warfare III's Create-a-Class lists its games'.
    let mut tabs: Vec<String> = Vec::new();
    if weapons {
        let mut games: Vec<(usize, &str)> = items.iter().map(|i| game_tab(i.index)).collect();
        games.sort_unstable();
        games.dedup();
        tabs = games.into_iter().map(|(_, t)| t.to_owned()).collect();
    }
    // bo2mp: the games whose weapons are coming, a tab each after the rest.
    if weapons {
        for g in &super::coming::COMING {
            tabs.push(g.tab.to_owned());
        }
    }
    if tabs.is_empty() {
        tabs.push(EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].1.split(" ·").next().unwrap_or("").to_owned() + "S");
    }
    let tab = s.page.min(tabs.len() - 1);
    if weapons && let Some(i) = super::coming::by_tab(&tabs[tab]) {
        return (tabs, vec![super::coming::placeholder(i)]);
    }
    let mut out = Vec::new();
    // (A slot that may be empty: NONE first.)
    if !matches!(slot, "primary") {
        out.push(Pick { value: 0, name: "NONE".to_owned(), desc: "Leave this slot empty.".to_owned(), image: String::new(), cost: 0, locked: false, level: 1, group: String::new() });
    }
    for i in items {
        if weapons && game_tab(i.index).1 != tabs[tab] {
            continue;
        }
        out.push(Pick {
            value: i.index,
            name: i.name.to_uppercase(),
            desc: i.desc.clone(),
            image: i.image.clone(),
            cost: i.cost,
            locked: i.locked,
            level: i.level,
            group: group_label(&i.group).to_owned(),
        });
    }
    if weapons && tab != 0 {
        out.retain(|p| p.value != 0);
    }
    (tabs, out)
}

/// The attachment picker's kinds, in order.
const ATTACHMENT_GROUPS: [&str; 5] = ["OPTICS", "BARREL", "UNDER BARREL", "MAGAZINE", "MODS"];

/// An attachment's kind in the picker, by its mp/attachmenttable.csv
/// reference (Black Ops II's), else by words of its name (other games').
pub(crate) fn attachment_group(reference: &str, name: &str) -> &'static str {
    let r = reference.to_ascii_lowercase();
    match r.split('_').next().unwrap_or("") {
        "reflex" | "acog" | "holo" | "mms" | "ir" | "dualoptic" | "rangefinder" | "vzoom" | "is" | "swayreduc" => return "OPTICS",
        "silencer" | "extbarrel" => return "BARREL",
        "grip" | "gl" | "mk" | "ft" | "tacknife" => return "UNDER BARREL",
        "extclip" | "dualclip" | "fastreload" => return "MAGAZINE",
        "" => {}
        _ => return "MODS",
    }
    let n = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["sight", "scope", "acog", "optic", "reflex", "eotech", "holo", "thermal", "zoom"]) {
        "OPTICS"
    } else if has(&["suppressor", "silencer", "barrel", "muzzle", "compensator", "flash"]) {
        "BARREL"
    } else if has(&["grip", "launcher", "shotgun", "bipod", "flamethrower"]) {
        "UNDER BARREL"
    } else if has(&["mag", "clip", "drum"]) {
        "MAGAZINE"
    } else {
        "MODS"
    }
}

/// The weapon picker's categories: the runs of one kind in its list
/// (start, end) (NONE, alone, first for a slot that may be empty).
pub(crate) fn categories(picks: &[Pick]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, p) in picks.iter().enumerate() {
        match out.last_mut() {
            Some((start, end)) if picks[*start].group == p.group => *end = i + 1,
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// Whether the slot open in the picker takes a weapon or an attachment
/// (the picker is then a category at a time, as Black Ops II's: left /
/// right in a category, up / down between them, `weapon_step`).
pub(crate) fn weapon_picker(s: &Shell) -> bool {
    let slot = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].0;
    matches!(slot, "primary" | "secondary") || slot.contains("attachment")
}

/// A move in the weapon picker: left / right through the category's guns,
/// up / down to the first of the category before or after (round).
pub(crate) fn weapon_step(s: &Shell, book: &ClassBook, dir: (i32, i32)) -> Option<usize> {
    let all = picks_arc(s, book);
    let cats = categories(&all.1);
    let at = cats.iter().position(|(a, b)| (*a..*b).contains(&s.focus))?;
    let (a, b) = cats[at];
    if dir.1 != 0 {
        let n = cats.len() as i32;
        return Some(cats[(at as i32 + dir.1).rem_euclid(n) as usize].0);
    }
    Some((s.focus as i32 + dir.0).clamp(a as i32, b as i32 - 1) as usize)
}

/// The picker opens on the tab and tile of what the slot holds.
pub(crate) fn open_pick(s: &mut Shell, book: &ClassBook, slot_index: usize) {
    s.slot = slot_index;
    s.screen = Screen::ItemPick;
    s.page = 0;
    s.focus = 0;
    let Some(c) = book.class(s.class) else { return };
    let have = c.get(EDIT_SLOTS[slot_index].0);
    let tabs = picks_arc(s, book).0.len();
    for t in 0..tabs {
        s.page = t;
        if let Some(k) = picks_arc(s, book).1.iter().position(|p| p.value == have) {
            s.focus = k;
            return;
        }
    }
    s.page = 0;
}

/// Put a choice in the slot (within the points); true when it went in.
pub(crate) fn choose(s: &mut Shell, book: &ClassBook, k: usize, now: f64) -> bool {
    let Some(c) = book.class(s.class) else { return false };
    let Some(p) = picks_arc(s, book).1.get(k).cloned() else { return false };
    if let Some(g) = super::coming::of_value(p.value) {
        s.note = Some((format!("{} weapons are coming soon.", g.title), now + 3.0));
        return false;
    }
    if p.locked {
        s.note = Some((format!("{} unlocks at level {}.", p.name, p.level), now + 3.0));
        return false;
    }
    let slot = EDIT_SLOTS[s.slot].0;
    let mut set: Vec<(String, i32)> = vec![(slot.to_owned(), p.value)];
    if slot == "primary" || slot == "secondary" {
        // A new weapon starts bare.
        if c.get(slot) != p.value {
            for k in 1..=3 {
                set.push((format!("{slot}attachment{k}"), 0));
            }
            set.push((format!("{slot}camo"), 0));
        }
    }
    if slot.ends_with("grenade") {
        set.push((format!("{slot}count"), if p.value > 0 { 1 } else { 0 }));
    }
    if slot.contains("attachment") && p.value > 0 {
        // The same attachment twice is once.
        let w = weapon_of(slot);
        for k in 1..=3 {
            let other = format!("{w}attachment{k}");
            if other != slot && c.get(&other) == p.value {
                set.push((other, 0));
            }
        }
    }
    if slot.starts_with("killstreak") && p.value > 0 {
        for k in 1..=3 {
            let other = format!("killstreak{k}");
            if other != slot && c.get(&other) == p.value {
                set.push((other, c.get(slot)));
            }
        }
    }
    apply(s, book, c, set, now)
}

/// A grenade count up or down (two of a lethal only with Danger Close).
pub(crate) fn count(s: &mut Shell, book: &ClassBook, lethal: bool, by: i32, now: f64) {
    let Some(c) = book.class(s.class) else { return };
    let slot = if lethal { "primarygrenade" } else { "specialgrenade" };
    let max = if !lethal || has_card(book, c, "danger_close") { 2 } else { 1 };
    let n = (c.get(&format!("{slot}count")).max(1) - 1 + by).rem_euclid(max) + 1;
    apply(s, book, c, vec![(format!("{slot}count"), n)], now);
}

fn apply(s: &mut Shell, book: &ClassBook, c: &ClassView, set: Vec<(String, i32)>, now: f64) -> bool {
    let mut slots = c.slots.clone();
    for (k, v) in &set {
        slots.insert(k.clone(), *v);
    }
    let spent = classes::spent(book, &slots);
    if spent > POINTS && spent > c.spent {
        s.note = Some((format!("Not enough points: that needs {spent} of {POINTS}."), now + 3.0));
        return false;
    }
    classes::edit(Edit { class: c.n, set });
    true
}

/// Points: ten thin bars, the spent ones lit in the accent, and the count.
fn points_bar(c: &mut Kids, cx: &Ctx, spent: i32, width: f32) {
    c.spawn((Node { width: cx.px(width), height: cx.px(30.0), column_gap: cx.px(4.0), align_items: AlignItems::Center, margin: UiRect::bottom(cx.px(4.0)), ..default() }, anim::from_left(cx.s, -40.0, 0.04)))
        .with_children(|b| {
            b.spawn((Text::new("POINTS"), cx.regular(20.0), TextColor(DIM), Node { margin: UiRect::right(cx.px(12.0)), ..default() }));
            for k in 0..POINTS {
                b.spawn((Node { flex_grow: 1.0, height: cx.px(4.0), ..default() }, BackgroundColor(if k < spent { cx.pal.accent } else { WHITE.with_alpha(0.14) })));
            }
            b.spawn((
                Node { margin: UiRect::left(cx.px(12.0)), ..default() },
                Text::new(format!("{spent}/{POINTS}")),
                cx.bold(20.0),
                TextColor(if spent > POINTS { ENEMY } else { WHITE }),
            ));
        });
}

/// Where the loadout's boxes answer to: the edit screen's row for a slot
/// (a click on the box opens it), and the slot in focus (lit).
struct Focus<'a> {
    rows: &'a [Row],
    focus_slot: Option<&'a str>,
}

impl Focus<'_> {
    fn row(&self, slot: &str) -> Option<usize> {
        let at = EDIT_SLOTS.iter().position(|(s, _)| *s == slot)?;
        self.rows.iter().position(|r| r.act == Act::EditSlot(at))
    }

    /// A grenade count's row (`primarygrenadecount`, `specialgrenadecount`).
    fn count_row(&self, slot: &str) -> Option<usize> {
        let what = if slot.starts_with("primary") { Setting::LethalCount } else { Setting::TacticalCount };
        self.rows.iter().position(|r| r.act == Act::Cycle(what))
    }

    fn lit(&self, slot: &str) -> bool {
        self.focus_slot == Some(slot)
    }
}

/// The overview's place for a slot: whether it is lit (in focus) and the
/// row a click on it opens. `None` focus: a summary that takes no clicks.
fn place(f: Option<&Focus>, slot: &str) -> (bool, Option<usize>) {
    f.map_or((false, None), |f| (f.lit(slot), f.row(slot)))
}

/// A box's hit target, when it has a row.
fn hit(e: &mut EntityCommands, row: Option<usize>) {
    if let Some(r) = row {
        e.insert((Hit::Row(r), Button));
    }
}

/// The ground of a slot in focus on the overview: a solid accent band, as
/// Ghosts' lit slot.
fn lit_band(cx: &Ctx) -> BackgroundGradient {
    fade_right(cx.pal.accent, lift(cx.pal.accent, -0.08))
}

/// A box's fill, as the Ghosts-style menus' bands: the screen's dark panel
/// colour, lighter at the top (the accent in it when lit).
fn tile_fill(cx: &Ctx, lit: bool) -> BackgroundGradient {
    let p = cx.pal.panel.to_srgba();
    let top = if lit { cx.pal.accent_a(0.28) } else { Color::srgba(p.red + 0.10, p.green + 0.10, p.blue + 0.11, 0.80) };
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::new(top, Val::Percent(0.0)),
        ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.90), Val::Percent(100.0)),
    ]))
}

/// A section's heading: its name dim, then a thin rule and what it holds
/// bright (`PRIMARY | M8A1`).
fn heading(c: &mut Kids, cx: &Ctx, label: &str, value: Option<&str>) {
    c.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
        h.spawn((Text::new(label.to_owned()), cx.regular(24.0), TextColor(DIM)));
        if let Some(v) = value {
            h.spawn((Node { width: cx.px(1.0), height: cx.px(24.0), ..default() }, BackgroundColor(FAINT)));
            h.spawn((Text::new(v.to_owned()), cx.bold(24.0), TextColor(WHITE)));
        }
    });
}

/// A section of the summary: a thin rule over it, sliding in a little
/// after the one above (`k`: its place).
/// `lit`: beside a picker, Some(true) for the section it fills (an accent
/// edge over a faint accent ground), Some(false) for the rest (dimmed).
fn section<'a>(c: &'a mut Kids, cx: &Ctx, width: f32, k: usize, lit: Option<bool>) -> EntityCommands<'a> {
    let on = lit == Some(true);
    let mut e = c.spawn((
        Node {
            width: cx.px(width),
            flex_direction: FlexDirection::Column,
            row_gap: cx.px(6.0),
            padding: UiRect::new(cx.px(if on { 10.0 } else { 0.0 }), Val::Px(0.0), cx.px(8.0), cx.px(8.0)),
            border: UiRect { top: cx.px(1.0), left: cx.px(if on { 3.0 } else { 0.0 }), ..default() },
            ..default()
        },
        BorderColor { left: cx.pal.accent, ..BorderColor::all(cx.pal.line) },
        BackgroundColor(if on { cx.pal.accent_a(0.12) } else { Color::NONE }),
        anim::from_left(cx.s, -40.0, 0.05 + k as f32 * 0.02),
    ));
    if lit == Some(false) {
        e.with_child((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.01, 0.03, 0.58)),
            ZIndex(10),
        ));
    }
    e
}

/// What a small cell of the summary shows.
enum Cell {
    /// The item's picture.
    Pic(String),
    /// An open slot (a diamond).
    Empty,
    /// A slot the class cannot use yet (a padlock: Gunfighter opens it).
    Locked,
}

/// A small cell: thin rules between cells, its picture, a diamond for an
/// open slot or a padlock for a locked one; on the overview lit as a solid
/// accent band when its slot is in focus, a click opening it (`at`: its
/// row).
#[allow(clippy::too_many_arguments)]
fn cell(c: &mut Kids, cx: &mut Ctx, what: Cell, first: bool, w: f32, h: f32, lit: bool, at: Option<usize>) {
    tally(1);
    let mut e = c.spawn((
        Node { width: cx.px(w), height: cx.px(h), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::left(cx.px(if first { 0.0 } else { 1.0 })), ..default() },
        BorderColor::all(cx.pal.line),
    ));
    if lit {
        e.insert(lit_band(cx));
    }
    hit(&mut e, at);
    e.with_children(|b| match what {
        Cell::Pic(p) => picture_contain(b, cx, &[p], w - 20.0, h - 12.0),
        Cell::Empty => {
            b.spawn((
                Node { width: cx.px(14.0), height: cx.px(14.0), ..default() },
                BackgroundColor(if lit { WHITE.with_alpha(0.85) } else { cx.pal.accent_a(0.55) }),
                UiTransform::from_rotation(Rot2::degrees(45.0)),
            ));
        }
        Cell::Locked => {
            b.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|l| {
                l.spawn((
                    Node { width: cx.px(16.0), height: cx.px(13.0), border: UiRect::new(cx.px(3.0), cx.px(3.0), cx.px(3.0), cx.px(0.0)), border_radius: BorderRadius::top(cx.px(8.0)), ..default() },
                    BorderColor::all(WHITE.with_alpha(0.40)),
                ));
                l.spawn((Node { width: cx.px(24.0), height: cx.px(18.0), border_radius: BorderRadius::all(cx.px(3.0)), ..default() }, BackgroundColor(WHITE.with_alpha(0.40))));
            });
        }
    });
}

/// A weapon's attachment and camo cells: each attachment slot the gun has
/// (the Gunfighter one locked without its wildcard), then its camo; with
/// the slot each is.
fn weapon_cells(book: &ClassBook, class: &ClassView, weapon: &str) -> Vec<(String, Cell)> {
    let Some(gun) = book.item(class.get(weapon)) else { return Vec::new() };
    let mut out = Vec::new();
    if !gun.attachments.is_empty() {
        let slots = if weapon == "primary" { 3 } else { 2 };
        for k in 1..=slots {
            let slot = format!("{weapon}attachment{k}");
            let value = class.get(&slot);
            let what = if !shown(book, class, &slot) && value <= 0 {
                Cell::Locked
            } else {
                gun.attachments.iter().find(|(n, ..)| *n == value).map(|(_, _, p)| p.clone()).filter(|p| !p.is_empty() && value > 0).map_or(Cell::Empty, Cell::Pic)
            };
            out.push((slot, what));
        }
    }
    let camo = format!("{weapon}camo");
    if shown(book, class, &camo) {
        let value = class.get(&camo);
        let what = camo_of(book, class.get(weapon), value).map(|(_, _, p)| p).filter(|p| !p.is_empty() && value > 0).map_or(Cell::Empty, Cell::Pic);
        out.push((camo, what));
    }
    out
}

/// A weapon's section: `PRIMARY | name`, the gun's picture, its cells
/// (under the primary; beside the smaller secondary). On the overview the
/// heading and picture are the weapon's slot (a solid accent band when in
/// focus), each cell its own.
#[allow(clippy::too_many_arguments)]
fn weapon_panel(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, weapon: &str, title: &str, width: f32, k: usize, lit: Option<bool>, f: Option<&Focus>) {
    let gun = book.item(class.get(weapon));
    let cells = weapon_cells(book, class, weapon);
    let big = weapon == "primary";
    let (on, at) = place(f, weapon);
    section(c, cx, width, k, lit).with_children(|s| {
        let names = gun.map(|g| vec![g.image.clone()]).unwrap_or_default();
        let name = gun.map_or_else(|| "None".to_owned(), |g| g.name.clone());
        let pad = if f.is_some() { 8.0 } else { 0.0 };
        let mut head = s.spawn(Node {
            width: cx.px(width),
            flex_direction: if big { FlexDirection::Column } else { FlexDirection::Row },
            align_items: if big { AlignItems::FlexStart } else { AlignItems::Center },
            padding: UiRect::axes(cx.px(pad), cx.px(pad * 0.5)),
            row_gap: cx.px(4.0),
            ..default()
        });
        if on {
            head.insert(lit_band(cx));
        }
        hit(&mut head, at);
        if big {
            head.with_children(|h| {
                heading(h, cx, title, Some(&name));
                h.spawn(Node { width: cx.px(width - 2.0 * pad), justify_content: JustifyContent::FlexStart, ..default() }).with_children(|p| picture_contain(p, cx, &names, 320.0, 96.0));
            });
            s.spawn(Node { width: cx.px(width), ..default() }).with_children(|row| {
                let w = width / 4.0;
                for (i, (slot, what)) in cells.into_iter().enumerate() {
                    let (lit, at) = place(f, &slot);
                    cell(row, cx, what, i == 0, w, 62.0, lit, at);
                }
            });
        } else {
            // (The secondary: its heading over a row of the picture and the
            // cells; the picture's band alone is the weapon's slot.)
            head.with_children(|h| heading(h, cx, title, Some(&name)));
            s.spawn(Node { width: cx.px(width), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                let mut p = row.spawn(Node { width: cx.px(width * 0.4), height: cx.px(66.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() });
                if on {
                    p.insert(lit_band(cx));
                }
                hit(&mut p, at);
                p.with_children(|p| picture_contain(p, cx, &names, 170.0, 60.0));
                let w = width * 0.6 / 3.0;
                for (i, (slot, what)) in cells.into_iter().enumerate() {
                    let (lit, at) = place(f, &slot);
                    cell(row, cx, what, i == 0, w, 62.0, lit, at);
                }
            });
        }
    });
}

/// Lethal or tactical, half of its row: the label, the item's picture and
/// name (and how many, when more than one). On the overview the box is the
/// slot, and a `x1` / `x2` chip after the name its count's row.
#[allow(clippy::too_many_arguments)]
fn small_box(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, slot: &str, label: &str, w: f32, first: bool, f: Option<&Focus>) {
    let item = book.item(class.get(slot));
    let n = class.get(&format!("{slot}count"));
    let (on, at) = place(f, slot);
    let mut e = c.spawn((
        Node {
            width: cx.px(w),
            flex_direction: FlexDirection::Column,
            row_gap: cx.px(4.0),
            padding: UiRect::new(cx.px(if first && f.is_none() { 0.0 } else { 10.0 }), cx.px(6.0), cx.px(2.0), cx.px(2.0)),
            border: UiRect::left(cx.px(if first { 0.0 } else { 1.0 })),
            ..default()
        },
        BorderColor::all(cx.pal.line),
    ));
    if on {
        e.insert(lit_band(cx));
    }
    hit(&mut e, at);
    tally(1);
    e.with_children(|b| {
        b.spawn((Text::new(label.to_owned()), cx.regular(24.0), TextColor(if on { WHITE } else { DIM })));
        b.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, height: cx.px(50.0), ..default() }).with_children(|r| match item {
            Some(i) => {
                picture_contain(r, cx, &[i.image.clone()], 56.0, 46.0);
                let count_row = f.and_then(|f| f.count_row(slot));
                let name = if n > 1 && count_row.is_none() { format!("{}  x{n}", i.name) } else { i.name.clone() };
                r.spawn((Text::new(name), cx.regular(19.0), TextColor(if on { WHITE } else { TEXT })));
                if let (Some(row), Some(f)) = (count_row, f) {
                    let lit = f.lit(&format!("{slot}count"));
                    r.spawn((
                        Hit::Row(row),
                        Button,
                        Node { padding: UiRect::axes(cx.px(8.0), cx.px(2.0)), border: UiRect::all(cx.px(1.0)), ..default() },
                        BorderColor::all(if lit { WHITE } else { WHITE.with_alpha(0.25) }),
                        BackgroundColor(if lit { cx.pal.accent } else { Color::srgba(0.0, 0.0, 0.0, 0.30) }),
                    ))
                    .with_child((Text::new(format!("x{}", n.max(1))), cx.bold(18.0), TextColor(WHITE)));
                }
            }
            None => cell(r, cx, Cell::Empty, true, 56.0, 46.0, false, None),
        });
    });
}

/// A perk's tile: its picture and name over a line in its column's colour
/// (perk 1, 2, 3 as Black Ops II colours them, in the screen's palette).
#[allow(clippy::too_many_arguments)]
fn perk_tile(c: &mut Kids, cx: &mut Ctx, item: Option<&ClassItem>, color: Color, w: f32, h: f32, lit: bool, at: Option<usize>) {
    tally(1);
    let mut e = c.spawn((
        Node { width: cx.px(w), height: cx.px(h), flex_shrink: 0.0, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center, row_gap: cx.px(2.0), overflow: Overflow::clip(), border: UiRect::bottom(cx.px(4.0)), ..default() },
        BorderColor::all(color),
    ));
    if lit {
        e.insert(lit_band(cx));
    } else {
        e.insert(tile_fill(cx, false));
    }
    hit(&mut e, at);
    let pic = (h - 34.0).max(24.0);
    e.with_children(|t| match item {
        Some(i) => {
            picture_contain(t, cx, &[i.image.clone()], 44.0, pic.min(40.0));
            t.spawn((Text::new(i.name.clone()), cx.regular(17.0), TextColor(WHITE)));
        }
        None => cell(t, cx, Cell::Empty, true, 44.0, pic.min(40.0), lit, None),
    });
}

/// The loadout summary, as the reference's: the Pick 10 points, then
/// sections split by thin rules - PRIMARY and SECONDARY (the gun, its
/// attachment and camo cells), LETHAL | TACTICAL, PERKS as tiles (a Perk
/// Greed's extra perk beside its own), WILDCARDS, SCORESTREAKS with the
/// score each takes.
fn loadout(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, left: f32, top: f32, width: f32) {
    loadout_ex(c, cx, book, class, left, top, width, None, None);
}

/// [`loadout`]; beside a picker (`lit`: the section it fills - `primary`,
/// `secondary`, `grenades`, `perks`, `wildcards`, `streaks`) the points
/// are left to the header and every other section is dimmed. With `f` it
/// is the edit screen's class overview: every slot a hit target, the one
/// in focus a solid accent band, the wildcards a row of tiles.
#[allow(clippy::too_many_arguments)]
fn loadout_ex(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, left: f32, top: f32, width: f32, lit: Option<&str>, f: Option<&Focus>) {
    let st = |key: &str| lit.map(|l| l == key);
    c.spawn((Node { width: cx.px(width), flex_direction: FlexDirection::Column, ..abs(Some(left), Some(top), None, None, cx) }, anim::from_left(cx.s, -60.0, 0.05))).with_children(|col| {
        if lit.is_none() && f.is_none() {
            points_bar(col, cx, class.spent, width);
        }
        weapon_panel(col, cx, book, class, "primary", "PRIMARY", width, 0, st("primary"), f);
        let second = if has_card(book, class, "overkill") { "SECOND PRIMARY" } else { "SECONDARY" };
        weapon_panel(col, cx, book, class, "secondary", second, width, 1, st("secondary"), f);
        section(col, cx, width, 2, st("grenades")).with_children(|s| {
            s.spawn(Node { width: cx.px(width), ..default() }).with_children(|row| {
                small_box(row, cx, book, class, "primarygrenade", "LETHAL", width * 0.5, true, f);
                small_box(row, cx, book, class, "specialgrenade", "TACTICAL", width * 0.5, false, f);
            });
        });
        let colors = [ENEMY, WHITE.with_alpha(0.55), cx.pal.accent];
        let perks: Vec<(String, Option<&ClassItem>, Color)> = (1..=3)
            .flat_map(|k| {
                let own = format!("specialty{k}");
                let greed = format!("specialty{}", k + 3);
                let extra = (shown(book, class, &greed) || class.get(&greed) > 0).then(|| (greed.clone(), book.item(class.get(&greed)), colors[k - 1]));
                std::iter::once((own.clone(), book.item(class.get(&own)), colors[k - 1])).chain(extra)
            })
            .collect();
        section(col, cx, width, 3, st("perks")).with_children(|s| {
            heading(s, cx, "PERKS", None);
            let w = (width - 4.0 * (perks.len() as f32 - 1.0)) / perks.len() as f32;
            let h = if f.is_some() { 70.0 } else { 78.0 };
            s.spawn(Node { column_gap: cx.px(4.0), ..default() }).with_children(|row| {
                for (slot, item, color) in perks {
                    let (lit, at) = place(f, &slot);
                    perk_tile(row, cx, item, color, w, h, lit, at);
                }
            });
        });
        let cards: Vec<String> = (1..=3).filter_map(|k| book.item(class.get(&format!("bonuscard{k}")))).map(|i| i.name.clone()).collect();
        section(col, cx, width, 4, st("wildcards")).with_children(|s| {
            let names = if cards.is_empty() { "None".to_owned() } else { cards.join("  ·  ") };
            if f.is_none() {
                heading(s, cx, "WILDCARDS", Some(&names));
                return;
            }
            heading(s, cx, "WILDCARDS", None);
            let w = (width - 8.0) / 3.0;
            s.spawn(Node { column_gap: cx.px(4.0), ..default() }).with_children(|row| {
                for k in 1..=3 {
                    let slot = format!("bonuscard{k}");
                    let (lit, at) = place(f, &slot);
                    wild_tile(row, cx, book.item(class.get(&slot)), w, lit, at);
                }
            });
        });
        streaks(col, cx, book, class, width, st("streaks"), f);
    });
}

/// A wildcard on the overview: its picture and name side by side.
fn wild_tile(c: &mut Kids, cx: &mut Ctx, item: Option<&ClassItem>, w: f32, lit: bool, at: Option<usize>) {
    tally(1);
    let mut e = c.spawn((Node { width: cx.px(w), height: cx.px(50.0), flex_shrink: 0.0, padding: UiRect::horizontal(cx.px(8.0)), column_gap: cx.px(8.0), align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() },));
    if lit {
        e.insert(lit_band(cx));
    } else {
        e.insert(tile_fill(cx, false));
    }
    hit(&mut e, at);
    e.with_children(|t| match item {
        Some(i) => {
            picture_contain(t, cx, &[i.image.clone()], 40.0, 40.0);
            t.spawn((Text::new(i.name.clone()), cx.regular(16.0), TextColor(WHITE), Node { max_width: cx.px(w - 64.0), ..default() }));
        }
        None => cell(t, cx, Cell::Empty, true, w - 16.0, 40.0, lit, None),
    });
}

/// A class at a glance (the in-game Choose Class shows it too).
pub(crate) fn class_card(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, left: f32, top: f32, width: f32) {
    loadout(c, cx, book, class, left, top, width.min(600.0));
}

/// The class's scorestreaks: the score each takes, its picture and name,
/// side by side; on the overview each a slot (a 1-3 badge before it).
fn streaks(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, width: f32, lit: Option<bool>, f: Option<&Focus>) {
    section(c, cx, width, 5, lit).with_children(|s| {
        heading(s, cx, "SCORESTREAKS", None);
        s.spawn(Node { width: cx.px(width), column_gap: cx.px(if f.is_some() { 4.0 } else { 0.0 }), ..default() }).with_children(|row| {
            let w = if f.is_some() { (width - 8.0) / 3.0 } else { width / 3.0 };
            for k in 1..=3 {
                let slot = format!("killstreak{k}");
                let item = book.item(class.get(&slot));
                let (on, at) = place(f, &slot);
                tally(1);
                let mut e = row.spawn(Node { width: cx.px(w), height: cx.px(if f.is_some() { 56.0 } else { 52.0 }), padding: UiRect::horizontal(cx.px(if f.is_some() { 6.0 } else { 0.0 })), column_gap: cx.px(8.0), align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() });
                if on {
                    e.insert(lit_band(cx));
                } else if f.is_some() {
                    e.insert(tile_fill(cx, false));
                }
                hit(&mut e, at);
                e.with_children(|b| {
                    if f.is_some() {
                        badge(b, cx, k);
                    }
                    match item {
                        Some(i) => {
                            if i.momentum > 0 && f.is_none() {
                                b.spawn((Text::new(i.momentum.to_string()), cx.bold(22.0), TextColor(WHITE)));
                            }
                            picture_contain(b, cx, &[i.image.clone()], 44.0, 44.0);
                            b.spawn((Text::new(i.name.clone()), cx.regular(if f.is_some() { 16.0 } else { 18.0 }), TextColor(TEXT), Node { max_width: cx.px(w - if f.is_some() { 96.0 } else { 110.0 }), ..default() }));
                        }
                        None => cell(b, cx, Cell::Empty, true, 44.0, 44.0, on, None),
                    }
                });
            }
        });
    });
}

pub(crate) fn view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, rows: &[Row]) {
    let started = std::time::Instant::now();
    TILES.store(0, std::sync::atomic::Ordering::Relaxed);
    let what = match s.screen {
        Screen::Classes => {
            // As the reference: CREATE A CLASS (a back chevron before it);
            // the classes as Ghosts' ruled rows from the left edge (the one
            // in focus an accent band, its points under them); the class's
            // loadout in the middle; the soldier preview's place at the
            // right (a render replaces it later).
            header_ex(c, cx, &["CREATE A CLASS"], None, true);
            c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(200.0), None, None, cx) }).with_children(|col| {
                ghosts::rows(col, cx, rows, s.focus, ghosts::Look::list(690.0));
            });
            // (The class of the row in focus, by its number: the
            // preview is always the class the row names.)
            if let Some(class) = focused_class(book, rows, s.focus) {
                loadout(c, cx, book, class, 780.0, 186.0, 560.0);
            }
            ghosts::soldier_preview(c, cx, 1420.0, 180.0, 440.0, 760.0);
            "cac classes"
        }
        Screen::ClassEdit => {
            let Some(class) = book.class(s.class) else { return };
            edit_view(c, cx, s, book, class, rows);
            "cac class"
        }
        Screen::ItemPick => {
            let Some(class) = book.class(s.class) else { return };
            pick_view(c, cx, s, book, class);
            "cac picker"
        }
        _ => return,
    };
    // bo2mp: how long the screen took to build, once per screen (and page):
    // on his PC a slow one shows in the log.
    let key = format!("{:?}/{}/{}/{}", s.screen, s.class, s.slot, s.page);
    let mut last = LOGGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *last != key {
        diag::info!(Ui, "{what}: build {:.1} ms, {} tiles", started.elapsed().as_secs_f64() * 1000.0, TILES.load(std::sync::atomic::Ordering::Relaxed));
        *last = key;
    }
}

/// The tiles and boxes the last build drew (the timing line's count).
static TILES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The screen whose build time was last logged.
static LOGGED: Mutex<String> = Mutex::new(String::new());

fn tally(n: usize) {
    TILES.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
}

/// The overview's frame (design units): its left column, the detail's
/// column in the middle, the soldier panel at the right.
const DETAIL_L: f32 = 646.0;
const DETAIL_W: f32 = 600.0;
const SOLDIER_L: f32 = 1290.0;

/// The edit screen, as Ghosts' class screen in the shell's look: the
/// pickers' header (the class's name behind an accent bar; his rank and the
/// Pick 10 points, the number and a thin bar, at the top right); at the
/// left the class overview (every slot a box, the one in focus a solid
/// accent band, a click opening it); in the middle what the slot in focus
/// holds - a weapon as the gun picker's right panel (its picture, what it
/// is, WEAPON STATS), else its icon, name, what it does and its Pick 10
/// cost; at the right the soldier preview's place (a plain dark panel with
/// the class's name: no soldier renders exist yet).
fn edit_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView, rows: &[Row]) {
    let focus_slot = rows.get(s.focus).and_then(|r| match r.act {
        Act::EditSlot(i) => Some(EDIT_SLOTS[i].0),
        Act::Cycle(Setting::LethalCount) => Some("primarygrenadecount"),
        Act::Cycle(Setting::TacticalCount) => Some("specialgrenadecount"),
        _ => None,
    });
    let f = Focus { rows, focus_slot };
    frame_head(c, cx, class, true);
    loadout_ex(c, cx, book, class, L, CARD_T, SUMMARY_W, None, Some(&f));
    if let Some(slot) = focus_slot {
        slot_detail(c, cx, book, class, slot, DETAIL_L, CARD_T + 4.0, DETAIL_W);
    }
    // The soldier preview's place.
    let (ground, rim) = ghosts::panel_ground(cx);
    c.spawn((
        Node {
            width: cx.px(R - SOLDIER_L),
            height: cx.px(770.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            padding: UiRect::all(cx.px(22.0)),
            row_gap: cx.px(4.0),
            border: UiRect::all(cx.px(1.0)).with_top(cx.px(2.0)),
            ..abs(Some(SOLDIER_L), Some(CARD_T), None, None, cx)
        },
        ground,
        rim,
        anim::slide(cx.s, Vec2::new(120.0, 0.0), 0.04),
    ))
    .with_children(|p| {
        p.spawn((Text::new(class.name.clone()), cx.light(44.0), TextColor(WHITE.with_alpha(0.85))));
        p.spawn((Text::new(format!("{} / {POINTS} POINTS", class.spent)), cx.bold(18.0), TextColor(DIM)));
    });
}

/// What the slot in focus holds, in the edit screen's middle column.
#[allow(clippy::too_many_arguments)]
fn slot_detail(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, slot: &str, left: f32, top: f32, w: f32) {
    let pal = cx.pal;
    let count = slot.strip_suffix("count");
    let slot = count.unwrap_or(slot);
    let label = EDIT_SLOTS.iter().find(|(s, _)| *s == slot).map_or("", |(_, l)| *l);
    let weapon = weapon_of(slot);
    let gun = class.get(weapon);
    let value = class.get(slot);
    // A weapon: as the gun picker's right panel.
    if matches!(slot, "primary" | "secondary") {
        let title = if slot == "secondary" && has_card(book, class, "overkill") { "SECOND PRIMARY" } else { label };
        match book.item(value) {
            Some(i) => {
                let p = Pick { value: i.index, name: i.name.to_uppercase(), desc: i.desc.clone(), image: i.image.clone(), cost: i.cost, locked: i.locked, level: i.level, group: group_label(&i.group).to_owned() };
                gun_detail(c, cx, book, &p, 0, left, top, w, Some(title));
            }
            None => empty_detail(c, cx, title, "No weapon in this slot.", left, top, w),
        }
        return;
    }
    // An attachment, or the camo: its picture big, what it is, and the
    // gun's WEAPON STATS under it (as the attachment picker's).
    if slot.contains("attachment") || slot.ends_with("camo") {
        let camo = slot.ends_with("camo");
        let (name, desc, image, cost) = if camo {
            match camo_of(book, gun, value).filter(|_| value > 0) {
                Some((n, name, image)) => {
                    let game = camo_tabs(book).iter().find(|(_, v)| v.iter().any(|(k, ..)| *k == n)).map_or("", |(t, _)| *t);
                    (name, format!("A {} camo. Worn on the gun in matches, first and third person.", title_case(game)), image, 0)
                }
                None => ("NONE".to_owned(), "The gun as it comes.".to_owned(), String::new(), 0),
            }
        } else {
            let att = book.item(gun).and_then(|g| g.attachments.iter().find(|(n, ..)| *n == value && value > 0));
            match att {
                Some((n, name, image)) => {
                    let (reference, desc) = book.att_info.get(&(gun, *n)).cloned().unwrap_or_default();
                    let desc = if desc.is_empty() { format!("{} attachment.", title_case(attachment_group(&reference, name))) } else { desc };
                    (name.to_uppercase(), desc, image.clone(), 1)
                }
                None => ("NONE".to_owned(), "No attachment.".to_owned(), String::new(), 0),
            }
        };
        let gun_name = book.item(gun).map_or_else(String::new, |g| g.name.clone());
        c.spawn((Node { width: cx.px(w), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), ..abs(Some(left), Some(top), None, None, cx) }, anim::from_left(cx.s, -40.0, 0.06))).with_children(|col| {
            col.spawn((Text::new(format!("{gun_name}  ·  {label}")), cx.regular(20.0), TextColor(DIM)));
            col.spawn((Text::new(title_case(&name)), cx.regular(40.0), TextColor(WHITE)));
            col.spawn(Node { width: cx.px(w), height: cx.px(230.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| {
                if image.is_empty() {
                    none_ring(pic, cx, 110.0);
                } else if camo {
                    // (A moving camo turns here: the one slot in focus.)
                    moving_pic(pic, cx, &image, 200.0, 200.0, true);
                } else {
                    picture_contain(pic, cx, std::slice::from_ref(&image), w * 0.6, 200.0);
                }
            });
            col.spawn((Text::new(desc), cx.regular(25.0), TextColor(DIM)));
            if cost > 0 {
                col.spawn((Text::new(format!("{cost} POINT")), cx.bold(20.0), TextColor(pal.accent)));
            }
        });
        stats_panel(c, cx, left, CARD_T + 770.0 - 262.0, w, book.stats.get(&gun).copied(), gun > 0);
        return;
    }
    // A perk, a wildcard, a scorestreak, lethal or tactical: its icon, name,
    // what it does, its Pick 10 cost (as the item picker's right panel).
    let title = match slot {
        "primarygrenade" => "LETHAL",
        "specialgrenade" => "TACTICAL",
        _ => label,
    };
    let item = book.item(value);
    let tier = slot.strip_prefix("specialty").and_then(|n| n.parse::<usize>().ok());
    let (ground, rim) = ghosts::panel_ground(cx);
    c.spawn((
        Node { width: cx.px(w), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), padding: UiRect::all(cx.px(22.0)), border: UiRect::top(cx.px(2.0)), ..abs(Some(left), Some(top - 4.0), None, None, cx) },
        ground,
        rim,
        anim::from_left(cx.s, -40.0, 0.06),
    ))
    .with_children(|col| {
        col.spawn((Text::new(title), cx.regular(20.0), TextColor(DIM)));
        col.spawn(Node { width: Val::Percent(100.0), height: cx.px(170.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| match item {
            Some(i) if !i.image.is_empty() => picture_contain(pic, cx, std::slice::from_ref(&i.image), w - 80.0, 160.0),
            Some(_) => {}
            None => none_ring(pic, cx, 100.0),
        });
        col.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|n| {
            if let Some(k) = slot.strip_prefix("killstreak").and_then(|k| k.parse::<usize>().ok()) {
                badge(n, cx, k);
            }
            n.spawn((Text::new(item.map_or_else(|| "Empty".to_owned(), |i| title_case(&i.name.to_uppercase()))), cx.bold(36.0), TextColor(WHITE)));
        });
        if let Some(t) = tier {
            col.spawn((Node { width: cx.px(80.0), height: cx.px(4.0), ..default() }, BackgroundColor(tier_color(cx, t))));
        }
        let desc = item.map_or_else(|| "Nothing in this slot: ENTER to choose.".to_owned(), |i| i.desc.clone());
        if !desc.is_empty() {
            col.spawn((Text::new(desc), cx.regular(23.0), TextColor(DIM)));
        }
        let mut facts: Vec<(String, String, Color)> = Vec::new();
        if let Some(i) = item {
            if i.momentum > 0 {
                facts.push(("SCORE".to_owned(), i.momentum.to_string(), WHITE));
            }
            let each = if slot.ends_with("grenade") { class.get(&format!("{slot}count")).max(1) } else { 1 };
            if i.cost > 0 {
                let n = i.cost * each;
                facts.push(("PICK 10 COST".to_owned(), format!("{n} POINT{}", if n == 1 { "" } else { "S" }), pal.accent));
            }
            if slot.ends_with("grenade") {
                facts.push(("COUNT".to_owned(), format!("x{each}"), if count.is_some() { pal.accent } else { WHITE }));
            }
        }
        facts.push(("CLASS".to_owned(), format!("{}/{POINTS}", class.spent), if class.spent > POINTS { ENEMY } else { WHITE }));
        for (k, v, color) in facts {
            col.spawn((Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::vertical(cx.px(6.0)), border: UiRect::top(cx.px(1.0)), ..default() }, BorderColor::all(pal.line))).with_children(|r| {
                r.spawn((Text::new(k), cx.regular(20.0), TextColor(DIM)));
                r.spawn((Text::new(v), cx.bold(20.0), TextColor(color)));
            });
        }
        if count.is_some() {
            col.spawn((Text::new("A / D changes how many he carries."), cx.regular(19.0), TextColor(FAINT)));
        }
    });
}

/// An empty weapon slot's detail: its label and a line.
fn empty_detail(c: &mut Kids, cx: &mut Ctx, title: &str, line: &str, left: f32, top: f32, w: f32) {
    c.spawn((Node { width: cx.px(w), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), ..abs(Some(left), Some(top), None, None, cx) }, anim::from_left(cx.s, -40.0, 0.06))).with_children(|col| {
        col.spawn((Text::new(title.to_owned()), cx.regular(20.0), TextColor(DIM)));
        col.spawn((Text::new("Empty"), cx.regular(40.0), TextColor(WHITE)));
        col.spawn(Node { width: cx.px(w), height: cx.px(230.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|p| none_ring(p, cx, 110.0));
        col.spawn((Text::new(line.to_owned()), cx.regular(25.0), TextColor(DIM)));
    });
    stats_panel(c, cx, left, CARD_T + 770.0 - 262.0, w, None, false);
}

/// The Ghosts-style dark band across the top of the Pick 10 screens.
fn title_band(c: &mut Kids, cx: &Ctx) {
    let dark = Color::srgba(0.0, 0.01, 0.03, 0.80);
    c.spawn((
        super::ghosts::top_shade(cx, 160.0),
        BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(dark, Val::Percent(0.0)), ColorStop::new(dark.with_alpha(0.0), Val::Percent(100.0))])),
    ));
}

/// The pickers' frame, in design units: the left and right edges of the
/// screen's content, the card's left edge and top, the summary's width.
const L: f32 = 126.0;
const R: f32 = 1794.0;
const CARD_L: f32 = 640.0;
const CARD_T: f32 = 168.0;
const SUMMARY_W: f32 = 476.0;

/// The loadout summary's section a slot's picker fills.
fn section_of(slot: &str) -> &'static str {
    if slot.ends_with("grenade") {
        "grenades"
    } else if slot.starts_with("primary") {
        "primary"
    } else if slot.starts_with("secondary") {
        "secondary"
    } else if slot.starts_with("specialty") {
        "perks"
    } else if slot.starts_with("bonuscard") {
        "wildcards"
    } else {
        "streaks"
    }
}

/// A perk column's colour (1-3; Perk Greed's 4-6 as their own), as the
/// loadout summary's tiles: red, grey, the accent.
fn tier_color(cx: &Ctx, n: usize) -> Color {
    match (n.max(1) - 1) % 3 {
        0 => ENEMY,
        1 => WHITE.with_alpha(0.55),
        _ => cx.pal.accent,
    }
}

/// A scorestreak's place in the class (1-3): a small square in the accent.
fn badge(c: &mut Kids, cx: &Ctx, k: usize) {
    c.spawn((
        Node { width: cx.px(24.0), height: cx.px(24.0), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(cx.pal.accent),
    ))
    .with_child((Text::new(k.to_string()), cx.black(17.0), TextColor(Color::BLACK)));
}

/// The pickers' frame, as the owner's references: the class's name at the
/// top left after an accent bar, his rank and the class's Pick 10 points at
/// the top right, a rule under them, and the class's loadout summary at
/// the left, dimmed but for the section this picker fills.
fn frame(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, class: &ClassView, slot: &str) {
    frame_head(c, cx, class, false);
    loadout_ex(c, cx, book, class, L, CARD_T, SUMMARY_W, Some(section_of(slot)), None);
}

/// The pickers' and the edit screen's header: the class's name behind an
/// accent bar, his rank and the Pick 10 points at the top right (`bar`: a
/// thin bar of the ten points under the number), the rule under them.
fn frame_head(c: &mut Kids, cx: &mut Ctx, class: &ClassView, bar: bool) {
    let pal = cx.pal;
    title_band(c, cx);
    c.spawn((Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, ..abs(Some(L), Some(54.0), None, None, cx) }, anim::from_left(cx.s, 60.0, 0.0))).with_children(|t| {
        t.spawn((Node { width: cx.px(7.0), height: cx.px(46.0), ..default() }, BackgroundColor(pal.accent)));
        t.spawn((Text::new(class.name.clone()), cx.light(60.0), TextColor(WHITE)));
    });
    // (His rank from the front end's player card; the points are Pick 10's.)
    let me = super::bridge::catalog().map(|c| c.player.clone());
    let icon = me.as_ref().and_then(|p| cx.pic(&[p.rank_picture.clone()]));
    c.spawn(Node { column_gap: cx.px(56.0), align_items: AlignItems::Center, ..abs(None, Some(66.0), Some(cx.w - R), None, cx) }).with_children(|r| {
        if let Some(p) = &me {
            r.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|k| {
                k.spawn((Text::new("Rank"), cx.regular(30.0), TextColor(TEXT)));
                if let Some(h) = icon {
                    k.spawn((ImageNode::new(h), Node { width: cx.px(38.0), height: cx.px(38.0), ..default() }));
                }
                k.spawn((Text::new(p.level.to_string()), cx.regular(30.0), TextColor(WHITE)));
                if p.prestige > 0 {
                    k.spawn((Text::new(format!("P{}", p.prestige)), cx.bold(22.0), TextColor(GOLD)));
                }
            });
        }
        r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(6.0), align_items: AlignItems::FlexEnd, ..default() }).with_children(|col| {
            col.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|k| {
                k.spawn((Text::new("Points"), cx.regular(30.0), TextColor(TEXT)));
                k.spawn((Node { width: cx.px(12.0), height: cx.px(12.0), margin: UiRect::horizontal(cx.px(4.0)), ..default() }, BackgroundColor(pal.accent), UiTransform::from_rotation(Rot2::degrees(45.0))));
                k.spawn((Text::new(format!("{}/{POINTS}", class.spent)), cx.regular(30.0), TextColor(if class.spent > POINTS { ENEMY } else { WHITE })));
            });
            // (Pick 10 as a slim bar: the points spent lit.)
            if bar {
                col.spawn(Node { width: cx.px(170.0), height: cx.px(3.0), column_gap: cx.px(3.0), ..default() }).with_children(|b| {
                    for k in 0..POINTS {
                        b.spawn((Node { flex_grow: 1.0, ..default() }, BackgroundColor(if k < class.spent { if class.spent > POINTS { ENEMY } else { pal.accent } } else { WHITE.with_alpha(0.16) })));
                    }
                });
            }
        });
    });
    c.spawn((Node { width: cx.px(R - L), height: cx.px(1.0), ..abs(Some(L), Some(140.0), None, None, cx) }, BackgroundColor(pal.line)));
}

/// A picker's card: a header with Ghosts' cut corner at its top left and
/// the title, an accent rule under it, then the card's dark ground (`body`
/// fills it). Slides in from the right.
#[allow(clippy::too_many_arguments)]
fn card(c: &mut Kids, cx: &mut Ctx, title: &str, left: f32, top: f32, w: f32, h: f32, body: impl FnOnce(&mut Kids, &mut Ctx)) {
    let p = cx.pal.panel.to_srgba();
    c.spawn((Node { width: cx.px(w), height: cx.px(h), flex_direction: FlexDirection::Column, ..abs(Some(left), Some(top), None, None, cx) }, anim::from_left(cx.s, -50.0, 0.04)))
        .with_children(|col| {
            card_head(col, cx, title);
            let ground = BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.90), Val::Percent(0.0)),
                ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.74), Val::Percent(100.0)),
            ]));
            col.spawn((
                Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::clip(), border: UiRect { left: cx.px(1.0), right: cx.px(1.0), bottom: cx.px(1.0), ..default() }, ..default() },
                BorderColor::all(cx.pal.line),
                ground,
            ))
            .with_children(|b| body(b, cx));
        });
}

/// A card's header: the cut corner (a filled triangle, its diagonal from
/// the bottom left up to the right), the band with the title, the accent
/// rule under them.
fn card_head(c: &mut Kids, cx: &Ctx, title: &str) {
    let (h, k) = (56.0, 30.0);
    let p = cx.pal.panel.to_srgba();
    let fill = Color::srgba(p.red + 0.05, p.green + 0.06, p.blue + 0.08, 0.96);
    c.spawn(Node { height: cx.px(h), flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn(Node { width: cx.px(k), height: cx.px(h), flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() }).with_children(|corner| {
            let d = k * std::f32::consts::SQRT_2;
            corner.spawn(Node { width: cx.px(k), height: cx.px(k), overflow: Overflow::clip(), ..default() }).with_child((
                Node { position_type: PositionType::Absolute, left: cx.px(k - d * 0.5), top: cx.px(k - d * 0.5), width: cx.px(d), height: cx.px(d), ..default() },
                BackgroundColor(fill),
                UiTransform::from_rotation(Rot2::degrees(45.0)),
            ));
            corner.spawn((Node { width: cx.px(k), flex_grow: 1.0, ..default() }, BackgroundColor(fill)));
        });
        r.spawn((Node { flex_grow: 1.0, height: cx.px(h), align_items: AlignItems::Center, padding: UiRect::left(cx.px(8.0)), ..default() }, BackgroundColor(fill)))
            .with_child((Text::new(title.to_owned()), cx.bold(30.0), TextColor(WHITE)));
    });
    c.spawn((Node { height: cx.px(2.0), flex_shrink: 0.0, ..default() }, fade_right(cx.pal.accent, cx.pal.accent.with_alpha(0.0))));
}

/// The keys under a card: (key, the pad's button, what it does).
fn card_hint(c: &mut Kids, cx: &Ctx, left: f32, top: f32, keys: &[(&str, &str, &str)]) {
    let key = lift(cx.pal.accent, 0.18);
    c.spawn((Node { column_gap: cx.px(28.0), align_items: AlignItems::Center, ..abs(Some(left), Some(top), None, None, cx) }, anim::rise(cx.s, 14.0, 0.10))).with_children(|r| {
        for (k, pad, what) in keys {
            r.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
                h.spawn((Text::new(if cx.pad { *pad } else { *k }), cx.bold(20.0), TextColor(key)));
                h.spawn((Text::new(*what), cx.regular(20.0), TextColor(DIM)));
            });
        }
    });
}

/// The run of tabs that fits `room` around the one in use: (first, last).
fn tab_span(labels: &[String], on: usize, room: f32, width: impl Fn(&str) -> f32) -> (usize, usize) {
    if labels.is_empty() {
        return (0, 0);
    }
    let on = on.min(labels.len() - 1);
    let (mut first, mut last, mut used) = (on, on, width(&labels[on]));
    loop {
        let before = first.checked_sub(1).map(|i| width(&labels[i])).filter(|w| used + w <= room);
        let after = (last + 1 < labels.len()).then(|| width(&labels[last + 1])).filter(|w| used + w <= room);
        match (before, after) {
            (Some(b), _) if on - first <= last - on || after.is_none() => {
                first -= 1;
                used += b;
            }
            (_, Some(a)) => {
                last += 1;
                used += a;
            }
            _ => break,
        }
    }
    (first, last)
}

/// A card's row of game tabs (Q / E, or a click): the one in use bright
/// over an accent line; more than fit scroll with it (`‹` / `›` for those
/// cut off, a click on one goes there).
fn tab_row(c: &mut Kids, cx: &mut Ctx, labels: &[String], on: usize, w: f32) {
    if labels.len() < 2 {
        return;
    }
    let pal = cx.pal;
    let (first, last) = tab_span(labels, on, w - 220.0, |l| l.chars().count() as f32 * 12.5 + 30.0);
    c.spawn((
        Node { height: cx.px(50.0), flex_shrink: 0.0, padding: UiRect::horizontal(cx.px(16.0)), column_gap: cx.px(2.0), align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() },
        BorderColor::all(pal.line),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22)),
    ))
    .with_children(|t| {
        super::view::hint(t, cx, "Q", "xenonbutton_lb", "");
        let more = |t: &mut Kids, cx: &Ctx, glyph: &str, to: usize| {
            t.spawn((Hit::Page(to), Button, Node { padding: UiRect::horizontal(cx.px(8.0)), ..default() })).with_child((Text::new(glyph.to_owned()), cx.bold(28.0), TextColor(DIM)));
        };
        if first > 0 {
            more(t, cx, "‹", first - 1);
        }
        for (i, label) in labels.iter().enumerate().take(last + 1).skip(first) {
            let sel = i == on;
            t.spawn((
                Hit::Page(i),
                Button,
                Node { height: cx.px(50.0), padding: UiRect::horizontal(cx.px(12.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(if sel { pal.accent_a(0.16) } else { Color::NONE }),
            ))
            .with_children(|b| {
                b.spawn((Text::new(label.clone()), if sel { cx.black(20.0) } else { cx.bold(19.0) }, TextColor(if sel { WHITE } else { DIM })));
                if sel {
                    b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(0.0), height: cx.px(3.0), ..default() }, BackgroundColor(pal.accent)));
                }
            });
        }
        if last + 1 < labels.len() {
            more(t, cx, "›", last + 1);
        }
        super::view::hint(t, cx, "E", "xenonbutton_rb", "");
    });
}

/// The first row of a scrolled grid: where it was, moved just enough to
/// show the row in focus.
fn scroll_rows(focus_row: usize, rows: usize, total: usize) -> usize {
    use std::sync::atomic::Ordering::Relaxed;
    let mut first = super::GRID_FIRST_ROW.load(Relaxed);
    if focus_row < first {
        first = focus_row;
    }
    if focus_row >= first + rows {
        first = focus_row + 1 - rows;
    }
    first = first.min(total.saturating_sub(rows));
    super::GRID_FIRST_ROW.store(first, Relaxed);
    first
}

/// A scroll bar: a faint track `h` high, the thumb where the rows shown
/// are among them all (nothing when they all fit).
fn scrollbar(c: &mut Kids, cx: &Ctx, h: f32, first: usize, shown: usize, total: usize) {
    if total <= shown {
        return;
    }
    let thumb = (h * shown as f32 / total as f32).max(30.0);
    let at = (h - thumb) * first as f32 / (total - shown).max(1) as f32;
    c.spawn((Node { width: cx.px(6.0), height: cx.px(h), ..default() }, BackgroundColor(WHITE.with_alpha(0.07)))).with_child((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: cx.px(at), height: cx.px(thumb), ..default() },
        BackgroundColor(WHITE.with_alpha(0.40)),
    ));
}

/// A picture fitted in a box, keeping its shape; a flipbook (Modern Warfare
/// III's moving pictures) turns, as the calling cards' do.
/// `animate`: false draws it still (only the tile in focus turns: a page of
/// moving camos read every frame of every one of them).
fn moving_pic(c: &mut Kids, cx: &mut Ctx, name: &str, w: f32, h: f32, animate: bool) {
    let pic = cx.pic(&[name.to_owned()]);
    let book = if animate { super::view::flipbook(cx, name) } else { None };
    let size = pic.as_ref().map(|p| cx.pic_size(p));
    c.spawn(Node { width: cx.px(w), height: cx.px(h), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|b| {
        if let (Some(p), Some(size)) = (pic, size) {
            let k = (w / size.x.max(1.0)).min(h / size.y.max(1.0));
            let mut e = b.spawn((ImageNode::new(p), Node { width: cx.px(size.x * k), height: cx.px(size.y * k), ..default() }));
            if let Some(book) = book {
                e.insert(book);
            }
        }
    });
}

/// NONE's picture: a ring with a stroke through it.
fn none_ring(c: &mut Kids, cx: &Ctx, size: f32) {
    c.spawn((
        Node { width: cx.px(size), height: cx.px(size), border: UiRect::all(cx.px(size / 16.0)), border_radius: BorderRadius::all(cx.px(size / 2.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BorderColor::all(WHITE.with_alpha(0.35)),
    ))
    .with_child((Node { width: cx.px(size / 16.0), height: cx.px(size * 0.94), ..default() }, BackgroundColor(WHITE.with_alpha(0.35)), UiTransform::from_rotation(Rot2::degrees(45.0))));
}

/// The mark of what the class has in the slot: a small accent diamond at a
/// tile's top right.
fn equipped_mark(c: &mut Kids, cx: &Ctx) {
    c.spawn((
        Node { position_type: PositionType::Absolute, right: cx.px(8.0), top: cx.px(8.0), width: cx.px(11.0), height: cx.px(11.0), ..default() },
        BackgroundColor(cx.pal.accent),
        UiTransform::from_rotation(Rot2::degrees(45.0)),
    ));
}

/// WEAPON STATS, as the references: a header between `‹` `›` (drawn faint:
/// there is one page of stats), then Accuracy, Damage, Range, Fire Rate and
/// Mobility with a white bar each (mp/statstable.csv's numbers, out of
/// 100). `bars` None: the labels alone (no gun in focus, or no numbers for
/// it - a line says which).
fn stats_panel(c: &mut Kids, cx: &Ctx, left: f32, top: f32, w: f32, bars: Option<[i32; 5]>, gun: bool) {
    let pal = cx.pal;
    let p = pal.panel.to_srgba();
    c.spawn((
        Node { width: cx.px(w), flex_direction: FlexDirection::Column, padding: UiRect::bottom(cx.px(8.0)), border: UiRect::all(cx.px(1.0)), ..abs(Some(left), Some(top), None, None, cx) },
        BorderColor::all(pal.line),
        BackgroundColor(Color::srgba(p.red, p.green, p.blue, 0.80)),
        anim::rise(cx.s, 24.0, 0.08),
    ))
    .with_children(|panel| {
        panel
            .spawn((
                Node { height: cx.px(48.0), padding: UiRect::horizontal(cx.px(14.0)), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() },
                BorderColor::all(pal.line),
                BackgroundColor(WHITE.with_alpha(0.04)),
            ))
            .with_children(|hd| {
                hd.spawn((Text::new("‹"), cx.bold(34.0), TextColor(FAINT)));
                hd.spawn((Text::new("WEAPON STATS"), cx.bold(28.0), TextColor(WHITE)));
                hd.spawn((Text::new("›"), cx.bold(34.0), TextColor(FAINT)));
            });
        let track = w * 0.58 - 30.0;
        for (i, label) in ["Accuracy", "Damage", "Range", "Fire Rate", "Mobility"].iter().enumerate() {
            panel.spawn(Node { height: cx.px(40.0), column_gap: cx.px(18.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(Node { width: cx.px(w * 0.42 - 18.0), justify_content: JustifyContent::FlexEnd, ..default() })
                    .with_child((Text::new(*label), cx.regular(25.0), TextColor(if bars.is_some() { TEXT } else { FAINT })));
                if let Some(b) = bars {
                    r.spawn((Node { width: cx.px(track), height: cx.px(10.0), ..default() }, BackgroundColor(WHITE.with_alpha(0.08))))
                        .with_child((Node { width: Val::Percent(b[i].clamp(0, 100) as f32), height: Val::Percent(100.0), ..default() }, BackgroundColor(WHITE)));
                }
            });
        }
        if bars.is_none() && gun {
            panel.spawn((Text::new("No stat numbers for this gun."), cx.regular(18.0), TextColor(FAINT), Node { margin: UiRect::left(cx.px(20.0)), ..default() }));
        }
    });
}

/// A slot's picker: the frame (the class's name, rank and points, the
/// dimmed summary) and the slot's card - guns, attachments, camos, or the
/// rest (lethal, tactical, perks, wildcards, scorestreaks).
fn pick_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView) {
    let slot = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].0;
    frame(c, cx, book, class, slot);
    if matches!(slot, "primary" | "secondary") {
        gun_view(c, cx, s, book, class);
    } else if slot.contains("attachment") {
        attachment_view(c, cx, s, book, class);
    } else if slot.ends_with("camo") {
        camo_view(c, cx, s, book, class);
    } else {
        item_view(c, cx, s, book, class);
    }
}

/// CHOOSE CAMO, as the owner's reference: a tab per game whose camos the
/// gun takes (Q / E), then a scrolled grid of five a row (each the swatch
/// in a dark tile, its name under it, dim but for the one in focus; the
/// class's camo marked), a scroll bar at the right, and at the foot the
/// camo in focus: its name and where it comes from at the left, EQUIPPED
/// or its game at the right. (No camo here has an unlock challenge, so
/// none is locked or new.)
fn camo_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView) {
    let pal = cx.pal;
    let slot = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].0;
    let all = picks_arc(s, book);
    let (tabs, picks) = (&all.0, &all.1);
    let tab = s.page.min(tabs.len().saturating_sub(1));
    let have = class.get(slot);
    let (w, h) = (R - CARD_L, 770.0);
    let cols = 5usize;
    super::GRID_COLS.store(cols, std::sync::atomic::Ordering::Relaxed);
    let (pad, gap, bar) = (18.0, 12.0, 22.0);
    let tw = (w - 2.0 * pad - bar - gap * (cols as f32 - 1.0)) / cols as f32;
    let (pic_h, name_h, info_h) = (118.0, 34.0, 112.0);
    let th = pic_h + 4.0 + name_h;
    let grid_h = h - 58.0 - if tabs.len() > 1 { 50.0 } else { 0.0 } - info_h - 2.0 * pad;
    let rows = ((grid_h + gap) / (th + gap)).floor().max(1.0) as usize;
    let total_rows = picks.len().div_ceil(cols).max(1);
    let first_row = scroll_rows(s.focus / cols, rows, total_rows);
    let (first, last) = (first_row * cols, ((first_row + rows) * cols).min(picks.len()));
    let game = tabs.get(tab).cloned().unwrap_or_default();
    card(c, cx, "CHOOSE CAMO", CARD_L, CARD_T, w, h, |b, cx| {
        tab_row(b, cx, tabs, tab, w);
        b.spawn(Node { height: cx.px(grid_h + 2.0 * pad), padding: UiRect::all(cx.px(pad)), column_gap: cx.px(bar - 6.0), flex_shrink: 0.0, ..default() }).with_children(|area| {
            area.spawn(Node { width: cx.px(w - 2.0 * pad - bar), flex_wrap: FlexWrap::Wrap, align_content: AlignContent::FlexStart, column_gap: cx.px(gap), row_gap: cx.px(gap), ..default() }).with_children(|grid| {
                for (k, p) in picks.iter().enumerate().take(last).skip(first) {
                    let on = k == s.focus;
                    tally(1);
                    grid.spawn((
                        Hit::Row(k),
                        Button,
                        Node { width: cx.px(tw), height: cx.px(th), flex_direction: FlexDirection::Column, row_gap: cx.px(4.0), overflow: Overflow::clip(), ..default() },
                        anim::rise(cx.s, 24.0, ((k - first) as f32 * 0.010).min(0.16)),
                    ))
                    .with_children(|t| {
                        t.spawn((
                            Node { width: cx.px(tw), height: cx.px(pic_h), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(cx.px(if on { 2.0 } else { 1.0 })), ..default() },
                            BorderColor::all(if on { pal.accent } else { WHITE.with_alpha(0.05) }),
                            BackgroundColor(if on { pal.accent_a(0.22) } else { Color::srgba(0.10, 0.11, 0.13, 0.85) }),
                        ))
                        .with_children(|pic| {
                            if p.image.is_empty() {
                                none_ring(pic, cx, 54.0);
                            } else {
                                moving_pic(pic, cx, &p.image, pic_h * 0.72, pic_h * 0.72, on);
                            }
                            if p.value == have {
                                equipped_mark(pic, cx);
                            }
                        });
                        t.spawn(Node { width: cx.px(tw), height: cx.px(name_h), justify_content: JustifyContent::Center, ..default() }).with_child((
                            Text::new(title_case(&p.name)),
                            cx.regular(21.0),
                            TextColor(if on { lift(pal.accent, 0.25) } else if p.value == have { WHITE } else { DIM }),
                        ));
                    });
                }
            });
            area.spawn(Node { width: cx.px(6.0), ..default() }).with_children(|sb| scrollbar(sb, cx, grid_h, first_row, rows, total_rows));
        });
        b.spawn((
            Node { height: cx.px(info_h), flex_shrink: 0.0, margin: UiRect::top(Val::Auto), padding: UiRect::axes(cx.px(18.0), cx.px(12.0)), justify_content: JustifyContent::SpaceBetween, border: UiRect::top(cx.px(1.0)), ..default() },
            BorderColor::all(pal.line),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
        ))
        .with_children(|info| {
            let Some(p) = picks.get(s.focus) else { return };
            info.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(4.0), max_width: cx.px(w * 0.72), ..default() }).with_children(|t| {
                t.spawn((Text::new(title_case(&p.name)), cx.regular(30.0), TextColor(WHITE)));
                t.spawn((Text::new(p.desc.clone()), cx.regular(22.0), TextColor(DIM)));
            });
            let (state, color) = if p.value == have { ("EQUIPPED".to_owned(), pal.accent) } else { (game.clone(), TEXT) };
            info.spawn((Text::new(state), cx.bold(26.0), TextColor(color)));
        });
    });
    card_hint(c, cx, CARD_L, CARD_T + h + 12.0, &[("Q / E", "LB / RB", "GAMES"), ("ENTER", "A", "EQUIP"), ("ESC", "B", "BACK")]);
}

/// The attachment picker, as the owner's reference: a card with a row per
/// kind (OPTICS, BARREL, ...: the label at the left, the attachments'
/// pictures in dark squares, the one in focus in the accent); under it the
/// attachment in focus (name, what it does, where the class has it) and
/// WEAPON STATS at the right - the gun's own (no per-attachment numbers
/// exist, so no change is shown).
fn attachment_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView) {
    let pal = cx.pal;
    let (slot, label) = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)];
    let weapon = weapon_of(slot);
    let all = picks_arc(s, book);
    let picks = &all.1;
    let cats = categories(picks);
    let have = class.get(slot);
    // (Where else on this gun the class has an attachment: its slot.)
    let elsewhere = |v: i32| (1..=3).find(|k| v > 0 && format!("{weapon}attachment{k}") != slot && class.get(&format!("{weapon}attachment{k}")) == v);
    let (w, h) = (R - CARD_L, 480.0);
    card(c, cx, label, CARD_L, CARD_T, w, h, |b, cx| {
        b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(cx.px(6.0)), ..default() }).with_children(|list| {
            for (ci, &(a, e)) in cats.iter().enumerate() {
                let lit = (a..e).contains(&s.focus);
                let name = if picks[a].group.is_empty() { "None".to_owned() } else { title_case(&picks[a].group) };
                list.spawn((
                    Node { padding: UiRect::vertical(cx.px(7.0)), column_gap: cx.px(24.0), align_items: AlignItems::Center, ..default() },
                    BackgroundColor(if lit { WHITE.with_alpha(0.035) } else { Color::NONE }),
                    anim::from_left(cx.s, -30.0, 0.05 + ci as f32 * 0.02),
                ))
                .with_children(|row| {
                    row.spawn(Node { width: cx.px(220.0), flex_shrink: 0.0, justify_content: JustifyContent::FlexEnd, ..default() })
                        .with_child((Text::new(name), cx.regular(28.0), TextColor(if lit { WHITE } else { TEXT })));
                    row.spawn(Node { width: cx.px(w - 300.0), flex_wrap: FlexWrap::Wrap, column_gap: cx.px(12.0), row_gap: cx.px(10.0), ..default() }).with_children(|tiles| {
                        for (k, p) in picks.iter().enumerate().take(e).skip(a) {
                            let on = k == s.focus;
                            tally(1);
                            tiles
                                .spawn((
                                    Hit::Row(k),
                                    Button,
                                    Node { width: cx.px(96.0), height: cx.px(86.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(cx.px(if on { 2.0 } else { 1.0 })), ..default() },
                                    BorderColor::all(if on { pal.accent } else { WHITE.with_alpha(0.05) }),
                                    BackgroundColor(if on { pal.accent_a(0.24) } else { Color::srgba(0.10, 0.11, 0.13, 0.85) }),
                                ))
                                .with_children(|t| {
                                    if p.value == 0 {
                                        none_ring(t, cx, 44.0);
                                    } else if p.image.is_empty() {
                                        t.spawn((Text::new(title_case(&p.name)), cx.regular(15.0), TextColor(TEXT), Node { max_width: cx.px(88.0), ..default() }));
                                    } else {
                                        picture_contain(t, cx, std::slice::from_ref(&p.image), 80.0, 66.0);
                                    }
                                    if p.value == have {
                                        equipped_mark(t, cx);
                                    } else if let Some(n) = elsewhere(p.value) {
                                        t.spawn((Text::new(n.to_string()), cx.bold(15.0), TextColor(DIM), abs(None, Some(3.0), Some(6.0), None, cx)));
                                    }
                                });
                        }
                    });
                });
            }
        });
    });
    card_hint(c, cx, CARD_L, CARD_T + h + 8.0, &[("A / D", "D-PAD", "ATTACHMENTS"), ("W / S", "D-PAD", "KINDS"), ("ENTER", "A", "EQUIP")]);
    let foot = CARD_T + h + 50.0;
    if let Some(p) = picks.get(s.focus) {
        c.spawn((Node { width: cx.px(540.0), flex_direction: FlexDirection::Column, row_gap: cx.px(10.0), ..abs(Some(CARD_L + 16.0), Some(foot), None, None, cx) }, anim::rise(cx.s, 20.0, 0.06))).with_children(|t| {
            t.spawn((Text::new(title_case(&p.name)), cx.regular(44.0), TextColor(WHITE)));
            let line = if p.desc.is_empty() && p.value > 0 { format!("{} attachment.", title_case(&p.group)) } else { p.desc.clone() };
            t.spawn((Text::new(line), cx.regular(26.0), TextColor(DIM)));
            let state = if p.value == have && p.value > 0 {
                Some(("EQUIPPED".to_owned(), pal.accent))
            } else {
                elsewhere(p.value).map(|n| (format!("ON ATTACHMENT {n}: choosing it moves it here"), TEXT))
            };
            if let Some((text, color)) = state {
                t.spawn((Text::new(text), cx.bold(20.0), TextColor(color)));
            }
            if p.cost > 0 {
                t.spawn((Text::new(format!("{} POINT{}", p.cost, if p.cost == 1 { "" } else { "S" })), cx.bold(20.0), TextColor(pal.accent)));
            }
        });
    }
    let gun = class.get(weapon);
    stats_panel(c, cx, CARD_L + 600.0, foot, R - CARD_L - 600.0, book.stats.get(&gun).copied(), gun > 0);
}

/// A category's name in the gun picker.
fn category_name(picks: &[Pick], start: usize) -> String {
    let p = &picks[start];
    if !p.group.is_empty() {
        p.group.clone()
    } else if p.value == 0 {
        "NONE".to_owned()
    } else {
        "OTHER".to_owned()
    }
}

/// The gun picker, as the owner's references: a card titled PRIMARY /
/// SECONDARY with a tab per game (Q / E: Black Ops II's, then each game's
/// whose guns are in), its categories as ruled rows, the one in focus open
/// under its row as a strip of its guns (A / D through them, W / S between
/// categories, as before); at the right the gun in focus: name, picture,
/// what it is, its game / points / level, and WEAPON STATS.
fn gun_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView) {
    let pal = cx.pal;
    let slot = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)].0;
    let title = match slot {
        "secondary" if has_card(book, class, "overkill") => "SECOND PRIMARY",
        "secondary" => "SECONDARY",
        _ => "PRIMARY",
    };
    let all = picks_arc(s, book);
    let (tabs, picks) = (&all.0, &all.1);
    let tab = s.page.min(tabs.len().saturating_sub(1));
    let cats = categories(picks);
    let Some(at) = cats.iter().position(|(a, b)| (*a..*b).contains(&s.focus)) else { return };
    let have = class.get(slot);
    let (w, h) = (640.0, 770.0);
    card(c, cx, title, CARD_L, CARD_T, w, h, |b, cx| {
        tab_row(b, cx, tabs, tab, w);
        b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(cx.px(10.0), cx.px(6.0)), ..default() }).with_children(|list| {
            for (ci, &(a, e)) in cats.iter().enumerate() {
                let on = ci == at;
                let mut row = list.spawn((
                    Node { height: cx.px(50.0), padding: UiRect::horizontal(cx.px(18.0)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, border: UiRect::bottom(cx.px(1.0)), ..default() },
                    BorderColor::all(pal.line),
                    if on { fade_right(pal.accent_a(0.55), pal.accent_a(0.06)) } else { fade_right(Color::NONE, Color::NONE) },
                    anim::from_left(cx.s, -30.0, 0.05 + ci as f32 * 0.015),
                ));
                // (Another category's row opens it under the mouse; the
                // open one takes no click, so a click never picks a gun
                // the player did not point at.)
                if !on {
                    row.insert((Hit::Row(a), Button));
                }
                row.with_children(|r| {
                    r.spawn((Text::new(category_name(picks, a)), cx.regular(27.0), TextColor(if on { WHITE } else { TEXT })));
                    r.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|n| {
                        if (a..e).any(|k| picks[k].value == have && have != 0) {
                            n.spawn((Node { width: cx.px(10.0), height: cx.px(10.0), ..default() }, BackgroundColor(pal.accent), UiTransform::from_rotation(Rot2::degrees(45.0))));
                        }
                        if picks[a].value != 0 && super::coming::of_value(picks[a].value).is_none() {
                            n.spawn((Text::new((e - a).to_string()), cx.bold(20.0), TextColor(DIM)));
                        }
                    });
                });
                if on && !(e - a == 1 && picks[a].value == 0) {
                    gun_strip(list, cx, s, picks, (a, e), have, w - 20.0);
                }
            }
        });
        // (The note under the list: NONE's line, else the keys.)
        let p = &picks[s.focus];
        let note = if p.value == 0 { p.desc.clone() } else { String::new() };
        if !note.is_empty() {
            b.spawn((Text::new(note), cx.regular(26.0), TextColor(TEXT), Node { margin: UiRect::all(cx.px(22.0)), ..default() }));
        }
    });
    card_hint(c, cx, CARD_L, CARD_T + h + 12.0, &[("Q / E", "LB / RB", "GAMES"), ("W / S", "D-PAD", "CATEGORIES"), ("A / D", "D-PAD", "WEAPONS")]);
    // The gun in focus.
    let (rl, rw) = (CARD_L + w + 40.0, R - CARD_L - w - 40.0);
    gun_detail(c, cx, book, &picks[s.focus], have, rl, CARD_T + 4.0, rw, None);
}

/// A gun's panel (the gun picker's right column; the edit screen's middle
/// with the weapon in focus): its name, picture, what it is, its game /
/// points / level, and WEAPON STATS at the foot. `label`: a dim line over
/// the name (the slot, on the edit screen).
#[allow(clippy::too_many_arguments)]
fn gun_detail(c: &mut Kids, cx: &mut Ctx, book: &ClassBook, p: &Pick, have: i32, rl: f32, top: f32, rw: f32, label: Option<&str>) {
    let pal = cx.pal;
    c.spawn((Node { width: cx.px(rw), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), ..abs(Some(rl), Some(top), None, None, cx) }, anim::from_left(cx.s, -40.0, 0.06))).with_children(|col| {
        if let Some(l) = label {
            col.spawn((Text::new(l.to_owned()), cx.regular(20.0), TextColor(DIM)));
        }
        if p.value != 0 {
            col.spawn((Text::new(title_case(&p.name)), cx.regular(40.0), TextColor(WHITE)));
        }
        col.spawn(Node { width: cx.px(rw), height: cx.px(230.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| {
            if let Some(g) = super::coming::of_value(p.value) {
                super::coming::banner(pic, cx, g, rw, 220.0);
            } else if p.locked {
                lock(pic, cx);
            } else if !p.image.is_empty() {
                picture_contain(pic, cx, std::slice::from_ref(&p.image), rw * 0.92, 220.0);
            }
        });
        if p.value != 0 && !p.desc.is_empty() {
            col.spawn((Text::new(if p.locked { format!("Unlocks at level {}.", p.level) } else { p.desc.clone() }), cx.regular(25.0), TextColor(DIM)));
        }
        let facts: Vec<(String, Color)> = [
            (super::coming::of_value(p.value).map_or_else(|| if p.value > 0 { game_full(p.value).to_owned() } else { String::new() }, |g| g.title.to_owned()), TEXT),
            (if p.cost > 0 { format!("{} POINT{}", p.cost, if p.cost == 1 { "" } else { "S" }) } else { String::new() }, pal.accent),
            (if p.locked { format!("LEVEL {}", p.level) } else { String::new() }, GOLD),
            (if p.value == have && p.value != 0 { "EQUIPPED".to_owned() } else { String::new() }, pal.accent),
        ]
        .into_iter()
        .filter(|(t, _)| !t.is_empty())
        .collect();
        col.spawn(Node { column_gap: cx.px(18.0), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|f| {
            for (t, color) in facts {
                f.spawn((Text::new(t), cx.bold(20.0), TextColor(color)));
            }
        });
    });
    let gun = p.value > 0 && super::coming::of_value(p.value).is_none();
    stats_panel(c, cx, rl, CARD_T + 770.0 - 262.0, rw, if gun { book.stats.get(&p.value).copied() } else { None }, gun);
}

/// The first gun shown of a category's strip of `n`, `fit` tiles wide, with
/// the gun `at` in focus. The strip moves a page at a time (the tiles
/// `k * fit ..`, the last page possibly short, so pages never overlap), so it
/// depends on the focus only through its page: the mouse on a shown tile
/// (which focuses it) and A / D within the page never scroll it, every gun
/// is on some page, and the strip moves only when the focus leaves it.
fn strip_start(at: usize, fit: usize, n: usize) -> usize {
    let fit = fit.max(1);
    at.min(n.saturating_sub(1)) / fit * fit
}

/// The open category's guns as a strip of tiles, as many as fit around the
/// one in focus, `‹` / `›` when more lie either side, `n / m` after them.
fn gun_strip(c: &mut Kids, cx: &mut Ctx, s: &Shell, picks: &[Pick], (a, e): (usize, usize), have: i32, w: f32) {
    let pal = cx.pal;
    let (tw, th, gap) = (134.0, 96.0, 10.0);
    let fit = (((w - 70.0) + gap) / (tw + gap)).floor().max(1.0) as usize;
    let n = e - a;
    let at = s.focus.clamp(a, e - 1) - a;
    let start = strip_start(at, fit, n);
    let end = (start + fit).min(n);
    c.spawn((Node { padding: UiRect::axes(cx.px(8.0), cx.px(10.0)), column_gap: cx.px(gap), align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() }, BorderColor::all(pal.line), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25))))
        .with_children(|strip| {
            let mut left = strip.spawn((Text::new("‹"), cx.bold(30.0), TextColor(if start > 0 { WHITE } else { FAINT })));
            if start > 0 {
                left.insert((Hit::Strip(a + start - 1), Button));
            }
            for k in a + start..a + end {
                let p = &picks[k];
                let on = k == s.focus;
                tally(1);
                strip
                    .spawn((
                        Hit::Row(k),
                        Button,
                        Node { width: cx.px(tw), height: cx.px(th), flex_direction: FlexDirection::Column, border: UiRect::all(cx.px(if on { 2.0 } else { 1.0 })), overflow: Overflow::clip(), ..default() },
                        BorderColor::all(if on { pal.accent } else { WHITE.with_alpha(0.05) }),
                        BackgroundColor(if on { pal.accent_a(0.24) } else { Color::srgba(0.10, 0.11, 0.13, 0.85) }),
                        anim::from_left(cx.s, 30.0, 0.0),
                    ))
                    .with_children(|t| {
                        t.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| {
                            if let Some(g) = super::coming::of_value(p.value) {
                                super::coming::banner(pic, cx, g, tw - 4.0, th - 28.0);
                            } else if p.locked {
                                lock(pic, cx);
                            } else if !p.image.is_empty() {
                                picture_contain(pic, cx, std::slice::from_ref(&p.image), tw - 14.0, th - 32.0);
                            }
                        });
                        t.spawn((Node { height: cx.px(22.0), padding: UiRect::horizontal(cx.px(6.0)), align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))))
                            .with_child((Text::new(p.name.clone()), cx.regular(15.0), TextColor(if on { WHITE } else if p.locked { FAINT } else { TEXT })));
                        if p.value == have && p.value != 0 {
                            equipped_mark(t, cx);
                        }
                    });
            }
            let mut right = strip.spawn((Text::new("›"), cx.bold(30.0), TextColor(if end < n { WHITE } else { FAINT })));
            if end < n {
                right.insert((Hit::Strip(a + end), Button));
            }
            strip.spawn((Text::new(format!("{}/{n}", at + 1)), cx.bold(18.0), TextColor(DIM)));
        });
}

/// LETHAL, TACTICAL, a perk, a wildcard or a scorestreak: a card with the
/// Pick 10 points across its top, then the choices - tiles four a row
/// (icon and name; a perk's underlined in its column's colour, a wildcard's
/// with what it opens), or for scorestreaks a list (the place each has in
/// the class as a 1-3 badge, icon, name, what it does, the score it takes);
/// a scroll bar when more than fit. At the right the one in focus: picture,
/// name, what it does, its cost and the points the class would spend with
/// it, its level when locked.
fn item_view(c: &mut Kids, cx: &mut Ctx, s: &Shell, book: &ClassBook, class: &ClassView) {
    let pal = cx.pal;
    let (slot, label) = EDIT_SLOTS[s.slot.min(EDIT_SLOTS.len() - 1)];
    let all = picks_arc(s, book);
    let picks = &all.1;
    let have = class.get(slot);
    let streak = slot.starts_with("killstreak");
    let wild = slot.starts_with("bonuscard");
    let tier = slot.strip_prefix("specialty").and_then(|n| n.parse::<usize>().ok());
    let title = match slot {
        "primarygrenade" => "LETHAL",
        "specialgrenade" => "TACTICAL",
        _ => label,
    };
    let order = |v: i32| (1..=3).find(|k| v > 0 && class.get(&format!("killstreak{k}")) == v);
    let (w, h) = (780.0, 770.0);
    let cols = if streak { 1usize } else { 4 };
    super::GRID_COLS.store(cols, std::sync::atomic::Ordering::Relaxed);
    let (pad, gap, bar) = (16.0, 10.0, 22.0);
    let tw = (w - 2.0 * pad - bar - gap * (cols as f32 - 1.0)) / cols as f32;
    let th = if streak { 80.0 } else if wild { 196.0 } else { 150.0 };
    let grid_h = h - 58.0 - 54.0 - 2.0 * pad;
    let rows = ((grid_h + gap) / (th + gap)).floor().max(1.0) as usize;
    let total_rows = picks.len().div_ceil(cols).max(1);
    let first_row = scroll_rows(s.focus / cols, rows, total_rows);
    let (first, last) = (first_row * cols, ((first_row + rows) * cols).min(picks.len()));
    card(c, cx, title, CARD_L, CARD_T, w, h, |b, cx| {
        // Pick 10: the points spent and left.
        b.spawn((Node { height: cx.px(54.0), flex_shrink: 0.0, padding: UiRect::horizontal(cx.px(18.0)), column_gap: cx.px(6.0), align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() }, BorderColor::all(pal.line)))
            .with_children(|pts| {
                pts.spawn((Text::new("PICK 10"), cx.bold(20.0), TextColor(DIM), Node { margin: UiRect::right(cx.px(12.0)), ..default() }));
                for k in 0..POINTS {
                    pts.spawn((Node { flex_grow: 1.0, height: cx.px(8.0), ..default() }, BackgroundColor(if k < class.spent { pal.accent } else { WHITE.with_alpha(0.12) })));
                }
                let left = POINTS - class.spent;
                pts.spawn((
                    Text::new(format!("{}/{POINTS}  ·  {} LEFT", class.spent, left.max(0))),
                    cx.bold(20.0),
                    TextColor(if left < 0 { ENEMY } else { WHITE }),
                    Node { margin: UiRect::left(cx.px(12.0)), ..default() },
                ));
            });
        b.spawn(Node { height: cx.px(grid_h + 2.0 * pad), padding: UiRect::all(cx.px(pad)), column_gap: cx.px(bar - 6.0), flex_shrink: 0.0, ..default() }).with_children(|area| {
            area.spawn(Node { width: cx.px(w - 2.0 * pad - bar), flex_wrap: FlexWrap::Wrap, align_content: AlignContent::FlexStart, column_gap: cx.px(gap), row_gap: cx.px(gap), ..default() }).with_children(|grid| {
                for (k, p) in picks.iter().enumerate().take(last).skip(first) {
                    let on = k == s.focus;
                    tally(1);
                    let mut tile = grid.spawn((
                        Hit::Row(k),
                        Button,
                        Node {
                            width: cx.px(tw),
                            height: cx.px(th),
                            flex_direction: if streak { FlexDirection::Row } else { FlexDirection::Column },
                            align_items: AlignItems::Center,
                            column_gap: cx.px(14.0),
                            padding: UiRect::axes(cx.px(if streak { 14.0 } else { 8.0 }), cx.px(8.0)),
                            overflow: Overflow::clip(),
                            border: UiRect::all(cx.px(if on { 2.0 } else { 1.0 })).with_bottom(cx.px(if tier.is_some() { 4.0 } else if on { 2.0 } else { 1.0 })),
                            ..default()
                        },
                        BorderColor { bottom: tier.map_or(if on { pal.accent } else { WHITE.with_alpha(0.05) }, |t| tier_color(cx, t)), ..BorderColor::all(if on { pal.accent } else { WHITE.with_alpha(0.05) }) },
                        tile_fill(cx, on),
                        anim::rise(cx.s, 24.0, ((k - first) as f32 * 0.012).min(0.18)),
                    ));
                    tile.with_children(|t| {
                        if streak {
                            match order(p.value) {
                                Some(n) => badge(t, cx, n),
                                None => {
                                    t.spawn(Node { width: cx.px(24.0), flex_shrink: 0.0, ..default() });
                                }
                            }
                        }
                        let (pw, ph) = if streak { (60.0, 60.0) } else { (tw * 0.5, if wild { 70.0 } else { 78.0 }) };
                        t.spawn(Node { width: cx.px(pw), height: cx.px(ph), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| {
                            if p.locked {
                                lock(pic, cx);
                            } else if p.value == 0 {
                                none_ring(pic, cx, ph.min(pw) * 0.8);
                            } else {
                                picture_contain(pic, cx, std::slice::from_ref(&p.image), pw, ph);
                            }
                        });
                        t.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: cx.px(2.0), align_items: if streak { AlignItems::FlexStart } else { AlignItems::Center }, ..default() }).with_children(|words| {
                            words.spawn((Text::new(title_case(&p.name)), cx.bold(if streak { 24.0 } else { 19.0 }), TextColor(if p.locked && !on { FAINT } else { WHITE })));
                            if (streak || wild) && p.value != 0 && !p.desc.is_empty() {
                                words.spawn((Text::new(p.desc.clone()), cx.regular(15.0), TextColor(DIM), Node { max_width: cx.px(if streak { tw - 260.0 } else { tw - 16.0 }), ..default() }));
                            }
                        });
                        if streak && let Some(i) = book.item(p.value).filter(|i| i.momentum > 0) {
                            t.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, flex_shrink: 0.0, ..default() }).with_children(|sc| {
                                sc.spawn((Text::new(i.momentum.to_string()), cx.bold(28.0), TextColor(WHITE)));
                                sc.spawn((Text::new("SCORE"), cx.regular(14.0), TextColor(DIM)));
                            });
                        }
                        // (Its tag: LV for a locked one, its cost, the
                        // class's own marked.)
                        let tag = if p.locked { Some((format!("LV {}", p.level), GOLD)) } else if !streak && p.cost > 0 { Some((p.cost.to_string(), pal.accent)) } else { None };
                        if let Some((text, color)) = tag {
                            t.spawn((Text::new(text), cx.bold(15.0), TextColor(color), abs(Some(8.0), Some(5.0), None, None, cx)));
                        }
                        if p.value == have && p.value != 0 && !streak {
                            equipped_mark(t, cx);
                        }
                    });
                }
            });
            area.spawn(Node { width: cx.px(6.0), ..default() }).with_children(|sb| scrollbar(sb, cx, grid_h, first_row, rows, total_rows));
        });
    });
    card_hint(c, cx, CARD_L, CARD_T + h + 12.0, &[("W / A / S / D", "D-PAD", "MOVE"), ("ENTER", "A", if streak { "SET STREAK" } else { "EQUIP" }), ("ESC", "B", "BACK")]);
    // The one in focus.
    let Some(p) = picks.get(s.focus) else { return };
    let (rl, rw) = (CARD_L + w + 30.0, R - CARD_L - w - 30.0);
    let (ground, rim) = super::ghosts::panel_ground(cx);
    // (The points the class would spend with it in the slot: Pick 10's
    // own count, `classes::spent`.)
    let mut slots = class.slots.clone();
    slots.insert(slot.to_owned(), p.value);
    if slot.ends_with("grenade") {
        slots.insert(format!("{slot}count"), if p.value > 0 { 1 } else { 0 });
    }
    let after = classes::spent(book, &slots);
    c.spawn((
        Node { width: cx.px(rw), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), padding: UiRect::all(cx.px(22.0)), border: UiRect::top(cx.px(2.0)), ..abs(Some(rl), Some(CARD_T), None, None, cx) },
        ground,
        rim,
        anim::from_left(cx.s, -60.0, 0.06),
    ))
    .with_children(|col| {
        col.spawn(Node { width: Val::Percent(100.0), height: cx.px(150.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|pic| {
            if p.locked {
                lock(pic, cx);
            } else if p.value == 0 {
                none_ring(pic, cx, 90.0);
            } else {
                picture_contain(pic, cx, std::slice::from_ref(&p.image), rw - 60.0, 140.0);
            }
        });
        col.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|n| {
            if let Some(k) = order(p.value) {
                badge(n, cx, k);
            }
            n.spawn((Text::new(title_case(&p.name)), cx.bold(34.0), TextColor(WHITE)));
        });
        if let Some(t) = tier {
            col.spawn((Node { width: cx.px(80.0), height: cx.px(4.0), ..default() }, BackgroundColor(tier_color(cx, t))));
        }
        if !p.desc.is_empty() {
            col.spawn((Text::new(p.desc.clone()), cx.regular(23.0), TextColor(DIM)));
        }
        let mut facts: Vec<(String, String, Color)> = Vec::new();
        if streak && let Some(i) = book.item(p.value).filter(|i| i.momentum > 0) {
            facts.push(("SCORE".to_owned(), i.momentum.to_string(), WHITE));
        }
        if p.cost > 0 {
            facts.push(("COST".to_owned(), format!("{} POINT{}", p.cost, if p.cost == 1 { "" } else { "S" }), pal.accent));
        }
        facts.push(("CLASS WITH IT".to_owned(), format!("{after}/{POINTS}"), if after > POINTS { ENEMY } else { WHITE }));
        if p.locked {
            facts.push(("UNLOCKS AT".to_owned(), format!("LEVEL {}", p.level), GOLD));
        }
        if p.value == have && p.value != 0 {
            facts.push(("IN THIS CLASS".to_owned(), "EQUIPPED".to_owned(), pal.accent));
        }
        for (k, v, color) in facts {
            col.spawn((Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::vertical(cx.px(6.0)), border: UiRect::top(cx.px(1.0)), ..default() }, BorderColor::all(pal.line))).with_children(|r| {
                r.spawn((Text::new(k), cx.regular(20.0), TextColor(DIM)));
                r.spawn((Text::new(v), cx.bold(20.0), TextColor(color)));
            });
        }
        // Scorestreaks: the class's three, in order (badge, picture, score;
        // the slot being set lit).
        if streak {
            col.spawn((Text::new("THIS CLASS"), cx.regular(20.0), TextColor(DIM), Node { margin: UiRect::top(cx.px(8.0)), ..default() }));
            col.spawn(Node { column_gap: cx.px(8.0), ..default() }).with_children(|row| {
                let sw = (rw - 44.0 - 16.0) / 3.0;
                for k in 1..=3 {
                    let key = format!("killstreak{k}");
                    let it = book.item(class.get(&key));
                    let lit = key == slot;
                    row.spawn((
                        Node { width: cx.px(sw), height: cx.px(110.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::all(cx.px(6.0)), row_gap: cx.px(2.0), border: UiRect::all(cx.px(if lit { 2.0 } else { 1.0 })), ..default() },
                        BorderColor::all(if lit { pal.accent } else { WHITE.with_alpha(0.06) }),
                        tile_fill(cx, lit),
                    ))
                    .with_children(|b| {
                        b.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|top| {
                            badge(top, cx, k);
                            if let Some(i) = it.filter(|i| i.momentum > 0) {
                                top.spawn((Text::new(i.momentum.to_string()), cx.bold(18.0), TextColor(WHITE)));
                            }
                        });
                        match it {
                            Some(i) => picture_contain(b, cx, std::slice::from_ref(&i.image), sw - 20.0, 62.0),
                            None => {
                                b.spawn((Text::new("+"), cx.regular(30.0), TextColor(WHITE.with_alpha(0.35))));
                            }
                        }
                    });
                }
            });
        }
    });
}

/// A padlock, drawn: its shackle a ring, its body a rounded block.
fn lock(c: &mut Kids, cx: &Ctx) {
    c.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|l| {
        l.spawn((
            Node { width: cx.px(30.0), height: cx.px(28.0), border: UiRect::new(cx.px(6.0), cx.px(6.0), cx.px(6.0), cx.px(0.0)), border_radius: BorderRadius::top(cx.px(15.0)), ..default() },
            BorderColor::all(WHITE.with_alpha(0.45)),
        ));
        l.spawn((Node { width: cx.px(46.0), height: cx.px(36.0), border_radius: BorderRadius::all(cx.px(6.0)), ..default() }, BackgroundColor(WHITE.with_alpha(0.45))));
    });
}

/// `SWAT-556` -> `Swat-556`, `FMJ` stays (Black Ops II's names in its
/// pickers; a word of four letters or fewer is left in capitals).
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            if w.len() <= 4 || w.chars().any(|c| c.is_ascii_digit()) {
                return w.to_owned();
            }
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod strip_tests {
    use super::strip_start;

    /// Every gun is shown on its own page, and a gun shown stays put
    /// under the mouse when it is focused (hover or click).
    #[test]
    fn every_gun_can_be_pointed_at_and_focusing_a_shown_gun_never_scrolls() {
        for fit in 1..=5usize {
            for n in 1..=14usize {
                let shown = |at: usize| {
                    let s = strip_start(at, fit, n);
                    (s, (s + fit).min(n))
                };
                for at in 0..n {
                    let (s, e) = shown(at);
                    assert!((s..e).contains(&at), "fit {fit} n {n}: {at} not shown in {s}..{e}");
                    // Mouse on any tile of that strip: same strip.
                    for k in s..e {
                        assert_eq!(shown(k), (s, e), "fit {fit} n {n}: hover {k} moved the strip");
                    }
                }
            }
        }
    }

    /// A / D through a category: the focus walks every gun in order, the
    /// strip moves only when the focus passes its last (or first) tile,
    /// and the 3rd tile of three is reached before the strip turns.
    #[test]
    fn keyboard_walks_every_gun_and_scrolls_only_past_the_last_tile() {
        for fit in 1..=5usize {
            for n in 1..=14usize {
                let mut seen = Vec::new();
                let mut prev = strip_start(0, fit, n);
                for at in 0..n {
                    let s = strip_start(at, fit, n);
                    if s != prev {
                        assert_eq!(at, prev + fit.min(n), "fit {fit} n {n}: scrolled early at {at}");
                    }
                    assert!(at < s + fit, "fit {fit} n {n}: {at} off the strip");
                    seen.push(at);
                    prev = s;
                }
                assert_eq!(seen.len(), n);
                // The last gun is shown, and backwards too.
                let last = strip_start(n - 1, fit, n);
                assert!((last..(last + fit).min(n)).contains(&(n - 1)));
                // A / D back onto a shown tile leaves the strip.
                for at in (0..n).rev() {
                    let s = strip_start(at, fit, n);
                    assert!((s..(s + fit).min(n)).contains(&at));
                }
            }
        }
    }

    /// The `‹` / `›` targets (`Hit::Strip`) land on a gun of the next /
    /// previous strip, so a mouse alone reaches every gun.
    #[test]
    fn the_arrows_reach_every_page() {
        let (fit, n) = (3usize, 8usize);
        let mut at = 0;
        let mut pages = 1;
        loop {
            let s = strip_start(at, fit, n);
            let end = (s + fit).min(n);
            if end >= n {
                break;
            }
            at = end; // the › target
            assert!(strip_start(at, fit, n) > s);
            pages += 1;
        }
        assert_eq!(pages, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::{Pick, categories};

    fn pick(group: &str) -> Pick {
        Pick { value: 1, name: String::new(), desc: String::new(), image: String::new(), cost: 1, locked: false, level: 1, group: group.to_owned() }
    }

    #[test]
    fn categories_are_runs_of_one_kind() {
        let picks = [pick(""), pick("SMG"), pick("SMG"), pick("AR"), pick("SNIPER"), pick("SNIPER")];
        assert_eq!(categories(&picks), vec![(0, 1), (1, 3), (3, 4), (4, 6)]);
        assert!(categories(&[]).is_empty());
    }

    #[test]
    fn tab_span_keeps_the_tab_in_use_and_fits_the_room() {
        let labels: Vec<String> = ["A", "B", "C", "D", "E"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(super::tab_span(&labels, 2, 30.0, |_| 10.0), (1, 3));
        assert_eq!(super::tab_span(&labels, 0, 30.0, |_| 10.0), (0, 2));
        assert_eq!(super::tab_span(&labels, 4, 100.0, |_| 10.0), (0, 4));
    }

    #[test]
    fn memo_builds_once_per_key() {
        let mut m = super::Memo::new();
        let a = m.get((1, 2), || vec![1]);
        let b = m.get((1, 2), || vec![2]);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert_eq!(m.builds, 1);
        let c = m.get((1, 3), || vec![3]);
        assert_eq!(*c, vec![3]);
        assert_eq!(m.builds, 2);
        // (Back to the first key: built again, the one kept is the last.)
        assert_eq!(*m.get((1, 2), || vec![4]), vec![4]);
        assert_eq!(m.builds, 3);
    }

    fn book() -> crate::shell::classes::ClassBook {
        use crate::shell::classes::{ClassBook, ClassItem, ClassView};
        let gun = |index: i32, group: &str| ClassItem {
            index,
            reference: format!("gun{index}"),
            name: format!("Gun {index}"),
            image: String::new(),
            desc: String::new(),
            cost: 1,
            momentum: 0,
            group: group.to_owned(),
            slot: "primary".to_owned(),
            level: 1,
            locked: false,
            attachments: Vec::new(),
        };
        ClassBook {
            classes: vec![ClassView { n: 0, name: "Custom 1".to_owned(), slots: std::collections::HashMap::from([("primary".to_owned(), 2)]), spent: 1 }],
            items: vec![gun(2, "weapon_assault"), gun(1, "weapon_smg"), gun(3, "weapon_smg")],
            ..Default::default()
        }
    }

    #[test]
    fn the_picker_is_built_again_only_when_its_list_changes() {
        use crate::shell::{Screen, Shell};
        let book = book();
        let mut s = Shell { screen: Screen::ItemPick, class: 0, slot: 0, ..Shell::default() };
        let key = super::picks_key(&s, &book);
        // (A cursor move is no new list.)
        s.focus = 2;
        assert_eq!(super::picks_key(&s, &book), key);
        s.page = 1;
        assert_ne!(super::picks_key(&s, &book), key);
        s.page = 0;
        // The cached list is the one built: Black Ops II's guns by kind.
        let picks = super::picks_arc(&s, &book);
        assert_eq!(picks.1.iter().map(|p| p.value).collect::<Vec<_>>(), vec![1, 3, 2]);
        assert_eq!(*picks, super::picks_built(&s, &book));
        assert_eq!(super::rows(&Shell { screen: Screen::ItemPick, ..s.clone() }, &book).len(), 3);
    }

    /// The test book as a new revision (as `classes::put_book` stamps it).
    fn stamped(mut b: crate::shell::classes::ClassBook, rev: u64) -> crate::shell::classes::ClassBook {
        b.rev = rev;
        b
    }

    fn edit(book: &crate::shell::classes::ClassBook, class: i32, slot: &str, value: i32) -> crate::shell::classes::ClassBook {
        book.with_edit(&crate::shell::classes::Edit { class, set: vec![(slot.to_owned(), value)] })
    }

    #[test]
    fn the_class_screens_show_each_edit_at_once() {
        use crate::shell::{Act, Screen, Shell};
        let mut book = stamped(crate::shell::classes::test_book(), 1_000);
        let s = Shell { screen: Screen::ClassEdit, class: 1, ..Shell::default() };
        let list = Shell { screen: Screen::Classes, ..Shell::default() };
        let edits = [
            ("primary", 1),
            ("primaryattachment1", 2),
            ("primarycamo", 5),
            ("secondaryattachment1", 1),
            ("primarygrenade", 10),
            ("specialgrenade", 11),
            ("specialty1", 21),
            ("specialty2", 22),
            ("bonuscard1", 30),
            ("killstreak1", 40),
            ("killstreak3", 41),
        ];
        for (k, (slot, value)) in edits.into_iter().enumerate() {
            let before = super::rows(&s, &book);
            book = stamped(edit(&book, 1, slot, value), 1_001 + k as u64);
            let c = book.class(1).unwrap();
            let at = super::EDIT_SLOTS.iter().position(|(x, _)| *x == slot).unwrap();
            let rows = super::rows(&s, &book);
            let row = rows.iter().find(|r| r.act == Act::EditSlot(at)).unwrap_or_else(|| panic!("{slot} listed"));
            assert_eq!(row.value.as_deref(), Some(super::slot_value(&book, c, slot).as_str()), "{slot}");
            assert_ne!(rows, before, "{slot}: the screen changed");
            assert_eq!(rows, super::rows_built(&s, &book), "{slot}");
            // The class list's points and the overview's class: the edited one.
            let listed = super::rows(&list, &book);
            assert_eq!(listed[1].sub, format!("{} / {} points", c.spent, super::POINTS), "{slot}");
            assert_eq!(super::focused_class(&book, &listed, 1), Some(c), "{slot}");
        }
        // A rename: the list's row and the overview's class.
        book = stamped(book.with_name(1, "RUSH"), 2_000);
        let listed = super::rows(&list, &book);
        assert_eq!(listed[1].label, "RUSH");
        assert_eq!(super::focused_class(&book, &listed, 1).unwrap().name, "RUSH");
    }

    #[test]
    fn another_class_is_its_own_rows_and_picks() {
        use crate::shell::{Act, Screen, Shell};
        let book = stamped(crate::shell::classes::test_book(), 3_000);
        let primary = super::EDIT_SLOTS.iter().position(|(x, _)| *x == "primary").unwrap();
        let value = |class: i32| {
            let s = Shell { screen: Screen::ClassEdit, class, ..Shell::default() };
            super::rows(&s, &book).into_iter().find(|r| r.act == Act::EditSlot(primary)).and_then(|r| r.value)
        };
        assert_eq!(value(0).as_deref(), Some("ITEM 1"));
        assert_eq!(value(1).as_deref(), Some("ITEM 2"));
        assert_eq!(value(0).as_deref(), Some("ITEM 1"));
        // (The overview: the class of the row in focus, by its number, also
        // with the list in another order.)
        let mut turned = book.clone();
        turned.classes.reverse();
        let turned = stamped(turned, 3_001);
        let listed = super::rows(&Shell { screen: Screen::Classes, ..Shell::default() }, &turned);
        for (k, r) in listed.iter().enumerate() {
            let Act::EditClass(n) = r.act else { panic!("a class row") };
            assert_eq!(super::focused_class(&turned, &listed, k).unwrap().n, n);
        }
        // The camo picker follows the class's gun: none, then one.
        let camo = super::EDIT_SLOTS.iter().position(|(x, _)| *x == "secondarycamo").unwrap();
        let s = Shell { screen: Screen::ItemPick, class: 1, slot: camo, ..Shell::default() };
        let bare = stamped(edit(&book, 1, "secondary", 0), 3_002);
        assert!(super::picks_arc(&s, &bare).1.is_empty());
        let armed = stamped(edit(&bare, 1, "secondary", 3), 3_003);
        assert!(super::picks_arc(&s, &armed).1.iter().any(|p| p.value == 5));
    }

    #[test]
    fn attachments_sort_into_kinds() {
        assert_eq!(super::attachment_group("silencer", ""), "BARREL");
        assert_eq!(super::attachment_group("grip", ""), "UNDER BARREL");
        assert_eq!(super::attachment_group("", "Extended Mags"), "MAGAZINE");
        assert_eq!(super::attachment_group("", "Red Dot Sight"), "OPTICS");
    }
}

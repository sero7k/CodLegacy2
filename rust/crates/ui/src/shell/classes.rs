//! bo2mp shell: his classes for the shell's own Create-a-Class and Choose
//! Class screens.
//!
//! Read from his stats (the custom match class set, the one his matches
//! give: `bo2_profile::LOCAL_MATCH_SET`) through BO2's scripts' host each
//! frame they are wanted, and changed there: the front end saves them as
//! BO2's own Create-a-Class did.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bo2_profile::{LOCAL_MATCH_SET, Profile, SLOTS, StatValue};
use hks_t6::host::Host;
use hks_t6::mp::Mp;

/// The points a class may spend (Pick 10).
pub(crate) const POINTS: i32 = 10;

/// An item he can put in a class.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassItem {
    pub index: i32,
    pub reference: String,
    pub name: String,
    pub image: String,
    pub desc: String,
    pub cost: i32,
    /// A scorestreak's score to earn it (statstable's momentum column; 0
    /// for anything else).
    pub momentum: i32,
    /// statstable's group (`weapon_smg`, `specialty`, ...).
    pub group: String,
    /// The class slot it goes in (`primary`, `specialty1`, `bonuscard`, ...).
    pub slot: String,
    /// The level it unlocks at (1 = from the start).
    pub level: i32,
    pub locked: bool,
    /// A weapon's attachments: (number, name, picture), 1 first.
    pub attachments: Vec<(i32, String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassView {
    pub n: i32,
    pub name: String,
    /// Slot -> item index (or attachment number, or count).
    pub slots: HashMap<String, i32>,
    pub spent: i32,
}

impl ClassView {
    pub fn get(&self, slot: &str) -> i32 {
        self.slots.get(slot).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClassBook {
    pub classes: Vec<ClassView>,
    pub items: Vec<ClassItem>,
    /// bo2mp: Black Ops II's camos (mp/attachmenttable.csv's "camo" weapon
    /// options), in its order.
    pub camos: Vec<Camo>,
    /// bo2mp: a weapon's stat bars (mp/statstable.csv's Accuracy, Damage,
    /// Range, Fire Rate and Mobility columns, `asset_game::CacStatBar`), by
    /// item index; a gun without them (most guest guns) is not here.
    pub stats: HashMap<i32, [i32; 5]>,
    /// bo2mp: a weapon's attachments' details, by (item index, attachment
    /// number): (its mp/attachmenttable.csv reference, its description).
    pub att_info: HashMap<(i32, i32), (String, String)>,
    /// bo2mp: which book this is (`generation` when it was put up; 0 for a
    /// book never put up). Every view and cache of a class keys on it, not
    /// on where the book sits in memory: a freed book's address is reused
    /// by a later one, so an address alone could say "nothing changed".
    pub rev: u64,
}

/// bo2mp: a camo he can put on a Black Ops II gun.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Camo {
    pub index: i32,
    pub name: String,
    pub image: String,
    /// One of the base game's camos (drawn in every match); the DLC camos'
    /// pictures come with their packs.
    pub drawn: bool,
    /// A material camo (Gold, Diamond, Carbon Fiber, the DLC camos), which
    /// other games' guns wear too; else a pattern laid on Black Ops II's
    /// own guns' camo-ready parts.
    pub material: bool,
}

impl ClassBook {
    pub fn camo(&self, index: i32) -> Option<&Camo> {
        (index > 0).then(|| self.camos.iter().find(|c| c.index == index)).flatten()
    }

    /// His class numbered `n` (`ClassView::n`), whatever its place in
    /// the list.
    pub fn class(&self, n: i32) -> Option<&ClassView> {
        self.classes.iter().find(|c| c.n == n)
    }

    /// The same classes, items and camos (whichever book each is).
    pub fn same_as(&self, other: &ClassBook) -> bool {
        self.classes == other.classes && self.items == other.items && self.camos == other.camos && self.stats == other.stats && self.att_info == other.att_info
    }

    /// This book with `e` made (`spent` counted again): what the profile
    /// will hold once `class_side` has written it. Scorestreaks are his,
    /// not a class's (`shared_slot`): one picked in any class is in every
    /// class.
    pub fn with_edit(&self, e: &Edit) -> ClassBook {
        let mut b = self.clone();
        if self.class(e.class).is_none() {
            return b;
        }
        for c in &mut b.classes {
            let mut touched = false;
            for (k, v) in &e.set {
                if c.n == e.class || shared_slot(k) {
                    c.slots.insert(k.clone(), *v);
                    touched = true;
                }
            }
            if touched {
                c.spent = spent(self, &c.slots);
            }
        }
        b
    }

    /// This book with class `n` named `name` (an empty name is its
    /// default, `Custom n`).
    pub fn with_name(&self, n: i32, name: &str) -> ClassBook {
        let mut b = self.clone();
        if let Some(c) = b.classes.iter_mut().find(|c| c.n == n) {
            c.name = if name.trim().is_empty() { format!("Custom {}", n + 1) } else { name.to_owned() };
        }
        b
    }

    pub fn item(&self, index: i32) -> Option<&ClassItem> {
        (index > 0).then(|| self.items.iter().find(|i| i.index == index)).flatten()
    }

    /// The items that go in a class slot, best first (by group, then the
    /// table's order).
    pub fn for_slot(&self, slot: &str) -> Vec<&ClassItem> {
        let want = slot_kind(slot);
        let mut v: Vec<&ClassItem> = self.items.iter().filter(|i| slot_kind(&i.slot) == want).collect();
        v.sort_by_key(|i| (group_order(&i.group), i.level, i.index));
        v
    }
}

/// The kind of slot an item goes in: `specialty1`, `primary`, `bonuscard`
/// (wildcards, any of the three), `killstreak` (any of the three).
pub(crate) fn slot_kind(slot: &str) -> String {
    let s = slot.to_ascii_lowercase();
    if s.starts_with("bonuscard") {
        "bonuscard".to_owned()
    } else if s.starts_with("killstreak") {
        "killstreak".to_owned()
    } else if let Some(n) = s.strip_prefix("specialty") {
        // (Perk Greed's extra perks are 4-6: the same lists as 1-3.)
        match n.parse::<i32>() {
            Ok(k) if k > 3 => format!("specialty{}", k - 3),
            _ => s,
        }
    } else {
        s
    }
}

/// A weapon group's name, and its order.
pub(crate) fn group_label(group: &str) -> &'static str {
    match group {
        "weapon_smg" => "SUBMACHINE GUNS",
        "weapon_assault" => "ASSAULT RIFLES",
        "weapon_cqb" => "SHOTGUNS",
        "weapon_lmg" => "LIGHT MACHINE GUNS",
        "weapon_sniper" => "SNIPER RIFLES",
        "weapon_pistol" => "PISTOLS",
        "weapon_launcher" => "LAUNCHERS",
        "weapon_special" => "SPECIALS",
        "weapon_knife" => "MELEE",
        _ => "",
    }
}

fn group_order(group: &str) -> i32 {
    ["weapon_smg", "weapon_assault", "weapon_cqb", "weapon_lmg", "weapon_sniper", "weapon_pistol", "weapon_launcher", "weapon_special", "weapon_knife"]
        .iter()
        .position(|g| *g == group)
        .map_or(99, |p| p as i32)
}

/// bo2mp: a slot that is his, not a class's: the scorestreaks. Black Ops
/// II keeps them in class 0 for every class (the match reads them there:
/// `sim::t6::loadout`, `Profile::has_streak_picks`); Create-a-Class wrote
/// them in the class being edited, so a streak picked in classes 2-5 was
/// shown there but never played, and the other classes showed their own.
pub(crate) fn shared_slot(slot: &str) -> bool {
    slot.starts_with("killstreak")
}

/// The stats path of a class slot in the set his matches give (a shared
/// slot in class 0's).
pub(crate) fn slot_key(class: i32, slot: &str) -> String {
    Profile::class_key(LOCAL_MATCH_SET, if shared_slot(slot) { 0 } else { class }, slot)
}

/// A change he made: slots of a class and their new values.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    pub class: i32,
    pub set: Vec<(String, i32)>,
}

#[derive(Default)]
struct Shared {
    book: Option<Arc<ClassBook>>,
    edits: Vec<Edit>,
    renames: Vec<(i32, String)>,
    built_at: f64,
    /// What the book's item lists were read from (`ItemsKey`): the same, and
    /// only the classes are read again.
    items_key: Option<ItemsKey>,
}

/// What a book's items, camos and stat bars depend on: how many items the
/// profile has, Unlock All, his rank, and how many strings and tables the
/// engine has loaded (names are localized from them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ItemsKey {
    items: usize,
    free: bool,
    rank: i32,
    strings: usize,
    tables: usize,
}

/// bo2mp: the last book revision handed out (`ClassBook::rev`: each book
/// put up takes the next).
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn put_book(s: &mut Shared, mut book: ClassBook) {
    book.rev = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    s.book = Some(Arc::new(book));
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static WANTED: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    let mut g = SHARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(Shared::default))
}

/// A screen that shows his classes is up (they are read each half second
/// while it is).
pub(crate) fn want(on: bool) {
    WANTED.store(on, Ordering::Relaxed);
}

pub(crate) fn book() -> Option<Arc<ClassBook>> {
    with(|s| s.book.clone())
}

/// A change he made. bo2mp: the book shown is changed at once (every
/// screen that shows a class draws it on its next frame); the profile is
/// written, and the book read back from it, on `class_side`'s next turn.
/// (The book used to change only then: the screen drawn right after an
/// edit was the old class, and a view keyed on the book's address could
/// keep it - the stale previews.)
pub(crate) fn edit(e: Edit) {
    with(|s| {
        if let Some(b) = s.book.clone() {
            put_book(s, b.with_edit(&e));
        }
        s.edits.push(e);
    });
}

/// His class `n` given a new name (shown at once, as `edit`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rename(n: i32, name: String) {
    with(|s| {
        if let Some(b) = s.book.clone() {
            put_book(s, b.with_name(n, &name));
        }
        s.renames.push((n, name));
    });
}

/// The points a class spends (as BO2's Create-a-Class counts them: each
/// item its cost, a grenade each one carried, each attachment one).
pub(crate) fn spent(book: &ClassBook, slots: &HashMap<String, i32>) -> i32 {
    let mut total = 0;
    for slot in SLOTS {
        let n = slots.get(slot).copied().unwrap_or(0);
        if n <= 0 || slot.ends_with("count") || slot.ends_with("camo") || slot.ends_with("reticle") {
            continue;
        }
        if slot.contains("attachment") {
            total += 1;
        } else {
            let each = if slot.ends_with("grenade") { slots.get(&format!("{slot}count")).copied().unwrap_or(1).max(1) } else { 1 };
            total += book.item(n).map_or(0, |i| i.cost) * each;
        }
    }
    total
}

fn localized(v: &hks_t6::host::EngineValues, key: &str, fallback: &str) -> String {
    if !key.is_empty()
        && let Some(t) = v.localize.get(&key.to_ascii_uppercase()).filter(|t| !t.is_empty())
    {
        return t.clone();
    }
    fallback.replace('_', " ").to_uppercase()
}

/// Black Ops II's camos: mp/attachmenttable.csv's weapon options of the
/// "camo" group (column 0 its number, 3 its name, 6 its swatch), and
/// whether mp/weaponoptions.csv makes it a material camo (column 3 = 1).
fn camos(v: &hks_t6::host::EngineValues) -> Vec<Camo> {
    let material: std::collections::HashSet<i32> = hks_t6::mp::table_rows(v, "mp/weaponoptions.csv")
        .iter()
        .filter(|r| r.get(1).is_some_and(|c| c.trim() == "camo") && r.get(3).is_some_and(|c| c.trim() == "1"))
        .filter_map(|r| r.first()?.trim().parse().ok())
        .collect();
    hks_t6::mp::table_rows(v, "mp/attachmenttable.csv")
        .iter()
        .filter(|r| r.get(1).is_some_and(|c| c.trim() == "camo") && r.get(2).is_some_and(|c| c.trim() == "weaponoption"))
        .filter_map(|r| {
            let index: i32 = r.first()?.trim().parse().ok()?;
            let reference = r.get(4).map_or("", |c| c.trim());
            (index > 0).then(|| Camo {
                index,
                name: localized(v, r.get(3).map_or("", |c| c.trim()), reference.trim_start_matches("camo_")),
                image: r.get(6).map_or_else(String::new, |c| c.trim().to_owned()),
                drawn: index <= 18 || material.contains(&index),
                material: material.contains(&index),
            })
        })
        .collect()
}

/// bo2mp: the weapons' stat bars in mp/statstable.csv (column 0 the item's
/// index; `asset_game::CacStatBar`'s columns), for the pickers' WEAPON
/// STATS. Rows with no numbers there are left out.
fn weapon_stats(v: &hks_t6::host::EngineValues) -> HashMap<i32, [i32; 5]> {
    let mut out = HashMap::new();
    for r in hks_t6::mp::table_rows(v, "mp/statstable.csv") {
        let Some(index) = r.first().and_then(|c| c.trim().parse::<i32>().ok()) else { continue };
        let mut bars = [0; 5];
        let mut any = false;
        for (k, bar) in asset_game::CacStatBar::ALL.into_iter().enumerate() {
            if let Some(n) = r.get(bar.column() as usize).and_then(|c| c.trim().parse::<i32>().ok()) {
                bars[k] = n;
                any = true;
            }
        }
        if any {
            out.insert(index, bars);
        }
    }
    out
}

fn items_key(v: &hks_t6::host::EngineValues, p: &Profile) -> ItemsKey {
    ItemsKey { items: p.items.len(), free: p.stat_num(super::bridge::UNLOCK_ALL_STAT) != 0.0, rank: p.rank(), strings: v.localize.len(), tables: v.tables.len() }
}

/// His five classes as the profile has them now (`spent` counted with
/// `book`'s items).
fn read_classes(p: &Profile, book: &ClassBook) -> Vec<ClassView> {
    let mut classes = Vec::new();
    for n in 0..5 {
        let mut slots = HashMap::new();
        for slot in SLOTS.iter().copied().chain(["killstreak1", "killstreak2", "killstreak3"]) {
            let key = slot_key(n, slot);
            if let Some(x) = p.stat(&key).and_then(StatValue::as_num) {
                slots.insert(slot.to_owned(), x as i32);
            }
        }
        let name = match p.stat(&format!("{LOCAL_MATCH_SET}.customclassname.{n}")) {
            Some(StatValue::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => format!("Custom {}", n + 1),
        };
        let spent = spent(book, &slots);
        classes.push(ClassView { n, name, slots, spent });
    }
    classes
}

fn build(host: &Host, mp: &Mp) -> ClassBook {
    let v = host.values.borrow();
    let m = mp.borrow();
    let p: &Profile = &m.profile;
    // (Locked by level unless he turned Unlock All on.)
    let free = p.stat_num(super::bridge::UNLOCK_ALL_STAT) != 0.0;
    let rank = p.rank();
    let mut items = Vec::new();
    let mut att_info = HashMap::new();
    for it in &p.items {
        if it.reference.is_empty() || it.slot.is_empty() {
            continue;
        }
        let kind = slot_kind(&it.slot);
        let wanted = matches!(kind.as_str(), "primary" | "secondary" | "primarygrenade" | "specialgrenade" | "bonuscard" | "killstreak")
            || kind.starts_with("specialty");
        if !wanted {
            continue;
        }
        let attachments = it
            .attachments
            .iter()
            .enumerate()
            .filter_map(|(k, _)| {
                let a = p.attachment(it.index, k as i32 + 1)?;
                att_info.insert((it.index, k as i32 + 1), (a.reference.clone(), localized(&v, &a.desc, "")));
                Some((k as i32 + 1, localized(&v, &a.name, &a.reference), a.image.clone()))
            })
            .collect();
        items.push(ClassItem {
            index: it.index,
            reference: it.reference.clone(),
            name: localized(&v, &it.name, &it.reference),
            image: it.image.clone(),
            desc: localized(&v, &it.desc, ""),
            cost: it.cost,
            momentum: it.momentum,
            group: it.group.clone(),
            slot: it.slot.clone(),
            level: it.unlock_rank + 1,
            locked: !free && it.unlock_rank > rank,
            attachments,
        });
    }
    let book_camos = camos(&v);
    let stats = weapon_stats(&v);
    let mut book = ClassBook { classes: Vec::new(), items, camos: book_camos, stats, att_info, rev: 0 };
    book.classes = read_classes(p, &book);
    book
}

/// Once a frame with BO2's scripts' host (front end or match): his edits
/// in, his classes out while a screen wants them.
pub(crate) fn class_side(host: &Host, mp: Option<&Mp>) {
    let Some(mp) = mp else {
        // bo2mp: no profile here (a Zombies match's HUD has none): his
        // edits went nowhere but the book shown, so the card showed the
        // class he built while his file and the match kept the old one.
        // Written to his file at once; the match reads it at his class
        // choice, the next front end or match HUD at its start.
        let has = with(|s| !s.edits.is_empty() || !s.renames.is_empty());
        if has {
            persist_edits(&crate::lui_frontend::stats_path(None));
        }
        return;
    };
    let edits = with(|s| std::mem::take(&mut s.edits));
    let renames = with(|s| std::mem::take(&mut s.renames));
    let changed = !edits.is_empty() || !renames.is_empty();
    if changed || WANTED.load(Ordering::Relaxed) {
        mp.borrow_mut().load(&host.values.borrow());
    }
    for e in edits {
        let mut m = mp.borrow_mut();
        for (slot, value) in e.set {
            let key = slot_key(e.class, &slot);
            m.profile.set_stat(key, StatValue::Num(value as f32));
        }
        diag::info!(Ui, "bo2mp shell: class {} changed", e.class + 1);
    }
    for (n, name) in renames {
        mp.borrow_mut().profile.set_stat(format!("{LOCAL_MATCH_SET}.customclassname.{n}"), StatValue::Str(name));
    }
    let now = host.now_ms();
    let due = with(|s| changed || s.book.is_none() || now - s.built_at > 500.0 || now < s.built_at);
    if !(WANTED.load(Ordering::Relaxed) || changed) || !due {
        return;
    }
    // bo2mp: the items (every game's guns, their attachments, localized)
    // are read again only when what they come from changed; each half
    // second otherwise only his five classes are (reading and comparing the
    // whole book twice a second hitched the Create-a-Class screens).
    let key = items_key(&host.values.borrow(), &mp.borrow().profile);
    let old = with(|s| s.book.clone().filter(|_| s.items_key == Some(key)));
    if let Some(old) = old {
        let classes = read_classes(&mp.borrow().profile, &old);
        let mut book = (*old).clone();
        book.classes = classes;
        with(|s| {
            put_read(s, book);
            s.built_at = now;
        });
        return;
    }
    let started = std::time::Instant::now();
    let book = build(host, mp);
    diag::info!(Ui, "bo2mp shell: class book read ({} items, {:.1} ms)", book.items.len(), started.elapsed().as_secs_f64() * 1000.0);
    with(|s| {
        put_read(s, book);
        s.items_key = Some(key);
        s.built_at = now;
    });
}

/// bo2mp: his queued edits and renames written straight to his stats file
/// (merged over what it holds), for a host with no profile to take them.
/// The book shown keeps them (`edit` laid them on it).
fn persist_edits(path: &std::path::Path) {
    let (edits, renames) = with(|s| (std::mem::take(&mut s.edits), std::mem::take(&mut s.renames)));
    let mut out: Vec<(String, StatValue)> = Vec::new();
    for e in &edits {
        for (slot, value) in &e.set {
            out.push((slot_key(e.class, slot), StatValue::Num(*value as f32)));
        }
        diag::info!(Ui, "bo2mp shell: class {} changed (no profile here: written to {})", e.class + 1, path.display());
    }
    for (n, name) in renames {
        out.push((format!("{LOCAL_MATCH_SET}.customclassname.{n}"), StatValue::Str(name)));
    }
    if !out.is_empty() {
        bo2_profile::save_in_background(path, bo2_profile::SaveJob::Merge(out), None);
    }
}

/// bo2mp: a book just read from the profile put up, if it is not the one
/// shown. What the profile holds wins over the book shown (an edit shown at
/// once that the profile did not take is undone), but edits made since this
/// turn took the queue (`edit`, `rename`: still queued) are laid on it, so
/// a read made just before them does not undo them for a frame.
fn put_read(s: &mut Shared, mut book: ClassBook) {
    for e in &s.edits {
        book = book.with_edit(e);
    }
    for (n, name) in &s.renames {
        book = book.with_name(*n, name);
    }
    if !s.book.as_deref().is_some_and(|b| b.same_as(&book)) {
        put_book(s, book);
    }
}

/// The preview's stand-in classes.
pub(crate) fn set_preview_book(b: ClassBook) {
    with(|s| put_book(s, b));
}

/// A small book for the tests: two guns with attachments, a pistol,
/// grenades, perks, a wildcard, scorestreaks and a camo; three classes.
#[cfg(test)]
pub(crate) fn test_book() -> ClassBook {
    let item = |index: i32, slot: &str, group: &str, cost: i32, atts: &[&str]| ClassItem {
        index,
        reference: format!("item{index}"),
        name: format!("Item {index}"),
        image: format!("menu_item{index}"),
        desc: String::new(),
        cost,
        momentum: if slot == "killstreak" { 400 } else { 0 },
        group: group.to_owned(),
        slot: slot.to_owned(),
        level: 1,
        locked: false,
        attachments: atts.iter().enumerate().map(|(k, a)| (k as i32 + 1, (*a).to_owned(), format!("menu_att_{a}"))).collect(),
    };
    let items = vec![
        item(1, "primary", "weapon_smg", 1, &["reflex", "silencer"]),
        item(2, "primary", "weapon_assault", 1, &["acog", "grip"]),
        item(3, "secondary", "weapon_pistol", 1, &["extclip"]),
        item(4, "secondary", "weapon_pistol", 1, &["silencer"]),
        item(10, "primarygrenade", "weapon_grenade", 1, &[]),
        item(11, "specialgrenade", "weapon_grenade", 1, &[]),
        item(20, "specialty1", "specialty", 1, &[]),
        item(21, "specialty1", "specialty", 1, &[]),
        item(22, "specialty2", "specialty", 1, &[]),
        item(30, "bonuscard", "bonuscard", 1, &[]),
        item(40, "killstreak", "killstreak", 0, &[]),
        item(41, "killstreak", "killstreak", 0, &[]),
    ];
    let class = |n: i32, primary: i32| ClassView {
        n,
        name: format!("Custom {}", n + 1),
        slots: HashMap::from([("primary".to_owned(), primary), ("secondary".to_owned(), 3)]),
        spent: 2,
    };
    ClassBook {
        classes: vec![class(0, 1), class(1, 2), class(2, 1)],
        items,
        camos: vec![Camo { index: 5, name: "GOLD".to_owned(), image: "menu_camo_gold".to_owned(), drawn: true, material: true }],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One class slot of each kind, its new value.
    const EDITS: [(&str, i32); 12] = [
        ("primary", 2),
        ("primaryattachment1", 2),
        ("primarycamo", 5),
        ("secondary", 4),
        ("secondaryattachment1", 1),
        ("primarygrenade", 10),
        ("specialgrenade", 11),
        ("specialty1", 21),
        ("specialty2", 22),
        ("bonuscard1", 30),
        ("killstreak1", 40),
        ("killstreak2", 41),
    ];

    #[test]
    fn an_edit_of_any_slot_is_in_the_book_with_its_points() {
        let book = test_book();
        for (slot, value) in EDITS {
            let b = book.with_edit(&Edit { class: 2, set: vec![(slot.to_owned(), value)] });
            let c = b.class(2).unwrap();
            assert_eq!(c.get(slot), value, "{slot}");
            let mut slots = book.class(2).unwrap().slots.clone();
            slots.insert(slot.to_owned(), value);
            assert_eq!(c.spent, spent(&book, &slots), "{slot}");
            // (The other classes as they were; a scorestreak is every
            // class's.)
            for n in [0, 1] {
                if shared_slot(slot) {
                    assert_eq!(b.class(n).unwrap().get(slot), value, "{slot}");
                } else {
                    assert_eq!(b.class(n), book.class(n), "{slot}");
                }
            }
            assert!(!b.same_as(&book), "{slot}");
        }
    }

    #[test]
    fn a_class_is_found_by_its_number_not_its_place() {
        let mut book = test_book();
        book.classes.reverse();
        assert_eq!(book.class(0).unwrap().n, 0);
        assert_eq!(book.class(2).unwrap().n, 2);
        let b = book.with_edit(&Edit { class: 0, set: vec![("primary".to_owned(), 2)] });
        assert_eq!(b.class(0).unwrap().get("primary"), 2);
        assert_eq!(b.class(2).unwrap().get("primary"), 1);
    }

    #[test]
    fn a_rename_is_in_the_book_and_an_empty_name_is_the_default() {
        let book = test_book();
        assert_eq!(book.with_name(1, "SNIPER").class(1).unwrap().name, "SNIPER");
        assert_eq!(book.with_name(1, "SNIPER").with_name(1, " ").class(1).unwrap().name, "Custom 2");
    }

    /// The tests that use the shared book take turns.
    static SHARED_BOOK: Mutex<()> = Mutex::new(());

    fn shared_book() -> std::sync::MutexGuard<'static, ()> {
        SHARED_BOOK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn an_edit_puts_up_a_new_book_at_once_and_a_read_back_keeps_it() {
        let _turn = shared_book();
        with(|s| *s = Shared::default());
        set_preview_book(test_book());
        let first = book().unwrap();
        assert!(first.rev > 0);
        // Each edit and rename: a new revision, the change in it, before
        // the profile has it.
        let mut rev = first.rev;
        for (slot, value) in EDITS {
            edit(Edit { class: 2, set: vec![(slot.to_owned(), value)] });
            let b = book().unwrap();
            assert!(b.rev > rev, "{slot}");
            rev = b.rev;
            assert_eq!(b.class(2).unwrap().get(slot), value, "{slot}");
        }
        rename(2, "RUSH".to_owned());
        let shown = book().unwrap();
        assert!(shown.rev > rev);
        assert_eq!(shown.class(2).unwrap().name, "RUSH");
        // A read of the profile from before the edits (`class_side` took
        // none yet) does not undo them: the queued ones are laid on it.
        with(|s| put_read(s, first.as_ref().clone()));
        let after = book().unwrap();
        assert_eq!(after.class(2), shown.class(2));
        assert_eq!(after.rev, shown.rev, "the same classes: no new book");
        // Once the profile took them, its read wins: one it did not take
        // is undone.
        let (edits, renames) = with(|s| (std::mem::take(&mut s.edits), std::mem::take(&mut s.renames)));
        assert_eq!((edits.len(), renames.len()), (EDITS.len(), 1));
        with(|s| put_read(s, first.as_ref().clone()));
        let undone = book().unwrap();
        assert!(undone.rev > shown.rev);
        assert!(undone.same_as(&first));
    }

    // --- End to end: his stats, `class_side`, every view, a save and a
    // new start. ---

    /// A statstable row (the columns `Profile::from_tables` reads).
    fn stat_row(index: i32, group: &str, slot: &str, atts: &str, classes: &str) -> Vec<String> {
        let mut r = vec![String::new(); 18];
        r[0] = index.to_string();
        r[2] = group.to_owned();
        r[3] = format!("ITEM_{index}");
        r[4] = format!("item{index}");
        r[6] = format!("menu_item{index}");
        r[8] = atts.to_owned();
        r[11] = classes.to_owned();
        r[12] = if slot == "killstreak" { "0" } else { "1" }.to_owned();
        r[13] = slot.to_owned();
        r[16] = if slot == "killstreak" { "400" } else { "0" }.to_owned();
        r
    }

    fn att_row(index: i32, group: &str, kind: &str, reference: &str) -> Vec<String> {
        let mut r = vec![String::new(); 13];
        r[0] = index.to_string();
        r[1] = group.to_owned();
        r[2] = kind.to_owned();
        r[3] = reference.to_uppercase();
        r[4] = reference.to_owned();
        r[6] = format!("menu_{reference}");
        r
    }

    /// BO2's scripts' host with his multiplayer answers and a small
    /// statstable (every default class: item 1 and the pistol 3), his
    /// stats file's `text` read first, as the front end and a match HUD
    /// start.
    fn front_end(text: Option<&str>) -> (Host, Mp) {
        let mut host = Host::new(Vec::new(), Box::new(|_: &str, _: &str, _: f32| 0.0));
        let mp = hks_t6::mp::install(&mut host);
        mp.borrow_mut().shell_classes = true;
        if let Some(t) = text {
            mp.borrow_mut().load_text(t);
        }
        let all = "class_custom_assault class_custom_smg class_custom_lmg class_custom_cqb class_custom_sniper";
        let stats = vec![
            stat_row(1, "weapon_smg", "primary", "reflex silencer", all),
            stat_row(2, "weapon_assault", "primary", "acog grip", ""),
            stat_row(3, "weapon_pistol", "secondary", "extclip", all),
            stat_row(4, "weapon_pistol", "secondary", "extclip silencer", ""),
            stat_row(10, "weapon_grenade", "primarygrenade", "", ""),
            stat_row(11, "weapon_grenade", "specialgrenade", "", ""),
            stat_row(20, "specialty", "specialty1", "", ""),
            stat_row(21, "specialty", "specialty1", "", ""),
            stat_row(22, "specialty", "specialty2", "", ""),
            stat_row(30, "bonuscard", "bonuscard", "", ""),
            stat_row(40, "killstreak", "killstreak", "", ""),
            stat_row(41, "killstreak", "killstreak", "", ""),
        ];
        let atts = vec![
            att_row(1, "attachment", "attachment", "reflex"),
            att_row(2, "attachment", "attachment", "silencer"),
            att_row(3, "attachment", "attachment", "acog"),
            att_row(4, "attachment", "attachment", "grip"),
            att_row(5, "attachment", "attachment", "extclip"),
            att_row(5, "camo", "weaponoption", "camo_gold"),
        ];
        {
            let mut v = host.values.borrow_mut();
            v.tables.insert("mp/statstable.csv".to_owned(), stats);
            v.tables.insert("mp/attachmenttable.csv".to_owned(), atts);
        }
        (host, mp)
    }

    /// Every view of class `n` shows `c` (its name, points and each slot):
    /// the class list's row and overview, the edit screen's rows, the
    /// pickers' class, the in-match Choose Class row and card.
    fn every_view_shows(book: &ClassBook, n: i32, c: &ClassView) {
        use super::super::cac;
        use super::super::{Act, Screen, Shell};
        let list = Shell { screen: Screen::Classes, ..Shell::default() };
        let rows = cac::rows(&list, book);
        let at = rows.iter().position(|r| r.act == Act::EditClass(n)).expect("listed");
        assert_eq!(rows[at].label, c.name.to_uppercase());
        assert_eq!(rows[at].sub, format!("{} / {POINTS} points", c.spent));
        assert_eq!(cac::focused_class(book, &rows, at), Some(c), "the list's overview");
        let edit = Shell { screen: Screen::ClassEdit, class: n, ..Shell::default() };
        assert_eq!(book.class(edit.class), Some(c), "the edit screen's class");
        for r in cac::rows(&edit, book) {
            if let Act::EditSlot(i) = r.act {
                let slot = cac::EDIT_SLOTS[i].0;
                assert_eq!(r.value, Some(cac::slot_value(book, c, slot)), "{slot}");
            }
        }
        let pick = Shell { screen: Screen::ItemPick, class: n, ..Shell::default() };
        assert_eq!(book.class(pick.class), Some(c), "the picker's class");
        // In a match: Choose Class's row (its card is the class at the
        // focused row's place).
        let ingame = super::super::ingame::rows(2);
        let k = ingame.iter().position(|r| r.act == Act::Choose(n as usize)).expect("in Choose Class");
        assert_eq!(ingame[k].label, c.name.to_uppercase());
        assert_eq!(book.classes.get(k), Some(c), "Choose Class's card");
    }

    /// His edit made where no profile is (a Zombies match's HUD): written
    /// to his file; the next start reads it, the match plays it, and BO2's
    /// class check putting the default back afterwards is refused.
    #[test]
    fn an_edit_with_no_profile_is_saved_and_played_and_kept() {
        let _turn = shared_book();
        with(|s| *s = Shared::default());
        let dir = std::env::temp_dir().join(format!("bo2mp_cac_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bo2mp_stats.txt");
        // His file: class 0 holds gun 1 (as the match read it).
        let (host, mp) = front_end(None);
        class_side(&host, Some(&mp));
        mp.borrow_mut().profile.set_stat(slot_key(0, "primary"), StatValue::Num(1.0));
        mp.borrow().profile.save_file(&file).unwrap();
        // He builds gun 2 in class 0 where no profile is, and leaves.
        set_preview_book(test_book());
        edit(Edit { class: 0, set: vec![("primary".to_owned(), 2)] });
        assert_eq!(book().unwrap().class(0).unwrap().get("primary"), 2, "shown at once");
        persist_edits(&file);
        assert!(with(|s| s.edits.is_empty()), "taken");
        bo2_profile::wait_saved(std::time::Duration::from_secs(10));
        let saved = Profile::read_stats_file(&file);
        assert_eq!(saved.get(&slot_key(0, "primary")).and_then(StatValue::as_num), Some(2.0), "in his file");
        // The next start (or a match's HUD) reads it; the match plays it.
        let text = std::fs::read_to_string(&file).unwrap();
        let (host2, mp2) = front_end(Some(&text));
        with(|s| *s = Shared::default());
        want(true);
        class_side(&host2, Some(&mp2));
        assert_eq!(book().unwrap().class(0).unwrap().get("primary"), 2, "the card");
        assert_eq!(mp2.borrow().profile.loadout_item(0, "primary"), 2, "the match's class 0");
        // BO2's class check puts the default back: refused.
        mp2.borrow_mut().set_stat(slot_key(0, "primary"), &hks_t6::value::Value::Num(1.0));
        assert_eq!(mp2.borrow().profile.loadout_item(0, "primary"), 2, "kept");
        want(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// His third report: the in-match Choose Class card showed Bal-27 /
    /// SMAW while he spawned with another gun. The front end (and the
    /// match's server, `sim::set_local_stats_path`) used
    /// `codlegacy-artifacts/bo2mp_stats.txt`, the match's card the profile
    /// folder's copy, stale since it was made. Now one file: a newer old
    /// copy is taken, and what Create-a-Class saves is what the card shows
    /// and the match gives (each slot of class 1).
    #[test]
    fn the_front_end_the_card_and_the_match_read_one_stats_file() {
        let _turn = shared_book();
        with(|s| *s = Shared::default());
        let dir = std::env::temp_dir().join(format!("bo2mp_one_file_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (profile, artifacts) = (dir.join("profile"), dir.join("codlegacy-artifacts"));
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        // SAFETY: test-only; the profile folder is read per call.
        unsafe { std::env::set_var("BO2_PROFILE_DIR", &profile) };
        // The profile folder's stale copy (class 0: gun 1), then the old
        // build's front end saving gun 2 in the artifacts folder.
        let (h, m) = front_end(None);
        class_side(&h, Some(&m));
        m.borrow_mut().profile.set_stat(slot_key(0, "primary"), StatValue::Num(1.0));
        m.borrow().profile.save_file(&profile.join("bo2mp_stats.txt")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        m.borrow_mut().profile.set_stat(slot_key(0, "primary"), StatValue::Num(2.0));
        m.borrow().profile.save_file(&artifacts.join("bo2mp_stats.txt")).unwrap();
        // The launcher's front end path, the match HUD's, the server's.
        let front = crate::lui_frontend::bo2mp_stats_file(Some(&artifacts));
        assert_eq!(front, crate::lui_frontend::stats_path(None), "one file");
        assert_eq!(front, profile.join("bo2mp_stats.txt"));
        let text = std::fs::read_to_string(&front).unwrap();
        assert_eq!(Profile::stats_from_text(&text).get(&slot_key(0, "primary")).and_then(StatValue::as_num), Some(2.0), "the newer copy taken");
        // Create-a-Class in the front end: class 1 (slot 0) rebuilt, saved
        // to the front end's path.
        let (host, mp) = front_end(Some(&text));
        want(true);
        class_side(&host, Some(&mp));
        let set: Vec<(String, i32)> = [("primary", 2), ("secondary", 4), ("primarygrenade", 10), ("primarygrenadecount", 1), ("specialgrenade", 11), ("specialgrenadecount", 2), ("specialty1", 21), ("specialty2", 22)]
            .iter()
            .map(|(s, v)| ((*s).to_owned(), *v))
            .collect();
        edit(Edit { class: 0, set: set.clone() });
        class_side(&host, Some(&mp));
        let all = mp.borrow().profile.stats.clone();
        bo2_profile::save_in_background(&front, bo2_profile::SaveJob::Whole(all), None);
        bo2_profile::wait_saved(std::time::Duration::from_secs(10));
        // The match: its HUD's card (read from `stats_path`) and the class
        // its server gives (read from the front end's path) are the edit.
        with(|s| *s = Shared::default());
        let card_text = std::fs::read_to_string(crate::lui_frontend::stats_path(None)).unwrap();
        let (host2, mp2) = front_end(Some(&card_text));
        class_side(&host2, Some(&mp2));
        let card = book().unwrap();
        let given = Profile::read_stats_file(&front);
        for (slot, v) in &set {
            assert_eq!(card.class(0).unwrap().get(slot), *v, "the card's {slot}");
            assert_eq!(mp2.borrow().profile.loadout_item(0, slot), *v, "the card's profile {slot}");
            assert_eq!(given.get(&slot_key(0, slot)).and_then(StatValue::as_num), Some(*v as f32), "the server's {slot}");
        }
        want(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_of_class_n_is_class_n_everywhere_and_no_other_class_changes() {
        let _turn = shared_book();
        with(|s| *s = Shared::default());
        want(true);
        let (host, mp) = front_end(None);
        class_side(&host, Some(&mp));
        let start = book().expect("his classes read");
        assert_eq!(start.classes.len(), 5);
        assert_eq!(start.class(0).unwrap().get("primary"), 1, "the default class");
        // Each class in turn, every kind of slot and its name.
        let edits: [(&str, i32); 14] = [
            ("primary", 2),
            ("primaryattachment1", 2),
            ("primarycamo", 5),
            ("secondary", 4),
            ("secondaryattachment1", 2),
            ("primarygrenade", 10),
            ("primarygrenadecount", 1),
            ("specialgrenade", 11),
            ("specialgrenadecount", 2),
            ("specialty1", 21),
            ("specialty2", 22),
            ("bonuscard1", 30),
            ("killstreak1", 41),
            ("killstreak2", 40),
        ];
        for n in [3, 0, 4, 1, 2] {
            let before = book().unwrap();
            for (k, (slot, value)) in edits.iter().enumerate() {
                // (Each class its own values: a gun or perk differs by class.)
                let value = if *slot == "primary" && n % 2 == 1 { 1 } else { *value };
                edit(Edit { class: n, set: vec![((*slot).to_owned(), value)] });
                // Shown at once, before the profile has it.
                assert_eq!(book().unwrap().class(n).unwrap().get(slot), value, "class {n} {slot} at once");
                class_side(&host, Some(&mp));
                let b = book().unwrap();
                assert_eq!(b.class(n).unwrap().get(slot), value, "class {n} {slot} after the read-back");
                assert_eq!(mp.borrow().profile.stat_num(&slot_key(n, slot)) as i32, value, "class {n} {slot} in his stats");
                assert!(k == 0 || edits[..k].iter().all(|(s, _)| b.class(n).unwrap().slots.contains_key(*s)), "earlier edits kept");
            }
            rename(n, format!("CLASS {n}"));
            class_side(&host, Some(&mp));
            // BO2's own class check, under the shell, puts the guns back:
            // refused (the set is Create-a-Class's).
            for slot in ["primary", "secondary", "primarycamo"] {
                mp.borrow_mut().set_stat(Profile::class_key(LOCAL_MATCH_SET, n, slot), &hks_t6::value::Value::Num(1.0));
            }
            mp.borrow_mut().set_stat(format!("{LOCAL_MATCH_SET}.customclassname.{n}"), &hks_t6::value::Value::str("Custom"));
            // (A while later: read again.)
            with(|s| s.built_at = -1.0e9);
            class_side(&host, Some(&mp));
            let b = book().unwrap();
            let c = b.class(n).unwrap().clone();
            assert_eq!(c.name, format!("CLASS {n}"));
            for (slot, _) in edits {
                let want = if slot == "primary" && n % 2 == 1 { 1 } else { edits.iter().find(|(s, _)| *s == slot).unwrap().1 };
                assert_eq!(c.get(slot), want, "class {n} {slot} kept");
            }
            assert_eq!(c.spent, spent(&b, &c.slots));
            every_view_shows(&b, n, &c);
            // No other class changed but for the scorestreaks (his, shared).
            for m in (0..5).filter(|m| *m != n) {
                let (was, now) = (before.class(m).unwrap(), b.class(m).unwrap());
                assert_eq!(now.name, was.name, "class {m} after editing {n}");
                for (slot, v) in &now.slots {
                    if shared_slot(slot) {
                        assert_eq!(*v, c.get(slot), "class {m} {slot}: his streaks");
                    } else {
                        assert_eq!(*v, was.get(slot), "class {m} {slot} after editing {n}");
                    }
                }
                every_view_shows(&b, m, now);
            }
        }
        // Saved and read by the next start (or a match's HUD): the same
        // classes, in every view.
        let edited = book().unwrap();
        let text = mp.borrow().to_text();
        let (host2, mp2) = front_end(Some(&text));
        with(|s| *s = Shared::default());
        class_side(&host2, Some(&mp2));
        let again = book().unwrap();
        assert_eq!(again.classes, edited.classes, "the classes as he left them");
        for c in &again.classes {
            every_view_shows(&again, c.n, c);
        }
        // The match plays them: his scorestreaks in class 0, where it reads.
        let p = &mp2.borrow().profile;
        assert_eq!(p.loadout_item(4, "primary"), 2);
        assert_eq!(p.loadout_item(0, "killstreak1"), 41);
        want(false);
    }
}

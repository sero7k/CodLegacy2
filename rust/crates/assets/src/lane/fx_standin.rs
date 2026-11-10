//! bo2mp: stand-ins for Modern Warfare III (2023) effects.
//!
//! MW3's own effects cannot be played yet (`docs/JUP.md`, "Effects"), so each
//! MW3 effect asked for, by name and category, takes an effect of a game that
//! is played properly: Black Ops III (the T7 lane) or Modern Warfare
//! Remastered / Horizon MW (the H1 lane). The table is data
//! (`data/fx_standins.tsv`, its header explains the columns): overrides
//! by whole path, keyword rules, then a default per category.
//!
//! Anything that asks for an MW3 effect goes through [`request`]: it gives
//! the stand-in or, when there is no good match, logs the effect once and
//! returns `None` so the caller skips it rather than show something wrong.
//! [`report_lines`] gives the counts by category and the unmatched list.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// What kind of effect it is (the classifier's classes, `Category::classify`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Category {
    MuzzleFlash,
    ShellEject,
    Impact,
    Explosion,
    Smoke,
    Fire,
    Spark,
    Dust,
    Water,
    Light,
    Decal,
    Vehicle,
    Other,
}

impl Category {
    pub(crate) const ALL: [Category; 13] = [
        Self::MuzzleFlash,
        Self::ShellEject,
        Self::Impact,
        Self::Explosion,
        Self::Smoke,
        Self::Fire,
        Self::Spark,
        Self::Dust,
        Self::Water,
        Self::Light,
        Self::Decal,
        Self::Vehicle,
        Self::Other,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::MuzzleFlash => "muzzle_flash",
            Self::ShellEject => "shell_eject",
            Self::Impact => "impact",
            Self::Explosion => "explosion",
            Self::Smoke => "smoke",
            Self::Fire => "fire",
            Self::Spark => "spark",
            Self::Dust => "dust",
            Self::Water => "water",
            Self::Light => "light",
            Self::Decal => "decal",
            Self::Vehicle => "vehicle",
            Self::Other => "other",
        }
    }

    /// The category from the effect's path alone (a tracer is `Other`).
    pub(crate) fn classify(name: &str) -> Self {
        let has_any = |s: &str, words: &[&str]| words.iter().any(|w| s.contains(w));
        let n = name.to_ascii_lowercase();
        if has_any(&n, &["muzzle", "muz_", "muzflash", "_mflash", "gunflash"]) {
            return Self::MuzzleFlash;
        }
        if has_any(&n, &["/shell/", "shell_", "brass", "casing", "_eject", "ejection"]) {
            return Self::ShellEject;
        }
        if has_any(&n, &["/_impact/", "_imp_", "bullet_hit", "_hit_"]) && !has_any(&n, &["exp", "veh_"]) {
            return Self::Impact;
        }
        if n.contains("tracer") {
            return Self::Other;
        }
        if n.contains("kickup") {
            return Self::Dust;
        }
        if n.contains("decal") {
            return Self::Decal;
        }
        if has_any(&n, &["spark", "ember", "pyro_spark"]) {
            return Self::Spark;
        }
        if has_any(&n, &["fire", "flame", "burn", "pyro", "thermite", "molotov"]) {
            return Self::Fire;
        }
        if has_any(&n, &["smoke", "smk", "steam", "vapor", "mist", "fog", "cloud", "gas"]) {
            return Self::Smoke;
        }
        if has_any(&n, &["dust", "dirt", "debris", "sand", "mud", "gravel", "particulate"]) {
            return Self::Dust;
        }
        if has_any(&n, &["water", "wtr", "rain", "splash", "drip", "fluid", "puddle"]) {
            return Self::Water;
        }
        if has_any(&n, &["/exp", "explo", "_exp_", "blast", "c4", "grenade", "mine"]) {
            return Self::Explosion;
        }
        if has_any(&n, &["flare", "light", "lens"]) {
            return Self::Light;
        }
        if has_any(&n, &["/veh", "vehicle", "tread", "_dmg_"]) {
            return Self::Vehicle;
        }
        Self::Other
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
}

/// The game a stand-in comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// Black Ops III.
    T7,
    /// Modern Warfare Remastered / Horizon MW.
    H1,
}

impl Source {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::T7 => "t7",
            Self::H1 => "h1",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "t7" => Some(Self::T7),
            "h1" => Some(Self::H1),
            _ => None,
        }
    }

    /// The namespace the match's effect catalog keeps this game's effects
    /// in.
    pub(crate) fn namespace(self) -> asset_core::AssetNamespace {
        match self {
            Self::T7 => asset_core::AssetNamespace::T7,
            Self::H1 => asset_core::AssetNamespace::H1,
        }
    }
}

/// Which effect of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceEffect {
    /// This exact effect (or impact table) name.
    Exact(String),
    /// The source's effect whose name holds all these words.
    Selector(Vec<String>),
}

impl SourceEffect {
    /// `=name` or `~a+b`.
    fn parse(e: &str) -> Option<Self> {
        if let Some(n) = e.strip_prefix('=') {
            (!n.is_empty()).then(|| Self::Exact(n.to_owned()))
        } else if let Some(w) = e.strip_prefix('~') {
            (!w.is_empty()).then(|| Self::Selector(w.split('+').map(str::to_ascii_lowercase).collect()))
        } else {
            None
        }
    }

    /// The source effect to use, given the names the source has. An exact
    /// name must be among them; a selector takes the shortest name holding
    /// all its words (then the first alphabetically).
    pub(crate) fn pick(&self, names: &[String]) -> Option<String> {
        match self {
            Self::Exact(n) => names.iter().find(|x| x.eq_ignore_ascii_case(n)).cloned(),
            Self::Selector(words) => names
                .iter()
                .filter(|n| {
                    let lower = n.to_ascii_lowercase();
                    words.iter().all(|w| lower.contains(w.as_str()))
                })
                .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
                .cloned(),
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Exact(n) => format!("={n}"),
            Self::Selector(w) => format!("~{}", w.join("+")),
        }
    }
}

/// A chosen stand-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Standin {
    pub category: Category,
    pub source: Source,
    pub effect: SourceEffect,
    /// The first-person effect, for a gun's flash and eject (`effect` is
    /// the world one).
    pub view: Option<SourceEffect>,
}

/// Why there is no stand-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unmatched {
    /// A row says there is no good match (or the category has none).
    NoMatch,
}

#[derive(Clone, Debug)]
enum Kind {
    Override(String),
    Rule(Vec<String>),
    Default(Category),
}

#[derive(Clone, Debug)]
struct Row {
    kind: Kind,
    /// `None`: any category.
    category: Option<Category>,
    /// `None`: no stand-in.
    to: Option<(Source, SourceEffect, Option<SourceEffect>)>,
}

/// The stand-in table.
#[derive(Debug, Default)]
pub(crate) struct Table {
    rows: Vec<Row>,
}

/// An effect path in the form the table matches: lower case, `/`, no
/// `.vfx`.
pub(crate) fn normalize(name: &str) -> String {
    let n = name.trim().replace('\\', "/").to_ascii_lowercase();
    n.strip_suffix(".vfx").map(str::to_owned).unwrap_or(n)
}

fn tokens(n: &str) -> Vec<&str> {
    n.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).collect()
}

impl Table {
    /// Read a table; the error names the first bad line.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut rows = Vec::new();
        for (k, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| format!("jup fx stand-ins, line {}: {why}", k + 1);
            let c: Vec<&str> = line.split_whitespace().collect();
            let [kind, pattern, category, source, effect] = c[..] else {
                return Err(bad("expected 5 columns"));
            };
            let to = match (source, effect) {
                ("-", "-") => None,
                ("-", _) | (_, "-") => return Err(bad("source and effect are both - or neither")),
                (s, e) => {
                    let source = Source::parse(s).ok_or_else(|| bad("unknown source"))?;
                    let (world, view) = match e.split_once('|') {
                        Some((w, v)) => (w, Some(v)),
                        None => (e, None),
                    };
                    let effect = SourceEffect::parse(world).ok_or_else(|| bad("effect starts with = or ~"))?;
                    // (The view effect is written without the =.)
                    let view = view
                        .map(|v| {
                            let v = if v.starts_with(['=', '~']) { v.to_owned() } else { format!("{}{v}", &world[..1]) };
                            SourceEffect::parse(&v).ok_or_else(|| bad("view effect after |"))
                        })
                        .transpose()?;
                    Some((source, effect, view))
                }
            };
            let cat = (category != "*").then(|| Category::parse(category).ok_or_else(|| bad("unknown category"))).transpose()?;
            let kind = match kind {
                "override" => Kind::Override(normalize(pattern)),
                "rule" => Kind::Rule(pattern.split('+').map(str::to_ascii_lowercase).collect()),
                "default" => Kind::Default(cat.ok_or_else(|| bad("a default needs its category"))?),
                _ => return Err(bad("kind is override, rule or default")),
            };
            rows.push(Row { kind, category: cat, to });
        }
        Ok(Self { rows })
    }

    /// The category of an effect: the caller's when it knows it, else the
    /// classifier's (`Category::classify`) on the path.
    pub(crate) fn category_of(name: &str, hint: Option<Category>) -> Category {
        hint.unwrap_or_else(|| Category::classify(name))
    }

    /// The stand-in for an MW3 effect.
    pub(crate) fn resolve(&self, name: &str, hint: Option<Category>) -> Result<Standin, Unmatched> {
        let n = normalize(name);
        let category = Self::category_of(&n, hint);
        let toks = tokens(&n);
        let fits = |r: &Row| r.category.is_none_or(|c| c == category);
        let row = self
            .rows
            .iter()
            .find(|r| matches!(&r.kind, Kind::Override(p) if *p == n) && fits(r))
            .or_else(|| {
                self.rows.iter().find(|r| match &r.kind {
                    Kind::Rule(words) => fits(r) && words.iter().all(|w| toks.iter().any(|t| t.starts_with(w.as_str()))),
                    _ => false,
                })
            })
            .or_else(|| self.rows.iter().find(|r| matches!(r.kind, Kind::Default(c) if c == category)));
        let Some((source, effect, view)) = row.and_then(|r| r.to.clone()) else {
            return Err(Unmatched::NoMatch);
        };
        Ok(Standin { category, source, effect, view })
    }
}

/// The built-in table.
pub(crate) fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        Table::parse(include_str!("../../data/fx_standins.tsv")).unwrap_or_else(|e| {
            eprintln!("bo2mp jup fx: {e}; no stand-ins");
            Table::default()
        })
    })
}

/// What one run asked for.
#[derive(Default)]
struct Ledger {
    /// Per category: the distinct effects matched, by source.
    matched: BTreeMap<Category, BTreeMap<Source, usize>>,
    seen: BTreeMap<String, Option<(Source, String)>>,
    unmatched: BTreeMap<String, (Category, Unmatched)>,
}

static LEDGER: Mutex<Option<Ledger>> = Mutex::new(None);

/// Ask for the stand-in of an MW3 effect (`site` says who asks, for the
/// log). `hint` is the category when the caller knows it (a gun's muzzle
/// flash). `None` means skip the effect: it was logged once.
pub(crate) fn request(site: &str, name: &str, hint: Option<Category>) -> Option<Standin> {
    let result = table().resolve(name, hint);
    let key = normalize(name);
    let Ok(mut guard) = LEDGER.lock() else {
        return result.ok();
    };
    let ledger = guard.get_or_insert_with(Ledger::default);
    match &result {
        Ok(s) => {
            if ledger.seen.insert(key, Some((s.source, s.effect.text()))).is_none() {
                *ledger.matched.entry(s.category).or_default().entry(s.source).or_default() += 1;
            }
        }
        Err(why) => {
            if ledger.seen.insert(key.clone(), None).is_none() {
                let category = Table::category_of(&key, hint);
                eprintln!("bo2mp jup fx: no stand-in for {key} ({}; {why:?}); skipped [{site}]", category.name());
                ledger.unmatched.insert(key, (category, *why));
            }
        }
    }
    result.ok()
}

/// Counts by category and the unmatched list, for the load report.
pub(crate) fn report_lines() -> Vec<String> {
    let Ok(guard) = LEDGER.lock() else {
        return Vec::new();
    };
    let Some(l) = guard.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in Category::ALL {
        let m = l.matched.get(&c);
        let u = l.unmatched.values().filter(|(k, _)| *k == c).count();
        if m.is_none() && u == 0 {
            continue;
        }
        let by_source: Vec<String> = m.into_iter().flatten().map(|(s, n)| format!("{} {n}", s.name())).collect();
        out.push(format!(
            "jup fx stand-ins: {}: {} matched ({}), {u} unmatched",
            c.name(),
            m.map_or(0, |m| m.values().sum::<usize>()),
            by_source.join(", ")
        ));
    }
    for (name, (c, why)) in &l.unmatched {
        out.push(format!("jup fx stand-ins: unmatched {name} ({}, {why:?})", c.name()));
    }
    out
}

/// What a gun asks for, as (slot word, category): its muzzle flash and shell
/// eject, and the impact of its bullets (a launcher's explosion instead; it
/// fires no brass, a melee weapon nothing).
fn gun_asks(group: &str) -> Vec<(&'static str, Category)> {
    if group.contains("melee") {
        return Vec::new();
    }
    let mut asks = vec![("muzzle_flash", Category::MuzzleFlash)];
    if group.contains("launcher") {
        asks.push(("explosion", Category::Explosion));
    } else {
        asks.push(("shell_eject", Category::ShellEject));
        asks.push(("impact", Category::Impact));
    }
    asks
}

/// The request name of a gun's slot. The guns' own effect paths are not read
/// (the weapon assets are not decoded, `docs/JUP.md`), so the request names
/// the slot by the gun's item group and name, whose words the rules match.
fn gun_request(gun: &str, group: &str, slot: &str) -> String {
    format!("weapon/{group}/{gun}/{slot}")
}

/// The impact types of Black Ops II's table a bullet makes (`ImpactType`
/// 1..=5: small, large, armour piercing, explosive, shotgun), as bits.
const BULLET_IMPACT_TYPES: u32 = 0b11_1110;

/// The cells of a Black Ops II impact row for a guest gun
/// (`WeaponCombatFx::impact_row`): the non-flesh surface types, then the four
/// flesh cells (`fx_iw4::FX_IMPACT_NONFLESH_COUNT`, `FX_IMPACT_FLESH_COUNT`).
const NONFLESH_CELLS: usize = 31;
const FLESH_CELLS: usize = 4;

/// An ImpactFx table's exit twin (`bullet_small_enter` ->
/// `bullet_small_exit`).
fn exit_table(enter: &str) -> Option<String> {
    enter.strip_suffix("_enter").map(|s| format!("{s}_exit"))
}

/// What was chosen out of the games' effect lists, and the impact cells read.
#[derive(Default)]
pub(crate) struct Plan {
    /// Per source effect (`SourceEffect::text`): the name it came to in the
    /// game's list (`None`: nothing there).
    picks: BTreeMap<(Source, String), Option<String>>,
    /// Per impact table: its 31 non-flesh cells then 4 flesh cells.
    impacts: BTreeMap<String, Vec<String>>,
}

static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

fn with_plan<R>(f: impl FnOnce(&mut Plan) -> R) -> Option<R> {
    let mut g = PLAN.lock().ok()?;
    Some(f(g.get_or_insert_with(Plan::default)))
}

impl Plan {
    /// Of `standins`, those of `source`: pick each effect in `names`, noting
    /// what each came to. The effects (not impacts) wanted, and the impact
    /// tables wanted (with their exit twins).
    fn choose(&mut self, source: Source, names: &[String], standins: &[Standin]) -> (Vec<String>, Vec<String>) {
        let (mut effects, mut tables) = (Vec::new(), Vec::new());
        for s in standins.iter().filter(|s| s.source == source) {
            for e in std::iter::once(&s.effect).chain(s.view.as_ref()) {
                if s.category == Category::Impact {
                    if let SourceEffect::Exact(t) = e {
                        tables.push(t.clone());
                        tables.extend(exit_table(t));
                    }
                    continue;
                }
                let picked = e.pick(names);
                self.picks.insert((source, e.text()), picked.clone());
                effects.extend(picked);
            }
        }
        effects.sort();
        effects.dedup();
        tables.sort();
        tables.dedup();
        (effects, tables)
    }

    fn picked(&self, source: Source, e: &SourceEffect) -> Option<&str> {
        self.picks.get(&(source, e.text())).and_then(|p| p.as_deref())
    }

    /// A gun's effects for its asks, from what was chosen.
    fn effects_for(&self, asks: &[(Category, Standin)]) -> asset_game::GuestEffects {
        use asset_game::{FX_SLOT_EXPLOSION, FX_SLOT_VIEW_EJECT, FX_SLOT_VIEW_FLASH, FX_SLOT_VIEW_LAST_EJECT, FX_SLOT_WORLD_EJECT, FX_SLOT_WORLD_FLASH, FX_SLOT_WORLD_LAST_EJECT};
        let mut slots = Vec::new();
        let mut impact = None;
        for (category, s) in asks {
            let ns = s.source.namespace();
            let world = self.picked(s.source, &s.effect).map(str::to_owned);
            let view = s.view.as_ref().and_then(|v| self.picked(s.source, v)).map(str::to_owned).or_else(|| world.clone());
            match category {
                Category::MuzzleFlash => {
                    slots.extend(view.map(|n| (FX_SLOT_VIEW_FLASH, ns, n)));
                    slots.extend(world.map(|n| (FX_SLOT_WORLD_FLASH, ns, n)));
                }
                Category::ShellEject => {
                    // (The last-shot ejects play the same: the donor's would
                    // otherwise on a last shot.)
                    for (bits, n) in [([FX_SLOT_VIEW_EJECT, FX_SLOT_VIEW_LAST_EJECT], view), ([FX_SLOT_WORLD_EJECT, FX_SLOT_WORLD_LAST_EJECT], world)] {
                        if let Some(n) = n {
                            slots.extend(bits.map(|b| (b, ns, n.clone())));
                        }
                    }
                }
                Category::Explosion => slots.extend(world.map(|n| (FX_SLOT_EXPLOSION, ns, n))),
                Category::Impact => {
                    let SourceEffect::Exact(table) = &s.effect else { continue };
                    let Some(enter) = self.impacts.get(table).filter(|c| c.iter().any(|n| !n.is_empty())) else { continue };
                    let exit = exit_table(table).and_then(|t| self.impacts.get(&t)).cloned().unwrap_or_default();
                    impact = Some(asset_game::GuestImpacts { namespace: ns, enter: enter.clone(), exit, types: BULLET_IMPACT_TYPES });
                }
                _ => {}
            }
        }
        asset_game::GuestEffects::new(slots, impact)
    }
}

/// A gun's stand-ins as the table gives them (no ledger).
fn gun_standins(table: &Table, gun: &str, group: &str) -> Vec<(Category, Standin)> {
    gun_asks(group)
        .into_iter()
        .filter_map(|(slot, category)| Some((category, table.resolve(&gun_request(gun, group, slot), Some(category)).ok()?)))
        .collect()
}

/// Every stand-in the guns of the roster ask for.
fn roster_standins(table: &Table) -> Vec<Standin> {
    bo2_profile::guest::JUP.iter().flat_map(|g| gun_standins(table, g.gun, g.group)).map(|(_, s)| s).collect()
}

/// Black Ops III's effect names are in: the effects and impact tables the
/// roster's stand-ins want from it. Called with the shared zones' list.
pub(crate) fn choose_t7(names: &[String]) -> (Vec<String>, Vec<String>) {
    let standins = roster_standins(table());
    with_plan(|p| p.choose(Source::T7, names, &standins)).unwrap_or_default()
}

/// Modern Warfare Remastered's effect names are in: the effects the roster's
/// stand-ins want from it.
pub(crate) fn choose_h1(names: &[String]) -> Vec<String> {
    let standins = roster_standins(table());
    with_plan(|p| p.choose(Source::H1, names, &standins).0).unwrap_or_default()
}

/// The impact tables read (`(table, 31 non-flesh cells, 4 flesh cells)`).
pub(crate) fn set_impact_cells(tables: Vec<(String, Vec<String>, Vec<String>)>) {
    with_plan(|p| {
        for (name, mut surf, flesh) in tables {
            surf.resize(NONFLESH_CELLS, String::new());
            surf.extend(flesh.into_iter().take(FLESH_CELLS));
            p.impacts.insert(name, surf);
        }
    });
}

/// A Modern Warfare III gun's effects (`gun`, its item group): the
/// stand-ins for its muzzle flash, shell eject and bullet impacts (a
/// launcher's explosion), as the weapon catalog takes them
/// (`WeaponBuild::set_guest_effects`). Each is asked for (so it is counted
/// and an unmatched one logged); a slot whose effect the games' lists did not
/// have is left out and plays the donor's.
pub(crate) fn guest_effects(gun: &str, group: &str) -> asset_game::GuestEffects {
    let mut asks = Vec::new();
    for (slot, category) in gun_asks(group) {
        if let Some(s) = request(&format!("gun {gun}"), &gun_request(gun, group, slot), Some(category)) {
            asks.push((category, s));
        }
    }
    with_plan(|p| p.effects_for(&asks)).unwrap_or_default()
}

/// The effects the stand-ins play, built.
pub(crate) struct StandinFx {
    pub fx: asset_game::FxCatalog,
    pub fx_models: asset_game::FxModelCatalog,
    pub material_seed: asset_material::MaterialCatalog,
    pub report: Vec<String>,
}

/// Reads the effects the roster's stand-ins want from Modern Warfare
/// Remastered's and Black Ops III's retail zones, only those (and the effects
/// they run), their sprites' materials linked into `material_seed`. A game
/// not installed leaves its stand-ins out, and the guns keep their donors'.
pub(crate) fn build_standin_fx(material_seed: asset_material::MaterialCatalog) -> StandinFx {
    build_standin_fx_in(super::hmw::standin_zone_dir(), super::t7::bo3_zone_dir(), material_seed)
}

/// As `build_standin_fx`, from the zone folders given (`mwr`'s `zone`,
/// Black Ops III's `zone`).
pub(crate) fn build_standin_fx_in(
    h1_dir: Option<std::path::PathBuf>,
    t7_dir: Result<std::path::PathBuf, String>,
    material_seed: asset_material::MaterialCatalog,
) -> StandinFx {
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let mut fx = asset_game::FxCatalog::default();
    let mut fx_models = asset_game::FxModelCatalog::default();
    let mut seed = material_seed;
    match h1_dir {
        Some(dir) => {
            let h1 = super::hmw::standin_effects(&dir, seed, Box::new(choose_h1));
            report.extend(h1.report);
            fx = h1.fx;
            fx_models = h1.fx_models;
            seed = h1.material_seed;
        }
        None => report.push("jup fx stand-ins: Modern Warfare Remastered's zone folder not found (Game Folders: mwr); no H1 stand-ins".into()),
    }
    match t7_dir {
        Ok(dir) => {
            let t7 = super::t7_map::shared_effects(&dir, seed, Box::new(choose_t7));
            report.extend(t7.report);
            seed = t7.material_seed;
            fx.set_capture_zone(asset_core::ZoneOwner::from_zone_path(&dir.join("core_common.ff")));
            let captured = t7.views.iter().filter(|v| fx.capture_t7_in(asset_core::AssetNamespace::T7, v)).count();
            report.push(format!("jup fx stand-ins: {captured} of {} Black Ops III effects captured", t7.views.len()));
            set_impact_cells(t7.impacts);
        }
        Err(e) => report.push(format!("jup fx stand-ins: {e}; no Black Ops III stand-ins")),
    }
    report.push(format!("jup fx stand-ins: {} effects, {} effect models built [{:.1} s]", fx.len(), fx_models.len(), started.elapsed().as_secs_f64()));
    StandinFx { fx, fx_models, material_seed: seed, report }
}

/// The effects a placed MW3 entity names (`fxid`, `fxdef`, `script_fxid`,
/// `vfx`), asked for with their own paths. The maps place their ambient
/// effects in a way not read yet (JUP.md), so this finds none until
/// entities carry them.
pub(crate) fn entity_effects(site: &str, entities: &str) -> usize {
    let mut n = 0;
    for block in entities.split('{') {
        for key in ["fxid", "fxdef", "script_fxid", "vfx"] {
            let Some(at) = block.find(&format!("\"{key}\" \"")) else { continue };
            let rest = &block[at + key.len() + 4..];
            if let Some(value) = rest.split('"').next().filter(|v| !v.is_empty()) {
                request(site, value, None);
                n += 1;
            }
        }
    }
    n
}

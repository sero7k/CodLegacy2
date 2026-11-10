//! Which builtins the loaded scripts call, and what the host does with each. The static census
//! walks every instruction of the scripts handed over (all of `common` plus the map's); the
//! status of each id is found by calling the host's own dispatcher with no arguments, so it can
//! not drift from the code: an id the dispatcher turns away is `Unimplemented`.

#![allow(dead_code, reason = "the census example and the tests use it; the game does not")]

use std::collections::BTreeMap;

use gsc_iw6::builtins::{self, Confidence, Kind};
use gsc_iw6_vm::{BuiltinUse, CallCtx, EntityId, HostReturn, Value};

use super::host::{HostEnt, Iw6Host};

/// What the host does with a builtin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Does what the engine does, as far as the sim shows it.
    Implemented,
    /// Does part of it or stands in for it (a trace that always misses, a table that is empty).
    Approximate,
    /// Takes the call and does nothing (setup the sim has no use for: precache, hud, vision).
    NoOp,
    /// The host turns it away: the VM logs it once, counts it and pushes `undefined`.
    Unimplemented,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Implemented => "implemented",
            Status::Approximate => "approximate",
            Status::NoOp => "no-op",
            Status::Unimplemented => "UNIMPLEMENTED",
        }
    }
}

/// Builtins that are accepted and have no effect on the sim (nothing to draw, hear or hit).
const NOOP: &[&str] = &[
    // Debug draws and developer helpers: nothing in a shipped match.
    "print3d", "devaddpitch", "devaddroll", "devaddyaw",
    // Setup nobody listens to offline, or for a client or server this match does not have.
    "setblurforplayer", "physicslaunchserver", "resetscriptslot", "connectnodepair", "disconnectnodepair", "setcursorhint",
    "clonebrushmodeltoscriptmodel", "moveshieldmodel", "sendmatchdata", "sendclientmatchdata", "setclientmatchdatadef", "kick", "bullettracer",
];

/// Builtins with a stand-in answer.
const APPROX: &[&str] = &[
    "getglass", "getscriptablearray",
    "setplayerangles",
    "getspectatingplayer",
    
    "movegravity",
    "bullettrace", "physicstrace", "getnode", "getnodearray", "getvehiclenode", "getvehiclenodearray", 
    "isvehicle", "getweaponattachments", "getbaseweaponname", "weaponclass",
    "getmatchrulesdata", "isusingmatchrulesdata", "soundgetplaybacktime", "getnorthyaw",
    "isweaponvalid", "botsconnecttype", "tablelookupistring", "strtok", "playloopedfx", "spawnfx", "triggerfx", "playfxontagforclients",
    "playsoundasmaster", "buttonpressed", "getplayerangles", 
    "getguid", "getrankedplayerdata", "getprivateplayerdata", "setrankedplayerdata", "setprivateplayerdata",
    // 2026-11-05: a store the script reads back, or a plain answer.
    "setplayerdata", "getplayerdata", "setcoopplayerdata", "getcoopplayerdata", "setcommonplayerdata", "getcommonplayerdata", "getcacplayerdata",
    "setcommonplayerdatareservedint", "setrankedplayerdatareservedint", "getrankedplayerdatareservedint", "setmatchdata", "getmatchdata",
    "setclientmatchdata", "getclientmatchdata", "isitemunlocked", "getxuid", "sighttracepassed", "bullettracepassed", "playerphysicstrace", "getspawnarray",
    "weaponclipsize", "weaponinventorytype", "getweaponmodel", "playerads", "setstance", "setcontents",
    "allowspectateteam", "notifyonplayercommand", 
];

/// Ids the VM itself answers (`gsc_iw6_vm::CoreIds`).
const VM_IDS: [u16; 5] = [0x2d, 0x18c, 0x1c0, 0x1c1, 0x1a6];

/// Whether the dispatcher accepts the id (a probe on a scratch host; an error from missing
/// arguments counts as accepted).
fn accepts(method: bool, id: u16) -> bool {
    let mut h = Iw6Host::default();
    let mut e = HostEnt::default();
    e.fields.insert(gsc_iw6_vm::Name::Id(super::host::F_CLASSNAME), Value::str("script_model"));
    h.add_ent(e);
    let this = Value::Entity(EntityId(0));
    let level = Value::Undefined;
    let mut ctx = CallCtx::detached(&this, &level, 0);
    let r = if method { super::builtins::method(&mut h, id, &mut ctx, &this, &[]) } else { super::builtins::function(&mut h, id, &mut ctx, &[]) };
    !matches!(r, Ok(HostReturn::Unimplemented))
}

/// The status of a builtin id (methods carry the 0x8000 bias as scripts encode them).
pub fn status(method: bool, id: u16) -> Status {
    let kind = if method { Kind::Method } else { Kind::Function };
    if !method && VM_IDS.contains(&id) {
        return Status::Implemented;
    }
    let name = builtins::name(kind, id);
    if !accepts(method, id) {
        return Status::Unimplemented;
    }
    match name {
        Some(_) if method && super::player::flag_op(id).is_some() => Status::Implemented,
        Some(n) if NOOP.contains(&n) || n.starts_with("unk_") => Status::NoOp,
        Some(n) if APPROX.contains(&n) => Status::Approximate,
        // Accepted by id with no name: the ids the dispatcher swallows (stubs, flags).
        None => Status::NoOp,
        Some(_) => Status::Implemented,
    }
}

/// One line of the census.
#[derive(Clone, Debug)]
pub struct Row {
    pub method: bool,
    pub id: u16,
    pub name: Option<&'static str>,
    pub confidence: Option<Confidence>,
    pub status: Status,
    pub calls: u64,
    pub scripts: Vec<String>,
}

/// The census of a set of scripts: every builtin they call with its status, most called first.
pub fn rows(uses: &BTreeMap<(bool, u16), BuiltinUse>) -> Vec<Row> {
    let mut rows: Vec<Row> = uses
        .iter()
        .map(|((method, id), u)| {
            let kind = if *method { Kind::Method } else { Kind::Function };
            let named = builtins::name_with_confidence(kind, *id);
            Row {
                method: *method,
                id: *id,
                name: named.map(|n| n.0),
                confidence: named.map(|n| n.1),
                status: status(*method, *id),
                calls: u.calls,
                scripts: u.scripts.iter().map(|s| script_label(s)).collect(),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.calls.cmp(&a.calls).then(a.id.cmp(&b.id)));
    rows
}

/// Script files the notes identified by content (`docs/exe_notes/IW6_GSC.md`).
const KNOWN_FILES: [(&str, &str); 12] = [
    ("#927", "common_scripts/utility"),
    ("#922", "common_scripts/_exploder"),
    ("#916", "common_scripts/_createfx"),
    ("#923", "common_scripts/_fx"),
    ("#919", "common_scripts/_destructible"),
    ("#1373", "maps/mp/_load"),
    ("#1357", "maps/mp/_compass"),
    ("#1385", "maps/mp/_utility"),
    ("#1474", "maps/mp/_movers"),
    ("#1399", "map precache"),
    ("#1398", "map fx"),
    ("#940", "toy_transformer destructible"),
];

fn script_label(name: &str) -> String {
    KNOWN_FILES.iter().find(|(id, _)| *id == name).map_or_else(|| name.to_owned(), |(id, n)| format!("{id} {n}"))
}

/// Counts by status: (implemented, approximate, no-op, unimplemented), for functions and methods.
pub fn tally(rows: &[Row]) -> [[usize; 4]; 2] {
    let mut t = [[0usize; 4]; 2];
    for r in rows {
        let s = match r.status {
            Status::Implemented => 0,
            Status::Approximate => 1,
            Status::NoOp => 2,
            Status::Unimplemented => 3,
        };
        t[usize::from(r.method)][s] += 1;
    }
    t
}

/// A Markdown table of the census.
pub fn markdown(rows: &[Row], title: &str) -> String {
    use std::fmt::Write;
    let t = tally(rows);
    let mut s = String::new();
    let _ = writeln!(s, "# {title}\n");
    let _ = writeln!(
        s,
        "{} builtin ids called. Functions: {} implemented, {} approximate, {} no-op, {} unimplemented. Methods: {} implemented, {} approximate, {} no-op, {} unimplemented.\n",
        rows.len(),
        t[0][0], t[0][1], t[0][2], t[0][3], t[1][0], t[1][1], t[1][2], t[1][3]
    );
    let _ = writeln!(s, "| id | kind | name | conf | status | call sites | scripts |\n|---|---|---|---|---|---|---|");
    for r in rows {
        let conf = match r.confidence {
            Some(Confidence::Verified) => "V",
            Some(Confidence::Likely) => "L",
            Some(Confidence::Guess) => "G",
            None => "-",
        };
        let mut scripts: Vec<&str> = r.scripts.iter().map(String::as_str).take(4).collect();
        if r.scripts.len() > 4 {
            scripts.push("...");
        }
        let _ = writeln!(
            s,
            "| {:#06x} | {} | {} | {} | {} | {} | {} |",
            r.id,
            if r.method { "method" } else { "function" },
            r.name.unwrap_or("(unnamed)"),
            conf,
            r.status.label(),
            r.calls,
            scripts.join(", ")
        );
    }
    s
}

//! Every compiled script of Shadows of Evil's zones decodes, and every call
//! links: to an export of the loaded scripts or to an engine builtin.
//!
//! Reads the retail zones from `T7_ZONES` (a folder with `core_patch.ff`,
//! `zm_patch.ff`, `zm_zod_patch.ff`), else `/mnt/user-data/uploads/zone`;
//! skipped (passes) when they are not there.

mod common;

use std::collections::{HashMap, HashSet};

use common::{objects, zone_dir};
use gsc_t7::{Insn, T7Object, names};

#[test]
fn shadows_of_evil_scripts_decode_and_link() {
    let Some(dir) = zone_dir() else {
        eprintln!("no T7 zones; skipped");
        return;
    };
    let mut all: Vec<T7Object> = Vec::new();
    let mut unread = 0;
    for zone in ["core_patch", "zm_patch", "zm_zod_patch"] {
        let z = fastfile_t7::Zone::open(&dir.join(format!("{zone}.ff"))).expect("zone");
        let mut n = 0;
        for bytes in objects(&z.image) {
            let o = T7Object::parse(bytes).unwrap_or_else(|e| panic!("{zone}: {e}"));
            if o.string_refs.0 != o.string_refs.1 || o.import_sites.0 != o.import_sites.1 {
                // (A table entry no reachable instruction reads.)
                eprintln!("{}: string fixups {:?}, import sites {:?} (listed, read)", o.name, o.string_refs, o.import_sites);
                unread += 1;
            }
            n += 1;
            all.push(o);
        }
        eprintln!("{zone}: {n} objects");
    }
    assert!(unread <= 8, "{unread} objects with table entries no instruction reads");
    // Later zones replace earlier objects of the same name (the patch
    // order); server scripts only.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (i, o) in all.iter().enumerate() {
        by_name.insert(o.name.clone(), i);
    }
    let server: Vec<&T7Object> = by_name.values().map(|&i| &all[i]).filter(|o| o.name.ends_with(".gsc")).collect();
    assert!(server.len() >= 450, "{} server scripts", server.len());
    let exports: HashSet<(u32, u32)> = server.iter().flat_map(|o| o.functions.iter().map(|f| (f.export.ns, f.export.name))).collect();
    let loaded_ns: HashSet<u32> = server.iter().flat_map(|o| o.functions.iter().map(|f| f.export.ns)).collect();
    // Builtins the scripts call whose records the exe builds in a way the
    // table reader does not see (their hashes are not immediates).
    let not_in_tables: HashSet<u32> = ["shellshock", "obituary"].iter().map(|n| gsc_t7::canon(n)).chain([0x769d_b7b8]).collect();
    let mut other_mode = 0;
    let dev_only: HashSet<u32> = ["assert", "assertmsg", "println", "iprintln", "iprintlnbold", "errormsg", "array"].iter().map(|n| gsc_t7::canon(n)).collect();
    let mut unresolved: HashMap<String, usize> = HashMap::new();
    let mut builtins = HashSet::new();
    let mut ops: HashMap<String, usize> = HashMap::new();
    for o in &server {
        for f in &o.functions {
            for (_, insn) in &f.code {
                let key = match insn {
                    Insn::Plain(op) | Insn::Int(op, _) | Insn::Str(op, _) | Insn::Field(op, _) | Insn::Call(op, ..) | Insn::Method(op, ..) | Insn::Jump(op, _) => format!("{op:?}"),
                    other => format!("{:?}", std::mem::discriminant(other)),
                };
                *ops.entry(key).or_default() += 1;
                let Insn::Call(_, _, i) = insn else { continue };
                let imp = &o.imports[*i as usize];
                if exports.contains(&(imp.ns, imp.name)) {
                    continue;
                }
                if names::builtin(imp.name).is_some() {
                    builtins.insert(imp.name);
                    continue;
                }
                if imp.developer() || dev_only.contains(&imp.name) || not_in_tables.contains(&imp.name) {
                    continue;
                }
                if !loaded_ns.contains(&imp.ns) || imp.flags & 0x20 == 0 {
                    // A namespaced call (flag 0x20 clear) into another
                    // mode's script (multiplayer's `objectives` and
                    // `globallogic_ui` functions from the quad tank).
                    other_mode += 1;
                    continue;
                }
                *unresolved.entry(format!("{} {}::{} (flags {:#x})", o.name, names::display(imp.ns), names::display(imp.name), imp.flags)).or_default() += 1;
            }
        }
    }
    let mut ops: Vec<_> = ops.into_iter().collect();
    ops.sort_by_key(|o| std::cmp::Reverse(o.1));
    eprintln!("instructions by kind: {ops:?}");
    let named = builtins.iter().filter(|h| names::builtin(**h).is_some_and(|b| !b.name.is_empty())).count();
    eprintln!("{} builtins called, {named} named; {other_mode} calls into scripts of other modes", builtins.len());
    assert!(other_mode <= 16, "{other_mode} calls into scripts not loaded");
    let mut u: Vec<_> = unresolved.into_iter().collect();
    u.sort();
    assert!(u.is_empty(), "{} unresolved calls: {:#?}", u.len(), &u[..u.len().min(40)]);
}

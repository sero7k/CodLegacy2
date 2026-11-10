//! The effects ("vfx") a zone holds: `jup_vfx <zone image | .ff> [--install <dir>] [--from n] [--to n] [--named]`.
//!
//! A `.zone` file is a decompressed zone image (any tool's); a `.ff` needs
//! the install for its Oodle DLL (Windows). Prints how many effects the
//! scan found, the layout check (effects with one all-shared emitter whose
//! computed end is the next effect's start), the module ids used, and with
//! `--named` the effects whose path the zone spells (`vfx\...\x.vfx`
//! strings of scriptables) with their emitters and modules; `--refs` lists
//! where else the zone mentions an effect's hash, with any coordinates beside it.
use std::collections::BTreeMap;

use fastfile_jup::vfx::{self, Storage};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut path = None;
    let (mut install, mut from, mut to, mut named, mut refs) = (None, 0usize, usize::MAX, false, false);
    let mut it = a.iter().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--install" => install = it.next().cloned(),
            "--from" => from = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--to" => to = it.next().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX),
            "--named" => named = true,
            "--refs" => refs = true,
            _ => path = Some(x.clone()),
        }
    }
    let path = path.expect("zone image or .ff");
    let z = if path.ends_with(".ff") {
        let oodle = fastfile_jup::oodle::load(std::path::Path::new(install.as_deref().expect("--install"))).expect("oodle");
        fastfile_jup::map::read_image(std::path::Path::new(&path), oodle.as_ref()).expect("zone").bytes
    } else {
        std::fs::read(&path).expect("read")
    };
    let all = vfx::scan(&z, from, to);
    println!("{} effects in {} bytes", all.len(), z.len());
    let mut by_emitters: BTreeMap<u32, usize> = BTreeMap::new();
    let mut ids: BTreeMap<u16, [usize; 4]> = BTreeMap::new();
    let (mut checked, mut exact) = (0, 0);
    for (k, v) in all.iter().enumerate() {
        *by_emitters.entry(v.emitters).or_default() += 1;
        for m in v.modules.iter().chain(&v.params) {
            let c = ids.entry(m.id).or_default();
            c[match m.storage {
                Storage::Inline => 0,
                Storage::InlineShared => 1,
                Storage::Back(_) => 2,
                Storage::Other(_) => 3,
            }] += 1;
        }
        if let (Some(len), Some(next)) = (v.extent(), all.get(k + 1)) {
            if next.at > v.at {
                checked += 1;
                exact += usize::from(v.at + len == next.at);
            }
        }
    }
    println!("emitter counts: {by_emitters:?}");
    println!("layout check: {exact} of {checked} one-emitter all-shared effects end exactly at the next effect");
    println!("entry ids [inline, inline-shared, back-ref, other]:");
    for (id, c) in &ids {
        println!("  {id:3} {c:?}");
    }
    if refs {
        // Where else the zone mentions an effect it holds: the map's own
        // placements, if it has them as data (JUP.md, "Where a map places
        // its effects"). Listed with the coordinates found beside each.
        let hashes: std::collections::HashSet<u64> = all.iter().map(|v| v.hash).collect();
        let own: std::collections::HashSet<usize> = all.iter().map(|v| v.at).collect();
        let found = vfx::references(&z, &hashes, &own);
        let with_origin = found.iter().filter(|r| r.before.is_some() || r.after.is_some()).count();
        println!("{} mentions of {} effects outside their headers, {with_origin} with coordinates beside them", found.len(), hashes.len());
        for r in found.iter().take(400) {
            println!("  {:#010x} hash {:016x} before {:?} after {:?}", r.at, r.hash, r.before, r.after);
        }
    }
    if named {
        let by_hash: BTreeMap<u64, &vfx::Vfx> = all.iter().map(|v| (v.hash, v)).collect();
        let mut at = 0;
        while let Some(p) = find(&z, b".vfx\0", at) {
            at = p + 1;
            let s = (0..p).rev().take(200).take_while(|&i| z[i] >= 0x20 && z[i] < 0x7f).last().map_or(p, |i| p - (p - i));
            let Ok(text) = std::str::from_utf8(&z[s..p + 4]) else { continue };
            if !text.starts_with("vfx") {
                continue;
            }
            match by_hash.get(&vfx::path_hash(text)) {
                Some(v) => println!(
                    "{text}: emitters {} flags {:#x} modules {:?} params {:?}",
                    v.emitters,
                    v.flags,
                    v.modules.iter().map(|m| m.id).collect::<Vec<_>>(),
                    v.params.iter().map(|m| m.id).collect::<Vec<_>>()
                ),
                None => println!("{text}: not in this zone's scan"),
            }
        }
    }
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    h.get(from..)?.windows(n.len()).position(|w| w == n).map(|p| p + from)
}

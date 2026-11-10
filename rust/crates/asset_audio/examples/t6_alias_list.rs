//! Lists a Black Ops II zone's sound aliases with their file names and bank
//! entry ids, and (given streamed/loaded banks) each entry's length:
//! `t6_alias_list <zone.ff> [bank.sabs|bank.sabl ...] [word ...]`.
//! With banks and no words, also lists the banks' entries no alias in the
//! zone names, longest first. `T6_ALIAS_FULL=1` prints every field of
//! each listed alias.
use std::collections::{HashMap, HashSet};

fn main() {
    let mut zone = None;
    let mut banks = Vec::new();
    let mut words = Vec::new();
    for a in std::env::args().skip(1) {
        let lower = a.to_ascii_lowercase();
        if zone.is_none() {
            zone = Some(a);
        } else if lower.ends_with(".sabs") || lower.ends_with(".sabl") {
            banks.push(sab_t6::BankFile::open(std::path::Path::new(&a)).expect("open bank"));
        } else {
            words.push(lower);
        }
    }
    let zone = zone.expect("zone.ff");
    let cap = asset_t6::capture_zone(std::path::Path::new(&zone)).expect("read zone");
    let entries: HashMap<u32, (String, sab_t6::BankEntry)> = banks
        .iter()
        .flat_map(|b| {
            let name = b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            b.entries.iter().map(move |e| (e.id, (name.clone(), *e)))
        })
        .collect();
    let mut named = HashSet::new();
    for bank in &cap.sound_banks {
        println!("bank {}: {} alias lists", bank.name, bank.aliases.len());
        for list in &bank.aliases {
            for a in &list.entries {
                named.insert(a.asset_id);
                let name = a.name.to_ascii_lowercase();
                if !words.is_empty() && !words.iter().any(|w| name.contains(w.as_str()) || a.asset_file.to_ascii_lowercase().contains(w.as_str())) {
                    continue;
                }
                let len = entries.get(&a.asset_id).map_or_else(String::new, |(b, e)| {
                    format!(" — {b} {:.1} s{}", f64::from(e.frame_count) / f64::from(e.rate()), if e.looping { " loop" } else { "" })
                });
                println!("  {} file `{}` id {:08x}{len}", a.name, a.asset_file, a.asset_id);
                if std::env::var_os("T6_ALIAS_FULL").is_some() {
                    println!("    {a:?}");
                }
            }
        }
    }
    if words.is_empty() && !banks.is_empty() {
        let mut rest: Vec<_> = entries.iter().filter(|(id, _)| !named.contains(id)).collect();
        rest.sort_by(|a, b| b.1.1.frame_count.cmp(&a.1.1.frame_count));
        for (id, (b, e)) in rest {
            println!("unnamed {id:08x} {b} {:.1} s", f64::from(e.frame_count) / f64::from(e.rate()));
        }
    }
}

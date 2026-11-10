//! Per map: how many material pictures there are and where each resolves
//! (zone, streamed part, download package, nowhere), from the zone indexes
//! alone (no `.xsub` data needed).
//!
//! `jup_map_coverage <install> [--listing file_list.txt] [--plan out.tsv]
//!   [--misses] [--max-side N] [--meshes] [--skip index.tsv]... <map zone name>...`
//!
//! `--plan` writes one row per wanted picture group found in a local part:
//! `pack<TAB>part<TAB>offset<TAB>size<TAB>key<TAB>map` (the byte ranges to cut
//! out of the install's parts). `--max-side` keeps only groups of pictures
//! whose best level is at most that many pixels (default 2048; 0 for all).
//! `--meshes` adds the streamed meshes of every LOD of every placed model.
//! `--skip` leaves out keys an earlier extraction's `index.tsv` already holds
//! (repeatable), so a new pack carries only what is missing.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use fastfile_jup::map::MapSources;
use fastfile_jup::map::coverage::{self, MapCoverage, PackIndex, Source};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let Some(install) = it.next().map(PathBuf::from) else {
        eprintln!("usage: jup_map_coverage <install> [--listing f] [--plan f] [--misses] <map>...");
        std::process::exit(2);
    };
    let (mut listing, mut plan, mut misses, mut meshes) = (None, None, false, false);
    let mut skip: std::collections::HashSet<u64> = Default::default();
    let mut max_side = 2048u32;
    let mut maps = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--listing" => listing = it.next().cloned(),
            "--plan" => plan = it.next().cloned(),
            "--misses" => misses = true,
            "--max-side" => max_side = it.next().and_then(|v| v.parse().ok()).map_or(2048, |v: u32| if v == 0 { u32::MAX } else { v }),
            "--meshes" => meshes = true,
            "--skip" => {
                if let Some(t) = it.next().and_then(|f| std::fs::read_to_string(f).ok()) {
                    skip.extend(t.lines().filter_map(|l| u64::from_str_radix(l.split('\t').next()?.trim(), 16).ok()));
                }
            }
            m => maps.push(m.to_owned()),
        }
    }
    let listing = listing
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| coverage::read_listing(&t))
        .unwrap_or_default();
    let idx = PackIndex::open(&install, &listing);
    println!(
        "indexes: {}",
        idx.packs.iter().map(|p| format!("{}{}({} rows, {} parts)", p.name, if p.cdn { "*" } else { "" }, p.entries.len(), p.part_count)).collect::<Vec<_>>().join(" ")
    );
    let oodle = fastfile_jup::oodle::load(&install).expect("oodle");
    let mut plan_rows: BTreeMap<(String, usize, u64), (u32, u64, String)> = BTreeMap::new();
    for map in &maps {
        let src = MapSources { install: install.clone(), name: map.clone() };
        let t = std::time::Instant::now();
        match coverage::analyse(&src, oodle.as_ref(), &idx) {
            Ok(c) => {
                print_map(&c, misses);
                println!("  ({:.1}s)", t.elapsed().as_secs_f32());
                // The wanted groups: every group of every picture that has a local entry.
                for r in c.pictures.iter().chain(c.others.iter().filter(|r| matches!(r.usage, 0x3a | 0x2b))) {
                    for (key, l, side) in &r.local_groups {
                        if *side <= max_side && !skip.contains(key) {
                            plan_rows.insert((l.pack.clone(), l.part, l.offset), (l.size, *key, c.name.clone()));
                        }
                    }
                }
            }
            Err(e) => println!("{map}: error: {e}"),
        }
        if meshes {
            match coverage::mesh_keys(&src, oodle.as_ref()) {
                Ok(keys) => {
                    let (mut local, mut cdn, mut none, mut skipped) = (0, 0, 0, 0);
                    for (key, _len) in &keys {
                        if skip.contains(key) {
                            skipped += 1;
                            continue;
                        }
                        match idx.locate(*key) {
                            Some(l) if !l.cdn => {
                                local += 1;
                                plan_rows.insert((l.pack.clone(), l.part, l.offset), (l.size, *key, map.clone()));
                            }
                            Some(_) => cdn += 1,
                            None => none += 1,
                        }
                    }
                    println!("  meshes: {} streamed keys: {local} local, {cdn} in a download index, {none} in no index, {skipped} already extracted", keys.len());
                }
                Err(e) => println!("  meshes: error: {e}"),
            }
        }
    }
    if let Some(p) = plan {
        let mut f = std::io::BufWriter::new(std::fs::File::create(p).expect("plan"));
        let mut total = 0u64;
        for ((pack, part, off), (size, key, _map)) in &plan_rows {
            writeln!(f, "{pack}-{part:05}.xsub\t{off}\t{size}\t{key:016x}").ok();
            total += u64::from(*size);
        }
        println!("plan: {} entries, {:.1} MiB", plan_rows.len(), total as f64 / 1048576.0);
    }
}

fn print_map(c: &MapCoverage, misses: bool) {
    println!("== {}", c.name);
    for l in c.report.iter().filter(|l| l.starts_with("materials:")) {
        println!("  {l}");
    }
    println!(
        "  materials {} (no header {}, no pictures {}, no colour picture {})",
        c.materials, c.materials_without_header, c.materials_without_pictures, c.materials_without_colour
    );
    let line = |label: &str, rows: &[coverage::PictureRow]| {
        let t = MapCoverage::tally(rows);
        let parts: Vec<String> = Source::ALL.iter().map(|s| format!("{} {}", s.label(), t.get(s).copied().unwrap_or(0))).collect();
        println!("  {label}: {} pictures: {}", rows.len(), parts.join(", "));
    };
    line("material pictures", &c.pictures);
    line("other pictures", &c.others);
    let mut by_usage: BTreeMap<u8, BTreeMap<Source, usize>> = BTreeMap::new();
    for r in &c.pictures {
        *by_usage.entry(r.usage).or_default().entry(r.source).or_default() += 1;
    }
    for (u, m) in by_usage {
        println!("    usage {u:#04x}: {}", m.iter().map(|(s, n)| format!("{} {n}", s.label())).collect::<Vec<_>>().join(", "));
    }
    let mut sides: BTreeMap<u32, usize> = BTreeMap::new();
    for r in c.pictures.iter().filter(|r| r.source == Source::Xsub) {
        *sides.entry(r.best_side).or_default() += 1;
    }
    println!("    best side of the xsub ones: {sides:?}");
    let mut fm: BTreeMap<(u8, Source), usize> = BTreeMap::new();
    for r in c.pictures.iter().filter(|r| !matches!(r.source, Source::Xsub | Source::Inline)) {
        *fm.entry((r.format, r.source)).or_default() += 1;
    }
    if !fm.is_empty() {
        println!("    not at full size by (format, source): {fm:x?}");
    }
    let mut tops: BTreeMap<&str, usize> = BTreeMap::new();
    for r in c.pictures.iter().filter(|r| r.source == Source::XsubPartial) {
        *tops.entry(match r.top_cdn { Some(true) => "top group in a download index", Some(false) => "top group local", None => "top group in no index" }).or_default() += 1;
    }
    if !tops.is_empty() {
        println!("    partial: {tops:?}");
    }
    if misses {
        for r in c.pictures.iter().filter(|r| !matches!(r.source, Source::Xsub | Source::Inline)) {
            println!(
                "    miss {:016x} {}x{} fmt {:#x} usage {:#x} {} groups local/cdn/none {:?}",
                r.hash, r.width, r.height, r.format, r.usage, r.source.label(), r.groups
            );
        }
    }
}

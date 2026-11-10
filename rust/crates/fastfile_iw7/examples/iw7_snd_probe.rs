//! probe: where IW7 gun sound aliases' clips live.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use fastfile_iw7::sab::{SabFile, bank_paths};
use fastfile_iw7::walk::{Capture, walk_zone};

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[1]);
    let mode = args.get(2).map(String::as_str).unwrap_or("sfx");
    if mode == "file" {
        let b = std::fs::read(&args[3]).unwrap();
        println!("{:?}", decode(&b).map(|(r, c, p, e)| (r, c, p.len(), e)));
        let e = decode_err(&b);
        println!("{e}");
        return;
    }
    if mode == "strings" {
        for z in &args[3..] {
            let Some(p) = fastfile_iw7::zone_path(dir, z) else { continue };
            let zone = fastfile_iw7::Zone::open(&p).unwrap();
            let mut cur = Vec::new();
            let mut out = std::collections::BTreeSet::new();
            for &b in zone.image.iter().chain(std::iter::once(&0u8)) {
                if b.is_ascii_graphic() {
                    cur.push(b);
                } else {
                    if cur.len() >= 4 {
                        out.insert(String::from_utf8_lossy(&cur).into_owned());
                    }
                    cur.clear();
                }
            }
            for s in zone.assets.script_strings.iter().flatten() {
                out.insert(s.clone());
            }
            for s in out {
                println!("{z}\t{s}");
            }
        }
        return;
    }
    let zones = ["patch_common_mp", "patch_common_core_mp", "common_core_mp", "common_mp", "global_core_mp"];
    let mut caps: Vec<(String, Capture, fastfile_iw7::Zone)> = Vec::new();
    for z in zones {
        let Some(p) = fastfile_iw7::zone_path(dir, z) else { continue };
        let zone = fastfile_iw7::Zone::open(&p).unwrap();
        let (mut cap, stop) = walk_zone(&zone);
        if let Some(s) = &stop
            && let Some(b) = fastfile_iw7::walk::recover_sound_bank(&zone, s)
        {
            cap.sound_banks.push(b);
        }
        eprintln!("{z}: banks {:?}", cap.sound_banks.iter().map(|b| (&b.name, b.aliases.len())).collect::<Vec<_>>());
        caps.push((z.to_owned(), cap, zone));
    }
    let mut banks: Vec<(String, SabFile)> = Vec::new();
    for z in ["patch_common_mp", "patch_common_core_mp", "common_core_mp", "common_mp"] {
        for p in bank_paths(dir, z) {
            let b = SabFile::open(&p).unwrap();
            eprintln!("bank {}: {} entries", p.display(), b.entries.len());
            banks.push((p.file_name().unwrap().to_string_lossy().into_owned(), b));
        }
    }
    // alias name -> (zone, records), patch first.
    let mut aliases: HashMap<String, (String, Vec<Vec<u8>>, Vec<Option<String>>)> = HashMap::new();
    let mut by_id: HashMap<u32, String> = HashMap::new();
    for (z, cap, _) in &caps {
        for b in &cap.sound_banks {
            for a in &b.aliases {
                let recs: Vec<Vec<u8>> = a.records.iter().map(|r| r.1.clone()).collect();
                for r in &recs {
                    by_id.entry(u32_at(r, 40)).or_insert(a.name.clone());
                }
                let files = a.entries.iter().map(|e| e.0.clone()).collect();
                aliases.entry(a.name.clone()).or_insert((format!("{z}/{}", b.name), recs, files));
            }
        }
    }
    let where_is = |id: u32| -> Option<String> {
        banks.iter().find(|(_, b)| b.find(id).is_some()).map(|(n, b)| {
            let e = b.find(id).unwrap();
            format!("{n} fmt{} {}Hz ch{} {:.2}s", e.format, e.rate, e.channels, f64::from(e.frames) / f64::from(e.rate.max(1)))
        })
    };
    if mode == "decode" {
        let want = args.get(3).cloned().unwrap_or_else(|| "weap_".into());
        let cap: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(400);
        let mut seen = BTreeSet::new();
        let (mut ok, mut bad, mut samples_off) = (0, 0, 0);
        let mut names: Vec<&String> = aliases.keys().filter(|n| n.starts_with(want.as_str())).collect();
        names.sort();
        for n in names {
            for r in &aliases[n].1 {
                let id = u32_at(r, 52);
                if id == 0 || !seen.insert(id) || ok + bad >= cap {
                    continue;
                }
                let Some((bn, b)) = banks.iter().find(|(_, b)| b.find(id).is_some()) else { continue };
                let e = *b.find(id).unwrap();
                let (off, len) = e.frames_range();
                let bytes = fastfile_iw7::sab::read_flac(&b.path, off, len, u64::from(e.frames), e.rate).unwrap();
                match decode(&bytes) {
                    Some((rate, ch, pcm, errs)) if !pcm.is_empty() => {
                        ok += 1;
                        let frames = pcm.len() / ch.max(1);
                        if frames as u64 != u64::from(e.frames) {
                            samples_off += 1;
                        }
                        let rms = (pcm.iter().map(|x| x * x).sum::<f32>() / pcm.len() as f32).sqrt();
                        let peak = pcm.iter().fold(0f32, |m, x| m.max(x.abs()));
                        let diff: f32 = (ch..pcm.len()).map(|i| (pcm[i] - pcm[i - ch]).powi(2)).sum::<f32>() / pcm.len() as f32;
                        println!(
                            "{n} {id:08x} [{bn}] {rate} Hz {ch} ch {:.3} s ({frames} / {} frames) rms {:.1} dBFS peak {:.1} dBFS hf {:.3} errs {errs}",
                            frames as f64 / f64::from(rate),
                            e.frames,
                            20.0 * rms.log10(),
                            20.0 * peak.log10(),
                            diff.sqrt() / rms.max(1e-9)
                        );
                        if let Some(d) = args.get(5) {
                            std::fs::write(format!("{d}/{id:08x}.flac"), &bytes).ok();
                        }
                    }
                    _ => {
                        bad += 1;
                        println!("{n} {id:08x} [{bn}] DOES NOT DECODE");
                    }
                }
            }
        }
        println!("decoded {ok}, failed {bad}, frame count off {samples_off}");
        return;
    }
    if mode == "weapons" {
        let mut names = BTreeSet::new();
        for (z, cap, _) in &caps {
            for w in &cap.weapons {
                names.insert(format!("{} [{z}{}]", w.name, if w.def.is_some() { "" } else { " placeholder" }));
            }
        }
        for n in names {
            println!("{n}");
        }
        return;
    }
    if mode == "alias" {
        for a in &args[3..] {
            show(a, &aliases, &by_id, &where_is, 0);
        }
        return;
    }
    if mode == "notes" {
        for (z, cap, zone) in &caps {
            for x in &cap.xanims {
                if args[3..].iter().any(|a| x.name.trim_start_matches(',') == a) && !x.notify.is_empty() {
                    let s = &zone.assets.script_strings;
                    let n: Vec<String> = x.notify.iter().map(|&(i, t)| format!("{}@{t:.2}", s.get(i as usize).cloned().flatten().unwrap_or_default())).collect();
                    println!("{} [{z}]: {n:?}", x.name);
                }
            }
        }
        return;
    }
    // sfx: every iw7 sfx package's aliases and their clips.
    let mut stats: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for (z, cap, _) in &caps {
        for p in &cap.sfx_packages {
            if !p.name.trim_start_matches(',').starts_with("iw7_") || p.sounds.is_empty() {
                continue;
            }
            println!("{} [{z}]", p.name);
            for (o, a) in &p.sounds {
                print!("  +{o}: ");
                let s = stats.entry(p.name.clone()).or_default();
                let (found, total) = show(a, &aliases, &by_id, &where_is, 2);
                s.0 += 1;
                s.1 += found;
                s.2 += total;
            }
        }
    }
    for (k, v) in stats {
        println!("{k}: {} aliases, clips {}/{}", v.0, v.1, v.2);
    }
}

type Aliases = HashMap<String, (String, Vec<Vec<u8>>, Vec<Option<String>>)>;

fn show(a: &str, aliases: &Aliases, by_id: &HashMap<u32, String>, where_is: &dyn Fn(u32) -> Option<String>, depth: usize) -> (usize, usize) {
    let Some((zb, recs, files)) = aliases.get(a) else {
        println!("{:depth$}{a}: NOT IN BANKS", "");
        return (0, 1);
    };
    let (mut found, mut total) = (0, 0);
    println!("{:depth$}{a} [{zb}] {} records", "", recs.len());
    let mut secs = BTreeSet::new();
    for (r, f) in recs.iter().zip(files) {
        let id = u32_at(r, 52);
        let sec = u32_at(r, 44);
        let ty = (u32_at(r, 108) >> 7) & 3;
        let vol = (f32::from_le_bytes(r[72..76].try_into().unwrap()), f32::from_le_bytes(r[76..80].try_into().unwrap()));
        let dist = (f32::from_le_bytes(r[96..100].try_into().unwrap()), f32::from_le_bytes(r[100..104].try_into().unwrap()));
        let ctx = (u32_at(r, 56), u32_at(r, 60));
        let w = where_is(id);
        total += 1;
        found += usize::from(w.is_some());
        println!(
            "{:depth$}   id {id:08x} ty{ty} vol {vol:?} dist {dist:?} ctx {ctx:x?} sec {sec:08x} {} file {} -> {}",
            "",
            by_id.get(&sec).cloned().unwrap_or_default(),
            f.clone().unwrap_or_default(),
            w.unwrap_or_else(|| "NOT STAGED".into())
        );
        if sec != 0
            && let Some(n) = by_id.get(&sec)
        {
            secs.insert(n.clone());
        }
    }
    if depth < 8 {
        for s in secs {
            if s != a {
                let (f, t) = show(&s, aliases, by_id, where_is, depth + 2);
                found += f;
                total += t;
            }
        }
    }
    (found, total)
}

fn decode(bytes: &[u8]) -> Option<(u32, usize, Vec<f32>, usize)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let probed = symphonia::default::get_probe().format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL)?;
    let id = track.id;
    let rate = track.codec_params.sample_rate?;
    let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let (mut out, mut ch, mut errs) = (Vec::new(), 0usize, 0usize);
    while let Ok(p) = format.next_packet() {
        if p.track_id() != id {
            continue;
        }
        match dec.decode(&p) {
            Ok(d) => {
                ch = ch.max(d.spec().channels.count());
                let mut b = SampleBuffer::<f32>::new(d.capacity() as u64, *d.spec());
                b.copy_interleaved_ref(d);
                out.extend_from_slice(b.samples());
            }
            Err(_) => errs += 1,
        }
    }
    Some((rate, ch, out, errs))
}

fn decode_err(bytes: &[u8]) -> String {
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let probed = match symphonia::default::get_probe().format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default()) {
        Ok(p) => p,
        Err(e) => return format!("probe: {e}"),
    };
    let mut format = probed.format;
    let Some(track) = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL) else { return "no track".into() };
    let id = track.id;
    let mut dec = match symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()) {
        Ok(d) => d,
        Err(e) => return format!("codec: {e}"),
    };
    let mut n = 0;
    loop {
        match format.next_packet() {
            Ok(p) => {
                if p.track_id() != id { continue; }
                if let Err(e) = dec.decode(&p) { return format!("packet {n}: {e}"); }
                n += 1;
            }
            Err(e) => return format!("after {n} packets: {e}"),
        }
    }
}

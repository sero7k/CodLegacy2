//! A map's client triggers: `iw7_client_triggers <map.ff>` prints their
//! count, types, audio triggers and states, the string table, and per
//! trigger its origin, hull count and box.
fn main() {
    let path = std::env::args().nth(1).expect("usage: iw7_client_triggers <map.ff>");
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&path)).expect("open");
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    let img = &zone.image;
    let u16s = |s: Option<fastfile_iw7::walk::Span>| s.map(|s| img[s.pos..s.pos + s.len].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap_or_default();
    let f32_at = |at: usize| f32::from_le_bytes(img[at..at + 4].try_into().unwrap());
    for m in &w.cap.map_ents {
        let c = &m.client_triggers;
        println!("{} client triggers", c.count);
        let types: Vec<u8> = c.types.map(|s| img[s.pos..s.pos + s.len].to_vec()).unwrap_or_default();
        let audio = u16s(c.audio);
        let states = u16s(c.audio_states);
        let mut hist = std::collections::BTreeMap::new();
        for &t in &types {
            *hist.entry(t).or_insert(0) += 1;
        }
        println!("types {hist:?}");
        if let Some(s) = c.strings {
            let raw = &img[s.pos..s.pos + s.len];
            let parts: Vec<(usize, String)> = {
                let mut out = Vec::new();
                let mut start = 0;
                for (i, &b) in raw.iter().enumerate() {
                    if b == 0 {
                        if i > start {
                            out.push((start, String::from_utf8_lossy(&raw[start..i]).into_owned()));
                        }
                        start = i + 1;
                    }
                }
                out
            };
            println!("strings ({} bytes): {:?}", s.len, parts.iter().take(60).collect::<Vec<_>>());
        }
        let model = |i: usize| c.models.map(|s| {
            let at = s.pos + 32 * i;
            (u16::from_le_bytes([img[at + 4], img[at + 5]]), u16::from_le_bytes([img[at + 6], img[at + 7]]))
        });
        for i in 0..c.count as usize {
            let o = c.origins.map(|s| [f32_at(s.pos + 12 * i), f32_at(s.pos + 12 * i + 4), f32_at(s.pos + 12 * i + 8)]);
            let (hc, first) = model(i).unwrap_or((0, 0));
            let bx = c.hulls.map(|s| {
                let at = s.pos + 32 * first as usize;
                ([f32_at(at), f32_at(at + 4), f32_at(at + 8)], [f32_at(at + 12), f32_at(at + 16), f32_at(at + 20)])
            });
            if i < 40 || types.get(i).is_some_and(|&t| t != types[0]) && i < 400 {
                println!("  #{i} type {:?} audio {:?} state {:?} origin {o:?} hulls {hc} first {first} box {bx:?}", types.get(i), audio.get(i), states.get(i));
            }
        }
    }
}

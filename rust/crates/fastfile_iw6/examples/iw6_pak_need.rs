//! Which `imagefile<N>.pak`s a Ghosts map's images stream from, with the
//! bytes it reads of each: `iw6_pak_need <mp_map.ff>...`
//! (`IW6_PAK_MAX=<px>` counts only the parts up to that size, default 512).
use std::collections::BTreeMap;

fn main() {
    let max: u32 = std::env::var("IW6_PAK_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
    for path in std::env::args().skip(1) {
        let zone = match fastfile_iw6::Zone::open(std::path::Path::new(&path)) {
            Ok(z) => z,
            Err(e) => {
                println!("{path}: open: {e}");
                continue;
            }
        };
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let mut per: BTreeMap<u16, (usize, u64, u64)> = BTreeMap::new();
        let (mut streamed, mut inline, mut refs) = (0, 0, 0);
        for rec in &cap.images {
            if fastfile_iw6::shared::is_reference(rec) {
                refs += 1;
                continue;
            }
            if !rec.streamed {
                inline += 1;
                continue;
            }
            streamed += 1;
            for i in 0..4 {
                let Some(f) = rec.stream_files[i] else { continue };
                if rec.streams[i].0 == 0 {
                    continue;
                }
                let fits = u32::from(rec.streams[i].0.max(rec.streams[i].1)) <= max || i == 0;
                let e = per.entry(f.file_index).or_default();
                e.0 += 1;
                let b = f.offset_end.saturating_sub(f.offset);
                e.1 += b;
                if fits {
                    e.2 += b;
                }
            }
        }
        if std::env::var_os("IW6_PAK_READ").is_some() {
            read_check(&cap, std::path::Path::new(&path), max);
        }
        println!("{path}: {} images: {streamed} streamed, {inline} inline, {refs} references", cap.images.len());
        for (pak, (n, all, fit)) in per {
            println!("  imagefile{pak}.pak: {n} parts, {all} bytes (<= {max}px parts: {fit})");
        }
    }
}

/// `IW6_PAK_READ=1`: reads every streamed image the way the lane does
/// (`image_pixels`, at most `max` px) from the paks beside the zone and
/// counts, per pak of its part 0, the outcomes and the failure reasons.
fn read_check(cap: &fastfile_iw6::walk::Capture, zone: &std::path::Path, max: u32) {
    let image = fastfile_iw6::Zone::open(zone).unwrap().image;
    let mut paks = fastfile_iw6::image::ImagePaks::new(zone.parent().unwrap());
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for rec in &cap.images {
        if !rec.streamed || fastfile_iw6::shared::is_reference(rec) {
            continue;
        }
        let pak = rec.stream_files[0].map_or(0, |f| f.file_index);
        let all_here = (0..4).filter(|&i| rec.streams[i].0 > 0).all(|i| {
            rec.stream_files[i].is_none_or(|f| zone.parent().unwrap().join(format!("imagefile{}.pak", f.file_index)).is_file())
        });
        let key = match fastfile_iw6::image::image_pixels(rec, &image, &mut paks, max) {
            Ok(px) => {
                // The whole chain: every level's bytes, down to 1x1.
                let (mut w, mut h, mut want) = (px.width, px.height, 0usize);
                for _ in 0..px.levels {
                    want += fastfile_iw6::image::level_bytes(px.dxgi_format, w, h).unwrap_or(0);
                    w = (w / 2).max(1);
                    h = (h / 2).max(1);
                }
                let chain = if want == px.data.len() { "" } else { " CHAIN-MISMATCH" };
                let blocks = if matches!(px.dxgi_format, 70..=84 | 94..=99) && (px.width % 4 != 0 || px.height % 4 != 0) { " NOT-4x4" } else { "" };
                format!("pak{pak} ok f{}{chain}{blocks}", rec.format)
            }
            Err(e) if e.contains("not read from") && !all_here => format!("pak{pak} other pak absent"),
            Err(e) => format!("pak{pak} FAIL {}", e.split(" from ").next().unwrap_or(&e)),
        };
        *out.entry(key).or_default() += 1;
    }
    for (k, n) in out {
        println!("  read {k}: {n}");
    }
}

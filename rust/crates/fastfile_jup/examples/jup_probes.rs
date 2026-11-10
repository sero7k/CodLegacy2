//! Lists a Modern Warfare III map's reflection probes and light grids, and
//! writes each readable probe's picture (octahedral, tonemapped) as a PNG:
//! `jup_probes <map zone .ff> <out dir>`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(zone), Some(dir)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: jup_probes <map zone .ff> <out dir>");
        std::process::exit(2);
    };
    let src = fastfile_jup::map::MapSources::beside(std::path::Path::new(zone));
    let oodle = fastfile_jup::oodle::load(&src.install).expect("oodle");
    let map = fastfile_jup::map::JupMap::load(&src, oodle.as_ref()).expect("map");
    let _ = std::fs::create_dir_all(dir);
    for l in map.report.iter().filter(|l| l.starts_with("lighting")) {
        println!("{l}");
    }
    for (k, p) in map.lighting.probes.iter().enumerate() {
        let Some(m) = p.load(map.packages.as_deref(), 128, oodle.as_ref()) else {
            println!("probe {k} at {:?}: picture not in the zone", p.origin);
            continue;
        };
        let a = m.ambient();
        println!("probe {k} at {:?}: up {:?} side {:?} down {:?}", p.origin, a.up, a.side, a.down);
        let n = m.size as u32;
        let px: Vec<u8> = m
            .rgb
            .iter()
            .flat_map(|c| c.map(|v| ((v / (1.0 + v)).powf(1.0 / 2.2) * 255.0).clamp(0.0, 255.0) as u8))
            .collect();
        let f = std::fs::File::create(format!("{dir}/probe{k}.png")).expect("png");
        let mut e = png::Encoder::new(f, n, n);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().expect("header").write_image_data(&px).expect("data");
    }
}

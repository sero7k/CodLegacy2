//! `t7_volumetric_check <map.ff>`: the map's lit fog volumes (flags, box)
//! and, for an eye at each one's centre looking along +x, what the view
//! packs and which of the exe's passes it runs (`render_t7::volumetric`);
//! and the world's cookie array image and the sun's cloud layer (`t7 cloud
//! shadows`). No GPU. The printed `T7_EYE` puts `t7_host_check` there.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).expect("map zone");
    let z = fastfile_t7::Zone::open(std::path::Path::new(path)).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let g = w.capture.world.gfx_worlds.first().expect("gfxworld");
    let sun_settings = g.sun_volumes.first().and_then(|&s| fastfile_t7::native::global_light_settings(&z.image, s, 0));
    let sun = sun_settings.as_ref().map(fastfile_t7::native::view_sun).unwrap_or_default();
    let cfg = |k: usize| sun.config[k / 4];
    println!(
        "t7 cloud shadows: {path}: cookie array image {:?} ({:?}); sun config 0x50 (cloud layer + 1) {}, 0x40 {:.4}, 0x44..0x4c {:.4} {:.4} {:.4}, 0x64 {:.4}",
        g.cookie_image,
        g.cookie_image.as_ref().and_then(|n| w.capture.images.iter().find(|i| &i.name == n)).map(|i| (i.width, i.height, i.depth, i.levels, i.format, i.xpak.len())),
        cfg(0x50),
        f32::from_bits(cfg(0x40)),
        f32::from_bits(cfg(0x44)),
        f32::from_bits(cfg(0x48)),
        f32::from_bits(cfg(0x4c)),
        f32::from_bits(cfg(0x64))
    );
    // Which order the array's bytes are in: each layer's mip 1 should be
    // its mip 0 box-filtered (uncompressed RGBA8 only).
    if let Some(im) = g.cookie_image.as_ref().and_then(|n| w.capture.images.iter().find(|i| &i.name == n)) {
        if let (Some((at, len)), 28 | 29 | 87 | 88) = (im.pixels, im.format) {
            let d = &z.image[at..at + len];
            let (cw, ch, layers) = (usize::from(im.width), usize::from(im.height), usize::from(im.depth.max(1)));
            let s0 = cw * ch * 4;
            let chain: usize = (0..usize::from(im.levels)).map(|l| (cw >> l).max(1) * (ch >> l).max(1) * 4).sum();
            let err = |m0: usize, m1: usize| -> f64 {
                let mut e = 0f64;
                for y in 0..ch / 2 {
                    for x in 0..cw / 2 {
                        for c in 0..4 {
                            let p = |xx: usize, yy: usize| f64::from(d[m0 + (yy * cw + xx) * 4 + c]);
                            let avg = (p(2 * x, 2 * y) + p(2 * x + 1, 2 * y) + p(2 * x, 2 * y + 1) + p(2 * x + 1, 2 * y + 1)) / 4.0;
                            e += (avg - f64::from(d[m1 + (y * (cw / 2) + x) * 4 + c])).abs();
                        }
                    }
                }
                e / (cw * ch) as f64
            };
            println!("t7 cloud shadows: {path}: cookie array {} bytes, {layers} layers x chain {chain} = {}", d.len(), layers * chain);
            for l in 0..layers.min(4) {
                let lm = if layers * chain <= d.len() { err(l * s0, layers * s0 + l * s0 / 4) } else { f64::NAN };
                let ym = if layers * chain <= d.len() { err(l * chain, l * chain + s0) } else { f64::NAN };
                println!("t7 cloud shadows: {path}: layer {l}: mip 1 vs filtered mip 0, mean abs error: level-major {lm:.2}, layer-major {ym:.2}");
            }
        }
    }
    println!("t7 cloud shadows: {path}: spot light cookie layers (light word +0x230: layer, lights): {:?}", render_t7::lights::MapLights::read(&z.image, &w.capture).cookie_layers());
    for (layer, o, d) in render_t7::lights::MapLights::read(&z.image, &w.capture).cookie_spots().into_iter().take(8) {
        // (An eye 150 units back from the light along its direction,
        // looking along it: `T7_EYE` for `t7_host_check`.)
        let e: [f32; 3] = std::array::from_fn(|k| o[k] - d[k] * 150.0);
        let pitch = -d[2].clamp(-1.0, 1.0).asin().to_degrees();
        let yaw = d[1].atan2(d[0]).to_degrees();
        println!("t7 cloud shadows: {path}: cookie spot light word {layer} at {o:?} dir {d:?}; T7_EYE=\"{:.0} {:.0} {:.0} {pitch:.0} {yaw:.0}\"", e[0], e[1], e[2]);
    }
    println!("t7 volumetrics: {path}: volumetric lights (spot, omni): {:?}", render_t7::lights::MapLights::read(&z.image, &w.capture).volumetric_counts());
    let Some(vols) = g.lit_fog_volumes else {
        println!("t7 volumetrics: {path}: no lit fog volumes");
        return;
    };
    let v = render_t7::volumetric::Volumes { image: &z.image, volumes: vols, planes: g.lit_fog_planes };
    let st = render_t7::volumetric::BankStates::new(&v);
    let f = |at: usize| f32::from_le_bytes(z.image[at..at + 4].try_into().unwrap());
    for i in 0..v.len() {
        let r = vols.0 + render_t7::volumetric::VOLUME * i;
        let lo = [f(r + 8), f(r + 12), f(r + 16)];
        let hi = [f(r + 20), f(r + 24), f(r + 28)];
        let eye: [f32; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5);
        let flags = u32::from_le_bytes(z.image[r + 0x2c..r + 0x30].try_into().unwrap());
        let view = render_t7::volumetric::View { eye, frustum: &[], sun_colour: sun.colour, sun_intensity: sun.intensity, sun_mode: sun.shadow_mode, lighting_state: 0 };
        let p = render_t7::volumetric::pack(&v, &st, &view);
        println!(
            "t7 volumetrics: {path}: volume {i}: flags {flags:#x}, box {lo:?} .. {hi:?}; eye at its centre packs {} (volumes {:?}, flags {:#x}), passes {:?}; T7_EYE=\"{:.0} {:.0} {:.0} 0 0\"",
            p.count,
            p.volumes,
            p.flags,
            render_t7::volumetric::plan(&render_t7::volumetric::Dvars::default(), &p),
            eye[0],
            eye[1],
            eye[2]
        );
        // Bank 0's 19 floats (colour, colour x intensity, weight, ranges,
        // anisotropy, ...) and the record's +0x20 colour.
        let bank: Vec<String> = (0..render_t7::volumetric::BANK_FLOATS).map(|k| format!("{}", f(r + 0x38 + 4 * k))).collect();
        println!("  bank 0: {}; +0x20 {:?}; +0x34 {}", bank.join(" "), [f(r + 0x20), f(r + 0x24), f(r + 0x28)], f(r + 0x34));
    }
    println!("t7 volumetrics: {path}: sun colour {:?} intensity {} mode {}", sun.colour, sun.intensity, sun.shadow_mode);
}

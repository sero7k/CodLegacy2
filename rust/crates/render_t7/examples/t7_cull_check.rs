//! `t7_cull_check <map.ff> [views]`: the G-buffer's frustum culling
//! (`render_t7::cull`) from the map's spawn points: per view, the draws,
//! instances and triangles the G-buffer drew before (all of them) and after,
//! and the GPU pass's indirect arguments held against the CPU test (no
//! pipelines or pictures: little memory). Also checks that every world
//! surface's bounds hold its vertices (what the culling trusts). With
//! `BO2MP_T7_LOD=exe` the static models' LOD picks (`render_t7::lod`) are
//! checked the same way, at 1920x1080 and the exe's LOD defaults.
use render_t7::cull;
use render_t7::scene::NO_CULL;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let max_views: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    // The G-buffer's draws (those with a G-buffer pass).
    let ready: Vec<usize> = (0..scene.draws.len()).filter(|&d| scene.materials[scene.draws[d].material].gbuffer_pass().is_some()).collect();
    let tris = |d: &render_t7::scene::Draw, inst: u32| u64::from(d.index_count / 3) * u64::from(inst);
    let all_tris: u64 = ready.iter().map(|&d| tris(&scene.draws[d], scene.draws[d].instances)).sum();
    let all_inst: u64 = ready.iter().map(|&d| u64::from(scene.draws[d].instances)).sum();
    println!("{}: {} G-buffer draws of {}, {} instances, {:.2} M triangles a frame before culling; {} cull ranges", scene.name, ready.len(), scene.draws.len(), all_inst, all_tris as f64 / 1e6, scene.cull_ranges.len());
    // Runs of consecutive draws sharing material and vertex source (one
    // multi-draw each): what draw merging could save.
    let runs = 1 + ready.windows(2).filter(|w| { let (a, b) = (&scene.draws[w[0]], &scene.draws[w[1]]); a.material != b.material || a.group != b.group }).count();
    let mut sorted: Vec<(usize, u8)> = ready.iter().map(|&d| (scene.draws[d].material, scene.draws[d].group)).collect();
    sorted.sort_unstable();
    sorted.dedup();
    println!("state runs: {runs} runs of same material and source in draw order ({} distinct material/source pairs)", sorted.len());
    // World bounds against the vertices.
    let (mut bad, mut checked) = (0, 0);
    for d in scene.draws.iter().filter(|d| d.group == 3) {
        let s = cull::draw_sphere(d);
        if s[3] < 0.0 {
            continue;
        }
        checked += 1;
        let idx = &scene.world.indices[d.first_index as usize..(d.first_index + d.index_count) as usize];
        let out = idx.iter().any(|&i| {
            let v = (d.base_vertex as usize + i as usize) * 12;
            let p: [f32; 3] = std::array::from_fn(|k| f32::from_le_bytes(scene.world.positions[v + 4 * k..v + 4 * k + 4].try_into().unwrap()));
            let dd = ((p[0] - s[0]).powi(2) + (p[1] - s[1]).powi(2) + (p[2] - s[2]).powi(2)).sqrt();
            dd > s[3]
        });
        bad += usize::from(out);
    }
    println!("world surfaces: {checked} with bounds, {bad} with a vertex outside their sphere");
    let views = render_t7::fx_check::spawn_views(&scene.map.entities);
    let step = (views.len() / max_views).max(1);
    let gpu = render_h1::gpu::Gpu::headless().ok();
    let c = gpu.as_ref().filter(|g| cull::wanted(&g.device)).map(|g| cull::Cull::new(&g.device, &scene, &ready, scene.instances.len() as u32));
    if c.is_none() {
        println!("(no GPU with INDIRECT_FIRST_INSTANCE here: CPU numbers only)");
    }
    let (mut sum_tris, mut sum_inst, mut sum_draws, mut n, mut mismatches) = (0u64, 0u64, 0u64, 0u64, 0usize);
    let mut worst = 0u64;
    for v in views.iter().step_by(step) {
        let eye = glam::Vec3::from(v.eye);
        let (yaw, pitch) = (v.yaw.to_radians(), v.pitch.to_radians());
        let fwd = glam::Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), -pitch.sin());
        let view = glam::Mat4::look_at_rh(eye, eye + fwd, glam::Vec3::Z);
        // (Black Ops II's 65 degree cg_fov is horizontal at 4:3; 16:9 wider.)
        let tan_y = (65f32.to_radians() * 0.5).tan() * 0.75;
        let proj = glam::Mat4::perspective_infinite_reverse_rh(2.0 * tan_y.atan(), 16.0 / 9.0, 4.0);
        let m = proj * view;
        let planes = cull::planes(m);
        let lod = render_t7::lod::View::new(1080, tan_y * 16.0 / 9.0, tan_y, 1.0, 1.0);
        let mut want: Vec<[u32; 2]> = Vec::with_capacity(ready.len());
        let (mut t, mut inst, mut draws) = (0u64, 0u64, 0u64);
        for &di in &ready {
            let d = &scene.draws[di];
            let k = if d.cull != NO_CULL {
                let r = &scene.cull_ranges[d.cull as usize];
                match &r.lod {
                    Some(rl) if d.lod != render_t7::scene::ANY_LOD => r.spheres.iter().enumerate().filter(|(j, s)| cull::visible(&planes, **s) && rl.drawn(&lod, v.eye, *j) == Some(usize::from(d.lod))).count() as u32,
                    _ => r.spheres.iter().filter(|s| cull::visible(&planes, **s)).count() as u32,
                }
            } else if cull::visible(&planes, cull::draw_sphere(d)) {
                d.instances
            } else {
                0
            };
            want.push([k, d.index_count]);
            t += tris(d, k);
            inst += u64::from(k);
            draws += u64::from(k > 0);
        }
        worst = worst.max(t);
        sum_tris += t;
        sum_inst += inst;
        sum_draws += draws;
        n += 1;
        if let (Some(c), Some(g)) = (&c, &gpu) {
            c.set_view(&g.queue, m, v.eye, &lod);
            let mut enc = g.device.create_command_encoder(&Default::default());
            c.record(&mut enc);
            let size = c.args.size();
            let rb = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            enc.copy_buffer_to_buffer(&c.args, 0, &rb, 0, size);
            g.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
            let data = rb.slice(..).get_mapped_range();
            let words: &[u32] = bytemuck::cast_slice(&data);
            for (j, w) in want.iter().enumerate() {
                let got = &words[5 * j..5 * j + 5];
                if got[1] != w[0] || got[0] != w[1] {
                    if mismatches < 8 {
                        println!("  mismatch draw {j}: GPU {got:?}, CPU {} instances", w[0]);
                    }
                    mismatches += 1;
                }
            }
        }
    }
    // The effects left out beyond their far fades draw nothing: the same
    // sprites from every view with and without (and how many effects are
    // evaluated).
    let mut uncut = scene.fx.clone_for_check();
    uncut.cull_far.clear();
    let (mut same, mut differ, mut evaluated, mut placed) = (0usize, 0usize, 0usize, 0usize);
    let (mut t_cut, mut t_all) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for v in views.iter().step_by(step) {
        for now in [1000, 7000, 31000] {
            let t = std::time::Instant::now();
            scene.fx.sprites(now, v.eye, &mut a);
            t_cut += t.elapsed();
            let t = std::time::Instant::now();
            uncut.sprites(now, v.eye, &mut b);
            t_all += t.elapsed();
            let eq = a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.record == y.record && x.material == y.material);
            if eq { same += 1 } else { differ += 1 }
            evaluated += scene.fx.placed.iter().filter(|p| scene.fx.cull_far.get(p.def).map(|c| c.def).is_none_or(|f| { let d2: f32 = (0..3).map(|k| (p.origin[k] - v.eye[k]).powi(2)).sum(); d2 <= f * f })).count();
            placed += scene.fx.placed.len();
        }
    }
    println!("effects: {same} view-times the same sprites with the far cut, {differ} differ; {evaluated} of {placed} placed effects evaluated; sim time {:.1} ms with the cut, {:.1} ms without (this build)", t_cut.as_secs_f64() * 1e3, t_all.as_secs_f64() * 1e3);
    let n = n.max(1);
    println!(
        "{n} spawn views: drawn after culling (mean) {:.0} of {} draws, {:.0} of {all_inst} instances, {:.2} of {:.2} M triangles ({:.0}%); worst view {:.2} M",
        sum_draws as f64 / n as f64,
        ready.len(),
        sum_inst as f64 / n as f64,
        sum_tris as f64 / n as f64 / 1e6,
        all_tris as f64 / 1e6,
        100.0 * sum_tris as f64 / n as f64 / all_tris.max(1) as f64,
        worst as f64 / 1e6
    );
    if c.is_some() {
        println!("GPU culling pass vs CPU: {mismatches} mismatched draws");
    }
}

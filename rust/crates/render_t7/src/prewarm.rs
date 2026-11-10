//! What the load can make before the map is read: the lighting stage's and
//! the post chain's programs and pipelines depend on `core_patch` and the
//! device only, so they are made side by side while the map zone is still
//! being read (the stages that need them then find them in `crate::memo`).

use std::collections::HashMap;
use std::path::PathBuf;

use render_h1::gpu::Gpu;

use crate::compute::{CodeShader, Program, code_shaders};
use crate::lighting::PERMUTATIONS;

/// The lighting stage's compute programs, side by side (`mode`: the shadow
/// mode when known, which picks the classify program; None makes both).
pub fn lighting_programs(gpu: &Gpu, shaders: &HashMap<String, CodeShader>, mode: Option<i32>) {
    // (The permuted programs are the slow ones: first, so no worker is left
    // with one at the end.)
    let mut specs: Vec<(String, Vec<u32>, bool)> = (0..PERMUTATIONS).map(|n| (format!("deferred_lighting_permuted({n})"), vec![4], true)).collect();
    specs.push(("deferred_permute_uber_resolve_gtao".into(), vec![1], false));
    specs.push(("deferred_permute_uber_resolve_gtao_2".into(), vec![], false));
    specs.push(("deferred_permute_clear".into(), vec![], false));
    if mode != Some(0) {
        specs.push(("deferred_permute_classify_swizzle".into(), vec![], false));
    }
    if mode.is_none() || mode == Some(0) {
        specs.push(("deferred_permute_classify_swizzle_no_sun".into(), vec![], false));
    }
    specs.push(("deferred_permute_draw".into(), vec![], false));
    crate::par::map(&specs, |(name, unfilterable, split)| {
        let Some(cs) = shaders.get(name) else { return };
        let _ = if *split { Program::new_split(gpu, cs, &[], unfilterable) } else { Program::new_with(gpu, cs, &[], unfilterable) };
    });
}

/// Starts making the programs and pipelines that need no map, on a thread
/// of their own. The returned handle may be dropped (it only warms the memo).
pub fn start(gpu: Gpu, zone_dir: PathBuf) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("t7_native_prewarm".into())
        .spawn(move || {
            let t = std::time::Instant::now();
            let Some(patch) = fastfile_t7::shared::zone(&zone_dir, "core_patch") else { return };
            let shaders = code_shaders(&patch.image, &patch.capture);
            let t_zone = t.elapsed().as_secs_f32();
            // The post chain's code passes and its compute programs, the
            // forward lists' culling and depth programs, with the lighting
            // stage's: one batch, the slow ones first.
            enum Job<'a> {
                Pass(&'a str),
                Cs(&'a str, Vec<(u32, &'static str)>, Vec<u32>, bool),
            }
            let techs = crate::post_pass::code_techs(&patch.image, &patch.capture, crate::post_run::techs());
            let hdr = if gpu.device.features().contains(wgpu::Features::RG11B10UFLOAT_RENDERABLE) { wgpu::TextureFormat::Rg11b10Ufloat } else { wgpu::TextureFormat::Rgba16Float };
            let mut jobs: Vec<Job> = crate::post_run::techs().iter().map(|n| Job::Pass(n)).collect();
            jobs.push(Job::Cs("create_lut2dv_compute", vec![], vec![], false));
            jobs.push(Job::Cs("light_culling_lights", vec![], vec![], false));
            jobs.push(Job::Cs("light_culling_probes", vec![], vec![], false));
            jobs.push(Job::Cs("downsample_depth_4x_compute", vec![(0, "r32float")], vec![0], true));
            // (Lighting first: its programs are the longest.)
            lighting_programs(&gpu, &shaders, None);
            crate::par::map(&jobs, |job| match job {
                Job::Pass(n) => {
                    if let Ok(techs) = &techs
                        && let Some(tech) = techs.get(*n)
                    {
                        let _ = crate::post_pass::CodePass::new(&gpu, tech, &crate::post_run::pass_formats(n, hdr), &[0x1d]);
                    }
                }
                Job::Cs(n, uav, unf, nonfiltering) => {
                    if let Some(cs) = shaders.get(*n) {
                        let _ = if *nonfiltering { Program::new_opts(&gpu, cs, uav, unf, true) } else { Program::new(&gpu, cs, uav) };
                    }
                }
            });
            diag_line(&format!("prewarm: core_patch read in {t_zone:.1}s; lighting, post and forward programs made in {:.1}s", t.elapsed().as_secs_f32()));
        })
        .ok()
}

/// A progress line for the log (taken with the report).
pub fn note(line: &str) {
    diag_line(line);
}

fn diag_line(line: &str) {
    if let Ok(mut l) = LINES.lock() {
        l.push(line.to_owned());
    }
}

static LINES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The prewarm's log lines since the last call.
pub fn take_report() -> Vec<String> {
    LINES.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

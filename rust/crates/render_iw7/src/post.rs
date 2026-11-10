//! IW7's display: automatic exposure and the vision's tone map and colour
//! table (`fastfile_iw7::light::Vision`, from `postfx_tonemap_macroclut`'s
//! maths: Insomniac's piecewise rational curve per channel, toe below the
//! crossover and shoulder above, then sRGB, then the 32^3 `clut0Image`).
//!
//! Exposure (`Vision::exposure_for`): with `tonemapAuto` the scene's
//! log-average HDR luminance is metered each frame (a quarter-size
//! log-luminance target halved down to one texel), adapted over time
//! ([`ADAPT_RATE`]), and the exposure brings it to `AUTO_KEY` moved by the
//! vision's adjustments, no brighter than `tonemapMaxExposure`; without it
//! the vision's fixed exposure. `BO2MP_IW7_EXPOSURE=<x>` multiplies it.
//!
//! As `postfx_tonemap_veil_filmgrain_macroclut` (code_post_gfx, read from
//! its translation): the scene is first mixed with its veil (`scene * (1 -
//! s) + veil * s`, `veilStrength`), the encode after the curve is
//! `1.055 x^(1/2.4) - 0.055` (no linear part), and the film grain
//! (`tonemapGrainStrength`) scales the final colour by `1 + n w g`, `n`
//! the shader's hash noise in -1..1 and `w = max(1 - m^2, 0)` with `m` the
//! colour's middle channel lifted by `0.1 saturate(1 - m)`. The veil is the
//! scene blurred wide (halvings to an eighth, each tap weighed by
//! `1 / (1 + luminance)` so lamps don't blot, then the glare passes'
//! 5-tap kernel 0.4921875 / 0.234375 / 0.01953125 across and down); the
//! scene's weight `1 - s` and the taps' spacing are ours (the engine sets
//! them). With `tonemapAutoExposureAdjustCurve` the automatic exposure is
//! moved by the scene's EV (`log2(EXPOSURE_K metered / AUTO_KEY)`, the fixed
//! exposure that would give it) along the vision's dark/mid/light points,
//! clamped to its min/max adjustment ([`ExposureCurve`]; the EV's units are
//! ours). The colour matrix the game applies after the curve is the
//! identity for the visions read (colorization and lift/mid/high unset).

use fastfile_iw7::light::{AUTO_KEY, EXPOSURE_K, LUT_SIZE, Vision};
use render_h1::gpu::Gpu;
use wgpu::util::DeviceExt;

/// How fast the metered light follows the scene (per second; the eye's
/// adaptation).
pub const ADAPT_RATE: f32 = 2.0;

/// The vision's other display values (`veilStrength`,
/// `tonemapGrainStrength`, `tonemapAdaptSpeed`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub veil: f32,
    pub grain: f32,
    /// The share of the way the adapted light moves toward the metered one
    /// each 1/60 s.
    pub adapt_speed: f32,
    /// `tonemapAutoExposureAdjustCurve`: the automatic exposure moved by
    /// the scene's brightness, from (dark, mid, light) EVs to adjustments
    /// in stops, clamped to (min, max).
    pub curve: Option<ExposureCurve>,
}

/// The automatic exposure's adjustment by the scene's EV.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureCurve {
    pub ev: [f32; 3],
    pub adjust: [f32; 3],
    pub min: f32,
    pub max: f32,
}

impl ExposureCurve {
    /// The adjustment (stops) at scene EV `ev`: linear between the points,
    /// flat beyond them, clamped.
    pub fn at(&self, ev: f32) -> f32 {
        let [e0, e1, e2] = self.ev;
        let [a0, a1, a2] = self.adjust;
        let lerp = |x: f32, xa: f32, xb: f32, ya: f32, yb: f32| if xb > xa { ya + (yb - ya) * ((x - xa) / (xb - xa)).clamp(0.0, 1.0) } else { ya };
        let a = if ev <= e1 { lerp(ev, e0, e1, a0, a1) } else { lerp(ev, e1, e2, a1, a2) };
        a.clamp(self.min.min(self.max), self.max.max(self.min))
    }
}

impl Default for Look {
    fn default() -> Self {
        Self { veil: 0.0, grain: 0.0, adapt_speed: 1.0 - (-ADAPT_RATE / 60.0f32).exp(), curve: None }
    }
}

impl Look {
    pub fn from_vision(text: &str) -> Self {
        let num = |k: &str| fastfile_iw7::light::vision_value(text, k).and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok()).filter(|v| v.is_finite());
        let d = Self::default();
        Self {
            veil: num("veilStrength").unwrap_or(0.0).clamp(0.0, 1.0),
            grain: num("tonemapGrainStrength").unwrap_or(0.0).clamp(0.0, 1.0),
            adapt_speed: num("tonemapAdaptSpeed").filter(|v| *v > 0.0).unwrap_or(d.adapt_speed).min(1.0),
            curve: num("tonemapAutoExposureAdjustCurve").filter(|v| *v != 0.0).map(|_| ExposureCurve {
                ev: [num("tonemapDarkEv").unwrap_or(0.0), num("tonemapMidEv").unwrap_or(0.0), num("tonemapLightEv").unwrap_or(0.0)],
                adjust: [num("tonemapDarkExposureAdjust").unwrap_or(0.0), num("tonemapMidExposureAdjust").unwrap_or(0.0), num("tonemapLightExposureAdjust").unwrap_or(0.0)],
                min: num("tonemapMinExposureAdjust").unwrap_or(-10.0),
                max: num("tonemapMaxExposureAdjust").unwrap_or(10.0),
            }),
        }
    }
}

/// A quarter-size copy of the HDR frame (2x2 bilinear taps of a 4x4 block).
const VEIL_DOWN: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var lin: sampler;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let dst = vec2<f32>(textureDimensions(src)) / 2.0;
    let uv = v.p.xy / dst;
    let t = 0.5 / vec2<f32>(textureDimensions(src));
    // (Each tap weighed by 1 / (1 + its luminance), so a few pixels far
    // brighter than the rest, lamps and glints, don't spread into blots.)
    var c = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0; i < 4; i++) {
        let o = vec2<f32>(select(-t.x, t.x, (i & 1) == 1), select(-t.y, t.y, (i & 2) == 2));
        let s = safe3(textureSampleLevel(src, lin, uv + o, 0.0).rgb);
        let w = 1.0 / (1.0 + dot(s, vec3<f32>(0.2126, 0.7152, 0.0722)));
        c += s * w;
        wsum += w;
    }
    return vec4<f32>(c / max(wsum, 1e-6), 1.0);
}
";

/// The glare passes' 5-tap blur along `d.xy` (texels).
const VEIL_BLUR: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var lin: sampler;
struct D { d: vec4<f32> };
@group(0) @binding(2) var<uniform> d: D;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(src));
    let uv = v.p.xy / size;
    let o = d.d.xy / size;
    var c = textureSampleLevel(src, lin, uv, 0.0).rgb * 0.4921875;
    c += (textureSampleLevel(src, lin, uv + o * 1.5, 0.0).rgb + textureSampleLevel(src, lin, uv - o * 1.5, 0.0).rgb) * 0.234375;
    c += (textureSampleLevel(src, lin, uv + o * 3.5, 0.0).rgb + textureSampleLevel(src, lin, uv - o * 3.5, 0.0).rgb) * 0.01953125;
    return vec4<f32>(c, 1.0);
}
";

const FULL_VS: &str = r"
struct Vo { @builtin(position) p: vec4<f32> };
// A value that is not finite (its exponent all ones: an infinity or NaN),
// tested on the bits since a shader may assume no NaN.
fn not_finite(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7f800000u) == 0x7f800000u;
}
// HDR colour made safe to average: not finite -> 0, negatives -> 0, at
// most half float's largest. (One infinite pixel from any surface would otherwise
// make the metered luminance infinite, the exposure 0 and, through the
// adaptation, every later frame black.)
fn safe3(c: vec3<f32>) -> vec3<f32> {
    if (not_finite(c.x) || not_finite(c.y) || not_finite(c.z)) { return vec3<f32>(0.0); }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(65504.0));
}
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}
";

/// Log luminance of the HDR frame, averaged over 4x4 texels.
const LOGLUM: &str = r"
@group(0) @binding(0) var hdr: texture_2d<f32>;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(hdr));
    let base = vec2<i32>(v.p.xy) * 4;
    var s = 0.0;
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let c = safe3(textureLoad(hdr, min(base + vec2<i32>(x, y), size - 1), 0).rgb);
            let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
            s += log(max(l, 1e-4));
        }
    }
    return vec4<f32>(s / 16.0, 0.0, 0.0, 1.0);
}
";

/// One halving (2x2 average, the edge texels taken again when odd).
const DOWN: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(src));
    let b = vec2<i32>(v.p.xy) * 2;
    var s = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            s += textureLoad(src, min(b + vec2<i32>(x, y), size - 1), 0).r;
        }
    }
    return vec4<f32>(s * 0.25, 0.0, 0.0, 1.0);
}
";

/// The adapted log luminance: toward this frame's by `k` (1 the first).
const ADAPT: &str = r"
@group(0) @binding(0) var now: texture_2d<f32>;
@group(0) @binding(1) var before: texture_2d<f32>;
struct A { k: vec4<f32> };
@group(0) @binding(2) var<uniform> a: A;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    var n = textureLoad(now, vec2<i32>(0, 0), 0).r;
    var b = textureLoad(before, vec2<i32>(0, 0), 0).r;
    // (Never keep a value that is not finite: it would stay for good.)
    if (not_finite(n)) { n = select(b, 0.0, not_finite(b)); }
    if (not_finite(b)) { b = n; }
    return vec4<f32>(mix(b, n, a.k.x), 0.0, 0.0, 1.0);
}
";

/// The tone map and colour table (the vision's numbers in `p`).
const DISPLAY: &str = r"
@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var lum: texture_2d<f32>;
@group(0) @binding(2) var clut: texture_3d<f32>;
@group(0) @binding(3) var lin: sampler;
struct P {
    // black, white, crossover, shoulder
    curve: vec4<f32>,
    // toe, auto (0/1), fixed exposure, auto key times its adjustment
    e: vec4<f32>,
    // max exposure (0: none), user scale, clut on (0/1), -
    m: vec4<f32>,
};
@group(0) @binding(4) var<uniform> p: P;
@group(0) @binding(5) var veil: texture_2d<f32>;
struct F {
    // veil strength, grain strength, noise scale, curve on (0/1)
    f: vec4<f32>,
    // the curve's dark, mid, light EVs and its least adjustment
    ev: vec4<f32>,
    // their adjustments (stops) and the most
    adj: vec4<f32>,
};
@group(0) @binding(6) var<uniform> fr: F;

fn tonemap(x: f32) -> f32 {
    let b = p.curve.x; let w = p.curve.y; let c = p.curve.z; let s = p.curve.w; let t = p.e.x;
    let k = (1.0 - t) * (c - b) / ((1.0 - s) * (w - c) + (1.0 - t) * (c - b));
    let x0 = max(x, 0.0);
    var y: f32;
    if (x0 < c) {
        y = k * (1.0 - t) * (x0 - b) / (c - (1.0 - t) * b - t * x0);
    } else {
        y = k + (1.0 - k) * (x0 - c) / (s * x0 + (1.0 - s) * w - c);
    }
    return clamp(y, 0.0, 1.0);
}

fn encode(c: f32) -> f32 {
    return max(1.055 * pow(max(c, 0.0), 1.0 / 2.4) - 0.055, 0.0);
}

@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    var e = p.e.z;
    if (p.e.y > 0.5) {
        let metered = exp(textureLoad(lum, vec2<i32>(0, 0), 0).r);
        e = p.e.w / max(metered, 1e-5);
        if (fr.f.w > 0.5) {
            // (The scene's EV: the fixed exposure's EV that would give the
            // automatic one, before its adjustments.)
            let ev = log2(max(EXPOSURE_K * metered / AUTO_KEY, 1e-6));
            var a: f32;
            if (ev <= fr.ev.y) {
                a = mix(fr.adj.x, fr.adj.y, clamp((ev - fr.ev.x) / max(fr.ev.y - fr.ev.x, 1e-4), 0.0, 1.0));
            } else {
                a = mix(fr.adj.y, fr.adj.z, clamp((ev - fr.ev.y) / max(fr.ev.z - fr.ev.y, 1e-4), 0.0, 1.0));
            }
            e *= exp2(clamp(a, fr.ev.w, fr.adj.w));
        }
        if (p.m.x > 0.0) { e = min(e, p.m.x); }
    }
    e *= p.m.y;
    let size = vec2<f32>(textureDimensions(hdr));
    let uv = v.p.xy / size;
    let scene = safe3(textureLoad(hdr, vec2<i32>(v.p.xy), 0).rgb);
    let glow = safe3(textureSampleLevel(veil, lin, uv, 0.0).rgb);
    let c = (scene * (1.0 - fr.f.x) + glow * fr.f.x) * e;
    var s = vec3<f32>(encode(tonemap(c.r)), encode(tonemap(c.g)), encode(tonemap(c.b)));
    if (p.m.z > 0.5) {
        // (Texel centres of the 32-texel table.)
        let n = 32.0;
        s = textureSampleLevel(clut, lin, s * ((n - 1.0) / n) + 0.5 / n, 0.0).rgb;
    }
    if (fr.f.y > 0.0) {
        // The game's grain: its hash of the pixel, weighted away from the
        // brightest and darkest.
        var q = fract(uv * fr.f.z) * 1024.0 - 512.0;
        let h = fract(q.yx * vec2<f32>(0.554549694, 0.308517009));
        q = q.yx * h + q;
        let n = fract(q.y * q.x) * 2.0 - 1.0;
        var m = max(min(s.b, max(s.r, s.g)), min(s.r, s.g));
        m = clamp(1.0 - m, 0.0, 1.0) * 0.1 + m;
        let w = max(1.0 - m * m, 0.0);
        s = s * (n * w * fr.f.y) + s;
    }
    return vec4<f32>(s, 1.0);
}
";

struct Sized {
    width: u32,
    height: u32,
    /// The log-luminance chain, biggest first, the last one texel.
    chain: Vec<(wgpu::Texture, wgpu::TextureView)>,
    chain_groups: Vec<wgpu::BindGroup>,
    adapt_groups: [wgpu::BindGroup; 2],
    /// The display's group reading adapted[i].
    display_groups: [wgpu::BindGroup; 2],
    /// The veil: halvings (a half, a quarter, an eighth) and the blur's
    /// two targets, with the groups reading each pass's source.
    veil: Vec<(wgpu::Texture, wgpu::TextureView)>,
    veil_groups: Vec<wgpu::BindGroup>,
}

/// The display's passes.
pub struct Post {
    gpu: Gpu,
    loglum: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    display: wgpu::RenderPipeline,
    one_layout: wgpu::BindGroupLayout,
    adapt_layout: wgpu::BindGroupLayout,
    display_layout: wgpu::BindGroupLayout,
    /// The adapted log luminance, ping-pong (1x1 R32Float).
    adapted: [(wgpu::Texture, wgpu::TextureView); 2],
    which: usize,
    first: bool,
    adapt_k: wgpu::Buffer,
    params: wgpu::Buffer,
    clut: wgpu::TextureView,
    sampler: wgpu::Sampler,
    sized: Option<Sized>,
    pub vision: Vision,
    pub look: Look,
    veil_down: wgpu::RenderPipeline,
    veil_blur: wgpu::RenderPipeline,
    veil_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    /// The blur's directions (across, down) and the frame's display values.
    dirs: [wgpu::Buffer; 2],
    frame: wgpu::Buffer,
    frames: u32,
}

fn tex_entry(binding: u32, dim: wgpu::TextureViewDimension, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: dim, multisampled: false },
        count: None,
    }
}

fn buf_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }
}

fn r32(g: &Gpu, w: u32, h: u32, label: &str) -> (wgpu::Texture, wgpu::TextureView) {
    let t = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let v = t.create_view(&Default::default());
    (t, v)
}

impl Post {
    /// `clut`: the vision's colour table (32^3 RGBA8, red fastest), if any.
    pub fn new(g: &Gpu, vision: Vision, clut: Option<&[u8]>, output: wgpu::TextureFormat) -> Self {
        let pipe = |label: &str, src: &str, layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat| {
            let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(format!("{FULL_VS}{src}").into()) });
            let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
            g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(format.into())] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let layout = |label: &str, e: &[wgpu::BindGroupLayoutEntry]| g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: e });
        let one_layout = layout("iw7 post one", &[tex_entry(0, wgpu::TextureViewDimension::D2, false)]);
        let adapt_layout = layout("iw7 post adapt", &[tex_entry(0, wgpu::TextureViewDimension::D2, false), tex_entry(1, wgpu::TextureViewDimension::D2, false), buf_entry(2)]);
        let display_layout = layout(
            "iw7 post display",
            &[
                tex_entry(0, wgpu::TextureViewDimension::D2, false),
                tex_entry(1, wgpu::TextureViewDimension::D2, false),
                tex_entry(2, wgpu::TextureViewDimension::D3, true),
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                buf_entry(4),
                tex_entry(5, wgpu::TextureViewDimension::D2, true),
                buf_entry(6),
            ],
        );
        let samp = wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let veil_layout = layout("iw7 veil", &[tex_entry(0, wgpu::TextureViewDimension::D2, true), samp]);
        let blur_layout = layout("iw7 veil blur", &[tex_entry(0, wgpu::TextureViewDimension::D2, true), samp, buf_entry(2)]);
        let loglum = pipe("iw7 loglum", LOGLUM, &one_layout, wgpu::TextureFormat::R32Float);
        let down = pipe("iw7 loglum down", DOWN, &one_layout, wgpu::TextureFormat::R32Float);
        let adapt = pipe("iw7 adapt", ADAPT, &adapt_layout, wgpu::TextureFormat::R32Float);
        let display_src = format!("const EXPOSURE_K: f32 = {EXPOSURE_K:?};\nconst AUTO_KEY: f32 = {AUTO_KEY:?};\n{DISPLAY}");
        let display = pipe("iw7 tonemap", &display_src, &display_layout, output);
        let veil_down = pipe("iw7 veil down", VEIL_DOWN, &veil_layout, wgpu::TextureFormat::Rgba16Float);
        let veil_blur = pipe("iw7 veil blur", VEIL_BLUR, &blur_layout, wgpu::TextureFormat::Rgba16Float);
        let dir = |d: [f32; 4]| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 veil dir"), contents: bytemuck::cast_slice(&d), usage: wgpu::BufferUsages::UNIFORM });
        let dirs = [dir([1.0, 0.0, 0.0, 0.0]), dir([0.0, 1.0, 0.0, 0.0])];
        let frame = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 display frame"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let n = LUT_SIZE as u32;
        let (table, on) = match clut.filter(|c| c.len() >= (n * n * n * 4) as usize) {
            Some(c) => (c.to_vec(), 1.0),
            None => (vec![255u8; 4], 0.0),
        };
        let side = if on > 0.0 { n } else { 1 };
        let clut_tex = g.device.create_texture_with_data(
            &g.queue,
            &wgpu::TextureDescriptor {
                label: Some("iw7 clut"),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: side },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                // (The table's bytes are the display values themselves.)
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &table,
        );
        let clut_view = clut_tex.create_view(&Default::default());
        let user = std::env::var("BO2MP_IW7_EXPOSURE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(1.0);
        let key = AUTO_KEY * (vision.auto_adjust + vision.adjust).exp2();
        let max = vision.max_exposure.map_or(0.0, |m| EXPOSURE_K * (-m).exp2());
        let params: [[f32; 4]; 3] = [[vision.black, vision.white, vision.crossover, vision.shoulder], [vision.toe, if vision.auto { 1.0 } else { 0.0 }, vision.exposure(), key], [max, user, on, 0.0]];
        let params = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 tonemap"), contents: bytemuck::cast_slice(&params), usage: wgpu::BufferUsages::UNIFORM });
        let adapt_k = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 adapt"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = g.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self {
            gpu: g.clone(),
            loglum,
            down,
            adapt,
            display,
            one_layout,
            adapt_layout,
            display_layout,
            adapted: [r32(g, 1, 1, "iw7 adapted 0"), r32(g, 1, 1, "iw7 adapted 1")],
            which: 0,
            first: true,
            adapt_k,
            params,
            clut: clut_view,
            sampler,
            sized: None,
            vision,
            look: Look::default(),
            veil_down,
            veil_blur,
            veil_layout,
            blur_layout,
            dirs,
            frame,
            frames: 0,
        }
    }

    fn size(&mut self, hdr: &wgpu::TextureView, width: u32, height: u32, rebuild: bool) {
        if !rebuild && self.sized.as_ref().is_some_and(|s| (s.width, s.height) == (width, height)) {
            return;
        }
        let g = &self.gpu;
        let mut chain = Vec::new();
        let (mut w, mut h) = (width.div_ceil(4).max(1), height.div_ceil(4).max(1));
        loop {
            chain.push(r32(g, w, h, "iw7 loglum"));
            if w == 1 && h == 1 {
                break;
            }
            (w, h) = (w.div_ceil(2), h.div_ceil(2));
        }
        let one = |v: &wgpu::TextureView| g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("iw7 post"), layout: &self.one_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) }] });
        // Group k reads chain[k - 1] (group 0 the HDR frame).
        let mut chain_groups = vec![one(hdr)];
        for c in &chain[..chain.len() - 1] {
            chain_groups.push(one(&c.1));
        }
        let last = &chain.last().expect("one texel").1;
        let adapt_group = |prev: usize| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 adapt"),
                layout: &self.adapt_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(last) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.adapted[prev].1) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.adapt_k.as_entire_binding() },
                ],
            })
        };
        // (Group i writes adapted[i], reading the other.)
        let adapt_groups = [adapt_group(1), adapt_group(0)];
        // The veil's targets: a half, a quarter, an eighth, then the blur's.
        let rgba = |w: u32, h: u32| {
            let t = g.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("iw7 veil"),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let v = t.create_view(&Default::default());
            (t, v)
        };
        let (e8w, e8h) = (width.div_ceil(8), height.div_ceil(8));
        let veil = vec![rgba(width.div_ceil(2), height.div_ceil(2)), rgba(width.div_ceil(4), height.div_ceil(4)), rgba(e8w, e8h), rgba(e8w, e8h), rgba(e8w, e8h)];
        let down_group = |src: &wgpu::TextureView| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 veil"),
                layout: &self.veil_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            })
        };
        let blur_group = |src: &wgpu::TextureView, k: usize| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 veil blur"),
                layout: &self.blur_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.dirs[k].as_entire_binding() },
                ],
            })
        };
        // (Passes: hdr -> half -> quarter -> eighth, eighth -> across, across -> down.)
        let veil_groups = vec![down_group(hdr), down_group(&veil[0].1), down_group(&veil[1].1), blur_group(&veil[2].1, 0), blur_group(&veil[3].1, 1)];
        let display_group = |w: usize| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 display"),
                layout: &self.display_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(hdr) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.adapted[w].1) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.clut) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 4, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&veil[4].1) },
                    wgpu::BindGroupEntry { binding: 6, resource: self.frame.as_entire_binding() },
                ],
            })
        };
        let display_groups = [display_group(0), display_group(1)];
        self.sized = Some(Sized { width, height, chain, chain_groups, adapt_groups, display_groups, veil, veil_groups });
    }

    /// Meters `hdr` (`Rgba16Float`, `width` x `height`), adapts over `dt`
    /// seconds and writes the display image into `out`. `rebuild`: the HDR
    /// view changed.
    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, hdr: &wgpu::TextureView, out: &wgpu::TextureView, width: u32, height: u32, dt: f32, rebuild: bool) -> u32 {
        self.size(hdr, width, height, rebuild);
        // (The vision's adapt speed is a share each 1/60 s.)
        let k = if self.first { 1.0 } else { 1.0 - (1.0 - self.look.adapt_speed).powf(dt.max(0.0) * 60.0) };
        self.first = false;
        self.gpu.queue.write_buffer(&self.adapt_k, 0, bytemuck::cast_slice(&[k, 0.0f32, 0.0, 0.0]));
        self.frames = self.frames.wrapping_add(1);
        let noise = 1.0 + (self.frames as f32 * 0.618_034).fract();
        let c = self.look.curve;
        let (ev, adj, on) = c.map_or(([0.0; 3], [0.0; 3], 0.0), |c| (c.ev, c.adjust, 1.0));
        let (lo, hi) = c.map_or((0.0, 0.0), |c| (c.min.min(c.max), c.max.max(c.min)));
        let f: [f32; 12] = [self.look.veil, self.look.grain, noise, on, ev[0], ev[1], ev[2], lo, adj[0], adj[1], adj[2], hi];
        self.gpu.queue.write_buffer(&self.frame, 0, bytemuck::cast_slice(&f));
        let s = self.sized.as_ref().expect("sized");
        let pass = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, group: &wgpu::BindGroup, target: &wgpu::TextureView| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iw7 post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, group, &[]);
            rp.draw(0..3, 0..1);
        };
        pass(enc, &self.loglum, &s.chain_groups[0], &s.chain[0].1);
        for i in 1..s.chain.len() {
            pass(enc, &self.down, &s.chain_groups[i], &s.chain[i].1);
        }
        let w = self.which;
        pass(enc, &self.adapt, &s.adapt_groups[w], &self.adapted[w].1);
        let veil_passes = if self.look.veil > 0.0 {
            for (i, (pipe, target)) in [(&self.veil_down, 0), (&self.veil_down, 1), (&self.veil_down, 2), (&self.veil_blur, 3), (&self.veil_blur, 4)].into_iter().enumerate() {
                pass(enc, pipe, &s.veil_groups[i], &s.veil[target].1);
            }
            5
        } else {
            0
        };
        // The display reads the texel just adapted.
        pass(enc, &self.display, &s.display_groups[w], out);
        self.which = 1 - w;
        s.chain.len() as u32 + 2 + veil_passes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exposure_curve_follows_riots_points_and_clamps() {
        let c = ExposureCurve { ev: [9.0, 11.0, 14.0], adjust: [0.0, 0.4, 1.0], min: -0.25, max: 0.75 };
        assert_eq!(c.at(5.0), 0.0);
        assert!((c.at(10.0) - 0.2).abs() < 1e-5);
        assert!((c.at(12.5) - 0.7).abs() < 1e-5);
        assert_eq!(c.at(20.0), 0.75);
    }
}

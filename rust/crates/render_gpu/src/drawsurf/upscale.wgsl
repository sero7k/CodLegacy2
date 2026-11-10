// bo2mp: FSR 1 (see upscale.rs). AMD FidelityFX Super Resolution 1.0's two
// passes, written out in WGSL from the published algorithm (MIT licence,
// copyright Advanced Micro Devices, Inc.): EASU, the edge-adaptive spatial
// upsample (a 12-tap kernel bent along the local edge direction), then RCAS,
// the robust contrast-adaptive sharpen (a 5-tap cross with a per-pixel limit
// on how far it may sharpen). Both work on perceptual (gamma-encoded) colour,
// which is what the match's frame already holds (its sRGB compositing space),
// read here through a non-sRGB view of the same texture.

struct Params {
    // x, y: the source (drawn) frame's size in texels; z, w: the output size.
    sizes: vec4<f32>,
    // x: RCAS strength (0 = off, else 2^-stops: 1 is the sharpest);
    // y: 1 when the destination is an sRGB texture (write linear colour).
    misc: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> FullscreenOut {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: FullscreenOut;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    return out;
}

fn load(p: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    return textureLoad(source, clamp(p, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
}

// FSR's luma: half of red and blue, all of green.
fn luma(c: vec3<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

// One tap of the kernel: a Lanczos-like lobe, stretched along `dir`.
fn easu_tap(
    acc: ptr<function, vec3<f32>>,
    weight: ptr<function, f32>,
    off: vec2<f32>,
    dir: vec2<f32>,
    len: vec2<f32>,
    lob: f32,
    clp: f32,
    c: vec3<f32>,
) {
    var v = vec2(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x);
    v = v * len;
    var d2 = v.x * v.x + v.y * v.y;
    d2 = min(d2, clp);
    var wb = 0.4 * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb = wb * wb;
    wa = wa * wa;
    wb = 1.5625 * wb - 0.5625;
    let w = wb * wa;
    *acc = *acc + c * w;
    *weight = *weight + w;
}

// Edge direction and length from one 5-luma cross (a top, b left, c centre,
// d right, e bottom), weighted by how near the output pixel is to `c`.
fn easu_set(dir: ptr<function, vec2<f32>>, len: ptr<function, f32>, w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32) {
    var len_x = max(abs(ld - lc), abs(lc - lb));
    let dir_x = ld - lb;
    (*dir).x = (*dir).x + dir_x * w;
    len_x = saturate(abs(dir_x) / max(len_x, 1.0e-8));
    len_x = len_x * len_x;
    *len = *len + len_x * w;
    var len_y = max(abs(le - lc), abs(lc - la));
    let dir_y = le - la;
    (*dir).y = (*dir).y + dir_y * w;
    len_y = saturate(abs(dir_y) / max(len_y, 1.0e-8));
    len_y = len_y * len_y;
    *len = *len + len_y * w;
}

// Pass 1: the drawn frame to the output size.
@fragment
fn easu(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let src_size = params.sizes.xy;
    let isize = vec2<i32>(src_size);
    // The output pixel's place in the source, in texel centres.
    var pp = frag.xy * (src_size / params.sizes.zw) - vec2(0.5);
    let fp = floor(pp);
    pp = pp - fp;
    let ip = vec2<i32>(fp);

    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let b = load(ip + vec2(0, -1), isize);
    let c = load(ip + vec2(1, -1), isize);
    let e = load(ip + vec2(-1, 0), isize);
    let f = load(ip + vec2(0, 0), isize);
    let g = load(ip + vec2(1, 0), isize);
    let h = load(ip + vec2(2, 0), isize);
    let i = load(ip + vec2(-1, 1), isize);
    let j = load(ip + vec2(0, 1), isize);
    let k = load(ip + vec2(1, 1), isize);
    let l = load(ip + vec2(2, 1), isize);
    let n = load(ip + vec2(0, 2), isize);
    let o = load(ip + vec2(1, 2), isize);

    let bl = luma(b);
    let cl = luma(c);
    let el = luma(e);
    let fl = luma(f);
    let gl = luma(g);
    let hl = luma(h);
    let il = luma(i);
    let jl = luma(j);
    let kl = luma(k);
    let ll = luma(l);
    let nl = luma(n);
    let ol = luma(o);

    var dir = vec2(0.0);
    var len = 0.0;
    easu_set(&dir, &len, (1.0 - pp.x) * (1.0 - pp.y), bl, el, fl, gl, jl);
    easu_set(&dir, &len, pp.x * (1.0 - pp.y), cl, fl, gl, hl, kl);
    easu_set(&dir, &len, (1.0 - pp.x) * pp.y, fl, il, jl, kl, nl);
    easu_set(&dir, &len, pp.x * pp.y, gl, jl, kl, ll, ol);

    // Normalise the direction; a flat area has none (horizontal, then).
    let dir2 = dir * dir;
    var dir_r = dir2.x + dir2.y;
    let zero = dir_r < (1.0 / 32768.0);
    dir_r = inverseSqrt(max(dir_r, 1.0e-12));
    if zero {
        dir_r = 1.0;
        dir.x = 1.0;
    }
    dir = dir * dir_r;

    // The kernel's shape: stretched along the edge, narrower the sharper it is.
    len = len * 0.5;
    len = len * len;
    let stretch = (dir.x * dir.x + dir.y * dir.y) / max(max(abs(dir.x), abs(dir.y)), 1.0e-8);
    let len2 = vec2(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((0.25 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;

    // The nearest four pixels bound the result (no ringing past them).
    let min4 = min(min(min(f, g), j), k);
    let max4 = max(max(max(f, g), j), k);

    var acc = vec3(0.0);
    var wsum = 0.0;
    easu_tap(&acc, &wsum, vec2(0.0, -1.0) - pp, dir, len2, lob, clp, b);
    easu_tap(&acc, &wsum, vec2(1.0, -1.0) - pp, dir, len2, lob, clp, c);
    easu_tap(&acc, &wsum, vec2(-1.0, 1.0) - pp, dir, len2, lob, clp, i);
    easu_tap(&acc, &wsum, vec2(0.0, 1.0) - pp, dir, len2, lob, clp, j);
    easu_tap(&acc, &wsum, vec2(0.0, 0.0) - pp, dir, len2, lob, clp, f);
    easu_tap(&acc, &wsum, vec2(-1.0, 0.0) - pp, dir, len2, lob, clp, e);
    easu_tap(&acc, &wsum, vec2(1.0, 1.0) - pp, dir, len2, lob, clp, k);
    easu_tap(&acc, &wsum, vec2(2.0, 1.0) - pp, dir, len2, lob, clp, l);
    easu_tap(&acc, &wsum, vec2(2.0, 0.0) - pp, dir, len2, lob, clp, h);
    easu_tap(&acc, &wsum, vec2(1.0, 0.0) - pp, dir, len2, lob, clp, g);
    easu_tap(&acc, &wsum, vec2(1.0, 2.0) - pp, dir, len2, lob, clp, o);
    easu_tap(&acc, &wsum, vec2(0.0, 2.0) - pp, dir, len2, lob, clp, n);
    let pix = min(max4, max(min4, acc / wsum));
    return vec4(saturate(pix), 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// RCAS never sharpens past this lobe (a quarter minus a sixteenth).
const RCAS_LIMIT: f32 = 0.1875;

// Pass 2: sharpen the enlarged frame, and hand it to the screen's format.
@fragment
fn rcas(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(params.sizes.zw);
    let sp = vec2<i32>(frag.xy);
    //    b
    //  d e f
    //    h
    let b = load(sp + vec2(0, -1), size);
    let d = load(sp + vec2(-1, 0), size);
    let e = load(sp, size);
    let f = load(sp + vec2(1, 0), size);
    let h = load(sp + vec2(0, 1), size);

    var pix = e;
    let strength = params.misc.x;
    if strength > 0.0 {
        // (Luma times two.)
        let bl = b.g + (b.b + b.r) * 0.5;
        let dl = d.g + (d.b + d.r) * 0.5;
        let el = e.g + (e.b + e.r) * 0.5;
        let fl = f.g + (f.b + f.r) * 0.5;
        let hl = h.g + (h.b + h.r) * 0.5;
        // Noise detection: a pixel unlike its neighbours' average is grain,
        // and is sharpened less.
        var nz = 0.25 * bl + 0.25 * dl + 0.25 * fl + 0.25 * hl - el;
        let lmax = max(max(max(bl, dl), max(el, fl)), hl);
        let lmin = min(min(min(bl, dl), min(el, fl)), hl);
        nz = saturate(abs(nz) / max(lmax - lmin, 1.0e-5));
        nz = -0.5 * nz + 1.0;

        let mn4 = min(min(min(b, d), f), h);
        let mx4 = max(max(max(b, d), f), h);
        // How far the lobe may go before a channel clips at 0 or at 1.
        let peak = vec2(1.0, -4.0);
        let hit_min = min(mn4, e) / max(4.0 * mx4, vec3(1.0e-5));
        let hit_max = (vec3(peak.x) - max(mx4, e)) / min(4.0 * mn4 + vec3(peak.y), vec3(-1.0e-5));
        let lobe_c = max(-hit_min, hit_max);
        var lobe = max(-RCAS_LIMIT, min(max(lobe_c.r, max(lobe_c.g, lobe_c.b)), 0.0)) * strength;
        lobe = lobe * nz;
        pix = (lobe * b + lobe * d + lobe * h + lobe * f + e) / (4.0 * lobe + 1.0);
    }
    pix = saturate(pix);
    if params.misc.y > 0.5 {
        pix = srgb_to_linear(pix);
    }
    return vec4(pix, 1.0);
}

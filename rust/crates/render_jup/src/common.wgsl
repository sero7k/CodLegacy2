// Shared by the passes: the frame's constants (group 0, binding 0).
// Matrices take positions relative to the eye, except `sun_vp` (absolute).
struct G {
    vp: mat4x4<f32>,
    inv_vp: mat4x4<f32>,
    sun_vp: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,     // xyz toward the sun
    sun_col: vec4<f32>,     // linear colour times intensity
    fog: vec4<f32>,         // start, half, opacity, normal_scale
    fog_col: vec4<f32>,     // rgb, w = 1 when the map has fog
    fog_sun: vec4<f32>,     // rgb toward the sun
    fog_misc: vec4<f32>,    // base_height, half_height, begin_deg, end_deg
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    ambient: vec4<f32>,     // rgb where no probe, w = probe count
    size: vec4<f32>,        // width, height, 1/width, 1/height
    misc: vec4<f32>,        // shadow on, exposure, shadow texel (world units), night
    amb_up: vec4<f32>,      // open-sky ambient light for a surface facing up
    amb_side: vec4<f32>,    // ... sideways
    amb_down: vec4<f32>,    // ... down (the ground's bounce)
    grid: vec4<f32>,        // sky visibility grid: origin x, origin y, cell, columns
    grid2: vec4<f32>,       // rows, first vec4 of its heights in `probes`, probe reach begin, end
    amb_misc: vec4<f32>,    // probe share of the ambient light, ambient occlusion strength, grid on, display curve
}

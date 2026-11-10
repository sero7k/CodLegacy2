//! The host's own WGSL: the material shader (shadow and G-buffer
//! passes), the lighting pass and the display stage. Each module is the
//! shared frame constants (`common.wgsl`) followed by its pass; the probe
//! (`examples/jup_native_check.rs`) validates all of them with naga.

const COMMON: &str = include_str!("common.wgsl");
const MATERIAL: &str = include_str!("material.wgsl");
const LIGHT: &str = include_str!("light.wgsl");
const DISPLAY: &str = include_str!("display.wgsl");
const LIGHTLIB: &str = include_str!("lightlib.wgsl");
const FORWARD: &str = include_str!("forward.wgsl");

/// One module of the host: its label and source.
pub struct Module {
    pub label: &'static str,
    pub source: String,
}

fn join(label: &'static str, body: &str) -> Module {
    Module { label, source: format!("{COMMON}\n{body}") }
}

/// The material shader: `vs_shadow`, `vs_main`, `fs_main`.
pub fn material() -> Module {
    join("jup_material", MATERIAL)
}

/// The lighting pass: `vs_full`, `fs_light`.
pub fn light() -> Module {
    Module { label: "jup_light", source: format!("{COMMON}\n{LIGHT}\n{LIGHTLIB}") }
}

/// The blended pass: the material shader's vertex stage and surface, then
/// `fs_forward` and `fs_water`.
pub fn forward() -> Module {
    Module { label: "jup_forward", source: format!("{COMMON}\n{MATERIAL}\n{LIGHTLIB}\n{FORWARD}") }
}

/// The display stage: `vs_full`, `fs_display`.
pub fn display() -> Module {
    join("jup_display", DISPLAY)
}

/// Every module, for validation.
pub fn all() -> Vec<Module> {
    vec![material(), light(), forward(), display()]
}

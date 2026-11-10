//! Code constants: the values the engine itself puts in a pass's constant
//! buffers, by Ghosts' `CONST_SRC_CODE` index (`names::CODE_CONSTS`). Each
//! is one or more 16-byte rows; a matrix's rows are the rows of the matrix
//! that takes a column vector (the shaders dot a position with each row).

use std::collections::{BTreeSet, HashMap};

use glam::{Mat4, Vec3};

use crate::names::{CODE_CONSTS, CODE_TEXTURES};

/// A code constant's index by name (no prefix); panics on an unknown name
/// (names are fixed by the game).
pub fn code_index(name: &str) -> u16 {
    code_const(name).unwrap_or_else(|| panic!("no Ghosts code constant named {name}"))
}

pub fn code_const(name: &str) -> Option<u16> {
    // (A map: the frame sets about sixty constants by name, each a scan of
    // every name before. `BO2MP_IW6_CODE_SCAN=1`: the scan.)
    static INDEX: std::sync::OnceLock<HashMap<&'static str, u16>> = std::sync::OnceLock::new();
    static SCAN: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_CODE_SCAN");
    if SCAN.get() {
        return CODE_CONSTS.iter().position(|n| *n == name).map(|i| i as u16);
    }
    INDEX
        .get_or_init(|| {
            let mut m = HashMap::with_capacity(CODE_CONSTS.len());
            // (The first of equal names, as the scan finds it.)
            for (i, n) in CODE_CONSTS.iter().enumerate() {
                m.entry(*n).or_insert(i as u16);
            }
            m
        })
        .get(name)
        .copied()
}

pub fn code_const_name(i: u16) -> &'static str {
    CODE_CONSTS.get(usize::from(i)).copied().unwrap_or("?")
}

pub fn code_texture_name(i: u32) -> &'static str {
    CODE_TEXTURES.get(i as usize).copied().unwrap_or("?")
}

#[derive(Debug, Default)]
pub struct CodeValues {
    pub(crate) rows: HashMap<u16, Vec<[f32; 4]>>,
    /// Indices a pass asked for that nothing set (reported).
    pub missing: std::sync::Mutex<BTreeSet<u16>>,
    /// The frame's eye-relative view-projection and eye: objects' full
    /// matrices are made from these.
    pub view_projection: Mat4,
    pub eye: Vec3,
    /// Eye-relative world to view.
    pub rotation: Mat4,
}

impl Clone for CodeValues {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            missing: std::sync::Mutex::new(self.missing.lock().map(|m| m.clone()).unwrap_or_default()),
            view_projection: self.view_projection,
            eye: self.eye,
            rotation: self.rotation,
        }
    }

    /// A copy that reuses `self`'s allocations (the frame's passes copy the
    /// frame's values a dozen times).
    fn clone_from(&mut self, source: &Self) {
        self.copy_from(source);
        if let (Ok(mut mine), Ok(theirs)) = (self.missing.lock(), source.missing.lock()) {
            mine.clone_from(&theirs);
        }
    }
}

impl CodeValues {
    pub fn set(&mut self, name: &str, rows: Vec<[f32; 4]>) {
        self.rows.insert(code_index(name), rows);
    }

    /// Sets the constant to `rows`, reusing the allocation it already has
    /// (a constant set every frame allocates once).
    pub fn set_rows(&mut self, name: &str, rows: &[[f32; 4]]) {
        let v = self.rows.entry(code_index(name)).or_default();
        v.clear();
        v.extend_from_slice(rows);
    }

    pub fn set_vec(&mut self, name: &str, v: [f32; 4]) {
        self.set_rows(name, &[v]);
    }

    pub fn set_matrix(&mut self, name: &str, m: Mat4) {
        self.set_rows(name, &[m.row(0).to_array(), m.row(1).to_array(), m.row(2).to_array(), m.row(3).to_array()]);
    }

    pub fn get(&self, name: &str) -> Option<&[[f32; 4]]> {
        self.rows.get(&code_index(name)).map(Vec::as_slice)
    }

    /// Becomes a copy of `other`, reusing its row allocations (the
    /// reported missing indices are kept).
    pub fn copy_from(&mut self, other: &CodeValues) {
        self.rows.retain(|k, _| other.rows.contains_key(k));
        for (k, v) in &other.rows {
            let mine = self.rows.entry(*k).or_default();
            mine.clear();
            mine.extend_from_slice(v);
        }
        self.view_projection = other.view_projection;
        self.eye = other.eye;
        self.rotation = other.rotation;
    }

    /// Forgets the constants a pass asked for that nothing set.
    pub fn clear_missing(&self) {
        if let Ok(mut m) = self.missing.lock() {
            m.clear();
        }
    }

    /// Copies `other`'s constants over this set's (constants only this set
    /// has are kept; no allocation once the keys exist).
    pub fn overlay_from(&mut self, other: &CodeValues) {
        for (k, v) in &other.rows {
            let mine = self.rows.entry(*k).or_default();
            mine.clear();
            mine.extend_from_slice(v);
        }
        self.view_projection = other.view_projection;
        self.eye = other.eye;
        self.rotation = other.rotation;
    }

    /// Row `row` of constant `index` (zero when not set).
    pub fn row(&self, index: u16, row: usize) -> [f32; 4] {
        match self.rows.get(&index) {
            Some(r) => r.get(row).copied().unwrap_or([0.0; 4]),
            None => {
                if let Ok(mut m) = self.missing.lock() {
                    m.insert(index);
                }
                [0.0; 4]
            }
        }
    }
}

/// Engine texture slots a pass names by index (`TEXTURE_SRC_CODE_*`).
pub mod tex {
    pub const BLACK: u32 = 0;
    pub const WHITE: u32 = 1;
    pub const IDENTITY_NORMAL_MAP: u32 = 2;
    pub const MODEL_LIGHTING: u32 = 3;
    pub const LIGHTMAP_PRIMARY: u32 = 4;
    pub const LIGHTMAP_SECONDARY: u32 = 5;
    pub const SHADOWMAP_SUN: u32 = 6;
    pub const RESOLVED_SCENE: u32 = 13;
    pub const FLOATZ: u32 = 26;
    pub const REFLECTION_PROBE: u32 = 38;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_match_the_shader_notes() {
        // (The indices the shaders' arguments were matched against.)
        for (n, i) in [
            ("FOG", 170),
            ("FOG_COLOR_LINEAR", 171),
            ("FOG_SUN_CONSTS", 173),
            ("FOG_SUN_COLOR_LINEAR", 174),
            ("FOG_SUN_DIR", 176),
            ("FOG_HEIGHT", 177),
            ("ZNEAR", 159),
            ("LIGHTING_LOOKUP_SCALE", 160),
            ("INV_SCENE_PROJECTION", 161),
            ("HDR_SKY_INTENSITY", 167),
            ("SHADOWMAP_SWITCH_PARTITION", 157),
            ("SHADOWMAP_SCALE", 158),
            ("SUN_SHADOWMAP_PIXEL_ADJUST", 235),
            ("BLACK_LEVEL", 254),
            ("BASE_LIGHTING_COORDS", 255),
            ("EYEOFFSET", 62),
            ("TRANSPOSE_SHADOW_LOOKUP_MATRIX", 305),
            ("TRANSPOSE_WORLD_MATRIX0", 329),
        ] {
            assert_eq!(code_const(n), Some(i), "{n}");
        }
        assert_eq!(code_texture_name(tex::MODEL_LIGHTING), "MODEL_LIGHTING");
        assert_eq!(code_texture_name(tex::SHADOWMAP_SUN), "SHADOWMAP_SUN");
        assert_eq!(code_texture_name(tex::FLOATZ), "FLOATZ");
        assert_eq!(code_texture_name(40), "MACROCLUT");
        assert_eq!(code_texture_name(56), "DYNAMIC_IRIS_EXPOSURE");
    }
}

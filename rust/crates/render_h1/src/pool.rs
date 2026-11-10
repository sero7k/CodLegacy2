//! Techniques, technique sets and shader programs gathered across zones,
//! by name. A zone that only names an asset (`,name`) takes it from an
//! earlier zone; a later zone's own asset of a name replaces an earlier one.

use std::collections::HashMap;

use fastfile_h1::AssetType;
use fastfile_h1::walk::{Capture, TechniqueRec};

/// A name without the leading `,` a reference carries, lower case.
pub fn key(name: &str) -> String {
    name.trim_start_matches(',').to_ascii_lowercase()
}

#[derive(Default)]
pub struct Pool {
    /// Technique by name.
    pub techniques: HashMap<String, TechniqueRec>,
    /// Technique set by name: (slot, technique name) for each filled slot.
    pub techsets: HashMap<String, Vec<(usize, String)>>,
    /// Shader program (DXBC) by (stage, name).
    pub programs: HashMap<(AssetType, String), Vec<u8>>,
    /// Vertex declaration routing by name: (source, destination, mask).
    pub decls: HashMap<String, Vec<(u8, u8, u8)>>,
    /// Technique set's world vertex format (its vertex layer stream).
    pub world_vert_format: HashMap<String, u8>,
}

impl Pool {
    /// Adds what one zone holds itself (not what it only names).
    pub fn add(&mut self, cap: &Capture, image: &[u8]) {
        for t in &cap.techniques {
            if t.pass_info.is_empty() || t.name.starts_with(',') {
                continue;
            }
            self.techniques.insert(key(&t.name), t.clone());
        }
        for s in &cap.techsets {
            if s.name.starts_with(',') || s.techniques.is_empty() {
                continue;
            }
            let slots = s.techniques.iter().map(|(i, n, _)| (*i, key(n))).collect();
            self.techsets.insert(key(&s.name), slots);
            self.world_vert_format
                .insert(key(&s.name), s.world_vert_format);
        }
        for (name, _, routing) in &cap.vertex_decls {
            if !name.starts_with(',') && !routing.is_empty() {
                self.decls.insert(key(name), routing.clone());
            }
        }
        for (ty, name, span) in &cap.shader_programs {
            if span.len == 0 || name.starts_with(',') {
                continue;
            }
            if let Some(b) = image.get(span.pos..span.pos + span.len) {
                self.programs.insert((*ty, key(name)), b.to_vec());
            }
        }
    }

    pub fn technique(&self, name: &str) -> Option<&TechniqueRec> {
        self.techniques.get(&key(name))
    }

    /// The technique a set puts in `slot`.
    pub fn technique_in(&self, techset: &str, slot: usize) -> Option<&TechniqueRec> {
        let set = self.techsets.get(&key(techset))?;
        let (_, name) = set.iter().find(|(i, _)| *i == slot)?;
        self.technique(name)
    }

    pub fn program(&self, ty: AssetType, name: &str) -> Option<&[u8]> {
        self.programs.get(&(ty, key(name))).map(Vec::as_slice)
    }
}

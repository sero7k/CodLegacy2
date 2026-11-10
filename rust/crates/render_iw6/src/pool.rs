//! Techniques, technique sets and shader programs gathered across zones,
//! by name. A map zone's technique sets are name stubs (`,name`): the
//! `techsets_*` zones hold the techniques and their programs. A later
//! zone's own asset of a name does not replace an earlier one (the zones
//! are added most specific first).

use std::collections::HashMap;

use fastfile_iw6::AssetType;
use fastfile_iw6::walk::{AssetId, Capture, ShaderArgRec};

/// A name without the leading `,` a reference carries, lower case.
pub fn key(name: &str) -> String {
    name.trim_start_matches(',').to_ascii_lowercase()
}

/// One pass with its shaders named.
#[derive(Clone, Debug, Default)]
pub struct Pass {
    pub vertex_shader: Option<String>,
    pub pixel_shader: Option<String>,
    pub hull_shader: Option<String>,
    pub vertex_decl: Option<String>,
    /// Argument counts: per primitive, per object, stable.
    pub arg_counts: [u8; 3],
    pub args: Vec<ShaderArgRec>,
}

#[derive(Clone, Debug, Default)]
pub struct Technique {
    pub name: String,
    pub flags: u16,
    pub passes: Vec<Pass>,
}

#[derive(Default)]
pub struct Pool {
    pub techniques: HashMap<String, Technique>,
    /// Technique set by name: (slot, technique name) for each filled slot.
    pub techsets: HashMap<String, Vec<(usize, String)>>,
    /// Shader program (DXBC) by (stage, name).
    pub programs: HashMap<(AssetType, String), Vec<u8>>,
    /// Technique set's world vertex format (its vertex layer stream's).
    pub world_vert_format: HashMap<String, u8>,
}

impl Pool {
    /// Adds what one zone holds itself (not what it only names).
    pub fn add(&mut self, cap: &Capture, image: &[u8]) {
        let name = |id: Option<AssetId>| id.map(|i| key(cap.name_of(i)));
        for t in &cap.techniques {
            if t.passes.is_empty() || t.name.starts_with(',') {
                continue;
            }
            self.techniques.entry(key(&t.name)).or_insert_with(|| Technique {
                name: t.name.clone(),
                flags: t.flags,
                passes: t
                    .passes
                    .iter()
                    .map(|p| Pass {
                        vertex_shader: name(p.vertex_shader),
                        pixel_shader: name(p.pixel_shader),
                        hull_shader: name(p.hull_shader),
                        vertex_decl: name(p.vertex_decl),
                        arg_counts: p.arg_counts,
                        args: p.args.clone(),
                    })
                    .collect(),
            });
        }
        for s in &cap.techsets {
            if s.name.starts_with(',') || s.techniques.is_empty() {
                continue;
            }
            // (A slot's technique by the walk's own record of it: a
            // technique read once for several sets is named in each.)
            let slots: Vec<(usize, String)> = s
                .techniques
                .iter()
                .map(|(i, n, _)| {
                    let rec = s
                        .technique_ids
                        .iter()
                        .find(|(j, _)| j == i)
                        .and_then(|(_, t)| cap.techniques.get(*t as usize));
                    let k = match rec {
                        Some(t) if !t.name.is_empty() && !t.name.starts_with(',') => key(&t.name),
                        _ => key(n),
                    };
                    (*i, k)
                })
                .collect();
            self.techsets.entry(key(&s.name)).or_insert(slots);
            self.world_vert_format.entry(key(&s.name)).or_insert(s.world_vert_format);
        }
        for (id, span) in &cap.shader_programs {
            if span.len == 0 {
                continue;
            }
            let n = cap.name_of(*id);
            if n.starts_with(',') {
                continue;
            }
            if let Some(b) = image.get(span.pos..span.pos + span.len) {
                self.programs
                    .entry((id.ty, key(n)))
                    .or_insert_with(|| b.to_vec());
            }
        }
    }

    pub fn technique(&self, name: &str) -> Option<&Technique> {
        self.techniques.get(&key(name))
    }

    /// The technique a set puts in `slot`.
    pub fn technique_in(&self, techset: &str, slot: usize) -> Option<&Technique> {
        let set = self.techsets.get(&key(techset))?;
        let (_, name) = set.iter().find(|(i, _)| *i == slot)?;
        self.technique(name)
    }

    pub fn program(&self, ty: AssetType, name: &str) -> Option<&[u8]> {
        self.programs.get(&(ty, key(name))).map(Vec::as_slice)
    }
}

//! bo2mp: a walked effect with its bytes and names copied out of the zone, so
//! it outlives the zone's image (the shared zones' effects are kept this way,
//! and the map's are handed to the converter the same way).

use crate::walk::{Capture, FxRec, FxVisualRec, Span};

/// One visual of an element, its names resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OwnedVisual {
    #[default]
    None,
    Material(String),
    Model(String),
    Effect(String),
    Sound(String),
    /// A decal's two materials.
    Mark([Option<String>; 2]),
    /// A light definition, vector field or particle simulation.
    Other,
}

/// An element: its `FxElemDef` record and the arrays it points at.
#[derive(Clone, Debug, Default)]
pub struct OwnedElem {
    pub record: Vec<u8>,
    pub vel_samples: Vec<u8>,
    pub vis_samples: Vec<u8>,
    pub visuals: Vec<OwnedVisual>,
    pub effect_on_impact: Option<String>,
    pub effect_on_death: Option<String>,
    pub effect_emitted: Option<String>,
    /// A trail's `FxTrailDef` record, vertices and indices.
    pub trail: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    /// A spark fountain's or light's extended record.
    pub extended: Vec<u8>,
    /// A flare's `FxFlareDef` (112 bytes) followed by its four tables
    /// (screen x and y intensity, source cosine intensity and scale), each
    /// `intervalCount + 1` floats; empty for other elements.
    pub flare: Vec<u8>,
}

/// An effect with its 64-byte header and elements.
#[derive(Clone, Debug, Default)]
pub struct OwnedFx {
    pub name: String,
    pub header: Vec<u8>,
    pub elems: Vec<OwnedElem>,
}

impl OwnedFx {
    /// Whether the effect has looping elements (it repeats by itself:
    /// `elemDefCountLooping`, at byte 20 of the header).
    pub fn loops(&self) -> bool {
        self.header
            .get(20..24)
            .is_some_and(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) > 0)
    }
}

fn bytes(image: &[u8], s: Option<Span>) -> Vec<u8> {
    s.map_or_else(Vec::new, |s| s.get(image).to_vec())
}

/// Copies `f` out of the zone `cap` was walked from.
pub fn own(f: &FxRec, cap: &Capture, image: &[u8]) -> OwnedFx {
    let name_of = |id| cap.name_of(id).trim_start_matches(',').to_owned();
    let clean = |n: &Option<String>| n.as_ref().map(|n| n.trim_start_matches(',').to_owned());
    OwnedFx {
        name: f.name.trim_start_matches(',').to_owned(),
        header: f.record.get(image).to_vec(),
        elems: f
            .elems
            .iter()
            .map(|el| OwnedElem {
                record: el.record.get(image).to_vec(),
                vel_samples: bytes(image, el.vel_samples),
                vis_samples: bytes(image, el.vis_samples),
                visuals: el
                    .visuals
                    .iter()
                    .map(|v| match v {
                        FxVisualRec::None => OwnedVisual::None,
                        FxVisualRec::Other => OwnedVisual::Other,
                        FxVisualRec::Material(id) => OwnedVisual::Material(name_of(*id)),
                        FxVisualRec::Model(id) => OwnedVisual::Model(name_of(*id)),
                        FxVisualRec::Effect(n) => {
                            OwnedVisual::Effect(n.trim_start_matches(',').to_owned())
                        }
                        FxVisualRec::Sound(n) => OwnedVisual::Sound(n.clone()),
                        FxVisualRec::Mark(pair) => {
                            OwnedVisual::Mark(pair.map(|id| id.map(name_of)))
                        }
                    })
                    .collect(),
                effect_on_impact: clean(&el.effect_on_impact),
                effect_on_death: clean(&el.effect_on_death),
                effect_emitted: clean(&el.effect_emitted),
                trail: el.trail.map(|t| {
                    (
                        t.record.get(image).to_vec(),
                        bytes(image, t.verts),
                        bytes(image, t.inds),
                    )
                }),
                extended: bytes(image, el.extended),
                flare: el.flare.map_or_else(Vec::new, |f| {
                    let mut out = f.def.get(image).to_vec();
                    // (Each table takes its `count + 1` floats whether it was
                    // present or not, so the offsets hold.)
                    for (k, t) in f.tables.iter().enumerate() {
                        let count = out
                            .get(64 + 4 * k..68 + 4 * k)
                            .and_then(|b| b.try_into().ok())
                            .map_or(0, u32::from_le_bytes) as usize;
                        let mut bytes = t.get(image).to_vec();
                        bytes.resize(4 * (count + 1), 0);
                        out.extend_from_slice(&bytes);
                    }
                    out
                }),
            })
            .collect(),
    }
}

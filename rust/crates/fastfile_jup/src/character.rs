//! A Modern Warfare III operator's body or head model: its part zone's
//! LOD 0 ([`crate::gun::read_model`]: positions in the model-wide box its
//! skinned surfaces share, bones as the guns' with their name hashes) and
//! each vertex's bones and weights.
//!
//! A skinned surface (flag 0x0002) lists its vertices by bone count: those
//! of one bone, then of two, ... (`SurfaceRec::blend_counts`, +0x24). Its
//! stream 11 holds, group by group, one array per influence: the first
//! bone of each vertex of the group (u16 bone, u16 0: its weight is what
//! the others leave), then the second (u16 bone, u16 weight / 65536), and
//! so on. A bone is an index into the model's bones. A rigid surface binds
//! each of its ranges to one bone. Checked on Ghost's body: its densest
//! mesh's torso surface, 101, 1077 and 711 vertices of one, two and three
//! bones, 4388 words; the bones the spine's and clavicles'.
#![forbid(unsafe_code)]

use crate::gun::{self, PartModel};
use crate::surfs::stream;

/// A vertex's bones (indices into the model's) and weights, heaviest
/// first, summing to 1.
pub type Influences = Vec<(u16, f32)>;

/// An operator part model and its skin.
#[derive(Clone, Debug, Default)]
pub struct Character {
    pub model: PartModel,
    /// Per surface, per vertex.
    pub weights: Vec<Vec<Influences>>,
}

/// Read a character part zone (`mp_bd_body_*`, `mp_hd_head_*`).
pub fn read_character(z: &[u8]) -> Result<Character, String> {
    let (model, rec) = gun::read_model(z)?;
    let (at, len) = rec.blob;
    let blob = z.get(at..at + len).ok_or("mesh blob past the zone")?;
    let word = |o: usize| -> Option<(u16, u16)> {
        let b = blob.get(o..o + 4)?;
        Some((u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]])))
    };
    let mut weights = Vec::with_capacity(rec.surfaces.len());
    for s in &rec.surfaces {
        let n = s.vert_count as usize;
        let mut w: Vec<Influences> = vec![Vec::new(); n];
        let grouped: usize = s.blend_counts.iter().map(|&c| usize::from(c)).sum();
        match s.streams[stream::SKIN] {
            Some(o) if grouped == n => {
                let mut i = 0usize;
                let mut first = 0usize;
                for (k, &c) in s.blend_counts.iter().enumerate() {
                    let c = usize::from(c);
                    for slot in 0..=k {
                        for v in first..first + c {
                            let (bone, weight) = word(o as usize + 4 * i).ok_or("skin stream past the mesh")?;
                            i += 1;
                            w[v].push((bone, if slot == 0 { 0.0 } else { f32::from(weight) / 65536.0 }));
                        }
                    }
                    first += c;
                }
                for infl in &mut w {
                    let rest: f32 = infl.iter().skip(1).map(|x| x.1).sum();
                    if let Some(f) = infl.first_mut() {
                        f.1 = (1.0 - rest).max(0.0);
                    }
                    infl.sort_by(|a, b| b.1.total_cmp(&a.1));
                }
            }
            _ => {
                for r in &s.ranges {
                    let from = usize::from(r.first_vert);
                    for v in from..(from + usize::from(r.vert_count)).min(n) {
                        w[v] = vec![(r.bone, 1.0)];
                    }
                }
            }
        }
        for infl in &mut w {
            if infl.is_empty() {
                infl.push((0, 1.0));
            }
        }
        weights.push(w);
    }
    // (The model's surfaces are its LOD 0's: the same count, the same order.)
    if weights.len() != model.surfaces.len() {
        return Err("skin and surfaces differ in number".into());
    }
    Ok(Character { model, weights })
}

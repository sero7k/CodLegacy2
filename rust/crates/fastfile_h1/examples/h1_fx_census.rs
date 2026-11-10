//! A zone's effects in numbers: element types, lit types, lighting
//! fractions, emissive scales, looping counts and the per-element random
//! seeds (`docs/H1_NATIVE.md`, Effects):
//! `cargo run -p fastfile_h1 --release --example h1_fx_census -- <zone.ff>...`

use std::collections::BTreeMap;

use fastfile_h1::layout::{FxElemDef as E, FxElemVisStateSample as V};

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn main() {
    for path in std::env::args().skip(1) {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(&path)).expect("zone");
        let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
        println!("== {path}: {} effects (stop {:?})", cap.fx.len(), stop.map(|s| s.asset_index));
        let mut types: BTreeMap<u8, usize> = BTreeMap::new();
        let mut lit: BTreeMap<(u8, u8), usize> = BTreeMap::new();
        let mut loop_counts: BTreeMap<i32, usize> = BTreeMap::new();
        let (mut hdr_frac, mut emis_max, mut col_max, mut n_vis) = (BTreeMap::new(), 0.0f32, 0.0f32, 0usize);
        let mut flags_or = 0u32;
        let mut seeds = Vec::new();
        for f in &cap.fx {
            if f.name.starts_with(',') {
                continue;
            }
            for (k, el) in f.elems.iter().enumerate() {
                let r = el.record.get(&zone.image);
                let t = r[E::elemType];
                *types.entry(t).or_default() += 1;
                *lit.entry((r[E::elemLitType], t)).or_default() += 1;
                if (k as i32) < f.elem_counts.0 {
                    *loop_counts.entry(i32_at(r, E::spawn + 4)).or_default() += 1;
                }
                let h = f32_at(r, E::hdrLightingFrac);
                *hdr_frac.entry(format!("lit{} frac{} hdr{h:.2} ess{:.2}", r[E::elemLitType], r[E::lightingFrac], f32_at(r, E::emissiveScaleScale))).or_insert(0usize) += 1;
                flags_or |= i32_at(r, E::flags) as u32;
                seeds.push(i32_at(r, E::randomSeed));
                if let Some(s) = el.vis_samples {
                    for smp in s.get(&zone.image).chunks_exact(V::SIZE) {
                        n_vis += 1;
                        for half in [V::base, V::amplitude] {
                            for c in 0..3 {
                                col_max = col_max.max(f32_at(smp, half + V::base__color + 4 * c));
                                emis_max = emis_max.max(f32_at(smp, half + V::base__emissiveScale + 4 * c));
                            }
                        }
                    }
                }
            }
        }
        println!("types {types:?}");
        println!("(litType, type) {lit:?}");
        println!("looping counts {loop_counts:?}");
        let mut hf: Vec<_> = hdr_frac.into_iter().collect();
        hf.sort_by(|a, b| b.1.cmp(&a.1));
        println!("lighting {:?}", &hf[..hf.len().min(25)]);
        println!("vis samples {n_vis}: colour max {col_max}, emissive max {emis_max}; flags seen {flags_or:#x}");
        seeds.sort();
        println!("random seeds: {} (min {:?} max {:?})", seeds.len(), seeds.first(), seeds.last());
    }
}

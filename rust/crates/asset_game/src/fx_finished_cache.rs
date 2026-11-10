//! Finished effect catalog for the gun cache.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Snap {
    gaps: usize,
    lights: Vec<((String, u8), T7FxLight)>,
    defs: Vec<(String, OwnedFxEffectDef)>,
}

pub fn export_fx(fx: &FxCatalog) -> std::result::Result<Vec<u8>, String> {
    let snap = Snap {
        gaps: fx.published.capture_gaps,
        lights: fx
            .published
            .t7_lights
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        defs: fx
            .published
            .defs
            .iter()
            .zip(fx.published.zones.iter())
            .map(|(def, zone)| (zone.as_str().to_owned(), def.clone()))
            .collect(),
    };
    serde_json::to_vec(&snap).map_err(|e| e.to_string())
}

pub fn import_fx(bytes: &[u8]) -> Option<FxCatalog> {
    let snap: Snap = serde_json::from_slice(bytes).ok()?;
    let mut fx = FxCatalog::default();
    fx.published.capture_gaps = snap.gaps;
    fx.published.t7_lights = snap.lights.into_iter().collect();
    for (zone, def) in snap.defs {
        fx.set_capture_zone(ZoneOwner::intern(&zone));
        fx.set_capture_ns(def.namespace);
        fx.insert_owned(def);
    }
    Some(fx)
}

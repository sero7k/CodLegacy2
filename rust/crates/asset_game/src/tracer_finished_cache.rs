//! Finished tracer catalog for the gun cache.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Snap {
    gaps: usize,
    defs: Vec<(String, OwnedTracerDef)>,
}

pub fn export_tracers(tracers: &TracerCatalog) -> std::result::Result<Vec<u8>, String> {
    let snap = Snap {
        gaps: tracers.published.capture_gaps,
        defs: tracers
            .published
            .defs
            .iter()
            .zip(tracers.published.zones.iter())
            .map(|(def, zone)| (zone.as_str().to_owned(), def.clone()))
            .collect(),
    };
    serde_json::to_vec(&snap).map_err(|e| e.to_string())
}

pub fn import_tracers(bytes: &[u8]) -> Option<TracerCatalog> {
    let snap: Snap = serde_json::from_slice(bytes).ok()?;
    let mut tracers = TracerCatalog::default();
    tracers.published.capture_gaps = snap.gaps;
    for (zone, def) in snap.defs {
        tracers.set_capture_zone(ZoneOwner::intern(&zone));
        tracers.set_capture_ns(def.namespace);
        tracers.published.insert_owned(def);
    }
    Some(tracers)
}

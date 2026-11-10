//! bo2mp: the host's weapon layout, sent to every client joining its match.
//! Each machine numbers weapons from the games it has installed; a client
//! lays its weapons out in the host's order (`WeaponRegistry::conformed`),
//! and a gun whose game it lacks is a Black Ops II gun of the same kind
//! carrying the host's numbers, so it predicts as the host simulates.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use weapon_iw4::WeaponCombatFacts;

use crate::EquipmentRuntimeFacts;

/// A match: the room's session and its match epoch.
pub type MatchId = ([u8; 16], u32);

/// A weapon row as the host has it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestRow {
    pub namespace: String,
    pub name: String,
    pub attachments: Vec<String>,
    pub alternate: bool,
    /// Another game's gun (one a client may lack): what a stand-in takes.
    pub guest: Option<GuestFacts>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuestFacts {
    pub weap_type: i32,
    pub weap_class: i32,
    pub inventory_type: i32,
    pub offhand_class: i32,
    pub script_name: String,
    pub display_key: String,
    /// Its name as the host's game shows it (the client may have no text
    /// for the key).
    pub display_text: String,
    pub combat: WeaponCombatFacts,
    pub equipment: EquipmentRuntimeFacts,
    pub scales: (f32, f32, f32),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GuestManifest {
    /// Rows 1.. in the host's order.
    pub rows: Vec<ManifestRow>,
    /// The host's tables over every row (0..): which weapons may run and
    /// their attachment-switch groups.
    pub runnable: Vec<bool>,
    pub transition_groups: Vec<u32>,
}

const MAGIC: &[u8; 4] = b"BO2W";
const HEADER: usize = 4 + 16 + 4 + 2 + 2;
/// A part's data (under a bootstrap stream's budget).
const PART: usize = 192 * 1024;

impl GuestManifest {
    fn encode(&self) -> Option<Vec<u8>> {
        let json = serde_json::to_vec(self).ok()?;
        Some(miniz_oxide::deflate::compress_to_vec_zlib(&json, 6))
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let json = miniz_oxide::inflate::decompress_to_vec_zlib(bytes).ok()?;
        serde_json::from_slice(&json).ok()
    }

    /// The manifest as the parts sent to a client.
    pub fn parts(&self, id: MatchId) -> Vec<Vec<u8>> {
        let Some(bytes) = self.encode() else {
            return Vec::new();
        };
        let chunks: Vec<&[u8]> = bytes.chunks(PART).collect();
        let count = chunks.len() as u16;
        chunks
            .into_iter()
            .enumerate()
            .map(|(i, data)| {
                let mut out = Vec::with_capacity(HEADER + data.len());
                out.extend_from_slice(MAGIC);
                out.extend_from_slice(&id.0);
                out.extend_from_slice(&id.1.to_le_bytes());
                out.extend_from_slice(&(i as u16).to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(data);
                out
            })
            .collect()
    }
}

/// Whether a host-to-client payload is a part of a manifest.
pub fn is_part(payload: &[u8]) -> bool {
    payload.len() >= HEADER && &payload[..4] == MAGIC
}

#[derive(Default)]
struct Received {
    id: Option<MatchId>,
    parts: Vec<Option<Vec<u8>>>,
    done: Option<Arc<GuestManifest>>,
}

/// A match's manifest, as the parts sent.
pub type HostParts = (MatchId, Arc<Vec<Vec<u8>>>);

static HOST: Mutex<Option<HostParts>> = Mutex::new(None);
static RECEIVED: Mutex<Option<Received>> = Mutex::new(None);

/// The host's manifest for its match (sent to each client by the relay).
pub fn publish_host(id: MatchId, manifest: &GuestManifest) {
    let parts = manifest.parts(id);
    diag::info!(
        Net,
        "bo2mp guest guns: weapon layout for joining clients: {} rows, {} guest, {} parts ({} bytes)",
        manifest.rows.len(),
        manifest.rows.iter().filter(|r| r.guest.is_some()).count(),
        parts.len(),
        parts.iter().map(Vec::len).sum::<usize>()
    );
    if let Ok(mut host) = HOST.lock() {
        *host = Some((id, Arc::new(parts)));
    }
}

pub fn clear_host() {
    if let Ok(mut host) = HOST.lock() {
        *host = None;
    }
}

pub fn host_parts() -> Option<HostParts> {
    HOST.lock().ok().and_then(|h| h.clone())
}

/// A part from the host.
pub fn receive_part(payload: &[u8]) {
    if !is_part(payload) {
        return;
    }
    let mut session = [0_u8; 16];
    session.copy_from_slice(&payload[4..20]);
    let epoch = u32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]);
    let index = usize::from(u16::from_le_bytes([payload[24], payload[25]]));
    let count = usize::from(u16::from_le_bytes([payload[26], payload[27]]));
    let id = (session, epoch);
    let Ok(mut slot) = RECEIVED.lock() else { return };
    let r = slot.get_or_insert_with(Received::default);
    if r.id != Some(id) || r.parts.len() != count {
        *r = Received {
            id: Some(id),
            parts: vec![None; count],
            done: None,
        };
    }
    if r.done.is_some() || index >= count {
        return;
    }
    r.parts[index] = Some(payload[HEADER..].to_vec());
    if r.parts.iter().all(Option::is_some) {
        let bytes: Vec<u8> = r.parts.iter().flatten().flatten().copied().collect();
        match GuestManifest::decode(&bytes) {
            Some(m) => {
                diag::info!(Net, "bo2mp guest guns: the host's weapon layout arrived: {} rows", m.rows.len());
                r.done = Some(Arc::new(m));
            }
            None => {
                diag::warn!(Net, "bo2mp guest guns: the host's weapon layout did not decode");
                r.parts = vec![None; count];
            }
        }
    }
}

/// The host's manifest for this match, once all of it arrived.
pub fn received(id: MatchId) -> Option<Arc<GuestManifest>> {
    let slot = RECEIVED.lock().ok()?;
    let r = slot.as_ref()?;
    (r.id == Some(id)).then(|| r.done.clone()).flatten()
}

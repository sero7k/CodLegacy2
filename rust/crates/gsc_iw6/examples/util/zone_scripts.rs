#![allow(dead_code)]
//! Shared by the examples: the scripts of a Ghosts zone.
use std::io::Read;

pub struct ZoneScript {
    pub name: String,
    pub stream: Vec<u8>,
    pub code: Vec<u8>,
}

pub struct ZoneScripts {
    pub walked: usize,
    pub assets: usize,
    /// Scripts the zone header lists.
    pub listed: usize,
    pub stop: Option<String>,
    pub scripts: Vec<ZoneScript>,
}

pub fn load(path: &str) -> Result<ZoneScripts, String> {
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(path)).map_err(|e| format!("{e:?}"))?;
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    let listed = zone
        .assets
        .assets
        .iter()
        .filter(|a| a.asset_type() == Some(fastfile_iw6::AssetType::ScriptFile))
        .count();
    let mut scripts = Vec::new();
    for sf in &cap.script_files {
        let (Some(b), Some(c)) = (sf.buffer, sf.bytecode) else { continue };
        let mut stream = Vec::new();
        if flate2::read::ZlibDecoder::new(b.get(&zone.image)).read_to_end(&mut stream).is_err() {
            continue;
        }
        scripts.push(ZoneScript { name: sf.name.clone(), stream, code: c.get(&zone.image).to_vec() });
    }
    Ok(ZoneScripts {
        walked: cap.walked,
        assets: zone.assets.assets.len(),
        listed,
        stop: stop.map(|s| s.error.to_string()),
        scripts,
    })
}

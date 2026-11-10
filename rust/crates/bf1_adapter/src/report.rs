//! Opens a BF1 level through the vendored readers and says what is in it:
//! the BF1 counterpart of the t7 world report. Read-only; heavy (indexes
//! every `cas.cat` of the install, parses the level's asset files), so call
//! it off the main thread ([`spawn_logged`]).

use std::path::{Path, PathBuf};
use std::time::Instant;

use fb_level::{Walker, World};
use fb_vfs::GameFs;

use crate::catalog::{self, KnownMap};
use crate::config::Bf1Error;

/// Sub-worlds of game modes, left out of the walk as the upstream viewer does.
const SKIPPED_SUBWORLDS: [&str; 7] = ["breakthrough0", "rush0", "tugofwar0", "dominiation0", "possession0", "teamdeathmatch0", "autotests"];

#[derive(Clone, Debug, Default)]
pub struct TerrainSummary {
    pub height_tiles: usize,
    pub colour_tiles: usize,
    pub has_layer_masks: bool,
    /// Terrain bounds in Frostbite metres (x, y up, z).
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub height_scale: f32,
    /// Height at the middle of the bounds, if a tile covers it.
    pub centre_height: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct WorldReport {
    pub map: String,
    pub install: PathBuf,
    pub sha1_indexed: usize,
    pub partitions: usize,
    pub parse_failures: usize,
    pub placed_meshes: usize,
    pub distinct_meshes: usize,
    pub subworlds: Vec<String>,
    pub top_entity_types: Vec<(String, usize)>,
    pub terrain: Option<TerrainSummary>,
    pub terrain_error: Option<String>,
    pub seconds: f32,
}

impl WorldReport {
    /// The report as log lines.
    pub fn lines(&self) -> Vec<String> {
        let mut l = vec![
            format!("bf1 world report: {} from {}", self.map, self.install.display()),
            format!("  archive index: {} sha1 entries; level data: {} asset files ({} unreadable)", self.sha1_indexed, self.partitions, self.parse_failures),
            format!("  objects: {} placed meshes, {} distinct mesh assets, {} sub-worlds walked", self.placed_meshes, self.distinct_meshes, self.subworlds.len()),
        ];
        if !self.top_entity_types.is_empty() {
            let s: Vec<String> = self.top_entity_types.iter().map(|(t, n)| format!("{t} x{n}")).collect();
            l.push(format!("  entities that place no mesh (spawns, logic, sound ...), most common: {}", s.join(", ")));
        }
        match (&self.terrain, &self.terrain_error) {
            (Some(t), _) => {
                l.push(format!(
                    "  terrain: {} height tiles, {} colour tiles, layer masks {}; bounds {:.0},{:.0},{:.0} .. {:.0},{:.0},{:.0} m, {:.4} m/step; centre height {}",
                    t.height_tiles,
                    t.colour_tiles,
                    if t.has_layer_masks { "read" } else { "NOT read" },
                    t.min[0], t.min[1], t.min[2], t.max[0], t.max[1], t.max[2],
                    t.height_scale,
                    t.centre_height.map_or_else(|| "none".to_owned(), |h| format!("{h:.1} m")),
                ));
            }
            (None, Some(e)) => l.push(format!("  terrain: NOT read: {e}")),
            (None, None) => l.push("  terrain: not attempted".to_owned()),
        }
        l.push(format!("  done in {:.1} s. Nothing is drawn or played: see docs/BF1.md for what remains.", self.seconds));
        l
    }
}

/// Opens `map` in the install at `root` and reports it.
pub fn open_and_report(root: &Path, map: &KnownMap) -> Result<WorldReport, Bf1Error> {
    let started = Instant::now();
    let read = |e: anyhow::Error| Bf1Error::Read(format!("{e:#}"));
    let mut fs = GameFs::open(root).map_err(read)?;
    fs.index_toc_chunks().map_err(read)?;
    let world = World::load(&fs, &[map.superbundle]).map_err(read)?;
    let mut walker = Walker::new(&world, &fs);
    walker.skip_subworlds = SKIPPED_SUBWORLDS.iter().map(|s| (*s).to_owned()).collect();
    walker.walk_level(map.level).map_err(read)?;
    let distinct: std::collections::HashSet<_> = walker.meshes.iter().filter_map(|m| m.mesh_at).collect();
    let mut others: Vec<(String, usize)> = walker.stats.other_entities.iter().map(|(k, v)| (k.clone(), *v)).collect();
    others.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    others.truncate(6);
    let (terrain, terrain_error) = match load_terrain(&fs, &world) {
        Ok(t) => (Some(t), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    Ok(WorldReport {
        map: format!("bf1:{}", map.id),
        install: root.to_path_buf(),
        sha1_indexed: fs.index.len(),
        partitions: world.partitions.len(),
        parse_failures: world.parse_failures,
        placed_meshes: walker.meshes.len(),
        distinct_meshes: distinct.len(),
        subworlds: walker.stats.subworlds.clone(),
        top_entity_types: others,
        terrain,
        terrain_error,
        seconds: started.elapsed().as_secs_f32(),
    })
}

/// The terrain, as the upstream viewer reads it (`TerrainData` ->
/// streaming tree resource -> height/colour tiles).
fn load_terrain(fs: &GameFs, world: &World) -> anyhow::Result<TerrainSummary> {
    use anyhow::Context;
    let part = (0..world.partitions.len())
        .find(|&i| world.partitions[i].primary().is_some_and(|p| p.value.type_name == "TerrainData"))
        .context("the level has no TerrainData")?;
    let Some(fb_ebx::Value::ResourceRef(rid)) = world.get((part, 0)).get("TerrainStreamingTreeResource") else {
        anyhow::bail!("TerrainData has no TerrainStreamingTreeResource")
    };
    let tree = world.read_res(fs, *rid)?;
    let (terrain, _stats) = fb_terrain::Terrain::load(
        &tree,
        |id| world.chunks.contains_key(id) || fs.toc_chunks.contains_key(id),
        |id| world.read_chunk(fs, id),
    )?;
    let centre = ((terrain.min[0] + terrain.max[0]) * 0.5, (terrain.min[2] + terrain.max[2]) * 0.5);
    Ok(TerrainSummary {
        height_tiles: terrain.tiles.len(),
        colour_tiles: terrain.colours.len(),
        has_layer_masks: terrain.layers.is_some(),
        min: terrain.min,
        max: terrain.max,
        height_scale: terrain.height_scale,
        centre_height: terrain.height_at(centre.0, centre.1),
    })
}

/// The `bf1:` request path: resolves the install (`configured` is the
/// `game_paths.cfg` line), then runs the report once on a background thread
/// and hands each log line to `log`. Returns the refusal to show now: the
/// config error when there is no usable install, else [`crate::NOT_PLAYABLE`].
pub fn refuse_and_report(spec: &str, configured: Option<PathBuf>, log: impl Fn(String) + Send + 'static) -> String {
    let map = match catalog::parse_spec(spec) {
        Ok(m) => m,
        Err(e) => return e.to_string(),
    };
    let root = match crate::config::resolve_from_env(configured.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.to_string(),
    };
    static RAN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let first = RAN.lock().is_ok_and(|mut r| {
        let fresh = !r.iter().any(|m| m == map.id);
        if fresh {
            r.push(map.id.to_owned());
        }
        fresh
    });
    if first {
        let spawned = std::thread::Builder::new().name("bf1-report".into()).spawn(move || match open_and_report(&root, map) {
            Ok(r) => r.lines().into_iter().for_each(&log),
            Err(e) => log(format!("bf1 world report failed: {e}")),
        });
        if spawned.is_err() {
            return "Battlefield 1: could not start the world report thread".to_owned();
        }
    }
    crate::NOT_PLAYABLE.to_owned()
}

use std::collections::{BTreeMap, BTreeSet};

/// bo2zm M3: a Black Ops II map's own compiled scripts, string tables and
/// entity strings, for the BO2 script runtime.
#[derive(Clone, Debug, Default)]
pub struct T6ScriptSet {
    /// Compiled script objects in zone load order (later replace earlier).
    pub objects: Vec<Vec<u8>>,
    /// bo2mp: a Black Ops III Zombies map's own compiled scripts (its patch
    /// zones'), in zone load order; when set the match runs them instead
    /// of `objects` (`sim::t6::t7`).
    pub t7_objects: Vec<Vec<u8>>,
    /// bo2mp: that map's nav data, its zone's Havok packfiles holding a nav
    /// mesh, one after another (`sim::t6::t7_ai`).
    pub t7_nav: Vec<u8>,
    /// bo2mp: that map's zones' script bundles: (name, pairs as (object,
    /// -1 for the bundle's own; key; kind; value; data word)).
    pub t7_bundles: Vec<(String, Vec<(i32, String, u32, String, u32)>)>,
    /// bo2mp: that map's AI types' characters (`lane::t7::map_props`).
    pub t7_cast: Vec<T7AiCast>,
    /// (name, columns, rows, cells row-major), later zones last.
    pub tables: Vec<(String, usize, usize, Vec<String>)>,
    pub entities: Vec<String>,
    /// The map's AI path nodes (GameWorldMp), in node order.
    pub path_nodes: Vec<asset_t6::PathNodeRef>,
    /// bo2mp: the map's baked node sight and smooth tables.
    pub path_tables: asset_t6::PathTablesRef,
    /// Every animation's timing, root motion and notetracks.
    pub anims: Vec<T6AnimFacts>,
    /// Animation state definitions (`animstatedefs/*.asd`): (name, text).
    pub animstatedefs: Vec<(String, String)>,
    /// The language zones' strings (`ZOMBIE_WEAPON_M14` -> its English
    /// text), later zones winning a name.
    pub strings: Vec<(String, String)>,
    /// Every sound alias the map's banks hold (lower case).
    pub sound_aliases: Vec<String>,
    /// bo2mp: the game type settings files (`mp/gamesettings_default.cfg`,
    /// `mp/gamesettings_tdm.cfg`, ...): (name, text), later zones last.
    pub gamesettings: Vec<(String, String)>,
    /// bo2mp: each sound alias's length in ms (lower-case name).
    pub sound_lengths: Vec<(String, u32)>,
    /// bo2mp: weapons whose weapon file marks them retrievable (bRetrievable).
    pub retrievable_weapons: Vec<String>,
    /// bo2mp: each vehicle's turret weapon and gunner weapons (the
    /// scorestreak helicopters' guns) and how it drives, later zones
    /// replacing earlier.
    pub vehicles: Vec<asset_t6::VehicleRef>,
    /// Screen shakes and blurs (`shock/NAME.shock`): (name, text), later
    /// zones replacing earlier (bo2mp: BO2's shellshock files).
    pub shocks: Vec<(String, String)>,
    /// bo2mp: the third-person player animation script and its anim type
    /// list (`mp/playeranim.script`, `mp/playeranimtypes.txt`).
    pub playeranim: Option<(String, String)>,
    /// Props that break in stages, by name (the mannequins, cars, clock
    /// that break piece by piece when shot), later zones replacing earlier.
    pub destructibles: Vec<std::sync::Arc<xmodel_runtime::T5DestructibleDef>>,
    /// bo2mp: the ragdoll definitions (`ragdoll.cfg`), how a dead body
    /// goes limp.
    pub ragdoll: Option<String>,
    /// bo2zm: barrier types (`ZBarrierDef`), later zones winning a name.
    pub zbarriers: Vec<asset_t6::ZBarrierDefRef>,
}

impl T6ScriptSet {
    pub fn export_finished(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&T6Blob::from(self)).map_err(|e| e.to_string())
    }

    pub fn import_finished(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice::<T6Blob>(bytes).ok().map(T6Blob::into_set)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct T6Blob {
    objects: Vec<Vec<u8>>,
    t7_objects: Vec<Vec<u8>>,
    t7_nav: Vec<u8>,
    t7_bundles: Vec<(String, Vec<(i32, String, u32, String, u32)>)>,
    t7_cast: Vec<T7AiCast>,
    tables: Vec<(String, usize, usize, Vec<String>)>,
    entities: Vec<String>,
    path_nodes: Vec<asset_t6::PathNodeRef>,
    path_tables: asset_t6::PathTablesRef,
    anims: Vec<T6AnimFacts>,
    animstatedefs: Vec<(String, String)>,
    strings: Vec<(String, String)>,
    sound_aliases: Vec<String>,
    gamesettings: Vec<(String, String)>,
    sound_lengths: Vec<(String, u32)>,
    retrievable_weapons: Vec<String>,
    vehicles: Vec<VehicleBlob>,
    shocks: Vec<(String, String)>,
    playeranim: Option<(String, String)>,
    destructibles: Vec<xmodel_runtime::T5DestructibleDef>,
    ragdoll: Option<String>,
    zbarriers: Vec<asset_t6::ZBarrierDefRef>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VehicleBlob {
    name: String,
    turret_weapon: String,
    gunner_weapons: Vec<String>,
    camera_mode: i32,
    max_speed: f32,
    max_speed_vertical: f32,
    accel: f32,
    accel_vertical: f32,
    reverse_scale: f32,
    camera_range: f32,
    camera_height: f32,
    camera_pitch: [f32; 2],
    camera_fov: f32,
    turret_pitch: [f32; 2],
    third_person_driver: i32,
    buttons: [String; 5],
}

impl T6Blob {
    fn from(set: &T6ScriptSet) -> Self {
        Self {
            objects: set.objects.clone(),
            t7_objects: set.t7_objects.clone(),
            t7_nav: set.t7_nav.clone(),
            t7_bundles: set.t7_bundles.clone(),
            t7_cast: set.t7_cast.clone(),
            tables: set.tables.clone(),
            entities: set.entities.clone(),
            path_nodes: set.path_nodes.clone(),
            path_tables: set.path_tables.clone(),
            anims: set.anims.clone(),
            animstatedefs: set.animstatedefs.clone(),
            strings: set.strings.clone(),
            sound_aliases: set.sound_aliases.clone(),
            gamesettings: set.gamesettings.clone(),
            sound_lengths: set.sound_lengths.clone(),
            retrievable_weapons: set.retrievable_weapons.clone(),
            vehicles: set.vehicles.iter().map(VehicleBlob::from_ref).collect(),
            shocks: set.shocks.clone(),
            playeranim: set.playeranim.clone(),
            destructibles: set.destructibles.iter().map(|d| (**d).clone()).collect(),
            ragdoll: set.ragdoll.clone(),
            zbarriers: set.zbarriers.clone(),
        }
    }

    fn into_set(self) -> T6ScriptSet {
        T6ScriptSet {
            objects: self.objects,
            t7_objects: self.t7_objects,
            t7_nav: self.t7_nav,
            t7_bundles: self.t7_bundles,
            t7_cast: self.t7_cast,
            tables: self.tables,
            entities: self.entities,
            path_nodes: self.path_nodes,
            path_tables: self.path_tables,
            anims: self.anims,
            animstatedefs: self.animstatedefs,
            strings: self.strings,
            sound_aliases: self.sound_aliases,
            gamesettings: self.gamesettings,
            sound_lengths: self.sound_lengths,
            retrievable_weapons: self.retrievable_weapons,
            vehicles: self.vehicles.into_iter().map(VehicleBlob::into_ref).collect(),
            shocks: self.shocks,
            playeranim: self.playeranim,
            destructibles: self.destructibles.into_iter().map(std::sync::Arc::new).collect(),
            ragdoll: self.ragdoll,
            zbarriers: self.zbarriers,
        }
    }
}

impl VehicleBlob {
    fn from_ref(v: &asset_t6::VehicleRef) -> Self {
        let d = v.drive;
        Self {
            name: v.name.clone(),
            turret_weapon: v.turret_weapon.clone(),
            gunner_weapons: v.gunner_weapons.clone(),
            camera_mode: d.camera_mode,
            max_speed: d.max_speed,
            max_speed_vertical: d.max_speed_vertical,
            accel: d.accel,
            accel_vertical: d.accel_vertical,
            reverse_scale: d.reverse_scale,
            camera_range: d.camera_range,
            camera_height: d.camera_height,
            camera_pitch: d.camera_pitch,
            camera_fov: d.camera_fov,
            turret_pitch: d.turret_pitch,
            third_person_driver: d.third_person_driver,
            buttons: d.buttons.map(str::to_owned),
        }
    }

    fn into_ref(self) -> asset_t6::VehicleRef {
        asset_t6::VehicleRef {
            name: self.name,
            turret_weapon: self.turret_weapon,
            gunner_weapons: self.gunner_weapons,
            drive: asset_t6::VehicleDrive {
                camera_mode: self.camera_mode,
                max_speed: self.max_speed,
                max_speed_vertical: self.max_speed_vertical,
                accel: self.accel,
                accel_vertical: self.accel_vertical,
                reverse_scale: self.reverse_scale,
                camera_range: self.camera_range,
                camera_height: self.camera_height,
                camera_pitch: self.camera_pitch,
                camera_fov: self.camera_fov,
                turret_pitch: self.turret_pitch,
                third_person_driver: self.third_person_driver,
                buttons: self.buttons.map(|s| intern_static(&s)),
            },
        }
    }
}

fn intern_static(s: &str) -> &'static str {
    match s {
        "" => "",
        "+gostand" => "+gostand",
        "+stance" => "+stance",
        "+usereload" => "+usereload",
        "+weapnext_inventory" => "+weapnext_inventory",
        "+smoke" => "+smoke",
        "+frag" => "+frag",
        "+breath_sprint" => "+breath_sprint",
        "+melee" => "+melee",
        "+speed_throw" => "+speed_throw",
        "+attack" => "+attack",
        other => Box::leak(other.to_owned().into_boxed_str()),
    }
}

/// bo2zm M3: what the server needs of an animation.
/// bo2mp: a Black Ops III character (an AI type's look): its body, the
/// heads one is picked from, and the models attached to it (hat, tie).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct T7Character {
    pub name: String,
    pub body: String,
    pub heads: Vec<String>,
    pub attached: Vec<String>,
}

/// bo2mp: an AI type (`spawner_zm_zod_zombie`) and its characters, one
/// picked per actor spawned.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct T7AiCast {
    pub aitype: String,
    pub characters: Vec<T7Character>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct T6AnimFacts {
    pub name: String,
    pub numframes: u16,
    pub framerate: f32,
    pub looping: bool,
    /// Root translation keys: (frame, position).
    pub delta_trans: Vec<(u16, [f32; 3])>,
    /// Root turn keys: (frame, quaternion x y z w).
    pub delta_quat: Vec<(u16, [f32; 4])>,
    /// Notetracks: (name, time 0..1).
    pub notifies: Vec<(String, f32)>,
}

#[derive(Clone, Debug, Default)]
pub struct ScriptSources {
    /// bo2zm M3: set for a Black Ops II map.
    pub t6: Option<T6ScriptSet>,
    sources: BTreeMap<String, Result<Vec<u8>, String>>,
    tables: BTreeMap<String, ScriptTable>,
    entities: Option<String>,
    configs: BTreeMap<String, String>,
    /// bo2mp: another game's map's own AI path nodes, for BO2's rules.
    pub foreign_path_nodes: Vec<asset_t6::PathNodeRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScriptTable {
    pub columns: usize,
    pub rows: usize,
    pub cells: Vec<String>,
}

fn normalize(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

impl ScriptSources {
    pub fn read(&self, module: &str) -> Result<Vec<u8>, String> {
        self.sources
            .get(module)
            .cloned()
            .unwrap_or_else(|| Err(format!("missing script asset {module}.gsc")))
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub(crate) fn asset_names(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for source in self.sources.values().filter_map(|s| s.as_ref().ok()) {
            quoted_names(source, &mut names);
        }
        if let Some(entities) = &self.entities {
            quoted_names(entities.as_bytes(), &mut names);
        }
        for table in self.tables.values() {
            names.extend(table.cells.iter().cloned());
        }
        names
    }

    pub fn tables(&self) -> &BTreeMap<String, ScriptTable> {
        &self.tables
    }

    pub fn entities(&self) -> Option<&str> {
        self.entities.as_deref()
    }

    pub fn config(&self, name: &str) -> Option<&str> {
        self.configs.get(&normalize(name)).map(String::as_str)
    }

    pub fn shocks(&self) -> impl Iterator<Item = (&str, &str)> {
        let t6 = self
            .t6
            .iter()
            .flat_map(|t6| t6.shocks.iter().map(|(n, t)| (n.as_str(), t.as_str())));
        self.configs
            .iter()
            .filter_map(|(path, text)| {
                let name = path.strip_prefix("shock/")?.strip_suffix(".shock")?;
                Some((name, text.as_str()))
            })
            .chain(t6)
    }

    pub(crate) fn capture(&mut self, name: &str, data: &[u8], compressed: bool) {
        let name = normalize(name);
        if name.ends_with(".cfg") || name.ends_with(".shock") || name == "radiant/keys.txt" {
            if let Some(text) = asset_world::decode_rawfile_text(data, compressed) {
                self.configs.insert(name, text);
            }
            return;
        }
        let Some(module) = name.strip_suffix(".gsc") else {
            return;
        };
        let source = if compressed {
            asset_transport::inflate_zlib(data).map_err(|e| e.to_string())
        } else {
            Ok(asset_world::decode_packed_rawfile(data).unwrap_or_else(|| data.to_vec()))
        }
        .map(|mut bytes| {
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            bytes
        });
        self.sources.insert(module.to_owned(), source);
    }

    pub(crate) fn capture_table(&mut self, table: &asset_game::CapturedStringTable) {
        self.tables.insert(
            normalize(&table.name),
            ScriptTable {
                columns: table.columns,
                rows: table.rows,
                cells: table.cells.clone(),
            },
        );
    }

    pub(crate) fn set_table_cells(
        &mut self,
        table: &str,
        key: &str,
        cells: &[(usize, String)],
    ) -> bool {
        let Some(table) = self.tables.get_mut(&normalize(table)) else {
            return false;
        };
        let Some(row) = table
            .cells
            .chunks_exact_mut(table.columns.max(1))
            .take(table.rows)
            .find(|row| row[0].eq_ignore_ascii_case(key))
        else {
            return false;
        };
        for (column, value) in cells {
            if let Some(cell) = row.get_mut(*column) {
                cell.clone_from(value);
            }
        }
        true
    }

    pub(crate) fn insert_source(&mut self, module: &str, source: String) {
        self.sources
            .insert(normalize(module), Ok(source.into_bytes()));
    }

    pub(crate) fn set_entities(&mut self, entities: String) {
        self.entities = Some(entities);
    }

    pub(crate) fn overlay(&mut self, other: Self) {
        self.sources.extend(other.sources);
        self.tables.extend(other.tables);
        self.configs.extend(other.configs);
        if other.entities.is_some() {
            self.entities = other.entities;
        }
        if other.t6.is_some() {
            self.t6 = other.t6;
        }
        if !other.foreign_path_nodes.is_empty() {
            self.foreign_path_nodes = other.foreign_path_nodes;
        }
    }

    /// Common-walk scripts. A Black Ops II `t6` set is not part of this blob.
    pub fn export_common(&self) -> Result<Vec<u8>, String> {
        if self.t6.is_some() {
            return Err("common script blob does not hold a Black Ops II script set".into());
        }
        #[derive(serde::Serialize)]
        struct Blob<'a> {
            sources: Vec<(&'a str, &'a Result<Vec<u8>, String>)>,
            tables: Vec<(&'a str, &'a ScriptTable)>,
            entities: &'a Option<String>,
            configs: Vec<(&'a str, &'a String)>,
            nodes: &'a [asset_t6::PathNodeRef],
        }
        let blob = Blob {
            sources: self.sources.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            tables: self.tables.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            entities: &self.entities,
            configs: self.configs.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            nodes: &self.foreign_path_nodes,
        };
        serde_json::to_vec(&blob).map_err(|e| e.to_string())
    }

    pub fn import_common(bytes: &[u8]) -> Option<Self> {
        #[derive(serde::Deserialize)]
        struct Blob {
            sources: Vec<(String, Result<Vec<u8>, String>)>,
            tables: Vec<(String, ScriptTable)>,
            entities: Option<String>,
            configs: Vec<(String, String)>,
            nodes: Vec<asset_t6::PathNodeRef>,
        }
        let blob: Blob = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            t6: None,
            sources: blob.sources.into_iter().collect(),
            tables: blob.tables.into_iter().collect(),
            entities: blob.entities,
            configs: blob.configs.into_iter().collect(),
            foreign_path_nodes: blob.nodes,
        })
    }
}

fn quoted_names(bytes: &[u8], names: &mut BTreeSet<String>) {
    let mut at = 0;
    while at < bytes.len() {
        match (bytes[at], bytes.get(at + 1).copied()) {
            (b'/', Some(b'/')) => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            (b'/', Some(b'*')) => {
                at += 2;
                while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                    at += 1;
                }
                at = (at + 2).min(bytes.len());
            }
            (b'"', _) => {
                at += 1;
                let start = at;
                while at < bytes.len() && bytes[at] != b'"' {
                    if bytes[at] == b'\\' && at + 1 < bytes.len() {
                        at += 1;
                    }
                    at += 1;
                }
                if let Ok(name) = std::str::from_utf8(&bytes[start..at]) {
                    names.insert(name.to_owned());
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
}

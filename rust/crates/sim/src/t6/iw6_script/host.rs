//! bo2mp: the script VM's view of a Ghosts map: its entities (the map's entity blocks with their
//! raw numeric keys, what the scripts spawn, the players), their engine fields, and the effects
//! the builtins ask of the game (movers, sounds, effects, exploders). It knows nothing of the
//! sim `World`: the glue in `mod.rs` feeds it poses and events and applies `Effect`s, so it can
//! be tested on its own.

use std::collections::{BTreeMap, HashMap};

use gsc_iw6_vm::{ArrRef, CallCtx, EntityId, Host, HostReturn, Key, Name, ObjRef, Value};

// Engine field ids (`gsc_iw6/data/fields_iw6.names`).
pub const F_ANGLES: u16 = 47;
pub const F_CLASSNAME: u16 = 97;
pub const F_CODE_CLASSNAME: u16 = 99;
pub const F_HEALTH: u16 = 271;
pub const F_MODEL: u16 = 392;
pub const F_ORIGIN: u16 = 430;
pub const F_NOTEWORTHY: u16 = 506;
pub const F_LINKNAME: u16 = 504;
pub const F_SPAWNFLAGS: u16 = 542;
pub const F_TARGET: u16 = 640;
pub const F_TARGETNAME: u16 = 642;
pub const F_TEAM: u16 = 643;
pub const F_STRUCT_LIST: u16 = 31988;

/// Keys whose map value is always text.
const TEXT_KEYS: [u16; 11] = [97, 99, 392, 504, 506, 640, 642, 27766, 27614, 27658, 27665];

/// What a builtin asked the game to do (applied by the glue after the VM stepped).
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `moveto`: from where the entity stands now.
    MoveTo { ent: u32, to: [f32; 3], secs: f32, accel: f32, decel: f32 },
    /// `rotateto`.
    RotateTo { ent: u32, to: [f32; 3], secs: f32, accel: f32, decel: f32 },
    /// A script wrote `origin` / `angles` (the entity jumps there).
    Place { ent: u32, origin: Option<[f32; 3]>, angles: Option<[f32; 3]> },
    /// `spawn`: a new entity (`radius` / `height` are a trigger's size).
    Spawn { ent: u32, class: String, origin: [f32; 3], spawnflags: i32, radius: f32, height: f32 },
    SetModel { ent: u32, model: String },
    /// `hide` (true) / `show` (false).
    Hide { ent: u32, hidden: bool },
    /// `solid` (true) / `notsolid` (false).
    Solid { ent: u32, solid: bool },
    /// `setcandamage`.
    CanDamage { ent: u32, on: bool },
    /// `makeusable` (true) / `makeunusable`.
    Usable { ent: u32, on: bool },
    Hint { ent: u32, text: String },
    Delete { ent: u32 },
    /// `linkto`. READ from the handler: with no offsets the entity keeps the place it stands at
    /// now relative to the parent's tag; with offsets (`explicit`) they are the place relative to it.
    Link { ent: u32, parent: u32, tag: String, offset: [f32; 3], angles: [f32; 3], explicit: bool },
    Unlink { ent: u32 },
    /// A sound at a place or on an entity (`playsound`, `playsoundatposition`).
    Sound { ent: Option<u32>, at: Option<[f32; 3]>, alias: String },
    /// `playloopsound` (Some) / `stoploopsound` (None).
    LoopSound { ent: u32, alias: Option<String> },
    /// `playfx` / `triggerfx` at a place.
    Fx { path: String, origin: [f32; 3], forward: [f32; 3], up: [f32; 3] },
    /// `playfxontag`.
    FxOnTag { path: String, ent: u32, tag: String },
    /// `stopfxontag` (the emitter stops, what is flying finishes) / `killfxontag` (everything goes).
    StopFxOnTag { path: String, ent: u32, tag: String, kill: bool },
    /// `playloopedfx`: the effect repeats every `repeat` seconds at a place.
    LoopedFx { path: String, repeat: f32, origin: [f32; 3], forward: [f32; 3] },
    /// `hidepart` (true) / `showpart` (false): a tag's part of the model.
    HidePart { ent: u32, tag: String, hide: bool },
    /// `attach` (true) / `detach` (false): a model on a tag of the entity's model.
    Attach { ent: u32, model: String, tag: String, on: bool },
    /// `radiusdamage` from a script (inflictor `ent`): the host has already hurt its own entities;
    /// the sim hurts the players.
    RadiusDamage { origin: [f32; 3], range: f32, max: f32, min: f32, attacker: Option<u32>, inflictor: u32, means: String, weapon: String },
    /// `activateclientexploder(id, ..., delay)`.
    Exploder { id: i32, delay: f32 },
    Earthquake { scale: f32, duration: f32, at: [f32; 3], radius: f32 },
    PhysicsBlast { at: [f32; 3], radius: f32, inner: f32, magnitude: f32 },
    /// `connectpaths` (true) / `disconnectpaths`.
    Paths { ent: u32, connect: bool },
    /// `scriptmodelplayanim(name)`: the model plays the animation from now (READ from the handler: the
    /// entity's trajectory becomes the animated type, started at the level time, lasting the
    /// animation's frames over its rate). `via_tree` is `useanimtree(name)`, which binds the
    /// entity to the named animation the same way (READ: the handler looks the name up and stores
    /// it with the level time); a name no animation has is a no-op.
    Anim { ent: u32, name: String, via_tree: bool },
    /// `scriptmodelclearanim`-like stop (the handler resets the trajectory to stationary).
    ClearAnim { ent: u32 },
    /// `setmovingplatformenabled`.
    Platform { ent: u32, on: bool },
    /// `setclientomnvar(name, value)` on a player (the HUD reads these).
    Omnvar { ent: u32, name: String, value: f32 },
    /// `playlocalsound`: the sound for one player.
    LocalSound { ent: u32, alias: String },
    Shellshock { ent: u32, name: String, secs: f32 },
    /// `suicide`.
    Kill { ent: u32 },
    /// `cloneplayer`: a corpse of the player stays.
    CloneCorpse { ent: u32, secs: f32 },
    /// `setlightintensity` on a light entity.
    LightIntensity { ent: u32, value: f32 },
    /// `visionsetnaked` and kin (`kind` is naked, thermal, pain, night, missilecam).
    VisionSet { kind: String, name: String, secs: f32 },
}

/// One entity of the script VM.
#[derive(Clone, Debug, Default)]
pub struct HostEnt {
    /// The sim's entity for it, when it has one.
    pub zm: Option<u32>,
    pub gone: bool,
    /// Its place in the map's entity blocks.
    pub block: Option<usize>,
    pub fields: HashMap<Name, Value>,
    /// The scripts hid it (`hide`).
    pub hidden: bool,
    pub solid: bool,
    pub can_damage: bool,
    /// The parent it is linked to (host id).
    pub parent: Option<u32>,
    /// A player's client number.
    pub client: Option<u32>,
    /// `setcandamage` (the engine's takedamage byte).
    pub takedamage: bool,
    /// `setcanradiusdamage(false)` sets it: blasts pass the entity by.
    pub no_radius_damage: bool,
    /// `attach`ed models: (model, tag).
    pub attached: Vec<(String, String)>,
    /// `setscriptmoverkillcam`'s type.
    pub killcam: Option<String>,
    /// Made by a script (`spawn`), not by the map.
    pub spawned: bool,
    /// A roster bot (set by the glue).
    pub bot: bool,
}

/// A string table: `(columns, rows, cells)`.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub columns: usize,
    pub rows: usize,
    pub cells: Vec<String>,
}

impl Table {
    pub fn cell(&self, row: usize, col: usize) -> Option<&str> {
        (row < self.rows && col < self.columns).then(|| self.cells.get(row * self.columns + col).map(String::as_str)).flatten()
    }
}

/// The script host.
pub struct Iw6Host {
    pub ents: Vec<HostEnt>,
    /// Level time in ms (the sim's).
    pub now_ms: i64,
    pub dvars: BTreeMap<String, String>,
    pub tables: HashMap<String, Table>,
    /// `loadfx` paths; the id is the index + 1.
    pub fx_paths: Vec<String>,
    pub out: Vec<Effect>,
    /// Entities removed since the glue last looked.
    pub removed: Vec<u32>,
    /// Notifies the host asks of the VM after a call (`playsound(.., notify)` and friends), as
    /// (due ms, target, name).
    pub timers: Vec<(i64, Value, String)>,
    /// Lines for the log.
    pub log: Vec<String>,
    /// Builtins that were called but are not implemented: id -> calls.
    pub missing: BTreeMap<u16, u64>,
    pub rng: u64,
    /// Per client: the host entity of the player.
    pub players: BTreeMap<u32, u32>,
    /// Sounds' lengths in ms by alias, when known (otherwise `default_sound_ms`).
    pub sound_ms: HashMap<String, i64>,
    pub default_sound_ms: i64,
    /// Names seen for `getent` keys that are not engine fields (name -> field id).
    pub key_ids: HashMap<String, u16>,
    /// Per-model tag origins (model space) and bounds, by lower-case model name, as the zones'
    /// models give them (`gettagorigin`, damage boxes). Empty: unknown models.
    pub models: HashMap<String, ModelInfo>,
    /// Script-model animation: the zone's clips and skeletons, and what plays on which entity.
    pub anim: super::anim::Animator,
    /// The player data the scripts write and read back (`setrankedplayerdata` and kin): a
    /// private store, by player and path. The engine's own profile is not modelled.
    pub player_data: HashMap<(u32, String), Value>,
    /// What the player-side builtins that only keep a value were last given (no effect on the sim).
    pub stores: Stores,
    /// What the scripts built on each player (weapons, perks, flags, ...), by host entity.
    pub pstates: HashMap<u32, super::player::PlayerState>,
    /// `(clip, stock)` a weapon starts with, by lower-case name, when the glue knows the weapon files.
    pub weapon_info: HashMap<String, (i32, i32)>,
    /// HUD elements the scripts made.
    pub huds: Vec<super::player::HudRec>,
    /// Names the precache calls registered, by kind (`model`, `shader`, ...).
    pub precached: BTreeMap<String, std::collections::BTreeSet<String>>,
}

/// Small stores of the builtins whose handlers only record what the script set (and read it back).
#[derive(Clone, Debug, Default)]
pub struct Stores {
    /// `setteamscore(team, score)`, by team name.
    pub team_scores: BTreeMap<String, i32>,
    /// `allowspectateteam(team, allowed)`: (player entity, team or "freelook"/"none") -> allowed.
    pub spectate: BTreeMap<(u32, String), bool>,
    /// `setcontents(int)`, by entity.
    pub contents: HashMap<u32, i32>,
    /// `setmovespeedscale(f)`, by entity.
    pub move_speed_scale: HashMap<u32, f32>,
    /// `setstance(name)`, by entity.
    pub stance: HashMap<u32, String>,
    /// `setcamerapoint(index, origin, angles)`: (player entity, index) -> (origin, angles).
    pub camera: BTreeMap<(u32, u32), ([f32; 3], [f32; 3])>,
    /// `notifyonplayercommand(notify, command)`: (player entity, command) -> notifies.
    pub commands: BTreeMap<(u32, String), Vec<String>>,
    /// `setclientdvars` / `setclientdvar`: (player entity, name) -> value.
    pub client_dvars: BTreeMap<(u32, String), String>,
    /// `setslowmotion(start, end, time)` as last set: (start scale, end scale, seconds).
    pub slow_motion: Option<(f32, f32, f32)>,
    /// `map_restart` / `exitlevel` asked for (`true` the restart), with the persistence flag.
    pub level_exit: Option<(bool, bool)>,
    /// `setlightintensity`, by light entity.
    pub light_intensity: HashMap<u32, f32>,
    /// `setdepthoffield`, by player: near start, near end, far start, far end, near blur, far blur.
    pub depth_of_field: HashMap<u32, [f32; 6]>,
    /// `visionsetnaked` and kin as last set: kind -> (name, seconds).
    pub vision: BTreeMap<String, (String, f32)>,
    /// `setminimap`'s material.
    pub minimap: Option<String>,
}

/// What the host knows about a model.
#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// (tag name lower case, origin in model space).
    pub tags: Vec<(String, [f32; 3])>,
}

impl Default for Iw6Host {
    fn default() -> Iw6Host {
        Iw6Host {
            ents: Vec::new(),
            now_ms: 0,
            dvars: BTreeMap::new(),
            tables: HashMap::new(),
            fx_paths: Vec::new(),
            out: Vec::new(),
            removed: Vec::new(),
            timers: Vec::new(),
            log: Vec::new(),
            missing: BTreeMap::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
            players: BTreeMap::new(),
            sound_ms: HashMap::new(),
            default_sound_ms: 1000,
            key_ids: HashMap::new(),
            models: HashMap::new(),
            anim: super::anim::Animator::default(),
            player_data: HashMap::new(),
            stores: Stores::default(),
            pstates: HashMap::new(),
            weapon_info: HashMap::new(),
            huds: Vec::new(),
            precached: BTreeMap::new(),
        }
    }
}

pub fn parse_vec3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|x| x.parse::<f32>().ok());
    let v = [it.next()??, it.next()??, it.next()??];
    it.next().is_none().then_some(v)
}

/// A map key's value as the script sees it. INFERRED from how the scripts use the keys: the
/// engine's own fields are typed (`origin`, `angles` vectors, `spawnflags` an integer, `speed`
/// a float), the names and links are text, any other key is a number when it reads as one.
pub fn typed(key: u16, text: &str) -> Value {
    let t = text.trim();
    match key {
        F_ORIGIN | F_ANGLES => return parse_vec3(t).map_or_else(|| Value::str(text), Value::Vector),
        F_SPAWNFLAGS => return Value::Int(t.parse::<f32>().map_or(0, |f| f as i32)),
        546 => return t.parse::<f32>().map_or_else(|_| Value::str(text), Value::Float),
        _ => {}
    }
    if TEXT_KEYS.contains(&key) {
        return Value::str(text);
    }
    if let Ok(i) = t.parse::<i32>()
        && !t.starts_with('+')
        && (t == "0" || !t.starts_with('0'))
    {
        return Value::Int(i);
    }
    if let Ok(f) = t.parse::<f32>()
        && f.is_finite()
        && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+'))
    {
        return Value::Float(f);
    }
    if let Some(v) = parse_vec3(t) {
        return Value::Vector(v);
    }
    Value::str(text)
}

/// A name as the host keys fields: text that is an engine field becomes its id.
pub fn norm(n: &Name) -> Name {
    match n {
        Name::Text(s) => match gsc_iw6::builtins::field_id(s) {
            Some(id) => Name::Id(id),
            None => n.clone(),
        },
        Name::Id(_) => n.clone(),
    }
}

/// Text of a value as dvars and string compares take it.
pub fn text_of(v: &Value) -> String {
    match v {
        Value::Str(s) | Value::IStr(s) => s.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => gsc_iw6_vm::fmt_g(f64::from(*f)),
        Value::Vector(p) => format!("{} {} {}", gsc_iw6_vm::fmt_g(f64::from(p[0])), gsc_iw6_vm::fmt_g(f64::from(p[1])), gsc_iw6_vm::fmt_g(f64::from(p[2]))),
        Value::Undefined => String::new(),
        other => other.to_string(),
    }
}

impl Iw6Host {
    /// A new entity (host id).
    pub fn add_ent(&mut self, ent: HostEnt) -> u32 {
        self.ents.push(ent);
        (self.ents.len() - 1) as u32
    }

    pub fn ent(&self, id: u32) -> Option<&HostEnt> {
        self.ents.get(id as usize).filter(|e| !e.gone)
    }
    pub fn ent_mut(&mut self, id: u32) -> Option<&mut HostEnt> {
        self.ents.get_mut(id as usize).filter(|e| !e.gone)
    }

    pub fn field(&self, id: u32, f: u16) -> Option<&Value> {
        self.ent(id)?.fields.get(&Name::Id(f))
    }
    pub fn origin(&self, id: u32) -> [f32; 3] {
        self.field(id, F_ORIGIN).and_then(Value::as_vector).unwrap_or([0.0; 3])
    }
    pub fn angles(&self, id: u32) -> [f32; 3] {
        self.field(id, F_ANGLES).and_then(Value::as_vector).unwrap_or([0.0; 3])
    }
    pub fn class(&self, id: u32) -> String {
        self.field(id, F_CLASSNAME).and_then(Value::as_str).unwrap_or("").to_owned()
    }
    pub fn set_field(&mut self, id: u32, f: u16, v: Value) {
        if let Some(e) = self.ent_mut(id) {
            e.fields.insert(Name::Id(f), v);
        }
    }

    /// Where the sim moved an entity to (a mover's step): no effect is queued.
    pub fn set_pose(&mut self, id: u32, origin: [f32; 3], angles: [f32; 3]) {
        if let Some(e) = self.ent_mut(id) {
            e.fields.insert(Name::Id(F_ORIGIN), Value::Vector(origin));
            e.fields.insert(Name::Id(F_ANGLES), Value::Vector(angles));
        }
    }

    /// The map's entity blocks as host entities (all but structs and path nodes); returns the
    /// structs as VM objects' field lists: `(block, keys)`.
    pub fn load_blocks(&mut self, blocks: &[Vec<(u32, String)>]) -> Vec<(usize, Vec<(u16, Value)>)> {
        let mut structs = Vec::new();
        for (i, b) in blocks.iter().enumerate() {
            let fields: Vec<(u16, Value)> = b
                .iter()
                .filter(|(k, _)| *k <= u32::from(u16::MAX))
                .map(|(k, v)| (*k as u16, typed(*k as u16, v)))
                .collect();
            let class = fields.iter().find(|(k, _)| *k == F_CLASSNAME).and_then(|(_, v)| v.as_str()).unwrap_or("").to_owned();
            if class == "script_struct" {
                structs.push((i, fields));
                continue;
            }
            if class.starts_with("node_") {
                continue;
            }
            let mut e = HostEnt { block: Some(i), solid: true, ..Default::default() };
            for (k, v) in fields {
                e.fields.insert(Name::Id(k), v);
            }
            e.fields.entry(Name::Id(F_SPAWNFLAGS)).or_insert(Value::Int(0));
            e.fields.entry(Name::Id(F_ORIGIN)).or_insert(Value::Vector([0.0; 3]));
            e.fields.entry(Name::Id(F_ANGLES)).or_insert(Value::Vector([0.0; 3]));
            e.fields.entry(Name::Id(F_CODE_CLASSNAME)).or_insert(Value::str(&class));
            self.add_ent(e);
        }
        structs
    }

    /// The engine field a script names by text (`"targetname"`), else a field by the name the
    /// map's keys were given.
    pub fn key_name(&self, key: &str) -> Name {
        match gsc_iw6::builtins::field_id(key) {
            Some(id) => Name::Id(id),
            None => match self.key_ids.get(&key.to_ascii_lowercase()) {
                Some(id) => Name::Id(*id),
                None => Name::Text(key.to_owned()),
            },
        }
    }

    /// Entities whose `key` field equals `value` (text compare, case-insensitive).
    pub fn find(&self, key: &str, value: &str) -> Vec<u32> {
        let name = self.key_name(key);
        let alt = if matches!(name, Name::Id(F_CODE_CLASSNAME)) { Some(Name::Id(F_CLASSNAME)) } else { None };
        self.ents
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.gone)
            .filter(|(_, e)| {
                let v = e.fields.get(&name).or_else(|| alt.as_ref().and_then(|a| e.fields.get(a)));
                v.is_some_and(|v| text_of(v).eq_ignore_ascii_case(value))
            })
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Entities linked to `id` (`linkto`), in id order.
    pub fn children_of(&self, id: u32) -> Vec<u32> {
        (0..self.ents.len() as u32).filter(|&c| self.ent(c).is_some_and(|e| e.parent == Some(id))).collect()
    }

    /// The model name an entity shows (lower case; empty for none).
    pub fn model_of(&self, id: u32) -> String {
        self.field(id, F_MODEL).map(text_of).unwrap_or_default().to_ascii_lowercase()
    }

    /// World origin and angles of a tag of an entity's model. A model with a skeleton gives the
    /// bone as it stands animated; else the model's tag table (origin only); a tag the model does
    /// not have (or a model the host knows nothing of) stands at the entity's origin.
    pub fn tag_world(&self, id: u32, tag: &str) -> ([f32; 3], [f32; 3]) {
        self.tag_world_at(id, self.origin(id), self.angles(id), tag)
    }

    /// `tag_world` for the entity standing at `origin` with `angles` (the sim's place for it).
    pub fn tag_world_at(&self, id: u32, o: [f32; 3], a: [f32; 3], tag: &str) -> ([f32; 3], [f32; 3]) {
        let model = self.model_of(id);
        if let Some(w) = self.anim.tag_world(id, &model, o, a, tag, self.now_ms) {
            return w;
        }
        let want = tag.to_ascii_lowercase();
        let p = self.models.get(&model).and_then(|m| m.tags.iter().find(|(n, _)| *n == want)).map_or([0.0; 3], |(_, p)| *p);
        let (f, r, u) = super::builtins::angle_vectors(a);
        let w = std::array::from_fn(|k| o[k] + f[k] * p[0] - r[k] * p[1] + u[k] * p[2]);
        (w, a)
    }

    pub fn next_rand(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn rand_unit(&mut self) -> f32 {
        ((self.next_rand() >> 40) as f32) / ((1u64 << 24) as f32)
    }

    pub fn dvar(&self, name: &str) -> Option<&str> {
        self.dvars.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
    pub fn set_dvar(&mut self, name: &str, value: &str) {
        self.dvars.insert(name.to_ascii_lowercase(), value.to_owned());
    }

    /// An entity value for host id `id`.
    pub fn value(id: u32) -> Value {
        Value::Entity(EntityId(id))
    }

    /// Takes the effects queued so far.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.out)
    }
}

impl Host for Iw6Host {
    fn get_entity_field(&mut self, ent: EntityId, field: &Name) -> Option<Value> {
        let e = self.ents.get(ent.0 as usize)?;
        if e.gone {
            return None;
        }
        let n = norm(field);
        match e.fields.get(&n) {
            Some(v) => Some(v.clone()),
            // Engine-defined and always present.
            None => match n {
                Name::Id(F_ORIGIN) | Name::Id(F_ANGLES) => Some(Value::Vector([0.0; 3])),
                _ => None,
            },
        }
    }

    fn set_entity_field(&mut self, ent: EntityId, field: &Name, value: &Value) -> bool {
        let n = norm(field);
        let Some(e) = self.ents.get_mut(ent.0 as usize) else { return false };
        if e.gone {
            return false;
        }
        match (&n, value) {
            (_, Value::Undefined) => {
                e.fields.remove(&n);
            }
            (Name::Id(F_ORIGIN), Value::Vector(v)) => {
                e.fields.insert(n, value.clone());
                self.out.push(Effect::Place { ent: ent.0, origin: Some(*v), angles: None });
            }
            (Name::Id(F_ANGLES), Value::Vector(v)) => {
                e.fields.insert(n, value.clone());
                self.out.push(Effect::Place { ent: ent.0, origin: None, angles: Some(*v) });
            }
            (Name::Id(F_MODEL), v) => {
                e.fields.insert(n, v.clone());
                self.out.push(Effect::SetModel { ent: ent.0, model: text_of(v) });
            }
            _ => {
                e.fields.insert(n, value.clone());
            }
        }
        true
    }

    fn call_function(&mut self, id: u16, ctx: &mut CallCtx<'_>, args: &[Value]) -> Result<HostReturn, String> {
        super::builtins::function(self, id, ctx, args)
    }

    fn call_method(&mut self, id: u16, ctx: &mut CallCtx<'_>, this: &Value, args: &[Value]) -> Result<HostReturn, String> {
        super::builtins::method(self, id, ctx, this, args)
    }
}

/// An array of entities.
pub fn ent_array(ids: impl IntoIterator<Item = u32>) -> Value {
    Value::Array(ArrRef::from_values(ids.into_iter().map(Iw6Host::value)))
}

/// A struct object from fields.
pub fn struct_from(fields: &[(u16, Value)]) -> ObjRef {
    let s = ObjRef::new_struct();
    for (k, v) in fields {
        s.set(Name::Id(*k), v.clone());
    }
    s
}

/// `key` of an array by text.
pub fn arr_key(s: &str) -> Key {
    Key::Str(s.into())
}

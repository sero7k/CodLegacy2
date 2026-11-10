//! The builtins the Ghosts map scripts call, over `Iw6Host`. Names come from the exe's handlers
//! (`docs/exe_notes/IW6_GSC_BUILTINS.md`); the rules (argument checks, minimum times) from the
//! handlers where it matters (`moveto`, `rotateto`: a time of 0 or less is an error, at least
//! 0.001 s, accel + decel may not exceed the time).

use gsc_iw6::builtins::{self, Kind};
use gsc_iw6_vm::{ArrRef, CallCtx, EntityId, HostReturn, Key, Name, ObjRef, Value};

use super::host::{ent_array, text_of, Effect, Iw6Host, F_CLASSNAME, F_CODE_CLASSNAME, F_HEALTH, F_MODEL, F_ORIGIN};

type R = Result<HostReturn, String>;

fn num(a: &[Value], i: usize, what: &str) -> Result<f32, String> {
    a.get(i).and_then(Value::as_float).ok_or_else(|| format!("{what}: argument {} is not a number", i + 1))
}
fn opt_num(a: &[Value], i: usize) -> f32 {
    a.get(i).and_then(Value::as_float).unwrap_or(0.0)
}
fn vec3(a: &[Value], i: usize, what: &str) -> Result<[f32; 3], String> {
    a.get(i).and_then(Value::as_vector).ok_or_else(|| format!("{what}: argument {} is not a vector", i + 1))
}
fn text(a: &[Value], i: usize) -> String {
    a.get(i).map(text_of).unwrap_or_default()
}
fn val(v: Value) -> R {
    Ok(HostReturn::Value(v))
}
fn flt(f: f32) -> R {
    val(Value::Float(f))
}
fn ent_id(v: &Value) -> Option<u32> {
    v.as_entity().map(|e| e.0)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The engine's angles of a direction (`vectortoangles`): pitch up negative.
pub fn vector_to_angles(v: [f32; 3]) -> [f32; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    if x == 0.0 && y == 0.0 {
        return [if z > 0.0 { 270.0 } else { 90.0 }, 0.0, 0.0];
    }
    let mut yaw = y.atan2(x).to_degrees();
    if yaw < 0.0 {
        yaw += 360.0;
    }
    let fwd = (x * x + y * y).sqrt();
    let mut pitch = (-z).atan2(fwd).to_degrees();
    if pitch < 0.0 {
        pitch += 360.0;
    }
    [pitch, yaw, 0.0]
}

/// forward, right, up of angles (pitch, yaw, roll degrees).
pub fn angle_vectors(a: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sp, cp) = a[0].to_radians().sin_cos();
    let (sy, cy) = a[1].to_radians().sin_cos();
    let (sr, cr) = a[2].to_radians().sin_cos();
    let f = [cp * cy, cp * sy, -sp];
    let r = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let u = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    (f, r, u)
}

/// The engine's checks on the time of a move or rotation: `time` > 0 (at least 0.001),
/// `accel`, `decel` >= 0, accel + decel within the time (a millionth over is let through).
pub fn move_times(time: f32, accel: f32, decel: f32, what: &str) -> Result<(f32, f32, f32), String> {
    if time <= 0.0 {
        return Err(format!("{what}: time must be positive"));
    }
    let time = time.max(0.001);
    if accel < 0.0 {
        return Err(format!("{what}: acceleration time must be non-negative"));
    }
    if decel < 0.0 {
        return Err(format!("{what}: deceleration time must be non-negative"));
    }
    let mut t = time;
    if time < accel + decel {
        t = time * 1.000_000_5;
        if t < accel + decel {
            return Err(format!("{what}: acceleration plus deceleration time is more than the time"));
        }
        t = time;
    }
    Ok((t, accel, decel))
}

/// The entity's box relative to its origin: its model's bounds, else a unit box.
fn bounds_of(h: &Iw6Host, e: u32) -> ([f32; 3], [f32; 3]) {
    h.models.get(&h.model_of(e)).map_or(([-1.0; 3], [1.0; 3]), |m| (m.mins, m.maxs))
}

fn sort_origin(h: &Iw6Host, v: &Value) -> [f32; 3] {
    match v {
        Value::Entity(e) => h.origin(e.0),
        Value::Object(o) => o.get(&Name::Id(F_ORIGIN)).and_then(|v| v.as_vector()).unwrap_or([0.0; 3]),
        Value::Vector(p) => *p,
        _ => [0.0; 3],
    }
}

fn fx_index(h: &mut Iw6Host, path: &str) -> i32 {
    let full = if path.contains('/') { path.to_owned() } else { format!("fx/{path}") };
    if let Some(i) = h.fx_paths.iter().position(|p| p.eq_ignore_ascii_case(&full)) {
        return i as i32 + 1;
    }
    h.fx_paths.push(full);
    h.fx_paths.len() as i32
}

fn fx_path(h: &Iw6Host, a: &Value) -> Option<String> {
    let i = a.as_int()?;
    h.fx_paths.get((i - 1) as usize).cloned()
}

/// A builtin function call.
pub fn function(h: &mut Iw6Host, id: u16, _ctx: &mut CallCtx<'_>, a: &[Value]) -> R {
    if id == 0x175 {
        // Is this MOD name an explosive kind? The exe's per-MOD flag table is set for
        // EXPLOSIVE_BULLET, GRENADE, GRENADE_SPLASH, PROJECTILE, PROJECTILE_SPLASH, EXPLOSIVE.
        let m = text(a, 0).to_ascii_uppercase();
        let hit = matches!(
            m.as_str(),
            "MOD_EXPLOSIVE_BULLET" | "MOD_GRENADE" | "MOD_GRENADE_SPLASH" | "MOD_PROJECTILE" | "MOD_PROJECTILE_SPLASH" | "MOD_EXPLOSIVE"
        );
        return val(Value::Int(i32::from(hit)));
    }
    if matches!(id, 0x65 | 0x133) {
        // Dedicated-server stubs: the handlers have no body (READ).
        return Ok(HostReturn::None);
    }
    if id == 0x1f0 {
        // bo2mp: the number of path nodes; the map has no nav data here.
        return val(Value::Int(0));
    }
    if matches!(id, 0x1dd | 0xb9) {
        // bo2mp: an unnamed id the map teleport code walks like a list of entities; none here.
        return val(Value::Array(ArrRef::new()));
    }
    let Some(name) = builtins::name(Kind::Function, id) else {
        *h.missing.entry(id).or_default() += 1;
        return Ok(HostReturn::Unimplemented);
    };
    if let Some(r) = super::player::function(h, name, a) {
        return r;
    }
    match name {
        // ---- dvars, time ----------------------------------------------------------------
        "getdvar" => {
            let n = text(a, 0);
            let v = match h.dvar(&n) {
                Some(v) => v.to_owned(),
                None => a.get(1).map(text_of).unwrap_or_default(),
            };
            val(Value::str(&v))
        }
        "getdvarint" => {
            let n = text(a, 0);
            let s = h.dvar(&n).map(str::to_owned).or_else(|| a.get(1).map(text_of)).unwrap_or_default();
            val(Value::Int(s.trim().parse::<f64>().unwrap_or(0.0) as i32))
        }
        "getdvarfloat" => {
            let n = text(a, 0);
            let s = h.dvar(&n).map(str::to_owned).or_else(|| a.get(1).map(text_of)).unwrap_or_default();
            flt(s.trim().parse::<f32>().unwrap_or(0.0))
        }
        "setdvar" | "setsaveddvar" | "setdevdvar" => {
            let n = text(a, 0);
            let v = match a.get(1) {
                Some(Value::Vector(p)) => format!("{} {} {}", p[0], p[1], p[2]),
                Some(v) => text_of(v).replace('"', "'"),
                None => String::new(),
            };
            h.set_dvar(&n, &v);
            Ok(HostReturn::None)
        }
        "setdvarifuninitialized" => {
            let n = text(a, 0);
            if h.dvar(&n).is_none() {
                let v = text(a, 1);
                h.set_dvar(&n, &v);
            }
            Ok(HostReturn::None)
        }
        "gettime" => val(Value::Int(h.now_ms.clamp(0, i64::from(i32::MAX)) as i32)),
        // ---- numbers --------------------------------------------------------------------
        "randomint" => {
            let m = a.first().and_then(Value::as_int).ok_or("randomint: not a number")?;
            if m < 1 {
                return Err(format!("randomint parm 1 ({m}) must be positive"));
            }
            let r = (h.next_rand() % m as u64) as i32;
            val(Value::Int(r))
        }
        "randomintrange" => {
            let (lo, hi) = (a.first().and_then(Value::as_int).unwrap_or(0), a.get(1).and_then(Value::as_int).unwrap_or(0));
            if hi <= lo {
                return val(Value::Int(lo));
            }
            let r = lo + (h.next_rand() % (hi - lo) as u64) as i32;
            val(Value::Int(r))
        }
        "randomfloat" => {
            let m = num(a, 0, "randomfloat")?;
            let r = h.rand_unit();
            flt(r * m)
        }
        "randomfloatrange" => {
            let (lo, hi) = (num(a, 0, "randomfloatrange")?, num(a, 1, "randomfloatrange")?);
            let r = h.rand_unit();
            flt(lo + r * (hi - lo))
        }
        "abs" => flt(num(a, 0, "abs")?.abs()),
        "min" => flt(num(a, 0, "min")?.min(num(a, 1, "min")?)),
        "max" => flt(num(a, 0, "max")?.max(num(a, 1, "max")?)),
        "clamp" => flt(num(a, 0, "clamp")?.clamp(num(a, 1, "clamp")?, num(a, 2, "clamp")?.max(num(a, 1, "clamp")?))),
        "floor" => flt(num(a, 0, "floor")?.floor()),
        "ceil" => flt(num(a, 0, "ceil")?.ceil()),
        "sqrt" => flt(num(a, 0, "sqrt")?.sqrt()),
        "sin" => flt(num(a, 0, "sin")?.to_radians().sin()),
        "cos" => flt(num(a, 0, "cos")?.to_radians().cos()),
        "int" => match a.first() {
            Some(Value::Str(s)) => val(Value::Int(s.trim().parse::<f64>().unwrap_or(0.0) as i32)),
            Some(v) => val(Value::Int(v.as_float().map_or(0, |f| f as i32))),
            None => Err("int: missing argument".into()),
        },
        "float" => match a.first() {
            Some(Value::Str(s)) => flt(s.trim().parse::<f32>().unwrap_or(0.0)),
            Some(v) => flt(v.as_float().unwrap_or(0.0)),
            None => Err("float: missing argument".into()),
        },
        // ---- vectors --------------------------------------------------------------------
        "distance" => flt(len(sub(vec3(a, 0, "distance")?, vec3(a, 1, "distance")?))),
        "distancesquared" => {
            let d = sub(vec3(a, 0, "distancesquared")?, vec3(a, 1, "distancesquared")?);
            flt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
        }
        "length" => flt(len(vec3(a, 0, "length")?)),
        "length2dsquared" => {
            let v = vec3(a, 0, "length2dsquared")?;
            flt(v[0] * v[0] + v[1] * v[1])
        }
        "vectordot" => {
            let (p, q) = (vec3(a, 0, "vectordot")?, vec3(a, 1, "vectordot")?);
            flt(p[0] * q[0] + p[1] * q[1] + p[2] * q[2])
        }
        "vectornormalize" => {
            let v = vec3(a, 0, "vectornormalize")?;
            let l = len(v);
            val(Value::Vector(if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { v }))
        }
        "vectortoangles" => val(Value::Vector(vector_to_angles(vec3(a, 0, "vectortoangles")?))),
        "anglestoforward" => val(Value::Vector(angle_vectors(vec3(a, 0, "anglestoforward")?).0)),
        "anglestoright" => val(Value::Vector(angle_vectors(vec3(a, 0, "anglestoright")?).1)),
        "anglestoup" => val(Value::Vector(angle_vectors(vec3(a, 0, "anglestoup")?).2)),
        // ---- strings --------------------------------------------------------------------
        "tolower" => val(Value::str(&text(a, 0).to_lowercase())),
        "issubstr" => val(Value::Int(i32::from(text(a, 0).contains(&text(a, 1))))),
        "isendstr" => val(Value::Int(i32::from(text(a, 0).ends_with(&text(a, 1))))),
        "getsubstr" => {
            let s = text(a, 0);
            let from = a.get(1).and_then(Value::as_int).unwrap_or(0).max(0) as usize;
            let to = a.get(2).and_then(Value::as_int).map_or(s.len(), |t| t.max(0) as usize).min(s.len());
            val(Value::str(s.get(from.min(to)..to).unwrap_or("")))
        }
        "strcmp" => val(Value::Int(match text(a, 0).cmp(&text(a, 1)) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Greater => 1,
        })),
        "strtok" => {
            let (s, d) = (text(a, 0), text(a, 1));
            let parts: Vec<Value> = s.split(|c| d.contains(c)).filter(|p| !p.is_empty()).map(Value::str).collect();
            val(Value::array(parts))
        }
        "isstring" => val(Value::Int(i32::from(matches!(a.first(), Some(Value::Str(_)))))),
        // ---- entities -------------------------------------------------------------------
        "getent" => {
            let (v, k) = (text(a, 0), text(a, 1));
            let hits = h.find(&k, &v);
            match hits.first() {
                Some(&e) => {
                    if hits.len() > 1 {
                        h.log.push(format!("getent({v:?}, {k:?}): {} entities, the first taken", hits.len()));
                    }
                    val(Iw6Host::value(e))
                }
                None => val(Value::Undefined),
            }
        }
        "getentarray" => {
            if a.is_empty() {
                let all: Vec<u32> = (0..h.ents.len() as u32).filter(|&i| h.ent(i).is_some()).collect();
                return val(ent_array(all));
            }
            let (v, k) = (text(a, 0), text(a, 1));
            val(ent_array(h.find(&k, &v)))
        }
        "spawn" => {
            // spawn(classname, origin, [spawnflags], [radius], [height]): a script_model, a
            // script_origin, a trigger (radius and height are the trigger's size).
            let class = text(a, 0);
            let origin = vec3(a, 1, "spawn")?;
            let spawnflags = a.get(2).and_then(Value::as_int).unwrap_or(0);
            let (radius, height) = (opt_num(a, 3), opt_num(a, 4));
            let mut e = super::host::HostEnt { solid: false, spawned: true, ..Default::default() };
            e.fields.insert(Name::Id(F_CLASSNAME), Value::str(&class));
            e.fields.insert(Name::Id(F_CODE_CLASSNAME), Value::str(&class));
            e.fields.insert(Name::Id(F_ORIGIN), Value::Vector(origin));
            e.fields.insert(Name::Id(47), Value::Vector([0.0; 3]));
            e.fields.insert(Name::Id(542), Value::Int(spawnflags));
            e.fields.insert(Name::Id(F_HEALTH), Value::Int(0));
            if class.starts_with("trigger") {
                e.fields.insert(h.key_name("radius"), Value::Float(radius));
                e.fields.insert(h.key_name("height"), Value::Float(height));
            }
            let id = h.add_ent(e);
            h.out.push(Effect::Spawn { ent: id, class, origin, spawnflags, radius, height });
            val(Iw6Host::value(id))
        }
        "sortbydistance" => {
            let Some(Value::Array(arr)) = a.first() else { return val(Value::Undefined) };
            let from = a.get(1).and_then(Value::as_vector).unwrap_or([0.0; 3]);
            let mut items: Vec<(f32, Value)> = arr
                .values()
                .into_iter()
                .map(|v| {
                    let d = sub(sort_origin(h, &v), from);
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2], v)
                })
                .collect();
            items.sort_by(|x, y| x.0.total_cmp(&y.0));
            val(Value::array(items.into_iter().map(|(_, v)| v)))
        }
        "isplayer" => val(Value::Int(i32::from(a.first().and_then(ent_id).is_some_and(|e| h.ent(e).is_some_and(|x| x.client.is_some()))))),
        "isalive" => val(Value::Int(i32::from(a.first().and_then(ent_id).is_some_and(|e| h.ent(e).is_some())))),
        "isai" | "isbot" | "isagent" | "isvehicle" => val(Value::Int(0)),
        "getnode" => val(Value::Undefined),
        "getnodearray" | "getvehiclenodearray" => val(Value::Array(ArrRef::new())),
        "getvehiclenode" => val(Value::Undefined),
        "connectnodepair" | "disconnectnodepair" => Ok(HostReturn::None),
        // ---- traces ---------------------------------------------------------------------
        "bullettrace" => {
            let end = vec3(a, 1, "bullettrace")?;
            let r = ArrRef::new();
            r.set(Key::Str("position".into()), Value::Vector(end));
            r.set(Key::Str("fraction".into()), Value::Float(1.0));
            r.set(Key::Str("normal".into()), Value::Vector([0.0, 0.0, 1.0]));
            r.set(Key::Str("surfacetype".into()), Value::str("default"));
            val(Value::Array(r))
        }
        "physicstrace" => val(Value::Vector(vec3(a, 1, "physicstrace")?)),
        // ---- effects and sounds ---------------------------------------------------------
        "loadfx" => {
            let p = text(a, 0);
            val(Value::Int(fx_index(h, &p)))
        }
        "playfx" => {
            let (Some(path), Ok(origin)) = (a.first().and_then(|v| fx_path(h, v)), vec3(a, 1, "playfx")) else { return Ok(HostReturn::None) };
            let forward = a.get(2).and_then(Value::as_vector).unwrap_or([0.0, 0.0, 1.0]);
            let up = a.get(3).and_then(Value::as_vector).unwrap_or([0.0; 3]);
            h.out.push(Effect::Fx { path, origin, forward, up });
            Ok(HostReturn::None)
        }
        "playloopedfx" => {
            // playloopedfx(fx, repeat, origin, [cull], [forward], [up])
            let (Some(path), Ok(origin)) = (a.first().and_then(|v| fx_path(h, v)), vec3(a, 2, "playloopedfx")) else { return Ok(HostReturn::None) };
            let forward = a.get(4).and_then(Value::as_vector).unwrap_or([0.0, 0.0, 1.0]);
            h.out.push(Effect::LoopedFx { path, repeat: opt_num(a, 1), origin, forward });
            val(Value::Undefined)
        }
        "spawnfx" => {
            // An effect entity: `triggerfx` plays it.
            let (path, origin) = (a.first().and_then(|v| fx_path(h, v)).unwrap_or_default(), vec3(a, 1, "spawnfx")?);
            let forward = a.get(2).and_then(Value::as_vector).unwrap_or([0.0, 0.0, 1.0]);
            let mut e = super::host::HostEnt::default();
            e.fields.insert(Name::Id(F_CLASSNAME), Value::str("script_fx"));
            e.fields.insert(Name::Id(F_ORIGIN), Value::Vector(origin));
            e.fields.insert(Name::Id(47), Value::Vector(forward));
            e.fields.insert(Name::Text("fx".into()), Value::str(&path));
            val(Iw6Host::value(h.add_ent(e)))
        }
        "triggerfx" => {
            if let Some(e) = a.first().and_then(ent_id)
                && let Some(Value::Str(path)) = h.ent(e).and_then(|x| x.fields.get(&Name::Text("fx".into()))).cloned()
            {
                let (origin, forward) = (h.origin(e), h.angles(e));
                h.out.push(Effect::Fx { path: path.to_string(), origin, forward, up: [0.0; 3] });
            }
            Ok(HostReturn::None)
        }
        "playfxontag" | "playfxontagforclients" => {
            if let (Some(path), Some(e)) = (a.first().and_then(|v| fx_path(h, v)), a.get(1).and_then(ent_id)) {
                h.out.push(Effect::FxOnTag { path, ent: e, tag: text(a, 2) });
            }
            Ok(HostReturn::None)
        }
        "stopfxontag" | "killfxontag" => {
            if let (Some(path), Some(e)) = (a.first().and_then(|v| fx_path(h, v)), a.get(1).and_then(ent_id)) {
                h.out.push(Effect::StopFxOnTag { path, ent: e, tag: text(a, 2), kill: name == "killfxontag" });
            }
            Ok(HostReturn::None)
        }
        "playsoundatposition" => {
            let at = vec3(a, 0, "playsoundatposition")?;
            h.out.push(Effect::Sound { ent: None, at: Some(at), alias: text(a, 1) });
            Ok(HostReturn::None)
        }
        "activateclientexploder" => {
            let id = a.first().and_then(Value::as_int).unwrap_or(0);
            let delay = a.get(2).and_then(Value::as_float).unwrap_or(0.0);
            h.out.push(Effect::Exploder { id, delay });
            Ok(HostReturn::None)
        }
        "earthquake" => {
            h.out.push(Effect::Earthquake { scale: num(a, 0, "earthquake")?, duration: num(a, 1, "earthquake")?, at: vec3(a, 2, "earthquake")?, radius: num(a, 3, "earthquake")? });
            Ok(HostReturn::None)
        }
        "physicsexplosionsphere" => {
            h.out.push(Effect::PhysicsBlast { at: vec3(a, 0, "physicsexplosionsphere")?, radius: num(a, 1, "physicsexplosionsphere")?, inner: opt_num(a, 2), magnitude: opt_num(a, 3) });
            Ok(HostReturn::None)
        }
        // ---- tables ---------------------------------------------------------------------
        "tablelookup" | "tablelookupistring" => {
            let (file, sc, v, rc) = (text(a, 0), a.get(1).and_then(Value::as_int).unwrap_or(0), text(a, 2), a.get(3).and_then(Value::as_int).unwrap_or(0));
            let Some(t) = h.tables.get(&file.to_ascii_lowercase()) else { return val(Value::str("")) };
            for r in 0..t.rows {
                if t.cell(r, sc.max(0) as usize).is_some_and(|c| c.eq_ignore_ascii_case(&v)) {
                    let c = t.cell(r, rc.max(0) as usize).unwrap_or("").to_owned();
                    return val(if name == "tablelookupistring" { Value::IStr(c.as_str().into()) } else { Value::str(&c) });
                }
            }
            val(Value::str(""))
        }
        "tablelookupbyrow" => {
            let (file, r, c) = (text(a, 0), a.get(1).and_then(Value::as_int).unwrap_or(0), a.get(2).and_then(Value::as_int).unwrap_or(0));
            let s = h.tables.get(&file.to_ascii_lowercase()).and_then(|t| t.cell(r.max(0) as usize, c.max(0) as usize)).unwrap_or("").to_owned();
            val(Value::str(&s))
        }
        "tablelookupgetrownum" => {
            let (file, c, v) = (text(a, 0), a.get(1).and_then(Value::as_int).unwrap_or(0), text(a, 2));
            let row = h.tables.get(&file.to_ascii_lowercase()).and_then(|t| (0..t.rows).find(|&r| t.cell(r, c.max(0) as usize).is_some_and(|x| x.eq_ignore_ascii_case(&v))));
            val(Value::Int(row.map_or(-1, |r| r as i32)))
        }
        // ---- setup nobody listens to -----------------------------------------------------
        "precachemodel" | "playsoundtoplayers" | "precacheshader" | "precachestring" | "precacheitem" | "precachemenu" | "precacherumble" | "visionsetnaked"
        | "visionsetthermal" | "visionsetpain" | "visionsetnight" | "visionsetmissilecam" | "setminimap" | "print3d" | "iprintlnbold"
        | "resetscriptslot" => Ok(HostReturn::None),
        // getglass(name): the handle of the map's glass with that name. INFERRED from the map scripts
        // (`glass = getglass(self.target); if (!isdefined(glass)) return; level waittill("glass_destroyed")`):
        // the glass world is not in the host, so no glass is found and those waits never start.
        "getglass" => val(Value::Undefined),
        // getscriptablearray(value, key): the map's scriptable entities with that key (INFERRED from
        // mp_snow's `getscriptablearray("snowman", "targetname")` iterated with getfirstarraykey).
        "getscriptablearray" => {
            let (v, k) = (text(a, 0), text(a, 1));
            let hits: Vec<u32> = h.find(&k, &v).into_iter().filter(|&e| h.class(e).starts_with("scriptable")).collect();
            val(ent_array(hits))
        }
        "getnorthyaw" => flt(0.0),
        "newhudelem" | "newclienthudelem" => val(Value::Object(ObjRef::new_struct())),
        "getmatchrulesdata" => val(Value::Int(0)),
        "isusingmatchrulesdata" | "botsconnecttype" | "isweaponvalid" => val(Value::Int(0)),
        "getweaponattachments" => val(Value::Array(ArrRef::new())),
        "getbaseweaponname" | "weaponclass" => val(Value::str(&text(a, 0))),
        "soundgetplaybacktime" => {
            let ms = h.sound_ms.get(&text(a, 0).to_ascii_lowercase()).copied().unwrap_or(h.default_sound_ms);
            val(Value::Int(ms as i32))
        }
        _ => more_function(h, id, name, a),
    }
}

/// A builtin method call (`this` is the entity).
pub fn method(h: &mut Iw6Host, id: u16, ctx: &mut CallCtx<'_>, this: &Value, a: &[Value]) -> R {
    let Some(name) = builtins::name(Kind::Method, id) else {
        return method_unnamed(h, id, ctx, this, a);
    };
    if let Value::Object(o) = this
        && let Some(r) = super::player::hud_method(h, name, o, a)
    {
        return r;
    }
    let Some(e) = ent_id(this) else {
        // A method on a struct or a freed value.
        *h.missing.entry(id).or_default() += 1;
        return Err(format!("{name}: not an entity ({})", this.type_name()));
    };
    if h.ent(e).is_none() {
        return Err(format!("{name}: entity {e} is gone"));
    }
    if let Some(r) = super::player::flag_method(h, id, e) {
        return r;
    }
    if let Some(r) = super::player::method(h, name, e, a) {
        return r;
    }
    match name {
        "getorigin" => val(Value::Vector(h.origin(e))),
        "enablelinkto" => Ok(HostReturn::None),
        "setcandamage" => {
            // The engine's takedamage byte (READ: a nonzero byte lets G_Damage through).
            let on = a.first().and_then(Value::as_int).unwrap_or(0) != 0;
            if let Some(x) = h.ent_mut(e) {
                x.takedamage = on;
            }
            h.out.push(Effect::CanDamage { ent: e, on });
            Ok(HostReturn::None)
        }
        "setcanradiusdamage" => {
            // Clears entity flag 0x10 for a nonzero argument, sets it for 0 (READ): blasts skip
            // an entity with the flag.
            let on = a.first().and_then(Value::as_int).unwrap_or(0) != 0;
            if let Some(x) = h.ent_mut(e) {
                x.no_radius_damage = !on;
            }
            Ok(HostReturn::None)
        }
        "attach" | "detach" => {
            // attach(model, [tag], [ignore collision]) / detach(model, [tag]): at most 19 models.
            let (model, tag) = (text(a, 0), text(a, 1));
            let on = name == "attach";
            if let Some(x) = h.ent_mut(e) {
                if on {
                    if x.attached.len() >= 19 {
                        return Err("attach: too many attached models".into());
                    }
                    x.attached.push((model.clone(), tag.clone()));
                } else if let Some(i) = x.attached.iter().position(|(m, t)| m.eq_ignore_ascii_case(&model) && t.eq_ignore_ascii_case(&tag)) {
                    x.attached.remove(i);
                } else {
                    return Err(format!("detach: model '{model}' is not attached at tag '{tag}'"));
                }
            }
            h.out.push(Effect::Attach { ent: e, model, tag, on });
            Ok(HostReturn::None)
        }
        "moveshieldmodel" => Ok(HostReturn::None),
        "radiusdamage" => {
            // ent radiusdamage(origin, range, max, min, [attacker], [MOD], [weapon]): a blast
            // from the entity (the inflictor). READ from the engine's blast routine.
            let origin = vec3(a, 0, "radiusdamage")?;
            let (range, max, min) = (num(a, 1, "radiusdamage")?, num(a, 2, "radiusdamage")?, num(a, 3, "radiusdamage")?);
            if range < 0.0 || max < 0.0 || min < 0.0 {
                return Err("radiusdamage: range and damages must not be negative".into());
            }
            let attacker = a.get(4).and_then(ent_id);
            let means = a.get(5).map(text_of).filter(|m| !m.is_empty()).unwrap_or_else(|| "MOD_EXPLOSIVE".to_owned());
            let weapon = a.get(6).map(text_of).unwrap_or_default();
            for (t, n, args) in super::events::radius_notifies(h, origin, range, max, min, attacker, e, &means, &weapon) {
                ctx.notify(t, &n, args);
            }
            h.out.push(Effect::RadiusDamage { origin, range, max, min, attacker, inflictor: e, means, weapon });
            Ok(HostReturn::None)
        }
        "getlinkedchildren" => val(ent_array(h.children_of(e))),
        "setscriptmoverkillcam" => {
            let t = text(a, 0);
            const KINDS: [&str; 9] = ["helicopter", "airstrike", "explosive", "fast explosive", "rocket", "rocket_corpse", "turret", "javelin", "missile"];
            if !KINDS.contains(&t.as_str()) {
                return Err(format!("setscriptmoverkillcam: unknown kill cam type '{t}'"));
            }
            if let Some(x) = h.ent_mut(e) {
                x.killcam = Some(t);
            }
            Ok(HostReturn::None)
        }
        "getcurrentweapon" => val(Value::str("none")),
        "getentitynumber" => val(Value::Int(e as i32)),
        "hidepart" | "showpart" => {
            // hidepart(tag, [model]): sets (clears) the bit of the tag's part in the entity's
            // part mask. READ from the handlers (they differ by the one operation).
            h.out.push(Effect::HidePart { ent: e, tag: text(a, 0), hide: name == "hidepart" });
            Ok(HostReturn::None)
        }
        "physicslaunchserver" => Ok(HostReturn::None),
        "movegravity" => {
            // movegravity(velocity, time): the entity flies under gravity for `time` seconds
            // (trajectory type 5: from where it stands, with that velocity). The sim moves it
            // straight to where the arc ends (gravity 800), over the same time.
            let v = vec3(a, 0, "movegravity")?;
            let t = num(a, 1, "movegravity")?.max(0.0);
            let mut to = h.origin(e);
            to[0] += v[0] * t;
            to[1] += v[1] * t;
            to[2] += v[2] * t - 0.5 * 800.0 * t * t;
            h.out.push(Effect::MoveTo { ent: e, to, secs: t.max(0.05), accel: 0.0, decel: 0.0 });
            Ok(HostReturn::None)
        }
        "moveto" => {
            let to = vec3(a, 0, "moveto")?;
            let (secs, accel, decel) = move_times(num(a, 1, "moveto")?, opt_num(a, 2), opt_num(a, 3), "moveto")?;
            h.out.push(Effect::MoveTo { ent: e, to, secs, accel, decel });
            Ok(HostReturn::None)
        }
        "movex" | "movey" | "movez" => {
            let axis = match name {
                "movex" => 0,
                "movey" => 1,
                _ => 2,
            };
            let (secs, accel, decel) = move_times(num(a, 1, name)?, opt_num(a, 2), opt_num(a, 3), name)?;
            let mut to = h.origin(e);
            to[axis] += num(a, 0, name)?;
            h.out.push(Effect::MoveTo { ent: e, to, secs, accel, decel });
            Ok(HostReturn::None)
        }
        "rotateto" | "rotateto_shortest" => {
            let mut to = vec3(a, 0, name)?;
            if name == "rotateto_shortest" {
                // Each axis by the shorter way round (the handler wraps the difference).
                let cur = h.angles(e);
                for k in 0..3 {
                    let d = (to[k] - cur[k]) / 360.0;
                    to[k] = (d - (d + 0.5).floor()) * 360.0 + cur[k];
                }
            }
            let (secs, accel, decel) = move_times(num(a, 1, name)?, opt_num(a, 2), opt_num(a, 3), name)?;
            h.out.push(Effect::RotateTo { ent: e, to, secs, accel, decel });
            Ok(HostReturn::None)
        }
        "rotatepitch" | "rotateyaw" | "rotateroll" => {
            let axis = match name {
                "rotatepitch" => 0,
                "rotateyaw" => 1,
                _ => 2,
            };
            let (secs, accel, decel) = move_times(num(a, 1, name)?, opt_num(a, 2), opt_num(a, 3), name)?;
            let mut to = h.angles(e);
            to[axis] += num(a, 0, name)?;
            h.out.push(Effect::RotateTo { ent: e, to, secs, accel, decel });
            Ok(HostReturn::None)
        }
        "linkto" => {
            let parent = a.first().and_then(ent_id).ok_or("linkto: the parent is not an entity")?;
            let tag = a.get(1).map(text_of).unwrap_or_default();
            let offset = a.get(2).and_then(Value::as_vector).unwrap_or([0.0; 3]);
            let angles = a.get(3).and_then(Value::as_vector).unwrap_or([0.0; 3]);
            if let Some(x) = h.ent_mut(e) {
                x.parent = Some(parent);
            }
            h.out.push(Effect::Link { ent: e, parent, tag, offset, angles, explicit: a.len() >= 3 });
            Ok(HostReturn::None)
        }
        "unlink" => {
            if let Some(x) = h.ent_mut(e) {
                x.parent = None;
            }
            h.out.push(Effect::Unlink { ent: e });
            Ok(HostReturn::None)
        }
        "setmodel" => {
            let m = text(a, 0);
            h.set_field(e, F_MODEL, Value::str(&m));
            h.out.push(Effect::SetModel { ent: e, model: m });
            Ok(HostReturn::None)
        }
        "hide" | "show" => {
            let hidden = name == "hide";
            if let Some(x) = h.ent_mut(e) {
                x.hidden = hidden;
            }
            h.out.push(Effect::Hide { ent: e, hidden });
            Ok(HostReturn::None)
        }
        "solid" | "notsolid" => {
            let solid = name == "solid";
            if let Some(x) = h.ent_mut(e) {
                x.solid = solid;
            }
            h.out.push(Effect::Solid { ent: e, solid });
            Ok(HostReturn::None)
        }
        "connectpaths" | "disconnectpaths" => {
            h.out.push(Effect::Paths { ent: e, connect: name == "connectpaths" });
            Ok(HostReturn::None)
        }
        "delete" => {
            // (The engine refuses to delete an entity that still has linked children; here the
            // children are let go instead of killing the script.)
            for c in h.children_of(e) {
                if let Some(x) = h.ent_mut(c) {
                    x.parent = None;
                }
                h.out.push(Effect::Unlink { ent: c });
            }
            if let Some(x) = h.ents.get_mut(e as usize) {
                x.gone = true;
            }
            h.removed.push(e);
            h.out.push(Effect::Delete { ent: e });
            Ok(HostReturn::None)
        }
        "playsound" | "playsoundasmaster" | "playsoundonmovingent" => {
            let alias = text(a, 0);
            h.out.push(Effect::Sound { ent: Some(e), at: None, alias: alias.clone() });
            if let Some(n) = a.get(1).map(text_of).filter(|n| !n.is_empty()) {
                let ms = h.sound_ms.get(&alias.to_ascii_lowercase()).copied().unwrap_or(h.default_sound_ms);
                let due = h.now_ms + ms;
                h.timers.push((due, this.clone(), n));
            }
            Ok(HostReturn::None)
        }
        "playloopsound" => {
            h.out.push(Effect::LoopSound { ent: e, alias: Some(text(a, 0)) });
            Ok(HostReturn::None)
        }
        "stoploopsound" => {
            h.out.push(Effect::LoopSound { ent: e, alias: None });
            Ok(HostReturn::None)
        }
        "makeusable" | "makeunusable" => {
            h.out.push(Effect::Usable { ent: e, on: name == "makeusable" });
            Ok(HostReturn::None)
        }
        "sethintstring" => {
            h.out.push(Effect::Hint { ent: e, text: text(a, 0) });
            Ok(HostReturn::None)
        }
        "useanimtree" | "scriptmodelplayanim" => {
            let n = text(a, 0);
            let now = h.now_ms;
            if h.anim.play(e, &n, now) {
                h.out.push(Effect::Anim { ent: e, name: n, via_tree: name == "useanimtree" });
            }
            Ok(HostReturn::None)
        }
        "scriptmodelclearanim" => {
            h.anim.clear(e);
            h.out.push(Effect::ClearAnim { ent: e });
            Ok(HostReturn::None)
        }
        "setcursorhint" | "clonebrushmodeltoscriptmodel" | "setlightintensity" | "geteye" => match name {
            "geteye" => val(Value::Vector(h.origin(e))),
            _ => Ok(HostReturn::None),
        },
        "getlightintensity" => flt(1.0),
        "gettagorigin" => val(Value::Vector(h.tag_world(e, &text(a, 0)).0)),
        "gettagangles" => val(Value::Vector(h.tag_world(e, &text(a, 0)).1)),
        "getpointinbounds" => {
            // A point of the entity's bounds by fractions of each axis (0.5 is the middle).
            let (lo, hi) = bounds_of(h, e);
            let f = |i: usize| a.get(i).and_then(Value::as_float).unwrap_or(0.5);
            let o = h.origin(e);
            val(Value::Vector([o[0] + lo[0] + (hi[0] - lo[0]) * f(0), o[1] + lo[1] + (hi[1] - lo[1]) * f(1), o[2] + lo[2] + (hi[2] - lo[2]) * f(2)]))
        }
        "istouching" => {
            // Boxes overlap (an entity without a model counts as its origin, grown by 1).
            let Some(o) = a.first().and_then(ent_id) else { return val(Value::Int(0)) };
            let (la, ha) = bounds_of(h, e);
            let (lb, hb) = bounds_of(h, o);
            let (pa, pb) = (h.origin(e), h.origin(o));
            let hit = (0..3).all(|k| pa[k] + la[k] <= pb[k] + hb[k] && pb[k] + lb[k] <= pa[k] + ha[k]);
            val(Value::Int(i32::from(hit)))
        }
        "playlocalsound" | "playsoundtoteam" => Ok(HostReturn::None),
        "setorigin" => {
            let v = vec3(a, 0, "setorigin")?;
            h.set_field(e, F_ORIGIN, Value::Vector(v));
            h.out.push(Effect::Place { ent: e, origin: Some(v), angles: None });
            Ok(HostReturn::None)
        }
        "destroy" | "fadeovertime" | "settext" | "setshader" | "setvalue" | "settimer" => Ok(HostReturn::None),
        // Player data (rank, unlocks, settings): scripts read back what they wrote; the rest of
        // the profile reads as 0. A private store; the engine's profile is not modelled. The
        // family is one handler with a different data bank (READ: ranked 0, private 1, coop 2,
        // common 3, the player's own 4; `getcacplayerdata` reads the custom-class bank).
        "setrankedplayerdata" | "setprivateplayerdata" | "setplayerdata" | "setcoopplayerdata" | "setcommonplayerdata" => {
            set_pdata(h, pdata_bank(name), e, a);
            Ok(HostReturn::None)
        }
        "getrankedplayerdata" | "getprivateplayerdata" | "getplayerdata" | "getcoopplayerdata" | "getcommonplayerdata" | "getcacplayerdata" => {
            val(get_pdata(h, pdata_bank(name), e, a))
        }
        "setrankedplayerdatareservedint" | "setcommonplayerdatareservedint" => {
            // (name, int): the reserved ints of the bank, read back by the get form.
            let bank = if name.contains("ranked") { "ranked" } else { "common" };
            h.player_data.insert((e, format!("{bank}/reserved/{}", text(a, 0))), Value::Int(a.get(1).and_then(Value::as_int).unwrap_or(0)));
            Ok(HostReturn::None)
        }
        "getrankedplayerdatareservedint" => val(h.player_data.get(&(e, format!("ranked/reserved/{}", text(a, 0)))).cloned().unwrap_or(Value::Int(0))),
        // Player-side calls with nothing for the sim to do (HUD, settings, weapons the game
        // script owns, effects on the view): accepted; a script that asks for a value gets a sane
        // one (nothing pressed, standing, no weapons, no ammo).
        "setclientdvar" | "setclientomnvar" | "setdepthoffield" | "setoffhandprimaryclass" | "setoffhandsecondaryclass" | "setweaponammoclip" | "setweaponammostock"
        | "shellshock" | "givemaxammo" | "openpopupmenu" | "closepopupmenu" | "takeweapon" | "giveweapon" | "devaddpitch" | "devaddroll" | "devaddyaw"
        | "setperk" | "unsetperk" | "clearperks" | "iprintln" | "iprintlnbold" | "setweaponhudiconoverrides" | "setspawnweapon" | "suicide" => Ok(HostReturn::None),
        "buttonpressed" | "hasweapon" => val(Value::Int(0)),
        "getstance" => val(Value::str("stand")),
        "getplayerangles" => val(Value::Vector(h.angles(e))),
        "getweaponslistall" => val(Value::Array(gsc_iw6_vm::ArrRef::new())),
        "getweaponammoclip" | "getweaponammostock" => val(Value::Int(0)),
        "getguid" => val(Value::str(&format!("{:016x}", h.ent(e).and_then(|x| x.client).unwrap_or(0)))),
        _ => more_method(h, ctx, id, name, e, a),
    }
}

/// Methods the exe notes could not name.
fn method_unnamed(h: &mut Iw6Host, id: u16, _ctx: &mut CallCtx<'_>, this: &Value, a: &[Value]) -> R {
    let e = this.as_entity();
    if let Some(EntityId(p)) = e
        && let Some(r) = super::player::flag_method(h, id, p)
    {
        return r;
    }
    match id {
        // enablelinkto-like: sets a flag on a brush / model so it can be linked (flag 0x20).
        // Node / path flags and unnamed render flags with no effect on the sim.
        // Unnamed in the tables: sets entity flag 0x20000000 (READ; called on effect entities that are not
        // in createfx mode); nothing the sim shows.
        0x80d7 => Ok(HostReturn::None),
        // setmovingplatformenabled? on a brush entity.
        0x8375 => {
            if let Some(EntityId(e)) = e {
                h.out.push(Effect::Platform { ent: e, on: a.first().and_then(Value::as_int).unwrap_or(0) != 0 });
            }
            Ok(HostReturn::None)
        }
        _ => {
            *h.missing.entry(id).or_default() += 1;
            Ok(HostReturn::Unimplemented)
        }
    }
}

// ---- the rest of mp_frag's census (2026-11-05) ----------------------------------------------
//
// Every id below was read from its handler in the decompile (`docs/exe_notes/IW6_SCRIPT_CENSUS.md`,
// "Handlers read"). Marks as elsewhere: READ is what the handler does, INFERRED a meaning put to a
// value it only moves. The player-side calls keep what the script set and read it back; what has
// no meaning offline is a no-op or a plain answer (the VM never gets `undefined` back for them).

/// The data bank a player-data builtin addresses (READ: one handler with a bank number, ranked 0,
/// private 1, coop 2, common 3; the plain form has its own).
fn pdata_bank(name: &str) -> &'static str {
    for (needle, bank) in [("ranked", "ranked"), ("private", "private"), ("coop", "coop"), ("common", "common"), ("cac", "cac")] {
        if name.contains(needle) {
            return bank;
        }
    }
    "player"
}

/// Stores `value` at the path (the arguments but the last) of a bank; the engine's structured
/// data would check the path against the data definition, here any path is taken.
fn set_pdata(h: &mut Iw6Host, bank: &str, ent: u32, a: &[Value]) {
    if a.len() < 2 {
        return;
    }
    let path = a[..a.len().saturating_sub(1)].iter().map(text_of).collect::<Vec<_>>().join("/");
    h.player_data.insert((ent, format!("{bank}/{path}")), a.last().cloned().unwrap_or(Value::Undefined));
}

/// What a script stored at the path, else 0 (the profile of a player who has never played).
fn get_pdata(h: &Iw6Host, bank: &str, ent: u32, a: &[Value]) -> Value {
    let path = a.iter().map(text_of).collect::<Vec<_>>().join("/");
    h.player_data.get(&(ent, format!("{bank}/{path}"))).cloned().unwrap_or(Value::Int(0))
}

/// The match data's owner (the match is not a player's).
pub const MATCH: u32 = u32::MAX;

/// The point of the segment `a`..`b` nearest to `p` (READ: the parameter along the segment is the
/// projection of `p - a` on `b - a` over its squared length; the middle is exact in the handler,
/// INFERRED that the ends clamp to the segment's ends).
pub fn nearest_on_segment(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> [f32; 3] {
    let d = sub(b, a);
    let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if l2 == 0.0 {
        return a;
    }
    let pa = sub(p, a);
    let t = (pa[0] * d[0] + pa[1] * d[1] + pa[2] * d[2]) / l2;
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]
    }
}

/// Whether a player stands in the box a player would take at `origin` (READ: half 15 across and 35
/// high around the point 35 above it).
pub fn telefrag_hit(origin: [f32; 3], other: [f32; 3]) -> bool {
    let c = [origin[0], origin[1], origin[2] + 35.0];
    let o = [other[0], other[1], other[2] + 35.0];
    (c[0] - o[0]).abs() < 30.0 && (c[1] - o[1]).abs() < 30.0 && (c[2] - o[2]).abs() < 70.0
}

/// The spectate flags `allowspectateteam` takes (READ: the four constants it compares against).
pub const SPECTATE_TARGETS: [&str; 4] = ["axis", "allies", "freelook", "none"];

/// What a stub answers with.
#[derive(Clone, Copy)]
enum Stub {
    /// Returns nothing.
    Nothing,
    /// An int, 0.
    Zero,
    /// An int, 1.
    One,
    /// An empty string.
    Empty,
}

/// Methods read from their handlers (docs/exe_notes/IW6_SCRIPT_CENSUS.md) that touch a player's
/// state or the clients and have nothing to show the sim: what each answers with. `unk_` ids are
/// the ones whose handler is read but whose script name cannot be told.
fn method_stub(id: u16) -> Option<Stub> {
    use Stub::*;
    Some(match id {
        // Set or clear a flag on the entity or player; hud element moves and scales; client
        // commands; view and weapon housekeeping.
        0x8005 | 0x800a | 0x801f | 0x8029 | 0x804e | 0x8082 | 0x8083 | 0x8086 | 0x808f | 0x80a6 | 0x80a8 | 0x80ab | 0x80ac | 0x80b1 | 0x80c2
        | 0x80cf | 0x8176 | 0x817f | 0x81aa | 0x81ae | 0x8247 | 0x8265 | 0x8267 | 0x8293 | 0x8295 | 0x8296 | 0x8299 | 0x82bd | 0x82bf | 0x82c0
        | 0x82c8 | 0x82d5 | 0x8304 | 0x8309 | 0x831c | 0x831e | 0x8321 | 0x832c | 0x8337 | 0x833f | 0x8340 | 0x8352 | 0x83a7 | 0x83b7 | 0x83b8
        | 0x83e8 | 0x83fa | 0x83fb | 0x83fc | 0x8405 | 0x840d | 0x8423 => Nothing,
        // Getters of a bit or an int of the player state: the idle answer.
        0x8015 | 0x8016 | 0x8017 | 0x81db | 0x82c1 | 0x82d4 | 0x82de | 0x8330 | 0x8335 | 0x833e | 0x836d | 0x83f1 | 0x83fe | 0x8406 | 0x840c | 0x841d => Zero,
        // True for any player (the handler only checks that `this` is a client).
        0x8291 | 0x82dc => One,
        0x8306 | 0x83d8 | 0x83d9 | 0x83da => Empty,
        _ => return None,
    })
}

fn stub_value(s: Stub) -> R {
    match s {
        Stub::Nothing => Ok(HostReturn::None),
        Stub::Zero => val(Value::Int(0)),
        Stub::One => val(Value::Int(1)),
        Stub::Empty => val(Value::str("")),
    }
}

/// Builtin functions of the rest of the census.
fn more_function(h: &mut Iw6Host, id: u16, name: &str, a: &[Value]) -> R {
    match name {
        // ---- structured data: the match's own bank ---------------------------------------
        "setmatchdata" | "setclientmatchdata" => {
            set_pdata(h, if name == "setmatchdata" { "match" } else { "clientmatch" }, MATCH, a);
            Ok(HostReturn::None)
        }
        "getmatchdata" | "getclientmatchdata" => val(get_pdata(h, if name == "getmatchdata" { "match" } else { "clientmatch" }, MATCH, a)),
        "sendmatchdata" | "sendclientmatchdata" | "setclientmatchdatadef" => Ok(HostReturn::None),
        // ---- teams ----------------------------------------------------------------------
        "getteamscore" | "setteamscore" => {
            let team = text(a, 0).to_ascii_lowercase();
            if team != "axis" && team != "allies" {
                return Err(format!("{name}: team must be axis or allies, not '{team}'"));
            }
            if name == "setteamscore" {
                let v = a.get(1).and_then(Value::as_int).unwrap_or(0);
                h.stores.team_scores.insert(team, v);
                return Ok(HostReturn::None);
            }
            val(Value::Int(h.stores.team_scores.get(&team).copied().unwrap_or(0)))
        }
        "newteamhudelem" => val(Value::Object(ObjRef::new_struct())),
        // ---- geometry and traces ---------------------------------------------------------
        "pointonsegmentnearesttopoint" => val(Value::Vector(nearest_on_segment(vec3(a, 0, name)?, vec3(a, 1, name)?, vec3(a, 2, name)?))),
        "positionwouldtelefrag" => {
            let at = vec3(a, 0, name)?;
            let hit = h.players.values().any(|&p| h.ent(p).is_some() && telefrag_hit(at, h.origin(p)));
            val(Value::Int(i32::from(hit)))
        }
        // The map is not traced here: nothing is in the way (see `bullettrace`).
        "sighttracepassed" | "bullettracepassed" | "unk_0159" => val(Value::Int(1)),
        "playerphysicstrace" => val(Value::Vector(vec3(a, 1, name)?)),
        "getspawnarray" => {
            if a.is_empty() {
                return Err("GetSpawnArray requires a spawn entity's name.".into());
            }
            val(ent_array(h.find("classname", &text(a, 0))))
        }
        // ---- level control ---------------------------------------------------------------
        "map_restart" | "exitlevel" => {
            let persist = a.first().and_then(Value::as_int).unwrap_or(0) != 0;
            h.stores.level_exit = Some((name == "map_restart", persist));
            h.log.push(format!("{name}({persist}): the sim ends the match itself"));
            Ok(HostReturn::None)
        }
        "setslowmotion" => {
            let start = num(a, 0, name)?;
            let end = a.get(1).and_then(Value::as_float).unwrap_or(1.0);
            let secs = a.get(2).and_then(Value::as_float).unwrap_or(1.0);
            h.stores.slow_motion = Some((start, end, secs));
            Ok(HostReturn::None)
        }
        "kick" => {
            h.log.push(format!("kick({}): not carried out", text(a, 0)));
            Ok(HostReturn::None)
        }
        // ---- weapons (no weapon tables are loaded) --------------------------------------
        "weaponclipsize" | "unk_012f" | "unk_0061" | "unk_0250" | "unk_0251" | "unk_0253" => val(Value::Int(0)),
        "weaponinventorytype" => val(Value::str("primary")),
        "getweaponmodel" => val(Value::str("")),
        // Stubs of the dedicated server, magic grenades, the persistence screens, flags: nothing
        // for the sim (READ: the handlers only set a global or send a client command).
        "unk_0060" | "unk_0062" | "unk_00ba" | "unk_00bb" | "unk_00ee" | "bullettracer" | "unk_0134" | "unk_0135" | "unk_014c" | "unk_0157" | "unk_0169"
        | "unk_0187" | "unk_021e" | "unk_023a" | "unk_023b" | "unk_0248" | "unk_024f" => Ok(HostReturn::None),
        _ => {
            *h.missing.entry(id).or_default() += 1;
            Ok(HostReturn::Unimplemented)
        }
    }
}

/// Builtin methods of the rest of the census (`e` is the entity the method is called on).
fn more_method(h: &mut Iw6Host, _ctx: &mut CallCtx<'_>, id: u16, name: &str, e: u32, a: &[Value]) -> R {
    match name {
        "setplayerangles" => {
            // The view angles of a player; the sim keeps the entity's angles.
            if let Some(v) = a.first().and_then(Value::as_vector) {
                if let Some(x) = h.ents.get_mut(e as usize) {
                    x.fields.insert(Name::Id(super::host::F_ANGLES), Value::Vector(v));
                }
            }
            Ok(HostReturn::None)
        }
        // The player this spectator follows: nobody is modelled.
        "getspectatingplayer" => val(Value::Undefined),
        // Screen blur for a player: nothing draws it here.
        "setblurforplayer" => Ok(HostReturn::None),
        // The class of the offhand secondary slot ("flash", "smoke", ...); the loadout is not modelled.
        "getoffhandsecondaryclass" => val(Value::str("flash")),
        "isitemunlocked" => {
            // READ: 2 for a name the unlock table has no row for, else whether the player has it
            // (everything is open here: the profile is not modelled).
            let n = text(a, 0);
            let known = h.tables.get("mp/unlocktable.csv").is_none_or(|t| (0..t.rows).any(|r| t.cell(r, 0).is_some_and(|c| c.eq_ignore_ascii_case(&n))));
            val(Value::Int(if known { 1 } else { 2 }))
        }
        "getxuid" => {
            // The 64-bit id as lower case hex; "0" for a non-client. INFERRED the id's form.
            match h.ent(e).and_then(|x| x.client) {
                Some(c) => val(Value::str(&format!("{:x}", 0x0110_0001_0000_0000u64 + u64::from(c)))),
                None => val(Value::str("0")),
            }
        }
        "allowspectateteam" => {
            // allowspectateteam(team|"freelook"|"none", allowed): READ the four names; a nonzero
            // value allows (clears the restriction bit).
            let t = text(a, 0).to_ascii_lowercase();
            if !SPECTATE_TARGETS.contains(&t.as_str()) {
                return Err(format!("allowspectateteam: unknown target '{t}'"));
            }
            let allowed = a.get(1).and_then(Value::as_int).unwrap_or(0) != 0;
            h.stores.spectate.insert((e, t), allowed);
            Ok(HostReturn::None)
        }
        "notifyonplayercommand" => {
            // (notify, command): the command's press says the notify on the player (the sim's
            // input calls `commands_for`).
            let (notify, command) = (text(a, 0), text(a, 1));
            let v = h.stores.commands.entry((e, command)).or_default();
            if !v.contains(&notify) {
                v.push(notify);
            }
            Ok(HostReturn::None)
        }
        "setclientdvars" => {
            if a.len() % 2 != 0 {
                return Err("setclientdvars: the arguments are name and value pairs".into());
            }
            for kv in a.chunks(2) {
                h.stores.client_dvars.insert((e, text_of(&kv[0])), text_of(&kv[1]));
            }
            Ok(HostReturn::None)
        }
        "setstance" => {
            h.stores.stance.insert(e, text(a, 0));
            Ok(HostReturn::None)
        }
        "setmovespeedscale" => {
            h.stores.move_speed_scale.insert(e, num(a, 0, name)?);
            Ok(HostReturn::None)
        }
        "setcontents" => {
            let old = h.stores.contents.insert(e, a.first().and_then(Value::as_int).unwrap_or(0)).unwrap_or(0);
            val(Value::Int(old))
        }
        "setcamerapoint" => {
            // (index < 4, origin, angles): READ the range check; the handler keeps both vectors.
            if let Some(i) = a.first().and_then(Value::as_int).filter(|i| (0..4).contains(i)) {
                h.stores.camera.insert((e, i as u32), (vec3(a, 1, name)?, vec3(a, 2, name)?));
            }
            Ok(HostReturn::None)
        }
        "playerads" => flt(0.0),
        "getweaponslistprimaries" | "getweaponslistoffhands" => val(Value::Array(ArrRef::new())),
        "viewkick" | "scaleovertime" | "moveovertime" | "setpulsefx" | "cloneplayer" | "takeallweapons" => Ok(HostReturn::None),
        _ if name.starts_with("unk_") => match method_stub(id) {
            Some(s) => stub_value(s),
            None => {
                *h.missing.entry(id).or_default() += 1;
                Ok(HostReturn::Unimplemented)
            }
        },
        _ => {
            *h.missing.entry(id).or_default() += 1;
            Ok(HostReturn::Unimplemented)
        }
    }
}

/// The notifies a pressed player command asks for (`notifyonplayercommand`).
pub fn commands_for(h: &Iw6Host, player: u32, command: &str) -> Vec<String> {
    h.stores.commands.get(&(player, command.to_owned())).cloned().unwrap_or_default()
}

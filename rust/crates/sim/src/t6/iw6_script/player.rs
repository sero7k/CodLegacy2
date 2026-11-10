//! The player-side and HUD builtins of the Ghosts map scripts: the state a script builds on a player
//! (weapons, ammo, offhand classes, perks, client variables and dvars, flags, menus, views) and the
//! HUD elements it creates, kept as the engine's handlers keep them so a script reads back what it set.
//!
//! READ from the handlers (`docs/exe_notes/IW6_GSC_BUILTINS.md`, `iw6_builtins.tsv`): the argument
//! counts and ranges that are errors in the engine are errors here (`settimer` needs exactly one
//! argument and at least a millisecond, `setshader` one or three, `fadeovertime` is in (0, 60] s,
//! `setperk` takes exactly three); a HUD element's kind is the handler's (`settext` 1, `setvalue`
//! 2, `setshader` 4, `settimer` 5) and each setter clears the others' state. What the sim does not
//! model (the loadout tables, the profile) answers plainly, marked INFERRED where a value is put
//! to a name the handler only moves. Nothing here touches the sim `World`: effects go out as
//! [`Effect`]s, the HUD is polled with [`Iw6Host::hud_views`].

use std::collections::{BTreeMap, BTreeSet};

use gsc_iw6_vm::{ArrRef, HostReturn, Name, ObjRef, Value};

use super::host::{text_of, Effect, Iw6Host, F_HEALTH};

type R = Result<HostReturn, String>;

/// A weapon a player carries.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponSlot {
    pub name: String,
    pub clip: i32,
    pub stock: i32,
}

/// What the scripts built on one player.
#[derive(Clone, Debug, Default)]
pub struct PlayerState {
    pub weapons: Vec<WeaponSlot>,
    /// The weapon in hand; empty is `none`.
    pub current: String,
    pub offhand_primary: Option<u8>,
    pub offhand_secondary: Option<u8>,
    /// Perks by name, with the engine's two arguments (bit, slot).
    pub perks: BTreeMap<String, (bool, bool)>,
    pub omnvars: BTreeMap<String, Value>,
    /// `setclientdvar(s)`.
    pub dvars: BTreeMap<String, String>,
    /// Client flag bits the flag setters changed.
    pub flags: u32,
    pub menu: Option<String>,
    pub messages: Vec<String>,
    pub shellshock: Option<(String, f32)>,
    pub view_kick: Option<f32>,
    pub hud_icon_overrides: Vec<Value>,
}

/// The offhand class names (READ: six constant strings, the index is stored; the first, `none`, and
/// the order are INFERRED from the scripts' calls).
pub const OFFHAND_CLASSES: [&str; 6] = ["none", "frag", "throwingknife", "smoke", "flash", "other"];

/// Weapons the carried list calls offhands (grenades and gadgets): INFERRED from Ghosts' names.
pub fn is_offhand(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("grenade") || n.starts_with("throwingknife") || n.starts_with("c4") || n.starts_with("claymore") || n.starts_with("trophy") || n.starts_with("bouncingbetty") || n.starts_with("scrambler")
}

/// Client flag changes the unnamed flag setters make: (builtin id, bit, set). READ from the
/// handlers (each sets or clears one bit of the client's flag word); what the flags mean is not known.
pub const FLAG_OPS: [(u16, u32, bool); 14] = [
    (0x8310, 0x80, true),
    (0x8311, 0x80, false),
    (0x8312, 0x1000, true),
    (0x8313, 0x1000, false),
    (0x8314, 0x800, true),
    (0x8315, 0x800, false),
    (0x82bf, 0x8000, true),
    (0x831d, 0x100, true),
    (0x82d6, 0x400, true),
    (0x82d7, 0x400, false),
    (0x800a, 0x0200_0000, true),
    (0x831c, 0x10, true),
    (0x831e, 0x10, false),
    (0x80d7, 0x2000_0000, true),
];

/// The flag bit a flag-setter id changes (and whether it sets it).
pub fn flag_op(id: u16) -> Option<(u32, bool)> {
    FLAG_OPS.iter().find(|(i, _, _)| *i == id).map(|&(_, bit, set)| (bit, set))
}

/// A HUD element's kind (READ: the type word the setters write).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HudKind {
    #[default]
    None,
    Text,
    Value,
    Shader,
    Timer,
}

/// A HUD element the scripts made; the fields they write directly (`x`, `y`, `alpha`, `color`, ...)
/// live on its script object.
#[derive(Clone, Debug)]
pub struct HudRec {
    pub obj: ObjRef,
    /// Shown to one player only (`newclienthudelem`).
    pub client: Option<u32>,
    /// Shown to one team only (`newteamhudelem`).
    pub team: Option<String>,
    pub kind: HudKind,
    pub text: String,
    pub value: f32,
    pub shader: String,
    pub size: [i32; 2],
    /// A timer's end in level ms.
    pub timer_end: i64,
    /// `fadeovertime`: (start ms, length ms, alpha and colour at the start).
    pub fade: Option<(i64, i64, f32, [f32; 3])>,
    /// `moveovertime`: (start ms, length ms, x and y at the start).
    pub mv: Option<(i64, i64, [f32; 2])>,
    /// `scaleovertime`: (start ms, length ms, width and height at the start).
    pub scale: Option<(i64, i64, [i32; 2])>,
    pub pulse: Option<[i32; 4]>,
    pub alive: bool,
}

/// One HUD element as a renderer would draw it right now.
#[derive(Clone, Debug, PartialEq)]
pub struct HudView {
    pub id: usize,
    pub client: Option<u32>,
    pub team: Option<String>,
    pub kind: HudKind,
    pub text: String,
    pub value: f32,
    pub shader: String,
    pub size: [i32; 2],
    pub pos: [f32; 2],
    pub alpha: f32,
    pub color: [f32; 3],
    pub font_scale: f32,
    /// Seconds left on a timer (0 when it ran out or the element is not a timer).
    pub timer_left: f32,
}

const HUD_MARK: &str = "__hud";

fn num(a: &[Value], i: usize, what: &str) -> Result<f32, String> {
    a.get(i).and_then(Value::as_float).ok_or_else(|| format!("{what}: argument {} is not a number", i + 1))
}

fn val(v: Value) -> R {
    Ok(HostReturn::Value(v))
}

/// The weapon name as the carried list keys it.
fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

impl Iw6Host {
    pub fn pstate(&mut self, e: u32) -> &mut PlayerState {
        self.pstates.entry(e).or_default()
    }

    /// `(clip, stock)` a weapon starts with; set from the weapon files when the glue has them.
    pub fn weapon_ammo(&self, name: &str) -> (i32, i32) {
        self.weapon_info.get(&key(name)).copied().unwrap_or((0, 0))
    }

    fn hud_obj(&self, obj: &ObjRef) -> Option<usize> {
        obj.get(&Name::Text(HUD_MARK.into())).and_then(|v| v.as_int()).map(|i| i as usize)
    }

    fn hud_f(&self, obj: &ObjRef, field: &str) -> Option<f32> {
        obj.get(&self.key_name(field)).and_then(|v| v.as_float())
    }

    /// A new HUD element object: a script struct whose fields the script writes, registered here.
    pub fn new_hud(&mut self, client: Option<u32>, team: Option<String>) -> Value {
        let obj = ObjRef::new_struct();
        obj.set(Name::Text(HUD_MARK.into()), Value::Int(self.huds.len() as i32));
        self.huds.push(HudRec {
            obj: obj.clone(),
            client,
            team,
            kind: HudKind::None,
            text: String::new(),
            value: 0.0,
            shader: String::new(),
            size: [0, 0],
            timer_end: 0,
            fade: None,
            mv: None,
            scale: None,
            pulse: None,
            alive: true,
        });
        Value::Object(obj)
    }

    /// The elements to draw at level time `now_ms`: live ones with their fade, move and scale in
    /// progress applied and timers counted down.
    pub fn hud_views(&self, now_ms: i64) -> Vec<HudView> {
        let mut out = Vec::new();
        for (id, h) in self.huds.iter().enumerate().filter(|(_, h)| h.alive) {
            let f = |name: &str, d: f32| self.hud_f(&h.obj, name).unwrap_or(d);
            let target_alpha = f("alpha", 1.0);
            let target_color = match h.obj.get(&self.key_name("color")) {
                Some(Value::Vector(c)) => c,
                _ => [1.0; 3],
            };
            let progress = |start: i64, len: i64| ((now_ms - start) as f32 / len.max(1) as f32).clamp(0.0, 1.0);
            let (alpha, color) = match h.fade {
                Some((start, len, a0, c0)) => {
                    let t = progress(start, len);
                    (a0 + (target_alpha - a0) * t, std::array::from_fn(|k| c0[k] + (target_color[k] - c0[k]) * t))
                }
                None => (target_alpha, target_color),
            };
            let (tx, ty) = (f("x", 0.0), f("y", 0.0));
            let pos = match h.mv {
                Some((start, len, p0)) => {
                    let t = progress(start, len);
                    [p0[0] + (tx - p0[0]) * t, p0[1] + (ty - p0[1]) * t]
                }
                None => [tx, ty],
            };
            let size = match h.scale {
                Some((start, len, s0)) => {
                    let t = progress(start, len);
                    std::array::from_fn(|k| (s0[k] as f32 + (h.size[k] - s0[k]) as f32 * t).round() as i32)
                }
                None => h.size,
            };
            out.push(HudView {
                id,
                client: h.client,
                team: h.team.clone(),
                kind: h.kind,
                text: h.text.clone(),
                value: h.value,
                shader: h.shader.clone(),
                size,
                pos,
                alpha,
                color,
                font_scale: f("fontscale", 1.0),
                timer_left: if h.kind == HudKind::Timer { ((h.timer_end - now_ms).max(0)) as f32 / 1000.0 } else { 0.0 },
            });
        }
        out
    }

    /// Whether a player has a flag bit set by the flag setters.
    pub fn client_flag(&self, e: u32, bit: u32) -> bool {
        self.pstates.get(&e).is_some_and(|p| p.flags & bit != 0)
    }

    /// Whether a player is a bot (a roster entry the sim says is one).
    pub fn is_bot(&self, e: u32) -> bool {
        self.ent(e).is_some_and(|x| x.bot)
    }
}

/// A method on a HUD element object (`this` is the script struct `newhudelem` returned).
pub fn hud_method(h: &mut Iw6Host, name: &str, obj: &ObjRef, a: &[Value]) -> Option<R> {
    let id = h.hud_obj(obj)?;
    let now = h.now_ms;
    let alpha_now = h.hud_f(obj, "alpha").unwrap_or(1.0);
    let color_now = match obj.get(&h.key_name("color")) {
        Some(Value::Vector(c)) => c,
        _ => [1.0; 3],
    };
    let (x_now, y_now) = (h.hud_f(obj, "x").unwrap_or(0.0), h.hud_f(obj, "y").unwrap_or(0.0));
    let rec = h.huds.get_mut(id)?;
    let r: R = match name {
        "settext" => {
            rec.kind = HudKind::Text;
            rec.text = a.iter().map(text_of).collect();
            Ok(HostReturn::None)
        }
        "setvalue" => match num(a, 0, name) {
            Ok(v) => {
                rec.kind = HudKind::Value;
                rec.value = v;
                Ok(HostReturn::None)
            }
            Err(e) => Err(e),
        },
        "setshader" => {
            if !matches!(a.len(), 1 | 3) {
                return Some(Err("setshader: one argument, or three with the size".into()));
            }
            let m = a.first().map(text_of).unwrap_or_default();
            if m.is_empty() {
                return Some(Err("setshader: the material name is empty".into()));
            }
            let (w, hh) = if a.len() == 3 { (a[1].as_int().unwrap_or(0), a[2].as_int().unwrap_or(0)) } else { (0, 0) };
            if w < 0 || hh < 0 {
                return Some(Err("setshader: width and height must not be negative".into()));
            }
            rec.kind = HudKind::Shader;
            rec.shader = m;
            rec.size = [w, hh];
            Ok(HostReturn::None)
        }
        "settimer" => {
            if a.len() != 1 {
                return Some(Err("settimer: needs one argument".into()));
            }
            let ms = (num(a, 0, name).ok()? * 1000.0).ceil() as i64;
            if ms < 1 {
                return Some(Err("settimer: the time must be at least a millisecond".into()));
            }
            rec.kind = HudKind::Timer;
            rec.timer_end = now + ms;
            Ok(HostReturn::None)
        }
        "fadeovertime" => {
            let s = num(a, 0, name).ok()?;
            if s <= 0.0 || s > 60.0 {
                return Some(Err("fadeovertime: the time must be above 0 and at most 60 seconds".into()));
            }
            rec.fade = Some((now, (s * 1000.0 + 0.5).floor() as i64, alpha_now, color_now));
            Ok(HostReturn::None)
        }
        "moveovertime" => {
            let s = num(a, 0, name).ok()?;
            if s <= 0.0 || s > 60.0 {
                return Some(Err("moveovertime: the time must be above 0 and at most 60 seconds".into()));
            }
            rec.mv = Some((now, (s * 1000.0 + 0.5).floor() as i64, [x_now, y_now]));
            Ok(HostReturn::None)
        }
        "scaleovertime" => {
            let s = num(a, 0, name).ok()?;
            if s <= 0.0 || s > 60.0 {
                return Some(Err("scaleovertime: the time must be above 0 and at most 60 seconds".into()));
            }
            let from = rec.size;
            rec.size = [a.get(1).and_then(Value::as_int).unwrap_or(from[0]), a.get(2).and_then(Value::as_int).unwrap_or(from[1])];
            rec.scale = Some((now, (s * 1000.0 + 0.5).floor() as i64, from));
            Ok(HostReturn::None)
        }
        "setpulsefx" => {
            rec.pulse = Some(std::array::from_fn(|k| a.get(k).and_then(Value::as_int).unwrap_or(0)));
            Ok(HostReturn::None)
        }
        "destroy" => {
            rec.alive = false;
            Ok(HostReturn::None)
        }
        _ => return None,
    };
    Some(r)
}

/// Player and weapon methods that keep state; `None` when the name is not one of them.
pub fn method(h: &mut Iw6Host, name: &str, e: u32, a: &[Value]) -> Option<R> {
    let text = |i: usize| a.get(i).map(text_of).unwrap_or_default();
    let r: R = match name {
        "giveweapon" => {
            let w = text(0);
            if w.is_empty() {
                return Some(Err("giveweapon: the weapon name is empty".into()));
            }
            let (clip, stock) = h.weapon_ammo(&w);
            let p = h.pstate(e);
            if !p.weapons.iter().any(|s| s.name.eq_ignore_ascii_case(&w)) {
                p.weapons.push(WeaponSlot { name: w.clone(), clip, stock });
            }
            Ok(HostReturn::None)
        }
        "takeweapon" => {
            let w = text(0);
            let p = h.pstate(e);
            p.weapons.retain(|s| !s.name.eq_ignore_ascii_case(&w));
            if p.current.eq_ignore_ascii_case(&w) {
                p.current.clear();
            }
            Ok(HostReturn::None)
        }
        "takeallweapons" => {
            let p = h.pstate(e);
            p.weapons.clear();
            p.current.clear();
            Ok(HostReturn::None)
        }
        "hasweapon" => {
            let w = text(0);
            val(Value::Int(i32::from(h.pstate(e).weapons.iter().any(|s| s.name.eq_ignore_ascii_case(&w)))))
        }
        "setspawnweapon" => {
            let w = text(0);
            let (clip, stock) = h.weapon_ammo(&w);
            let p = h.pstate(e);
            if !p.weapons.iter().any(|s| s.name.eq_ignore_ascii_case(&w)) {
                p.weapons.push(WeaponSlot { name: w.clone(), clip, stock });
            }
            p.current = w;
            Ok(HostReturn::None)
        }
        "getcurrentweapon" => {
            let p = h.pstate(e);
            val(Value::str(if p.current.is_empty() { "none" } else { &p.current }))
        }
        "getweaponslistall" => val(Value::array(h.pstate(e).weapons.iter().map(|s| Value::str(&s.name)).collect::<Vec<_>>())),
        "getweaponslistprimaries" => val(Value::array(h.pstate(e).weapons.iter().filter(|s| !is_offhand(&s.name)).map(|s| Value::str(&s.name)).collect::<Vec<_>>())),
        "getweaponslistoffhands" => val(Value::array(h.pstate(e).weapons.iter().filter(|s| is_offhand(&s.name)).map(|s| Value::str(&s.name)).collect::<Vec<_>>())),
        "setweaponammoclip" | "setweaponammostock" => {
            let w = text(0);
            let n = a.get(1).and_then(Value::as_int).unwrap_or(0).max(0);
            let stock = name == "setweaponammostock";
            if let Some(s) = h.pstate(e).weapons.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&w)) {
                if stock {
                    s.stock = n;
                } else {
                    s.clip = n;
                }
            }
            Ok(HostReturn::None)
        }
        "getweaponammoclip" | "getweaponammostock" => {
            let w = text(0);
            let v = h.pstate(e).weapons.iter().find(|s| s.name.eq_ignore_ascii_case(&w)).map_or(0, |s| if name == "getweaponammoclip" { s.clip } else { s.stock });
            val(Value::Int(v))
        }
        "givemaxammo" => {
            let w = text(0);
            let (clip, stock) = h.weapon_ammo(&w);
            if let Some(s) = h.pstate(e).weapons.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&w)) {
                s.clip = s.clip.max(clip);
                s.stock = s.stock.max(stock);
            }
            Ok(HostReturn::None)
        }
        "setoffhandprimaryclass" | "setoffhandsecondaryclass" => {
            if a.len() != 1 {
                return Some(Err(format!("{name}: needs one argument")));
            }
            let c = text(0).to_ascii_lowercase();
            let Some(i) = OFFHAND_CLASSES.iter().position(|n| *n == c) else {
                return Some(Err(format!("{name}: unknown offhand class '{c}'")));
            };
            let p = h.pstate(e);
            if name == "setoffhandprimaryclass" {
                p.offhand_primary = Some(i as u8);
            } else {
                p.offhand_secondary = Some(i as u8);
            }
            Ok(HostReturn::None)
        }
        "getoffhandsecondaryclass" => {
            let i = h.pstate(e).offhand_secondary.unwrap_or(4) as usize;
            val(Value::str(OFFHAND_CLASSES[i.min(5)]))
        }
        "setperk" => {
            if a.len() != 3 {
                return Some(Err("setperk: needs the perk and two flags".into()));
            }
            let perk = text(0).to_ascii_lowercase();
            if perk.is_empty() {
                return Some(Err("setperk: the perk name is empty".into()));
            }
            let flags = (a[1].as_int().unwrap_or(0) != 0, a[2].as_int().unwrap_or(0) != 0);
            h.pstate(e).perks.insert(perk, flags);
            Ok(HostReturn::None)
        }
        "unsetperk" => {
            let perk = text(0).to_ascii_lowercase();
            h.pstate(e).perks.remove(&perk);
            Ok(HostReturn::None)
        }
        "clearperks" => {
            h.pstate(e).perks.clear();
            Ok(HostReturn::None)
        }
        "hasperk" => {
            let perk = text(0).to_ascii_lowercase();
            val(Value::Int(i32::from(h.pstate(e).perks.contains_key(&perk))))
        }
        "setclientomnvar" => {
            if a.len() != 2 {
                return Some(Err("setclientomnvar: needs a name and a value".into()));
            }
            let n = text(0);
            h.pstate(e).omnvars.insert(n.clone(), a[1].clone());
            h.out.push(Effect::Omnvar { ent: e, name: n, value: a[1].as_float().unwrap_or(0.0) });
            Ok(HostReturn::None)
        }
        "setclientdvar" => {
            let (n, v) = (text(0), text(1));
            h.stores.client_dvars.insert((e, n.clone()), v.clone());
            h.pstate(e).dvars.insert(n, v);
            Ok(HostReturn::None)
        }
        "playlocalsound" => {
            // Bots and agents hear nothing (READ: the handler skips them).
            if !h.is_bot(e) {
                h.out.push(Effect::LocalSound { ent: e, alias: text(0) });
            }
            Ok(HostReturn::None)
        }
        "playsoundtoteam" => {
            // playsoundtoteam(alias, team, [excluded player]): READ the team must be allies or axis.
            let team = text(1).to_ascii_lowercase();
            if team != "allies" && team != "axis" {
                return Some(Err(format!("playsoundtoteam: team must be allies or axis, not '{team}'")));
            }
            h.out.push(Effect::Sound { ent: Some(e), at: None, alias: text(0) });
            Ok(HostReturn::None)
        }
        "shellshock" => {
            let d = num(a, 1, name).ok()?;
            if !(0.0..=60.0).contains(&d) {
                return Some(Err("shellshock: the duration is 0 to 60 seconds".into()));
            }
            h.pstate(e).shellshock = Some((text(0), d));
            h.out.push(Effect::Shellshock { ent: e, name: text(0), secs: d });
            Ok(HostReturn::None)
        }
        "viewkick" => {
            let amount = num(a, 0, name).ok()?;
            h.pstate(e).view_kick = Some(amount);
            Ok(HostReturn::None)
        }
        "iprintln" | "iprintlnbold" => {
            let m = text(0);
            h.pstate(e).messages.push(m);
            Ok(HostReturn::None)
        }
        "openpopupmenu" => {
            h.pstate(e).menu = Some(text(0));
            Ok(HostReturn::None)
        }
        "closepopupmenu" => {
            h.pstate(e).menu = None;
            Ok(HostReturn::None)
        }
        "setweaponhudiconoverrides" => {
            h.pstate(e).hud_icon_overrides = a.to_vec();
            Ok(HostReturn::None)
        }
        "suicide" => {
            h.set_field(e, F_HEALTH, Value::Int(0));
            h.out.push(Effect::Kill { ent: e });
            Ok(HostReturn::None)
        }
        "cloneplayer" => {
            h.out.push(Effect::CloneCorpse { ent: e, secs: a.first().and_then(Value::as_float).unwrap_or(0.0) });
            Ok(HostReturn::None)
        }
        "setlightintensity" => {
            let v = num(a, 0, name).ok()?;
            if v < 0.0 {
                return Some(Err("setlightintensity: the intensity must not be negative".into()));
            }
            h.stores.light_intensity.insert(e, v);
            h.out.push(Effect::LightIntensity { ent: e, value: v });
            Ok(HostReturn::None)
        }
        "getlightintensity" => val(Value::Float(h.stores.light_intensity.get(&e).copied().unwrap_or(1.0))),
        "geteye" => {
            // The eye of a player: the stance's view height above the origin (INFERRED from the
            // movement tables: stand 60, crouch 40, prone 11); anything else is its origin.
            let o = h.origin(e);
            let up = if h.ent(e).is_some_and(|x| x.client.is_some()) {
                match h.stores.stance.get(&e).map(String::as_str) {
                    Some("crouch") => 40.0,
                    Some("prone") => 11.0,
                    _ => 60.0,
                }
            } else {
                0.0
            };
            val(Value::Vector([o[0], o[1], o[2] + up]))
        }
        "getstance" => val(Value::str(h.stores.stance.get(&e).map_or("stand", String::as_str))),
        "setdepthoffield" => {
            // Six floats (near start, near end, far start, far end, near blur, far blur); READ the
            // blur strengths are 4 to 10.
            if a.len() != 6 {
                return Some(Err("setdepthoffield: needs six numbers".into()));
            }
            let v: Vec<f32> = a.iter().map(|x| x.as_float().unwrap_or(0.0)).collect();
            if !(4.0..=10.0).contains(&v[4]) || !(4.0..=10.0).contains(&v[5]) {
                return Some(Err("setdepthoffield: the blur strengths are 4 to 10".into()));
            }
            h.stores.depth_of_field.insert(e, [v[0], v[1], v[2], v[3], v[4], v[5]]);
            Ok(HostReturn::None)
        }
        "enablelinkto" => {
            h.pstate(e).flags |= 0x20;
            Ok(HostReturn::None)
        }
        _ => return None,
    };
    Some(r)
}

/// Functions that keep state; `None` when the name is not one of them.
pub fn function(h: &mut Iw6Host, name: &str, a: &[Value]) -> Option<R> {
    let text = |i: usize| a.get(i).map(text_of).unwrap_or_default();
    let r: R = match name {
        "newhudelem" => val(h.new_hud(None, None)),
        "newclienthudelem" => {
            let p = a.first().and_then(|v| v.as_entity()).map(|e| e.0);
            if p.is_none() {
                return Some(Err("newclienthudelem: the argument is not a player".into()));
            }
            val(h.new_hud(p, None))
        }
        "newteamhudelem" => {
            let t = text(0).to_ascii_lowercase();
            if t != "allies" && t != "axis" {
                return Some(Err(format!("newteamhudelem: team must be allies or axis, not '{t}'")));
            }
            val(h.new_hud(None, Some(t)))
        }
        "isalive" => {
            // READ: the entity's health is above zero.
            let alive = a.first().and_then(|v| v.as_entity()).is_some_and(|e| h.ent(e.0).is_some_and(|x| x.client.is_none() || x.health_above_zero(h.field(e.0, F_HEALTH))));
            val(Value::Int(i32::from(alive)))
        }
        "isbot" | "isagent" | "isai" => {
            let e = a.first().and_then(|v| v.as_entity()).map(|e| e.0);
            let bot = e.is_some_and(|e| h.is_bot(e));
            // (Agents are not modelled: `isagent` is false for everyone.)
            val(Value::Int(i32::from(bot && name != "isagent")))
        }
        "precachemodel" | "precacheshader" | "precachestring" | "precacheitem" | "precachemenu" | "precacherumble" | "precachemodel_" => {
            let kind = name.trim_start_matches("precache");
            h.precached.entry(kind.to_owned()).or_default().insert(text(0).to_ascii_lowercase());
            Ok(HostReturn::None)
        }
        "visionsetnaked" | "visionsetthermal" | "visionsetpain" | "visionsetnight" | "visionsetmissilecam" => {
            // (name, [time seconds, default 1]): READ the default of 1000 ms.
            let secs = a.get(1).and_then(Value::as_float).unwrap_or(1.0);
            let n = text(0);
            h.stores.vision.insert(name.trim_start_matches("visionset").to_owned(), (n.clone(), secs));
            h.out.push(Effect::VisionSet { kind: name.trim_start_matches("visionset").to_owned(), name: n, secs });
            Ok(HostReturn::None)
        }
        "setminimap" => {
            h.stores.minimap = Some(text(0));
            Ok(HostReturn::None)
        }
        "print3d" => Ok(HostReturn::None),
        "earthquake" => {
            let scale = num(a, 0, name).ok()?;
            let duration = num(a, 1, name).ok()?;
            let at = a.get(2).and_then(Value::as_vector).unwrap_or([0.0; 3]);
            let radius = num(a, 3, name).ok()?;
            // READ: scale, duration and radius must be positive for the engine to send the event.
            if scale > 0.0 && duration > 0.0 && radius > 0.0 {
                h.out.push(Effect::Earthquake { scale, duration, at, radius });
            }
            Ok(HostReturn::None)
        }
        "playsoundtoplayers" => {
            h.out.push(Effect::Sound { ent: None, at: None, alias: text(0) });
            Ok(HostReturn::None)
        }
        _ => return None,
    };
    Some(r)
}

/// A stateful flag setter by id.
pub fn flag_method(h: &mut Iw6Host, id: u16, e: u32) -> Option<R> {
    let (bit, set) = flag_op(id)?;
    let p = h.pstate(e);
    if set {
        p.flags |= bit;
    } else {
        p.flags &= !bit;
    }
    Some(Ok(HostReturn::None))
}

impl super::host::HostEnt {
    /// Whether the entity's health value (if any) is above zero; an entity without the field counts as alive.
    pub fn health_above_zero(&self, health: Option<&Value>) -> bool {
        health.and_then(Value::as_int).is_none_or(|h| h > 0)
    }
}

/// The set of precached names of a kind (for tests and the load report).
pub fn precached<'a>(h: &'a Iw6Host, kind: &str) -> BTreeSet<&'a str> {
    h.precached.get(kind).map(|s| s.iter().map(String::as_str).collect()).unwrap_or_default()
}

#[allow(dead_code)]
fn _array(v: Vec<Value>) -> Value {
    Value::Array(ArrRef::from_values(v))
}

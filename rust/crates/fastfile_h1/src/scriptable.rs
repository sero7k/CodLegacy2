//! Modern Warfare Remastered's scriptables: the map's barrels, cars, lamps, breakable props and
//! moving parts, which the engine runs from data alone (no script).
//!
//! A [`Def`] (the `ScriptableDef` asset) lists parts; each part lists states; entering a state
//! runs its events (swap the model, play or stop an effect, play a sound, an animation, an
//! explosion, a health drain, launch a physics piece, move or turn the part, change state after a
//! delay). A map places [`Instance`]s of defs (`clipMap_t::scriptableMapEnts`). What the data
//! says is READ from the zones (`h1_scriptables` census); how the engine steps it (damage to
//! health to the next state) is in `docs/H1_NATIVE.md`, "Scriptables".

use crate::walk::{AssetId, Capture};

/// One event of a state's on-enter list (`ScriptableEventDef`, 40 bytes: a type and a union).
/// Asset references are resolved to their names. The type numbers follow the union's member
/// order (READ from the layout; 0, 1, 2, 3, 4, 7, 8, 10, 11 and 12 also from the zone walker).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// 0: show another model on the instance.
    SetModel { model: Option<String> },
    /// 1: an effect at a tag, looping every `loop_time` ms when non-zero.
    PlayFx { fx: Option<String>, tag: u32, loop_time: u16, use_angles: bool },
    /// 2: stop that effect.
    StopFx { fx: Option<String>, tag: u32 },
    /// 3: a sound by alias.
    PlaySound { alias: Option<String>, looping: bool },
    /// 4: an animation.
    PlayAnim { name: Option<String>, stateful: bool, rate_min: u16, rate_max: u16 },
    /// 5: radius damage and a push (`doExplosion`).
    Explosion { force_min: u16, force_max: u16, radius: u16, damage_min: u16, damage_max: u16, ai_avoid: bool },
    /// 6: damage to anyone near, every `interval` ms (`healthDrain`).
    HealthDrain { amount: i16, interval: u16, bad_place_radius: u16 },
    /// 7: a physics piece thrown off (a model, a launch direction in 1/127 units and force scales).
    PhysicsLaunch { model: Option<String>, dir: [i8; 3], expl_force_scale: u16, bullet_force_scale: u16 },
    /// 8: a light's colour, intensity and radius (the part's `noteworthy` names the light).
    Light { noteworthy: Option<String> },
    /// 9: the sun.
    Sunlight,
    /// 10: a screen shake.
    Shake { rumble: Option<String>, duration: u16, radius: u16 },
    /// 11: move the part by a range (units, `i16`), over a time range (ms).
    Translate { min: [i16; 3], max: [i16; 3], time_min: u16, time_max: u16, flags: u16 },
    /// 12: turn the part by a range (`i16` angle units), over a time range (ms).
    Rotate { min: [i16; 3], max: [i16; 3], time_min: u16, time_max: u16, flags: u16 },
    /// 13: go to another state of the part after a delay range (ms).
    StateChange { target: u8, delay_min: u16, delay_max: u16 },
    /// Any other type number.
    Other(i32),
}

/// A state (`ScriptableStateDef`, 24 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Script string numbers (resolved with the zone's script strings by the caller).
    pub name: u32,
    pub tag: u32,
    pub damage_flags: u8,
    pub parent_transfer_rate: u8,
    pub parent_receive_rate: u8,
    /// The health this state starts with (0: the state cannot be damaged away).
    pub max_health: u32,
    pub events: Vec<Event>,
}

/// A part (`ScriptablePartDef`, 16 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    pub name: u32,
    pub flags: u8,
    pub states: Vec<State>,
}

/// The `ScriptableDef` asset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Def {
    pub name: String,
    pub base_model: Option<String>,
    pub base_collision_brush: Option<String>,
    pub destroyed_collision_brush: Option<String>,
    pub kind: i32,
    pub flags: u8,
    pub parts: Vec<Part>,
    /// Notetrack names and types (1 sound, else effect).
    pub notetracks: Vec<(u32, i32)>,
    pub event_stream_size: u8,
    pub event_constants_size: u8,
}

/// A placed scriptable (`ScriptableInstance`, 152 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instance {
    /// The def's name.
    pub def: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// Script string number.
    pub targetname: u32,
    pub pre_brush_model: u16,
    pub post_brush_model: u16,
    pub flags: u8,
    pub target_count: u8,
}

/// The def an asset id names, if the capture holds it.
pub fn def_of<'a>(defs: &'a [Def], cap: &Capture, id: Option<AssetId>) -> Option<&'a Def> {
    let id = id?;
    let name = cap.name_of(id);
    defs.iter().find(|d| d.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let d = Def::default();
        assert!(d.parts.is_empty() && d.base_model.is_none());
    }
}

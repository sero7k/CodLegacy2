//! bo2mp: script-model animation for the Ghosts map scripts.
//!
//! READ from the handlers: `scriptmodelplayanim(name)` and `useanimtree(name)` both look the
//! animation up and start it on the entity from the level time (the entity becomes the engine's
//! animated type, lasting the clip's frames over its rate); a name no animation has changes
//! nothing. `scriptmodelclearanim` (0x826f) puts the entity back to stationary. The animation is
//! played by the clients from the start time, so the pose at any moment is the clip at
//! `now - start`: looping clips wrap, the others hold their last frame.
//!
//! Not read from the handlers (INFERRED, from the animation's own data and the scripts): a
//! non-looping clip says `end` on its entity when it has run its length, and a notetrack says its
//! name when passed (the scripts of mp_frag wait for neither).
//!
//! The pose of a bone is the clip's track for it (parent local), else the bind pose; entities
//! linked to a tag follow the tag's animated pose (`Animator::tag_world`).

use std::collections::{BTreeMap, HashMap};

use iw6_pose::{Bone, Clip, Skeleton};

/// The animations and skeletons of the map zone, by lower case name.
#[derive(Clone, Debug, Default)]
pub struct AnimBank {
    pub clips: HashMap<String, Clip>,
    pub skeletons: HashMap<String, Skeleton>,
}

impl AnimBank {
    pub fn new(clips: &[Clip], skeletons: &[Skeleton]) -> AnimBank {
        AnimBank {
            clips: clips.iter().map(|c| (c.name.to_ascii_lowercase(), c.clone())).collect(),
            skeletons: skeletons.iter().map(|s| (s.name.to_ascii_lowercase(), s.clone())).collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.skeletons.is_empty()
    }
}

/// An animation running on an entity.
#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    pub clip: String,
    pub start_ms: i64,
    /// Ms into the clip when the notetracks were last looked at.
    pub seen_ms: i64,
    /// `end` has been said.
    pub ended: bool,
    /// The clip this one replaced, fading out (see [`Animator::blend_ms`]).
    pub fade: Option<Fade>,
    /// Root motion banked from the clips that played before, in the entity's own frame.
    pub base: [f32; 3],
}

/// A clip being replaced: it keeps playing from its own start while its weight runs down over
/// `len_ms` from `from_ms` (level time).
#[derive(Clone, Debug, PartialEq)]
pub struct Fade {
    pub clip: String,
    pub start_ms: i64,
    pub from_ms: i64,
    pub len_ms: i64,
}

/// The time two clips take to cross-fade when a script starts one over another. INFERRED: the
/// handlers read give the engine no blend time of their own (the animation is simply replaced on
/// the clients); 200 ms is a stand-in so that a switch does not snap. `BO2MP_IW6_ANIM_BLEND_MS`
/// sets it, 0 turns blending off (the old behaviour: the new clip is the whole pose at once).
pub const BLEND_MS_DEFAULT: i64 = 200;

/// Whether a script-touched entity of this class is drawn by the sim's model poses: the script
/// model and brush model classes always; any other class (a prop the map placed with its own
/// classname, a vehicle stand-in, a `misc` model) only when an animation plays on it and it has a
/// model. `BO2MP_IW6_ANIM_ANY_CLASS=0` keeps the old two-class filter.
pub fn publishable_class(class: &str, animating: bool, has_model: bool) -> bool {
    if matches!(class, "script_model" | "script_brushmodel") {
        return true;
    }
    static ANY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let any = *ANY.get_or_init(|| std::env::var("BO2MP_IW6_ANIM_ANY_CLASS").map(|v| v.trim() != "0").unwrap_or(true));
    any && animating && has_model
}

fn blend_ms_env() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("BO2MP_IW6_ANIM_BLEND_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(BLEND_MS_DEFAULT).max(0))
}

fn root_motion_on() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("BO2MP_IW6_ROOT_MOTION").map(|v| v.trim() != "0").unwrap_or(true))
}

#[derive(Clone, Debug, Default)]
pub struct Animator {
    pub bank: AnimBank,
    /// Host entity -> what it plays.
    pub playing: BTreeMap<u32, Playing>,
    /// Calls with a name no clip has: name -> count (for the log).
    pub unknown: BTreeMap<String, u32>,
    /// The cross-fade time (None: the environment's / the default).
    pub blend_ms: Option<i64>,
    /// Root motion on or off (None: the environment's / on).
    pub root_motion: Option<bool>,
}

impl Animator {
    /// Starts `name` on `ent`; false (nothing changes) for an animation the zones do not have.
    pub fn play(&mut self, ent: u32, name: &str, now_ms: i64) -> bool {
        let key = name.to_ascii_lowercase();
        if !self.bank.clips.contains_key(&key) {
            *self.unknown.entry(key).or_default() += 1;
            return false;
        }
        let blend = self.blend_ms.unwrap_or_else(blend_ms_env);
        let (mut fade, mut base) = (None, [0.0; 3]);
        if let Some(old) = self.playing.get(&ent) {
            // The root motion the old clip walked stays with the entity.
            if let Some(off) = self.bank.clips.get(&old.clip).and_then(|c| c.root_offset(now_ms - old.start_ms)) {
                base = [old.base[0] + off[0], old.base[1] + off[1], old.base[2] + off[2]];
            } else {
                base = old.base;
            }
            if blend > 0 && old.clip != key {
                fade = Some(Fade { clip: old.clip.clone(), start_ms: old.start_ms, from_ms: now_ms, len_ms: blend });
            }
        }
        self.playing.insert(ent, Playing { clip: key, start_ms: now_ms, seen_ms: 0, ended: false, fade, base });
        true
    }

    /// Stops the entity's animation (it keeps the bind pose).
    pub fn clear(&mut self, ent: u32) {
        self.playing.remove(&ent);
    }

    /// The notifies due at `now_ms`: (entity, name). Notetracks passed, then `end` for a clip
    /// that has run its length.
    pub fn advance(&mut self, now_ms: i64) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        for (&ent, p) in self.playing.iter_mut() {
            let Some(c) = self.bank.clips.get(&p.clip) else { continue };
            let len = c.length_ms().max(1);
            let elapsed = (now_ms - p.start_ms).max(0);
            let (from, to) = (p.seen_ms, elapsed);
            if c.looping {
                // Across a wrap: the end of the clip, then its start.
                let (a, b) = (from % len, to % len);
                let names: Vec<String> = if to / len > from / len || b < a {
                    c.notes_between(a as f32 / 1000.0, len as f32 / 1000.0).into_iter().chain(c.notes_between(0.0, b as f32 / 1000.0)).map(str::to_owned).collect()
                } else {
                    c.notes_between(a as f32 / 1000.0, b as f32 / 1000.0).into_iter().map(str::to_owned).collect()
                };
                out.extend(names.into_iter().map(|n| (ent, n)));
            } else {
                let names: Vec<String> = c.notes_between(from.min(len) as f32 / 1000.0, to.min(len) as f32 / 1000.0).into_iter().map(str::to_owned).collect();
                out.extend(names.into_iter().map(|n| (ent, n)));
                if !p.ended && c.finished(elapsed) {
                    p.ended = true;
                    out.push((ent, "end".to_owned()));
                }
            }
            p.seen_ms = to;
            if p.fade.as_ref().is_some_and(|f| now_ms >= f.from_ms + f.len_ms) {
                p.fade = None;
            }
        }
        out
    }

    /// Whether `ent` has a clip that still changes its pose at `now_ms`.
    pub fn moving(&self, ent: u32, now_ms: i64) -> bool {
        self.playing.get(&ent).and_then(|p| self.bank.clips.get(&p.clip).map(|c| (p, c))).is_some_and(|(p, c)| c.looping || !c.finished(now_ms - p.start_ms) || p.fade.as_ref().is_some_and(|f| now_ms < f.from_ms + f.len_ms))
    }

    /// The model-space pose of every bone of `model`'s skeleton as `ent` stands at `now_ms`:
    /// its clip's, else the bind pose. None for a model with no skeleton.
    pub fn pose(&self, ent: u32, model: &str, now_ms: i64) -> Option<Vec<Bone>> {
        let skel = self.bank.skeletons.get(&model.to_ascii_lowercase())?;
        let Some((c, p)) = self.playing.get(&ent).and_then(|p| self.bank.clips.get(&p.clip).map(|c| (c, p))) else {
            return Some(iw6_pose::pose(skel, None, 0.0));
        };
        let secs = c.time_secs(now_ms - p.start_ms);
        // A clip it replaced a moment ago fades out under it.
        if let Some(f) = p.fade.as_ref().filter(|f| now_ms < f.from_ms + f.len_ms) {
            if let Some(old) = self.bank.clips.get(&f.clip) {
                let w = ((now_ms - f.from_ms) as f32 / f.len_ms.max(1) as f32).clamp(0.0, 1.0);
                return Some(iw6_pose::pose_blend(skel, &[(Some(old), old.time_secs(now_ms - f.start_ms), 1.0 - w), (Some(c), secs, w)]));
            }
        }
        Some(iw6_pose::pose(skel, Some(c), secs))
    }

    /// Where the root motion has taken `ent` by `now_ms`, in the entity's own frame (forward, left,
    /// up): the clips banked, then the playing one's walk. Zero without a delta or with the switch off.
    pub fn root_local(&self, ent: u32, now_ms: i64) -> [f32; 3] {
        if !self.root_motion.unwrap_or_else(root_motion_on) {
            return [0.0; 3];
        }
        let Some(p) = self.playing.get(&ent) else { return [0.0; 3] };
        let off = self.bank.clips.get(&p.clip).and_then(|c| c.root_offset(now_ms - p.start_ms)).unwrap_or([0.0; 3]);
        [p.base[0] + off[0], p.base[1] + off[1], p.base[2] + off[2]]
    }

    /// The same shift in the world for an entity standing at `angles`.
    pub fn root_shift(&self, ent: u32, angles: [f32; 3], now_ms: i64) -> [f32; 3] {
        let l = self.root_local(ent, now_ms);
        if l == [0.0; 3] {
            return l;
        }
        let ax = iw6_pose::axes_of_angles(angles);
        std::array::from_fn(|k| ax[0][k] * l[0] + ax[1][k] * l[1] + ax[2][k] * l[2])
    }

    /// The skinning pose of every bone (posed model space from the bind pose), for drawing.
    pub fn skin(&self, ent: u32, model: &str, now_ms: i64) -> Option<Vec<Bone>> {
        let skel = self.bank.skeletons.get(&model.to_ascii_lowercase())?;
        let posed = self.pose(ent, model, now_ms)?;
        Some(iw6_pose::skin(skel, &posed))
    }

    /// The pose in the world of tag `tag` of the model of `ent`, which stands at `origin` with `angles`.
    pub fn tag_bone_world(&self, ent: u32, model: &str, origin: [f32; 3], angles: [f32; 3], tag: &str, now_ms: i64) -> Option<Bone> {
        let skel = self.bank.skeletons.get(&model.to_ascii_lowercase())?;
        let b = skel.bones.iter().position(|n| n.eq_ignore_ascii_case(tag))?;
        let posed = self.pose(ent, model, now_ms)?;
        let shift = self.root_shift(ent, angles, now_ms);
        let at = (iw6_pose::quat_of_axes(iw6_pose::axes_of_angles(angles)), [origin[0] + shift[0], origin[1] + shift[1], origin[2] + shift[2]]);
        Some(iw6_pose::compose(at, posed[b]))
    }

    /// The pose of a child standing at `child` (origin, angles) in the frame of tag `tag` of the
    /// parent now: what `linkto` without offsets stores.
    #[allow(clippy::too_many_arguments)]
    pub fn link_local(&self, ent: u32, model: &str, origin: [f32; 3], angles: [f32; 3], tag: &str, now_ms: i64, child: ([f32; 3], [f32; 3])) -> Option<Bone> {
        let bone = self.tag_bone_world(ent, model, origin, angles, tag, now_ms)?;
        let at = (iw6_pose::quat_of_axes(iw6_pose::axes_of_angles(child.1)), child.0);
        Some(iw6_pose::compose(iw6_pose::invert(bone), at))
    }

    /// Where a child with frame `local` in tag `tag` stands now (origin, angles).
    pub fn link_world(&self, ent: u32, model: &str, origin: [f32; 3], angles: [f32; 3], tag: &str, now_ms: i64, local: Bone) -> Option<([f32; 3], [f32; 3])> {
        let bone = self.tag_bone_world(ent, model, origin, angles, tag, now_ms)?;
        let w = iw6_pose::compose(bone, local);
        Some((w.1, iw6_pose::angles_of_axes(iw6_pose::axes(w.0))))
    }

    /// World origin and angles of tag `tag` of the model of `ent`, which stands at `origin` with
    /// `angles` (None: the model has no such bone).
    pub fn tag_world(&self, ent: u32, model: &str, origin: [f32; 3], angles: [f32; 3], tag: &str, now_ms: i64) -> Option<([f32; 3], [f32; 3])> {
        let skel = self.bank.skeletons.get(&model.to_ascii_lowercase())?;
        let b = skel.bones.iter().position(|n| n.eq_ignore_ascii_case(tag))?;
        let posed = self.pose(ent, model, now_ms)?;
        let shift = self.root_shift(ent, angles, now_ms);
        Some(iw6_pose::place([origin[0] + shift[0], origin[1] + shift[1], origin[2] + shift[2]], angles, posed[b]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iw6_pose::Skeleton;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    fn bank() -> AnimBank {
        let skel = Skeleton {
            name: "prop".into(),
            bones: vec!["root".into(), "j_a".into()],
            parents: vec![None, Some(0)],
            local: vec![(iw6_pose::IDENTITY, [0.0; 3]), (iw6_pose::IDENTITY, [0.0; 3])],
            world: vec![(iw6_pose::IDENTITY, [0.0; 3]), (iw6_pose::IDENTITY, [0.0; 3])],
        };
        // Two one-second clips: `a` holds j_a at y=0, `b` at y=10; `b` also walks 100 forward.
        let mk = |name: &str, y: f32, walk: f32, looping: bool| iw6_pose::Clip {
            name: name.into(),
            frames: 2,
            rate: 2.0,
            looping,
            bones: vec!["j_a".into()],
            rot: vec![vec![]],
            trans: vec![vec![[0.0, y, 0.0]; 3]],
            delta: if walk != 0.0 { vec![[0.0; 3], [walk / 2.0, 0.0, 0.0], [walk, 0.0, 0.0]] } else { Vec::new() },
            ..Default::default()
        };
        AnimBank::new(&[mk("a", 0.0, 0.0, true), mk("b", 10.0, 100.0, false)], &[skel])
    }

    fn animator(blend: i64) -> Animator {
        Animator { bank: bank(), blend_ms: Some(blend), root_motion: Some(true), ..Default::default() }
    }

    #[test]
    fn a_clip_started_over_another_fades_in_over_the_blend_time() {
        let mut an = animator(200);
        assert!(an.play(1, "a", 0));
        assert!(an.play(1, "b", 1000));
        let y = |an: &Animator, t: i64| an.pose(1, "prop", t).unwrap()[1].1[1];
        assert!((y(&an, 1000) - 0.0).abs() < 1e-3, "starts on the old pose");
        assert!((y(&an, 1100) - 5.0).abs() < 1e-3, "half way");
        assert!((y(&an, 1200) - 10.0).abs() < 1e-3);
        assert!((y(&an, 1900) - 10.0).abs() < 1e-3);
        // Still fading at 1100, done after `advance` past the end.
        assert!(an.moving(1, 1100));
        an.advance(1300);
        assert!(an.playing[&1].fade.is_none());
    }

    #[test]
    fn no_blend_time_is_the_old_cut() {
        let mut an = animator(0);
        an.play(1, "a", 0);
        an.play(1, "b", 1000);
        assert!((an.pose(1, "prop", 1000).unwrap()[1].1[1] - 10.0).abs() < 1e-3);
        // The same clip again restarts without a fade.
        let mut an = animator(200);
        an.play(1, "b", 0);
        an.play(1, "b", 500);
        assert!(an.playing[&1].fade.is_none());
    }

    #[test]
    fn root_motion_moves_the_model_and_stays_across_a_switch() {
        let mut an = animator(0);
        an.play(1, "b", 0);
        assert!(close(an.root_local(1, 500), [50.0, 0.0, 0.0]));
        assert!(close(an.root_local(1, 5000), [100.0, 0.0, 0.0]));
        // Yawed 90 degrees, forward is +y in the world.
        assert!(close(an.root_shift(1, [0.0, 90.0, 0.0], 5000), [0.0, 100.0, 0.0]));
        // A clip without a delta started after it keeps what was walked.
        an.play(1, "a", 6000);
        assert!(close(an.root_local(1, 7000), [100.0, 0.0, 0.0]));
        // A linked child follows the shifted bone.
        let w = an.tag_world(1, "prop", [1000.0, 0.0, 0.0], [0.0; 3], "j_a", 7000).unwrap();
        assert!(close(w.0, [1100.0, 0.0, 0.0]));
        // Switched off, nothing moves.
        an.root_motion = Some(false);
        assert_eq!(an.root_local(1, 7000), [0.0; 3]);
    }

    #[test]
    fn an_animated_prop_of_any_class_is_published_a_plain_one_is_not() {
        use super::publishable_class as ok;
        assert!(ok("script_model", false, true) && ok("script_brushmodel", false, false));
        assert!(ok("misc_prop", true, true));
        assert!(!ok("misc_prop", false, true), "nothing plays on it");
        assert!(!ok("misc_prop", true, false), "no model to draw");
    }
}

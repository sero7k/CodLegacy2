//! bo2mp: re-timing a decoded clip so that its "hit" moment lands on a
//! given time. A pure function of the clip: no clock, no catalog.
//!
//! The first-person quick melee of Ghosts is played for every gun, but the
//! gun's gameplay timings (when the blow lands, how long the swing lasts)
//! stay the gun's own. So the clip is resampled on a time axis that is two
//! linear pieces: the source up to its hit moment runs over the time from
//! the press to the gun's hit time, the rest over the time that is left.

use crate::xanim_clip::{AnimClip, ClipNotify, FrameIndices, Keyed, Rotation, Track, Translation};

/// The frame rate of a re-timed clip (it is resampled, not copied).
const RETIME_FRAME_RATE: f32 = 60.0;

/// The part of the clip a hit may fall in, as a fraction of its length
/// (a hit at the very start or very end would leave one piece empty).
const HIT_MARGIN: f32 = 0.05;

/// The bones that swing a right-handed blow: their angular speed finds the
/// moment of a clip that has no contact note.
const SWING_BONES: [&str; 4] = [
    "j_shoulder_ri",
    "j_elbow_ri",
    "j_wrist_ri",
    "j_wristtwist_ri",
];

/// A time axis of two linear pieces: `dst` seconds from the start of the
/// new clip -> `src` seconds in the old one. The hit of the old clip
/// (`src_hit`) is at `dst_hit` in the new.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitWarp {
    pub src_duration: f32,
    pub src_hit: f32,
    pub dst_duration: f32,
    pub dst_hit: f32,
}

impl HitWarp {
    /// `dst_hit` <= 0 (a gun with no hit delay) keeps the clip's own
    /// proportions: its hit lands at the same fraction of the new length.
    /// A hit outside the clip is pulled in by [`HIT_MARGIN`].
    pub fn new(src_duration: f32, src_hit: f32, dst_duration: f32, dst_hit: f32) -> Self {
        let src_hit = src_hit.clamp(src_duration * HIT_MARGIN, src_duration * (1.0 - HIT_MARGIN));
        let dst_hit = if dst_hit > 0.0 && dst_hit.is_finite() {
            dst_hit
        } else if src_duration > 0.0 {
            dst_duration * src_hit / src_duration
        } else {
            0.0
        };
        let dst_hit = dst_hit.clamp(dst_duration * HIT_MARGIN, dst_duration * (1.0 - HIT_MARGIN));
        Self {
            src_duration,
            src_hit,
            dst_duration,
            dst_hit,
        }
    }

    /// The old clip's time for a time in the new one.
    pub fn src_time(&self, dst: f32) -> f32 {
        let dst = dst.clamp(0.0, self.dst_duration);
        if dst <= self.dst_hit {
            if self.dst_hit <= 0.0 {
                0.0
            } else {
                dst / self.dst_hit * self.src_hit
            }
        } else {
            let tail = self.dst_duration - self.dst_hit;
            if tail <= 0.0 {
                self.src_duration
            } else {
                self.src_hit + (dst - self.dst_hit) / tail * (self.src_duration - self.src_hit)
            }
        }
    }

    /// A note's place in the new clip: its fraction of the old clip's length
    /// (as clips store notes) -> its fraction of the new one's.
    pub fn retime_note(&self, fraction: f32) -> f32 {
        if self.dst_duration <= 0.0 || self.src_duration <= 0.0 {
            return fraction;
        }
        (self.dst_time(fraction.clamp(0.0, 1.0) * self.src_duration) / self.dst_duration)
            .clamp(0.0, 1.0)
    }

    /// The new clip's time for a time in the old one (the inverse).
    pub fn dst_time(&self, src: f32) -> f32 {
        let src = src.clamp(0.0, self.src_duration);
        if src <= self.src_hit {
            if self.src_hit <= 0.0 {
                0.0
            } else {
                src / self.src_hit * self.dst_hit
            }
        } else {
            let tail = self.src_duration - self.src_hit;
            if tail <= 0.0 {
                self.dst_duration
            } else {
                self.dst_hit + (src - self.src_hit) / tail * (self.dst_duration - self.dst_hit)
            }
        }
    }
}

impl AnimClip {
    /// The moment (seconds) the clip's blow lands: a `contact` note when it
    /// has one, else the instant the right arm turns fastest, else the
    /// middle.
    pub fn hit_time(&self) -> f32 {
        let duration = self.duration();
        if let Some(note) = self
            .notifies
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case("contact"))
        {
            return note.time.clamp(0.0, 1.0) * duration;
        }
        let tracks: Vec<usize> = SWING_BONES
            .iter()
            .filter_map(|bone| {
                self.tracks
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(bone))
            })
            .collect();
        let frames = usize::from(self.numframes);
        if tracks.is_empty() || frames < 2 || self.framerate <= 0.0 {
            return duration * 0.5;
        }
        let mut best = (0.0f32, 0usize);
        for f in 0..frames {
            let mut speed = 0.0f32;
            for &track in &tracks {
                let q = |frame: usize| {
                    self.sample_track(track, frame as f32 / self.framerate)
                        .and_then(|s| s.rotation)
                };
                if let (Some(a), Some(b)) = (q(f), q(f + 1)) {
                    speed += anim_iw4::quat_dot(a, b).abs().clamp(0.0, 1.0).acos();
                }
            }
            if speed > best.0 {
                best = (speed, f);
            }
        }
        (best.1 as f32 + 0.5) / self.framerate
    }

    /// The clip resampled on `warp`'s time axis, `warp.dst_duration` long.
    /// Same tracks in the same order (so a table binding the old clip's
    /// tracks to bones binds this one's), the old clip's notes carried over
    /// on the same warp (a note keeps the frame of the swing it was on).
    pub fn retimed(&self, warp: &HitWarp) -> AnimClip {
        let frames = ((warp.dst_duration * RETIME_FRAME_RATE).round() as usize).max(2);
        let framerate = frames as f32 / warp.dst_duration.max(f32::EPSILON);
        let src_times: Vec<f32> = (0..=frames)
            .map(|f| warp.src_time(f as f32 / framerate))
            .collect();
        let tracks = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, track)| {
                let rotation = match &track.rotation {
                    Rotation::HalfKeyed(_) | Rotation::FullKeyed(_) => Rotation::FullKeyed(Keyed {
                        values: src_times
                            .iter()
                            .filter_map(|&t| self.sample_track(i, t).and_then(|s| s.rotation))
                            .collect(),
                        frames: FrameIndices::Dense,
                    }),
                    other => other.clone(),
                };
                let translation = match &track.translation {
                    Translation::SmallKeyed(_) | Translation::FullKeyed(_) => {
                        Translation::FullKeyed(Keyed {
                            values: src_times
                                .iter()
                                .filter_map(|&t| {
                                    self.sample_track(i, t).and_then(|s| s.translation)
                                })
                                .collect(),
                            frames: FrameIndices::Dense,
                        })
                    }
                    other => other.clone(),
                };
                Track {
                    name: track.name.clone(),
                    rotation,
                    translation,
                }
            })
            .collect();
        AnimClip {
            name: self.name.clone(),
            framerate,
            numframes: frames as u16,
            looping: false,
            tracks,
            notifies: self
                .notifies
                .iter()
                .map(|note| ClipNotify {
                    name: note.name.clone(),
                    time: warp.retime_note(note.time),
                })
                .collect(),
            delta_translation: Translation::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xanim_clip::ClipNotify;

    #[test]
    fn warp_puts_the_hit_where_asked_and_inverts() {
        let w = HitWarp::new(0.8, 0.2, 1.0, 0.5);
        assert!((w.dst_time(0.2) - 0.5).abs() < 1e-6);
        assert!((w.src_time(0.5) - 0.2).abs() < 1e-6);
        assert!((w.src_time(0.0)).abs() < 1e-6);
        assert!((w.src_time(1.0) - 0.8).abs() < 1e-6);
        for s in [0.0, 0.1, 0.2, 0.5, 0.8] {
            assert!((w.src_time(w.dst_time(s)) - s).abs() < 1e-5);
        }
        // Monotonic: the clip never runs backwards.
        let mut last = -1.0;
        for i in 0..=100 {
            let t = w.src_time(i as f32 / 100.0);
            assert!(t >= last);
            last = t;
        }
    }

    #[test]
    fn no_delay_keeps_proportions_and_wild_hits_are_pulled_in() {
        let w = HitWarp::new(0.8, 0.4, 0.4, 0.0);
        assert!((w.dst_hit - 0.2).abs() < 1e-6);
        let w = HitWarp::new(1.0, 0.5, 1.0, 5.0);
        assert!((w.dst_hit - 0.95).abs() < 1e-6);
        let w = HitWarp::new(1.0, 2.0, 1.0, 0.5);
        assert!((w.src_hit - 0.95).abs() < 1e-6);
    }

    fn keyed_rot(angles: &[f32]) -> Rotation {
        Rotation::FullKeyed(Keyed {
            values: angles
                .iter()
                .map(|a| {
                    let h = a * 0.5;
                    [h.sin(), 0.0, 0.0, h.cos()]
                })
                .collect(),
            frames: FrameIndices::Dense,
        })
    }

    /// 30 frames at 30 fps (1 s); the right wrist turns fast around frame 12.
    fn swing_clip() -> AnimClip {
        let angles: Vec<f32> = (0..=30)
            .map(|f| {
                let x = (f as f32 - 12.0) / 2.0;
                1.5 / (1.0 + (-x).exp())
            })
            .collect();
        AnimClip {
            name: "swing".into(),
            framerate: 30.0,
            numframes: 30,
            looping: false,
            tracks: vec![
                Track {
                    name: "j_wrist_ri".into(),
                    rotation: keyed_rot(&angles),
                    translation: Translation::Default,
                },
                Track {
                    name: "j_wrist_le".into(),
                    rotation: Rotation::FullConstant([0.0, 0.0, 0.0, 1.0]),
                    translation: Translation::Constant([1.0, 2.0, 3.0]),
                },
            ],
            notifies: vec![ClipNotify {
                name: "end".into(),
                time: 1.0,
            }],
            delta_translation: Translation::Default,
        }
    }

    #[test]
    fn hit_is_the_fastest_turn_or_the_contact_note() {
        let mut clip = swing_clip();
        let t = clip.hit_time();
        assert!((t - 12.0 / 30.0).abs() < 0.06, "{t}");
        clip.notifies.push(ClipNotify {
            name: "Contact".into(),
            time: 0.25,
        });
        assert!((clip.hit_time() - 0.25).abs() < 1e-6);
        clip.notifies.clear();
        clip.tracks.remove(0);
        assert!((clip.hit_time() - 0.5).abs() < 1e-6);
    }

    /// A note rides the same warp as the poses: a note on the clip's blow
    /// lands on the new blow time, notes keep their order, the first and
    /// last stay at the ends.
    #[test]
    fn notes_ride_the_warp() {
        let mut clip = swing_clip();
        clip.notifies = ["start", "contact", "after", "end"]
            .into_iter()
            .zip([0.0, 0.4, 0.7, 1.0])
            .map(|(name, time)| ClipNotify { name: name.into(), time })
            .collect();
        let warp = HitWarp::new(clip.duration(), clip.hit_time(), 0.5, 0.35);
        assert!((clip.hit_time() - 0.4).abs() < 1e-6);
        let out = clip.retimed(&warp);
        let at = |n: &str| out.notifies.iter().find(|x| x.name == n).unwrap().time;
        assert!(at("start").abs() < 1e-6);
        // contact: 0.35 s of the 0.5 s clip.
        assert!((at("contact") - 0.7).abs() < 1e-4, "{}", at("contact"));
        assert!(at("after") > at("contact") && at("after") < 1.0);
        assert!((at("end") - 1.0).abs() < 1e-6);
        // Crossing the note while playing the new clip fires it once, at
        // the new time.
        let fired = out.crossed_notifies(0.30, 0.36);
        assert_eq!(fired, vec!["contact".to_string()]);
        assert!(out.crossed_notifies(0.36, 0.40).is_empty());
    }

    #[test]
    fn retimed_clip_has_the_new_length_the_same_tracks_and_the_hit_on_time() {
        let clip = swing_clip();
        let hit = clip.hit_time();
        let warp = HitWarp::new(clip.duration(), hit, 0.6, 0.3);
        let out = clip.retimed(&warp);
        assert!((out.duration() - 0.6).abs() < 1e-4, "{}", out.duration());
        assert_eq!(out.tracks.len(), 2);
        assert_eq!(out.tracks[1].name, "j_wrist_le");
        assert!(matches!(out.tracks[1].rotation, Rotation::FullConstant(_)));
        assert!(!out.looping);
        // The `end` note stays at the end.
        assert_eq!(out.notifies.len(), 1);
        assert!((out.notifies[0].time - 1.0).abs() < 1e-6);
        // The new clip's fastest turn is at the asked time.
        let new_hit = out.hit_time();
        assert!((new_hit - 0.3).abs() < 0.05, "{new_hit}");
        // Ends match the source's ends.
        let a = clip.sample_track(0, 0.0).unwrap().rotation.unwrap();
        let b = out.sample_track(0, 0.0).unwrap().rotation.unwrap();
        assert!(anim_iw4::quat_dot(a, b).abs() > 0.9999);
        let a = clip
            .sample_track(0, clip.duration())
            .unwrap()
            .rotation
            .unwrap();
        let b = out
            .sample_track(0, out.duration())
            .unwrap()
            .rotation
            .unwrap();
        assert!(anim_iw4::quat_dot(a, b).abs() > 0.9999);
    }
}

//! bo2mp: what the bake is building right now, for the loading screen.
//!
//! A load that writes a baked file says so here ([`begin`]: game, map, step
//! and how many items); the loading screen reads [`current`] each frame and
//! shows "Black Ops 3 - Skyjacked - pictures 412/2190". Cancelling the load
//! sets [`Job::cancel`]; the writer then stops adding to its half-written
//! file and throws it away (the next load bakes again). The load itself goes
//! on the old way.
//!
//! This crate knows nothing of the loading screen (no engine types): one job
//! at a time, plain atomics.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The job being built, if any (a newer one replaces it).
static CURRENT: Mutex<Option<Arc<Job>>> = Mutex::new(None);

/// One baked file being written.
#[derive(Debug)]
pub struct Job {
    /// "Black Ops 3 - Skyjacked - pictures".
    label: String,
    total: AtomicU64,
    done: AtomicU64,
    canceled: AtomicBool,
    ended: AtomicBool,
    kept: AtomicBool,
    /// Why it is made again ("game files changed"; empty: not said).
    why: Mutex<String>,
}

impl Job {
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Why this copy is made again, for the row: "first load", "game
    /// updated" (see [`crate::why`]).
    pub fn set_why(&self, why: &str) {
        if let Ok(mut w) = self.why.lock() {
            *w = why.to_owned();
        }
    }

    /// What [`Job::set_why`] said (empty: nothing).
    pub fn why(&self) -> String {
        self.why.lock().map(|w| w.clone()).unwrap_or_else(|_| String::new())
    }

    /// Items written so far and in all.
    pub fn counts(&self) -> (u64, u64) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// One more item done.
    pub fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    /// The load was cancelled: what was written is thrown away.
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Relaxed)
    }

    /// The writer is done with it; `kept`: the file is the map's now.
    pub fn end(&self, kept: bool) {
        self.kept.store(kept, Ordering::Relaxed);
        self.ended.store(true, Ordering::Relaxed);
    }

    /// `None` while it runs, else whether the file was kept.
    pub fn outcome(&self) -> Option<bool> {
        self.ended.load(Ordering::Relaxed).then(|| self.kept.load(Ordering::Relaxed))
    }
}

/// A baked file for `game`'s `map` starts: `step` names what it holds
/// ("pictures"), `total` how many (0: not known yet; the row then shows
/// "pictures 412"). Replaces any older job.
pub fn begin(game: &str, map: &str, step: &str, total: u64) -> Arc<Job> {
    let job = Arc::new(Job {
        label: format!("{} - {} - {step}", game_title(game), map_title(map)),
        total: AtomicU64::new(total),
        done: AtomicU64::new(0),
        canceled: AtomicBool::new(false),
        ended: AtomicBool::new(false),
        kept: AtomicBool::new(false),
        why: Mutex::new(String::new()),
    });
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some(job.clone());
    }
    job
}

/// The newest job (it may have ended: see [`Job::outcome`]).
pub fn current() -> Option<Arc<Job>> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

/// A game's name as the menus say it, from its folder code.
pub fn game_title(game: &str) -> String {
    match game {
        "t5" => "Black Ops".into(),
        "t6" => "Black Ops 2".into(),
        "t7" => "Black Ops 3".into(),
        "iw4" => "Modern Warfare 2".into(),
        "iw5" => "Modern Warfare 3".into(),
        "iw6" => "Ghosts".into(),
        "h1" => "Modern Warfare Remastered".into(),
        "iw3" => "Call of Duty 4".into(),
        "t4" => "World at War".into(),
        other => other.into(),
    }
}

/// `mp_skyjacked` -> "Skyjacked", `mp_nuketown_2020` -> "Nuketown 2020".
pub fn map_title(map: &str) -> String {
    let bare = map.strip_prefix("mp_").unwrap_or(map);
    bare.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_game_map_and_step() {
        let job = begin("t7", "mp_skyjacked", "pictures", 2190);
        job.add(412);
        assert_eq!(job.label(), "Black Ops 3 - Skyjacked - pictures");
        assert_eq!(job.counts(), (412, 2190));
        assert_eq!(job.outcome(), None);
        job.cancel();
        job.end(false);
        assert!(job.is_canceled());
        assert_eq!(job.outcome(), Some(false));
        assert_eq!(map_title("mp_nuketown_2020"), "Nuketown 2020");
        assert_eq!(map_title("zm_x"), "Zm X");
    }
}

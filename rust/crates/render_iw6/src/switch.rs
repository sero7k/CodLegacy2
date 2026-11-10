//! Environment switches read once, not every frame.
//!
//! `std::env::var` takes the process's environment lock and allocates a
//! `String`; a switch read in the frame's path (`bloom_on`, `dof::on`, ...)
//! paid that every frame. A `Switch` / `Number` keeps the value it read and
//! reads again only after `refresh()` (the offline `iw6_shot` variants set
//! and restore variables between pictures and call it; nothing else changes
//! the environment while a map runs).
//!
//! Same meaning as the reads they replace: a default-on switch is off when
//! the variable is `0`; a default-off one is on when it is `1` (both after
//! trimming).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

static GENERATION: AtomicU32 = AtomicU32::new(1);

/// The environment changed: every switch reads its variable again at its
/// next use.
pub fn refresh() {
    GENERATION.fetch_add(1, Relaxed);
}

/// The refresh count (a cache of anything derived from the environment keys on it).
pub fn generation() -> u32 {
    GENERATION.load(Relaxed)
}

/// One on/off variable with a default.
pub struct Switch {
    name: &'static str,
    default_on: bool,
    seen: AtomicU32,
    value: AtomicBool,
}

impl Switch {
    /// On unless `name` is `0`.
    pub const fn on(name: &'static str) -> Self {
        Self { name, default_on: true, seen: AtomicU32::new(0), value: AtomicBool::new(true) }
    }

    /// Off unless `name` is `1`.
    pub const fn off(name: &'static str) -> Self {
        Self { name, default_on: false, seen: AtomicU32::new(0), value: AtomicBool::new(false) }
    }

    pub fn get(&self) -> bool {
        let g = GENERATION.load(Relaxed);
        if self.seen.load(Relaxed) == g {
            return self.value.load(Relaxed);
        }
        let v = match std::env::var(self.name) {
            Ok(s) => {
                let s = s.trim();
                if self.default_on { s != "0" } else { s == "1" }
            }
            Err(_) => self.default_on,
        };
        self.value.store(v, Relaxed);
        self.seen.store(g, Relaxed);
        v
    }
}

/// One number variable with a default (NaN-free: a value that does not
/// parse is the default).
pub struct Number {
    name: &'static str,
    default: f32,
    seen: AtomicU32,
    bits: AtomicU32,
}

impl Number {
    pub const fn new(name: &'static str, default: f32) -> Self {
        Self { name, default, seen: AtomicU32::new(0), bits: AtomicU32::new(0) }
    }

    pub fn get(&self) -> f32 {
        let g = GENERATION.load(Relaxed);
        if self.seen.load(Relaxed) == g {
            return f32::from_bits(self.bits.load(Relaxed));
        }
        let v = std::env::var(self.name).ok().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(self.default);
        self.bits.store(v.to_bits(), Relaxed);
        self.seen.store(g, Relaxed);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_follows_its_variable_after_a_refresh() {
        static S: Switch = Switch::on("RENDER_IW6_TEST_SWITCH_ON");
        static T: Switch = Switch::off("RENDER_IW6_TEST_SWITCH_OFF");
        static N: Number = Number::new("RENDER_IW6_TEST_NUMBER", 2.5);
        assert!(S.get() && !T.get());
        assert_eq!(N.get(), 2.5);
        // SAFETY: tests of this module are the only readers of these names.
        unsafe {
            std::env::set_var("RENDER_IW6_TEST_SWITCH_ON", "0");
            std::env::set_var("RENDER_IW6_TEST_SWITCH_OFF", " 1 ");
            std::env::set_var("RENDER_IW6_TEST_NUMBER", "0.25");
        }
        // (Held until the refresh.)
        assert!(S.get() && !T.get());
        refresh();
        assert!(!S.get() && T.get());
        assert_eq!(N.get(), 0.25);
    }
}

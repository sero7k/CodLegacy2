//! A small in-memory sliding-window rate limiter.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct Limiter {
    hits: Mutex<HashMap<String, Vec<i64>>>,
}

/// The longest window any caller uses; entries older than this are dropped.
const LONGEST_WINDOW: i64 = 3600;

impl Limiter {
    fn with<T>(&self, f: impl FnOnce(&mut HashMap<String, Vec<i64>>) -> T) -> T {
        let mut g = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }

    /// Count this attempt and say whether it is within `max` per `window` seconds.
    pub fn allow(&self, key: &str, max: usize, window: i64, now: i64) -> bool {
        self.with(|m| {
            if m.len() > 50_000 {
                m.retain(|_, v| v.last().is_some_and(|t| now - t < LONGEST_WINDOW));
            }
            let v = m.entry(key.to_string()).or_default();
            v.retain(|t| now - t < window);
            if v.len() >= max {
                return false;
            }
            v.push(now);
            true
        })
    }

    /// Whether `key` already has `max` or more recorded events in `window`.
    pub fn blocked(&self, key: &str, max: usize, window: i64, now: i64) -> bool {
        self.with(|m| m.get(key).is_some_and(|v| v.iter().filter(|t| now - **t < window).count() >= max))
    }

    pub fn record(&self, key: &str, now: i64) {
        self.with(|m| m.entry(key.to_string()).or_default().push(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_slides() {
        let l = Limiter::default();
        for i in 0..3 {
            assert!(l.allow("k", 3, 60, 100 + i));
        }
        assert!(!l.allow("k", 3, 60, 110));
        assert!(l.allow("k", 3, 60, 170));
        assert!(l.allow("other", 3, 60, 110));
    }

    #[test]
    fn failures_block_until_they_age_out() {
        let l = Limiter::default();
        for t in 0..5 {
            l.record("u", t);
        }
        assert!(l.blocked("u", 5, 300, 10));
        assert!(!l.blocked("u", 5, 300, 400));
    }
}

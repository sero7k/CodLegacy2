//! Counts of what the scripts did (optional; `Vm::enable_census`).

use std::collections::{BTreeMap, HashMap};

use crate::scripts::ScriptSet;
use crate::value::FuncRef;

#[derive(Clone, Debug, Default)]
pub struct Census {
    pub instructions: u64,
    pub threads_started: u64,
    pub errors: u64,
    /// Calls per (script index, function index), threads included.
    pub calls: HashMap<(u32, u32), u64>,
    pub builtin_functions: BTreeMap<u16, u64>,
    pub builtin_methods: BTreeMap<u16, u64>,
    pub notifies: BTreeMap<String, u64>,
    pub waittills: BTreeMap<String, u64>,
    pub endons: BTreeMap<String, u64>,
    pub waits: u64,
    pub strings: BTreeMap<String, u64>,
    pub entity_field_reads: BTreeMap<String, u64>,
    pub entity_field_writes: BTreeMap<String, u64>,
    pub level_field_reads: BTreeMap<String, u64>,
    pub level_field_writes: BTreeMap<String, u64>,
}

fn top<K: Clone + Ord, V: Copy + Ord>(m: &BTreeMap<K, V>) -> Vec<(K, V)> {
    let mut v: Vec<(K, V)> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

impl Census {
    /// A text report.
    pub fn report(&self, scripts: &ScriptSet) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(s, "instructions: {}", self.instructions);
        let _ = writeln!(s, "threads started: {}", self.threads_started);
        let _ = writeln!(s, "waits: {}", self.waits);
        let _ = writeln!(s, "script errors: {}", self.errors);
        let _ = writeln!(s, "\n== builtin functions (id: calls) ==");
        for (k, v) in top(&self.builtin_functions) {
            let _ = writeln!(s, "function {k:#06x} ({k}): {v}");
        }
        let _ = writeln!(s, "\n== builtin methods (id: calls) ==");
        for (k, v) in top(&self.builtin_methods) {
            let _ = writeln!(s, "method {k:#06x} ({}): {v}", k.wrapping_sub(0x8000));
        }
        for (title, m) in [
            ("notifies sent", &self.notifies),
            ("waittill names", &self.waittills),
            ("endon names", &self.endons),
            ("entity fields read", &self.entity_field_reads),
            ("entity fields written", &self.entity_field_writes),
            ("level fields read", &self.level_field_reads),
            ("level fields written", &self.level_field_writes),
        ] {
            let _ = writeln!(s, "\n== {title} ==");
            for (k, v) in top(m) {
                let _ = writeln!(s, "{k}: {v}");
            }
        }
        let _ = writeln!(s, "\n== strings used (get_string executed) ==");
        for (k, v) in top(&self.strings) {
            let _ = writeln!(s, "{k:?}: {v}");
        }
        let _ = writeln!(s, "\n== functions called (top 200) ==");
        let mut calls: Vec<_> = self.calls.iter().collect();
        calls.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for ((sc, f), n) in calls.into_iter().take(200) {
            let _ = writeln!(s, "{}: {n}", scripts.describe(FuncRef { script: *sc, func: *f }));
        }
        s
    }
}

//! The whole program's memory as Windows (or Linux) counts it: what is in RAM
//! now (working set), its highest so far, and what the program has committed
//! (private bytes, which the page file backs whether or not it is in RAM).

/// One reading of the program's memory, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessMemory {
    /// In RAM now (Task Manager's "Working set").
    pub working_set: u64,
    /// The highest the working set has been since the program started.
    pub peak_working_set: u64,
    /// Committed by the program (Task Manager's "Commit size").
    pub private: u64,
}

/// The program's memory now, where it can be read.
pub fn process_memory() -> Option<ProcessMemory> {
    imp::process_memory()
}

/// One `memory:` log line for a stage of the game (menu, loading, in match,
/// after map change), with the live heap when the counting allocator is on.
pub fn memory_line(stage: &str) -> String {
    let mut line = format!("memory: {stage}");
    if let Some(mem) = process_memory() {
        line.push_str(&format!(
            " working_mib={} peak_mib={} private_mib={}",
            mem.working_set >> 20,
            mem.peak_working_set >> 20,
            mem.private >> 20
        ));
    } else {
        line.push_str(" (not readable here)");
    }
    if let Some(heap) = crate::process_live_heap_bytes() {
        line.push_str(&format!(" heap_mib={}", heap >> 20));
    }
    line
}

#[cfg(windows)]
mod imp {
    use super::ProcessMemory;

    /// `PROCESS_MEMORY_COUNTERS_EX`.
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }

    pub(super) fn process_memory() -> Option<ProcessMemory> {
        let mut counters = Counters {
            cb: size_of::<Counters>() as u32,
            ..Counters::default()
        };
        // SAFETY: the current process's pseudo handle, and a counters block
        // whose size is passed with it.
        let ok = unsafe {
            K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb)
        };
        (ok != 0).then(|| ProcessMemory {
            working_set: counters.working_set_size as u64,
            peak_working_set: counters.peak_working_set_size as u64,
            private: counters.private_usage as u64,
        })
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ProcessMemory;

    pub(super) fn process_memory() -> Option<ProcessMemory> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kib = |key: &str| {
            status.lines().find_map(|line| {
                line.strip_prefix(key)?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        };
        Some(ProcessMemory {
            working_set: kib("VmRSS:")? << 10,
            peak_working_set: kib("VmHWM:")? << 10,
            private: kib("RssAnon:").map_or(0, |n| n << 10) + kib("VmSwap:").map_or(0, |n| n << 10),
        })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub(super) fn process_memory() -> Option<super::ProcessMemory> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_this_programs_memory() {
        let Some(mem) = super::process_memory() else {
            return;
        };
        // A running test holds at least a few hundred KiB, and the peak is
        // never under the reading taken with it.
        assert!(mem.working_set > 256 << 10, "{mem:?}");
        assert!(mem.peak_working_set >= mem.working_set, "{mem:?}");
        assert!(super::memory_line("test").starts_with("memory: test working_mib="));
    }
}

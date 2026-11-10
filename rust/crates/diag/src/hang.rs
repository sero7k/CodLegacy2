//! Names the call a stall is stuck in. The frame reporter knows a frame came
//! late and which part of it ran long ("Ui 73068 ms"), not which system or
//! call: when no frame has come for a while, a watcher thread pauses each of
//! the program's threads in turn, walks its stack, and logs the stacks that
//! are in this program's own code (a system reading a file, waiting on a
//! lock, ...). Frames carry on as soon as the thread is resumed; a thread is
//! paused for microseconds.
//!
//! Off by default; `CODLEGACY_HANG_SAMPLER=1` turns it on.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// When the last frame came (ms since `start`).
static LAST_BEAT: AtomicU64 = AtomicU64::new(0);

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    start().elapsed().as_millis() as u64
}

/// A frame came. Called once a frame; the first call starts the watcher.
pub fn beat() {
    LAST_BEAT.store(now_ms(), Ordering::Relaxed);
    static WATCHER: OnceLock<()> = OnceLock::new();
    WATCHER.get_or_init(|| {
        // Off unless asked for: pausing every thread to walk its stack is
        // a test tool, not something a player's game should risk.
        if codlegacy_env::var_os("CODLEGACY_HANG_SAMPLER").is_none() || !imp::AVAILABLE {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("hang sampler".into())
            .spawn(watch);
        if let Err(e) = spawned {
            crate::info!(Launch, "hang sampler: no thread: {e}");
        }
    });
}

/// How long without a frame before each sample of one stall (ms); after the
/// last, one every `REPEAT_MS`.
const SAMPLE_AT_MS: [u64; 3] = [400, 1500, 5000];
const REPEAT_MS: u64 = 10_000;

/// How many samples may wait for their names; more are dropped.
const SAMPLES_QUEUED: usize = 32;

/// A stall's first samples are also logged unnamed (image+offset), at once,
/// with this many frames per thread.
const RAW_SAMPLES: usize = 2;
const RAW_FRAMES: usize = 8;

/// One walk of every thread during a stall, waiting for its names.
struct Sample {
    /// Which stall (its last frame's beat).
    stall: u64,
    /// The stall's sample count, this one included.
    taken: usize,
    /// No frame for this long (ms) when it was taken.
    late: u64,
    /// When it was taken (s since the program started).
    at: f64,
    walked_ms: f64,
    modules: imp::Modules,
    stacks: Vec<Stack>,
}

fn watch() {
    // Naming reads the program's .pdb: 195 s cold on his hard drive. Done
    // here, it held the next samples back, and the cold menu's 187 s stall
    // got one sample at 0.4 s. The samples queue for a naming thread.
    let (to_namer, samples) = std::sync::mpsc::sync_channel::<Sample>(SAMPLES_QUEUED);
    let spawned = std::thread::Builder::new()
        .name("hang sampler names".into())
        .spawn(move || name_samples(samples));
    if let Err(e) = spawned {
        crate::info!(Launch, "hang sampler: no naming thread: {e}");
        return;
    }
    let mut stall_from = 0u64;
    let mut taken = 0usize;
    let mut dropped = 0usize;
    // Each thread's name, asked once per stall. (The loaded images are
    // listed again for every sample: a DLL unloaded since would leave its
    // tables unmapped.)
    let mut thread_names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    imp::start_rescuer();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let beat = LAST_BEAT.load(Ordering::Relaxed);
        let late = now_ms().saturating_sub(beat);
        if beat != stall_from {
            // A frame came: a new stall starts its samples again.
            stall_from = beat;
            taken = 0;
            thread_names.clear();
        }
        let due = SAMPLE_AT_MS.get(taken).copied().unwrap_or_else(|| {
            SAMPLE_AT_MS[SAMPLE_AT_MS.len() - 1]
                + REPEAT_MS * (taken + 1 - SAMPLE_AT_MS.len()) as u64
        });
        if late < due {
            continue;
        }
        taken += 1;
        let started = Instant::now();
        let modules = imp::Modules::now();
        let (stacks, rescued) = imp::sample_threads(&modules, &mut thread_names);
        let walked_ms = started.elapsed().as_secs_f64() * 1000.0;
        if rescued > 0 {
            crate::warn!(
                World,
                "hang sampler: {rescued} threads stayed paused over 250 ms and were resumed (their stacks dropped)"
            );
        }
        if taken <= RAW_SAMPLES {
            log_raw(&stacks, &modules, late);
        }
        let sample = Sample {
            stall: stall_from,
            taken,
            late,
            at: crate::process_elapsed_ns() as f64 / 1e9,
            walked_ms,
            modules,
            stacks,
        };
        match to_namer.try_send(sample) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                dropped += 1;
                if dropped.is_power_of_two() {
                    crate::warn!(World, "hang sampler: {dropped} samples dropped (their names are behind)");
                }
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
        }
    }
}

/// Each thread's top frames as image+offset, without symbols: the names
/// read the program's .pdb, which on a busy drive came minutes after the
/// stall (the offsets are named later from the same build's .pdb).
fn log_raw(stacks: &[Stack], modules: &imp::Modules, late: u64) {
    for stack in stacks {
        let frames: Vec<String> = stack.pcs.iter().take(RAW_FRAMES).map(|&pc| modules.place(pc)).collect();
        crate::warn!(
            World,
            "hang sampler: no frame for {:.1} s; thread {} ({}) raw: {}",
            late as f64 / 1000.0,
            stack.id,
            stack.name,
            frames.join(" <- ")
        );
    }
}

/// Names each sample's stacks and logs the threads in this program's code
/// (a line can come well after its sample: its "at" is when it was taken).
fn name_samples(samples: std::sync::mpsc::Receiver<Sample>) {
    // At background I/O priority, so the game's own reads go first.
    imp::quiet_io(true);
    let mut names = imp::Symbols::new();
    let mut stall = None;
    let mut last_lines: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for sample in samples {
        if stall != Some(sample.stall) {
            stall = Some(sample.stall);
            last_lines.clear();
        }
        let started = Instant::now();
        let late = sample.late as f64 / 1000.0;
        let at = sample.at;
        let mut shown = 0usize;
        for stack in &sample.stacks {
            let frames: Vec<(String, bool)> = stack
                .pcs
                .iter()
                .map(|&pc| {
                    let (name, named) = names.name(pc, &sample.modules);
                    // Unnamed code in the exe is this program's too.
                    let ours = sample.modules.in_exe(pc) && (!named || is_ours(&name));
                    (name, ours)
                })
                .collect();
            if !frames.iter().any(|f| f.1) {
                continue;
            }
            shown += 1;
            let line = compact(&frames);
            if last_lines.get(&stack.id) == Some(&line) {
                crate::warn!(
                    World,
                    "hang sampler: no frame for {late:.1} s (at {at:.1} s); thread {} ({}): same as before",
                    stack.id,
                    stack.name
                );
                continue;
            }
            crate::warn!(
                World,
                "hang sampler: no frame for {late:.1} s (at {at:.1} s); thread {} ({}): {line}",
                stack.id,
                stack.name
            );
            last_lines.insert(stack.id, line);
        }
        if shown == 0 && sample.taken <= 2 {
            crate::warn!(
                World,
                "hang sampler: no frame for {late:.1} s (at {at:.1} s); none of {} threads is in this program's code (all waiting)",
                sample.stacks.len()
            );
        }
        if sample.taken == 1 {
            crate::info!(
                World,
                "hang sampler: {} threads walked in {:.0} ms, named in {:.0} ms",
                sample.stacks.len(),
                sample.walked_ms,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}

/// The crates whose frames are only the machinery around this program's
/// code (the runtime, bevy's scheduler, thread pools, the OS).
const MACHINERY: &[&str] = &[
    "std",
    "core",
    "alloc",
    "bevy_",
    "async_",
    "futures",
    "event_listener",
    "parking",
    "concurrent_queue",
    "crossbeam",
    "rayon",
    "tokio",
    "mio",
    "hashbrown",
    "smallvec",
    "tracing",
    "log",
    "winit",
    "diag",
];

/// A frame in this program's own code: a Rust path (`crate::module::fn`)
/// outside the machinery.
fn is_ours(frame: &str) -> bool {
    let path = frame.trim_start_matches('<');
    let Some((krate, _)) = path.split_once("::") else {
        return false;
    };
    !MACHINERY.iter().any(|m| {
        if m.ends_with('_') {
            krate.starts_with(m)
        } else {
            krate == *m || krate.starts_with(&format!("{m}_"))
        }
    })
}

/// The stack innermost first, the call it is in kept, runs of machinery
/// frames shortened to "...", at most 18 frames.
fn compact(frames: &[(String, bool)]) -> String {
    let mut out: Vec<&str> = Vec::new();
    for (i, (f, ours)) in frames.iter().enumerate() {
        if i < 2 || *ours {
            out.push(f);
        } else if out.last() != Some(&"...") {
            out.push("...");
        }
        if out.len() >= 18 {
            break;
        }
    }
    out.join(" <- ")
}

struct Stack {
    id: u32,
    name: String,
    pcs: Vec<u64>,
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod imp {
    use super::Stack;
    use core::ffi::c_void;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

    pub(super) const AVAILABLE: bool = true;

    type Handle = isize;

    /// The x86_64 `CONTEXT` (1232 bytes, 16-aligned), read at its offsets.
    #[repr(C, align(16))]
    struct Context([u8; 1232]);

    const CONTEXT_FULL: u32 = 0x0010_000B;
    const FLAGS_AT: usize = 0x30;
    const RSP_AT: usize = 0x98;
    const RIP_AT: usize = 0xF8;

    impl Context {
        fn u64_at(&self, at: usize) -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.0[at..at + 8]);
            u64::from_le_bytes(b)
        }
        fn set_u64(&mut self, at: usize, v: u64) {
            self.0[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
    }

    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner_pid: u32,
        base_pri: i32,
        delta_pri: i32,
        flags: u32,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentThread() -> Handle;
        fn SetThreadPriority(h: Handle, priority: i32) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn SuspendThread(h: Handle) -> u32;
        fn ResumeThread(h: Handle) -> u32;
        fn GetThreadContext(h: Handle, ctx: *mut Context) -> i32;
        fn K32EnumProcessModules(
            process: Handle,
            modules: *mut Handle,
            bytes: u32,
            needed: *mut u32,
        ) -> i32;
        fn K32GetModuleInformation(
            process: Handle,
            module: Handle,
            info: *mut [usize; 3],
            bytes: u32,
        ) -> i32;
        fn GetModuleFileNameW(module: Handle, name: *mut u16, len: u32) -> u32;
        fn GetModuleHandleW(name: *const u16) -> Handle;
        fn RtlVirtualUnwind(
            handler_type: u32,
            image_base: u64,
            pc: u64,
            entry: *mut c_void,
            ctx: *mut Context,
            handler_data: *mut *mut c_void,
            establisher: *mut u64,
            pointers: *mut c_void,
        ) -> *mut c_void;
        fn LoadLibraryW(name: *const u16) -> Handle;
        fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
        fn LocalFree(mem: *mut c_void) -> *mut c_void;
    }

    const TH32CS_SNAPTHREAD: u32 = 0x4;
    const THREAD_ACCESS: u32 = 0x0002 | 0x0008 | 0x0040; // suspend/resume, get context, query
    const MAX_FRAMES: usize = 48;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationThread(
            h: Handle,
            class: u32,
            info: *mut c_void,
            len: u32,
            written: *mut u32,
        ) -> i32;
    }

    /// A loaded image and its function table (the `.pdata` every x86_64
    /// image carries: begin, end, unwind data, as offsets into the image).
    #[derive(Clone, Copy)]
    struct Module {
        base: u64,
        end: u64,
        table: u64,
        entries: usize,
    }

    /// The loaded images, read before the threads are paused. The walk looks
    /// a pc up in these tables itself: Windows's own lookup takes a lock a
    /// paused thread may hold (the loader's, while a DLL loads), and a walk
    /// waiting on it froze that thread for seconds.
    pub(super) struct Modules {
        list: Vec<Module>,
        names: Vec<String>,
        exe: (u64, u64),
    }

    impl Modules {
        pub(super) fn now() -> Self {
            let mut out = Self {
                list: Vec::new(),
                names: Vec::new(),
                exe: (0, 0),
            };
            // SAFETY: this process's own module list; each image's headers
            // stay mapped while it is loaded.
            unsafe {
                let process = GetCurrentProcess();
                let mut handles: Vec<Handle> = vec![0; 1024];
                let mut needed = 0u32;
                let bytes = (handles.len() * size_of::<Handle>()) as u32;
                if K32EnumProcessModules(process, handles.as_mut_ptr(), bytes, &raw mut needed) == 0
                {
                    return out;
                }
                handles.truncate((needed as usize / size_of::<Handle>()).min(handles.len()));
                let exe = GetModuleHandleW(core::ptr::null()) as u64;
                let mut found: Vec<(Module, String)> = Vec::new();
                for h in handles {
                    let mut info = [0usize; 3];
                    if K32GetModuleInformation(process, h, &raw mut info, 24) == 0 {
                        continue;
                    }
                    let (base, size) = (info[0] as u64, info[1] as u32 as u64);
                    let pe = base + ((base + 0x3C) as *const u32).read_unaligned() as u64;
                    // PE32+ only; its exception directory is the fourth.
                    if ((pe + 24) as *const u16).read_unaligned() != 0x20B {
                        continue;
                    }
                    let rva = ((pe + 160) as *const u32).read_unaligned() as u64;
                    let len = ((pe + 164) as *const u32).read_unaligned() as usize;
                    let mut name = [0u16; 260];
                    let n = GetModuleFileNameW(h, name.as_mut_ptr(), 260) as usize;
                    let path = String::from_utf16_lossy(&name[..n.min(260)]);
                    let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
                    if base == exe {
                        out.exe = (base, base + size);
                    }
                    let (table, entries) = if rva == 0 {
                        (0, 0)
                    } else {
                        (base + rva, len / 12)
                    };
                    let module = Module {
                        base,
                        end: base + size,
                        table,
                        entries,
                    };
                    found.push((module, file));
                }
                found.sort_by_key(|m| m.0.base);
                for (m, n) in found {
                    out.list.push(m);
                    out.names.push(n);
                }
            }
            out
        }

        fn index(&self, pc: u64) -> Option<usize> {
            let i = self.list.partition_point(|m| m.base <= pc).checked_sub(1)?;
            (pc < self.list[i].end).then_some(i)
        }

        pub(super) fn in_exe(&self, pc: u64) -> bool {
            pc >= self.exe.0 && pc < self.exe.1
        }

        /// The image a pc is in, and its function entry (null: a leaf).
        /// Reads only the image's own table: no locks, no allocation.
        fn entry(&self, pc: u64) -> Option<(u64, *mut c_void)> {
            let m = self.list[self.index(pc)?];
            let rva = (pc - m.base) as u32;
            let (mut lo, mut hi) = (0usize, m.entries);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let at = m.table + mid as u64 * 12;
                // SAFETY: inside the image's exception directory.
                let (begin, end, unwind) = unsafe {
                    (
                        (at as *const u32).read_unaligned(),
                        ((at + 4) as *const u32).read_unaligned(),
                        ((at + 8) as *const u32).read_unaligned(),
                    )
                };
                if rva < begin {
                    hi = mid;
                } else if rva >= end {
                    lo = mid + 1;
                } else {
                    // The low bit marks an entry that points at another.
                    let entry = if unwind & 1 == 1 {
                        m.base + (unwind & !1) as u64
                    } else {
                        at
                    };
                    return Some((m.base, entry as *mut c_void));
                }
            }
            Some((m.base, core::ptr::null_mut()))
        }

        /// "file+0x…" for a pc no symbol names.
        pub(super) fn place(&self, pc: u64) -> String {
            match self.index(pc) {
                Some(i) => format!("{}+{:#x}", self.names[i], pc - self.list[i].base),
                None => format!("{pc:#x}"),
            }
        }
    }

    /// The thread being walked (0: none), and since when (ms).
    static PAUSED: AtomicIsize = AtomicIsize::new(0);
    static PAUSED_AT: AtomicU64 = AtomicU64::new(0);
    /// Set by the rescuer once it has resumed a thread.
    static RESCUED: AtomicBool = AtomicBool::new(false);
    const RESCUE_AFTER_MS: u64 = 250;

    /// If a walk waits on a lock the paused thread holds (the loader's,
    /// while a DLL loads), the walk and the thread would wait forever: this
    /// thread resumes any thread left paused too long. It takes no locks.
    pub(super) fn start_rescuer() {
        let spawned = std::thread::Builder::new()
            .name("hang sampler rescue".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let h = PAUSED.load(Ordering::SeqCst);
                    if h == 0
                        || super::now_ms().saturating_sub(PAUSED_AT.load(Ordering::SeqCst))
                            < RESCUE_AFTER_MS
                    {
                        continue;
                    }
                    if PAUSED
                        .compare_exchange(h, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        // SAFETY: the sampler keeps the handle open until RESCUED is set.
                        unsafe { ResumeThread(h) };
                        RESCUED.store(true, Ordering::SeqCst);
                    }
                }
            });
        if let Err(e) = spawned {
            crate::info!(Launch, "hang sampler: no rescue thread: {e}");
        }
    }

    /// This thread's background mode (low I/O and memory priority) on or off.
    pub(super) fn quiet_io(on: bool) {
        // THREAD_MODE_BACKGROUND_BEGIN / _END.
        let mode = if on { 0x0001_0000 } else { 0x0002_0000 };
        // SAFETY: this thread's own pseudo handle.
        unsafe { SetThreadPriority(GetCurrentThread(), mode) };
    }

    /// The thread's environment block (where its stack's range is kept), or 0.
    unsafe fn thread_block(h: Handle) -> u64 {
        // THREAD_BASIC_INFORMATION: the block's address at 8, 48 bytes in all.
        let mut info = [0u64; 6];
        // SAFETY: an open handle with query access and a buffer of the size asked.
        let status = unsafe {
            NtQueryInformationThread(h, 0, info.as_mut_ptr().cast(), 48, core::ptr::null_mut())
        };
        if status < 0 { 0 } else { info[1] }
    }

    /// Pause each thread but this one, walk its stack into a fixed buffer
    /// (nothing is allocated while a thread is paused: it may hold the heap's
    /// lock), resume it.
    pub(super) fn sample_threads(
        modules: &Modules,
        names: &mut HashMap<u32, String>,
    ) -> (Vec<Stack>, usize) {
        let mut stacks = Vec::new();
        let mut rescued = 0usize;
        // SAFETY: plain Win32 calls on this process's own threads; every
        // handle opened is closed, every paused thread resumed.
        unsafe {
            let pid = GetCurrentProcessId();
            let me = GetCurrentThreadId();
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == -1 {
                return (stacks, 0);
            }
            let mut ids = Vec::new();
            let mut entry = ThreadEntry {
                size: size_of::<ThreadEntry>() as u32,
                usage: 0,
                thread_id: 0,
                owner_pid: 0,
                base_pri: 0,
                delta_pri: 0,
                flags: 0,
            };
            let mut more = Thread32First(snapshot, &raw mut entry) != 0;
            while more {
                if entry.owner_pid == pid && entry.thread_id != me {
                    ids.push(entry.thread_id);
                }
                more = Thread32Next(snapshot, &raw mut entry) != 0;
            }
            CloseHandle(snapshot);
            for id in ids {
                let h = OpenThread(THREAD_ACCESS, 0, id);
                if h == 0 {
                    continue;
                }
                let mut pcs = [0u64; MAX_FRAMES];
                let mut count = 0usize;
                let teb = thread_block(h);
                if teb != 0 && SuspendThread(h) != u32::MAX {
                    PAUSED_AT.store(super::now_ms(), Ordering::SeqCst);
                    PAUSED.store(h, Ordering::SeqCst);
                    let mut ctx = Context([0; 1232]);
                    ctx.0[FLAGS_AT..FLAGS_AT + 4].copy_from_slice(&CONTEXT_FULL.to_le_bytes());
                    // The stack's committed range, read while it cannot grow.
                    let stack = (
                        ((teb + 0x10) as *const u64).read(),
                        ((teb + 0x8) as *const u64).read(),
                    );
                    if GetThreadContext(h, &raw mut ctx) != 0 {
                        count = unwind(&mut ctx, &mut pcs, stack, modules);
                    }
                    if PAUSED
                        .compare_exchange(h, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        ResumeThread(h);
                    } else {
                        // The rescuer resumed it: the walk ran on a moving
                        // stack, so it is dropped. Wait for the rescuer to be
                        // done with the handle before closing it.
                        while !RESCUED.swap(false, Ordering::SeqCst) {
                            std::thread::yield_now();
                        }
                        count = 0;
                        rescued += 1;
                    }
                }
                let name = (count > 0)
                    .then(|| names.entry(id).or_insert_with(|| thread_name(h)).clone());
                CloseHandle(h);
                if let Some(name) = name {
                    stacks.push(Stack {
                        id,
                        name,
                        pcs: pcs[..count].to_vec(),
                    });
                }
            }
        }
        (stacks, rescued)
    }

    /// Walk a paused thread's stack from its context (the function tables
    /// every x86_64 Windows image carries).
    /// Every read stays inside the thread's stack (`stack`, low to high) and
    /// each step must move up it, so a frame with no unwind data (a driver's,
    /// say) ends the walk instead of reading stray memory.
    unsafe fn unwind(
        ctx: &mut Context,
        pcs: &mut [u64; MAX_FRAMES],
        stack: (u64, u64),
        modules: &Modules,
    ) -> usize {
        let mut count = 0;
        let mut last_rsp = 0u64;
        while count < MAX_FRAMES {
            let pc = ctx.u64_at(RIP_AT);
            let rsp = ctx.u64_at(RSP_AT);
            if pc == 0
                || rsp < stack.0
                || rsp + 8 > stack.1
                || !rsp.is_multiple_of(8)
                || (count > 0 && rsp <= last_rsp)
            {
                break;
            }
            // The rescuer resumed the thread: its stack is moving under the
            // walk, so stop (the caller drops what was walked).
            if PAUSED.load(Ordering::SeqCst) == 0 {
                break;
            }
            last_rsp = rsp;
            pcs[count] = pc;
            count += 1;
            // Code outside every image (made at run time): no way up.
            let Some((image_base, entry)) = modules.entry(pc) else {
                break;
            };
            if entry.is_null() {
                // A leaf function: its return address is on top of the stack.
                // SAFETY: the paused thread's own stack, in this process.
                let ret = unsafe { *(rsp as *const u64) };
                ctx.set_u64(RIP_AT, ret);
                ctx.set_u64(RSP_AT, rsp + 8);
            } else {
                // A frame-pointer function unwinds from its frame register,
                // not from rsp: a wrong pc from the frame below (a guessed
                // leaf, say) leaves junk there, and Windows would read
                // through it (a crash at a near-null address).
                // SAFETY: the entry and its unwind info are in a loaded image.
                if let Some(reg) = unsafe { frame_register(image_base, entry, pc) } {
                    let at = ctx.u64_at(0x78 + 8 * reg);
                    if at < stack.0 || at > stack.1 {
                        break;
                    }
                }
                let mut data = core::ptr::null_mut();
                let mut frame = 0u64;
                // SAFETY: the entry found for this pc, the context it came from.
                unsafe {
                    RtlVirtualUnwind(
                        0,
                        image_base,
                        pc,
                        entry,
                        ctx,
                        &raw mut data,
                        &raw mut frame,
                        core::ptr::null_mut(),
                    );
                }
            }
        }
        count
    }

    /// The register (0 = rax … 15 = r15) the unwind at `pc` will read the
    /// frame from, or None when it unwinds from rsp alone: no frame register,
    /// or `pc` is in the prologue before the frame register is set.
    unsafe fn frame_register(image_base: u64, entry: *mut c_void, pc: u64) -> Option<usize> {
        let entry = entry as u64;
        // SAFETY (whole body): RUNTIME_FUNCTION and UNWIND_INFO in the image.
        let begin = unsafe { (entry as *const u32).read_unaligned() } as u64;
        let info = image_base + unsafe { ((entry + 8) as *const u32).read_unaligned() } as u64;
        let head = unsafe { (info as *const [u8; 4]).read_unaligned() };
        let reg = (head[3] & 0x0F) as usize;
        if reg == 0 {
            return None;
        }
        let offset = pc.saturating_sub(image_base + begin);
        if offset >= head[1] as u64 {
            return Some(reg);
        }
        // Inside the prologue: only codes already run count (offset of the
        // instruction's end). UWOP_SET_FPREG is op 3; the others' slot counts
        // follow the x64 unwind format.
        let codes = info + 4;
        let mut i = 0u64;
        while i < head[2] as u64 {
            let code = unsafe { ((codes + i * 2) as *const [u8; 2]).read_unaligned() };
            let (op, op_info) = (code[1] & 0x0F, code[1] >> 4);
            if op == 3 && (code[0] as u64) <= offset {
                return Some(reg);
            }
            i += match op {
                1 if op_info == 0 => 2,
                1 => 3,
                4 | 8 => 2,
                5 | 9 => 3,
                _ => 1,
            };
        }
        None
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// The thread's name (`std::thread::Builder::name` sets it), or "".
    unsafe fn thread_name(h: Handle) -> String {
        type GetDescription = unsafe extern "system" fn(Handle, *mut *mut u16) -> i32;
        static GET: std::sync::OnceLock<Option<GetDescription>> = std::sync::OnceLock::new();
        let get = *GET.get_or_init(|| {
            // SAFETY: kernel32 is loaded in every process; the export has
            // this signature where it exists (Windows 10 1607 and later).
            unsafe {
                let k = LoadLibraryW(wide("kernel32.dll").as_ptr());
                let p = GetProcAddress(k, c"GetThreadDescription".as_ptr().cast());
                (!p.is_null()).then(|| core::mem::transmute::<*mut c_void, GetDescription>(p))
            }
        });
        let Some(get) = get else { return String::new() };
        let mut text: *mut u16 = core::ptr::null_mut();
        // SAFETY: an open thread handle and a pointer the call fills; the
        // text is freed with LocalFree as the call's documentation says.
        unsafe {
            if get(h, &raw mut text) < 0 || text.is_null() {
                return String::new();
            }
            let mut len = 0;
            while *text.add(len) != 0 && len < 256 {
                len += 1;
            }
            let name = String::from_utf16_lossy(core::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            name
        }
    }

    /// Addresses to function names through dbghelp and the program's .pdb
    /// (beside the exe), each address looked up once.
    pub(super) struct Symbols {
        from_addr: Option<SymFromAddr>,
        cache: HashMap<u64, (String, bool)>,
    }

    type SymFromAddr = unsafe extern "system" fn(Handle, u64, *mut u64, *mut u8) -> i32;

    impl Symbols {
        pub(super) fn new() -> Self {
            type SymInitialize = unsafe extern "system" fn(Handle, *const u16, i32) -> i32;
            type SymSetOptions = unsafe extern "system" fn(u32) -> u32;
            // SAFETY: dbghelp's exports with these signatures; only this
            // thread uses dbghelp (it is not thread-safe).
            let from_addr = unsafe {
                let dll = LoadLibraryW(wide("dbghelp.dll").as_ptr());
                let init = GetProcAddress(dll, c"SymInitializeW".as_ptr().cast());
                let options = GetProcAddress(dll, c"SymSetOptions".as_ptr().cast());
                let from = GetProcAddress(dll, c"SymFromAddrW".as_ptr().cast());
                if dll == 0 || init.is_null() || options.is_null() || from.is_null() {
                    None
                } else {
                    // Undecorated names, symbols loaded on first use, no
                    // prompts, no symbol server.
                    core::mem::transmute::<*mut c_void, SymSetOptions>(options)(
                        0x2 | 0x4 | 0x200 | 0x80000,
                    );
                    // The exe's folder, where its .pdb is; with none, dbghelp's own search.
                    let exe = std::env::current_exe();
                    let dir = if let Ok(p) = &exe
                        && let Some(d) = p.parent()
                    {
                        d.to_string_lossy().into_owned()
                    } else {
                        String::new()
                    };
                    let ok = core::mem::transmute::<*mut c_void, SymInitialize>(init)(
                        GetCurrentProcess(),
                        wide(&dir).as_ptr(),
                        1,
                    );
                    (ok != 0).then(|| core::mem::transmute::<*mut c_void, SymFromAddr>(from))
                }
            };
            Self {
                from_addr,
                cache: HashMap::new(),
            }
        }

        /// The function's name (true), or where it is (false).
        pub(super) fn name(&mut self, pc: u64, modules: &Modules) -> (String, bool) {
            if let Some(n) = self.cache.get(&pc) {
                return n.clone();
            }
            let name = match self.look_up(pc) {
                Some(n) => (n, true),
                None => (modules.place(pc), false),
            };
            self.cache.insert(pc, name.clone());
            name
        }

        fn look_up(&self, pc: u64) -> Option<String> {
            let from_addr = self.from_addr?;
            // SYMBOL_INFOW: 88 bytes of fields (its name's length at 76, the
            // room for it at 80, the name at 84), then the name's room.
            const MAX_NAME: usize = 512;
            let mut info = vec![0u64; (88 + MAX_NAME * 2) / 8 + 1];
            let bytes = info.as_mut_ptr().cast::<u8>();
            let mut displacement = 0u64;
            // SAFETY: a zeroed, 8-aligned SYMBOL_INFOW with its size and the
            // name's room set, large enough for both.
            unsafe {
                bytes.cast::<u32>().write(88);
                bytes.add(80).cast::<u32>().write(MAX_NAME as u32);
                if from_addr(GetCurrentProcess(), pc, &raw mut displacement, bytes) == 0 {
                    return None;
                }
                let len = (bytes.add(76).cast::<u32>().read() as usize).min(MAX_NAME);
                let name = core::slice::from_raw_parts(bytes.add(84).cast::<u16>(), len);
                Some(String::from_utf16_lossy(name))
            }
        }
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
mod imp {
    use super::Stack;

    pub(super) const AVAILABLE: bool = false;

    pub(super) struct Modules;

    impl Modules {
        pub(super) fn now() -> Self {
            Self
        }
        pub(super) fn in_exe(&self, _pc: u64) -> bool {
            false
        }
        pub(super) fn place(&self, pc: u64) -> String {
            format!("{pc:#x}")
        }
    }

    pub(super) fn sample_threads(
        _modules: &Modules,
        _names: &mut std::collections::HashMap<u32, String>,
    ) -> (Vec<Stack>, usize) {
        (Vec::new(), 0)
    }

    pub(super) fn quiet_io(_on: bool) {}

    pub(super) fn start_rescuer() {}

    pub(super) struct Symbols;

    impl Symbols {
        pub(super) fn new() -> Self {
            Self
        }
        pub(super) fn name(&mut self, pc: u64, _modules: &Modules) -> (String, bool) {
            (format!("{pc:#x}"), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ours_is_a_crate_outside_the_machinery() {
        assert!(is_ours("ui::foreign_maps::map_picture"));
        assert!(is_ours("<assets::lane::Foo as core::ops::Drop>::drop"));
        assert!(!is_ours("std::fs::read"));
        assert!(!is_ours("bevy_ecs::schedule::executor::run"));
        assert!(!is_ours("async_executor::Executor::run"));
        assert!(!is_ours("NtReadFile"));
        assert!(!is_ours("parking_lot::raw_mutex::lock_slow"));
    }

    #[test]
    fn compact_keeps_the_top_and_our_frames() {
        let frames: Vec<(String, bool)> = [
            "NtReadFile",
            "ReadFile",
            "std::fs::read",
            "std::io::x",
            "ui::maps::load",
            "bevy_ecs::run",
            "bevy_app::run",
        ]
        .map(|f| (f.to_string(), is_ours(f)))
        .to_vec();
        assert_eq!(
            compact(&frames),
            "NtReadFile <- ReadFile <- ... <- ui::maps::load <- ..."
        );
    }
}

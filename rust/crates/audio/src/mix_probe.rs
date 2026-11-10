//! bo2mp: a silent voice that times the sound device's callbacks from inside
//! the mix, and readies the device thread for it.
//!
//! The device asks for a buffer at a time and the mixer fills it in one go, so
//! the probe's samples arrive in bursts: a burst's length is the time the mixer
//! spent on that callback, and the gap between bursts is how long the device
//! waited. A burst that takes longer than the sound it makes, or a long wait,
//! is a buffer the device ran out of (static).
//!
//! The probe's first sample is pulled on the device thread itself, so that is
//! where the thread joins the Windows "Pro Audio" class and flushes denormal
//! floats to zero. Measured on a CPU burner spinning on all cores but one
//! (6v6 bots, 10 s windows): neither, 12 of 17 windows late or overrun;
//! Pro Audio only, 13 of 17 overrun; both, 0 of 16, mix 0.8-2.7 ms mean per
//! 10 ms callback.

use std::num::NonZero;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bevy::audio::{
    AddAudioSource, AudioPlayer, ChannelCount, Decodable, PlaybackSettings, SampleRate, Source,
};
use bevy::prelude::*;

const PROBE_RATE: u32 = 48_000;
const PROBE_LANES: u16 = 2;
/// The clock is read once per this many samples.
const STAMP_EVERY: u32 = 8;
/// A pause this long between samples ends a callback.
const BURST_BREAK: Duration = Duration::from_millis(1);
/// The device waited this long between callbacks: its buffer ran dry.
const LATE_WAIT: Duration = Duration::from_millis(25);

/// Callbacks seen since the last report.
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
/// Their mixer time, summed and the longest, in microseconds.
static WORK_SUM_US: AtomicU64 = AtomicU64::new(0);
static WORK_MAX_US: AtomicU64 = AtomicU64::new(0);
/// The longest wait from one callback's start to the next, in microseconds.
static WAIT_MAX_US: AtomicU64 = AtomicU64::new(0);
/// Callbacks that started LATE_WAIT or more after the one before.
static LATE: AtomicU64 = AtomicU64::new(0);
/// Callbacks whose mixing took longer than the sound they produced.
static OVERRUN: AtomicU64 = AtomicU64::new(0);

/// Whether the device thread joined the Windows "Pro Audio" class:
/// 0 not yet, 1 joined, 2 refused.
static PRO_AUDIO: AtomicU8 = AtomicU8::new(0);

/// "yes", "refused" or "not yet": did the device thread get "Pro Audio" priority.
pub(crate) fn pro_audio() -> &'static str {
    match PRO_AUDIO.load(Ordering::Relaxed) {
        1 => "yes",
        2 => "refused",
        _ => "not yet",
    }
}

/// Puts the calling thread (the device callback) in the Windows multimedia
/// class "Pro Audio" at high priority, so a busy CPU does not starve the
/// mix. cpal alone only raises it to time-critical inside a normal process,
/// which other busy programs still delay by tens of ms (static).
#[cfg(windows)]
fn join_pro_audio() {
    use core::ffi::c_void;
    #[link(name = "avrt")]
    unsafe extern "system" {
        fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> *mut c_void;
        fn AvSetMmThreadPriority(task: *mut c_void, priority: i32) -> i32;
    }
    const AVRT_PRIORITY_HIGH: i32 = 1;
    let name: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
    let mut index = 0u32;
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the
    // call and `index` is a valid out pointer; the handle is only passed back.
    let joined = unsafe {
        let task = AvSetMmThreadCharacteristicsW(name.as_ptr(), &mut index);
        !task.is_null() && AvSetMmThreadPriority(task, AVRT_PRIORITY_HIGH) != 0
    };
    PRO_AUDIO.store(if joined { 1 } else { 2 }, Ordering::Relaxed);
}

#[cfg(not(windows))]
fn join_pro_audio() {}

/// Rounds tiny floats to zero on this thread (FTZ and DAZ). An echo or filter
/// fading out of a voice gets down to "denormal" floats, which the CPU works on
/// many times slower; the mix then falls behind the device (static).
#[cfg(target_arch = "x86_64")]
#[allow(deprecated)]
fn flush_denormals() {
    use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
    const FTZ: u32 = 1 << 15;
    const DAZ: u32 = 1 << 6;
    // SAFETY: SSE is always present on x86_64; this only changes how this
    // thread rounds results below the smallest normal float.
    unsafe { _mm_setcsr(_mm_getcsr() | FTZ | DAZ) };
}

#[cfg(not(target_arch = "x86_64"))]
fn flush_denormals() {}

#[derive(Asset, TypePath, Clone)]
pub struct MixProbe;

pub struct MixProbeDecoder {
    /// The device thread is set up (on the first sample pulled).
    readied: bool,
    count: u32,
    last_stamp: Option<Instant>,
    burst_start: Option<Instant>,
    burst_samples: u32,
}

impl MixProbeDecoder {
    fn stamp(&mut self, now: Instant) {
        let Some(last) = self.last_stamp.replace(now) else {
            self.burst_start = Some(now);
            return;
        };
        if now.duration_since(last) < BURST_BREAK {
            return;
        }
        // `last` was the end of the previous callback; `now` starts a new one.
        if let Some(start) = self.burst_start.replace(now) {
            let work = last.duration_since(start);
            let wait = now.duration_since(start);
            let sound = Duration::from_secs_f64(
                f64::from(self.burst_samples) / f64::from(PROBE_RATE * u32::from(PROBE_LANES)),
            );
            CALLBACKS.fetch_add(1, Ordering::Relaxed);
            WORK_SUM_US.fetch_add(work.as_micros() as u64, Ordering::Relaxed);
            WORK_MAX_US.fetch_max(work.as_micros() as u64, Ordering::Relaxed);
            WAIT_MAX_US.fetch_max(wait.as_micros() as u64, Ordering::Relaxed);
            LATE.fetch_add(u64::from(wait >= LATE_WAIT), Ordering::Relaxed);
            OVERRUN.fetch_add(u64::from(work > sound), Ordering::Relaxed);
        }
        self.burst_samples = 0;
    }
}

impl Iterator for MixProbeDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if !self.readied {
            // The first sample is pulled on the device's own thread.
            self.readied = true;
            join_pro_audio();
            flush_denormals();
        }
        self.burst_samples += 1;
        self.count = self.count.wrapping_add(1);
        if self.count.is_multiple_of(STAMP_EVERY) {
            self.stamp(Instant::now());
        }
        Some(0.0)
    }
}

impl Source for MixProbeDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(PROBE_LANES).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(PROBE_RATE).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for MixProbe {
    type Decoder = MixProbeDecoder;

    fn decoder(&self) -> Self::Decoder {
        MixProbeDecoder {
            readied: false,
            count: 0,
            last_stamp: None,
            burst_start: None,
            burst_samples: 0,
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_audio_source::<MixProbe>();
    app.add_systems(Startup, start_probe);
}

fn start_probe(mut probes: ResMut<Assets<MixProbe>>, mut commands: Commands) {
    // CODLEGACY_HEADLESS=1: no device to time or ready.
    if crate::AudioSilent::no_device() {
        return;
    }
    commands.spawn((
        Name::new("mix probe"),
        AudioPlayer(probes.add(MixProbe)),
        PlaybackSettings::ONCE,
    ));
}

/// The callbacks since the last call: count, mean and longest mixer time (ms),
/// longest wait (ms), late callbacks, overrun callbacks. Resets the tallies.
pub(crate) fn take_report() -> (u64, f64, f64, f64, u64, u64) {
    let n = CALLBACKS.swap(0, Ordering::Relaxed);
    let sum = WORK_SUM_US.swap(0, Ordering::Relaxed);
    let mean = if n == 0 {
        0.0
    } else {
        sum as f64 / n as f64 / 1000.0
    };
    (
        n,
        mean,
        WORK_MAX_US.swap(0, Ordering::Relaxed) as f64 / 1000.0,
        WAIT_MAX_US.swap(0, Ordering::Relaxed) as f64 / 1000.0,
        LATE.swap(0, Ordering::Relaxed),
        OVERRUN.swap(0, Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_are_counted_and_timed() {
        let _ = take_report();
        let mut probe = MixProbe.decoder();
        // (Its first sample readies this thread; the test drives `stamp` itself.)
        probe.readied = true;
        let t0 = Instant::now();
        // Two callbacks of 960 samples (10 ms of sound), 10 ms apart, then a
        // third 40 ms later (late), each taking 2 ms to mix.
        let at = |probe: &mut MixProbeDecoder, start_ms: u64| {
            for i in 0..960 / STAMP_EVERY {
                probe.burst_samples += STAMP_EVERY;
                let micros =
                    start_ms * 1000 + u64::from(i) * 2000 / u64::from(960 / STAMP_EVERY - 1);
                probe.stamp(t0 + Duration::from_micros(micros));
            }
        };
        at(&mut probe, 0);
        at(&mut probe, 10);
        at(&mut probe, 50);
        at(&mut probe, 60);
        let (n, mean, max, wait, late, over) = take_report();
        assert_eq!(n, 3);
        assert!(
            (1.9..2.1).contains(&mean) && (1.9..2.1).contains(&max),
            "{mean} {max}"
        );
        assert!((39.0..41.0).contains(&wait), "{wait}");
        assert_eq!((late, over), (1, 0));
    }

    #[test]
    fn the_first_sample_readies_the_thread_and_is_silent() {
        let mut probe = MixProbe.decoder();
        assert_eq!(probe.next(), Some(0.0));
        assert!(probe.readied);
        #[cfg(windows)]
        assert_ne!(pro_audio(), "not yet");
    }
}

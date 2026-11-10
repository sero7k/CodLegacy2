//! bo2mp patch: the master bus. Every sound is summed on a mixer of its own,
//! metered, and run through a limiter before it reaches the device, so a loud
//! moment with many voices is turned down instead of clipping into static.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::source::{LimitSettings, Zero};
use rodio::{ChannelCount, MixerDeviceSink, SampleRate, Source};

/// Samples metered since the last [`take_meter`].
static SAMPLES: AtomicU64 = AtomicU64::new(0);
/// Of those, the ones over full scale before the limiter (would have clipped).
static OVER_IN: AtomicU64 = AtomicU64::new(0);
/// The ones still over full scale after it.
static OVER_OUT: AtomicU64 = AtomicU64::new(0);
/// The loudest sample before and after the limiter (`f32` bits).
static PEAK_IN: AtomicU32 = AtomicU32::new(0);
static PEAK_OUT: AtomicU32 = AtomicU32::new(0);

/// Set: the bus still mixes, meters and limits everything, then hands the
/// device silence (the game's `CODLEGACY_SOUND=off`).
static MUTED: AtomicBool = AtomicBool::new(false);

/// Silences the device from here on; the meter still reads the real mix.
pub fn mute() {
    MUTED.store(true, Ordering::Relaxed);
}

/// Whether [`mute`] was called.
pub fn muted() -> bool {
    MUTED.load(Ordering::Relaxed)
}

/// The meter adds its counts to the totals this often, not every sample.
const FLUSH_EVERY: u32 = 4800;

/// What the master bus saw since the last call: samples, samples over full
/// scale before and after the limiter, and the peaks before and after.
/// Resets the counts.
pub fn take_meter() -> (u64, u64, u64, f32, f32) {
    (
        SAMPLES.swap(0, Ordering::Relaxed),
        OVER_IN.swap(0, Ordering::Relaxed),
        OVER_OUT.swap(0, Ordering::Relaxed),
        f32::from_bits(PEAK_IN.swap(0, Ordering::Relaxed)),
        f32::from_bits(PEAK_OUT.swap(0, Ordering::Relaxed)),
    )
}

/// Opens the master bus on the device and returns the mixer sounds are added to.
pub(crate) fn open(stream: &MixerDeviceSink) -> rodio::mixer::Mixer {
    let channels = stream.config().channel_count();
    let rate = stream.config().sample_rate();
    let (mixer, sum) = rodio::mixer::mixer(channels, rate);
    // An empty mixer ends, and the device would drop it: keep one silent voice.
    mixer.add(Zero::new(channels, rate));
    let limited = Meter::new(sum, &OVER_IN, &PEAK_IN).limit(LimitSettings::default());
    stream
        .mixer()
        .add(Meter::new(limited, &OVER_OUT, &PEAK_OUT).counting());
    mixer
}

/// Counts samples over full scale and the peak, flushing to the statics.
struct Meter<S> {
    inner: S,
    over_total: &'static AtomicU64,
    peak_total: &'static AtomicU32,
    counts_samples: bool,
    n: u32,
    over: u64,
    peak: f32,
}

impl<S> Meter<S> {
    fn new(inner: S, over_total: &'static AtomicU64, peak_total: &'static AtomicU32) -> Self {
        Self {
            inner,
            over_total,
            peak_total,
            counts_samples: false,
            n: 0,
            over: 0,
            peak: 0.0,
        }
    }

    /// This meter also counts the samples and applies [`mute`] (the last
    /// stage before the device; only one meter does).
    fn counting(mut self) -> Self {
        self.counts_samples = true;
        self
    }
}

impl<S: Source> Iterator for Meter<S> {
    type Item = rodio::Sample;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let x = self.inner.next()?;
        let a = (x as f32).abs();
        self.over += u64::from(a > 1.0);
        if a > self.peak {
            self.peak = a;
        }
        self.n += 1;
        if self.n >= FLUSH_EVERY {
            if self.counts_samples {
                SAMPLES.fetch_add(u64::from(self.n), Ordering::Relaxed);
            }
            self.over_total.fetch_add(self.over, Ordering::Relaxed);
            // Positive floats order the same as their bits.
            self.peak_total
                .fetch_max(self.peak.to_bits(), Ordering::Relaxed);
            self.n = 0;
            self.over = 0;
            self.peak = 0.0;
        }
        Some(if self.counts_samples && muted() {
            0.0
        } else {
            x
        })
    }
}

impl<S: Source> Source for Meter<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_keeps_a_loud_sum_under_full_scale() {
        let ch = ChannelCount::new(2).unwrap();
        let rate = SampleRate::new(48_000).unwrap();
        let (mixer, sum) = rodio::mixer::mixer(ch, rate);
        // Three full-scale square waves at once: the sum reaches 3.0.
        for _ in 0..3 {
            let wave: Vec<f32> = (0..96_000)
                .map(|i| if (i / 48) % 2 == 0 { 1.0 } else { -1.0 })
                .collect();
            mixer.add(rodio::buffer::SamplesBuffer::new(ch, rate, wave));
        }
        let _ = take_meter();
        let out: Vec<f32> = Meter::new(sum, &OVER_IN, &PEAK_IN)
            .limit(LimitSettings::default())
            .map(|x| x as f32)
            .collect();
        let (_, over_in, _, peak_in, _) = take_meter();
        assert!(over_in > 90_000 && peak_in > 2.9, "{over_in} {peak_in}");
        // After the 5 ms attack the limiter holds the output under full scale.
        let late_over = out[4_800..].iter().filter(|x| x.abs() > 1.0).count();
        assert_eq!(late_over, 0);
    }
}

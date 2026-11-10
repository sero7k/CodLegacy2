#[path = "t5_stream.rs"]
pub(crate) mod t5_stream;

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::{num::NonZero, sync::Arc, time::Duration};

use bevy::{
    audio::{ChannelCount, Decodable, PlaybackSettings, SampleRate, Source, Volume},
    prelude::*,
};

/// Decoders the mixer holds right now (opened, not yet dropped).
pub(crate) static LIVE_SOURCES: AtomicI64 = AtomicI64::new(0);
/// Samples every decoder has handed the mixer so far.
pub(crate) static SAMPLES_MIXED: AtomicU64 = AtomicU64::new(0);
/// Of the live decoders, the ones ringing a room echo.
pub(crate) static LIVE_ECHOES: AtomicI64 = AtomicI64::new(0);
/// bo2mp: most voices that ring a room echo at once; later ones play dry.
/// Every echo runs on the one sound thread: 180 echoing voices took 816 ms
/// per second of sound (32: 68 ms; dry 180: 150 ms), so a 12-bot firefight
/// starved the device: static, and the sound sped up and slowed down.
const ECHO_CAP: i64 = 24;

/// bo2mp: samples mixed since the last call (reset), live decoders, echoing ones.
pub(crate) fn mixer_counts() -> (u64, i64, i64) {
    (
        SAMPLES_MIXED.swap(0, Ordering::Relaxed),
        LIVE_SOURCES.load(Ordering::Relaxed),
        LIVE_ECHOES.load(Ordering::Relaxed),
    )
}

#[derive(Clone, Debug)]
pub struct LivePan {
    left: Arc<AtomicU32>,
    right: Arc<AtomicU32>,
}

impl LivePan {
    pub fn unity() -> Self {
        Self::new(1.0, 1.0)
    }

    pub fn new(left: f32, right: f32) -> Self {
        Self {
            left: Arc::new(AtomicU32::new(left.to_bits())),
            right: Arc::new(AtomicU32::new(right.to_bits())),
        }
    }

    pub fn set(&self, left: f32, right: f32) {
        self.left.store(left.to_bits(), Ordering::Relaxed);
        self.right.store(right.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.left.load(Ordering::Relaxed)),
            f32::from_bits(self.right.load(Ordering::Relaxed)),
        )
    }
}

/// A clip's interleaved samples. A clip whose every sample is a whole step of
/// 1/32768 (a 16-bit or 8-bit source neither resampled nor folded) is kept as
/// those steps: half the memory of `f32`, read back as exactly the same `f32`.
/// CODLEGACY_AUDIO_PCM16=0 keeps every clip as `f32`.
#[derive(Clone, Debug)]
pub(crate) enum Samples {
    F32(Arc<[f32]>),
    I16(Arc<[i16]>),
}

impl From<Arc<[f32]>> for Samples {
    fn from(samples: Arc<[f32]>) -> Self {
        Self::F32(samples)
    }
}

impl From<Vec<f32>> for Samples {
    fn from(samples: Vec<f32>) -> Self {
        Self::F32(samples.into())
    }
}

impl Samples {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::F32(s) => s.len(),
            Self::I16(s) => s.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<f32> {
        match self {
            Self::F32(s) => s.get(i).copied(),
            Self::I16(s) => s.get(i).map(|&v| f32::from(v) / 32768.0),
        }
    }

    /// Memory the samples take.
    pub(crate) fn bytes(&self) -> u64 {
        match self {
            Self::F32(s) => (s.len() * size_of::<f32>()) as u64,
            Self::I16(s) => (s.len() * size_of::<i16>()) as u64,
        }
    }

    pub(crate) fn is_16_bit(&self) -> bool {
        matches!(self, Self::I16(_))
    }

    pub(crate) fn to_f32(&self) -> Arc<[f32]> {
        match self {
            Self::F32(s) => Arc::clone(s),
            Self::I16(s) => s.iter().map(|&v| f32::from(v) / 32768.0).collect(),
        }
    }

    pub(crate) fn truncated(&self, n: usize) -> Self {
        match self {
            Self::F32(s) => Self::F32(s[..n.min(s.len())].into()),
            Self::I16(s) => Self::I16(s[..n.min(s.len())].into()),
        }
    }

    /// The same samples as 16-bit steps, when every one of them is one;
    /// otherwise unchanged.
    pub(crate) fn packed(self) -> Self {
        match &self {
            Self::F32(s) => match steps_of(s) {
                Some(steps) => Self::I16(steps.into()),
                None => self,
            },
            Self::I16(_) => self,
        }
    }

    /// The samples rounded to the nearest 16-bit step (at most half a step
    /// off, about -96 dB), when all of them are inside the 16-bit range; a
    /// clip that overshoots it (a resample's ringing past full scale) stays
    /// `f32`, so nothing is clipped.
    pub(crate) fn quantized(self) -> Self {
        match &self {
            Self::F32(s) => match rounded_steps_of(s) {
                Some(steps) => Self::I16(steps.into()),
                None => self,
            },
            Self::I16(_) => self,
        }
    }
}

fn rounded_steps_of(samples: &[f32]) -> Option<Vec<i16>> {
    let mut out = Vec::with_capacity(samples.len());
    for &x in samples {
        let v = (x * 32768.0).round();
        if !(-32768.0..=32767.0).contains(&v) {
            return None;
        }
        out.push(v as i16);
    }
    Some(out)
}

/// `x * 32768` for every sample, when each is a whole number in the 16-bit
/// range (and none is -0.0, which would come back as +0.0).
fn steps_of(samples: &[f32]) -> Option<Vec<i16>> {
    let mut out = Vec::with_capacity(samples.len());
    for &x in samples {
        let v = x * 32768.0;
        if v != v.trunc() || !(-32768.0..=32767.0).contains(&v) || x.to_bits() == 0x8000_0000 {
            return None;
        }
        out.push(v as i16);
    }
    Some(out)
}

#[derive(Asset, TypePath, Clone)]
pub struct PcmAudio {
    samples: Samples,
    channels: u16,
    sample_rate: u32,
    live_pan: Option<LivePan>,
    /// bo2zm: the room echo this play sends into, and how much.
    echo: Option<(Arc<crate::room_echo::EchoParams>, f32)>,
}

#[derive(Asset, TypePath, Clone)]
pub struct LoopingPcmAudio(PcmAudio);

#[derive(Bundle)]
pub struct LoopingPcmPlayback {
    player: AudioPlayer<LoopingPcmAudio>,
    settings: PlaybackSettings,
}

impl LoopingPcmPlayback {
    pub fn new(handle: Handle<LoopingPcmAudio>, volume: Volume) -> Self {
        Self {
            player: AudioPlayer(handle),
            settings: PlaybackSettings::ONCE.with_volume(volume),
        }
    }
}

impl PcmAudio {
    pub(crate) fn from_prepared(
        samples: Arc<[f32]>,
        channels: u16,
        sample_rate: u32,
    ) -> Option<Self> {
        Self::from_samples(samples.into(), channels, sample_rate)
    }

    pub(crate) fn from_samples(samples: Samples, channels: u16, sample_rate: u32) -> Option<Self> {
        if samples.is_empty() || channels == 0 || sample_rate == 0 {
            return None;
        }
        Some(Self {
            samples,
            channels,
            sample_rate,
            live_pan: None,
            echo: None,
        })
    }

    pub(crate) fn samples(&self) -> &Samples {
        &self.samples
    }

    /// The root mean square of the samples (0 to 1).
    pub fn rms(&self) -> f32 {
        let n = self.samples.len().max(1) as f64;
        let sum: f64 = (0..self.samples.len())
            .filter_map(|i| self.samples.get(i))
            .map(|x| f64::from(x) * f64::from(x))
            .sum();
        (sum / n).sqrt() as f32
    }

    pub(crate) fn channel_count(&self) -> u16 {
        self.channels
    }

    pub(crate) fn rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn with_live_pan(&self) -> Self {
        Self {
            samples: self.samples.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
            live_pan: Some(LivePan::unity()),
            echo: None,
        }
    }

    /// bo2zm: this play also sends `send` (0..1) into the room's echo (it
    /// plays as stereo, then rings on for the echo's tail).
    pub fn with_echo(mut self, params: Arc<crate::room_echo::EchoParams>, send: f32) -> Self {
        if self.live_pan.is_none() {
            self.live_pan = Some(LivePan::unity());
        }
        self.echo = Some((params, send));
        self
    }

    pub fn live_pan(&self) -> Option<&LivePan> {
        self.live_pan.as_ref()
    }

    pub fn into_looping(self) -> LoopingPcmAudio {
        LoopingPcmAudio(self)
    }

    fn decoder_with_looping(&self, looping: bool) -> PcmDecoder {
        LIVE_SOURCES.fetch_add(1, Ordering::Relaxed);
        PcmDecoder {
            samples: self.samples.clone(),
            pos: 0,
            src_channels: self.channels,
            sample_rate: self.sample_rate,
            live: self.live_pan.clone(),
            pending_right: None,
            looping,
            echo: None,
            tail_left: 0,
        }
        .with_echo(self.echo.as_ref().filter(|_| !looping))
    }
}

impl LoopingPcmAudio {
    pub fn live_pan(&self) -> Option<&LivePan> {
        self.0.live_pan()
    }
}

pub struct PcmDecoder {
    samples: Samples,

    pos: usize,
    src_channels: u16,
    sample_rate: u32,
    live: Option<LivePan>,
    pending_right: Option<f32>,
    looping: bool,
    echo: Option<crate::room_echo::Echo>,
    /// Echo frames still to ring after the clip.
    tail_left: usize,
}

impl PcmDecoder {
    fn with_echo(mut self, echo: Option<&(Arc<crate::room_echo::EchoParams>, f32)>) -> Self {
        if let Some((params, send)) = echo
            && *send > 0.0
            && self.live.is_some()
            && LIVE_ECHOES
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < ECHO_CAP).then_some(n + 1)
                })
                .is_ok()
        {
            let e = crate::room_echo::Echo::new(params, self.sample_rate, *send);
            self.tail_left = e.tail_frames();
            self.echo = Some(e);
        }
        self
    }

    fn remaining_out(&self) -> usize {
        if self.live.is_some() {
            let extra = usize::from(self.pending_right.is_some());
            let ch = self.src_channels.max(1) as usize;
            let frames_left = self.samples.len().saturating_sub(self.pos) / ch;
            extra + (frames_left + self.tail_left) * 2
        } else {
            self.samples.len().saturating_sub(self.pos)
        }
    }

    fn rewind_for_loop(&mut self) -> bool {
        if self.pos < self.samples.len() {
            return true;
        }
        if !self.looping || self.samples.is_empty() {
            return false;
        }
        self.pos = 0;
        true
    }
}

impl PcmDecoder {
    fn next_sample(&mut self) -> Option<f32> {
        if let Some(right) = self.pending_right.take() {
            return Some(right);
        }
        if !self.rewind_for_loop() {
            // The clip is over; its echo rings on.
            let (Some(echo), Some(live)) = (&mut self.echo, &self.live) else {
                return None;
            };
            if self.tail_left == 0 {
                return None;
            }
            self.tail_left -= 1;
            let (gain_l, gain_r) = live.get();
            let g = (gain_l + gain_r) * 0.5;
            let (wl, wr) = echo.tick(0.0);
            self.pending_right = Some(wr * g);
            return Some(wl * g);
        }
        let Some(live) = &self.live else {
            let sample = self.samples.get(self.pos)?;
            self.pos += 1;
            return Some(sample);
        };
        let (gain_l, gain_r) = live.get();
        let ch = self.src_channels.max(1) as usize;
        let left = self.samples.get(self.pos)?;
        // (A mono clip's next sample is the next frame, not its right side.)
        let right = if ch >= 2 {
            self.samples.get(self.pos + 1).unwrap_or(left)
        } else {
            left
        };
        self.pos += ch;
        if let Some(echo) = &mut self.echo {
            let g = (gain_l + gain_r) * 0.5;
            let (wl, wr) = echo.tick((left + right) * 0.5);
            self.pending_right = Some(right * gain_r + wr * g);
            return Some(left * gain_l + wl * g);
        }
        self.pending_right = Some(right * gain_r);
        Some(left * gain_l)
    }
}

impl Iterator for PcmDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.next_sample()?;
        SAMPLES_MIXED.fetch_add(1, Ordering::Relaxed);
        // CODLEGACY_SOUND=off: every voice still plays (sinks, timing, loop and
        // stop all run as normal, so the log can prove them) but the device
        // only ever receives digital silence: the master bus mutes it after
        // its meter (bo2mp), or, should that not be set, the voice itself.
        Some(
            if crate::AudioSilent::active() && !bevy::audio::master_bus::muted() {
                0.0
            } else {
                sample
            },
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.looping {
            return (0, None);
        }
        let remaining = self.remaining_out();
        (remaining, Some(remaining))
    }
}

impl Source for PcmDecoder {
    fn current_span_len(&self) -> Option<usize> {
        (!self.looping).then(|| self.remaining_out())
    }

    fn channels(&self) -> ChannelCount {
        let n = if self.live.is_some() {
            2
        } else {
            self.src_channels.max(1)
        };
        NonZero::new(n).unwrap_or(NonZero::new(1).unwrap())
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.sample_rate).unwrap_or(NonZero::new(22_050).unwrap())
    }

    fn total_duration(&self) -> Option<Duration> {
        if self.looping {
            return None;
        }
        let frames = self.samples.len() as u64 / self.src_channels.max(1) as u64
            + self.echo.as_ref().map_or(0, |e| e.tail_frames() as u64);
        Some(Duration::from_secs_f64(
            frames as f64 / self.sample_rate.max(1) as f64,
        ))
    }
}

impl Drop for PcmDecoder {
    fn drop(&mut self) {
        LIVE_SOURCES.fetch_sub(1, Ordering::Relaxed);
        if self.echo.is_some() {
            LIVE_ECHOES.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Decodable for PcmAudio {
    type Decoder = PcmDecoder;

    fn decoder(&self) -> Self::Decoder {
        self.decoder_with_looping(false)
    }
}

impl Decodable for LoopingPcmAudio {
    type Decoder = PcmDecoder;

    fn decoder(&self) -> Self::Decoder {
        self.0.decoder_with_looping(true)
    }
}

/// bo2mp: a Black Ops streamed sound file as it is in its IWDs (its own
/// header, then ADPCM), decoded.
pub fn decode_t5_stream(bytes: &[u8]) -> Option<PcmAudio> {
    t5_stream::decode(bytes)
}

pub fn decode_audio_bytes(bytes: &[u8]) -> Option<PcmAudio> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut samples: Vec<f32> = Vec::new();
    let mut channels: u16 = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(_) => continue,
        };
        channels = channels.max(decoded.spec().channels.count() as u16);
        let mut interleaved = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        interleaved.copy_interleaved_ref(decoded);
        samples.extend_from_slice(interleaved.samples());
    }
    if samples.is_empty() || channels == 0 {
        return None;
    }
    Some(PcmAudio {
        samples: samples.into(),
        channels,
        sample_rate,
        live_pan: None,
        echo: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Arc<crate::room_echo::EchoParams> {
        Arc::new(crate::room_echo::EchoParams {
            early_ms: 30.0,
            early_gain: 0.5,
            decay_s: 0.8,
            late_gain: 1.0,
            wet: 1.0,
            input_lpf: 8000.0,
            damp_lpf: 5000.0,
            size: 1.0,
            diffusion: 0.5,
        })
    }

    fn clip(samples: Vec<f32>) -> PcmAudio {
        PcmAudio::from_prepared(samples.into(), 1, 48_000).expect("a clip")
    }

    /// A float clip rounds to within half a 16-bit step; one past full
    /// scale stays `f32`.
    #[test]
    fn pcm16_lossy_is_within_half_a_step() {
        let s: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.0137).sin() * 0.999).collect();
        let q = Samples::from(s.clone()).quantized();
        assert!(q.is_16_bit());
        for (i, &x) in s.iter().enumerate() {
            let err = (q.get(i).unwrap() - x).abs() * 32768.0;
            assert!(err <= 0.5 + 1e-3, "sample {i}: {err} steps off");
        }
        // Past full scale: kept as float, not clipped.
        assert!(!Samples::from(vec![0.5_f32, 1.2]).quantized().is_16_bit());
        // An exact clip is unchanged by it.
        let exact = vec![0.5_f32, -1.0, 1.0 / 32768.0];
        let q = Samples::from(exact.clone()).quantized();
        assert_eq!((0..3).map(|i| q.get(i).unwrap()).collect::<Vec<_>>(), exact);
    }

    /// A 16-bit clip packs and plays back the very same bits; a resampled
    /// one (or one holding -0.0) stays `f32`.
    #[test]
    fn pcm16_is_exact() {
        let steps: Vec<f32> = [-32768i16, -1, 0, 1, 12345, 32767].iter().map(|&s| f32::from(s) / 32768.0).collect();
        let packed = Samples::from(steps.clone()).packed();
        assert!(packed.is_16_bit());
        assert_eq!(packed.bytes(), (steps.len() * 2) as u64);
        let plain = clip(steps.clone()).with_live_pan();
        let small = PcmAudio::from_samples(packed, 1, 48_000).expect("a clip").with_live_pan();
        let a: Vec<u32> = plain.decoder().map(f32::to_bits).collect();
        let b: Vec<u32> = small.decoder().map(f32::to_bits).collect();
        assert_eq!(a, b);
        assert!(!Samples::from(vec![0.1f32, 0.2]).packed().is_16_bit());
        assert!(!Samples::from(vec![0.0f32, -0.0]).packed().is_16_bit());
        assert!(!Samples::from(vec![1.0f32]).packed().is_16_bit());
    }

    /// Past the cap, new voices play dry; once voices end, echo comes back.
    #[test]
    fn room_echo_is_capped() {
        let echoing = clip(vec![0.1; 64]).with_live_pan().with_echo(room(), 0.5);
        let held: Vec<PcmDecoder> = (0..ECHO_CAP + 6).map(|_| echoing.decoder()).collect();
        let rung = held.iter().filter(|d| d.echo.is_some()).count();
        assert!(rung as i64 <= ECHO_CAP, "{rung} echoing voices");
        assert!(held.iter().any(|d| d.echo.is_none()));
        drop(held);
        assert!(echoing.decoder().echo.is_some());
    }

    /// What one second of output costs for many panned voices, with and
    /// without the room echo (run with `--ignored --nocapture`, optimised).
    #[test]
    #[ignore]
    fn voice_cost_per_second() {
        let noise: Vec<f32> = (0..48_000)
            .map(|i| ((i * 7919 % 1000) as f32 / 1000.0) - 0.5)
            .collect();
        for echo in [false, true] {
            for voices in [32usize, 64, 128, 180] {
                let mut decoders: Vec<PcmDecoder> = (0..voices)
                    .map(|_| {
                        let panned = clip(noise.clone()).with_live_pan();
                        let panned = if echo {
                            panned.with_echo(room(), 0.5)
                        } else {
                            panned
                        };
                        panned.decoder()
                    })
                    .collect();
                let began = std::time::Instant::now();
                let mut sink = 0.0f32;
                for _ in 0..96_000 {
                    for d in &mut decoders {
                        sink += d.next().unwrap_or(0.0);
                    }
                }
                println!(
                    "voice cost: echo={echo} {voices} voices, 1 s took {:?} ({sink})",
                    began.elapsed()
                );
            }
        }
    }
}

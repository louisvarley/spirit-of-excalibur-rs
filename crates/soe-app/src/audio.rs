//! OPL2 music playback for `MUSIC.RES` scores.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use soe_engine::smus::{InstrumentBank, Operator, Score, Timbre, scaled_total_level};

struct Playback {
    samples: Vec<f32>,
    position: usize,
    muted: bool,
}

impl Playback {
    fn fill(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_mut(channels) {
            let (left, right) = if self.muted || self.samples.len() < 2 {
                (0.0, 0.0)
            } else {
                if self.position + 1 >= self.samples.len() {
                    self.position = 0;
                }
                let left = self.samples[self.position];
                let right = self.samples[self.position + 1];
                self.position += 2;
                (left, right)
            };
            if channels == 1 {
                frame[0] = (left + right) * 0.5;
            } else {
                frame[0] = left;
                frame[1] = right;
                for sample in frame.iter_mut().skip(2) {
                    *sample = 0.0;
                }
            }
        }
    }
}

pub struct AudioPlayer {
    _stream: cpal::Stream,
    shared: Arc<Mutex<Playback>>,
    sample_rate: u32,
    muted: Arc<AtomicBool>,
    instruments: InstrumentBank,
}

impl AudioPlayer {
    pub fn new() -> Option<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let format = supported.sample_format();
        let config = supported.config();
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels);
        let shared = Arc::new(Mutex::new(Playback {
            samples: Vec::new(),
            position: 0,
            muted: false,
        }));
        let callback_shared = Arc::clone(&shared);
        let error = |error| eprintln!("audio: {error}");
        let stream = match format {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    config,
                    move |data: &mut [f32], _| {
                        let mut playback = callback_shared
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner());
                        playback.fill(data, channels);
                    },
                    error,
                    None,
                )
                .ok()?,
            cpal::SampleFormat::I16 => {
                let scratch_shared = Arc::clone(&shared);
                device
                    .build_output_stream(
                        config,
                        move |data: &mut [i16], _| {
                            let mut scratch = vec![0.0f32; data.len()];
                            let mut playback = scratch_shared
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner());
                            playback.fill(&mut scratch, channels);
                            for (dst, src) in data.iter_mut().zip(scratch) {
                                *dst = (src.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                            }
                        },
                        error,
                        None,
                    )
                    .ok()?
            }
            cpal::SampleFormat::U16 => {
                let scratch_shared = Arc::clone(&shared);
                device
                    .build_output_stream(
                        config,
                        move |data: &mut [u16], _| {
                            let mut scratch = vec![0.0f32; data.len()];
                            let mut playback = scratch_shared
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner());
                            playback.fill(&mut scratch, channels);
                            for (dst, src) in data.iter_mut().zip(scratch) {
                                *dst = ((src.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX))
                                    as u16;
                            }
                        },
                        error,
                        None,
                    )
                    .ok()?
            }
            _ => return None,
        };
        stream.play().ok()?;
        Some(Self {
            _stream: stream,
            shared,
            sample_rate,
            muted: Arc::new(AtomicBool::new(false)),
            instruments: InstrumentBank::default(),
        })
    }

    /// Use the original instrument bank (`SEX.IDF`) for named instruments.
    #[must_use]
    pub fn with_instruments(mut self, instruments: InstrumentBank) -> Self {
        self.instruments = instruments;
        self
    }

    pub fn play_score(&self, score: &Score) {
        let samples = render_stereo(score, &self.instruments, self.sample_rate);
        let mut playback = self
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        playback.samples = samples;
        playback.position = 0;
    }

    pub fn toggle_mute(&self) -> bool {
        let muted = !self.muted.fetch_xor(true, Ordering::Relaxed);
        let mut playback = self
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        playback.muted = muted;
        muted
    }
}

fn note_to_fnum(tone: u8) -> (u16, u8) {
    let frequency = 440.0 * 2f64.powf((f64::from(tone) - 69.0) / 12.0);
    let mut block = 0u8;
    loop {
        let fnum = frequency * (1u32 << (20 - block)) as f64 / 49_716.0;
        if fnum <= 1023.0 || block == 7 {
            return (fnum.round().clamp(0.0, 1023.0) as u16, block);
        }
        block += 1;
    }
}

/// OPL patch from an original `SEX.IDF` timbre, as the AdLib driver programs
/// it: wave select 0, `additive` when the modulator's FM flag is clear.
fn timbre_patch(timbre: &Timbre) -> oplon::ChannelPatch {
    let op = |o: &Operator| oplon::OpPatch {
        mul: o.multiple,
        waveform: 0,
        tl: o.total_level,
        ksl: o.ksl,
        ksr: o.ksr,
        sustaining: o.sustaining,
        attack: o.attack,
        decay: o.decay,
        sustain: o.sustain_level,
        release: o.release,
        am: o.tremolo,
        vib: o.vibrato,
    };
    oplon::ChannelPatch {
        modulator: op(&timbre.modulator),
        carrier: op(&timbre.carrier),
        feedback: timbre.modulator.feedback,
        additive: !timbre.modulator.fm,
    }
}

/// Fallback patches for instruments missing from the bank.
fn patch_for(name: Option<&str>) -> oplon::ChannelPatch {
    use oplon::{ChannelPatch, OpPatch};
    let op = |mul, tl, attack, decay, sustain, release, waveform| OpPatch {
        mul,
        waveform,
        tl,
        attack,
        decay,
        sustain,
        release,
        ..OpPatch::default()
    };
    let name = name.unwrap_or("piano").to_ascii_lowercase();
    let (modulator, carrier, feedback, additive) = match name.as_str() {
        "clarinet" | "mclar" | "pclar" => (
            op(2, 28, 12, 4, 9, 6, 0),
            op(1, 0, 12, 4, 9, 6, 0),
            0,
            false,
        ),
        "horn" => (
            op(1, 22, 10, 6, 10, 6, 0),
            op(1, 0, 10, 6, 10, 6, 0),
            1,
            false,
        ),
        "strings" => (
            op(2, 24, 6, 8, 12, 7, 0),
            op(1, 0, 6, 8, 12, 7, 0),
            1,
            false,
        ),
        "tptvib" => (
            op(1, 16, 15, 5, 8, 7, 0),
            op(1, 0, 15, 5, 8, 7, 0),
            2,
            false,
        ),
        "flute" | "vocflute" => (
            op(1, 63, 10, 10, 12, 7, 0),
            op(1, 0, 10, 10, 12, 7, 0),
            0,
            false,
        ),
        "tomdrum" => (
            op(7, 8, 15, 15, 0, 15, 0),
            op(7, 0, 15, 15, 0, 15, 0),
            4,
            true,
        ),
        _ => (
            op(1, 34, 14, 8, 4, 7, 0),
            op(1, 0, 14, 8, 4, 7, 0),
            0,
            false,
        ),
    };
    ChannelPatch {
        modulator,
        carrier,
        feedback,
        additive,
    }
}

fn render_stereo(score: &Score, bank: &InstrumentBank, sample_rate: u32) -> Vec<f32> {
    use oplon::Opl2;
    let spq = score.seconds_per_quarter();
    let mut notes: Vec<_> = score
        .timed_notes()
        .into_iter()
        .filter(|note| note.tone < 128 && note.duration > 0.0)
        .map(|note| {
            (
                note.start * spq,
                (note.start + note.duration) * spq,
                note.tone,
                note.velocity,
                note.instrument,
            )
        })
        .collect();
    notes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if notes.is_empty() {
        return Vec::new();
    }
    let end_time = notes
        .iter()
        .map(|note| note.1)
        .fold(0.0f64, f64::max)
        // Render the whole song so the loop restarts at its real end.
        .clamp(0.5, 600.0)
        + 0.25;
    let total = (end_time * f64::from(sample_rate)).ceil() as usize;
    let mut out = Vec::with_capacity(total * 2);
    let mut opl = Opl2::new(sample_rate);
    const CHANNELS: usize = 9;
    let mut channel_end = [f64::INFINITY; CHANNELS];
    let mut next_note = 0usize;
    let dt = 1.0 / f64::from(sample_rate);
    let mut t = 0.0f64;
    while out.len() < total * 2 {
        for (channel, end) in channel_end.iter_mut().enumerate() {
            if *end <= t {
                opl.key_off(channel);
                *end = f64::INFINITY;
            }
        }
        while next_note < notes.len() && notes[next_note].0 <= t {
            let (start, end, tone, velocity, instrument) = notes[next_note];
            let channel = channel_end
                .iter()
                .position(|end| end.is_infinite())
                .unwrap_or_else(|| {
                    channel_end
                        .iter()
                        .enumerate()
                        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map_or(0, |(index, _)| index)
                });
            let name = score
                .instruments
                .get(usize::from(instrument))
                .map(String::as_str);
            let original = name.and_then(|name| bank.find(name));
            let patch = original.map_or_else(|| patch_for(name), timbre_patch);
            opl.key_off(channel);
            opl.set_patch(channel, &patch);
            let (fnum, block) = note_to_fnum(tone);
            opl.set_frequency(channel, fnum, block);
            let tl = if original.is_some() {
                scaled_total_level(patch.carrier.tl, velocity)
            } else {
                let attenuation = (127 - i32::from(velocity)) / 2;
                (i32::from(patch.carrier.tl) + attenuation).clamp(0, 63) as u8
            };
            opl.set_carrier_tl(channel, tl);
            opl.key_on(channel);
            channel_end[channel] = if end <= start { start + 0.05 } else { end };
            next_note += 1;
        }
        let (left, right) = opl.render_frame();
        out.push((left as f32 / 65_536.0 * 0.7).clamp(-1.0, 1.0));
        out.push((right as f32 / 65_536.0 * 0.7).clamp(-1.0, 1.0));
        t += dt;
    }
    let peak = out
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    if peak > 1e-6 {
        let gain = 0.8 / peak;
        for sample in &mut out {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
    out
}

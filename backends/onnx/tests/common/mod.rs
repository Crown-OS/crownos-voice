#![allow(dead_code)]

use std::f32::consts::{PI, TAU};
use std::ops::Range;
use voice_abi::{FRAME_LEN, SAMPLE_RATE};

pub const RATE: f32 = SAMPLE_RATE as f32;
const SEGMENT_SECONDS: f32 = 0.5;
const LEAD_IN_SECONDS: f32 = 1.0;
const SEGMENTS: usize = 8;
const VOICE_RMS: f32 = 0.05;
const NOISE_RMS: f32 = 0.01;
const FUNDAMENTAL_HZ: f32 = 140.0;
const FORMANTS: [(f32, f32); 3] = [(700.0, 80.0), (1_220.0, 90.0), (2_600.0, 120.0)];
const GAP_SKIP_AFTER_VOICE_SECONDS: f32 = 0.15;
const GAP_SKIP_BEFORE_VOICE_SECONDS: f32 = 0.02;

/// Formant-filtered pulse train in syllables, alternating with gaps, plus white noise: the
/// fixture of `backends/cpu/tests/common`, which the parity tolerances were measured on.
pub struct NoisySpeech {
    pub clean: Vec<f32>,
    pub noisy: Vec<f32>,
    pub voiced: Vec<Range<usize>>,
}

pub fn seconds(duration: f32) -> usize {
    (duration * RATE) as usize
}

pub fn white_noise(len: usize, rms: f32) -> Vec<f32> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let scale = rms * 3f32.sqrt();
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32 / (1u64 << 23) as f32 - 1.0) * scale
        })
        .collect()
}

fn vowel(len: usize) -> Vec<f32> {
    let mut resonators = [[0f32; 2]; FORMANTS.len()];
    let mut phase = 0.0;
    (0..len)
        .map(|n| {
            let vibrato = 1.0 + 0.03 * (TAU * 5.0 * n as f32 / RATE).sin();
            phase += FUNDAMENTAL_HZ * vibrato / RATE;
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            let mut sample = 0.0;
            for (&(frequency, bandwidth), state) in FORMANTS.iter().zip(&mut resonators) {
                let radius = (-PI * bandwidth / RATE).exp();
                let out = pulse + 2.0 * radius * (TAU * frequency / RATE).cos() * state[0]
                    - radius * radius * state[1];
                state[1] = state[0];
                state[0] = out;
                sample += out;
            }
            let envelope = (PI * n as f32 / len as f32).sin().powi(2);
            sample * envelope
        })
        .collect()
}

pub fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

impl NoisySpeech {
    pub fn new() -> Self {
        let segment = seconds(SEGMENT_SECONDS);
        let lead_in = seconds(LEAD_IN_SECONDS);
        let len = (lead_in + segment * SEGMENTS).next_multiple_of(FRAME_LEN);
        let mut clean = vec![0.0; len];
        let voiced: Vec<Range<usize>> = (0..SEGMENTS)
            .step_by(2)
            .map(|index| lead_in + index * segment..lead_in + (index + 1) * segment)
            .collect();
        let syllable = vowel(segment);
        let scale = VOICE_RMS / rms(&syllable);
        for range in &voiced {
            clean[range.clone()]
                .iter_mut()
                .zip(&syllable)
                .for_each(|(out, sample)| *out = sample * scale);
        }
        let noisy = clean
            .iter()
            .zip(white_noise(len, NOISE_RMS))
            .map(|(voice, noise)| voice + noise)
            .collect();
        Self {
            clean,
            noisy,
            voiced,
        }
    }

    /// Noise-only stretches after the lead-in, trimmed so model latency and release don't count.
    pub fn gaps(&self) -> Vec<Range<usize>> {
        self.voiced
            .windows(2)
            .map(|pair| {
                pair[0].end + seconds(GAP_SKIP_AFTER_VOICE_SECONDS)
                    ..pair[1].start - seconds(GAP_SKIP_BEFORE_VOICE_SECONDS)
            })
            .collect()
    }

    pub fn noise_reduction_db(&self, enhanced: &[f32]) -> f32 {
        let energy = |signal: &[f32]| -> f32 {
            self.gaps()
                .into_iter()
                .map(|gap| {
                    signal[gap]
                        .iter()
                        .map(|sample| sample * sample)
                        .sum::<f32>()
                })
                .sum()
        };
        10.0 * (energy(&self.noisy) / energy(enhanced).max(f32::MIN_POSITIVE)).log10()
    }

    /// Output level in the voiced stretches relative to the clean voice, in dB.
    pub fn voice_level_db(&self, enhanced: &[f32]) -> f32 {
        let voiced = |signal: &[f32]| -> Vec<f32> {
            self.voiced
                .iter()
                .skip(1)
                .flat_map(|range| signal[range.clone()].iter().copied())
                .collect()
        };
        20.0 * (rms(&voiced(enhanced)) / rms(&voiced(&self.clean))).log10()
    }
}

use voice_abi::SAMPLE_RATE;

use crate::util::biquad::{Biquad, BiquadCoefficients};
use crate::util::decibel::db_to_gain;

/// Integrated loudness offline jobs are normalised to.
pub const TARGET_LUFS: f32 = -16.0;

const SUB_BLOCK_LEN: usize = SAMPLE_RATE as usize / 10;
const SUB_BLOCKS_PER_BLOCK: usize = 4;
const BLOCK_LEN: f64 = (SUB_BLOCK_LEN * SUB_BLOCKS_PER_BLOCK) as f64;
const LOUDNESS_OFFSET_DB: f64 = -0.691;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;

/// ITU-R BS.1770 stage 1 at 48 kHz: the head-related high shelf.
const K_WEIGHTING_SHELF: BiquadCoefficients = BiquadCoefficients {
    b0: 1.535_124_859_586_97,
    b1: -2.691_696_189_406_38,
    b2: 1.198_392_810_852_85,
    a1: -1.690_659_293_182_41,
    a2: 0.732_480_774_215_85,
};

/// ITU-R BS.1770 stage 2 at 48 kHz: the RLB high-pass.
const K_WEIGHTING_HIGH_PASS: BiquadCoefficients = BiquadCoefficients {
    b0: 1.0,
    b1: -2.0,
    b2: 1.0,
    a1: -1.990_047_454_833_98,
    a2: 0.990_072_250_366_21,
};

/// EBU R128 integrated loudness of a mono 48 kHz signal, measured incrementally.
///
/// Stores one `f64` per 100 ms of audio (about 288 KiB per hour).
#[derive(Clone, Debug)]
pub struct Loudness {
    shelf: Biquad,
    high_pass: Biquad,
    sub_block_energy: f64,
    sub_block_filled: usize,
    recent_sub_blocks: [f64; SUB_BLOCKS_PER_BLOCK],
    sub_blocks_seen: usize,
    block_powers: Vec<f64>,
}

impl Loudness {
    pub fn new() -> Self {
        Self {
            shelf: Biquad::new(K_WEIGHTING_SHELF),
            high_pass: Biquad::new(K_WEIGHTING_HIGH_PASS),
            sub_block_energy: 0.0,
            sub_block_filled: 0,
            recent_sub_blocks: [0.0; SUB_BLOCKS_PER_BLOCK],
            sub_blocks_seen: 0,
            block_powers: Vec::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            let weighted = self.high_pass.filter(self.shelf.filter(f64::from(sample)));
            self.sub_block_energy += weighted * weighted;
            self.sub_block_filled += 1;
            if self.sub_block_filled == SUB_BLOCK_LEN {
                self.finish_sub_block();
            }
        }
    }

    fn finish_sub_block(&mut self) {
        self.recent_sub_blocks[self.sub_blocks_seen % SUB_BLOCKS_PER_BLOCK] = self.sub_block_energy;
        self.sub_blocks_seen += 1;
        self.sub_block_energy = 0.0;
        self.sub_block_filled = 0;
        if self.sub_blocks_seen >= SUB_BLOCKS_PER_BLOCK {
            self.block_powers
                .push(self.recent_sub_blocks.iter().sum::<f64>() / BLOCK_LEN);
        }
    }

    /// Gated integrated loudness in LUFS; `None` until a 400 ms block rises above the -70 LUFS gate.
    pub fn integrated_lufs(&self) -> Option<f32> {
        let absolute_gate = loudness_to_power(ABSOLUTE_GATE_LUFS);
        let relative_gate =
            mean_power_above(&self.block_powers, absolute_gate)? * db_to_power(RELATIVE_GATE_LU);
        let gated = mean_power_above(&self.block_powers, absolute_gate.max(relative_gate))?;
        Some(power_to_loudness(gated) as f32)
    }

    /// Linear gain that brings the measured signal to `target_lufs`.
    pub fn gain_to(&self, target_lufs: f32) -> Option<f32> {
        self.integrated_lufs()
            .map(|lufs| db_to_gain(target_lufs - lufs))
    }

    /// Linear gain that brings the measured signal to [`TARGET_LUFS`].
    pub fn normalization_gain(&self) -> Option<f32> {
        self.gain_to(TARGET_LUFS)
    }
}

impl Default for Loudness {
    fn default() -> Self {
        Self::new()
    }
}

fn mean_power_above(block_powers: &[f64], gate: f64) -> Option<f64> {
    let (sum, count) = block_powers
        .iter()
        .filter(|&&power| power > gate)
        .fold((0.0, 0usize), |(sum, count), power| {
            (sum + power, count + 1)
        });
    (count > 0).then(|| sum / count as f64)
}

fn db_to_power(db: f64) -> f64 {
    10f64.powf(db / 10.0)
}

fn loudness_to_power(lufs: f64) -> f64 {
    db_to_power(lufs - LOUDNESS_OFFSET_DB)
}

fn power_to_loudness(power: f64) -> f64 {
    LOUDNESS_OFFSET_DB + 10.0 * power.log10()
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn sine(amplitude: f32, seconds: usize) -> Vec<f32> {
        (0..SAMPLE_RATE as usize * seconds)
            .map(|n| amplitude * (TAU * 997.0 * n as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_reads_minus_three_lufs() {
        let mut loudness = Loudness::new();
        loudness.push(&sine(1.0, 5));
        let lufs = loudness.integrated_lufs().unwrap_or(f32::NAN);
        assert!((lufs + 3.01).abs() < 0.05, "{lufs}");
    }

    fn measure(signal: &[f32]) -> f32 {
        let mut loudness = Loudness::new();
        loudness.push(signal);
        loudness.integrated_lufs().unwrap_or(f32::NAN)
    }

    #[test]
    fn gates_silence_and_quiet_passages() {
        assert!(Loudness::new().integrated_lufs().is_none());
        let silence = vec![0.0; SAMPLE_RATE as usize * 3];
        assert!(measure(&silence).is_nan());

        let speech = sine(0.1, 8);
        let mixed: Vec<f32> = [silence, speech.clone(), sine(0.001, 8)].concat();
        assert!((measure(&mixed) - measure(&speech)).abs() < 0.5);
    }

    #[test]
    fn normalization_gain_reaches_target() {
        let signal: Vec<f32> = [sine(0.05, 3), sine(0.2, 3)].concat();
        let mut loudness = Loudness::new();
        loudness.push(&signal);
        let gain = loudness.normalization_gain().unwrap_or(f32::NAN);
        let normalised: Vec<f32> = signal.iter().map(|sample| sample * gain).collect();
        assert!((measure(&normalised) - TARGET_LUFS).abs() < 0.01);
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let signal = sine(0.3, 3);
        let mut whole = Loudness::new();
        whole.push(&signal);
        let mut chunked = Loudness::new();
        signal.chunks(333).for_each(|chunk| chunked.push(chunk));
        assert_eq!(whole.integrated_lufs(), chunked.integrated_lufs());
    }
}

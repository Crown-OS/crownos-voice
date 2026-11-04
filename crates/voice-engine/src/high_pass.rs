use voice_abi::SAMPLE_RATE;

use crate::util::biquad::{Biquad, BiquadCoefficients};

/// Cut-off of the rumble filter in front of every suppressor.
pub const HIGH_PASS_CUTOFF_HZ: f32 = 80.0;

/// Butterworth high-pass removing handling noise, wind rumble and DC below [`HIGH_PASS_CUTOFF_HZ`].
#[derive(Clone, Debug)]
pub struct HighPass {
    section: Biquad,
}

impl HighPass {
    pub fn new() -> Self {
        let coefficients =
            BiquadCoefficients::high_pass(f64::from(HIGH_PASS_CUTOFF_HZ), f64::from(SAMPLE_RATE));
        Self {
            section: Biquad::new(coefficients),
        }
    }

    /// Filters `input` into `output`; extra samples of the longer slice are left untouched.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (sample, filtered) in input.iter().zip(output) {
            *filtered = self.section.filter(f64::from(*sample)) as f32;
        }
    }
}

impl Default for HighPass {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn steady_state_gain(frequency_hz: f32) -> f32 {
        let input: Vec<f32> = (0..SAMPLE_RATE)
            .map(|n| (TAU * frequency_hz * n as f32 / SAMPLE_RATE as f32).sin())
            .collect();
        let mut output = vec![0.0; input.len()];
        HighPass::new().process(&input, &mut output);
        let settled = input.len() / 2..;
        let peak = |samples: &[f32]| {
            samples
                .iter()
                .fold(0f32, |peak, sample| peak.max(sample.abs()))
        };
        peak(&output[settled.clone()]) / peak(&input[settled])
    }

    #[test]
    fn passes_voice_and_cuts_rumble() {
        assert!(steady_state_gain(1_000.0) > 0.99);
        assert!(
            (steady_state_gain(HIGH_PASS_CUTOFF_HZ) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02
        );
        assert!(steady_state_gain(20.0) < 0.07);
    }
}

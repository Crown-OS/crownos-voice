use std::f64::consts::{FRAC_1_SQRT_2, TAU};

/// Normalised (`a0 == 1`) coefficients of a second-order IIR section.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BiquadCoefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoefficients {
    /// RBJ cookbook Butterworth high-pass.
    pub(crate) fn high_pass(cutoff_hz: f64, sample_rate: f64) -> Self {
        let omega = TAU * cutoff_hz / sample_rate;
        let alpha = omega.sin() / (2.0 * FRAC_1_SQRT_2);
        let cos = omega.cos();
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + cos) / 2.0 / a0;
        Self {
            b0,
            b1: -2.0 * b0,
            b2: b0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
        }
    }
}

/// Transposed direct form II section with `f64` state, so low cut-offs stay exact.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Biquad {
    coefficients: BiquadCoefficients,
    state: [f64; 2],
}

impl Biquad {
    pub(crate) const fn new(coefficients: BiquadCoefficients) -> Self {
        Self {
            coefficients,
            state: [0.0; 2],
        }
    }

    #[inline]
    pub(crate) fn filter(&mut self, sample: f64) -> f64 {
        let BiquadCoefficients { b0, b1, b2, a1, a2 } = self.coefficients;
        let output = b0 * sample + self.state[0];
        self.state[0] = b1 * sample - a1 * output + self.state[1];
        self.state[1] = b2 * sample - a2 * output;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_pass_blocks_dc() {
        let mut section = Biquad::new(BiquadCoefficients::high_pass(80.0, 48_000.0));
        let settled = (0..48_000)
            .map(|_| section.filter(1.0))
            .last()
            .unwrap_or(1.0);
        assert!(settled.abs() < 1e-6);
    }
}

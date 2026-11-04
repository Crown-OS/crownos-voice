use voice_abi::SAMPLE_RATE;

use crate::util::decibel::db_to_gain;

const CEILING_DBFS: f32 = -1.0;
const RELEASE_SECONDS: f32 = 0.05;

/// Instant-attack peak limiter: no sample leaves above [`CEILING_DBFS`].
#[derive(Clone, Debug)]
pub struct Limiter {
    ceiling: f32,
    release: f32,
    gain: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub fn new() -> Self {
        Self {
            ceiling: db_to_gain(CEILING_DBFS),
            release: (-1.0 / (RELEASE_SECONDS * SAMPLE_RATE as f32)).exp(),
            gain: 1.0,
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for sample in samples {
            let required = self.ceiling / sample.abs().max(self.ceiling);
            self.gain = (1.0 - (1.0 - self.gain) * self.release).min(required);
            *sample *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_peaks_and_recovers() {
        let mut limiter = Limiter::new();
        let mut burst = vec![2.0f32; 480];
        limiter.process(&mut burst);
        assert!(burst.iter().all(|sample| *sample <= limiter.ceiling + 1e-6));

        let mut quiet = vec![0.1f32; SAMPLE_RATE as usize / 2];
        limiter.process(&mut quiet);
        assert!((quiet[quiet.len() - 1] - 0.1).abs() < 1e-3);
    }
}

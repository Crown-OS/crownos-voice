use voice_abi::{FRAME_LEN, SAMPLE_RATE};

use crate::limiter::Limiter;
use crate::util::decibel::{db_to_gain, power_to_db};

const TARGET_LEVEL_DBFS: f32 = -20.0;
const MAX_BOOST_DB: f32 = 12.0;
const MAX_CUT_DB: f32 = 12.0;
const GATE_DBFS: f32 = -50.0;
const LEVEL_TIME_CONSTANT_SECONDS: f32 = 0.4;
const GAIN_SLEW_DB_PER_SECOND: f32 = 10.0;

/// Gentle automatic gain control towards [`TARGET_LEVEL_DBFS`] followed by a peak limiter.
///
/// Frames below [`GATE_DBFS`] keep the current gain, so pauses never pump the noise floor up.
#[derive(Clone, Debug)]
pub struct LevelControl {
    level_db: f32,
    gain_db: f32,
    gain: f32,
    limiter: Limiter,
}

impl LevelControl {
    pub fn new() -> Self {
        Self {
            level_db: TARGET_LEVEL_DBFS,
            gain_db: 0.0,
            gain: 1.0,
            limiter: Limiter::new(),
        }
    }

    /// Current AGC gain in dB, before the limiter.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for block in samples.chunks_mut(FRAME_LEN) {
            self.track_level(block);
            self.apply_gain_ramp(block);
        }
        self.limiter.process(samples);
    }

    fn track_level(&mut self, block: &[f32]) {
        let mean_square =
            block.iter().map(|sample| sample * sample).sum::<f32>() / block.len() as f32;
        let block_db = power_to_db(mean_square);
        let block_seconds = block.len() as f32 / SAMPLE_RATE as f32;
        if block_db > GATE_DBFS {
            let smoothing = (-block_seconds / LEVEL_TIME_CONSTANT_SECONDS).exp();
            self.level_db = block_db + (self.level_db - block_db) * smoothing;
        }
        let wanted_db = (TARGET_LEVEL_DBFS - self.level_db).clamp(-MAX_CUT_DB, MAX_BOOST_DB);
        let max_step_db = GAIN_SLEW_DB_PER_SECOND * block_seconds;
        self.gain_db += (wanted_db - self.gain_db).clamp(-max_step_db, max_step_db);
    }

    fn apply_gain_ramp(&mut self, block: &mut [f32]) {
        let start = self.gain;
        self.gain = db_to_gain(self.gain_db);
        let step = (self.gain - start) / block.len() as f32;
        for (index, sample) in block.iter_mut().enumerate() {
            *sample *= start + step * (index + 1) as f32;
        }
    }
}

impl Default for LevelControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(level_control: &mut LevelControl, amplitude: f32, seconds: usize) -> Vec<f32> {
        let mut samples: Vec<f32> = (0..SAMPLE_RATE as usize * seconds)
            .map(|n| if n % 2 == 0 { amplitude } else { -amplitude })
            .collect();
        level_control.process(&mut samples);
        samples
    }

    #[test]
    fn boosts_quiet_speech_within_limits() {
        let mut level_control = LevelControl::new();
        run(&mut level_control, db_to_gain(-30.0), 4);
        assert!((level_control.gain_db() - 10.0).abs() < 0.5);
        run(&mut level_control, db_to_gain(-45.0), 6);
        assert!((level_control.gain_db() - MAX_BOOST_DB).abs() < 0.1);
    }

    #[test]
    fn ignores_silence_and_limits_peaks() {
        let mut level_control = LevelControl::new();
        run(&mut level_control, db_to_gain(-70.0), 3);
        assert!(level_control.gain_db().abs() < 1e-3);
        let loud = run(&mut level_control, 1.5, 1);
        assert!(
            loud.iter()
                .all(|sample| sample.abs() <= db_to_gain(-1.0) + 1e-6)
        );
    }
}

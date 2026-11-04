use std::io;

use voice_abi::SAMPLE_RATE;
use voice_engine::{Limiter, Loudness, TARGET_LUFS};

use super::pcm_sink::SampleSink;

/// Recordings up to this long are normalised from their exact integrated loudness.
const LOOKAHEAD_SECONDS: usize = 10;
const LOOKAHEAD_SAMPLES: usize = LOOKAHEAD_SECONDS * SAMPLE_RATE as usize;
const MAX_BOOST_DB: f32 = 20.0;

/// EBU R128 normalisation to [`TARGET_LUFS`] of a stream that cannot be read twice.
///
/// Output trails the measurement by [`LOOKAHEAD_SAMPLES`]; each released stretch gets the gain
/// for the integrated loudness of everything measured so far, ramped from the previous gain.
pub struct LookaheadNormalizer {
    measured: Loudness,
    held: Vec<f32>,
    gain: Option<f32>,
    max_gain: f32,
    limiter: Limiter,
}

impl LookaheadNormalizer {
    pub fn new() -> Self {
        Self {
            measured: Loudness::new(),
            held: Vec::new(),
            gain: None,
            max_gain: db_to_gain(MAX_BOOST_DB),
            limiter: Limiter::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32], sink: &mut impl SampleSink) -> io::Result<()> {
        self.measured.push(samples);
        self.held.extend_from_slice(samples);
        match self.held.len().checked_sub(LOOKAHEAD_SAMPLES) {
            Some(ready) if ready > 0 => self.release(ready, sink),
            _ => Ok(()),
        }
    }

    pub fn finish(&mut self, sink: &mut impl SampleSink) -> io::Result<()> {
        self.release(self.held.len(), sink)
    }

    fn release(&mut self, count: usize, sink: &mut impl SampleSink) -> io::Result<()> {
        let estimate = self.measured.gain_to(TARGET_LUFS).or(self.gain);
        let target = estimate.unwrap_or(1.0).min(self.max_gain);
        let start = self.gain.unwrap_or(target);
        let released = &mut self.held[..count];
        let step = (target - start) / count as f32;
        for (index, sample) in released.iter_mut().enumerate() {
            *sample *= start + step * (index + 1) as f32;
        }
        self.limiter.process(released);
        sink.write(released)?;
        self.held.drain(..count);
        self.gain = Some(target);
        Ok(())
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn tone(amplitude: f32, seconds: usize) -> Vec<f32> {
        (0..seconds * SAMPLE_RATE as usize)
            .map(|n| amplitude * (TAU * 440.0 * n as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    fn normalized(input: &[f32]) -> io::Result<Vec<f32>> {
        let mut normalizer = LookaheadNormalizer::new();
        let mut output = Vec::new();
        for block in input.chunks(SAMPLE_RATE as usize) {
            normalizer.push(block, &mut output)?;
        }
        normalizer.finish(&mut output)?;
        Ok(output)
    }

    fn integrated_lufs(signal: &[f32]) -> f32 {
        let mut loudness = Loudness::new();
        loudness.push(signal);
        loudness.integrated_lufs().unwrap_or(f32::NAN)
    }

    #[test]
    fn a_recording_within_the_lookahead_reaches_the_target_exactly() -> io::Result<()> {
        let input = tone(0.05, LOOKAHEAD_SECONDS - 2);
        let output = normalized(&input)?;
        assert_eq!(output.len(), input.len());
        assert!((integrated_lufs(&output) - TARGET_LUFS).abs() < 0.05);
        Ok(())
    }

    #[test]
    fn a_longer_recording_converges_on_the_target() -> io::Result<()> {
        let input = [tone(0.05, 2), tone(0.1, 2)].concat().repeat(10);
        let output = normalized(&input)?;
        assert_eq!(output.len(), input.len());
        let lufs = integrated_lufs(&output);
        assert!((lufs - TARGET_LUFS).abs() < 0.5, "{lufs}");
        Ok(())
    }

    #[test]
    fn peaks_stay_below_full_scale() -> io::Result<()> {
        let mut input = tone(0.05, 4);
        input[SAMPLE_RATE as usize] = 0.9;
        let output = normalized(&input)?;
        assert!(output.iter().all(|sample| sample.abs() < 0.9));
        Ok(())
    }
}

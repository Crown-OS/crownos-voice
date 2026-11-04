use voice_abi::Model;

use crate::{HighPass, LevelControl, Suppressor};

/// The real-time enhancement chain: [`HighPass`] → suppressor → [`LevelControl`] (Studio only).
///
/// Holds one scratch buffer that grows to the longest block seen, so a stream of equal blocks
/// never allocates after its first one.
pub struct Chain<S> {
    high_pass: HighPass,
    suppressor: S,
    level_control: Option<LevelControl>,
    filtered: Vec<f32>,
}

impl<S: Suppressor> Chain<S> {
    pub fn new(model: Model, suppressor: S) -> Self {
        Self {
            high_pass: HighPass::new(),
            suppressor,
            level_control: (model == Model::Studio).then(LevelControl::new),
            filtered: Vec::new(),
        }
    }

    pub fn suppressor(&self) -> &S {
        &self.suppressor
    }

    pub fn into_suppressor(self) -> S {
        self.suppressor
    }
}

impl<S: Suppressor> Suppressor for Chain<S> {
    type Error = S::Error;

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), S::Error> {
        self.filtered.resize(input.len(), 0.0);
        self.high_pass.process(input, &mut self.filtered);
        self.suppressor.process(&self.filtered, output)?;
        if let Some(level_control) = &mut self.level_control {
            level_control.process(output);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use voice_abi::FRAME_LEN;

    use super::*;

    struct Halve;

    impl Suppressor for Halve {
        type Error = Infallible;

        fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), Infallible> {
            input
                .iter()
                .zip(output)
                .for_each(|(sample, halved)| *halved = sample * 0.5);
            Ok(())
        }
    }

    fn tone(len: usize) -> Vec<f32> {
        (0..len).map(|n| 0.5 * (n as f32 * 0.13).sin()).collect()
    }

    #[test]
    fn runs_high_pass_then_suppressor() -> Result<(), Infallible> {
        let input = tone(4 * FRAME_LEN);
        let mut expected = vec![0.0; input.len()];
        HighPass::new().process(&input, &mut expected);
        expected.iter_mut().for_each(|sample| *sample *= 0.5);

        let mut output = vec![0.0; input.len()];
        Chain::new(Model::Standard, Halve).process(&input, &mut output)?;
        assert_eq!(output, expected);
        Ok(())
    }

    #[test]
    fn only_studio_limits_the_output() -> Result<(), Infallible> {
        let input: Vec<f32> = tone(4 * FRAME_LEN)
            .iter()
            .map(|sample| sample * 8.0)
            .collect();
        let peak = |model| -> Result<f32, Infallible> {
            let mut output = vec![0.0; input.len()];
            Chain::new(model, Halve).process(&input, &mut output)?;
            Ok(output
                .iter()
                .fold(0f32, |peak, sample| peak.max(sample.abs())))
        };
        assert!(peak(Model::Standard)? > 1.5);
        assert!(peak(Model::Studio)? < 0.9);
        Ok(())
    }
}

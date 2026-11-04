use super::erb_bands::ErbBands;
use super::post_filter::post_filter;
use super::stft::{StftAnalysis, StftSynthesis};
use super::{
    Complex32, DF_BINS, DF_COEFS_LEN, DF_ORDER, ERB_BANDS, FREQ_BINS, HOP_SIZE, LOOKAHEAD_FRAMES,
    MAX_LSNR_FOR_DEEP_FILTER_DB, MAX_LSNR_FOR_GAINS_DB, MIN_LSNR_DB,
};

const HISTORY_FRAMES: usize = if DF_ORDER > LOOKAHEAD_FRAMES {
    DF_ORDER
} else {
    LOOKAHEAD_FRAMES + 1
};
const ZERO: Complex32 = Complex32::new(0.0, 0.0);

/// The three network outputs for one time step.
#[derive(Clone, Copy, Debug)]
pub struct NetworkOutput<'a> {
    /// `enc`'s `lsnr[.., t, 0]`.
    pub lsnr: f32,
    /// `erb_dec`'s `m[.., .., t, ..]`.
    pub erb_gains: &'a [f32; ERB_BANDS],
    /// `df_dec`'s `coefs[.., t, ..]`, laid out `[DF_BINS][DF_ORDER][re, im]`.
    pub df_coefs: &'a [f32; DF_COEFS_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stages {
    Mute,
    Bypass,
    GainsOnly,
    GainsAndDeepFilter,
}

impl Stages {
    fn for_lsnr(lsnr: f32) -> Self {
        if lsnr < MIN_LSNR_DB {
            Self::Mute
        } else if lsnr > MAX_LSNR_FOR_GAINS_DB {
            Self::Bypass
        } else if lsnr > MAX_LSNR_FOR_DEEP_FILTER_DB {
            Self::GainsOnly
        } else {
            Self::GainsAndDeepFilter
        }
    }
}

/// Applies the network outputs of time step `t` to the noisy spectrum of `t - LOOKAHEAD_FRAMES`
/// and synthesises it, exactly like libDF's `DfTract::process`. The output therefore lags the
/// input by [`super::LATENCY_SAMPLES`].
pub struct Enhancer {
    analysis: StftAnalysis,
    synthesis: StftSynthesis,
    bands: ErbBands,
    history: Box<[[Complex32; FREQ_BINS]]>,
    newest: usize,
    enhanced: [Complex32; FREQ_BINS],
    post_filter_beta: Option<f32>,
    noisy_mix: Option<f32>,
}

impl Enhancer {
    pub fn new() -> Self {
        Self {
            analysis: StftAnalysis::new(),
            synthesis: StftSynthesis::new(),
            bands: ErbBands::new(),
            history: vec![[ZERO; FREQ_BINS]; HISTORY_FRAMES].into_boxed_slice(),
            newest: 0,
            enhanced: [ZERO; FREQ_BINS],
            post_filter_beta: None,
            noisy_mix: None,
        }
    }

    /// Enables the post-filter (libDF default `beta`: 0.02) for stronger suppression.
    pub fn with_post_filter(mut self, beta: f32) -> Self {
        self.post_filter_beta = (beta > 0.0).then_some(beta);
        self
    }

    /// Never attenuates more than `limit_db` by mixing the noisy spectrum back in.
    pub fn with_attenuation_limit(mut self, limit_db: f32) -> Self {
        self.noisy_mix = Some(10f32.powf(-limit_db.abs() / 20.0));
        self
    }

    /// Takes the noisy hop of time step `t` with the network outputs computed from the features
    /// of that same step, and writes one hop of enhanced audio.
    pub fn enhance(
        &mut self,
        noisy: &[f32; HOP_SIZE],
        network: &NetworkOutput<'_>,
        output: &mut [f32; HOP_SIZE],
    ) {
        self.newest = (self.newest + 1) % HISTORY_FRAMES;
        self.analysis.analyze(noisy, &mut self.history[self.newest]);
        let stages = Stages::for_lsnr(network.lsnr);

        let target_slot = self.slot(LOOKAHEAD_FRAMES);
        self.enhanced = self.history[target_slot];
        match stages {
            Stages::Mute => self.enhanced.fill(ZERO),
            Stages::Bypass => {}
            Stages::GainsOnly | Stages::GainsAndDeepFilter => {
                self.bands
                    .apply_gains(&mut self.enhanced, network.erb_gains);
            }
        }
        if stages == Stages::GainsAndDeepFilter {
            self.deep_filter(network.df_coefs);
        }

        let target = &self.history[target_slot];
        if let (Some(beta), Stages::GainsOnly | Stages::GainsAndDeepFilter) =
            (self.post_filter_beta, stages)
        {
            post_filter(target, &mut self.enhanced, beta);
        }
        if let Some(mix) = self.noisy_mix {
            for (enhanced, noisy) in self.enhanced.iter_mut().zip(target) {
                *enhanced = *enhanced * (1.0 - mix) + noisy * mix;
            }
        }
        self.synthesis.synthesize(&mut self.enhanced, output);
    }

    fn slot(&self, age: usize) -> usize {
        (self.newest + HISTORY_FRAMES - age) % HISTORY_FRAMES
    }

    /// Replaces the lowest bins with the complex FIR over the last [`DF_ORDER`] noisy frames.
    fn deep_filter(&mut self, coefs: &[f32; DF_COEFS_LEN]) {
        let slots: [usize; DF_ORDER] = std::array::from_fn(|tap| self.slot(DF_ORDER - 1 - tap));
        let taps_by_age = slots.map(|slot| &self.history[slot]);
        let (bin_taps, _) = coefs.as_chunks::<{ DF_ORDER * 2 }>();
        for (bin, (filtered, taps)) in self.enhanced[..DF_BINS]
            .iter_mut()
            .zip(bin_taps)
            .enumerate()
        {
            let (pairs, _) = taps.as_chunks::<2>();
            *filtered = pairs
                .iter()
                .zip(&taps_by_age)
                .map(|(&[re, im], frame)| frame[bin] * Complex32::new(re, im))
                .sum();
        }
    }
}

impl Default for Enhancer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::LATENCY_SAMPLES;
    use super::*;

    const UNIT_GAINS: [f32; ERB_BANDS] = [1.0; ERB_BANDS];
    const NO_COEFS: [f32; DF_COEFS_LEN] = [0.0; DF_COEFS_LEN];

    fn signal() -> Vec<f32> {
        (0..HOP_SIZE * 10)
            .map(|n| 0.2 * (n as f32 * 0.031).sin())
            .collect()
    }

    fn run(network: &NetworkOutput<'_>, input: &[f32]) -> Vec<f32> {
        let mut enhancer = Enhancer::new();
        let mut output = vec![0.0; input.len()];
        for (frame, out) in input
            .as_chunks::<HOP_SIZE>()
            .0
            .iter()
            .zip(output.as_chunks_mut::<HOP_SIZE>().0)
        {
            enhancer.enhance(frame, network, out);
        }
        output
    }

    fn max_error_after_latency(input: &[f32], output: &[f32]) -> f32 {
        input
            .iter()
            .zip(&output[LATENCY_SAMPLES..])
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max)
    }

    #[test]
    fn clean_speech_passes_with_the_model_latency() {
        let input = signal();
        let network = NetworkOutput {
            lsnr: MAX_LSNR_FOR_GAINS_DB + 5.0,
            erb_gains: &[0.0; ERB_BANDS],
            df_coefs: &NO_COEFS,
        };
        assert!(max_error_after_latency(&input, &run(&network, &input)) < 1e-5);
    }

    #[test]
    fn noise_only_frames_are_muted() {
        let network = NetworkOutput {
            lsnr: MIN_LSNR_DB - 5.0,
            erb_gains: &UNIT_GAINS,
            df_coefs: &NO_COEFS,
        };
        assert!(run(&network, &signal()).iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn identity_deep_filter_selects_the_lookahead_frame() {
        let mut coefs = NO_COEFS;
        let identity_tap = 2 * (DF_ORDER - 1 - LOOKAHEAD_FRAMES);
        coefs
            .as_chunks_mut::<{ DF_ORDER * 2 }>()
            .0
            .iter_mut()
            .for_each(|taps| taps[identity_tap] = 1.0);
        let network = NetworkOutput {
            lsnr: 0.0,
            erb_gains: &UNIT_GAINS,
            df_coefs: &coefs,
        };
        let input = signal();
        assert!(max_error_after_latency(&input, &run(&network, &input)) < 1e-5);
    }
}

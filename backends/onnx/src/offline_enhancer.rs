use voice_abi::BackendError;
use voice_engine::dfn::{Enhancer, FeatureExtractor, HOP_SIZE, NetworkOutput};

use crate::dfn_networks::DfnNetworks;
use crate::feature_sequence::FeatureSequence;
use crate::network_spec::MAX_SEQUENCE_FRAMES;
use crate::provider::Provider;

/// Feature frames (2 s) re-run ahead of each call so the recurrent state can settle.
const CONTEXT_FRAMES: usize = 200;
const MAX_CHUNK_FRAMES: usize = MAX_SEQUENCE_FRAMES - CONTEXT_FRAMES;

/// DeepFilterNet3 over whole sequences: every chunk runs the stateless graphs over the retained
/// context plus its new frames, then enhances only the new frames. Features, STFT and synthesis
/// state carry over between calls, so the output is one continuous stream lagging the input by
/// [`voice_engine::dfn::LATENCY_SAMPLES`], exactly like the CPU backend.
pub(crate) struct OfflineEnhancer {
    networks: DfnNetworks,
    provider: Provider,
    extractor: FeatureExtractor,
    enhancer: Enhancer,
    features: FeatureSequence,
}

impl OfflineEnhancer {
    pub(crate) fn new(networks: DfnNetworks, provider: Provider, enhancer: Enhancer) -> Self {
        Self {
            networks,
            provider,
            extractor: FeatureExtractor::new(),
            enhancer,
            features: FeatureSequence::default(),
        }
    }

    pub(crate) fn provider(&self) -> Provider {
        self.provider
    }

    pub(crate) fn process(
        &mut self,
        input: &[[f32; HOP_SIZE]],
        output: &mut [[f32; HOP_SIZE]],
    ) -> Result<(), BackendError> {
        input
            .chunks(MAX_CHUNK_FRAMES)
            .zip(output.chunks_mut(MAX_CHUNK_FRAMES))
            .try_for_each(|(input, output)| self.process_chunk(input, output))
    }

    fn process_chunk(
        &mut self,
        input: &[[f32; HOP_SIZE]],
        output: &mut [[f32; HOP_SIZE]],
    ) -> Result<(), BackendError> {
        for frame in input {
            self.features.push(&self.extractor.extract(frame));
        }
        let context = self.features.len() - input.len();
        let steps = self.provider.sequence_frames(self.features.len());
        let (feat_erb, feat_spec) = self.features.tensors(steps);
        let enhancer = &mut self.enhancer;
        let result = self.networks.run(steps, feat_erb, feat_spec, |estimates| {
            for (index, (noisy, enhanced)) in input.iter().zip(output).enumerate() {
                let step = context + index;
                let network = NetworkOutput {
                    lsnr: estimates.lsnr[step],
                    erb_gains: &estimates.erb_gains[step],
                    df_coefs: &estimates.df_coefs[step],
                };
                enhancer.enhance(noisy, &network, enhanced);
            }
        });
        self.features.keep_newest(CONTEXT_FRAMES);
        result
    }
}

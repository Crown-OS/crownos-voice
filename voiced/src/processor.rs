use voice_abi::Model;
use voice_abi::host::{BackendInstance, LoadError};
use voice_engine::{Chain, Suppressor};

/// Everything done to one stream's or job's audio, a whole number of frames at a time.
pub struct FrameProcessor {
    chain: Chain<BackendInstance>,
    latency_samples: usize,
}

impl FrameProcessor {
    pub fn new(model: Model, instance: BackendInstance, latency_samples: usize) -> Self {
        Self {
            chain: Chain::new(model, instance),
            latency_samples,
        }
    }

    /// How far the output lags the input.
    pub fn latency_samples(&self) -> usize {
        self.latency_samples
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), LoadError> {
        self.chain.process(input, output)
    }
}

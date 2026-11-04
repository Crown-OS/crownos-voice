use std::ffi::CStr;
use std::path::Path;

use voice_abi::{Backend, BackendError, DeviceName, FRAME_LEN, Model, Probe, Workload};
use voice_engine::dfn::{Enhancer, MODEL_DIR_NAME};

use crate::dfn_networks::DfnNetworks;
use crate::offline_enhancer::OfflineEnhancer;
use crate::provider::Provider;

/// libDF's default post-filter strength, used for the Studio profile like the CPU backend.
const STUDIO_POST_FILTER_BETA: f32 = 0.02;

/// One offline DeepFilterNet3 job on the best working ONNX Runtime execution provider.
pub struct OnnxBackend {
    engine: OfflineEnhancer,
}

impl OnnxBackend {
    /// Runs on `provider` only, loading the graphs from `network_dir` (the `deepfilternet3`
    /// directory itself).
    pub fn with_provider(
        model: Model,
        network_dir: &Path,
        provider: Provider,
    ) -> Result<Self, BackendError> {
        let enhancer = enhancer_for(model)?;
        let networks = DfnNetworks::load(network_dir, provider)?;
        Ok(Self {
            engine: OfflineEnhancer::new(networks, provider, enhancer),
        })
    }

    pub fn provider(&self) -> Provider {
        self.engine.provider()
    }
}

impl Backend for OnnxBackend {
    const NAME: &'static CStr = c"onnx";

    fn probe(workload: Workload) -> Probe {
        match (workload, Provider::detected().first()) {
            (Workload::Offline, Some(best)) => Probe {
                score: best.score(),
                device: DeviceName::new(best.label()),
            },
            _ => Probe::UNUSABLE,
        }
    }

    fn latency_samples(_model: Model) -> u32 {
        voice_engine::dfn::LATENCY_SAMPLES as u32
    }

    fn create(model: Model, workload: Workload, models_dir: &Path) -> Result<Self, BackendError> {
        if workload == Workload::Realtime {
            return Err(BackendError::Unavailable(
                "real-time DeepFilterNet3 on ONNX Runtime".into(),
            ));
        }
        enhancer_for(model)?;
        let network_dir = models_dir.join(MODEL_DIR_NAME);
        let mut last_error = BackendError::Unavailable("ONNX Runtime".into());
        for provider in Provider::detected() {
            match Self::with_provider(model, &network_dir, provider) {
                Ok(backend) => return Ok(backend),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError> {
        if input.len() != output.len() || !input.len().is_multiple_of(FRAME_LEN) {
            return Err(BackendError::InvalidLength(input.len()));
        }
        self.engine
            .process(input.as_chunks().0, output.as_chunks_mut().0)
    }
}

fn enhancer_for(model: Model) -> Result<Enhancer, BackendError> {
    match model {
        Model::Light => Err(BackendError::Unavailable(
            "the light model on ONNX Runtime".into(),
        )),
        Model::Standard => Ok(Enhancer::new()),
        Model::Studio => Ok(Enhancer::new().with_post_filter(STUDIO_POST_FILTER_BETA)),
    }
}

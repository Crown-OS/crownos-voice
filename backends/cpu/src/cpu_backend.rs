use std::ffi::CStr;
use std::path::Path;

use voice_abi::{Backend, BackendError, DeviceName, FRAME_LEN, Model, Probe, Workload};

use crate::deep_filter::{DeepFilter, Strength};
use crate::rnnoise::Rnnoise;

const REALTIME_SCORE: u32 = 50;
const OFFLINE_SCORE: u32 = 40;
const DEVICE_LABEL: &str = "DeepFilterNet3";
const RNNOISE_LATENCY_SAMPLES: u32 = FRAME_LEN as u32;
const DEEP_FILTER_LATENCY_SAMPLES: u32 = 3 * FRAME_LEN as u32;

/// One enhancement stream on the CPU.
pub struct CpuBackend {
    engine: Engine,
}

enum Engine {
    Light(Box<Rnnoise>),
    DeepFilter(DeepFilter),
}

impl Backend for CpuBackend {
    const NAME: &'static CStr = c"cpu";

    fn probe(workload: Workload) -> Probe {
        let score = match workload {
            Workload::Realtime => REALTIME_SCORE,
            Workload::Offline => OFFLINE_SCORE,
        };
        Probe {
            score,
            device: DeviceName::new(DEVICE_LABEL),
        }
    }

    fn latency_samples(model: Model) -> u32 {
        match model {
            Model::Light => RNNOISE_LATENCY_SAMPLES,
            Model::Standard | Model::Studio => DEEP_FILTER_LATENCY_SAMPLES,
        }
    }

    fn create(model: Model, _workload: Workload, _models_dir: &Path) -> Result<Self, BackendError> {
        let engine = match model {
            Model::Light => Engine::Light(Box::new(Rnnoise::new())),
            Model::Standard => Engine::DeepFilter(DeepFilter::new(Strength::Standard)?),
            Model::Studio => Engine::DeepFilter(DeepFilter::new(Strength::Studio)?),
        };
        Ok(Self { engine })
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError> {
        if input.len() != output.len() || !input.len().is_multiple_of(FRAME_LEN) {
            return Err(BackendError::InvalidLength(input.len()));
        }
        let frames = input
            .as_chunks::<FRAME_LEN>()
            .0
            .iter()
            .zip(output.as_chunks_mut::<FRAME_LEN>().0);
        for (input, output) in frames {
            match &mut self.engine {
                Engine::Light(rnnoise) => rnnoise.process_frame(input, output),
                Engine::DeepFilter(deep_filter) => deep_filter.process_frame(input, output)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_filter_latency_matches_the_engine() {
        let reported = <CpuBackend as Backend>::latency_samples(Model::Standard);
        assert_eq!(reported as usize, voice_engine::dfn::LATENCY_SAMPLES);
        assert_eq!(
            <CpuBackend as Backend>::latency_samples(Model::Light) as usize,
            FRAME_LEN
        );
    }
}

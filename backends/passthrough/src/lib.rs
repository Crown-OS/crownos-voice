//! A backend that returns its input unchanged, for testing `crown-voiced`.

use std::ffi::CStr;
use std::path::Path;

use voice_abi::{Backend, BackendError, DeviceName, Model, Probe, VoiceBackendV1, Workload};

/// The lowest usable score, so any real backend outranks it.
const PASSTHROUGH_SCORE: u32 = 1;

pub struct Passthrough;

impl Backend for Passthrough {
    const NAME: &'static CStr = c"passthrough";

    fn probe(_workload: Workload) -> Probe {
        Probe {
            score: PASSTHROUGH_SCORE,
            device: DeviceName::new("copy"),
        }
    }

    fn create(
        _model: Model,
        _workload: Workload,
        _models_dir: &Path,
    ) -> Result<Self, BackendError> {
        Ok(Self)
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError> {
        output.copy_from_slice(input);
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub static CROWN_VOICE_BACKEND_V1: VoiceBackendV1 = voice_abi::export::<Passthrough>();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_input_to_output() {
        let input: Vec<f32> = (0..voice_abi::FRAME_LEN)
            .map(|sample| sample as f32)
            .collect();
        let mut output = vec![0.0; input.len()];
        let mut backend = Passthrough;
        assert!(backend.process(&input, &mut output).is_ok());
        assert_eq!(input, output);
    }

    #[test]
    fn exports_the_current_abi() {
        assert_eq!(CROWN_VOICE_BACKEND_V1.abi_version, voice_abi::ABI_VERSION);
        assert_eq!(CROWN_VOICE_BACKEND_V1.name(), "passthrough");
        assert_eq!(
            Passthrough::probe(Workload::Realtime).score,
            PASSTHROUGH_SCORE
        );
    }
}

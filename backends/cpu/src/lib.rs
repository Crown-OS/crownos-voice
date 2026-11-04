//! CPU enhancement backend: RNNoise for [`voice_abi::Model::Light`], DeepFilterNet3 on tract for
//! [`voice_abi::Model::Standard`] and [`voice_abi::Model::Studio`]. Both models are embedded, so
//! the models directory is ignored.

mod cpu_backend;
mod deep_filter;
mod rnnoise;

pub use cpu_backend::CpuBackend;
use voice_abi::VoiceBackendV1;

#[unsafe(no_mangle)]
pub static CROWN_VOICE_BACKEND_V1: VoiceBackendV1 = voice_abi::export::<CpuBackend>();

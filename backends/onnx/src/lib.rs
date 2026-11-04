//! ONNX Runtime enhancement backend: DeepFilterNet3 (the official three-graph export) for
//! [`voice_abi::Workload::Offline`], on the best installed execution provider. ONNX Runtime is
//! mapped from `$ORT_DYLIB_PATH` or `/usr/lib/libonnxruntime.so` only when an instance is created.

mod accelerator;
mod dfn_networks;
mod execution_providers;
mod feature_sequence;
mod network_spec;
mod offline_enhancer;
mod onnx_backend;
mod provider;
mod runtime;
mod system_view;
mod util;

pub use onnx_backend::OnnxBackend;
pub use provider::Provider;
use voice_abi::VoiceBackendV1;

#[unsafe(no_mangle)]
pub static CROWN_VOICE_BACKEND_V1: VoiceBackendV1 = voice_abi::export::<OnnxBackend>();

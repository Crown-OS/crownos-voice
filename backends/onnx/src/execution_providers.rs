use ort::ep::{self, ExecutionProviderDispatch};

use crate::network_spec::{MAX_SEQUENCE_FRAMES, NetworkSpec};
use crate::provider::Provider;
use crate::util::cache_dir::cache_dir;

const OPENVINO_NPU: &str = "NPU";
const OPENVINO_GPU: &str = "GPU";

/// The ONNX Runtime registrations for running `network` on `provider`. Each one fails the
/// session instead of silently falling back, so the caller can try the next provider.
pub(crate) fn execution_providers(
    provider: Provider,
    network: &NetworkSpec,
) -> Vec<ExecutionProviderDispatch> {
    let cache = || cache_dir(provider.label());
    let registrations = match provider {
        Provider::TensorRt => vec![
            tensor_rt(network, cache()).build(),
            ep::CUDA::default().build(),
        ],
        Provider::Cuda => vec![ep::CUDA::default().build()],
        Provider::MiGraphX => vec![ep::MIGraphX::default().build()],
        Provider::OpenVinoNpu => vec![openvino(OPENVINO_NPU, cache()).build()],
        Provider::OpenVinoGpu => vec![openvino(OPENVINO_GPU, cache()).build()],
        Provider::VitisAi => vec![
            cache()
                .map_or_else(ep::Vitis::default, |dir| {
                    ep::Vitis::default().with_cache_dir(dir)
                })
                .build(),
        ],
        Provider::Cpu => Vec::new(),
    };
    registrations
        .into_iter()
        .map(ExecutionProviderDispatch::error_on_failure)
        .collect()
}

/// One optimisation profile over the whole `S` range, so TensorRT never rebuilds an engine.
fn tensor_rt(network: &NetworkSpec, cache: Option<String>) -> ep::TensorRT {
    let largest = network.tensor_rt_shapes(MAX_SEQUENCE_FRAMES);
    let tensor_rt = ep::TensorRT::default()
        .with_profile_min_shapes(network.tensor_rt_shapes(1))
        .with_profile_opt_shapes(&largest)
        .with_profile_max_shapes(largest);
    match cache {
        Some(dir) => tensor_rt
            .with_engine_cache(true)
            .with_engine_cache_path(&dir)
            .with_timing_cache(true)
            .with_timing_cache_path(dir),
        None => tensor_rt,
    }
}

fn openvino(device: &str, cache: Option<String>) -> ep::OpenVINO {
    let openvino = ep::OpenVINO::default().with_device_type(device);
    match cache {
        Some(dir) => openvino.with_cache_dir(dir),
        None => openvino,
    }
}

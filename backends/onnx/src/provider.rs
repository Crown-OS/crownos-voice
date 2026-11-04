use std::path::Path;

use crate::accelerator::Accelerator;
use crate::network_spec::MAX_SEQUENCE_FRAMES;
use crate::runtime;
use crate::system_view::{HostSystem, SystemView};

/// Shared by every provider library ONNX Runtime loads on demand.
const SHARED_PROVIDER_LIBRARY: &str = "shared";

/// An ONNX Runtime execution provider, in [`Provider::PREFERENCE`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// TensorRT, with CUDA for the nodes TensorRT rejects.
    TensorRt,
    Cuda,
    MiGraphX,
    OpenVinoNpu,
    OpenVinoGpu,
    VitisAi,
    Cpu,
}

impl Provider {
    pub const PREFERENCE: [Self; 7] = [
        Self::TensorRt,
        Self::Cuda,
        Self::MiGraphX,
        Self::OpenVinoNpu,
        Self::OpenVinoGpu,
        Self::VitisAi,
        Self::Cpu,
    ];

    /// Offline probe score: accelerators above 100, the CPU below the native CPU backend's 40.
    pub const fn score(self) -> u32 {
        match self {
            Self::TensorRt | Self::Cuda => 300,
            Self::MiGraphX => 250,
            Self::OpenVinoNpu | Self::OpenVinoGpu => 200,
            Self::VitisAi => 150,
            Self::Cpu => 30,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::TensorRt => "TensorRT",
            Self::Cuda => "CUDA",
            Self::MiGraphX => "MIGraphX",
            Self::OpenVinoNpu => "OpenVINO NPU",
            Self::OpenVinoGpu => "OpenVINO GPU",
            Self::VitisAi => "Vitis AI NPU",
            Self::Cpu => "ONNX Runtime CPU",
        }
    }

    /// Usable providers on this machine, best first; empty when ONNX Runtime is not installed.
    pub fn detected() -> Vec<Self> {
        usable(&HostSystem, &runtime::library_path()).collect()
    }

    const fn accelerator(self) -> Option<Accelerator> {
        match self {
            Self::TensorRt | Self::Cuda => Some(Accelerator::NvidiaGpu),
            Self::MiGraphX => Some(Accelerator::AmdGpu),
            Self::OpenVinoNpu => Some(Accelerator::IntelNpu),
            Self::OpenVinoGpu => Some(Accelerator::IntelGpu),
            Self::VitisAi => Some(Accelerator::AmdNpu),
            Self::Cpu => None,
        }
    }

    /// Stems of the `libonnxruntime_providers_<stem>.so` files next to `libonnxruntime.so`.
    const fn library_stems(self) -> &'static [&'static str] {
        match self {
            Self::TensorRt => &[SHARED_PROVIDER_LIBRARY, "tensorrt", "cuda"],
            Self::Cuda => &[SHARED_PROVIDER_LIBRARY, "cuda"],
            Self::MiGraphX => &[SHARED_PROVIDER_LIBRARY, "migraphx"],
            Self::OpenVinoNpu | Self::OpenVinoGpu => &[SHARED_PROVIDER_LIBRARY, "openvino"],
            Self::VitisAi => &[SHARED_PROVIDER_LIBRARY, "vitisai"],
            Self::Cpu => &[],
        }
    }

    /// Length of the time axis to run `frames` at. Providers that compile per shape get a
    /// power-of-two bucket, zero-padded at the end, which the causal graphs ignore.
    pub(crate) fn sequence_frames(self, frames: usize) -> usize {
        match self {
            Self::MiGraphX | Self::OpenVinoNpu | Self::OpenVinoGpu | Self::VitisAi => {
                frames.next_power_of_two().min(MAX_SEQUENCE_FRAMES)
            }
            Self::TensorRt | Self::Cuda | Self::Cpu => frames,
        }
    }

    fn is_usable(self, system: &dyn SystemView, runtime_dir: &Path) -> bool {
        self.accelerator()
            .is_none_or(|device| device.is_present(system))
            && self.library_stems().iter().all(|stem| {
                system.exists(&runtime_dir.join(format!("libonnxruntime_providers_{stem}.so")))
            })
    }
}

fn usable<'a>(
    system: &'a dyn SystemView,
    runtime_library: &'a Path,
) -> impl Iterator<Item = Provider> + 'a {
    let runtime_dir = runtime_library.parent().unwrap_or(Path::new(""));
    let runtime_present = system.exists(runtime_library);
    Provider::PREFERENCE
        .into_iter()
        .filter(move |provider| runtime_present && provider.is_usable(system, runtime_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_view::fake::FakeSystem;

    const RUNTIME: &str = "/usr/lib/libonnxruntime.so";
    const SHARED: &str = "/usr/lib/libonnxruntime_providers_shared.so";
    const CUDA: &str = "/usr/lib/libonnxruntime_providers_cuda.so";
    const TENSOR_RT: &str = "/usr/lib/libonnxruntime_providers_tensorrt.so";
    const MIGRAPHX: &str = "/usr/lib/libonnxruntime_providers_migraphx.so";
    const OPENVINO: &str = "/usr/lib/libonnxruntime_providers_openvino.so";
    const VITIS_AI: &str = "/usr/lib/libonnxruntime_providers_vitisai.so";

    fn detect(system: &FakeSystem) -> Vec<Provider> {
        usable(system, Path::new(RUNTIME)).collect()
    }

    #[test]
    fn nothing_is_usable_without_the_runtime() {
        let system = FakeSystem::default().with_files(&[SHARED, CUDA, "/dev/nvidia0"]);
        assert!(detect(&system).is_empty());
    }

    #[test]
    fn the_bare_runtime_offers_its_cpu_provider() {
        let system = FakeSystem::default().with_files(&[RUNTIME, "/dev/nvidia0", "/dev/kfd"]);
        assert_eq!(detect(&system), [Provider::Cpu]);
    }

    #[test]
    fn nvidia_prefers_tensor_rt_then_cuda() {
        let system =
            FakeSystem::default().with_files(&[RUNTIME, SHARED, CUDA, TENSOR_RT, "/dev/nvidia0"]);
        assert_eq!(
            detect(&system),
            [Provider::TensorRt, Provider::Cuda, Provider::Cpu]
        );
        let without_tensor_rt =
            FakeSystem::default().with_files(&[RUNTIME, SHARED, CUDA, "/dev/nvidia0"]);
        assert_eq!(detect(&without_tensor_rt), [Provider::Cuda, Provider::Cpu]);
    }

    #[test]
    fn provider_libraries_need_their_device() {
        let system = FakeSystem::default().with_files(&[
            RUNTIME, SHARED, CUDA, TENSOR_RT, MIGRAPHX, OPENVINO, VITIS_AI,
        ]);
        assert_eq!(detect(&system), [Provider::Cpu]);
    }

    #[test]
    fn provider_libraries_need_the_shared_library() {
        let system = FakeSystem::default().with_files(&[RUNTIME, CUDA, "/dev/nvidia0"]);
        assert_eq!(detect(&system), [Provider::Cpu]);
    }

    #[test]
    fn intel_prefers_the_npu_over_the_gpu() {
        let system = FakeSystem::default()
            .with_files(&[RUNTIME, SHARED, OPENVINO])
            .with_device("/sys/class/accel/accel0", "intel_vpu")
            .with_device("/sys/class/drm/renderD128", "xe");
        assert_eq!(
            detect(&system),
            [Provider::OpenVinoNpu, Provider::OpenVinoGpu, Provider::Cpu]
        );
        assert_eq!(Provider::OpenVinoNpu.label(), "OpenVINO NPU");
    }

    #[test]
    fn amd_prefers_migraphx_over_vitis_ai() {
        let system = FakeSystem::default()
            .with_files(&[RUNTIME, SHARED, MIGRAPHX, VITIS_AI, OPENVINO, "/dev/kfd"])
            .with_device("/sys/class/accel/accel0", "amdxdna")
            .with_device("/sys/class/drm/renderD128", "amdgpu");
        assert_eq!(
            detect(&system),
            [Provider::MiGraphX, Provider::VitisAi, Provider::Cpu]
        );
    }

    #[test]
    fn providers_are_looked_up_next_to_the_runtime() {
        let system = FakeSystem::default().with_files(&[
            "/opt/ort/lib/libonnxruntime.so",
            "/opt/ort/lib/libonnxruntime_providers_shared.so",
            "/opt/ort/lib/libonnxruntime_providers_cuda.so",
            CUDA,
            "/dev/nvidia0",
        ]);
        let found: Vec<_> = usable(&system, Path::new("/opt/ort/lib/libonnxruntime.so")).collect();
        assert_eq!(found, [Provider::Cuda, Provider::Cpu]);
        assert!(detect(&system).is_empty());
    }

    #[test]
    fn scores_follow_the_preference_order() {
        let scores = Provider::PREFERENCE.map(Provider::score);
        assert!(scores.is_sorted_by(|better, worse| better >= worse));
        let (accelerators, cpu) = Provider::PREFERENCE.split_at(Provider::PREFERENCE.len() - 1);
        assert!(accelerators.iter().all(|provider| provider.score() > 100));
        assert_eq!(cpu, [Provider::Cpu]);
        assert!(Provider::Cpu.score() < 40);
    }

    #[test]
    fn compiling_providers_run_power_of_two_sequences() {
        assert_eq!(Provider::MiGraphX.sequence_frames(300), 512);
        assert_eq!(
            Provider::VitisAi.sequence_frames(MAX_SEQUENCE_FRAMES),
            MAX_SEQUENCE_FRAMES
        );
        assert_eq!(Provider::Cuda.sequence_frames(300), 300);
        assert_eq!(Provider::TensorRt.sequence_frames(300), 300);
    }
}

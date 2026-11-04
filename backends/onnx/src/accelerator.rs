use std::path::Path;

use crate::system_view::SystemView;

const NVIDIA_DEVICE: &str = "/dev/nvidia0";
const AMD_COMPUTE_DEVICE: &str = "/dev/kfd";
/// One entry per `/dev/accel/accel*` node.
const ACCEL_CLASS: &str = "/sys/class/accel";
/// One `renderD*` entry per `/dev/dri/renderD*` node.
const DRM_CLASS: &str = "/sys/class/drm";
const ACCEL_NODE_PREFIX: &str = "accel";
const RENDER_NODE_PREFIX: &str = "renderD";
const INTEL_NPU_DRIVERS: &[&str] = &["intel_vpu"];
const AMD_NPU_DRIVERS: &[&str] = &["amdxdna"];
const INTEL_GPU_DRIVERS: &[&str] = &["i915", "xe"];

/// A device an execution provider needs, recognised by its device node and kernel driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Accelerator {
    NvidiaGpu,
    AmdGpu,
    IntelNpu,
    IntelGpu,
    AmdNpu,
}

impl Accelerator {
    pub(crate) fn is_present(self, system: &dyn SystemView) -> bool {
        match self {
            Self::NvidiaGpu => system.exists(Path::new(NVIDIA_DEVICE)),
            Self::AmdGpu => system.exists(Path::new(AMD_COMPUTE_DEVICE)),
            Self::IntelNpu => {
                has_node_bound_to(system, ACCEL_CLASS, ACCEL_NODE_PREFIX, INTEL_NPU_DRIVERS)
            }
            Self::AmdNpu => {
                has_node_bound_to(system, ACCEL_CLASS, ACCEL_NODE_PREFIX, AMD_NPU_DRIVERS)
            }
            Self::IntelGpu => {
                has_node_bound_to(system, DRM_CLASS, RENDER_NODE_PREFIX, INTEL_GPU_DRIVERS)
            }
        }
    }
}

fn has_node_bound_to(
    system: &dyn SystemView,
    class: &str,
    node_prefix: &str,
    drivers: &[&str],
) -> bool {
    let class = Path::new(class);
    system
        .entry_names(class)
        .iter()
        .filter(|node| {
            node.to_str()
                .is_some_and(|node| node.starts_with(node_prefix))
        })
        .filter_map(|node| system.link_target_name(&class.join(node).join("device/driver")))
        .any(|driver| drivers.iter().any(|wanted| driver == *wanted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_view::fake::FakeSystem;

    #[test]
    fn npus_are_told_apart_by_driver() {
        let intel = FakeSystem::default().with_device("/sys/class/accel/accel0", "intel_vpu");
        let amd = FakeSystem::default().with_device("/sys/class/accel/accel0", "amdxdna");
        assert!(
            Accelerator::IntelNpu.is_present(&intel) && !Accelerator::AmdNpu.is_present(&intel)
        );
        assert!(Accelerator::AmdNpu.is_present(&amd) && !Accelerator::IntelNpu.is_present(&amd));
    }

    #[test]
    fn only_intel_render_nodes_count_as_intel_gpus() {
        let hybrid = FakeSystem::default()
            .with_device("/sys/class/drm/card1", "i915")
            .with_device("/sys/class/drm/renderD128", "amdgpu")
            .with_device("/sys/class/drm/renderD129", "nvidia");
        assert!(!Accelerator::IntelGpu.is_present(&hybrid));
        let xe = FakeSystem::default().with_device("/sys/class/drm/renderD128", "xe");
        assert!(Accelerator::IntelGpu.is_present(&xe));
    }

    #[test]
    fn gpus_are_found_by_their_compute_nodes() {
        let system = FakeSystem::default().with_files(&["/dev/nvidia0", "/dev/kfd"]);
        assert!(
            Accelerator::NvidiaGpu.is_present(&system) && Accelerator::AmdGpu.is_present(&system)
        );
        assert!(!Accelerator::NvidiaGpu.is_present(&FakeSystem::default()));
    }
}

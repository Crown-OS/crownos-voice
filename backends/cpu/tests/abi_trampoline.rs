use std::ffi::c_void;
use std::ptr;

use voice_abi::{
    ABI_VERSION, FRAME_LEN, Model, Probe, Status, StreamConfig, VoiceBackendV1, Workload, export,
};
use voice_cpu::{CROWN_VOICE_BACKEND_V1, CpuBackend};

const TABLE: VoiceBackendV1 = export::<CpuBackend>();
const MODELS_DIR: &std::ffi::CStr = c"/nonexistent/models";

fn probe(workload: Workload) -> Probe {
    let mut report = Probe::UNUSABLE;
    // SAFETY: `report` is a valid out-pointer.
    assert_eq!(unsafe { (TABLE.probe)(workload, &mut report) }, Status::Ok);
    report
}

fn create(model: Model) -> *mut c_void {
    let config = StreamConfig {
        model,
        workload: Workload::Realtime,
        models_dir: MODELS_DIR.as_ptr(),
    };
    let mut instance = ptr::null_mut();
    // SAFETY: `config` and `instance` outlive the call.
    assert_eq!(
        unsafe { (TABLE.create)(&config, &mut instance) },
        Status::Ok
    );
    assert!(!instance.is_null());
    instance
}

fn process(instance: *mut c_void, input: &[f32], output: &mut [f32]) -> Status {
    assert_eq!(input.len(), output.len());
    // SAFETY: `instance` is live and both buffers hold `input.len()` samples.
    unsafe { (TABLE.process)(instance, input.as_ptr(), output.as_mut_ptr(), input.len()) }
}

#[test]
fn exported_table_describes_the_cpu_backend() {
    assert_eq!(CROWN_VOICE_BACKEND_V1.abi_version, ABI_VERSION);
    assert_eq!(CROWN_VOICE_BACKEND_V1.name(), "cpu");
    assert_eq!(TABLE.name(), "cpu");
}

#[test]
fn probe_scores_stay_below_accelerators() {
    let realtime = probe(Workload::Realtime);
    let offline = probe(Workload::Offline);
    assert_eq!((realtime.score, offline.score), (50, 40));
    assert_eq!(realtime.device.as_str(), "DeepFilterNet3");
}

#[test]
fn every_model_runs_through_the_trampolines() {
    let input: Vec<f32> = (0..FRAME_LEN * 3)
        .map(|n| 0.1 * (n as f32 * 0.07).sin())
        .collect();
    for model in [Model::Light, Model::Standard, Model::Studio] {
        let instance = create(model);
        let mut output = vec![f32::NAN; input.len()];
        assert_eq!(
            process(instance, &input, &mut output),
            Status::Ok,
            "{model:?}"
        );
        assert!(output.iter().all(|sample| sample.is_finite()), "{model:?}");
        assert_eq!(
            process(instance, &input[..100], &mut output[..100]),
            Status::InvalidInput
        );
        // SAFETY: created above and destroyed exactly once.
        unsafe { (TABLE.destroy)(instance) };
    }
}

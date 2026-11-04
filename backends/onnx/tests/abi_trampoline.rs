use std::ffi::{CString, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use voice_abi::{
    ABI_VERSION, Model, Probe, Status, StreamConfig, VoiceBackendV1, Workload, export,
};
use voice_engine::dfn::{DF_DECODER_FILE, ENCODER_FILE, ERB_DECODER_FILE, MODEL_DIR_NAME};
use voice_onnx::{CROWN_VOICE_BACKEND_V1, OnnxBackend};

const TABLE: VoiceBackendV1 = export::<OnnxBackend>();
const MISSING_RUNTIME: &str = "/nonexistent/libonnxruntime.so";

fn probe(workload: Workload) -> Probe {
    let mut report = Probe::UNUSABLE;
    // SAFETY: `report` is a valid out-pointer.
    assert_eq!(unsafe { (TABLE.probe)(workload, &mut report) }, Status::Ok);
    report
}

fn create(model: Model, workload: Workload, models_dir: &Path) -> Status {
    let models_dir = CString::new(models_dir.as_os_str().as_bytes()).unwrap_or_default();
    let config = StreamConfig {
        model,
        workload,
        models_dir: models_dir.as_ptr(),
    };
    let mut instance: *mut c_void = ptr::null_mut();
    // SAFETY: `config` and `instance` outlive the call.
    let status = unsafe { (TABLE.create)(&config, &mut instance) };
    assert!(
        instance.is_null(),
        "no instance may exist without ONNX Runtime"
    );
    status
}

/// Placeholder model files, so `create` gets as far as loading ONNX Runtime.
fn fake_models_dir() -> std::io::Result<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("voice-onnx-abi-{}", std::process::id()));
    let network_dir = root.join(MODEL_DIR_NAME);
    std::fs::create_dir_all(&network_dir)?;
    for file in [ENCODER_FILE, ERB_DECODER_FILE, DF_DECODER_FILE] {
        std::fs::write(network_dir.join(file), b"")?;
    }
    Ok(root)
}

/// One test, so the environment is only changed while no other test thread exists.
#[test]
fn without_onnx_runtime_the_backend_is_unavailable_and_never_crashes() -> std::io::Result<()> {
    // SAFETY: single-threaded at this point; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("ORT_DYLIB_PATH", MISSING_RUNTIME) };
    assert_eq!(CROWN_VOICE_BACKEND_V1.abi_version, ABI_VERSION);
    assert_eq!(TABLE.name(), "onnx");

    for workload in [Workload::Realtime, Workload::Offline] {
        let report = probe(workload);
        assert_eq!(report.score, 0, "{workload:?}");
        assert_eq!(report.device.as_str(), "");
    }

    let models_dir = fake_models_dir()?;
    for model in [Model::Light, Model::Standard, Model::Studio] {
        for workload in [Workload::Realtime, Workload::Offline] {
            assert_eq!(
                create(model, workload, &models_dir),
                Status::Unavailable,
                "{model:?} {workload:?}"
            );
        }
    }
    assert_eq!(
        create(
            Model::Standard,
            Workload::Offline,
            Path::new("/nonexistent/models")
        ),
        Status::Unavailable
    );

    let not_a_library = models_dir.join(MODEL_DIR_NAME).join(ENCODER_FILE);
    // SAFETY: as above; the trampolines run on this thread.
    unsafe { std::env::set_var("ORT_DYLIB_PATH", &not_a_library) };
    assert_eq!(probe(Workload::Offline).device.as_str(), "ONNX Runtime CPU");
    assert_eq!(
        create(Model::Standard, Workload::Offline, &models_dir),
        Status::Unavailable,
        "a runtime that fails to load is reported, not fatal"
    );
    std::fs::remove_dir_all(models_dir)
}

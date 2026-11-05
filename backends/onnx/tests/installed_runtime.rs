//! What the backend reports on this machine's ONNX Runtime (`$ORT_DYLIB_PATH`) and accelerators,
//! with the DeepFilterNet3 export in `$DFN_MODELS_DIR`. Hosts without accelerators, such as CI
//! runners, only have the CPU execution provider; nothing here assumes more.

use std::path::{Path, PathBuf};

use voice_abi::{Backend, BackendError, FRAME_LEN, Model, Workload};
use voice_onnx::{OnnxBackend, Provider};

const MODELS_DIR_VARIABLE: &str = "DFN_MODELS_DIR";
const INSTALLED_MODELS_DIR: &str = "/usr/share/crownos/voice/models/deepfilternet3";
const BLOCK_SAMPLES: usize = 100 * FRAME_LEN;

fn network_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR_VARIABLE)
        .map_or_else(|| PathBuf::from(INSTALLED_MODELS_DIR), PathBuf::from)
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files"]
fn the_cpu_provider_is_always_offered_and_works() -> Result<(), BackendError> {
    let detected = Provider::detected();
    eprintln!("detected execution providers: {detected:?}");
    assert_eq!(detected.last(), Some(&Provider::Cpu));
    let backend = OnnxBackend::with_provider(Model::Standard, &network_dir(), Provider::Cpu)?;
    assert_eq!(backend.provider(), Provider::Cpu);
    Ok(())
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files"]
fn probe_reports_the_best_detected_provider_for_offline_work_only() {
    let best = Provider::detected()[0];
    let offline = OnnxBackend::probe(Workload::Offline);
    eprintln!(
        "offline probe: {} at score {}",
        offline.device.as_str(),
        offline.score
    );
    assert_eq!(
        (offline.score, offline.device.as_str()),
        (best.score(), best.label())
    );
    assert_eq!(OnnxBackend::probe(Workload::Realtime).score, 0);
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files"]
fn create_picks_a_working_provider_and_rejects_what_it_cannot_run() -> Result<(), BackendError> {
    let models_dir = network_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut backend = OnnxBackend::create(Model::Studio, Workload::Offline, &models_dir)?;
    eprintln!("created on {:?}", backend.provider());
    let mut output = vec![0.0; BLOCK_SAMPLES];
    backend.process(&vec![0.0; BLOCK_SAMPLES], &mut output)?;
    assert!(output.iter().all(|sample| sample.is_finite()));
    for (model, workload) in [
        (Model::Light, Workload::Offline),
        (Model::Standard, Workload::Realtime),
    ] {
        assert!(matches!(
            OnnxBackend::create(model, workload, &models_dir),
            Err(BackendError::Unavailable(_))
        ));
    }
    Ok(())
}

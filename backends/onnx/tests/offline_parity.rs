//! Live checks against an installed ONNX Runtime (`$ORT_DYLIB_PATH`) and the official
//! DeepFilterNet3 export (`$DFN_MODELS_DIR`, the directory holding `enc.onnx`).

mod common;

use std::fmt::Display;
use std::path::PathBuf;

use common::{NoisySpeech, seconds};
use deep_filter_rt::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use voice_abi::{Backend, BackendError, FRAME_LEN, Model};
use voice_onnx::{OnnxBackend, Provider};

const MODELS_DIR_VARIABLE: &str = "DFN_MODELS_DIR";
const INSTALLED_MODELS_DIR: &str = "/usr/share/crownos/voice/models/deepfilternet3";
const SETTLE_SECONDS: f32 = 1.0;
/// `EXACT_DIFFERENCE_DB` of `backends/cpu/tests/dfn_parity.rs` (measured: -32 dB).
const EXACT_DIFFERENCE_DB: f32 = -25.0;
const REQUIRED_REDUCTION_DB: f32 = 10.0;
const MAX_VOICE_CHANGE_DB: f32 = 3.0;
/// The daemon's offline block: one second per `process` call.
const DAEMON_BLOCK_FRAMES: usize = 100;
/// The CPU backend's Studio post-filter.
const STUDIO_POST_FILTER_BETA: f32 = 0.02;
const MONO: usize = 1;

fn network_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR_VARIABLE)
        .map_or_else(|| PathBuf::from(INSTALLED_MODELS_DIR), PathBuf::from)
}

fn enhance_onnx(
    backend: &mut OnnxBackend,
    noisy: &[f32],
    block_frames: usize,
) -> Result<Vec<f32>, BackendError> {
    let mut output = vec![0.0; noisy.len()];
    for (input, output) in noisy
        .chunks(block_frames * FRAME_LEN)
        .zip(output.chunks_mut(block_frames * FRAME_LEN))
    {
        backend.process(input, output)?;
    }
    Ok(output)
}

fn failed(error: impl Display) -> BackendError {
    BackendError::Failed(error.to_string())
}

/// What the CPU backend computes: libDF's streaming DeepFilterNet3 on tract.
fn enhance_tract(model: Model, noisy: &[f32]) -> Result<Vec<f32>, BackendError> {
    let params = RuntimeParams::default_with_ch(MONO);
    let params = match model {
        Model::Studio => params.with_post_filter(STUDIO_POST_FILTER_BETA),
        Model::Standard | Model::Light => params,
    };
    let mut tract = DfTract::new(DfParams::default(), &params).map_err(failed)?;
    let mut output = vec![0.0; noisy.len()];
    for (frame, enhanced) in noisy
        .chunks_exact(FRAME_LEN)
        .zip(output.chunks_exact_mut(FRAME_LEN))
    {
        tract
            .process(
                ArrayView2::from_shape((MONO, FRAME_LEN), frame).map_err(failed)?,
                ArrayViewMut2::from_shape((MONO, FRAME_LEN), enhanced).map_err(failed)?,
            )
            .map_err(failed)?;
    }
    Ok(output)
}

/// Energy of `actual - expected` relative to `expected`, after the models settled.
fn difference_db(actual: &[f32], expected: &[f32]) -> f32 {
    let settled = seconds(SETTLE_SECONDS)..expected.len();
    let error: f32 = actual[settled.clone()]
        .iter()
        .zip(&expected[settled.clone()])
        .map(|(a, b)| (a - b).powi(2))
        .sum();
    let reference: f32 = expected[settled].iter().map(|b| b * b).sum();
    10.0 * (error / reference).log10()
}

/// A backend on every detected provider that builds.
fn working_backends(model: Model) -> Vec<OnnxBackend> {
    let backends: Vec<_> = Provider::detected()
        .into_iter()
        .filter_map(|provider| {
            OnnxBackend::with_provider(model, &network_dir(), provider)
                .inspect_err(|error| eprintln!("{provider:?} skipped: {error}"))
                .ok()
        })
        .collect();
    assert!(
        !backends.is_empty(),
        "no execution provider works; check ORT_DYLIB_PATH and DFN_MODELS_DIR"
    );
    backends
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files"]
fn whole_jobs_match_the_cpu_backend() -> Result<(), BackendError> {
    let speech = NoisySpeech::new();
    for model in [Model::Standard, Model::Studio] {
        let tract = enhance_tract(model, &speech.noisy)?;
        for mut backend in working_backends(model) {
            let onnx = enhance_onnx(&mut backend, &speech.noisy, speech.noisy.len() / FRAME_LEN)?;
            let difference = difference_db(&onnx, &tract);
            eprintln!(
                "{model:?} on {:?}: {difference:.1} dB from tract",
                backend.provider()
            );
            assert!(
                difference < EXACT_DIFFERENCE_DB,
                "{model:?} on {:?}",
                backend.provider()
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files"]
fn daemon_sized_blocks_still_suppress_noise() -> Result<(), BackendError> {
    let speech = NoisySpeech::new();
    for mut backend in working_backends(Model::Standard) {
        let onnx = enhance_onnx(&mut backend, &speech.noisy, DAEMON_BLOCK_FRAMES)?;
        let reduction = speech.noise_reduction_db(&onnx);
        let voice = speech.voice_level_db(&onnx);
        eprintln!(
            "{:?}: gap noise reduction {reduction:.1} dB, voice level {voice:+.1} dB",
            backend.provider()
        );
        assert!(reduction > REQUIRED_REDUCTION_DB && voice.abs() < MAX_VOICE_CHANGE_DB);
    }
    Ok(())
}

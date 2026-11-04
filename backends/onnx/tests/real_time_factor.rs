//! Speed of offline enhancement per execution provider. Needs `$ORT_DYLIB_PATH` and
//! `$DFN_MODELS_DIR` like `offline_parity`; meaningful only in release builds.

use std::path::PathBuf;
use std::time::Instant;

use voice_abi::{Backend, BackendError, FRAME_LEN, Model, SAMPLE_RATE};
use voice_onnx::{OnnxBackend, Provider};

const MODELS_DIR_VARIABLE: &str = "DFN_MODELS_DIR";
const INSTALLED_MODELS_DIR: &str = "/usr/share/crownos/voice/models/deepfilternet3";
/// The daemon's offline block: one second per `process` call.
const BLOCK_SAMPLES: usize = 100 * FRAME_LEN;
/// Covers engine builds for the first few sequence lengths.
const WARMUP_BLOCKS: usize = 3;
const TIMED_SECONDS: usize = 30;

fn network_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR_VARIABLE)
        .map_or_else(|| PathBuf::from(INSTALLED_MODELS_DIR), PathBuf::from)
}

fn noisy_tone(len: usize) -> Vec<f32> {
    let mut state = 0x2545_F491u32;
    (0..len)
        .map(|n| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            0.1 * (n as f32 * 0.05).sin() + (state as f32 / u32::MAX as f32 - 0.5) * 0.02
        })
        .collect()
}

fn real_time_factor(backend: &mut OnnxBackend) -> Result<f64, BackendError> {
    let input = noisy_tone(BLOCK_SAMPLES * (WARMUP_BLOCKS + TIMED_SECONDS));
    let mut output = vec![0.0; BLOCK_SAMPLES];
    let mut blocks = input.chunks_exact(BLOCK_SAMPLES);
    for block in blocks.by_ref().take(WARMUP_BLOCKS) {
        backend.process(block, &mut output)?;
    }
    let started = Instant::now();
    for block in blocks {
        backend.process(block, &mut output)?;
    }
    let audio_seconds = (TIMED_SECONDS * BLOCK_SAMPLES) as f64 / f64::from(SAMPLE_RATE);
    Ok(started.elapsed().as_secs_f64() / audio_seconds)
}

/// Run with `cargo test --release -p crownos-voice-backend-onnx --test real_time_factor -- --ignored --nocapture`.
#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 ONNX files; run in release"]
fn every_working_provider_runs_faster_than_real_time() -> Result<(), BackendError> {
    let mut measured = 0;
    for provider in Provider::detected() {
        let mut backend =
            match OnnxBackend::with_provider(Model::Standard, &network_dir(), provider) {
                Ok(backend) => backend,
                Err(error) => {
                    eprintln!("{provider:?} skipped: {error}");
                    continue;
                }
            };
        let factor = real_time_factor(&mut backend)?;
        eprintln!(
            "{provider:?}: real-time factor {factor:.3} ({:.0}x real time)",
            factor.recip()
        );
        assert!(factor < 1.0, "{provider:?} is slower than real time");
        measured += 1;
    }
    assert!(
        measured > 0,
        "no execution provider works; check ORT_DYLIB_PATH and DFN_MODELS_DIR"
    );
    Ok(())
}

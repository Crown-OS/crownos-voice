mod common;

use std::path::Path;
use std::time::Instant;

use common::NoisySpeech;
use voice_abi::{Backend, BackendError, FRAME_LEN, Model, Workload};
use voice_cpu::CpuBackend;

const WARMUP_FRAMES: usize = 50;
const DEADLINE_MS: f64 = 10.0;

fn milliseconds_per_frame(model: Model) -> Result<f64, BackendError> {
    let mut backend = CpuBackend::create(model, Workload::Realtime, Path::new("/nonexistent"))?;
    let speech = NoisySpeech::new();
    let mut output = vec![0.0; FRAME_LEN];
    let mut frames = speech.noisy.chunks_exact(FRAME_LEN);
    for frame in frames.by_ref().take(WARMUP_FRAMES) {
        backend.process(frame, &mut output)?;
    }
    let timed_frames = frames.len();
    let started = Instant::now();
    for frame in frames {
        backend.process(frame, &mut output)?;
    }
    Ok(started.elapsed().as_secs_f64() * 1e3 / timed_frames as f64)
}

/// Run with `cargo test --release -p crownos-voice-backend-cpu --test timing -- --ignored --nocapture`.
#[test]
#[ignore = "meaningful only in release builds"]
fn every_model_meets_the_realtime_deadline() -> Result<(), BackendError> {
    for model in [Model::Light, Model::Standard, Model::Studio] {
        let per_frame = milliseconds_per_frame(model)?;
        eprintln!("{model:?}: {per_frame:.3} ms per 10 ms frame");
        assert!(
            per_frame < DEADLINE_MS,
            "{model:?} takes {per_frame:.2} ms per frame"
        );
    }
    Ok(())
}

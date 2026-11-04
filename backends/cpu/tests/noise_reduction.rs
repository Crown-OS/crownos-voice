mod common;

use common::{NoisySpeech, enhance};
use voice_abi::{BackendError, Model};

const REQUIRED_REDUCTION_DB: f32 = 10.0;
const MAX_VOICE_LOSS_DB: f32 = -10.0;

fn assert_suppresses_noise(model: Model) -> Result<(), BackendError> {
    let speech = NoisySpeech::new();
    let enhanced = enhance(model, &speech.noisy)?;
    let reduction = speech.noise_reduction_db(&enhanced);
    let voice = speech.voice_level_db(&enhanced);
    eprintln!("{model:?}: {reduction:.1} dB noise reduction in gaps, voice level {voice:+.1} dB");
    assert!(
        reduction >= REQUIRED_REDUCTION_DB,
        "{model:?} reduced gap noise by only {reduction:.1} dB"
    );
    assert!(
        voice > MAX_VOICE_LOSS_DB,
        "{model:?} removed the voice too ({voice:.1} dB)"
    );
    Ok(())
}

#[test]
fn light_suppresses_noise_in_gaps() -> Result<(), BackendError> {
    assert_suppresses_noise(Model::Light)
}

#[test]
fn standard_suppresses_noise_in_gaps() -> Result<(), BackendError> {
    assert_suppresses_noise(Model::Standard)
}

#[test]
fn studio_suppresses_noise_in_gaps() -> Result<(), BackendError> {
    assert_suppresses_noise(Model::Studio)
}

//! Offline jobs through `crown-voiced` on the real CPU and ONNX Runtime plugins. Needs ONNX
//! Runtime (`$ORT_DYLIB_PATH` or `/usr/lib/libonnxruntime.so`) and the DeepFilterNet3 export in
//! `$CROWN_VOICE_MODELS`; run with `cargo test --release --test real_backends -- --ignored`.

mod common;
#[path = "../../backends/onnx/tests/common/mod.rs"]
mod noisy_speech;

use std::path::PathBuf;

use anyhow::{Context, ensure};
use common::pink_noise::pink_noise;
use common::{CPU, ONNX, Plugin, RunningDaemon, backends, enhance};
use noisy_speech::{NoisySpeech, rms};
use voice_engine::{Loudness, TARGET_LUFS};
use voice_protocol::Enhancement;

const MODELS_DIR_VARIABLE: &str = "CROWN_VOICE_MODELS";
const INSTALLED_MODELS_DIR: &str = "/usr/share/crownos/voice/models";
const REQUIRED_SNR_GAIN_DB: f32 = 10.0;
const MAX_SPEECH_LOSS_DB: f32 = 6.0;
const MAX_LOUDNESS_ERROR_LU: f32 = 1.0;
/// -40 dBFS, under the fixture's -26 dBFS voice.
const NOISE_RMS: f32 = 0.01;
/// Reported instead of minus infinity for digital silence.
const SILENCE_DBFS: f32 = -120.0;
const ALL_PROFILES: [Enhancement; 3] = [
    Enhancement::Light,
    Enhancement::NoiseSuppression,
    Enhancement::StudioVoice,
];
/// The ONNX backend runs DeepFilterNet3 only.
const DEEP_FILTER_PROFILES: [Enhancement; 2] =
    [Enhancement::NoiseSuppression, Enhancement::StudioVoice];

fn models_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR_VARIABLE)
        .map_or_else(|| PathBuf::from(INSTALLED_MODELS_DIR), PathBuf::from)
}

fn decibels(level: f32) -> f32 {
    (20.0 * level.log10()).max(SILENCE_DBFS)
}

/// The fixture's syllables over pink instead of white noise.
fn speech_in_pink_noise() -> NoisySpeech {
    let mut speech = NoisySpeech::new();
    speech.noisy = speech
        .clean
        .iter()
        .zip(pink_noise(speech.clean.len(), NOISE_RMS))
        .map(|(voice, noise)| voice + noise)
        .collect();
    speech
}

/// RMS levels in dBFS of the noise-only gaps and of the voiced stretches after the first.
#[derive(Clone, Copy, Debug)]
struct Levels {
    noise: f32,
    speech: f32,
}

impl Levels {
    fn of(speech: &NoisySpeech, signal: &[f32]) -> Self {
        let gathered = |ranges: &[std::ops::Range<usize>]| -> Vec<f32> {
            ranges
                .iter()
                .flat_map(|range| signal[range.clone()].iter().copied())
                .collect()
        };
        Self {
            noise: decibels(rms(&gathered(&speech.gaps()))),
            speech: decibels(rms(&gathered(&speech.voiced[1..]))),
        }
    }

    fn signal_to_noise(self) -> f32 {
        self.speech - self.noise
    }
}

/// Runs every profile in `profiles` on a daemon with only `plugins` installed, checks the output
/// and returns the plugin that served the jobs.
fn assert_cleans(
    test: &str,
    plugins: &[Plugin],
    profiles: &[Enhancement],
) -> anyhow::Result<String> {
    let daemon = RunningDaemon::start(test, plugins, &models_dir())?;
    let mut client = daemon.connect()?;
    let speech = speech_in_pink_noise();
    let before = Levels::of(&speech, &speech.noisy);
    for &enhancement in profiles {
        let output = enhance(&mut client, enhancement, &speech.noisy)?;
        ensure!(
            output.len() == speech.noisy.len(),
            "{enhancement:?} changed the length"
        );
        let after = Levels::of(&speech, &output);
        let snr_gain = after.signal_to_noise() - before.signal_to_noise();
        eprintln!(
            "{test} {enhancement:?}: noise {:.1} -> {:.1} dBFS, speech {:.1} -> {:.1} dBFS, SNR +{snr_gain:.1} dB",
            before.noise, after.noise, before.speech, after.speech
        );
        ensure!(
            snr_gain >= REQUIRED_SNR_GAIN_DB,
            "{enhancement:?} gained only {snr_gain:.1} dB SNR"
        );
        if enhancement == Enhancement::StudioVoice {
            let mut loudness = Loudness::new();
            loudness.push(&output);
            let lufs = loudness.integrated_lufs().context("silent output")?;
            eprintln!("{test} {enhancement:?}: {lufs:.1} LUFS");
            ensure!(
                (lufs - TARGET_LUFS).abs() < MAX_LOUDNESS_ERROR_LU,
                "{lufs:.1} LUFS"
            );
        } else {
            ensure!(
                after.speech > before.speech - MAX_SPEECH_LOSS_DB,
                "{enhancement:?} removed the speech too"
            );
        }
    }
    let served = backends(&mut client)?
        .into_iter()
        .find(|backend| backend.loaded)
        .context("no backend served the jobs")?;
    Ok(served.name)
}

#[test]
#[ignore = "builds the release plugins; see the module docs"]
fn the_cpu_plugin_cleans_every_profile() -> anyhow::Result<()> {
    assert_eq!(assert_cleans("cpu", &[CPU], &ALL_PROFILES)?, "cpu");
    Ok(())
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 export; see the module docs"]
fn the_onnx_plugin_cleans_through_the_daemon() -> anyhow::Result<()> {
    assert_eq!(
        assert_cleans("onnx", &[ONNX], &DEEP_FILTER_PROFILES)?,
        "onnx"
    );
    Ok(())
}

#[test]
#[ignore = "needs ONNX Runtime and the DeepFilterNet3 export; see the module docs"]
fn offline_jobs_go_to_the_best_probed_plugin() -> anyhow::Result<()> {
    let daemon = RunningDaemon::start("best", &[CPU, ONNX], &models_dir())?;
    let mut client = daemon.connect()?;
    let probed = backends(&mut client)?;
    let onnx = probed
        .iter()
        .find(|backend| backend.name == "onnx")
        .context("the onnx plugin was not probed")?;
    eprintln!("probed: {probed:?}");
    assert!(onnx.offline_score > 0, "ONNX Runtime was not found");
    assert_eq!(onnx.realtime_score, 0, "the onnx plugin is offline only");
    let best = probed
        .iter()
        .max_by_key(|backend| backend.offline_score)
        .context("no plugins")?
        .name
        .clone();
    let speech = speech_in_pink_noise();
    enhance(&mut client, Enhancement::NoiseSuppression, &speech.noisy)?;
    let served: Vec<_> = backends(&mut client)?
        .into_iter()
        .filter(|backend| backend.loaded)
        .map(|backend| backend.name)
        .collect();
    assert_eq!(served, [best]);
    Ok(())
}

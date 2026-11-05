//! Starts `crown-voiced` against the passthrough backend and runs an offline job through it.

mod common;

use std::os::fd::OwnedFd;
use std::path::Path;

use anyhow::Context;
use common::{PASSTHROUGH, REPLY_TIMEOUT, RunningDaemon, backends, enhance};
use voice_engine::{HighPass, Loudness, TARGET_LUFS};
use voice_protocol::{Enhancement, SAMPLE_RATE};

/// One second and a partial frame, to exercise padding.
const SAMPLES: usize = 48_123;
const PROBE_CACHE: &str = "cache/crownos/voice/backends.ron";
const NO_MODELS: &str = "/nonexistent/models";

fn start_daemon(test: &str) -> anyhow::Result<RunningDaemon> {
    RunningDaemon::start(test, &[PASSTHROUGH], Path::new(NO_MODELS))
}

fn tone(len: usize) -> Vec<f32> {
    (0..len)
        .map(|index| 0.05 * (index as f32 * 0.05).sin())
        .collect()
}

#[test]
fn enhances_pcm_through_the_passthrough_backend() -> anyhow::Result<()> {
    let daemon = start_daemon("enhance")?;
    let mut client = daemon.connect()?;

    let backends_before = backends(&mut client)?;
    let passthrough = backends_before
        .iter()
        .find(|backend| backend.name == "passthrough")
        .context("not probed")?;
    assert_eq!(
        (passthrough.realtime_score, passthrough.offline_score),
        (1, 1)
    );
    assert!(
        !passthrough.loaded,
        "probing does not keep the library mapped"
    );

    let input = tone(SAMPLES);
    let output = enhance(&mut client, Enhancement::NoiseSuppression, &input)?;
    let mut expected = vec![0.0; input.len()];
    HighPass::new().process(&input, &mut expected);
    assert_eq!(output.len(), input.len());
    assert!(
        output
            .iter()
            .zip(&expected)
            .all(|(got, want)| (got - want).abs() < 1e-6),
        "passthrough output is not the high-passed input, sample for sample"
    );
    assert!(
        daemon.root.join(PROBE_CACHE).is_file(),
        "probe results are cached"
    );

    assert!(
        backends(&mut client)?
            .iter()
            .any(|backend| backend.name == "passthrough" && backend.loaded),
        "kept for the idle TTL"
    );
    Ok(())
}

#[test]
fn studio_voice_is_loudness_normalized() -> anyhow::Result<()> {
    let daemon = start_daemon("studio")?;
    let mut client = daemon.connect()?;
    let input = tone(3 * SAMPLE_RATE as usize);
    let output = enhance(&mut client, Enhancement::StudioVoice, &input)?;
    assert_eq!(output.len(), input.len());
    let mut loudness = Loudness::new();
    loudness.push(&output);
    let lufs = loudness.integrated_lufs().context("silent output")?;
    assert!((lufs - TARGET_LUFS).abs() < 0.5, "{lufs} LUFS");
    Ok(())
}

#[test]
fn rejects_enhancement_off() -> anyhow::Result<()> {
    let daemon = start_daemon("reject-off")?;
    let mut client = daemon.connect()?;
    let (input, _) = std::io::pipe()?;
    let (_, output) = std::io::pipe()?;
    let job = client.enhance_pcm(
        Enhancement::Off,
        OwnedFd::from(input),
        OwnedFd::from(output),
    )?;
    assert!(client.wait(job, REPLY_TIMEOUT).is_err());
    Ok(())
}

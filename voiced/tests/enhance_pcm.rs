//! Starts `crown-voiced` against the passthrough backend and runs an offline job through it.

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use voice_engine::{HighPass, Loudness, TARGET_LUFS};
use voice_protocol::{Enhancement, SAMPLE_RATE, voice};

const PASSTHROUGH_LIBRARY: &str = "libvoice_passthrough.so";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const REPLY_TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));
/// One second and a partial frame, to exercise padding.
const SAMPLES: usize = 48_123;
const PROBE_CACHE: &str = "cache/crownos/voice/backends.ron";

struct RunningDaemon {
    child: Child,
    root: PathBuf,
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn passthrough_library() -> anyhow::Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "-p",
            "crownos-voice-backend-passthrough",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .context("could not run cargo")?;
    ensure!(status.success(), "building the passthrough backend failed");
    let profile_dir = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("test binary has no profile directory")?;
    Ok(profile_dir.join(PASSTHROUGH_LIBRARY))
}

fn start_daemon(test: &str) -> anyhow::Result<RunningDaemon> {
    let root = std::env::temp_dir().join(format!("crown-voiced-{test}-{}", std::process::id()));
    let backends = root.join("backends");
    let runtime = root.join("runtime");
    std::fs::create_dir_all(&backends)?;
    std::fs::create_dir_all(&runtime)?;
    std::fs::copy(passthrough_library()?, backends.join(PASSTHROUGH_LIBRARY))?;
    let child = Command::new(env!("CARGO_BIN_EXE_crown-voiced"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("CROWN_VOICE_BACKENDS", &backends)
        .env("CROWN_VOICE_MODELS", root.join("models"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("RUST_LOG", "warn")
        .env_remove("LISTEN_PID")
        .spawn()?;
    Ok(RunningDaemon { child, root })
}

fn connect(daemon: &RunningDaemon) -> anyhow::Result<voice::Client> {
    let socket_dir = daemon.root.join("runtime/crownos");
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        match voice::Client::connect_in(&socket_dir) {
            Ok(client) => return Ok(client),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => bail!("daemon did not come up: {error}"),
        }
    }
}

fn pcm(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

fn samples(pcm: &[u8]) -> Vec<f32> {
    pcm.as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

fn tone(len: usize) -> Vec<f32> {
    (0..len)
        .map(|index| 0.05 * (index as f32 * 0.05).sin())
        .collect()
}

fn enhance(
    client: &mut voice::Client,
    enhancement: Enhancement,
    input: &[f32],
) -> anyhow::Result<Vec<f32>> {
    let (input_reader, mut input_writer) = std::io::pipe()?;
    let (mut output_reader, output_writer) = std::io::pipe()?;
    let job = client.enhance_pcm(
        enhancement,
        OwnedFd::from(input_reader),
        OwnedFd::from(output_writer),
    )?;
    client.wait(job, REPLY_TIMEOUT)?;
    let writer = std::thread::spawn({
        let input = pcm(input);
        move || input_writer.write_all(&input)
    });
    let mut output = Vec::new();
    output_reader.read_to_end(&mut output)?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("writer panicked"))??;
    Ok(samples(&output))
}

#[test]
fn enhances_pcm_through_the_passthrough_backend() -> anyhow::Result<()> {
    let daemon = start_daemon("enhance")?;
    let mut client = connect(&daemon)?;

    let status = client.status()?;
    let status = client.wait(status, REPLY_TIMEOUT)?;
    let passthrough = status
        .backends
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

    let status = client.status()?;
    let status = client.wait(status, REPLY_TIMEOUT)?;
    assert!(
        status
            .backends
            .iter()
            .any(|backend| backend.name == "passthrough" && backend.loaded),
        "kept for the idle TTL"
    );
    Ok(())
}

#[test]
fn studio_voice_is_loudness_normalized() -> anyhow::Result<()> {
    let daemon = start_daemon("studio")?;
    let mut client = connect(&daemon)?;
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
    let mut client = connect(&daemon)?;
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

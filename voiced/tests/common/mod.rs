//! A `crown-voiced` started on its own runtime, cache and plugin directories, and the client
//! calls the integration tests make against it.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use voice_protocol::{BackendInfo, Enhancement, voice};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
pub const REPLY_TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));
/// `target/debug` holds the `dev` profile; every other profile has a directory of its own name.
const DEV_PROFILE_DIR: &str = "debug";

/// A plugin crate and the library it builds.
#[derive(Clone, Copy, Debug)]
pub struct Plugin {
    pub package: &'static str,
    pub library: &'static str,
}

pub const PASSTHROUGH: Plugin = Plugin {
    package: "crownos-voice-backend-passthrough",
    library: "libvoice_passthrough.so",
};
pub const CPU: Plugin = Plugin {
    package: "crownos-voice-backend-cpu",
    library: "libvoice_cpu.so",
};
pub const ONNX: Plugin = Plugin {
    package: "crownos-voice-backend-onnx",
    library: "libvoice_onnx.so",
};

pub struct RunningDaemon {
    child: Child,
    pub root: PathBuf,
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Builds `plugins` in the profile of this test binary and returns where their libraries are.
fn build_plugins(plugins: &[Plugin]) -> anyhow::Result<PathBuf> {
    let profile_dir = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("test binary has no profile directory")?;
    let profile = profile_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|&name| name != DEV_PROFILE_DIR);
    let mut cargo = Command::new(env!("CARGO"));
    cargo.args(["build", "--quiet"]);
    if let Some(profile) = profile {
        cargo.args(["--profile", profile]);
    }
    for plugin in plugins {
        cargo.args(["-p", plugin.package]);
    }
    let status = cargo
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .context("could not run cargo")?;
    ensure!(status.success(), "building {plugins:?} failed");
    Ok(profile_dir)
}

impl RunningDaemon {
    /// Starts the daemon with only `plugins` installed and the models in `models_dir`.
    pub fn start(test: &str, plugins: &[Plugin], models_dir: &Path) -> anyhow::Result<Self> {
        let root = std::env::temp_dir().join(format!("crown-voiced-{test}-{}", std::process::id()));
        let backends = root.join("backends");
        let runtime = root.join("runtime");
        std::fs::create_dir_all(&backends)?;
        std::fs::create_dir_all(&runtime)?;
        let built = build_plugins(plugins)?;
        for plugin in plugins {
            std::fs::copy(built.join(plugin.library), backends.join(plugin.library))?;
        }
        let child = Command::new(env!("CARGO_BIN_EXE_crown-voiced"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("CROWN_VOICE_BACKENDS", &backends)
            .env("CROWN_VOICE_MODELS", models_dir)
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("RUST_LOG", "warn")
            .env_remove("LISTEN_PID")
            .spawn()?;
        Ok(Self { child, root })
    }

    pub fn connect(&self) -> anyhow::Result<voice::Client> {
        let socket_dir = self.root.join("runtime/crownos");
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match voice::Client::connect_in(&socket_dir) {
                Ok(client) => return Ok(client),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => bail!("daemon did not come up: {error}"),
            }
        }
    }
}

pub fn backends(client: &mut voice::Client) -> anyhow::Result<Vec<BackendInfo>> {
    let status = client.status()?;
    Ok(client.wait(status, REPLY_TIMEOUT)?.backends)
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

/// Runs one offline job over `input` and returns everything it wrote.
pub fn enhance(
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

//! Opens an enhanced microphone and holds it:
//! `cargo run --example open_stream [source] [seconds] [light|standard|studio]`.

use std::time::Duration;

use voice_protocol::{Enhancement, voice};

const DEFAULT_HOLD_SECONDS: u64 = 15;
const REPLY_TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));

fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let source = arguments.next().filter(|name| name != "default");
    let hold = Duration::from_secs(
        arguments
            .next()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(DEFAULT_HOLD_SECONDS),
    );
    let enhancement = match arguments.next().as_deref() {
        Some("light") => Enhancement::Light,
        Some("studio") => Enhancement::StudioVoice,
        _ => Enhancement::NoiseSuppression,
    };
    let mut client = voice::Client::connect()?;
    let node = client.open_stream(source, enhancement)?;
    let node = client.wait(node, REPLY_TIMEOUT)?;
    println!("{node:?}");
    std::thread::sleep(hold);
    let status = client.status()?;
    println!("{:?}", client.wait(status, REPLY_TIMEOUT)?);
    Ok(())
}

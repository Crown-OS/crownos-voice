//! Enhances raw mono `f32` 48 kHz PCM: `cargo run --example enhance_file <light|standard|studio> <in> <out>`.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::time::Duration;

use anyhow::{Context, bail};
use voice_protocol::{Enhancement, voice};

const REPLY_TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));

fn enhancement(name: &str) -> anyhow::Result<Enhancement> {
    Ok(match name {
        "light" => Enhancement::Light,
        "standard" => Enhancement::NoiseSuppression,
        "studio" => Enhancement::StudioVoice,
        other => bail!("unknown enhancement {other}"),
    })
}

fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [name, input, output] = arguments.as_slice() else {
        bail!("usage: enhance_file <light|standard|studio> <in> <out>");
    };
    let mut client = voice::Client::connect()?;
    let (input_reader, mut input_writer) = std::io::pipe()?;
    let (mut output_reader, output_writer) = std::io::pipe()?;
    let job = client.enhance_pcm(
        enhancement(name)?,
        OwnedFd::from(input_reader),
        OwnedFd::from(output_writer),
    )?;
    println!("{:?}", client.wait(job, REPLY_TIMEOUT)?);
    let mut source = File::open(input).context("could not open the input")?;
    let writer = std::thread::spawn(move || std::io::copy(&mut source, &mut input_writer));
    let mut sink = File::create(output).context("could not create the output")?;
    let written = std::io::copy(&mut output_reader, &mut sink)?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("writer panicked"))??;
    println!("wrote {written} bytes");
    Ok(())
}

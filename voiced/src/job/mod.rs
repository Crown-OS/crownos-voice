//! Offline jobs: enhance raw PCM from one pipe into another on a dedicated thread.

mod finishing;
mod latency_aligner;
mod lookahead_normalizer;
mod pcm_sink;

use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crownos_ipc::PeerId;
use voice_abi::host::LoadError;
use voice_abi::{FRAME_LEN, Model, Workload};
use voice_protocol::JobId;

use crate::notice::{Notice, Notifier};
use crate::processor::FrameProcessor;
use crate::registry::Registry;
use crate::util::pcm::{self, SAMPLE_BYTES};
use crate::util::pipe_io::CancellablePipe;
use finishing::Finishing;
use latency_aligner::LatencyAligner;
use pcm_sink::PcmSink;

/// One second of audio per backend call.
const FRAMES_PER_BLOCK: usize = 100;
const BLOCK_SAMPLES: usize = FRAMES_PER_BLOCK * FRAME_LEN;

#[derive(Debug, thiserror::Error)]
enum JobError {
    #[error("no voice backend can run this")]
    NoBackend,
    #[error("pipe: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Backend(#[from] LoadError),
}

/// What `enhance_pcm` asked for.
pub struct JobPlan {
    pub job: JobId,
    pub owner: PeerId,
    pub model: Model,
}

/// A running job; dropping it cancels the job, which then closes its output.
pub struct OfflineJob {
    pub owner: PeerId,
    cancel: Arc<AtomicBool>,
}

impl OfflineJob {
    /// Starts the job thread, which creates the backend instance before reading any audio.
    pub fn spawn(
        plan: JobPlan,
        registry: Arc<Registry>,
        pipes: (OwnedFd, OwnedFd),
        notifier: Notifier,
    ) -> io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&cancel);
        let JobPlan { job, owner, model } = plan;
        std::thread::Builder::new()
            .name(format!("voice-job-{}", job.0))
            .spawn(move || {
                let failure = match run(&registry, model, pipes, &cancelled) {
                    Ok(()) => None,
                    Err(_) if cancelled.load(Ordering::Relaxed) => None,
                    Err(error) => Some(error.to_string()),
                };
                notifier.notify(Notice::JobEnded { job, failure });
            })?;
        Ok(Self { owner, cancel })
    }
}

impl Drop for OfflineJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn run(
    registry: &Registry,
    model: Model,
    (input, output): (OwnedFd, OwnedFd),
    cancel: &AtomicBool,
) -> Result<(), JobError> {
    let (_, processor) = registry
        .choose(model, Workload::Offline)
        .ok_or(JobError::NoBackend)?;
    let input = CancellablePipe::new(File::from(input), cancel)?;
    let output = CancellablePipe::new(File::from(output), cancel)?;
    enhance_pipe(
        processor,
        Finishing::for_model(model),
        input,
        PcmSink::new(output, BLOCK_SAMPLES),
    )
}

fn enhance_pipe(
    mut processor: FrameProcessor,
    mut finishing: Finishing,
    mut input: CancellablePipe,
    mut sink: PcmSink,
) -> Result<(), JobError> {
    let mut aligner = LatencyAligner::new(processor.latency_samples());
    let mut raw = vec![[0; SAMPLE_BYTES]; BLOCK_SAMPLES];
    let mut dry = vec![0.0; BLOCK_SAMPLES];
    let mut wet = vec![0.0; BLOCK_SAMPLES];
    loop {
        let samples = input.read_full(raw.as_flattened_mut())? / SAMPLE_BYTES;
        if samples > 0 {
            let frames_len = samples.next_multiple_of(FRAME_LEN);
            pcm::decode(&raw[..samples], &mut dry[..samples]);
            dry[samples..frames_len].fill(0.0);
            aligner.owe(samples);
            processor.process(&dry[..frames_len], &mut wet[..frames_len])?;
            finishing.write(aligner.align(&wet[..frames_len]), &mut sink)?;
        }
        if samples < BLOCK_SAMPLES {
            break;
        }
    }
    dry.fill(0.0);
    while let Some(len) = aligner.outstanding(BLOCK_SAMPLES) {
        processor.process(&dry[..len], &mut wet[..len])?;
        finishing.write(aligner.align(&wet[..len]), &mut sink)?;
    }
    finishing.finish(&mut sink)?;
    Ok(())
}

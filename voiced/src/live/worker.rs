//! The thread that runs a stream's backend, off the real-time path.

use std::io;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use voice_abi::FRAME_LEN;
use voice_protocol::StreamId;

use super::deadline::DeadlineMonitor;
use super::rings::WorkerRings;
use crate::notice::{Notice, Notifier};
use crate::processor::FrameProcessor;

/// Upper bound on how long the worker sleeps without captured audio.
const IDLE_PARK: Duration = Duration::from_millis(50);

pub struct EnhancementWorker {
    replacements: Sender<FrameProcessor>,
    thread: Option<JoinHandle<()>>,
}

struct WorkerState {
    stream: StreamId,
    processor: FrameProcessor,
    replacements: Receiver<FrameProcessor>,
    monitor: DeadlineMonitor,
    notifier: Notifier,
}

impl EnhancementWorker {
    /// Starts the worker; it waits for [`EnhancementWorker::attach`] before touching audio.
    pub fn spawn(
        stream: StreamId,
        processor: FrameProcessor,
        monitor: DeadlineMonitor,
        notifier: Notifier,
    ) -> io::Result<(Self, Sender<WorkerRings>)> {
        let (replacements, replacement_receiver) = channel();
        let (rings_sender, rings_receiver) = channel();
        let state = WorkerState {
            stream,
            processor,
            replacements: replacement_receiver,
            monitor,
            notifier,
        };
        let thread = std::thread::Builder::new()
            .name(format!("voice-stream-{}", stream.0))
            .spawn(move || {
                if let Ok(rings) = rings_receiver.recv() {
                    state.run(rings);
                }
            })?;
        Ok((
            Self {
                replacements,
                thread: Some(thread),
            },
            rings_sender,
        ))
    }

    pub fn thread(&self) -> Option<Thread> {
        self.thread.as_ref().map(|handle| handle.thread().clone())
    }

    /// Swaps the processor between two frames, without interrupting the stream.
    pub fn replace(&self, processor: FrameProcessor) {
        let _ = self.replacements.send(processor);
    }
}

impl Drop for EnhancementWorker {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl WorkerState {
    fn run(mut self, mut rings: WorkerRings) {
        let mut dry = [0.0; FRAME_LEN];
        let mut wet = [0.0; FRAME_LEN];
        loop {
            if let Some(latest) = self.replacements.try_iter().last() {
                self.processor = latest;
            }
            if self.monitor.is_overloaded(Instant::now()) {
                self.notifier.notify(Notice::DeadlinesMissed(self.stream));
            }
            if rings.wet.slots() < FRAME_LEN || rings.captured.pop_entire_slice(&mut dry).is_err() {
                if rings.captured.is_abandoned() {
                    return;
                }
                std::thread::park_timeout(IDLE_PARK);
                continue;
            }
            if let Err(error) = self.processor.process(&dry, &mut wet) {
                log::warn!("stream {}: backend failed a frame: {error}", self.stream.0);
                wet = dry;
                self.monitor.misses().fetch_add(1, Ordering::Relaxed);
            }
            let _ = rings.wet.push_entire_slice(&wet);
        }
    }
}

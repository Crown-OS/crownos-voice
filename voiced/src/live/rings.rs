//! Lock-free plumbing between the PipeWire callbacks and the enhancement worker.
//!
//! Capture writes every block to two rings, or to neither, so `dry` and `captured`
//! always hold the same sample stream. The worker turns `captured` into `wet`.
//! The output callback emits the oldest dry samples' enhanced copy, or the dry
//! samples themselves when the worker missed the deadline.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::Thread;

use rtrb::{Consumer, Producer, RingBuffer};
use voice_abi::FRAME_LEN;

use crate::util::pcm::{self, SAMPLE_BYTES};

/// Enough for the largest PipeWire quantum (8192) plus the deadline budget.
const RING_SAMPLES: usize = 1 << 15;
/// How many quanta the worker has to deliver a processed frame.
pub const DEADLINE_QUANTA: usize = 2;
/// Nominal latency the enhancement adds.
pub const LATENCY_SAMPLES: u32 = (DEADLINE_QUANTA * FRAME_LEN) as u32;

/// Runs in the capture callback.
pub struct CaptureTap {
    captured: Producer<f32>,
    dry: Producer<f32>,
    worker: Thread,
}

impl CaptureTap {
    pub fn push(&mut self, raw: &[[u8; SAMPLE_BYTES]]) {
        let decoded = || raw.iter().map(|bytes| f32::from_le_bytes(*bytes));
        if let (Ok(captured), Ok(dry)) = (
            self.captured.write_chunk_uninit(raw.len()),
            self.dry.write_chunk_uninit(raw.len()),
        ) {
            captured.fill_from_iter(decoded());
            dry.fill_from_iter(decoded());
        }
        self.worker.unpark();
    }
}

/// The worker's ends: dry frames in, enhanced frames out.
pub struct WorkerRings {
    pub captured: Consumer<f32>,
    pub wet: Producer<f32>,
}

/// Runs in the output callback of the virtual source.
pub struct DeadlineMixer {
    dry: Consumer<f32>,
    wet: Consumer<f32>,
    /// Enhanced samples whose dry twins were already emitted or skipped.
    stale_wet: usize,
    primed: bool,
    misses: Arc<AtomicU32>,
}

impl DeadlineMixer {
    pub fn fill(&mut self, out: &mut [[u8; SAMPLE_BYTES]]) {
        let budget = DEADLINE_QUANTA * out.len().max(FRAME_LEN);
        let buffered = self.dry.slots();
        if !self.primed && buffered < budget {
            out.fill(0.0f32.to_le_bytes());
            return;
        }
        self.primed = true;
        if buffered > budget + 2 * out.len() {
            self.skip_dry(buffered - budget);
        }
        self.discard_stale_wet();
        let taken = out.len().min(self.dry.slots());
        let (emitted, silent) = out.split_at_mut(taken);
        silent.fill(0.0f32.to_le_bytes());
        if !silent.is_empty() {
            self.primed = false;
        }
        let Ok(dry) = self.dry.read_chunk(taken) else {
            return;
        };
        match self.wet.read_chunk(taken) {
            Ok(wet) if self.stale_wet == 0 => {
                copy_chunk(wet.as_slices(), emitted);
                wet.commit_all();
            }
            _ => {
                copy_chunk(dry.as_slices(), emitted);
                self.stale_wet += taken;
                if taken > 0 {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        dry.commit_all();
    }

    fn skip_dry(&mut self, samples: usize) {
        if let Ok(chunk) = self.dry.read_chunk(samples) {
            chunk.commit_all();
            self.stale_wet += samples;
        }
    }

    fn discard_stale_wet(&mut self) {
        let stale = self.stale_wet.min(self.wet.slots());
        if let Ok(chunk) = self.wet.read_chunk(stale) {
            chunk.commit_all();
            self.stale_wet -= stale;
        }
    }
}

fn copy_chunk((first, second): (&[f32], &[f32]), out: &mut [[u8; SAMPLE_BYTES]]) {
    let (head, tail) = out.split_at_mut(first.len());
    pcm::encode(first, head);
    pcm::encode(second, tail);
}

/// Rings for one stream; `worker` is unparked whenever audio is captured.
pub fn stream_rings(
    worker: Thread,
    misses: Arc<AtomicU32>,
) -> (CaptureTap, WorkerRings, DeadlineMixer) {
    let (captured_producer, captured) = RingBuffer::new(RING_SAMPLES);
    let (dry_producer, dry) = RingBuffer::new(RING_SAMPLES);
    let (wet_producer, wet) = RingBuffer::new(RING_SAMPLES);
    (
        CaptureTap {
            captured: captured_producer,
            dry: dry_producer,
            worker,
        },
        WorkerRings {
            captured,
            wet: wet_producer,
        },
        DeadlineMixer {
            dry,
            wet,
            stale_wet: 0,
            primed: false,
            misses,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUANTUM: usize = FRAME_LEN;

    fn raw(value: f32) -> Vec<[u8; SAMPLE_BYTES]> {
        vec![value.to_le_bytes(); QUANTUM]
    }

    fn mixed(mixer: &mut DeadlineMixer) -> f32 {
        let mut out = raw(-1.0);
        mixer.fill(&mut out);
        f32::from_le_bytes(out[0])
    }

    fn enhance_all(rings: &mut WorkerRings) {
        let mut frame = [0.0; FRAME_LEN];
        while rings.captured.pop_entire_slice(&mut frame).is_ok() {
            frame.iter_mut().for_each(|sample| *sample *= 10.0);
            let _ = rings.wet.push_entire_slice(&frame);
        }
    }

    fn rings() -> (CaptureTap, WorkerRings, DeadlineMixer, Arc<AtomicU32>) {
        let misses = Arc::new(AtomicU32::new(0));
        let (tap, worker, mixer) = stream_rings(std::thread::current(), Arc::clone(&misses));
        (tap, worker, mixer, misses)
    }

    #[test]
    fn stays_silent_until_the_deadline_budget_is_buffered() {
        let (mut tap, _worker, mut mixer, misses) = rings();
        tap.push(&raw(1.0));
        assert_eq!(mixed(&mut mixer), 0.0);
        assert_eq!(misses.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn emits_enhanced_audio_when_the_worker_keeps_up() {
        let (mut tap, mut worker, mut mixer, misses) = rings();
        for block in 1..=DEADLINE_QUANTA {
            tap.push(&raw(block as f32));
        }
        enhance_all(&mut worker);
        assert_eq!(mixed(&mut mixer), 10.0);
        assert_eq!(misses.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_missed_deadline_emits_dry_audio_and_drops_the_late_frame() {
        let (mut tap, mut worker, mut mixer, misses) = rings();
        tap.push(&raw(1.0));
        tap.push(&raw(2.0));
        assert_eq!(mixed(&mut mixer), 1.0, "nothing enhanced yet: dry");
        assert_eq!(misses.load(Ordering::Relaxed), 1);
        tap.push(&raw(3.0));
        enhance_all(&mut worker);
        assert_eq!(
            mixed(&mut mixer),
            20.0,
            "the late enhanced copy of 1.0 is skipped"
        );
        assert_eq!(mixed(&mut mixer), 30.0);
        assert_eq!(misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn caps_latency_after_the_output_stalls() {
        let (mut tap, mut worker, mut mixer, _misses) = rings();
        for block in 1..=10 {
            tap.push(&raw(block as f32));
        }
        enhance_all(&mut worker);
        assert_eq!(
            mixed(&mut mixer),
            90.0,
            "jumps to the newest budget's worth of audio"
        );
    }
}

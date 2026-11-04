use std::io;

use crate::util::pcm::{self, SAMPLE_BYTES};
use crate::util::pipe_io::CancellablePipe;

/// Where finished samples go.
pub trait SampleSink {
    fn write(&mut self, samples: &[f32]) -> io::Result<()>;
}

/// Encodes samples into a pipe through a fixed scratch buffer.
pub struct PcmSink<'a> {
    pipe: CancellablePipe<'a>,
    raw: Vec<[u8; SAMPLE_BYTES]>,
}

impl<'a> PcmSink<'a> {
    pub fn new(pipe: CancellablePipe<'a>, scratch_samples: usize) -> Self {
        Self {
            pipe,
            raw: vec![[0; SAMPLE_BYTES]; scratch_samples],
        }
    }
}

impl SampleSink for PcmSink<'_> {
    fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        for chunk in samples.chunks(self.raw.len()) {
            let raw = &mut self.raw[..chunk.len()];
            pcm::encode(chunk, raw);
            self.pipe.write_all(raw.as_flattened())?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl SampleSink for Vec<f32> {
    fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        self.extend_from_slice(samples);
        Ok(())
    }
}

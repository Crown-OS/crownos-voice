use voice_abi::FRAME_LEN;

/// Regroups arbitrary-sized chunks into whole [`FRAME_LEN`] frames without allocating.
///
/// Whole frames inside a chunk are handed out in place; only a frame straddling two chunks is
/// copied through the internal buffer.
#[derive(Clone, Debug)]
pub struct FrameAccumulator {
    pending: [f32; FRAME_LEN],
    filled: usize,
}

impl FrameAccumulator {
    pub const fn new() -> Self {
        Self {
            pending: [0.0; FRAME_LEN],
            filled: 0,
        }
    }

    /// Samples waiting for the rest of their frame.
    pub const fn pending(&self) -> usize {
        self.filled
    }

    pub fn clear(&mut self) {
        self.filled = 0;
    }

    /// Appends `chunk` and calls `on_frame` for every frame it completes, in order.
    pub fn push(&mut self, mut chunk: &[f32], mut on_frame: impl FnMut(&[f32; FRAME_LEN])) {
        if self.filled > 0 {
            let taken = chunk.len().min(FRAME_LEN - self.filled);
            let (head, tail) = chunk.split_at(taken);
            self.pending[self.filled..self.filled + taken].copy_from_slice(head);
            self.filled += taken;
            chunk = tail;
            if self.filled < FRAME_LEN {
                return;
            }
            on_frame(&self.pending);
        }
        let (frames, rest) = chunk.as_chunks::<FRAME_LEN>();
        frames.iter().for_each(&mut on_frame);
        self.pending[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }
}

impl Default for FrameAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regroups_uneven_chunks_in_order() {
        let signal: Vec<f32> = (0..FRAME_LEN * 5 + 17).map(|n| n as f32).collect();
        let mut accumulator = FrameAccumulator::new();
        let mut frames = Vec::new();
        for chunk in signal.chunks(173) {
            accumulator.push(chunk, |frame| frames.extend_from_slice(frame));
        }
        assert_eq!(frames, signal[..FRAME_LEN * 5]);
        assert_eq!(accumulator.pending(), 17);
    }

    #[test]
    fn passes_aligned_frames_through() {
        let signal = vec![1.0; FRAME_LEN * 3];
        let mut count = 0;
        FrameAccumulator::new().push(&signal, |_| count += 1);
        assert_eq!(count, 3);
    }
}

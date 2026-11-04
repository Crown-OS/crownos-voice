use voice_abi::FRAME_LEN;

/// Splits whole [`FRAME_LEN`] frames back into arbitrary-sized chunks without allocating.
///
/// The output-side twin of [`crate::FrameAccumulator`]: frames that fit the requested chunk are
/// written straight into it, only a frame straddling two chunks is kept in the internal buffer.
#[derive(Clone, Debug)]
pub struct FrameDispenser {
    current: [f32; FRAME_LEN],
    consumed: usize,
}

impl FrameDispenser {
    pub const fn new() -> Self {
        Self {
            current: [0.0; FRAME_LEN],
            consumed: FRAME_LEN,
        }
    }

    /// Samples of the current frame not yet handed out.
    pub const fn buffered(&self) -> usize {
        FRAME_LEN - self.consumed
    }

    pub fn clear(&mut self) {
        self.consumed = FRAME_LEN;
    }

    /// Fills `out`, asking `next_frame` to write a whole frame whenever the buffer runs dry.
    ///
    /// Stops early when `next_frame` returns `false`; returns how many samples of `out` were
    /// written.
    pub fn pull(
        &mut self,
        out: &mut [f32],
        mut next_frame: impl FnMut(&mut [f32; FRAME_LEN]) -> bool,
    ) -> usize {
        let mut written = 0;
        while written < out.len() {
            if self.consumed == FRAME_LEN {
                if let Some(frame) = out[written..].first_chunk_mut::<FRAME_LEN>() {
                    if !next_frame(frame) {
                        break;
                    }
                    written += FRAME_LEN;
                    continue;
                }
                if !next_frame(&mut self.current) {
                    break;
                }
                self.consumed = 0;
            }
            let taken = (out.len() - written).min(FRAME_LEN - self.consumed);
            out[written..written + taken]
                .copy_from_slice(&self.current[self.consumed..self.consumed + taken]);
            self.consumed += taken;
            written += taken;
        }
        written
    }
}

impl Default for FrameDispenser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counting_source() -> impl FnMut(&mut [f32; FRAME_LEN]) -> bool {
        let mut next = 0.0;
        move |frame| {
            frame.iter_mut().for_each(|sample| {
                *sample = next;
                next += 1.0;
            });
            true
        }
    }

    #[test]
    fn splits_frames_into_uneven_chunks() {
        let mut dispenser = FrameDispenser::new();
        let mut source = counting_source();
        let mut collected = Vec::new();
        for len in [100, 700, 480, 1, 959] {
            let mut chunk = vec![0.0; len];
            assert_eq!(dispenser.pull(&mut chunk, &mut source), len);
            collected.extend(chunk);
        }
        let expected: Vec<f32> = (0..collected.len()).map(|n| n as f32).collect();
        assert_eq!(collected, expected);
        assert_eq!(dispenser.buffered(), 5 * FRAME_LEN - collected.len());
    }

    #[test]
    fn stops_when_frames_run_out() {
        let mut dispenser = FrameDispenser::new();
        let mut remaining = 1;
        let mut out = vec![0.0; FRAME_LEN * 2];
        let written = dispenser.pull(&mut out, |frame| {
            frame.fill(1.0);
            remaining -= 1;
            remaining >= 0
        });
        assert_eq!(written, FRAME_LEN);
    }
}

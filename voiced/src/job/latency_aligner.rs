use voice_abi::FRAME_LEN;

/// Lines enhanced audio up with its input, sample for sample.
///
/// Drops the processor's leading latency and anything produced from padding past the input's end.
pub struct LatencyAligner {
    leading: usize,
    owed: usize,
}

impl LatencyAligner {
    pub fn new(latency_samples: usize) -> Self {
        Self {
            leading: latency_samples,
            owed: 0,
        }
    }

    /// Records `samples` more input samples, each owed one output sample.
    pub fn owe(&mut self, samples: usize) {
        self.owed += samples;
    }

    /// The part of the next processed block that belongs in the output.
    pub fn align<'a>(&mut self, processed: &'a [f32]) -> &'a [f32] {
        let dropped = self.leading.min(processed.len());
        self.leading -= dropped;
        let kept = (processed.len() - dropped).min(self.owed);
        self.owed -= kept;
        &processed[dropped..dropped + kept]
    }

    /// Whole frames of trailing silence still to process once the input ended, at most `limit`.
    pub fn outstanding(&self, limit: usize) -> Option<usize> {
        (self.owed > 0).then(|| {
            (self.leading + self.owed)
                .next_multiple_of(FRAME_LEN)
                .min(limit)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATENCY: usize = 1440;

    fn delayed(input: &[f32]) -> Vec<f32> {
        std::iter::repeat_n(0.0, LATENCY)
            .chain(input.iter().copied())
            .collect()
    }

    #[test]
    fn removes_the_latency_and_the_padding() {
        let input: Vec<f32> = (1..=1000).map(|n| n as f32).collect();
        let padded_len = input.len().next_multiple_of(FRAME_LEN);
        let mut stream = delayed(&input);
        let mut aligner = LatencyAligner::new(LATENCY);
        aligner.owe(input.len());
        let mut output = aligner.align(&stream[..padded_len]).to_vec();
        let mut processed = padded_len;
        while let Some(len) = aligner.outstanding(FRAME_LEN) {
            stream.resize(stream.len().max(processed + len), 0.0);
            output.extend_from_slice(aligner.align(&stream[processed..processed + len]));
            processed += len;
        }
        assert_eq!(output, input);
        assert!(processed.is_multiple_of(FRAME_LEN));
    }

    #[test]
    fn without_latency_only_padding_is_dropped() {
        let mut aligner = LatencyAligner::new(0);
        aligner.owe(3);
        assert_eq!(aligner.align(&[1.0, 2.0, 3.0, 0.0, 0.0]), [1.0, 2.0, 3.0]);
        assert_eq!(aligner.outstanding(FRAME_LEN), None);
    }
}

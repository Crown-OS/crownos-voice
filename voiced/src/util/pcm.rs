//! Raw `f32` little-endian PCM, as carried by PipeWire buffers and offline pipes.

pub const SAMPLE_BYTES: usize = size_of::<f32>();

pub fn decode(raw: &[[u8; SAMPLE_BYTES]], samples: &mut [f32]) {
    samples
        .iter_mut()
        .zip(raw)
        .for_each(|(sample, bytes)| *sample = f32::from_le_bytes(*bytes));
}

pub fn encode(samples: &[f32], raw: &mut [[u8; SAMPLE_BYTES]]) {
    raw.iter_mut()
        .zip(samples)
        .for_each(|(bytes, sample)| *bytes = sample.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let samples = [0.5, -1.0, 0.25];
        let mut raw = [[0; SAMPLE_BYTES]; 3];
        encode(&samples, &mut raw);
        let mut decoded = [0.0; 3];
        decode(&raw, &mut decoded);
        assert_eq!(samples, decoded);
    }
}

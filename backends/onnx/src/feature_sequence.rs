use voice_engine::dfn::{Complex32, DF_BINS, ERB_BANDS, Features};

/// Encoder features of consecutive hops, laid out as the encoder's input tensors.
#[derive(Default)]
pub(crate) struct FeatureSequence {
    erb_frames: Vec<[f32; ERB_BANDS]>,
    spectrum_frames: Vec<[Complex32; DF_BINS]>,
    feat_spec: Vec<f32>,
}

impl FeatureSequence {
    pub(crate) fn push(&mut self, features: &Features<'_>) {
        self.erb_frames.push(*features.erb);
        self.spectrum_frames.push(*features.complex);
    }

    pub(crate) fn len(&self) -> usize {
        self.spectrum_frames.len()
    }

    /// `feat_erb [steps][ERB_BANDS]` and `feat_spec [re|im][steps][DF_BINS]`, zero past the
    /// pushed frames. `steps` must be at least [`Self::len`].
    pub(crate) fn tensors(&mut self, steps: usize) -> (&[f32], &[f32]) {
        self.erb_frames.resize(steps, [0.0; ERB_BANDS]);
        self.feat_spec.clear();
        self.feat_spec.resize(2 * steps * DF_BINS, 0.0);
        let (real, imaginary) = self.feat_spec.split_at_mut(steps * DF_BINS);
        let planes = real
            .as_chunks_mut::<DF_BINS>()
            .0
            .iter_mut()
            .zip(imaginary.as_chunks_mut::<DF_BINS>().0);
        for (frame, (real, imaginary)) in self.spectrum_frames.iter().zip(planes) {
            for (bin, (re, im)) in frame.iter().zip(real.iter_mut().zip(imaginary)) {
                (*re, *im) = (bin.re, bin.im);
            }
        }
        (self.erb_frames.as_flattened(), &self.feat_spec)
    }

    /// Drops the padding and all but the newest `frames` frames.
    pub(crate) fn keep_newest(&mut self, frames: usize) {
        let pushed = self.spectrum_frames.len();
        let stale = pushed.saturating_sub(frames);
        self.erb_frames.truncate(pushed);
        self.erb_frames.drain(..stale);
        self.spectrum_frames.drain(..stale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_frame(sequence: &mut FeatureSequence, value: f32) {
        let erb = [value; ERB_BANDS];
        let complex = [Complex32::new(value, -value); DF_BINS];
        sequence.push(&Features {
            erb: &erb,
            complex: &complex,
        });
    }

    #[test]
    fn tensors_are_time_major_erb_and_planar_spectra() {
        let mut sequence = FeatureSequence::default();
        push_frame(&mut sequence, 1.0);
        push_frame(&mut sequence, 2.0);
        let (feat_erb, feat_spec) = sequence.tensors(3);
        assert_eq!(feat_erb.len(), 3 * ERB_BANDS);
        assert_eq!(feat_erb[ERB_BANDS], 2.0);
        assert!(feat_erb[2 * ERB_BANDS..].iter().all(|value| *value == 0.0));
        let (real, imaginary) = feat_spec.split_at(3 * DF_BINS);
        assert_eq!((real[0], real[DF_BINS], real[2 * DF_BINS]), (1.0, 2.0, 0.0));
        assert_eq!((imaginary[0], imaginary[DF_BINS]), (-1.0, -2.0));
    }

    #[test]
    fn keep_newest_drops_padding_and_old_frames() {
        let mut sequence = FeatureSequence::default();
        (0..5).for_each(|frame| push_frame(&mut sequence, frame as f32));
        let _ = sequence.tensors(8);
        sequence.keep_newest(2);
        assert_eq!(sequence.len(), 2);
        let (feat_erb, feat_spec) = sequence.tensors(2);
        assert_eq!((feat_erb[0], feat_erb[ERB_BANDS]), (3.0, 4.0));
        assert_eq!(feat_spec[DF_BINS], 4.0);
    }
}

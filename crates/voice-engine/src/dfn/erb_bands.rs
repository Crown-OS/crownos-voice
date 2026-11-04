use voice_abi::SAMPLE_RATE;

use super::{Complex32, ERB_BANDS, FFT_SIZE, FREQ_BINS, MIN_BINS_PER_ERB_BAND};

const ERB_SCALE: f32 = 9.265;
const ERB_MIN_BANDWIDTH_HZ: f32 = 24.7;

fn hz_to_erb(hz: f32) -> f32 {
    ERB_SCALE * (hz / (ERB_MIN_BANDWIDTH_HZ * ERB_SCALE)).ln_1p()
}

fn erb_to_hz(erb: f32) -> f32 {
    ERB_MIN_BANDWIDTH_HZ * ERB_SCALE * ((erb / ERB_SCALE).exp() - 1.0)
}

/// The ERB filterbank of DeepFilterNet: [`ERB_BANDS`] rectangular bands covering every FFT bin.
#[derive(Clone, Debug)]
pub(crate) struct ErbBands {
    widths: [usize; ERB_BANDS],
}

impl ErbBands {
    /// Same rounding as libDF's `erb_fb`, so features match what the model was trained on.
    pub(crate) fn new() -> Self {
        let bin_width_hz = SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let erb_step = hz_to_erb(SAMPLE_RATE as f32 / 2.0) / ERB_BANDS as f32;
        let min_width = MIN_BINS_PER_ERB_BAND as isize;
        let mut widths = [0; ERB_BANDS];
        let (mut previous_edge, mut borrowed) = (0isize, 0isize);
        for (band, width) in widths.iter_mut().enumerate() {
            let edge = (erb_to_hz((band + 1) as f32 * erb_step) / bin_width_hz).round() as isize;
            let natural = edge - previous_edge - borrowed;
            borrowed = (min_width - natural).max(0);
            *width = natural.max(min_width) as usize;
            previous_edge = edge;
        }
        let below_last: usize = widths[..ERB_BANDS - 1].iter().sum();
        widths[ERB_BANDS - 1] = FREQ_BINS - below_last;
        Self { widths }
    }

    fn bands<'a, T>(&'a self, mut bins: &'a [T]) -> impl Iterator<Item = &'a [T]> {
        self.widths.iter().map(move |&width| {
            let (band, rest) = bins.split_at(width);
            bins = rest;
            band
        })
    }

    /// Mean power of each band.
    pub(crate) fn band_powers(
        &self,
        spectrum: &[Complex32; FREQ_BINS],
        powers: &mut [f32; ERB_BANDS],
    ) {
        for (power, band) in powers.iter_mut().zip(self.bands(spectrum)) {
            *power = band.iter().map(Complex32::norm_sqr).sum::<f32>() / band.len() as f32;
        }
    }

    /// Multiplies every bin by the gain of its band.
    pub(crate) fn apply_gains(
        &self,
        spectrum: &mut [Complex32; FREQ_BINS],
        gains: &[f32; ERB_BANDS],
    ) {
        let mut bins = spectrum.as_mut_slice();
        for (&width, &gain) in self.widths.iter().zip(gains) {
            let (band, rest) = bins.split_at_mut(width);
            band.iter_mut().for_each(|bin| *bin *= gain);
            bins = rest;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_cover_every_bin_with_libdf_widths() {
        let bands = ErbBands::new();
        assert_eq!(bands.widths.iter().sum::<usize>(), FREQ_BINS);
        assert_eq!(bands.widths[..4], [2, 2, 2, 2]);
        assert!(
            bands
                .widths
                .iter()
                .all(|&width| width >= MIN_BINS_PER_ERB_BAND)
        );
    }
}

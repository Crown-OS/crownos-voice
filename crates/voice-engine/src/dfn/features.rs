use super::erb_bands::ErbBands;
use super::stft::StftAnalysis;
use super::{Complex32, DF_BINS, ERB_BANDS, FREQ_BINS, HOP_SIZE, NORM_ALPHA};

const ERB_MEAN_INIT_DB: [f32; 2] = [-60.0, -90.0];
const DF_UNIT_INIT: [f32; 2] = [0.001, 0.0001];
const ERB_FEATURE_SCALE: f32 = 40.0;
const POWER_FLOOR: f32 = 1e-10;

/// The encoder inputs of one frame.
#[derive(Clone, Copy, Debug)]
pub struct Features<'a> {
    /// `feat_erb[.., t, ..]`: mean-normalised ERB band energies in dB / 40.
    pub erb: &'a [f32; ERB_BANDS],
    /// `feat_spec[.., re|im, t, ..]`: unit-normalised complex spectrum of the lowest bins.
    pub complex: &'a [Complex32; DF_BINS],
}

/// Turns hops of noisy audio into DeepFilterNet3 encoder features, exactly like libDF's
/// `DFState::feat_erb` and `DFState::feat_cplx`.
pub struct FeatureExtractor {
    analysis: StftAnalysis,
    bands: ErbBands,
    spectrum: [Complex32; FREQ_BINS],
    erb: [f32; ERB_BANDS],
    complex: [Complex32; DF_BINS],
    erb_mean: [f32; ERB_BANDS],
    df_unit: [f32; DF_BINS],
}

impl FeatureExtractor {
    pub fn new() -> Self {
        Self {
            analysis: StftAnalysis::new(),
            bands: ErbBands::new(),
            spectrum: [Complex32::new(0.0, 0.0); FREQ_BINS],
            erb: [0.0; ERB_BANDS],
            complex: [Complex32::new(0.0, 0.0); DF_BINS],
            erb_mean: linear_ramp(ERB_MEAN_INIT_DB),
            df_unit: linear_ramp(DF_UNIT_INIT),
        }
    }

    /// Features of the next hop; must be fed every hop of the stream, in order.
    pub fn extract(&mut self, frame: &[f32; HOP_SIZE]) -> Features<'_> {
        self.analysis.analyze(frame, &mut self.spectrum);
        self.bands.band_powers(&self.spectrum, &mut self.erb);
        for (feature, mean) in self.erb.iter_mut().zip(&mut self.erb_mean) {
            let db = 10.0 * (*feature + POWER_FLOOR).log10();
            *mean = db * (1.0 - NORM_ALPHA) + *mean * NORM_ALPHA;
            *feature = (db - *mean) / ERB_FEATURE_SCALE;
        }
        for ((feature, bin), unit) in self
            .complex
            .iter_mut()
            .zip(&self.spectrum)
            .zip(&mut self.df_unit)
        {
            *unit = bin.norm() * (1.0 - NORM_ALPHA) + *unit * NORM_ALPHA;
            *feature = bin / unit.sqrt();
        }
        Features {
            erb: &self.erb,
            complex: &self.complex,
        }
    }
}

impl Default for FeatureExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn linear_ramp<const N: usize>([first, last]: [f32; 2]) -> [f32; N] {
    let step = (last - first) / (N - 1) as f32;
    std::array::from_fn(|index| first + index as f32 * step)
}

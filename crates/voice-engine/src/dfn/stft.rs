use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use super::{Complex32, FFT_SIZE, FREQ_BINS, HOP_SIZE};

const OVERLAP: usize = FFT_SIZE - HOP_SIZE;
const ZERO: Complex32 = Complex32::new(0.0, 0.0);

/// The Vorbis power-complementary window DeepFilterNet was trained with.
fn vorbis_window() -> Box<[f32]> {
    let half = (FFT_SIZE / 2) as f64;
    (0..FFT_SIZE)
        .map(|index| {
            let inner = (FRAC_PI_2 * (index as f64 + 0.5) / half).sin();
            (FRAC_PI_2 * inner * inner).sin() as f32
        })
        .collect()
}

/// Windowed real FFT of the last [`FFT_SIZE`] samples, one hop at a time, scaled like libDF.
pub(crate) struct StftAnalysis {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Box<[f32]>,
    previous: [f32; OVERLAP],
    windowed: Vec<f32>,
    scratch: Vec<Complex32>,
}

impl StftAnalysis {
    pub(crate) fn new() -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(FFT_SIZE);
        Self {
            windowed: fft.make_input_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window: vorbis_window(),
            previous: [0.0; OVERLAP],
        }
    }

    pub(crate) fn analyze(
        &mut self,
        frame: &[f32; HOP_SIZE],
        spectrum: &mut [Complex32; FREQ_BINS],
    ) {
        let history = self.previous.iter().chain(frame);
        for ((windowed, sample), weight) in self.windowed.iter_mut().zip(history).zip(&self.window)
        {
            *windowed = sample * weight;
        }
        self.previous.copy_from_slice(&frame[HOP_SIZE - OVERLAP..]);
        if self
            .fft
            .process_with_scratch(&mut self.windowed, spectrum, &mut self.scratch)
            .is_err()
        {
            spectrum.fill(ZERO);
        }
        let normalisation = 1.0 / FFT_SIZE as f32;
        spectrum.iter_mut().for_each(|bin| *bin *= normalisation);
    }
}

/// Inverse of [`StftAnalysis`]: inverse FFT, synthesis window and overlap-add.
pub(crate) struct StftSynthesis {
    ifft: Arc<dyn ComplexToReal<f32>>,
    window: Box<[f32]>,
    overlap: [f32; OVERLAP],
    signal: Vec<f32>,
    scratch: Vec<Complex32>,
}

impl StftSynthesis {
    pub(crate) fn new() -> Self {
        let ifft = RealFftPlanner::new().plan_fft_inverse(FFT_SIZE);
        Self {
            signal: ifft.make_output_vec(),
            scratch: ifft.make_scratch_vec(),
            ifft,
            window: vorbis_window(),
            overlap: [0.0; OVERLAP],
        }
    }

    /// Consumes `spectrum` (it is used as FFT scratch) and writes the next hop of audio.
    pub(crate) fn synthesize(
        &mut self,
        spectrum: &mut [Complex32; FREQ_BINS],
        frame: &mut [f32; HOP_SIZE],
    ) {
        spectrum[0].im = 0.0;
        spectrum[FREQ_BINS - 1].im = 0.0;
        if self
            .ifft
            .process_with_scratch(spectrum, &mut self.signal, &mut self.scratch)
            .is_err()
        {
            self.signal.fill(0.0);
        }
        self.signal
            .iter_mut()
            .zip(&self.window)
            .for_each(|(sample, weight)| *sample *= weight);
        let (head, tail) = self.signal.split_at(HOP_SIZE);
        for ((out, sample), overlap) in frame.iter_mut().zip(head).zip(&self.overlap) {
            *out = sample + overlap;
        }
        self.overlap.copy_from_slice(&tail[tail.len() - OVERLAP..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_then_synthesis_is_a_one_hop_delay() {
        let signal: Vec<f32> = (0..HOP_SIZE * 6)
            .map(|n| (n as f32 * 0.05).sin() * 0.3)
            .collect();
        let (mut analysis, mut synthesis) = (StftAnalysis::new(), StftSynthesis::new());
        let mut spectrum = [ZERO; FREQ_BINS];
        let mut output = Vec::new();
        for frame in signal.as_chunks::<HOP_SIZE>().0 {
            let mut out = [0.0; HOP_SIZE];
            analysis.analyze(frame, &mut spectrum);
            synthesis.synthesize(&mut spectrum, &mut out);
            output.extend(out);
        }
        let error = signal
            .iter()
            .zip(&output[OVERLAP..])
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(error < 1e-5, "{error}");
    }
}

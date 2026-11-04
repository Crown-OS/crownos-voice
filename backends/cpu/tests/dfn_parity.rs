//! Checks `voice_engine::dfn` against libDF (vendored as `deep-filter-rt`), and that the offline
//! recipe for ONNX Runtime (whole or chunked sequences through the stateless official export)
//! reproduces the streaming tract model.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{NoisySpeech, seconds};
use deep_filter_rt::DFState;
use deep_filter_rt::tract::DfTract;
use ndarray::{ArrayView2, ArrayViewMut2};
use tract_onnx::prelude::*;
use tract_onnx::tract_hir::internal::format_err;
use tract_onnx::tract_hir::shapefactoid;
use voice_abi::SAMPLE_RATE;
use voice_engine::dfn::{
    CONV_CHANNELS, Complex32, DF_BINS, DF_COEFS_LEN, DF_DECODER_FILE, EMBEDDING_LEN, ENCODER_FILE,
    ERB_BANDS, ERB_DECODER_FILE, Enhancer, FFT_SIZE, FREQ_BINS, FeatureExtractor, HOP_SIZE,
    MIN_BINS_PER_ERB_BAND, NORM_ALPHA, NetworkOutput,
};

const MODELS_DIR_VARIABLE: &str = "DFN_MODELS_DIR";
const INSTALLED_MODELS_DIR: &str = "/usr/share/crownos/voice/models/deepfilternet3";
const CHUNK_FRAMES: usize = 100;
const CONTEXT_FRAMES: usize = 50;
const SETTLE_SECONDS: f32 = 1.0;
const EXACT_DIFFERENCE_DB: f32 = -25.0;
const REQUIRED_REDUCTION_DB: f32 = 10.0;
const MAX_VOICE_CHANGE_DB: f32 = 3.0;

#[test]
fn features_match_libdf() {
    let mut reference = DFState::new(
        SAMPLE_RATE as usize,
        FFT_SIZE,
        HOP_SIZE,
        ERB_BANDS,
        MIN_BINS_PER_ERB_BAND,
    );
    reference.init_norm_states(DF_BINS);
    let mut extractor = FeatureExtractor::new();
    let (mut spectrum, mut erb, mut complex) = (
        vec![Complex32::default(); FREQ_BINS],
        vec![0.0; ERB_BANDS],
        vec![Complex32::default(); DF_BINS],
    );
    let speech = NoisySpeech::new();
    for frame in speech.noisy.as_chunks::<HOP_SIZE>().0.iter().take(200) {
        reference.analysis(frame, &mut spectrum);
        reference.feat_erb(&spectrum, NORM_ALPHA, &mut erb);
        reference.feat_cplx(&spectrum[..DF_BINS], NORM_ALPHA, &mut complex);
        let features = extractor.extract(frame);
        let erb_error = features
            .erb
            .iter()
            .zip(&erb)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        let complex_error = features
            .complex
            .iter()
            .zip(&complex)
            .map(|(a, b)| (a - b).norm())
            .fold(0f32, f32::max);
        assert!(
            erb_error < 1e-4 && complex_error < 1e-4,
            "{erb_error} {complex_error}"
        );
    }
}

/// An input of one network: `[1, channels, S, width]` or, without channels, `[1, S, width]`.
#[derive(Clone, Copy)]
struct Input {
    channels: Option<usize>,
    width: usize,
}

const fn planes(channels: usize, width: usize) -> Input {
    Input {
        channels: Some(channels),
        width,
    }
}

const fn rows(width: usize) -> Input {
    Input {
        channels: None,
        width,
    }
}

type Network = Arc<TypedRunnableModel>;

fn load(path: &Path, inputs: &[Input]) -> TractResult<Network> {
    let mut model = tract_onnx::onnx()
        .with_ignore_output_shapes(true)
        .model_for_path(path)?;
    let steps = model.symbols.sym("S");
    for (index, input) in inputs.iter().enumerate() {
        let (time, width) = (steps.clone(), input.width);
        let fact = match input.channels {
            Some(channels) => shapefactoid!(1, channels, time, width),
            None => shapefactoid!(1, time, width),
        };
        model = model.with_input_fact(index, InferenceFact::dt_shape(f32::datum_type(), fact))?;
    }
    model.into_optimized()?.into_runnable()
}

struct Networks {
    encoder: Network,
    erb_decoder: Network,
    df_decoder: Network,
}

/// Per-step network outputs of one sequence.
struct Estimates {
    lsnr: Vec<f32>,
    erb_gains: Vec<f32>,
    df_coefs: Vec<f32>,
}

impl Networks {
    fn load(dir: &Path) -> TractResult<Self> {
        let skip = |width| planes(CONV_CHANNELS, width);
        Ok(Self {
            encoder: load(
                &dir.join(ENCODER_FILE),
                &[planes(1, ERB_BANDS), planes(2, DF_BINS)],
            )?,
            erb_decoder: load(
                &dir.join(ERB_DECODER_FILE),
                &[
                    rows(EMBEDDING_LEN),
                    skip(ERB_BANDS / 4),
                    skip(ERB_BANDS / 4),
                    skip(ERB_BANDS / 2),
                    skip(ERB_BANDS),
                ],
            )?,
            df_decoder: load(
                &dir.join(DF_DECODER_FILE),
                &[rows(EMBEDDING_LEN), skip(DF_BINS)],
            )?,
        })
    }

    /// Runs one stateless sequence, as ONNX Runtime would: `feat_erb [1,1,S,32]`, `feat_spec [1,2,S,96]`.
    fn run(
        &self,
        erb: &[[f32; ERB_BANDS]],
        complex: &[[Complex32; DF_BINS]],
    ) -> TractResult<Estimates> {
        let steps = erb.len();
        let feat_erb = Tensor::from_shape(&[1, 1, steps, ERB_BANDS], erb.as_flattened())?;
        let planar: Vec<f32> = [|bin: &Complex32| bin.re, |bin: &Complex32| bin.im]
            .iter()
            .flat_map(|part| complex.iter().flatten().map(part))
            .collect();
        let feat_spec = Tensor::from_shape(&[1, 2, steps, DF_BINS], &planar)?;
        let [e0, e1, e2, e3, emb, c0, lsnr]: [TValue; 7] = self
            .encoder
            .run(tvec!(feat_erb.into(), feat_spec.into()))?
            .into_iter()
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| format_err!("the encoder has 7 outputs"))?;
        let gains = self
            .erb_decoder
            .run(tvec!(emb.clone(), e3, e2, e1, e0))?
            .remove(0);
        let coefs = self.df_decoder.run(tvec!(emb, c0))?.remove(0);
        let values = |value: &TValue| -> TractResult<Vec<f32>> {
            Ok(value.try_as_plain_ram()?.as_slice::<f32>()?.to_vec())
        };
        Ok(Estimates {
            lsnr: values(&lsnr)?,
            erb_gains: values(&gains)?,
            df_coefs: values(&coefs)?,
        })
    }
}

fn models_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR_VARIABLE)
        .map_or_else(|| PathBuf::from(INSTALLED_MODELS_DIR), PathBuf::from)
}

fn enhance_offline(
    networks: &Networks,
    noisy: &[f32],
    chunk_frames: usize,
    context_frames: usize,
) -> TractResult<Vec<f32>> {
    let frames = noisy.as_chunks::<HOP_SIZE>().0;
    let mut extractor = FeatureExtractor::new();
    let (erb, complex): (Vec<_>, Vec<_>) = frames
        .iter()
        .map(|frame| {
            let features = extractor.extract(frame);
            (*features.erb, *features.complex)
        })
        .unzip();
    let mut enhancer = Enhancer::new();
    let mut output = vec![0.0; noisy.len()];
    let outputs = output.as_chunks_mut::<HOP_SIZE>().0;
    for start in (0..frames.len()).step_by(chunk_frames) {
        let end = (start + chunk_frames).min(frames.len());
        let first = start.saturating_sub(context_frames);
        let estimates = networks.run(&erb[first..end], &complex[first..end])?;
        for step in start..end {
            let local = step - first;
            let network = NetworkOutput {
                lsnr: estimates.lsnr[local],
                erb_gains: estimates.erb_gains[local * ERB_BANDS..][..ERB_BANDS].try_into()?,
                df_coefs: estimates.df_coefs[local * DF_COEFS_LEN..][..DF_COEFS_LEN].try_into()?,
            };
            enhancer.enhance(&frames[step], &network, &mut outputs[step]);
        }
    }
    Ok(output)
}

fn enhance_streaming(noisy: &[f32]) -> TractResult<Vec<f32>> {
    let mut model = DfTract::default();
    let mut output = vec![0.0; noisy.len()];
    for (frame, out) in noisy
        .chunks_exact(HOP_SIZE)
        .zip(output.chunks_exact_mut(HOP_SIZE))
    {
        model.process(
            ArrayView2::from_shape((1, HOP_SIZE), frame)?,
            ArrayViewMut2::from_shape((1, HOP_SIZE), out)?,
        )?;
    }
    Ok(output)
}

/// Energy of `actual - expected` relative to `expected`, after the models settled.
fn difference_db(actual: &[f32], expected: &[f32]) -> f32 {
    let settled = seconds(SETTLE_SECONDS)..expected.len();
    let error: f32 = actual[settled.clone()]
        .iter()
        .zip(&expected[settled.clone()])
        .map(|(a, b)| (a - b).powi(2))
        .sum();
    let reference: f32 = expected[settled].iter().map(|b| b * b).sum();
    10.0 * (error / reference).log10()
}

/// Needs the official export, e.g. from `packaging/fetch-models.sh`, in `$DFN_MODELS_DIR`.
#[test]
#[ignore = "needs the DeepFilterNet3 ONNX files; run in release"]
fn offline_recipe_reproduces_streaming_model() -> TractResult<()> {
    let networks = Networks::load(&models_dir())?;
    let speech = NoisySpeech::new();
    let streaming = enhance_streaming(&speech.noisy)?;
    let whole = enhance_offline(&networks, &speech.noisy, usize::MAX, 0)?;
    let chunked = enhance_offline(&networks, &speech.noisy, CHUNK_FRAMES, CONTEXT_FRAMES)?;
    let cold_chunks = enhance_offline(&networks, &speech.noisy, CHUNK_FRAMES, 0)?;

    let whole_vs_streaming = difference_db(&whole, &streaming);
    let chunked_vs_whole = difference_db(&chunked, &whole);
    let cold_vs_whole = difference_db(&cold_chunks, &whole);
    eprintln!(
        "whole vs streaming {whole_vs_streaming:.1} dB, chunked+context vs whole {chunked_vs_whole:.1} dB, \
         cold chunks vs whole {cold_vs_whole:.1} dB"
    );
    assert!(whole_vs_streaming < EXACT_DIFFERENCE_DB);
    for (name, output) in [
        ("whole", &whole),
        ("chunked", &chunked),
        ("cold chunks", &cold_chunks),
    ] {
        let reduction = speech.noise_reduction_db(output);
        let voice = speech.voice_level_db(output);
        eprintln!("{name}: gap noise reduction {reduction:.1} dB, voice level {voice:+.1} dB");
        assert!(
            reduction > REQUIRED_REDUCTION_DB && voice.abs() < MAX_VOICE_CHANGE_DB,
            "{name}"
        );
    }
    Ok(())
}

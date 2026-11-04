//! Shape of the official DeepFilterNet3 export (`config.ini` of `DeepFilterNet3_onnx.tar.gz`).

use voice_abi::FRAME_LEN;

/// Sub-directory of the models directory holding the ONNX files.
pub const MODEL_DIR_NAME: &str = "deepfilternet3";
pub const ENCODER_FILE: &str = "enc.onnx";
pub const ERB_DECODER_FILE: &str = "erb_dec.onnx";
pub const DF_DECODER_FILE: &str = "df_dec.onnx";

pub const FFT_SIZE: usize = 960;
/// One STFT hop is exactly one ABI frame.
pub const HOP_SIZE: usize = FRAME_LEN;
pub const FREQ_BINS: usize = FFT_SIZE / 2 + 1;
pub const ERB_BANDS: usize = 32;
pub const MIN_BINS_PER_ERB_BAND: usize = 2;
/// Lowest bins refined by the deep filter; the encoder's `feat_spec` width.
pub const DF_BINS: usize = 96;
/// Taps of the deep filter, oldest frame first.
pub const DF_ORDER: usize = 5;
/// Frames the network looks ahead (`max(conv_lookahead, df_lookahead)`).
pub const LOOKAHEAD_FRAMES: usize = 2;
/// Output delay against the input: STFT overlap plus the lookahead.
pub const LATENCY_SAMPLES: usize = FFT_SIZE - HOP_SIZE + LOOKAHEAD_FRAMES * HOP_SIZE;
/// Real values per frame of `df_dec`'s `coefs` output: `[DF_BINS][DF_ORDER][re, im]`.
pub const DF_COEFS_LEN: usize = DF_BINS * DF_ORDER * 2;
/// Channels of the encoder's convolutional skip outputs `e0..e3` and `c0`.
pub const CONV_CHANNELS: usize = 64;
/// Width of the encoder's `emb` output.
pub const EMBEDDING_LEN: usize = CONV_CHANNELS * ERB_BANDS / 4;
/// Exponential smoothing of the feature normalisation (`norm_tau = 1 s` at one frame per hop).
pub const NORM_ALPHA: f32 = 0.99;

/// Local SNR below which a frame is treated as noise only and muted.
pub const MIN_LSNR_DB: f32 = -10.0;
/// Local SNR above which a frame is clean speech and passes unchanged.
pub const MAX_LSNR_FOR_GAINS_DB: f32 = 30.0;
/// Local SNR above which the ERB gains suffice and the deep filter is skipped.
pub const MAX_LSNR_FOR_DEEP_FILTER_DB: f32 = 20.0;

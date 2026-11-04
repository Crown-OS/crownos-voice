//! DeepFilterNet3 signal processing around the three networks, for backends that run the
//! official ONNX export themselves (see the crate README, "DeepFilterNet on ONNX Runtime").
//!
//! Per hop: [`FeatureExtractor::extract`] → `enc` → `erb_dec` + `df_dec` → [`Enhancer::enhance`].

mod enhancer;
mod erb_bands;
mod features;
mod model;
mod post_filter;
mod stft;

pub use enhancer::{Enhancer, NetworkOutput};
pub use features::{FeatureExtractor, Features};
pub use model::*;
pub use realfft::num_complex::Complex32;

//! The contract between `crown-voiced` and its enhancement backends.
//!
//! A backend is a `cdylib` exporting one static, [`ENTRY_SYMBOL`], of type
//! [`VoiceBackendV1`], built with [`export`] from a type implementing [`Backend`].
//! The host side (feature `host`) loads plugins with [`host::LoadedBackend`] and
//! keeps the library mapped only while an instance is alive.
//!
//! Audio is always mono `f32` at [`SAMPLE_RATE`], processed in whole frames of
//! [`FRAME_LEN`] samples.

mod backend;
mod export;
mod types;

#[cfg(feature = "host")]
pub mod host;

pub use backend::{Backend, BackendError};
pub use export::export;
pub use types::{DeviceName, Model, Probe, Status, StreamConfig, VoiceBackendV1, Workload};

/// Version of [`VoiceBackendV1`]. Bumped on any layout change.
pub const ABI_VERSION: u32 = 1;
/// Name of the exported [`VoiceBackendV1`] static, NUL-terminated for `dlsym`.
pub const ENTRY_SYMBOL: &[u8] = b"CROWN_VOICE_BACKEND_V1\0";
/// Sample rate of every buffer crossing the ABI.
pub const SAMPLE_RATE: u32 = 48_000;
/// Samples per frame: 10 ms at [`SAMPLE_RATE`].
pub const FRAME_LEN: usize = 480;

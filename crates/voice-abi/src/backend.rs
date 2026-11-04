use std::ffi::CStr;
use std::path::Path;

use crate::{Model, Probe, Status, Workload};

/// Why a backend could not create or run an instance.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("{0} is not available on this machine")]
    Unavailable(String),
    #[error("buffer of {0} samples is not a whole number of frames")]
    InvalidLength(usize),
    #[error("{0}")]
    Failed(String),
}

impl BackendError {
    pub(crate) fn status(&self) -> Status {
        match self {
            Self::Unavailable(_) => Status::Unavailable,
            Self::InvalidLength(_) => Status::InvalidInput,
            Self::Failed(_) => Status::Failed,
        }
    }
}

/// Implemented by each plugin and turned into a C table by [`crate::export`].
pub trait Backend: Send + Sized + 'static {
    const NAME: &'static CStr;

    /// Cheap check of what this machine offers for `workload`. Must not load models.
    fn probe(workload: Workload) -> Probe;

    /// Samples by which `model`'s output lags its input; the host realigns offline output.
    fn latency_samples(_model: Model) -> u32 {
        0
    }

    fn create(model: Model, workload: Workload, models_dir: &Path) -> Result<Self, BackendError>;

    /// `input.len() == output.len()`, always a multiple of [`crate::FRAME_LEN`].
    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError>;
}

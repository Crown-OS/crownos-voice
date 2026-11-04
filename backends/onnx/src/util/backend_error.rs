use std::fmt::Display;

use voice_abi::BackendError;

pub(crate) fn failed(error: impl Display) -> BackendError {
    BackendError::Failed(format!("ONNX Runtime: {error}"))
}

use std::path::{Path, PathBuf};

use libloading::Library;
use ort::environment::GlobalThreadPoolOptions;
use ort::sys::OrtApiBase;
use voice_abi::BackendError;

const LIBRARY_VARIABLE: &str = "ORT_DYLIB_PATH";
/// Installed by the `onnxruntime-crownos` package.
const INSTALLED_LIBRARY: &str = "/usr/lib/libonnxruntime.so";
const API_BASE_SYMBOL: &[u8] = b"OrtGetApiBase\0";
const ENVIRONMENT_NAME: &str = "crown-voice";

/// `$ORT_DYLIB_PATH` if set, else the packaged `libonnxruntime.so`.
pub(crate) fn library_path() -> PathBuf {
    std::env::var_os(LIBRARY_VARIABLE)
        .filter(|path| !path.is_empty())
        .map_or_else(|| PathBuf::from(INSTALLED_LIBRARY), PathBuf::from)
}

/// Maps ONNX Runtime into the process with one thread pool shared by every session; later
/// calls reuse both.
pub(crate) fn load() -> Result<(), BackendError> {
    let path = library_path();
    check_loadable(&path)?;
    let environment = ort::init_from(&path).map_err(|error| unavailable(&path, error))?;
    environment
        .with_name(ENVIRONMENT_NAME)
        .with_global_thread_pool(GlobalThreadPoolOptions::default())
        .commit();
    Ok(())
}

/// `ort` deadlocks when its own load fails (its error type needs the very API it failed to
/// load), so the library and its API version are verified here first.
fn check_loadable(path: &Path) -> Result<(), BackendError> {
    // SAFETY: loading runs ONNX Runtime's initialisers, which `ort` would run anyway.
    let library = unsafe { Library::new(path) }.map_err(|error| unavailable(path, error))?;
    // SAFETY: `OrtGetApiBase` has this signature in every ONNX Runtime release.
    let api_base =
        unsafe { library.get::<unsafe extern "C" fn() -> *const OrtApiBase>(API_BASE_SYMBOL) }
            .map_err(|error| unavailable(path, error))?;
    // SAFETY: the returned base is static data of the still-loaded library.
    let api_supported = unsafe {
        let base = api_base();
        !base.is_null() && !((*base).GetApi)(ort::MINOR_VERSION).is_null()
    };
    if api_supported {
        Ok(())
    } else {
        Err(unavailable(
            path,
            format!("API version {} is not supported", ort::MINOR_VERSION),
        ))
    }
}

fn unavailable(path: &Path, error: impl std::fmt::Display) -> BackendError {
    BackendError::Unavailable(format!("ONNX Runtime at {} ({error})", path.display()))
}

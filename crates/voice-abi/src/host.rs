//! Loading backend plugins on demand.
//!
//! [`LoadedBackend`] owns the mapped library; every [`BackendInstance`] holds an
//! `Arc` to it, so the library is unmapped only after the host drops its handle
//! *and* the last instance is destroyed.

use std::ffi::{CString, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Arc;

use libloading::Library;

use crate::{
    ABI_VERSION, ENTRY_SYMBOL, Model, Probe, Status, StreamConfig, VoiceBackendV1, Workload,
};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not load backend {path}: {source}")]
    Library {
        path: PathBuf,
        source: libloading::Error,
    },
    #[error("{path} speaks voice ABI {found}, expected {ABI_VERSION}")]
    AbiMismatch { path: PathBuf, found: u32 },
    #[error("model directory {0} contains a NUL byte")]
    InvalidModelsDir(PathBuf),
    #[error("backend {backend} returned {status:?}")]
    Call { backend: String, status: Status },
}

/// A mapped plugin library and its exported table.
pub struct LoadedBackend {
    table: NonNull<VoiceBackendV1>,
    path: PathBuf,
    // Dropped last: `table` points into it.
    _library: Library,
}

// SAFETY: the table is immutable `'static` data inside the still-mapped library.
unsafe impl Send for LoadedBackend {}
unsafe impl Sync for LoadedBackend {}

impl LoadedBackend {
    /// Maps `path` and validates its table.
    ///
    /// # Safety
    /// `path` must be a voice backend plugin: loading runs its initialisers.
    pub unsafe fn load(path: &Path) -> Result<Arc<Self>, LoadError> {
        let library_error = |source| LoadError::Library {
            path: path.to_owned(),
            source,
        };
        // SAFETY: guaranteed by the caller.
        let library = unsafe { Library::new(path) }.map_err(library_error)?;
        // SAFETY: the symbol is declared as a `VoiceBackendV1` static by `export`.
        let table =
            unsafe { library.get::<*const VoiceBackendV1>(ENTRY_SYMBOL) }.map_err(library_error)?;
        let table = NonNull::new((*table).cast_mut())
            .ok_or_else(|| library_error(libloading::Error::DlSymUnknown))?;
        // SAFETY: non-null and pointing at the plugin's static table.
        let found = unsafe { table.as_ref() }.abi_version;
        if found != ABI_VERSION {
            return Err(LoadError::AbiMismatch {
                path: path.to_owned(),
                found,
            });
        }
        Ok(Arc::new(Self {
            table,
            path: path.to_owned(),
            _library: library,
        }))
    }

    fn table(&self) -> &VoiceBackendV1 {
        // SAFETY: valid for as long as `_library` is mapped, which is as long as `self`.
        unsafe { self.table.as_ref() }
    }

    pub fn name(&self) -> &str {
        self.table().name()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn probe(&self, workload: Workload) -> Probe {
        let mut report = Probe::UNUSABLE;
        // SAFETY: `report` is a valid out-pointer.
        match unsafe { (self.table().probe)(workload, &mut report) } {
            Status::Ok => report,
            _ => Probe::UNUSABLE,
        }
    }

    pub fn latency_samples(&self, model: Model) -> usize {
        // SAFETY: a plain value call into the validated table.
        let samples = unsafe { (self.table().latency)(model) };
        usize::try_from(samples).unwrap_or(0)
    }

    pub fn create(
        self: &Arc<Self>,
        model: Model,
        workload: Workload,
        models_dir: &Path,
    ) -> Result<BackendInstance, LoadError> {
        let models_dir_c = CString::new(models_dir.as_os_str().as_bytes())
            .map_err(|_| LoadError::InvalidModelsDir(models_dir.to_owned()))?;
        let config = StreamConfig {
            model,
            workload,
            models_dir: models_dir_c.as_ptr(),
        };
        let mut raw = std::ptr::null_mut();
        // SAFETY: `config` and `raw` outlive the call.
        let status = unsafe { (self.table().create)(&config, &mut raw) };
        match NonNull::new(raw) {
            Some(raw) if status == Status::Ok => Ok(BackendInstance {
                raw,
                backend: Arc::clone(self),
            }),
            _ => Err(self.call_error(status)),
        }
    }

    fn call_error(&self, status: Status) -> LoadError {
        LoadError::Call {
            backend: self.name().to_owned(),
            status,
        }
    }
}

/// One live enhancement state (one stream or one offline job).
pub struct BackendInstance {
    raw: NonNull<c_void>,
    backend: Arc<LoadedBackend>,
}

// SAFETY: `Backend: Send` is required of every plugin type.
unsafe impl Send for BackendInstance {}

impl BackendInstance {
    pub fn backend(&self) -> &LoadedBackend {
        &self.backend
    }

    /// Enhances `input` into `output`; both a whole number of frames long.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), LoadError> {
        if input.len() != output.len() {
            return Err(self.backend.call_error(Status::InvalidInput));
        }
        // SAFETY: `raw` is a live instance; buffers are valid for `len` samples.
        let status = unsafe {
            (self.backend.table().process)(
                self.raw.as_ptr(),
                input.as_ptr(),
                output.as_mut_ptr(),
                input.len(),
            )
        };
        match status {
            Status::Ok => Ok(()),
            status => Err(self.backend.call_error(status)),
        }
    }
}

impl Drop for BackendInstance {
    fn drop(&mut self) {
        // SAFETY: created by this backend's `create`, destroyed exactly once.
        unsafe { (self.backend.table().destroy)(self.raw.as_ptr()) };
    }
}

use std::ffi::{CStr, c_char, c_void};

/// Which enhancement network a stream runs.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Model {
    /// RNNoise-class: tiny, always real-time on any CPU.
    Light = 0,
    /// DeepFilterNet3-class noise suppression.
    Standard = 1,
    /// [`Model::Standard`] plus dereverberation.
    Studio = 2,
}

/// How the output is consumed, which decides what a backend optimises for.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Workload {
    /// A live 10 ms frame stream with a hard deadline.
    Realtime = 0,
    /// A file processed as fast as possible; latency is irrelevant.
    Offline = 1,
}

/// Result of every ABI call.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    /// The runtime, device or model this backend needs is not present.
    Unavailable = 1,
    /// A buffer length was not a whole number of frames.
    InvalidInput = 2,
    Failed = 3,
    /// The backend panicked; the instance must be destroyed.
    Panicked = 4,
}

/// Per-stream settings handed to `create`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StreamConfig {
    pub model: Model,
    pub workload: Workload,
    /// NUL-terminated directory holding the model files.
    pub models_dir: *const c_char,
}

/// Fixed-size, NUL-terminated device label (for example `"CUDA: RTX 4050"`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DeviceName(pub [u8; 64]);

impl DeviceName {
    pub const EMPTY: Self = Self([0; 64]);

    pub fn new(label: &str) -> Self {
        let mut bytes = [0; 64];
        let len = label.floor_char_boundary(bytes.len() - 1);
        bytes[..len].copy_from_slice(&label.as_bytes()[..len]);
        Self(bytes)
    }

    pub fn as_str(&self) -> &str {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        std::str::from_utf8(&self.0[..end]).unwrap_or_default()
    }
}

/// How well a backend can serve a workload on this machine.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    /// 0 means unusable; higher wins. CPU backends stay below 100, accelerators above.
    pub score: u32,
    pub device: DeviceName,
}

impl Probe {
    pub const UNUSABLE: Self = Self {
        score: 0,
        device: DeviceName::EMPTY,
    };
}

/// The table a backend plugin exports as [`crate::ENTRY_SYMBOL`].
#[repr(C)]
pub struct VoiceBackendV1 {
    pub abi_version: u32,
    pub name: *const c_char,
    pub probe: unsafe extern "C" fn(workload: Workload, out: *mut Probe) -> Status,
    /// Samples by which `model`'s output lags its input.
    pub latency: unsafe extern "C" fn(model: Model) -> u32,
    pub create: unsafe extern "C" fn(config: *const StreamConfig, out: *mut *mut c_void) -> Status,
    pub process: unsafe extern "C" fn(
        instance: *mut c_void,
        input: *const f32,
        output: *mut f32,
        len: usize,
    ) -> Status,
    pub destroy: unsafe extern "C" fn(instance: *mut c_void),
}

// SAFETY: the table is immutable and `name` points at a `'static` C string.
unsafe impl Sync for VoiceBackendV1 {}

impl VoiceBackendV1 {
    pub fn name(&self) -> &str {
        // SAFETY: `export` stores a `&'static CStr`; foreign tables are checked by the host loader.
        unsafe { CStr::from_ptr(self.name) }
            .to_str()
            .unwrap_or("unnamed")
    }
}

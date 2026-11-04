use std::ffi::{CStr, c_void};
use std::os::unix::ffi::OsStrExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use crate::{
    ABI_VERSION, Backend, FRAME_LEN, Model, Probe, Status, StreamConfig, VoiceBackendV1, Workload,
};

/// Builds the table a plugin exports:
///
/// ```ignore
/// #[unsafe(no_mangle)]
/// pub static CROWN_VOICE_BACKEND_V1: VoiceBackendV1 = voice_abi::export::<CpuBackend>();
/// ```
pub const fn export<B: Backend>() -> VoiceBackendV1 {
    VoiceBackendV1 {
        abi_version: ABI_VERSION,
        name: B::NAME.as_ptr(),
        probe: probe::<B>,
        latency: latency::<B>,
        create: create::<B>,
        process: process::<B>,
        destroy: destroy::<B>,
    }
}

fn guarded(body: impl FnOnce() -> Status) -> Status {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(Status::Panicked)
}

unsafe extern "C" fn probe<B: Backend>(workload: Workload, out: *mut Probe) -> Status {
    guarded(|| {
        let report = B::probe(workload);
        // SAFETY: the host passes a valid, writable `Probe`.
        unsafe { out.write(report) };
        Status::Ok
    })
}

unsafe extern "C" fn latency<B: Backend>(model: Model) -> u32 {
    catch_unwind(|| B::latency_samples(model)).unwrap_or(0)
}

unsafe extern "C" fn create<B: Backend>(
    config: *const StreamConfig,
    out: *mut *mut c_void,
) -> Status {
    guarded(|| {
        // SAFETY: the host passes a valid config whose `models_dir` is a NUL-terminated path.
        let config = unsafe { &*config };
        let models_dir = unsafe { CStr::from_ptr(config.models_dir) };
        let models_dir = Path::new(std::ffi::OsStr::from_bytes(models_dir.to_bytes()));
        match B::create(config.model, config.workload, models_dir) {
            Ok(instance) => {
                // SAFETY: `out` is a valid out-pointer owned by the host.
                unsafe { out.write(Box::into_raw(Box::new(instance)).cast()) };
                Status::Ok
            }
            Err(error) => error.status(),
        }
    })
}

unsafe extern "C" fn process<B: Backend>(
    instance: *mut c_void,
    input: *const f32,
    output: *mut f32,
    len: usize,
) -> Status {
    if !len.is_multiple_of(FRAME_LEN) {
        return Status::InvalidInput;
    }
    guarded(|| {
        // SAFETY: `instance` came from `create::<B>`; buffers hold `len` samples and do not overlap.
        let (instance, input, output) = unsafe {
            (
                &mut *instance.cast::<B>(),
                std::slice::from_raw_parts(input, len),
                std::slice::from_raw_parts_mut(output, len),
            )
        };
        instance
            .process(input, output)
            .map_or_else(|error| error.status(), |()| Status::Ok)
    })
}

unsafe extern "C" fn destroy<B: Backend>(instance: *mut c_void) {
    // SAFETY: `instance` came from `create::<B>` and is destroyed exactly once.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(unsafe { Box::from_raw(instance.cast::<B>()) })
    }));
}

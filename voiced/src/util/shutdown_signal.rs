use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use signal_hook::consts::{SIGINT, SIGTERM};

/// A flag raised by `SIGTERM` or `SIGINT`, so the service can stop cleanly.
pub fn shutdown_signal() -> io::Result<Arc<AtomicBool>> {
    let raised = Arc::new(AtomicBool::new(false));
    for signal in [SIGTERM, SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&raised))?;
    }
    Ok(raised)
}

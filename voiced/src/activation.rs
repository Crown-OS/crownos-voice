//! systemd socket activation (`sd_listen_fds`).

use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixListener;

const LISTEN_FDS_START: RawFd = 3;

/// The listening socket systemd passed in, if this process was socket-activated.
pub fn inherited_listener() -> Option<UnixListener> {
    let listen_pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let listen_fds: u32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if listen_pid != std::process::id() || listen_fds == 0 {
        return None;
    }
    log::info!("using the socket-activated listener");
    // SAFETY: systemd hands this process ownership of fds from LISTEN_FDS_START on.
    Some(unsafe { UnixListener::from_raw_fd(LISTEN_FDS_START) })
}

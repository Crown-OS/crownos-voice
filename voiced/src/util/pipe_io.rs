//! Pipe reads and writes that give up promptly once a cancel flag is raised.

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};

const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// A pipe end switched to non-blocking mode, so waits can observe `cancel`.
pub struct CancellablePipe<'a> {
    file: File,
    cancel: &'a AtomicBool,
}

impl<'a> CancellablePipe<'a> {
    pub fn new(file: File, cancel: &'a AtomicBool) -> io::Result<Self> {
        rustix::io::ioctl_fionbio(&file, true)?;
        Ok(Self { file, cancel })
    }

    /// Fills `buf` unless the writer closes first; returns the bytes read.
    pub fn read_full(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == ErrorKind::WouldBlock => self.wait(PollFlags::IN)?,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(filled)
    }

    pub fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.file.write(buf) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(written) => buf = &buf[written..],
                Err(error) if error.kind() == ErrorKind::WouldBlock => self.wait(PollFlags::OUT)?,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn wait(&self, readiness: PollFlags) -> io::Result<()> {
        let interval =
            Timespec::try_from(CANCEL_CHECK_INTERVAL).map_err(|_| ErrorKind::InvalidInput)?;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(ErrorKind::Interrupted.into());
            }
            let mut fds = [PollFd::new(&self.file, readiness)];
            match rustix::event::poll(&mut fds, Some(&interval)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => {}
                Ok(_) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_until_the_writer_closes() -> io::Result<()> {
        let (reader, mut writer) = io::pipe()?;
        let cancel = AtomicBool::new(false);
        let mut pipe =
            CancellablePipe::new(File::from(std::os::fd::OwnedFd::from(reader)), &cancel)?;
        writer.write_all(b"abc")?;
        drop(writer);
        let mut buf = [0; 8];
        assert_eq!(pipe.read_full(&mut buf)?, 3);
        Ok(())
    }

    #[test]
    fn a_raised_cancel_flag_stops_a_waiting_read() -> io::Result<()> {
        let (reader, _writer) = io::pipe()?;
        let cancel = AtomicBool::new(true);
        let mut pipe =
            CancellablePipe::new(File::from(std::os::fd::OwnedFd::from(reader)), &cancel)?;
        let error = pipe.read_full(&mut [0; 4]).err().map(|error| error.kind());
        assert_eq!(error, Some(ErrorKind::Interrupted));
        Ok(())
    }
}

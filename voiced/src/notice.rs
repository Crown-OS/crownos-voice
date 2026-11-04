//! Worker threads telling the IPC loop about things it must act on.

use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use voice_protocol::{JobId, StreamId};

use crate::deferred::ReplyTo;
use crate::live::{BackendSwitch, LiveStream, SwitchCause};

const WAKE_BATCH: usize = 64;

pub enum Notice {
    /// A live stream kept missing its deadline on its current backend.
    DeadlinesMissed(StreamId),
    /// An offline job stopped; `failure` is set when it did not finish cleanly.
    JobEnded { job: JobId, failure: Option<String> },
    /// `open_stream` finished creating the backend instance and the PipeWire nodes.
    StreamOpened {
        reply: ReplyTo,
        opened: Result<LiveStream, String>,
    },
    /// A new backend instance for a live stream is ready.
    BackendSwitched {
        stream: StreamId,
        cause: SwitchCause,
        switched: Result<BackendSwitch, String>,
    },
}

/// Cloneable sender that also wakes the IPC loop's `poll`.
#[derive(Clone)]
pub struct Notifier {
    notices: Sender<Notice>,
    wake: Arc<PipeWriter>,
}

impl Notifier {
    pub fn notify(&self, notice: Notice) {
        if self.notices.send(notice).is_ok() {
            let _ = (&*self.wake).write(&[1]);
        }
    }
}

/// The IPC loop's end: poll it for readability, then [`NoticeInbox::drain`].
pub struct NoticeInbox {
    notices: Receiver<Notice>,
    wake: PipeReader,
}

impl NoticeInbox {
    pub fn drain(&mut self) -> impl Iterator<Item = Notice> + '_ {
        let _ = self.wake.read(&mut [0; WAKE_BATCH]);
        self.notices.try_iter()
    }
}

impl AsFd for NoticeInbox {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.wake.as_fd()
    }
}

pub fn notice_channel() -> io::Result<(Notifier, NoticeInbox)> {
    let (reader, writer) = io::pipe()?;
    let (notices, receiver) = channel();
    Ok((
        Notifier {
            notices,
            wake: Arc::new(writer),
        },
        NoticeInbox {
            notices: receiver,
            wake: reader,
        },
    ))
}

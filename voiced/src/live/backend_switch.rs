use std::io;
use std::sync::Arc;

use voice_abi::Model;
use voice_protocol::StreamId;

use crate::deferred::ReplyTo;
use crate::notice::{Notice, Notifier};
use crate::processor::FrameProcessor;
use crate::registry::{BackendChoice, Registry};

pub const NO_BACKEND: &str = "no voice backend can run this";

/// Why a live stream gets a new backend instance.
pub enum SwitchCause {
    Requested(ReplyTo),
    Demotion,
    /// The `voice` settings changed the enhancement or the allowed backends.
    Settings,
}

/// A backend instance created for a running stream, ready to swap in.
pub struct BackendSwitch {
    pub model: Model,
    pub choice: BackendChoice,
    pub processor: FrameProcessor,
}

/// Creates the instance on its own thread, so the stream keeps its current one meanwhile.
pub fn spawn_backend_switch(
    stream: StreamId,
    cause: SwitchCause,
    model: Model,
    registry: Arc<Registry>,
    notifier: Notifier,
    create: impl FnOnce(&Registry) -> Option<(BackendChoice, FrameProcessor)> + Send + 'static,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("voice-switch-{}", stream.0))
        .spawn(move || {
            let switched = create(&registry)
                .map(|(choice, processor)| BackendSwitch {
                    model,
                    choice,
                    processor,
                })
                .ok_or_else(|| NO_BACKEND.to_owned());
            notifier.notify(Notice::BackendSwitched {
                stream,
                cause,
                switched,
            });
        })?;
    Ok(())
}

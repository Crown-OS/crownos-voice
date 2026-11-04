//! Service state: who owns which stream and job, and the backends serving them.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crownos_ipc::{Error, Message, PeerId, RemoteError, Server};
use voice_abi::Workload;
use voice_protocol::{JobId, StreamId, voice};

use crate::deferred::{self, ReplyTo, Routed};
use crate::job::OfflineJob;
use crate::live::{BackendSwitch, LiveStream, SwitchCause, spawn_backend_switch};
use crate::notice::{Notice, Notifier};
use crate::registry::Registry;

pub struct Daemon {
    pub registry: Arc<Registry>,
    pub streams: HashMap<StreamId, LiveStream>,
    pub jobs: HashMap<JobId, OfflineJob>,
    pub notifier: Notifier,
    next_id: u32,
    caller: Option<PeerId>,
}

impl Daemon {
    pub fn new(registry: Registry, notifier: Notifier) -> Self {
        Self {
            registry: Arc::new(registry),
            streams: HashMap::new(),
            jobs: HashMap::new(),
            notifier,
            next_id: 0,
            caller: None,
        }
    }

    /// Routes one request, remembering who sent it for the handler.
    pub fn dispatch(
        &mut self,
        server: &mut Server,
        peer: PeerId,
        message: Message,
    ) -> Result<(), Error> {
        match deferred::route(peer, message) {
            Routed::Deferred(reply, request) => {
                self.start(server, reply, request);
                Ok(())
            }
            Routed::Malformed => {
                server.drop_peer(peer);
                Ok(())
            }
            Routed::Immediate(message) => {
                self.caller = Some(peer);
                let result = voice::dispatch(self, server, peer, message);
                self.caller = None;
                result
            }
        }
    }

    pub fn caller(&self) -> Result<PeerId, RemoteError> {
        self.caller.ok_or_else(|| RemoteError::handler("no caller"))
    }

    pub fn allocate_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    pub fn is_idle(&self) -> bool {
        self.streams.is_empty() && self.jobs.is_empty()
    }

    /// Tears down everything `peer` opened.
    pub fn release(&mut self, peer: PeerId) {
        self.streams.retain(|_, stream| stream.owner != peer);
        self.jobs.retain(|_, job| job.owner != peer);
    }

    pub fn handle(&mut self, server: &mut Server, notice: Notice) -> Result<(), Error> {
        match notice {
            Notice::DeadlinesMissed(stream) => {
                self.demote(stream);
                Ok(())
            }
            Notice::JobEnded { job, failure } => {
                let still_listed = self.jobs.remove(&job).is_some();
                match failure {
                    Some(reason) if still_listed => {
                        log::warn!("job {} failed: {reason}", job.0);
                        server.emit(&voice::JobFailed { job, reason })
                    }
                    _ => Ok(()),
                }
            }
            Notice::StreamOpened { reply, opened } => {
                self.finish_open(server, reply, opened);
                Ok(())
            }
            Notice::BackendSwitched {
                stream,
                cause,
                switched,
            } => self.finish_switch(server, stream, cause, switched),
        }
    }

    fn finish_open(
        &mut self,
        server: &mut Server,
        reply: ReplyTo,
        opened: Result<LiveStream, String>,
    ) {
        match opened {
            Ok(live) => {
                if reply.send(server, Ok(&live.node)).is_ok() {
                    log::info!(
                        "stream {} on {} as {}",
                        live.node.stream.0,
                        live.node.backend,
                        live.node.node_name
                    );
                    self.streams.insert(live.node.stream, live);
                }
            }
            Err(reason) => {
                log::warn!("could not open a stream: {reason}");
                let _ = reply.send(server, Err(RemoteError::handler(reason)));
            }
        }
    }

    fn demote(&mut self, id: StreamId) {
        let Some(stream) = self.streams.get(&id) else {
            return;
        };
        if stream.choice.fallbacks().is_empty() {
            log::warn!(
                "stream {} misses deadlines and has no slower backend left",
                id.0
            );
            return;
        }
        let (choice, model) = (stream.choice.clone(), stream.model);
        let started = spawn_backend_switch(
            id,
            SwitchCause::Demotion,
            model,
            Arc::clone(&self.registry),
            self.notifier.clone(),
            move |registry| registry.demote(&choice, model, Workload::Realtime),
        );
        if let Err(error) = started {
            log::warn!("could not demote stream {}: {error}", id.0);
        }
    }

    fn finish_switch(
        &mut self,
        server: &mut Server,
        id: StreamId,
        cause: SwitchCause,
        switched: Result<BackendSwitch, String>,
    ) -> Result<(), Error> {
        let stream = self.streams.get_mut(&id);
        let outcome = match (switched, stream) {
            (Err(reason), _) => Err(reason),
            (Ok(_), None) => Err("no such stream".to_owned()),
            (Ok(switch), Some(stream)) => {
                if matches!(cause, SwitchCause::Demotion) && switch.model != stream.model {
                    return Ok(());
                }
                let backend = self
                    .registry
                    .label(switch.choice.current(), Workload::Realtime);
                stream.switch_backend(switch, backend);
                Ok(stream.node.clone())
            }
        };
        match cause {
            SwitchCause::Requested(reply) => {
                let _ = reply.send(server, outcome.as_ref().map_err(RemoteError::handler));
                Ok(())
            }
            SwitchCause::Demotion => match outcome {
                Ok(node) => {
                    log::info!("stream {} demoted to {}", id.0, node.backend);
                    server.emit(&voice::StreamChanged { node })
                }
                Err(reason) => {
                    log::warn!("stream {} could not be demoted: {reason}", id.0);
                    Ok(())
                }
            },
        }
    }

    pub fn sweep(&mut self, now: Instant) {
        self.registry.sweep(now);
    }
}

//! The `crownos-voice` protocol methods.

use std::os::fd::OwnedFd;
use std::sync::Arc;

use crownos_ipc::{RemoteError, Server};
use voice_abi::Workload;
use voice_protocol::{EnhancedNode, Enhancement, JobId, ServiceStatus, StreamId, voice};

use crate::daemon::Daemon;
use crate::deferred::{DeferredRequest, ReplyTo};
use crate::enhancement::model_for;
use crate::job::{JobPlan, OfflineJob};
use crate::live::{LiveStream, StreamPlan, SwitchCause, spawn_backend_switch};

const ANSWERED_WHEN_READY: &str = "answered once its backend is ready";

fn failure(message: impl Into<String>) -> RemoteError {
    RemoteError::handler(message)
}

fn model(enhancement: Enhancement) -> Result<voice_abi::Model, RemoteError> {
    model_for(enhancement)
        .ok_or_else(|| failure("Enhancement::Off needs no service; capture the source directly"))
}

impl Daemon {
    /// Starts a request whose reply waits for a backend instance; answers at once only on error.
    pub fn start(&mut self, server: &mut Server, reply: ReplyTo, request: DeferredRequest) {
        let started = match request {
            DeferredRequest::OpenStream(voice::open_stream {
                source,
                enhancement,
            }) => self.start_open(reply, source, enhancement),
            DeferredRequest::SetEnhancement(voice::set_enhancement {
                stream,
                enhancement,
            }) => self.start_switch(reply, stream, enhancement),
        };
        if let Err(error) = started {
            let _ = reply.send(server, Err(error));
        }
    }

    fn start_open(
        &mut self,
        reply: ReplyTo,
        source: Option<String>,
        enhancement: Enhancement,
    ) -> Result<(), RemoteError> {
        let model = model(enhancement)?;
        if source
            .as_deref()
            .is_some_and(|name| name.chars().any(char::is_control))
        {
            return Err(failure("invalid source name"));
        }
        let plan = StreamPlan {
            owner: reply.peer,
            stream: StreamId(self.allocate_id()),
            target: source,
            model,
        };
        LiveStream::open_in_background(
            plan,
            Arc::clone(&self.registry),
            self.notifier.clone(),
            reply,
        )
        .map_err(|error| failure(format!("could not start the stream: {error}")))
    }

    fn start_switch(
        &mut self,
        reply: ReplyTo,
        stream: StreamId,
        enhancement: Enhancement,
    ) -> Result<(), RemoteError> {
        let model = model(enhancement)?;
        if !self
            .streams
            .get(&stream)
            .is_some_and(|live| live.owner == reply.peer)
        {
            return Err(failure("no such stream"));
        }
        spawn_backend_switch(
            stream,
            SwitchCause::Requested(reply),
            model,
            Arc::clone(&self.registry),
            self.notifier.clone(),
            move |registry| registry.choose(model, Workload::Realtime),
        )
        .map_err(|error| failure(format!("could not switch the backend: {error}")))
    }
}

impl voice::Handler for Daemon {
    fn open_stream(
        &mut self,
        _source: Option<String>,
        _enhancement: Enhancement,
    ) -> Result<EnhancedNode, RemoteError> {
        Err(failure(ANSWERED_WHEN_READY))
    }

    fn set_enhancement(
        &mut self,
        _stream: StreamId,
        _enhancement: Enhancement,
    ) -> Result<EnhancedNode, RemoteError> {
        Err(failure(ANSWERED_WHEN_READY))
    }

    fn close_stream(&mut self, stream: StreamId) -> Result<(), RemoteError> {
        let caller = self.caller()?;
        match self.streams.get(&stream) {
            Some(live) if live.owner == caller => {
                self.streams.remove(&stream);
                Ok(())
            }
            _ => Err(failure("no such stream")),
        }
    }

    fn enhance_pcm(
        &mut self,
        enhancement: Enhancement,
        input: OwnedFd,
        output: OwnedFd,
    ) -> Result<JobId, RemoteError> {
        let plan = JobPlan {
            job: JobId(self.allocate_id()),
            owner: self.caller()?,
            model: model(enhancement)?,
        };
        let job = plan.job;
        let running = OfflineJob::spawn(
            plan,
            Arc::clone(&self.registry),
            (input, output),
            self.notifier.clone(),
        )
        .map_err(|error| failure(format!("could not start the job: {error}")))?;
        self.jobs.insert(job, running);
        Ok(job)
    }

    fn cancel_job(&mut self, job: JobId) -> Result<(), RemoteError> {
        let caller = self.caller()?;
        match self.jobs.get(&job) {
            Some(running) if running.owner == caller => {
                self.jobs.remove(&job);
                Ok(())
            }
            _ => Err(failure("no such job")),
        }
    }

    fn status(&mut self) -> Result<ServiceStatus, RemoteError> {
        Ok(ServiceStatus {
            backends: self.registry.status(),
            streams: self.streams.len() as u32,
            jobs: self.jobs.len() as u32,
        })
    }
}

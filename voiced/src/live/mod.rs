//! Live streams: an enhanced virtual microphone per `open_stream`.

mod backend_switch;
mod capture_target;
mod deadline;
mod graph;
mod rings;
mod source_query;
mod worker;

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Instant;

use anyhow::Context;
use crownos_ipc::PeerId;
use voice_abi::{Model, Workload};
use voice_protocol::{EnhancedNode, StreamId};

use crate::deferred::ReplyTo;
use crate::notice::{Notice, Notifier};
use crate::processor::FrameProcessor;
use crate::registry::{BackendChoice, Registry};
pub use backend_switch::{BackendSwitch, NO_BACKEND, SwitchCause, spawn_backend_switch};
use deadline::DeadlineMonitor;
use graph::{EnhancedGraph, GraphSpec};
use rings::LATENCY_SAMPLES;
use worker::EnhancementWorker;

/// `node.name` prefix of every node this service creates.
pub const NODE_PREFIX: &str = "crownos-voice";

/// One enhanced microphone. Fields drop in order: PipeWire stops before the worker.
pub struct LiveStream {
    pub owner: PeerId,
    pub node: EnhancedNode,
    pub model: Model,
    pub choice: BackendChoice,
    _graph: EnhancedGraph,
    worker: EnhancementWorker,
}

/// What `open_stream` decided before any backend exists.
pub struct StreamPlan {
    pub owner: PeerId,
    pub stream: StreamId,
    pub target: Option<String>,
    pub model: Model,
}

impl LiveStream {
    /// Opens the stream on a new thread and reports it as [`Notice::StreamOpened`].
    pub fn open_in_background(
        plan: StreamPlan,
        registry: Arc<Registry>,
        notifier: Notifier,
        reply: ReplyTo,
    ) -> io::Result<()> {
        std::thread::Builder::new()
            .name(format!("voice-open-{}", plan.stream.0))
            .spawn(move || {
                let opened = Self::open(plan, &registry, notifier.clone())
                    .map_err(|error| format!("{error:#}"));
                notifier.notify(Notice::StreamOpened { reply, opened });
            })?;
        Ok(())
    }

    fn open(plan: StreamPlan, registry: &Registry, notifier: Notifier) -> anyhow::Result<Self> {
        let (choice, processor) = registry
            .choose(plan.model, Workload::Realtime)
            .context(NO_BACKEND)?;
        let node = EnhancedNode {
            stream: plan.stream,
            node_name: format!("{NODE_PREFIX}.{}.{}", std::process::id(), plan.stream.0),
            backend: registry.label(choice.current(), Workload::Realtime),
            latency_samples: node_latency(&processor),
        };
        let misses = Arc::new(AtomicU32::new(0));
        let monitor = DeadlineMonitor::new(Arc::clone(&misses), Instant::now());
        let (worker, rings_sender) =
            EnhancementWorker::spawn(plan.stream, processor, monitor, notifier)?;
        let worker_thread = worker.thread().context("the worker thread is gone")?;
        let (tap, worker_rings, mixer) = rings::stream_rings(worker_thread, misses);
        rings_sender
            .send(worker_rings)
            .ok()
            .context("the worker thread exited early")?;
        let spec = GraphSpec {
            target: plan.target,
            node_name: node.node_name.clone(),
        };
        let graph = EnhancedGraph::spawn(spec, tap, mixer)
            .context("could not create the PipeWire nodes")?;
        Ok(Self {
            owner: plan.owner,
            node,
            model: plan.model,
            choice,
            _graph: graph,
            worker,
        })
    }

    /// Swaps in `switch` between two frames, without interrupting the stream.
    pub fn switch_backend(&mut self, switch: BackendSwitch, backend: String) {
        self.node.latency_samples = node_latency(&switch.processor);
        self.node.backend = backend;
        self.model = switch.model;
        self.choice = switch.choice;
        self.worker.replace(switch.processor);
    }
}

/// The ring deadline plus the backend's own lag.
fn node_latency(processor: &FrameProcessor) -> u32 {
    u32::try_from(processor.latency_samples())
        .map_or(u32::MAX, |backend| LATENCY_SAMPLES.saturating_add(backend))
}

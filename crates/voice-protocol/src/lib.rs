//! Control protocol of `crown-voiced` over `crownos-ipc`
//! (`$XDG_RUNTIME_DIR/crownos/crownos-voice.sock`).
//!
//! Audio never travels in frames. A live stream is a PipeWire node the client
//! connects to by name. An offline job reads raw mono `f32` little-endian PCM at
//! [`SAMPLE_RATE`] from the `input` pipe and writes the enhanced PCM to the
//! `output` pipe, closing it when done. Streams and jobs end when the connection
//! that opened them closes.

mod enhancement;

pub use enhancement::Enhancement;

use serde::{Deserialize, Serialize};

/// Sample rate of offline PCM pipes and of every enhanced node.
pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub u32);

/// A live enhanced microphone the client should capture from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnhancedNode {
    pub stream: StreamId,
    /// PipeWire `node.name`; pass it as `target.object`.
    pub node_name: String,
    /// Backend and device serving the stream, for example `"cpu: DeepFilterNet3"`.
    pub backend: String,
    /// Extra latency the enhancement adds, in samples at [`SAMPLE_RATE`].
    pub latency_samples: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub device: String,
    pub realtime_score: u32,
    pub offline_score: u32,
    pub loaded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub backends: Vec<BackendInfo>,
    pub streams: u32,
    pub jobs: u32,
}

crownos_ipc::protocol! {
    /// `crown-voiced`.
    pub mod voice: "crownos-voice" {
        /// Start enhancing `source` (a PipeWire `node.name`; `None` is the default source).
        fn open_stream(source: Option<String>, enhancement: Enhancement) -> EnhancedNode;
        /// Change a live stream's enhancement without reconnecting.
        fn set_enhancement(stream: StreamId, enhancement: Enhancement) -> EnhancedNode;
        fn close_stream(stream: StreamId);
        /// Enhance PCM read from `input`, writing the result to `output`.
        fn enhance_pcm(enhancement: Enhancement) fds(input, output) -> JobId;
        fn cancel_job(job: JobId);
        fn status() -> ServiceStatus;
        /// A live stream switched backend, for example after missing deadlines.
        event StreamChanged { node: EnhancedNode };
        /// An offline job stopped early; its `output` pipe is closed.
        event JobFailed { job: JobId, reason: String };
    }
}

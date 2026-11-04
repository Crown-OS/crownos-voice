//! The PipeWire side of a live stream: a capture stream and the virtual source apps record from.

use std::io::Cursor;
use std::sync::mpsc;
use std::thread::JoinHandle;

use pipewire as pw;
use pw::context::ContextBox;
use pw::main_loop::MainLoopRc;
use pw::properties::{PropertiesBox, properties};
use pw::spa::param::ParamType;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Value};
use pw::spa::sys::SPA_AUDIO_CHANNEL_MONO;
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{Stream, StreamBox, StreamFlags};
use voice_abi::SAMPLE_RATE;

use super::NODE_PREFIX;
use super::rings::{CaptureTap, DeadlineMixer};
use super::source_query::resolve_capture_target;
use crate::util::pcm::SAMPLE_BYTES;

const QUANTUM: &str = "480/48000";
const SOURCE_DESCRIPTION: &str = "CrownOS Enhanced Microphone";
/// The session manager never links two nodes of one link group, nor makes them follow the
/// default source, so no capture ever records an enhanced node, even one that became the default.
const LOOP_GUARD_LINK_GROUP: &str = NODE_PREFIX;

/// Where a live stream captures from and the virtual source it exposes.
pub struct GraphSpec {
    /// `node.name` of the microphone; `None` means the default source.
    pub target: Option<String>,
    pub node_name: String,
}

/// A PipeWire loop thread running one stream's two nodes until dropped.
pub struct EnhancedGraph {
    quit: pw::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl EnhancedGraph {
    pub fn spawn(spec: GraphSpec, tap: CaptureTap, mixer: DeadlineMixer) -> anyhow::Result<Self> {
        let (quit, quit_receiver) = pw::channel::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("voice-pipewire".into())
            .spawn(move || {
                if let Err(error) = run_graph(&spec, tap, mixer, quit_receiver, &ready_sender) {
                    let _ = ready_sender.send(Err(error));
                }
            })?;
        let mut graph = Self {
            quit,
            thread: Some(thread),
        };
        match ready.recv() {
            Ok(Ok(())) => Ok(graph),
            Ok(Err(error)) => {
                graph.stop();
                Err(error.into())
            }
            Err(_) => {
                graph.stop();
                Err(pw::Error::CreationFailed.into())
            }
        }
    }

    fn stop(&mut self) {
        let _ = self.quit.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for EnhancedGraph {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_graph(
    spec: &GraphSpec,
    tap: CaptureTap,
    mixer: DeadlineMixer,
    quit: pw::channel::Receiver<()>,
    ready: &mpsc::SyncSender<Result<(), pw::Error>>,
) -> Result<(), pw::Error> {
    let main_loop = MainLoopRc::new(None)?;
    let context = ContextBox::new(main_loop.loop_(), None)?;
    let core = context.connect(None)?;
    let format = mono_format()?;
    let resolved;
    let target = match spec.target.as_deref() {
        Some(named) => Some(named),
        None => {
            resolved = resolve_capture_target(&main_loop, &core)?;
            resolved.as_deref()
        }
    };
    log::info!(
        "{} captures from {}",
        spec.node_name,
        target.unwrap_or("the default source")
    );

    let capture = StreamBox::new(
        &core,
        "crownos-voice-capture",
        capture_properties(spec, target),
    )?;
    let _capture_listener = capture
        .add_local_listener_with_user_data(tap)
        .process(capture_block)
        .register()?;
    capture.connect(
        Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut [Pod::from_bytes(&format).ok_or(pw::Error::CreationFailed)?],
    )?;

    let source = StreamBox::new(&core, "crownos-voice-source", source_properties(spec))?;
    let _source_listener = source
        .add_local_listener_with_user_data(mixer)
        .process(play_block)
        .register()?;
    source.connect(
        Direction::Output,
        None,
        StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut [Pod::from_bytes(&format).ok_or(pw::Error::CreationFailed)?],
    )?;

    let quitting_loop = main_loop.clone();
    let _quit = quit.attach(main_loop.loop_(), move |()| quitting_loop.quit());
    let _ = ready.send(Ok(()));
    main_loop.run();
    Ok(())
}

fn capture_properties(spec: &GraphSpec, target: Option<&str>) -> PropertiesBox {
    let mut properties = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => format!("{}.capture", spec.node_name),
        *pw::keys::NODE_GROUP => spec.node_name.as_str(),
        *pw::keys::NODE_LINK_GROUP => LOOP_GUARD_LINK_GROUP,
        *pw::keys::NODE_LATENCY => QUANTUM,
    };
    if let Some(target) = target {
        properties.insert(*pw::keys::TARGET_OBJECT, target);
    }
    properties
}

fn source_properties(spec: &GraphSpec) -> PropertiesBox {
    properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CLASS => "Audio/Source",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => spec.node_name.as_str(),
        *pw::keys::NODE_DESCRIPTION => SOURCE_DESCRIPTION,
        *pw::keys::NODE_GROUP => spec.node_name.as_str(),
        *pw::keys::NODE_LINK_GROUP => LOOP_GUARD_LINK_GROUP,
        *pw::keys::NODE_LATENCY => QUANTUM,
    }
}

fn capture_block(stream: &Stream, tap: &mut CaptureTap) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let offset = data.chunk().offset() as usize;
    let size = data.chunk().size() as usize;
    if let Some(bytes) = data
        .data()
        .and_then(|bytes| bytes.get(offset..offset + size))
    {
        tap.push(bytes.as_chunks::<SAMPLE_BYTES>().0);
    }
}

fn play_block(stream: &Stream, mixer: &mut DeadlineMixer) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let requested = usize::try_from(buffer.requested()).unwrap_or_default();
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let written = data.data().map_or(0, |bytes| {
        let (samples, _) = bytes.as_chunks_mut::<SAMPLE_BYTES>();
        let frames = if requested == 0 {
            samples.len()
        } else {
            requested.min(samples.len())
        };
        mixer.fill(&mut samples[..frames]);
        frames * SAMPLE_BYTES
    });
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = SAMPLE_BYTES as i32;
    *chunk.size_mut() = u32::try_from(written).unwrap_or_default();
}

/// Mono 32-bit float at [`SAMPLE_RATE`], which PipeWire converts any device to.
fn mono_format() -> Result<Vec<u8>, pw::Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(SAMPLE_RATE);
    info.set_channels(1);
    let mut position = [0; MAX_CHANNELS];
    position[0] = SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let format = Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    });
    PodSerializer::serialize(Cursor::new(Vec::new()), &format)
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|_| pw::Error::CreationFailed)
}

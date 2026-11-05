use crownos_ipc::PeerId;

/// Who a live stream belongs to, and so who may change or close it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamOwner {
    Peer(PeerId),
    /// The all-apps microphone the `voice` settings ask for.
    AllApps,
}

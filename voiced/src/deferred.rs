//! Requests answered only once a backend instance exists, which is created off the IPC loop.

use crownos_ipc::{Error, Kind, Message, MethodMsg, PeerId, RemoteError, Server, postcard};
use voice_protocol::{EnhancedNode, voice};

/// Where a deferred reply goes.
#[derive(Clone, Copy, Debug)]
pub struct ReplyTo {
    pub peer: PeerId,
    req_id: u32,
}

impl ReplyTo {
    /// Fails when the peer is gone or not reading.
    pub fn send(
        self,
        server: &mut Server,
        result: Result<&EnhancedNode, RemoteError>,
    ) -> Result<(), Error> {
        match result {
            Ok(node) => server.reply(
                self.peer,
                self.req_id,
                &postcard::to_stdvec(node)?,
                Vec::new(),
            ),
            Err(error) => server.reply_err(self.peer, self.req_id, &error),
        }
    }
}

pub enum DeferredRequest {
    OpenStream(voice::open_stream),
    SetEnhancement(voice::set_enhancement),
}

pub enum Routed {
    Deferred(ReplyTo, DeferredRequest),
    Malformed,
    Immediate(Message),
}

/// Separates the requests whose reply waits for a backend from those answered right away.
pub fn route(peer: PeerId, message: Message) -> Routed {
    if message.kind != Kind::Request {
        return Routed::Immediate(message);
    }
    let reply = ReplyTo {
        peer,
        req_id: message.req_id,
    };
    let request = match message.selector {
        voice::open_stream::SELECTOR => {
            postcard::from_bytes(&message.payload).map(DeferredRequest::OpenStream)
        }
        voice::set_enhancement::SELECTOR => {
            postcard::from_bytes(&message.payload).map(DeferredRequest::SetEnhancement)
        }
        _ => return Routed::Immediate(message),
    };
    request.map_or(Routed::Malformed, |request| {
        Routed::Deferred(reply, request)
    })
}

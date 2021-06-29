use crate::packet::Packet;
use crate::peer::PeerMut;

#[derive(Debug)]
pub struct Event<'a, T> {
    pub peer: PeerMut<'a, T>,
    pub kind: EventKind,
}

/// Event variant.
#[derive(Debug)]
pub enum EventKind {
    /// A peer connected.
    Connect(u32),
    /// A peer disconnected.
    Disconnect(u32),
    /// A packet was received from a peer.
    Receive(Packet),
}

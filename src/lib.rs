//! benet is a crate providing convenient and idiomatic Rust bindings to the ENet library.
//!
//! For an explanation of what ENet is and what is it for, please see the project's [homepage](http://enet.bespin.org).

pub mod compress;
pub mod error;
pub mod event;
pub mod host;
pub mod packet;
pub mod peer;

mod init;

pub use crate::error::Error;
pub use crate::event::{Event, EventKind};
pub use crate::host::Host;
pub use crate::packet::{Flags as PacketFlags, Packet};
pub use crate::peer::{Peer, PeerInfo, PeerMut};

/// Returns the linked version of the ENet library.
pub fn linked_version() -> u32 {
    unsafe { enet_sys::enet_linked_version() }
}

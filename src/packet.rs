use crate::error::Error;
use crate::init::InitGuard;

use enet_sys::ENetPacket;
use std::fmt::{self, Debug, Formatter};
use std::mem;
use std::slice;

pub struct Packet {
    packet: *mut ENetPacket,
    channel_id: u8,
    _guard: InitGuard,
}

impl Packet {
    pub(crate) unsafe fn from_raw(packet: *mut ENetPacket, channel_id: u8) -> Result<Self, Error> {
        Ok(Self {
            packet,
            channel_id,
            _guard: InitGuard::new()?,
        })
    }

    pub(crate) unsafe fn into_raw(self) -> *mut ENetPacket {
        self.packet
    }

    /// Creates a new packet.
    ///
    /// Special care is taken not to copy the data vector by managing the memory manually when passing it to ENet.
    pub fn new(data: Vec<u8>, channel_id: u8, flags: Flags) -> Result<Self, Error> {
        let guard = InitGuard::new()?;
        let packet = unsafe {
            enet_sys::enet_packet_create(
                &data as *const _ as *const _,
                data.len(),
                flags.flags | enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_NO_ALLOCATE,
            )
        };

        if packet.is_null() {
            return Err(Error::Unknown);
        }

        let packet = unsafe { &mut *packet };
        packet.userData = data.capacity() as *mut _;
        packet.freeCallback = Some(free_callback);

        mem::forget(data);

        Ok(Packet {
            packet,
            channel_id,
            _guard: guard,
        })
    }

    /// Returns flags that the packet was created with.
    pub fn flags(&self) -> Flags {
        Flags {
            flags: unsafe { *self.packet }.flags,
        }
    }

    /// Returns data associated with this packet.
    pub fn data(&self) -> &[u8] {
        let packet = unsafe { &*self.packet };
        if packet.dataLength == 0 {
            return &[];
        }

        unsafe { slice::from_raw_parts(packet.data, packet.dataLength) }
    }

    /// Returns the channel associated with this packet.
    pub fn channel_id(&self) -> u8 {
        self.channel_id
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe {
            enet_sys::enet_packet_destroy(self.packet);
        }
    }
}

impl Debug for Packet {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Packet")
            .field("data", &self.data())
            .field("channel_id", &self.channel_id)
            .field("flags", &self.flags())
            .finish()
    }
}

/// Packet delivery options.
#[derive(Default, Clone, Copy)]
pub struct Flags {
    flags: u32,
}

impl Flags {
    /// Packet must be received by the target peer and resend attempts should be made until the packet is delivered.
    ///
    /// Panics if unsequenced has been set before.
    pub fn reliable(self) -> Self {
        if self.check_flag(enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_UNSEQUENCED) {
            panic!("reliable and unsequenced flags are not supported at the same time");
        }

        Self {
            flags: self.flags | enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_RELIABLE,
        }
    }

    pub fn is_reliable(&self) -> bool {
        self.check_flag(enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_RELIABLE)
    }

    /// Packet will not be sequenced with other packets (not supported for reliable packets).
    ///
    /// Panics if reliable has been set before.
    pub fn unsequenced(self) -> Self {
        if self.check_flag(enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_RELIABLE) {
            panic!("reliable and unsequenced flags are not supported at the same time");
        }

        Self {
            flags: self.flags | enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_UNSEQUENCED,
        }
    }

    pub fn is_unsequenced(&self) -> bool {
        self.check_flag(enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_UNSEQUENCED)
    }

    /// Packet will be fragmented using unreliable (instead of reliable) sends if it exceeds the MTU.
    pub fn unreliable_fragment(self) -> Self {
        Self {
            flags: self.flags | enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_UNRELIABLE_FRAGMENT,
        }
    }

    pub fn is_unreliable_fragment(self) -> bool {
        self.check_flag(enet_sys::_ENetPacketFlag_ENET_PACKET_FLAG_UNRELIABLE_FRAGMENT)
    }

    fn check_flag(&self, flag: u32) -> bool {
        self.flags & flag == flag
    }
}

impl Debug for Flags {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Flags")
            .field("reliable", &self.reliable())
            .field("unsequenced", &self.unsequenced())
            .field("unreliable_fragment", &self.unreliable_fragment())
            .finish()
    }
}

extern "C" fn free_callback(packet: *mut ENetPacket) {
    unsafe {
        let packet = &*packet;

        Vec::<u8>::from_raw_parts(packet.data, packet.dataLength, packet.userData as usize);
    }
}

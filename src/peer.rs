use crate::host::Host;
use crate::packet::Packet;
use crate::Error;

use enet_sys::ENetPeer;
use std::convert::TryInto;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::net::SocketAddrV4;
use std::ptr;
use std::time::Duration;

pub const PACKET_LOSS_SCALE: u32 = enet_sys::ENET_PEER_PACKET_LOSS_SCALE;
pub const PACKET_THROTTLE_INTERVAL: Duration =
    Duration::from_millis(enet_sys::ENET_PEER_PACKET_THROTTLE_INTERVAL as _);
pub const PACKET_THROTTLE_SCALE: u32 = enet_sys::ENET_PEER_PACKET_THROTTLE_SCALE;

/// Read-only view of a peer.
#[derive(Clone, Copy)]
pub struct Peer<'a, T> {
    peer: *const ENetPeer,
    _data: PhantomData<T>,
    _host: PhantomData<&'a Host<T>>,
    // No InitGuard here because always borrowed from a Host.
}

impl<T> Peer<'_, T> {
    pub(crate) unsafe fn from_raw(peer: *const ENetPeer) -> Self {
        Self {
            peer,
            _data: PhantomData,
            _host: PhantomData,
        }
    }

    /// Returns a reference to data associated with this peer.
    ///
    /// The data is created default-initialized for the first time a peer is returned from a [Host](crate::host::Host).
    pub fn data(&self) -> &T {
        unsafe { &*((*self.peer).data as *const T) }
    }

    /// Returns information about this peer.
    pub fn info(&self) -> PeerInfo {
        PeerInfo {
            peer: unsafe { &*self.peer },
        }
    }
}

impl<T: Debug> Debug for Peer<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peer")
            .field("data", self.data())
            .field("info", &self.info())
            .finish()
    }
}

/// Mutable view of a peer.
pub struct PeerMut<'a, T> {
    peer: *mut ENetPeer,
    disconnecting: bool,
    _data: PhantomData<T>,
    _host: PhantomData<&'a mut Host<T>>,
}

impl<T> PeerMut<'_, T> {
    fn drop_data(&mut self) {
        unsafe {
            let peer = &mut *self.peer;

            Box::from_raw(peer.data as *mut T);
            peer.data = ptr::null_mut();
        }
    }

    pub(crate) unsafe fn disconnecting(peer: *mut ENetPeer) -> Self {
        Self {
            peer,
            disconnecting: true,
            _data: PhantomData,
            _host: PhantomData,
        }
    }
}

impl<T: Default> PeerMut<'_, T> {
    pub(crate) unsafe fn from_raw(peer: *mut ENetPeer) -> Self {
        Self {
            peer,
            disconnecting: false,
            _data: PhantomData,
            _host: PhantomData,
        }
    }

    pub(crate) unsafe fn connecting(peer: *mut ENetPeer) -> Self {
        let peer = &mut *peer;
        peer.data = Box::leak(Box::new(T::default())) as *mut _ as *mut _;

        Self {
            peer,
            disconnecting: false,
            _data: PhantomData,
            _host: PhantomData,
        }
    }

    fn into_raw(self) -> *mut ENetPeer {
        self.peer
    }

    /// Request a disconnection from a peer.
    ///
    /// An [EventKind::Disconnect](crate::event::EventKind::Disconnect) will be generated by [Host::service](crate::host::Host::service) once the disconnection is complete.
    pub fn disconnect(self, data: u32) {
        unsafe {
            enet_sys::enet_peer_disconnect(self.into_raw(), data);
        }
    }

    /// Request a disconnection from a peer, but only after all queued outgoing packets are sent.
    ///
    /// An [EventKind::Disconnect](crate::event::EventKind::Disconnect) will be generated by [Host::service](crate::host::Host::service) once the disconnection is complete.
    pub fn disconnect_later(self, data: u32) {
        unsafe {
            enet_sys::enet_peer_disconnect_later(self.into_raw(), data);
        }
    }

    /// Force an immediate disconnection from a peer.
    ///
    /// No disconnect event will be generated and the data associated with this peer is dropped immediately.
    pub fn disconnect_now(mut self, data: u32) {
        self.drop_data();

        unsafe {
            enet_sys::enet_peer_disconnect_now(self.into_raw(), data);
        }
    }

    /// Sends a ping request to a peer.
    ///
    /// Ping requests factor into the mean round trip time as designated by [PeerInfo::round_trip_time](PeerInfo::round_trip_time). ENet automatically pings all connected peers at regular intervals, however, this function may be called to ensure more frequent ping requests.
    pub fn ping(&mut self) {
        unsafe {
            enet_sys::enet_peer_ping(self.peer);
        }
    }

    pub fn receive(&mut self) -> Option<Packet> {
        let mut channel_id = 0;

        let packet = unsafe { enet_sys::enet_peer_receive(self.peer, &mut channel_id as *mut _) };
        if packet.is_null() {
            return None;
        }

        Some(unsafe {
            // This unwrap will never fail because the existence of a peer implies the library has already been initialized.
            Packet::from_raw(packet, channel_id).unwrap()
        })
    }

    /// Forcefully disconnects a peer.
    ///
    /// The foreign host represented by the peer is not notified of the disconnection and will timeout on its connection to the local host.
    pub fn reset(self) {
        unsafe {
            enet_sys::enet_peer_reset(self.into_raw());
        }
    }

    /// Queues a packet to be sent.
    pub fn send(&mut self, packet: Packet) -> Result<(), Error> {
        let ret =
            unsafe { enet_sys::enet_peer_send(self.peer, packet.channel_id(), packet.into_raw()) };

        if ret < 0 {
            return Err(Error::Unknown);
        }

        Ok(())
    }

    /// Configures throttle parameter for a peer.
    ///
    /// Unreliable packets are dropped by ENet in response to the varying conditions of the Internet connection to the peer.
    /// The throttle represents a probability that an unreliable packet should not be dropped and thus sent by ENet to the peer.
    /// The lowest mean round trip time from the sending of a reliable packet to the receipt of its acknowledgement is measured
    /// over an amount of time specified by the interval parameter.
    /// If a measured round trip time happens to be significantly less than the mean round trip time measured over the interval,
    /// then the throttle probability is increased to allow more traffic by an amount specified in the acceleration parameter,
    /// which is a ratio to the [PACKET_THROTTLE_SCALE](PACKET_THROTTLE_SCALE) constant.
    /// If a measured round trip time happens to be significantly greater than the mean round trip time measured over the interval,
    /// then the throttle probability is decreased to limit traffic by an amount specified in the deceleration parameter,
    /// which is a ratio to the [PACKET_THROTTLE_SCALE](PACKET_THROTTLE_SCALE) constant.
    /// When the throttle has a value of [PACKET_THROTTLE_SCALE](PACKET_THROTTLE_SCALE), no unreliable packets are dropped by ENet, and so 100% of all unreliable packets will be sent.
    /// When the throttle has a value of 0, all unreliable packets are dropped by ENet, and so 0% of all unreliable packets will be sent. Intermediate values for the throttle represent intermediate probabilities between 0% and 100% of unreliable packets being sent.
    /// The bandwidth limits of the local and foreign hosts are taken into account to determine a sensible limit for the throttle probability above which it should not raise even in the best of conditions.
    pub fn configure_throttle(
        &mut self,
        interval: Duration,
        acceleration: u32,
        decceleration: u32,
    ) {
        unsafe {
            enet_sys::enet_peer_throttle_configure(
                self.peer,
                interval.as_millis().try_into().unwrap(),
                acceleration,
                decceleration,
            );
        }
    }

    pub fn set_timeout(
        &mut self,
        limit: Option<Duration>,
        min: Option<Duration>,
        max: Option<Duration>,
    ) {
        let millis = |duration: Option<Duration>| {
            duration
                .map(|duration| duration.as_millis().min(1))
                .unwrap_or(0)
                .try_into()
                .unwrap()
        };

        let limit = millis(limit);
        let min = millis(min);
        let max = millis(max);

        unsafe {
            enet_sys::enet_peer_timeout(self.peer, limit, min, max);
        }
    }
}

impl<T> PeerMut<'_, T> {
    /// Returns a reference to data associated with this peer.
    ///
    /// The data is created default-initialized for the first time a peer is returned from a [Host](crate::host::Host).
    pub fn data(&self) -> &T {
        unsafe { &*((*self.peer).data as *const T) }
    }

    /// Returns a mutable reference to data associated with this peer.
    ///
    /// The data is created default-initialized for the first time a peer is returned from a [Host](crate::host::Host).
    pub fn data_mut(&mut self) -> &mut T {
        unsafe { &mut *((*self.peer).data as *mut T) }
    }

    /// Returns information about this peer.
    pub fn info(&self) -> PeerInfo {
        PeerInfo {
            peer: unsafe { &*self.peer },
        }
    }
}

impl<T> Drop for PeerMut<'_, T> {
    fn drop(&mut self) {
        if self.disconnecting {
            self.drop_data();
        }
    }
}

impl<T: Debug> Debug for PeerMut<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peer")
            .field("data", self.data())
            .field("info", &self.info())
            .finish()
    }
}

/// Basic information about a peer.
#[derive(Clone, Copy)]
pub struct PeerInfo<'a> {
    peer: &'a ENetPeer,
}

impl Debug for PeerInfo<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("PeerInfo")
            .field("addr", &self.addr())
            .field("incoming_bandwidth", &self.incoming_bandwidth())
            .field("outgoing_bandwidth", &self.outgoing_bandwidth())
            .field("packet_loss", &self.packet_loss())
            .field("round_trip_time", &self.round_trip_time())
            .finish()
    }
}

impl PeerInfo<'_> {
    /// Remote address of the peer.
    pub fn addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(
            self.peer.address.host.to_ne_bytes().into(),
            self.peer.address.port,
        )
    }

    /// Incoming bandwith in bytes/second.
    pub fn incoming_bandwidth(&self) -> u32 {
        self.peer.incomingBandwidth
    }

    /// Outgoing bandwith in bytes/second.
    pub fn outgoing_bandwidth(&self) -> u32 {
        self.peer.outgoingBandwidth
    }

    /// Mean packet loss of reliable packets as a ratio with respect to the constant [PACKET_LOSS_SCALE].
    pub fn packet_loss(&self) -> u32 {
        self.peer.packetLoss
    }

    /// Mean round trip time (RTT) between sending a reliable packet and receiving its acknowledgement.
    pub fn round_trip_time(&self) -> Duration {
        Duration::from_millis(self.peer.roundTripTime as u64)
    }
}
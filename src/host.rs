use crate::compress::{Compressor, InputBuffer, OutputBuffer};
use crate::error::Error;
use crate::event::{Event, EventKind};
use crate::init::InitGuard;
use crate::packet::Packet;
use crate::peer::{Peer, PeerMut};

use core::slice;
use enet_sys::{ENetAddress, ENetBuffer, ENetCompressor, ENetEvent, ENetHost};
use libc::{c_void, size_t};
use std::any::Any;
use std::convert::TryInto;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::net::SocketAddrV4;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::time::Duration;

pub const MAXIMUM_CHANNEL_COUNT: usize = enet_sys::ENET_PROTOCOL_MAXIMUM_CHANNEL_COUNT as usize;

/// The host structure used for communicating with other peers.
pub struct Host<T> {
    host: *mut ENetHost,
    compressor_ctx: Box<CompressorCtx>,
    _marker: PhantomData<T>,
    _guard: InitGuard,
}

impl<T: Default> Host<T> {
    /// Create a new builder. Convenience function.
    pub fn builder() -> HostBuilder<T> {
        HostBuilder::default()
    }

    /// Broadcasts a packet to all peers associated with this host.
    pub fn broadcast(&mut self, packet: Packet) {
        unsafe {
            enet_sys::enet_host_broadcast(self.host, packet.channel_id(), packet.into_raw());
        }
    }

    /// Checks for any queued events on the host and dispatches one if available.
    pub fn check_events(&mut self) -> Result<Option<Event<'_, T>>, Error> {
        let mut event = MaybeUninit::uninit();

        let ret = unsafe { enet_sys::enet_host_check_events(self.host, event.as_mut_ptr()) };
        if ret < 0 {
            self.panic_check();
            return Err(Error::Unknown);
        }

        if ret == 0 {
            return Ok(None);
        }

        unsafe { Ok(translate_event(event.assume_init())) }
    }

    /// Initiates a connection to a foreign host.
    /// The peer returned will have not completed the connection until [Host::service](Host::service) notifies of an [EventKind::Connect](crate::event::EventKind::Connect) event for the peer.
    pub fn connect(
        &mut self,
        addr: SocketAddrV4,
        channel_count: usize,
        data: u32,
    ) -> Result<PeerMut<'_, T>, Error> {
        if channel_count == 0 {
            return Err(Error::InvalidArgument);
        }

        let addr = ENetAddress {
            host: u32::from_ne_bytes(addr.ip().octets()),
            port: addr.port(),
        };

        let peer = unsafe { enet_sys::enet_host_connect(self.host, &addr, channel_count, data) };
        if peer.is_null() {
            return Err(Error::Unknown);
        }

        Ok(unsafe { PeerMut::connecting(peer) })
    }

    /// Sends any queued packets on the host specified to its designated peers.
    // This function need only be used in circumstances where one wishes to send queued packets earlier than in a call to Host::service().
    pub fn flush(&mut self) {
        unsafe {
            enet_sys::enet_host_flush(self.host);
        }
    }

    /// Waits for events on the host specified and shuttles packets between the host and its peers.
    pub fn service(&mut self, timeout: Duration) -> Result<Option<Event<'_, T>>, Error> {
        let mut event = MaybeUninit::uninit();

        let ret = unsafe {
            enet_sys::enet_host_service(
                self.host,
                event.as_mut_ptr(),
                timeout.as_millis().try_into().unwrap(),
            )
        };

        if ret < 0 {
            self.panic_check();
            return Err(Error::Unknown);
        }

        Ok(unsafe { translate_event(event.assume_init()) })
    }

    /// Creates an iterator over all currently connected peers.
    pub fn peers(&self) -> impl Iterator<Item = Peer<'_, T>> {
        let host = unsafe { &*self.host };

        unsafe { slice::from_raw_parts(host.peers, host.peerCount) }
            .iter()
            .filter(|peer| !peer.data.is_null())
            .map(|peer| unsafe { Peer::from_raw(peer) })
    }

    /// Creates an iterator over all currently connected peers.
    pub fn peers_mut(&mut self) -> impl Iterator<Item = PeerMut<'_, T>> {
        let host = unsafe { &mut *self.host };

        unsafe { slice::from_raw_parts_mut(host.peers, host.peerCount) }
            .iter_mut()
            .filter(|peer| !peer.data.is_null())
            .map(|peer| unsafe { PeerMut::from_raw(peer) })
    }

    fn panic_check(&mut self) {
        if let Some(panic) = self.compressor_ctx.panic.take() {
            panic::resume_unwind(panic);
        }
    }

    fn set_compressor(&mut self, kind: Option<CompressorKind>) -> Result<(), Error> {
        match kind {
            Some(CompressorKind::Custom(compressor)) => {
                self.compressor_ctx.compressor = Some(compressor);

                let enet_compressor = ENetCompressor {
                    compress: Some(compress),
                    context: self.compressor_ctx.as_mut() as *mut CompressorCtx as *mut _,
                    decompress: Some(decompress),
                    destroy: Some(destroy),
                };

                unsafe {
                    enet_sys::enet_host_compress(self.host, &enet_compressor as *const _);
                }
            }
            Some(CompressorKind::RangeCoder) => {
                if unsafe { enet_sys::enet_host_compress_with_range_coder(self.host) } < 0 {
                    return Err(Error::Unknown);
                }

                self.compressor_ctx.compressor = None;
            }
            None => {
                unsafe {
                    enet_sys::enet_host_compress(self.host, ptr::null());
                }

                self.compressor_ctx.compressor = None;
            }
        }

        Ok(())
    }
}

impl<T: Debug + Default> Debug for Host<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

impl<T> Drop for Host<T> {
    fn drop(&mut self) {
        unsafe {
            let host = &*self.host;

            for i in 0..host.peerCount {
                PeerMut::<T>::disconnecting(host.peers.add(i));
            }

            enet_sys::enet_host_destroy(self.host);
        }
    }
}

#[derive(Default)]
pub struct HostBuilder<T> {
    addr: Option<SocketAddrV4>,
    peer_count: Option<usize>,
    channel_limit: Option<usize>,
    incoming_bandwidth: Option<u32>,
    outgoing_bandwidth: Option<u32>,
    compressor_kind: Option<CompressorKind>,
    _data: PhantomData<T>,
}

impl<T: Default> HostBuilder<T> {
    /// The address to listen on.
    /// By default, no address is set and thus the host can't be used as a server.
    pub fn addr(mut self, value: SocketAddrV4) -> Self {
        self.addr = Some(value);
        self
    }

    /// The maximum number of peers to allocate for the host. Default is 1.
    ///
    /// The value has to be non-zero.
    pub fn peer_count(mut self, value: usize) -> Self {
        self.peer_count = Some(value);
        self
    }

    /// The maximum number of channels to allocate for the host. Default is [MAXIMUM_CHANNEL_COUNT](MAXIMUM_CHANNEL_COUNT).
    ///
    /// The value has to be non-zero.
    pub fn channel_limit(mut self, value: usize) -> Self {
        self.channel_limit = Some(value);
        self
    }

    /// Incoming bandwidth limit. Default is unlimited.
    ///
    /// The value has to be non-zero.
    pub fn incoming_bandwidth(mut self, value: u32) -> Self {
        self.incoming_bandwidth = Some(value);
        self
    }

    /// Outgoing bandwidth limit. Default is unlimited.
    ///
    /// The value has to be non-zero.
    pub fn outgoing_bandwidth(mut self, value: u32) -> Self {
        self.outgoing_bandwidth = Some(value);
        self
    }

    /// Packet compressor. Default is uncompressed.
    pub fn compressor(mut self, value: CompressorKind) -> Self {
        self.compressor_kind = Some(value);
        self
    }

    /// Try to create a host based on the configuration.
    pub fn build(self) -> Result<Host<T>, Error> {
        let addr = self.addr.map(|addr| ENetAddress {
            host: u32::from_ne_bytes(addr.ip().octets()),
            port: addr.port(),
        });

        let peer_count = match self.peer_count {
            Some(0) => return Err(Error::InvalidArgument),
            Some(peer_count) => peer_count,
            None => 1,
        };

        let channel_limit = match self.channel_limit {
            Some(0) => return Err(Error::InvalidArgument),
            Some(channel_limit) => channel_limit,
            None => 1,
        };

        let incoming_bandwidth = match self.incoming_bandwidth {
            Some(0) => return Err(Error::InvalidArgument),
            Some(incoming_bandwidth) => incoming_bandwidth,
            None => 1,
        };

        let outgoing_bandwidth = match self.outgoing_bandwidth {
            Some(0) => return Err(Error::InvalidArgument),
            Some(outgoing_bandwidth) => outgoing_bandwidth,
            None => 1,
        };

        let guard = InitGuard::new()?;
        let host = unsafe {
            enet_sys::enet_host_create(
                addr.as_ref()
                    .map(|addr| addr as *const _)
                    .unwrap_or(ptr::null()),
                peer_count,
                channel_limit,
                incoming_bandwidth,
                outgoing_bandwidth,
            )
        };

        if host.is_null() {
            return Err(Error::Unknown);
        }

        let mut host = Host {
            host,
            compressor_ctx: Box::new(CompressorCtx {
                compressor: None,
                panic: None,
            }),
            _marker: PhantomData,
            _guard: guard,
        };

        host.set_compressor(self.compressor_kind)?;

        Ok(host)
    }
}

struct CompressorCtx {
    compressor: Option<Box<dyn Compressor + 'static>>,
    panic: Option<Box<dyn Any + Send>>,
}

/// Compressor for a host.
pub enum CompressorKind {
    /// A custom compressor.
    Custom(Box<dyn Compressor>),
    /// The ENet builtin range coder.
    RangeCoder,
}

unsafe extern "C" fn compress(
    context: *mut c_void,
    input_buffers: *const ENetBuffer,
    input_buffers_length: size_t,
    _input_limit: size_t,
    output_buffer: *mut u8,
    output_buffer_length: size_t,
) -> size_t {
    let ctx: &mut CompressorCtx = &mut *(context as *mut _);

    let input_buffers =
        slice::from_raw_parts(input_buffers as *const InputBuffer, input_buffers_length);

    let mut output_buffer = OutputBuffer {
        buffer: output_buffer,
        length: output_buffer_length,
        written: 0,
    };

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        ctx.compressor
            .as_mut()
            .unwrap()
            .decompress(input_buffers, &mut output_buffer)
    }));

    match result {
        Ok(Ok(_)) => output_buffer.written(),
        Ok(Err(_)) => 0,
        Err(err) => {
            ctx.panic = Some(err);
            0
        }
    }
}

unsafe extern "C" fn decompress(
    context: *mut c_void,
    input_buffer: *const u8,
    input_buffer_length: size_t,
    output_buffer: *mut u8,
    output_buffer_length: size_t,
) -> size_t {
    let ctx: &mut CompressorCtx = &mut *(context as *mut _);

    let input_buffer = InputBuffer {
        buffer: ENetBuffer {
            data: input_buffer as *mut _,
            dataLength: input_buffer_length,
        },
    };

    let mut output_buffer = OutputBuffer {
        buffer: output_buffer,
        length: output_buffer_length,
        written: 0,
    };

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        ctx.compressor
            .as_deref_mut()
            .unwrap()
            .compress(&[input_buffer], &mut output_buffer)
    }));

    match result {
        Ok(Ok(_)) => output_buffer.written(),
        Ok(Err(_)) => 0,
        Err(err) => {
            ctx.panic = Some(err);
            0
        }
    }
}

unsafe extern "C" fn destroy(_context: *mut c_void) {}

unsafe fn translate_event<'a, T: Default>(event: ENetEvent) -> Option<Event<'a, T>> {
    let (kind, peer) = match event.type_ {
        enet_sys::_ENetEventType_ENET_EVENT_TYPE_NONE => return None,
        enet_sys::_ENetEventType_ENET_EVENT_TYPE_CONNECT => (
            EventKind::Connect(event.data),
            PeerMut::connecting(event.peer),
        ),
        enet_sys::_ENetEventType_ENET_EVENT_TYPE_DISCONNECT => (
            EventKind::Disconnect(event.data),
            PeerMut::disconnecting(event.peer),
        ),
        enet_sys::_ENetEventType_ENET_EVENT_TYPE_RECEIVE => (
            EventKind::Receive(Packet::from_raw(event.packet, event.channelID).unwrap()),
            PeerMut::from_raw(event.peer),
        ),
        _ => unreachable!(),
    };

    Some(Event { peer, kind })
}
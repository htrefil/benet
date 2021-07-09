use enet_sys::ENetBuffer;
use std::fmt::{self, Debug, Formatter};
use std::io::{self, ErrorKind, Write};
use std::slice;

/// Generic packet (de)compression error.
///
/// This type implements `From<std::io::Error>` so that it's possible to use the `?` operator
/// to conveniently call functions returning `Result<T, std::io::Error>` in (de)compressor methods.
#[derive(Clone, Copy, Debug)]
pub struct Error;

impl From<io::Error> for Error {
    fn from(_err: io::Error) -> Self {
        Self
    }
}

pub trait Compressor {
    /// Compress input buffers into an output buffer.
    ///
    /// Not writing anything into the output buffer will cause [`Host::service`](crate::host::Host::service) and similar methods to return an error.
    fn compress(
        &mut self,
        input_buffers: &[InputBuffer],
        output_buffer: &mut OutputBuffer,
    ) -> Result<(), Error>;

    /// Decompress input buffers into an output buffer.
    ///
    /// Not writing anything into the output buffer will cause [`Host::service`](crate::host::Host::service) and similar methods to return an error.
    fn decompress(
        &mut self,
        input_buffers: &[InputBuffer],
        output_buffer: &mut OutputBuffer,
    ) -> Result<(), Error>;
}

/// Compression input buffer, essentially a fancy byte slice.
///
/// Use `.as_ref()` to get access to the contained data.
#[repr(transparent)]
pub struct InputBuffer {
    pub(crate) buffer: ENetBuffer,
}

impl Debug for InputBuffer {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("InputBuffer")
            .field("data", &self.as_ref())
            .finish()
    }
}

impl AsRef<[u8]> for InputBuffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.buffer.data as *const u8, self.buffer.dataLength) }
    }
}

/// (De)compression output buffer. Use the [`std::io::Write`] implementation to write processed data.
pub struct OutputBuffer {
    pub(crate) buffer: *mut u8,
    pub(crate) length: usize,
    pub(crate) written: usize,
}

impl Debug for OutputBuffer {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("OutputBuffer")
            .field("written", unsafe {
                &slice::from_raw_parts(self.buffer, self.written)
            })
            .field("remaining", &(self.length - self.written))
            .finish_non_exhaustive()
    }
}

impl OutputBuffer {
    /// Total buffer length.
    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length != 0
    }

    /// Number of written bytes.
    pub(crate) fn written(&self) -> usize {
        self.written
    }
}

impl Write for OutputBuffer {
    /// Write processed data to a buffer.
    /// Writing more than `self.len()` bytes will result in `Err(ErrorKind::WriteZero)` being returned.
    fn write(&mut self, data: &[u8]) -> Result<usize, io::Error> {
        if self.written + data.len() > self.length {
            return Err(ErrorKind::WriteZero.into());
        }

        for (i, b) in data.iter().copied().enumerate() {
            unsafe {
                self.buffer.add(i).write(b);
            }
        }

        Ok(data.len())
    }

    /// Calling this function has no effect.
    fn flush(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}

use std::error;
use std::fmt::{self, Display, Formatter};
use std::io;

/// The error type potentially returned by many functions in this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Library initialization failed.
    Init,
    /// An invalid argument was passed to a function.
    InvalidArgument,
    /// Standard IO error.
    Io(io::Error),
    /// Unspecified error from ENet.
    Unknown,
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match &self {
            Self::Init => write!(f, "Library initialization failed"),
            Self::InvalidArgument => write!(f, "Invalid argument"),
            Self::Io(err) => write!(f, "{}", err),
            Self::Unknown => write!(f, "Unspecified error"),
        }
    }
}

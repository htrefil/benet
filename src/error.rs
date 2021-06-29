use std::fmt::{self, Display, Formatter};

/// The error type potentially returned by many functions in this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Library initialization failed.
    Init,
    /// An invalid argument was passed to a function.
    InvalidArgument,
    /// Unspecified error from ENet.
    Unknown,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match &self {
            Self::Init => write!(f, "Library initialization failed"),
            Self::Unknown => write!(f, "Unspecified error"),
            Self::InvalidArgument => write!(f, "Invalid argument"),
        }
    }
}

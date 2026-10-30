//! Shared error type: malformed input returns [`Error`], never panics.

/// The crate's error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Format violations (magic, bounds, CRC, grammar).
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Format(m) => write!(f, "format: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Construct a format-level error (the parser's only variant).
    pub fn fmt_msg(m: impl Into<String>) -> Self {
        Error::Format(m.into())
    }
}

/// Crate result alias.
pub type Result<T> = std::result::Result<T, Error>;

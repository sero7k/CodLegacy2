//! Errors carry the support-page codes (L1 to L6) so a player can look them up.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// L1: the launcher key is missing or malformed.
    KeyMissing,
    /// L2: the key was revoked or replaced.
    KeyCancelled,
    /// L3: the account service could not be reached.
    Unreachable,
    /// L4: the Black Ops II files were not found.
    GameNotFound,
    /// L5: a file does not match its checksum.
    BadChecksum,
    /// L6: the join ticket expired or was already used.
    TicketRefused,
    /// Anything else (disk, bad manifest); no support code.
    Other,
}

impl Fault {
    pub fn code(self) -> &'static str {
        match self {
            Fault::KeyMissing => "L1",
            Fault::KeyCancelled => "L2",
            Fault::Unreachable => "L3",
            Fault::GameNotFound => "L4",
            Fault::BadChecksum => "L5",
            Fault::TicketRefused => "L6",
            Fault::Other => "",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub fault: Fault,
    pub message: String,
}

impl Error {
    pub fn new(fault: Fault, message: impl Into<String>) -> Self {
        Error { fault, message: message.into() }
    }
    pub fn other(message: impl Into<String>) -> Self {
        Error::new(Fault::Other, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.fault.code() {
            "" => write!(f, "{}", self.message),
            code => write!(f, "{} ({code})", self.message),
        }
    }
}

impl std::error::Error for Error {}

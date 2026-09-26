//! The command line's side of the channel: sending a command to the instance.

use std::fmt;
use std::io::{self, BufReader, Write as _};
use std::time::Duration;

use chartreuse_core::flavor;

use super::protocol::{self, ProtocolError};
use super::server::ANSWER_TIMEOUT;
use super::transport::{self, Stream};
use super::Command;

/// How long the command line waits for the reply: longer than the instance
/// waits for the app, so that its explanation arrives.
const REPLY_TIMEOUT: Duration = ANSWER_TIMEOUT.saturating_add(Duration::from_secs(10));

/// Why a command did not reach the instance, or was refused by it.
#[derive(Debug)]
pub enum SendError {
    /// Talking to the instance failed.
    Io(io::Error),
    /// The command cannot be written in the protocol, or the reply made no
    /// sense.
    Protocol(ProtocolError),
    /// The instance refused the command, for this reason.
    Refused(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                write!(f, "the running {} did not answer", flavor::DISPLAY_NAME)
            }
            Self::Io(error) => write!(
                f,
                "could not talk to the running {}: {error}",
                flavor::DISPLAY_NAME
            ),
            Self::Protocol(error) => error.fmt(f),
            Self::Refused(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Refused(_) => None,
        }
    }
}

impl From<io::Error> for SendError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ProtocolError> for SendError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

/// Sends `command` over `stream` (from [`connect_or_claim`](super::connect_or_claim))
/// and waits for the instance to take it.
///
/// # Errors
///
/// See [`SendError`].
pub fn send(mut stream: Stream, command: &Command) -> Result<(), SendError> {
    let request = protocol::encode_request(command)?;
    transport::set_timeout(&stream, REPLY_TIMEOUT)?;
    stream.write_all(&request)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let line = protocol::read_line(&mut reader)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the connection closed without a reply",
        )
    })?;
    protocol::decode_reply(&line)?.map_err(SendError::Refused)
}

//! One instance per user, and commands from the command line. Owned by
//! track 4C.
//!
//! One Chartreuse runs per user and flavor: the *instance*. A
//! `chartreuse capture {display,window,rectangle}` or `chartreuse open <file>`
//! started while it runs sends it the [`Command`] and exits, non-zero if the
//! instance refused it; with none running, the process becomes the instance
//! and runs the command once the app has booted.
//!
//! # Single instance
//!
//! Before iced starts, [`connect_or_claim`] settles the process's [`Role`]
//! through the [`Endpoint`] of this user and flavor:
//!
//! - **Unix** (macOS, Linux): a Unix domain socket, `ipc.sock`, beside a lock
//!   file, `instance.lock`, in a directory named after the bundle id:
//!   `~/Library/Caches/<bundle id>/` on macOS, `$XDG_RUNTIME_DIR/<bundle id>/`
//!   on Linux (the cache folder if that is unset). The process holding the
//!   lock is the instance and owns the socket. The kernel releases the lock
//!   however the process ends, so it never goes stale. The socket file stays
//!   behind when the instance ends, crashed or quit, and the next instance
//!   replaces it. Deciding by the lock rather than by whether the socket
//!   answers also settles two processes starting at once (a shortcut pressed
//!   twice): exactly one gets the lock, and the other connects to it.
//! - **Windows**: the named pipe `\\.\pipe\<bundle id>-<user>`, which only one
//!   process can create at a time and which vanishes with its process. It
//!   rejects remote clients, and clients do not talk to it if another user's
//!   process serves it.
//!
//! The wire format is in [`protocol`]: one request line and one reply line
//! per connection.
//!
//! # In the app
//!
//! The instance accepts clients on background threads ([`serve`]). Each
//! request arrives in `update` as [`Message::Received`] and is answered there:
//! a capture is refused while another one is in progress; anything else is
//! taken and runs as the message the status item sends for the same action
//! ([`Command::message`]), unless its client already stopped waiting and was
//! told that it was not taken. What happens next, such as a failed capture
//! or a file that does not decode, is reported in the app as for the status
//! item. A command given to the process that became the instance runs from
//! [`boot`].

pub mod protocol;

mod client;
#[cfg(windows)]
mod pipe;
mod server;
#[cfg(unix)]
mod socket;

#[cfg(windows)]
use pipe as transport;
#[cfg(unix)]
use socket as transport;

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use chartreuse_core::capture::CaptureMode;
use chartreuse_platform::EventReceiver;
use iced::{Subscription, Task};

pub use client::{send, SendError};
pub use server::{serve, Answer, Request};
pub use transport::{Endpoint, Listener, Stream};

use crate::app::{App, Message as AppMessage};
use crate::{capture, events, import};

/// How long [`connect_or_claim`] keeps trying while another process is the
/// instance but not listening: it is starting up or quitting.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

/// The pause between those tries.
const RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// What the command line asks the instance to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start a capture in this mode.
    Capture(CaptureMode),
    /// Open the image file at this path in an editor. The path is absolute:
    /// the instance's working directory is not the command line's.
    Open(PathBuf),
}

impl Command {
    /// The message that carries the command out: the one the status item
    /// sends for the same action.
    #[must_use]
    pub fn message(self) -> AppMessage {
        match self {
            Self::Capture(mode) => AppMessage::Capture(capture::Message::Start(mode)),
            Self::Open(path) => AppMessage::Import(import::Message::OpenPath(path)),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capture(mode) => write!(f, "capture {}", protocol::mode_word(*mode)),
            Self::Open(path) => write!(f, "open {}", path.display()),
        }
    }
}

/// This process's part, settled by [`connect_or_claim`].
#[derive(Debug)]
pub enum Role {
    /// No instance was running: this process is the instance and serves this
    /// listener.
    Instance(Listener),
    /// An instance is running: the connection to it.
    Client(Stream),
}

/// Becomes the instance at `endpoint`, or connects to the one running there.
///
/// # Errors
///
/// Claiming or connecting failed, or the instance did not start listening
/// within a few seconds.
pub fn connect_or_claim(endpoint: &Endpoint) -> io::Result<Role> {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    loop {
        if let Some(listener) = transport::claim(endpoint)? {
            return Ok(Role::Instance(listener));
        }
        match transport::connect(endpoint) {
            Ok(stream) => return Ok(Role::Client(stream)),
            Err(error) if transport::is_absent(&error) && Instant::now() < deadline => {
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

/// This feature's part of the app state ([`App::ipc`]).
#[derive(Debug, Default)]
pub struct State {
    /// Requests from other processes, while this process is the instance.
    requests: Option<EventReceiver<Request>>,
    /// The command on this process's command line, until [`boot`] runs it.
    command: Option<Command>,
}

impl State {
    /// The state of an instance serving `requests` (from [`serve`]; `None`
    /// without a channel), which runs `command` once booted.
    #[must_use]
    pub const fn new(requests: Option<EventReceiver<Request>>, command: Option<Command>) -> Self {
        Self { requests, command }
    }
}

/// This feature's messages ([`AppMessage::Ipc`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Another process sent this request.
    Received(Request),
}

pub fn boot(app: &mut App) -> Task<AppMessage> {
    match app.ipc.command.take() {
        Some(command) => {
            tracing::info!(%command, "running the command from the command line");
            Task::done(command.message())
        }
        None => Task::none(),
    }
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Received(request) => {
            let command = request.command();
            if let Some(reason) = refusal(app, command) {
                tracing::info!(%command, reason, "refused a command from the command line");
                request.answer(Err(reason));
                return Task::none();
            }
            if !request.answer(Ok(())) {
                tracing::info!(%command, "dropped a command its client stopped waiting for");
                return Task::none();
            }
            tracing::info!(%command, "received a command from the command line");
            Task::done(request.into_command().message())
        }
    }
}

pub fn subscription(app: &App) -> Subscription<AppMessage> {
    match &app.ipc.requests {
        Some(requests) => events::subscription(requests)
            .map(|request| AppMessage::Ipc(Message::Received(request))),
        None => Subscription::none(),
    }
}

/// Why the app will not take `command` now, if it will not.
fn refusal(app: &App, command: &Command) -> Option<String> {
    match command {
        Command::Capture(_) => app
            .capture
            .in_progress()
            .map(|_| "a capture is already in progress".to_owned()),
        Command::Open(_) => None,
    }
}

#[cfg(test)]
mod tests;

//! The instance's side of the channel: accepting commands and handing them to
//! the app.

use std::io::{self, BufReader, Write as _};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread;
use std::time::Duration;

use chartreuse_core::flavor;
use chartreuse_platform::event::{self, EventReceiver, EventSender};

use super::transport::{self, Listener, Stream};
use super::{protocol, Command};

/// How long a client may take to send its request, or to take the reply.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the app has to answer a request before the client is told it did
/// not. Clients wait longer than this for the reply.
pub(super) const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);

/// The pause after a failed accept, so that a lasting failure does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// The app's answer to a request: `Ok` if it took the command, or why not.
type Reply = Result<(), String>;

/// Where the app gives its answer, once: `None` after that, or once the
/// client stopped waiting.
type ReplySlot = Mutex<Option<mpsc::SyncSender<Reply>>>;

/// A command from another process, waiting for the app's answer.
///
/// Cloning shares the answer: the first one given counts.
#[derive(Debug, Clone)]
pub struct Request {
    command: Command,
    reply: Arc<ReplySlot>,
}

impl Request {
    /// A request for `command`, and where its answer arrives.
    #[must_use]
    pub fn new(command: Command) -> (Self, Answer) {
        let (reply, answer) = mpsc::sync_channel(1);
        let reply = Arc::new(Mutex::new(Some(reply)));
        let answer = Answer {
            answer,
            reply: Arc::downgrade(&reply),
        };
        (Self { command, reply }, answer)
    }

    /// The command requested.
    #[must_use]
    pub const fn command(&self) -> &Command {
        &self.command
    }

    /// Tells the client whether the app took the command: `Ok`, or why not.
    /// Returns whether the client gets this answer: not if it stopped
    /// waiting (and was told the command was not taken), or if a clone
    /// answered first.
    pub fn answer(&self, answer: Reply) -> bool {
        let mut reply = self.reply.lock().unwrap_or_else(PoisonError::into_inner);
        // Sent under the lock, which the client stops waiting under.
        reply
            .take()
            .is_some_and(|reply| reply.try_send(answer).is_ok())
    }

    /// The command requested.
    #[must_use]
    pub fn into_command(self) -> Command {
        self.command
    }
}

/// The app's answer to a [`Request`].
#[derive(Debug)]
pub struct Answer {
    answer: mpsc::Receiver<Reply>,
    reply: Weak<ReplySlot>,
}

impl Answer {
    /// Waits up to `timeout` for the answer. A request dropped unanswered, or
    /// not answered in time, is refused; once this returns, the app can no
    /// longer answer.
    ///
    /// # Errors
    ///
    /// The reason the command was refused.
    pub fn wait(self, timeout: Duration) -> Reply {
        match self.answer.recv_timeout(timeout) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => {
                // Stop the app from answering, under the lock it answers
                // under; an answer given before that still counts.
                if let Some(reply) = self.reply.upgrade() {
                    reply.lock().unwrap_or_else(PoisonError::into_inner).take();
                }
                self.answer.try_recv().unwrap_or_else(|_| {
                    Err(format!(
                        "{} did not take the command in time",
                        flavor::DISPLAY_NAME
                    ))
                })
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(format!("{} dropped the command", flavor::DISPLAY_NAME))
            }
        }
    }
}

/// Serves `listener` on background threads, one per client. Each request
/// arrives through the returned receiver, and its client gets the answer
/// (or a malformed request its error) as the reply.
///
/// # Errors
///
/// The accepting thread could not be started.
pub fn serve(listener: Listener) -> io::Result<EventReceiver<Request>> {
    let (sender, requests) = event::channel();
    thread::Builder::new()
        .name("ipc".into())
        .spawn(move || accept_loop(listener, &sender))?;
    Ok(requests)
}

fn accept_loop(mut listener: Listener, sender: &EventSender<Request>) {
    loop {
        match listener.accept() {
            Ok(stream) => {
                let sender = sender.clone();
                let spawned = thread::Builder::new()
                    .name("ipc client".into())
                    .spawn(move || {
                        if let Err(error) = serve_client(stream, &sender) {
                            tracing::warn!(%error, "serving a command-line client failed");
                        }
                    });
                if let Err(error) = spawned {
                    tracing::warn!(%error, "could not serve a command-line client");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "accepting a command-line client failed");
                thread::sleep(ACCEPT_BACKOFF);
            }
        }
    }
}

/// Reads the client's request, hands it to the app, and writes the reply.
fn serve_client(stream: Stream, sender: &EventSender<Request>) -> io::Result<()> {
    // Fails (EINVAL on macOS) for a client that already left, such as one
    // that only checked that the instance runs; reading then just ends.
    if let Err(error) = transport::set_timeout(&stream, IO_TIMEOUT) {
        tracing::debug!(%error, "cannot time the command-line client out");
    }
    let mut reader = BufReader::new(stream);
    let Some(line) = protocol::read_line(&mut reader)? else {
        tracing::debug!("a command-line client checked that the instance runs");
        return Ok(());
    };
    let reply = match protocol::decode_request(&line) {
        Ok(command) => deliver(command, sender),
        Err(error) => {
            tracing::warn!(%error, "refused a malformed request");
            Err(error.to_string())
        }
    };
    let stream = reader.get_mut();
    stream.write_all(&protocol::encode_reply(&reply))?;
    stream.flush()
}

/// Hands `command` to the app and waits for its answer.
fn deliver(command: Command, sender: &EventSender<Request>) -> Result<(), String> {
    let (request, answer) = Request::new(command);
    if !sender.send(request) {
        return Err(format!("{} is quitting", flavor::DISPLAY_NAME));
    }
    answer.wait(ANSWER_TIMEOUT)
}

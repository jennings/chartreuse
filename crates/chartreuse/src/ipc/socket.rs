//! The channel on Unix (macOS, Linux): a Unix domain socket, owned by
//! whichever process holds the lock file next to it.

use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use chartreuse_core::flavor;

/// A connection between the command line and the instance.
pub type Stream = UnixStream;

/// Where the instance of one flavor listens: a per-user directory holding
/// `instance.lock` and `ipc.sock`.
#[derive(Debug, Clone)]
pub struct Endpoint {
    directory: PathBuf,
}

impl Endpoint {
    /// This user's endpoint for this build's flavor, in a directory named
    /// after the bundle id: in `~/Library/Caches` on macOS; in
    /// `$XDG_RUNTIME_DIR` elsewhere, or the cache folder (`~/.cache`) if that
    /// is unset.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if there is no such folder (no home).
    pub fn current() -> io::Result<Self> {
        let base = if cfg!(target_os = "macos") {
            dirs::cache_dir()
        } else {
            dirs::runtime_dir().or_else(dirs::cache_dir)
        };
        base.map(|base| Self::in_directory(base.join(flavor::BUNDLE_ID)))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "there is no per-user folder for the instance's socket",
                )
            })
    }

    /// The endpoint in `directory`, which is created (private to the user)
    /// when an instance claims it.
    #[must_use]
    pub const fn in_directory(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub(super) fn socket(&self) -> PathBuf {
        self.directory.join("ipc.sock")
    }

    fn lock(&self) -> PathBuf {
        self.directory.join("instance.lock")
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.socket().display().fmt(f)
    }
}

/// The instance's listening socket, with the lock that makes it the instance.
///
/// Nothing removes the socket file when the instance ends; the next instance
/// replaces it.
#[derive(Debug)]
pub struct Listener {
    listener: UnixListener,
    /// Held open for the listener's life; the kernel releases the lock when
    /// it closes, including when the process dies.
    _lock: File,
}

impl Listener {
    /// Waits for the next client.
    ///
    /// # Errors
    ///
    /// As `accept(2)`.
    pub fn accept(&mut self) -> io::Result<Stream> {
        self.listener.accept().map(|(stream, _)| stream)
    }
}

/// Claims the instance: takes the lock and listens, unless another process
/// holds the lock (`None`).
///
/// The lock file is never removed: a process that locked a removed file
/// would think itself the instance beside the one locking its replacement.
///
/// # Errors
///
/// Creating the directory, opening or locking the lock file, removing the
/// previous instance's socket, or binding the socket failed.
pub fn claim(endpoint: &Endpoint) -> io::Result<Option<Listener>> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&endpoint.directory)?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(endpoint.lock())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(error)) => return Err(error),
    }
    // Holding the lock, this process is the only instance: a socket file here
    // is the previous instance's, which nobody listens on any more.
    let socket = endpoint.socket();
    match fs::remove_file(&socket) {
        Ok(()) => {
            tracing::debug!(path = %socket.display(), "removed the previous instance's socket")
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&socket)?;
    Ok(Some(Listener {
        listener,
        _lock: lock,
    }))
}

/// Connects to the instance.
///
/// # Errors
///
/// As `connect(2)`; see [`is_absent`].
pub fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    UnixStream::connect(endpoint.socket())
}

/// Whether a [`connect`] error means nothing is listening (yet, or any more).
pub fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// Bounds how long reads and writes on `stream` may block.
///
/// # Errors
///
/// As `setsockopt(2)`.
pub fn set_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

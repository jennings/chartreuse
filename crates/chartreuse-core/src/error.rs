//! The error type shared by every Chartreuse crate.

use std::sync::Arc;

use crate::hotkey::Hotkey;
use crate::permission::Permission;

/// A `Result` defaulting to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong in Chartreuse.
///
/// The `Display` text is a complete sentence fragment suitable for the user-facing
/// alert (`report_error` in the app crate). The type is `Clone` so it can travel in
/// iced messages.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The feature has no implementation on this platform yet. The payload names the
    /// feature, e.g. `"global hotkeys"`.
    #[error("{0} is not supported on this platform yet")]
    Unsupported(&'static str),
    #[error("Chartreuse does not have {0} permission")]
    PermissionDenied(Permission),
    /// Registering a global hotkey failed, typically because another program owns it.
    #[error("the hotkey {hotkey} could not be registered: {reason}")]
    HotkeyUnavailable { hotkey: Hotkey, reason: String },
    #[error("the clipboard does not contain an image")]
    ClipboardEmpty,
    /// An image buffer is malformed (wrong length, zero size, too large).
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("the image could not be decoded: {0}")]
    Decode(String),
    #[error("the image could not be encoded: {0}")]
    Encode(String),
    #[error("invalid settings: {0}")]
    Config(String),
    #[error("{context}: {source}")]
    Io {
        /// What was being attempted, e.g. `"writing /tmp/a.png"`.
        context: String,
        #[source]
        source: Arc<std::io::Error>,
    },
    /// An operating-system API failed; the payload describes the call and result.
    #[error("{0}")]
    Platform(String),
}

impl Error {
    /// Wraps an I/O error with a description of what was being attempted.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source: Arc::new(source),
        }
    }
}

//! Windows: small helpers shared by the Win32 backends.

use chartreuse_core::Error;

/// `s` as a NUL-terminated UTF-16 string, for `PCWSTR` arguments.
pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The text of a UTF-16 buffer up to its first NUL (or its end).
pub(super) fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// An [`Error::Platform`] for a failed Win32 or COM call.
pub(super) fn platform_error(context: &str, error: &::windows::core::Error) -> Error {
    Error::Platform(format!("{context}: {error}"))
}

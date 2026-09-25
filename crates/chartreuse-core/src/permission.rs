//! Operating-system privacy permissions.

use std::fmt;

/// A permission Chartreuse may need from the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// macOS Screen Recording (TCC), needed to capture anything but the wallpaper.
    ScreenRecording,
}

/// The user-facing permission name, as the OS settings show it.
impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ScreenRecording => "Screen Recording",
        })
    }
}

/// Whether a permission is currently granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionStatus {
    Granted,
    /// Not granted: never asked, refused, or revoked. macOS does not distinguish
    /// these for Screen Recording.
    Denied,
}

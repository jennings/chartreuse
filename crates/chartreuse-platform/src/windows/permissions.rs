//! Windows: privacy permissions.
//!
//! Windows asks desktop applications for no permission to capture the screen or
//! other windows (Windows.Graphics.Capture, `BitBlt`, and `PrintWindow` all just
//! work), so every permission is granted and there is no settings page to open.

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::Result;

use crate::permissions::Permissions;

/// The Windows [`Permissions`] backend.
#[derive(Debug, Default)]
pub struct WindowsPermissions;

impl WindowsPermissions {
    pub fn new() -> Self {
        Self
    }
}

impl Permissions for WindowsPermissions {
    fn status(&self, _permission: Permission) -> Result<PermissionStatus> {
        Ok(PermissionStatus::Granted)
    }

    fn request(&self, _permission: Permission) -> Result<PermissionStatus> {
        Ok(PermissionStatus::Granted)
    }

    fn open_settings(&self, permission: Permission) -> Result<()> {
        // Unreachable in practice: the app only offers this for a denied
        // permission, and nothing is ever denied here.
        tracing::debug!(%permission, "Windows has no settings for this permission");
        Ok(())
    }
}

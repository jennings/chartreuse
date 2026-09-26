//! X11: privacy permissions. X11 has none: every client may read the screen
//! and grab keys, so screen recording is always granted and there is no
//! settings page for it.

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::{Error, Result};

use crate::permissions::Permissions;

/// The X11 [`Permissions`] backend.
#[derive(Debug, Default)]
pub struct X11Permissions;

impl X11Permissions {
    pub fn new() -> Self {
        Self
    }
}

impl Permissions for X11Permissions {
    fn status(&self, _permission: Permission) -> Result<PermissionStatus> {
        Ok(PermissionStatus::Granted)
    }

    fn request(&self, permission: Permission) -> Result<PermissionStatus> {
        self.status(permission)
    }

    fn open_settings(&self, _permission: Permission) -> Result<()> {
        Err(Error::Unsupported(
            "opening the screen capture permission settings",
        ))
    }
}

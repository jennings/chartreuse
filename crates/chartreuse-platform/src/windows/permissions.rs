//! Windows: privacy permissions. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::{Error, Result};

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
        Err(Error::Unsupported("permission checking"))
    }

    fn request(&self, _permission: Permission) -> Result<PermissionStatus> {
        Err(Error::Unsupported("permission checking"))
    }

    fn open_settings(&self, _permission: Permission) -> Result<()> {
        Err(Error::Unsupported("permission checking"))
    }
}

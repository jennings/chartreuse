//! Wayland: privacy permissions. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::{Error, Result};

use crate::permissions::Permissions;

/// The Wayland [`Permissions`] backend.
#[derive(Debug, Default)]
pub struct WaylandPermissions;

impl WaylandPermissions {
    pub fn new() -> Self {
        Self
    }
}

impl Permissions for WaylandPermissions {
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

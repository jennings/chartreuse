//! X11: privacy permissions. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

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
        Err(Error::Unsupported("permission checking"))
    }

    fn request(&self, _permission: Permission) -> Result<PermissionStatus> {
        Err(Error::Unsupported("permission checking"))
    }

    fn open_settings(&self, _permission: Permission) -> Result<()> {
        Err(Error::Unsupported("permission checking"))
    }
}

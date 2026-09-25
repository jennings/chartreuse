//! Windows: the status item. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::status_item::{StatusItem, StatusItemHandle};

/// The Windows [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct WindowsStatusItem;

impl WindowsStatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for WindowsStatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        Err(Error::Unsupported("the status item"))
    }
}

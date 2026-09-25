//! Wayland: the status item. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::status_item::{StatusItem, StatusItemHandle};

/// The Wayland [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct WaylandStatusItem;

impl WaylandStatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for WaylandStatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        Err(Error::Unsupported("the status item"))
    }
}

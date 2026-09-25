//! macOS: the status item. Implemented by track 1A (`NSStatusItem`).
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::status_item::{StatusItem, StatusItemHandle};

/// The macOS [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct MacosStatusItem;

impl MacosStatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for MacosStatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        Err(Error::Unsupported("the status item"))
    }
}

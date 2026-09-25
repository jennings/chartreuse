//! X11: the status item. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::status_item::{StatusItem, StatusItemHandle};

/// The X11 [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct X11StatusItem;

impl X11StatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for X11StatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        Err(Error::Unsupported("the status item"))
    }
}

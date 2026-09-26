//! X11: the status item, a StatusNotifierItem tray icon shared with the
//! Wayland backend (see [`crate::linux::tray`]).

use chartreuse_core::Result;

use crate::linux::tray;
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
        tray::install()
    }
}

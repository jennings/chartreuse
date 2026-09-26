//! Wayland: the status item, a StatusNotifierItem tray icon shared with the
//! X11 backend (see [`crate::linux::tray`]).

use chartreuse_core::Result;

use crate::linux::tray;
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
        tray::install()
    }
}

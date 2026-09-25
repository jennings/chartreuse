//! Wayland: overlay window styling. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::overlay_style::{NativeWindow, OverlayWindowStyle};

/// The Wayland [`OverlayWindowStyle`] backend.
#[derive(Debug, Default)]
pub struct WaylandOverlayStyle;

impl WaylandOverlayStyle {
    pub fn new() -> Self {
        Self
    }
}

impl OverlayWindowStyle for WaylandOverlayStyle {
    fn apply(&self, _window: NativeWindow<'_>) -> Result<()> {
        Err(Error::Unsupported("overlay window styling"))
    }
}

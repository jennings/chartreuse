//! macOS: overlay window styling. Implemented by track 2C (`NSWindow` level and collection behavior).
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::overlay_style::{NativeWindow, OverlayWindowStyle};

/// The macOS [`OverlayWindowStyle`] backend.
#[derive(Debug, Default)]
pub struct MacosOverlayStyle;

impl MacosOverlayStyle {
    pub fn new() -> Self {
        Self
    }
}

impl OverlayWindowStyle for MacosOverlayStyle {
    fn apply(&self, _window: NativeWindow<'_>) -> Result<()> {
        Err(Error::Unsupported("overlay window styling"))
    }
}

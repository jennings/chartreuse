//! Windows: overlay window styling. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::overlay_style::{NativeWindow, OverlayWindowStyle};

/// The Windows [`OverlayWindowStyle`] backend.
#[derive(Debug, Default)]
pub struct WindowsOverlayStyle;

impl WindowsOverlayStyle {
    pub fn new() -> Self {
        Self
    }
}

impl OverlayWindowStyle for WindowsOverlayStyle {
    fn apply(&self, _window: NativeWindow<'_>) -> Result<()> {
        Err(Error::Unsupported("overlay window styling"))
    }
}

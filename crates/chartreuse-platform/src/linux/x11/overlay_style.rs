//! X11: overlay window styling. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::overlay_style::{NativeWindow, OverlayWindowStyle};

/// The X11 [`OverlayWindowStyle`] backend.
#[derive(Debug, Default)]
pub struct X11OverlayStyle;

impl X11OverlayStyle {
    pub fn new() -> Self {
        Self
    }
}

impl OverlayWindowStyle for X11OverlayStyle {
    fn apply(&self, _window: NativeWindow<'_>) -> Result<()> {
        Err(Error::Unsupported("overlay window styling"))
    }
}

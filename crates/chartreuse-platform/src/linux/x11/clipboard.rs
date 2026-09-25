//! X11: clipboard access. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};

use crate::clipboard::Clipboard;

/// The X11 [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct X11Clipboard;

impl X11Clipboard {
    pub fn new() -> Self {
        Self
    }
}

impl Clipboard for X11Clipboard {
    fn write_image(&self, _image: &Image) -> Result<()> {
        Err(Error::Unsupported("clipboard access"))
    }

    fn read_image(&self) -> Result<Option<Image>> {
        Err(Error::Unsupported("clipboard access"))
    }
}

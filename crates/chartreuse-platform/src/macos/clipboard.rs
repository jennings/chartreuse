//! macOS: clipboard access. Implemented by track 2B (`NSPasteboard`).
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};

use crate::clipboard::Clipboard;

/// The macOS [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct MacosClipboard;

impl MacosClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl Clipboard for MacosClipboard {
    fn write_image(&self, _image: &Image) -> Result<()> {
        Err(Error::Unsupported("clipboard access"))
    }

    fn read_image(&self) -> Result<Option<Image>> {
        Err(Error::Unsupported("clipboard access"))
    }
}

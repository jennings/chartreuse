//! X11: the `CLIPBOARD` selection (see [`crate::linux::clipboard`]).

use chartreuse_core::image::Image;
use chartreuse_core::Result;

use crate::clipboard::Clipboard;
use crate::linux::clipboard::SystemClipboard;

/// The X11 [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct X11Clipboard(SystemClipboard);

impl X11Clipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for X11Clipboard {
    fn write_image(&self, image: &Image) -> Result<()> {
        self.0.write_image(image)
    }

    fn read_image(&self) -> Result<Option<Image>> {
        self.0.read_image()
    }
}

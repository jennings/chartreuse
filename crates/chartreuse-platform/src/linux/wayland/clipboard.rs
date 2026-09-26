//! Wayland: the clipboard, through the data-control protocols (see
//! [`crate::linux::clipboard`]).

use chartreuse_core::image::Image;
use chartreuse_core::Result;

use crate::clipboard::Clipboard;
use crate::linux::clipboard::SystemClipboard;

/// The Wayland [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct WaylandClipboard(SystemClipboard);

impl WaylandClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for WaylandClipboard {
    fn write_image(&self, image: &Image) -> Result<()> {
        self.0.write_image(image)
    }

    fn read_image(&self) -> Result<Option<Image>> {
        self.0.read_image()
    }
}

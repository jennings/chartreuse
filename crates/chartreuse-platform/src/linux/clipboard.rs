//! The clipboard of both Linux backends, through `arboard`, which exchanges
//! images as `image/png`.
//!
//! - X11: the `CLIPBOARD` selection. arboard serves copied images from a
//!   background thread for as long as the clipboard handle lives (Chartreuse
//!   keeps it for the whole run) and hands them to the desktop's clipboard
//!   manager, if one runs, when it is dropped.
//! - Wayland (`WAYLAND_DISPLAY` set): the core `wl_data_device` clipboard
//!   needs keyboard focus and an input serial, which only the focused window's
//!   own connection (winit's) has. So arboard uses the data-control protocols
//!   (`ext-data-control`, `wlr-data-control`: KDE, wlroots compositors),
//!   which work without focus, and falls back to the X11 selection through
//!   XWayland where the compositor has neither (GNOME), which Mutter keeps in
//!   sync with the Wayland clipboard.

use std::borrow::Cow;

use chartreuse_core::geometry::PhysicalSize;
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use parking_lot::Mutex;

/// A lazily opened clipboard handle.
#[derive(Default)]
pub struct SystemClipboard {
    handle: Mutex<Option<arboard::Clipboard>>,
}

impl std::fmt::Debug for SystemClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClipboard").finish_non_exhaustive()
    }
}

impl SystemClipboard {
    pub fn write_image(&self, image: &Image) -> Result<()> {
        let data = arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: Cow::Borrowed(image.pixels()),
        };
        self.with(|clipboard| clipboard.set_image(data))
            .map_err(|error| failed("copying the image", &error))
    }

    pub fn read_image(&self) -> Result<Option<Image>> {
        let data = match self.with(arboard::Clipboard::get_image) {
            Ok(data) => data,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(arboard::Error::ConversionFailure) => {
                return Err(Error::Decode(
                    "the clipboard image is not a valid PNG".into(),
                ));
            }
            Err(error) => return Err(failed("pasting", &error)),
        };
        let size = |length: usize| {
            u32::try_from(length)
                .map_err(|_| Error::InvalidImage("clipboard image too large".into()))
        };
        let size = PhysicalSize::new(size(data.width)?, size(data.height)?);
        Image::new(size, data.bytes.into_owned()).map(Some)
    }

    fn with<T>(
        &self,
        action: impl FnOnce(&mut arboard::Clipboard) -> std::result::Result<T, arboard::Error>,
    ) -> std::result::Result<T, arboard::Error> {
        let mut handle = self.handle.lock();
        let clipboard = match &mut *handle {
            Some(clipboard) => clipboard,
            empty => empty.insert(arboard::Clipboard::new()?),
        };
        action(clipboard)
    }
}

fn failed(what: &str, error: &arboard::Error) -> Error {
    Error::Platform(format!("{what} failed: {error}"))
}

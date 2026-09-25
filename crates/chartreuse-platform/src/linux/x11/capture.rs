//! X11: display and window capture. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::image::Image;
use chartreuse_core::window::WindowId;
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use crate::capture::{Capture, DisplayCapture};

/// The X11 [`Capture`] backend.
#[derive(Debug, Default)]
pub struct X11Capture;

impl X11Capture {
    pub fn new() -> Self {
        Self
    }
}

impl Capture for X11Capture {
    fn capture_displays(&self) -> BoxFuture<'static, Result<Vec<DisplayCapture>>> {
        future::ready(Err(Error::Unsupported("screen capture"))).boxed()
    }

    fn capture_window(&self, _window: WindowId) -> BoxFuture<'static, Result<Image>> {
        future::ready(Err(Error::Unsupported("window capture"))).boxed()
    }
}

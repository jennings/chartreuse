//! X11: display capture, reading each monitor's area of the root window
//! (which holds what is on screen, composited or not) with MIT-SHM.

use chartreuse_core::image::Image;
use chartreuse_core::window::WindowId;
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use super::connection;
use super::displays::Desktop;
use super::image::Reader;
use crate::capture::{Capture, DisplayCapture};
use crate::linux::blocking;

/// The X11 [`Capture`] backend. X11 has no screen capture permission: every
/// client may read the screen.
#[derive(Debug, Default)]
pub struct X11Capture;

impl X11Capture {
    pub fn new() -> Self {
        Self
    }
}

impl Capture for X11Capture {
    fn capture_displays(&self) -> BoxFuture<'static, Result<Vec<DisplayCapture>>> {
        blocking::run("display capture", || {
            let x11 = connection::get()?;
            let desktop = Desktop::query(x11)?;
            let root = x11.root();
            let visual = x11.screen().root_visual;
            let mut reader = Reader::new(x11);
            desktop
                .displays()
                .map(|(area, display)| {
                    let image = reader.read(root, area, visual)?;
                    Ok(DisplayCapture { display, image })
                })
                .collect()
        })
    }

    fn capture_window(&self, _window: WindowId) -> BoxFuture<'static, Result<Image>> {
        future::ready(Err(Error::Unsupported("window capture"))).boxed()
    }
}

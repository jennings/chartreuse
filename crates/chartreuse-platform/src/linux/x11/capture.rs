//! X11: display capture, reading each monitor's area of the root window
//! (which holds what is on screen, composited or not) with MIT-SHM, and
//! window capture.
//!
//! Under a compositing manager a window is read from its Composite pixmap,
//! which holds all of it even where other windows cover it. Without one,
//! windows draw straight to the screen and only the root window has their
//! pixels: then the capture is the window's area of the screen, including
//! whatever covers it.

use chartreuse_core::geometry::{PhysicalPoint, PhysicalRect};
use chartreuse_core::image::Image;
use chartreuse_core::window::WindowId;
use chartreuse_core::{Error, Result};
use futures::future::BoxFuture;
use x11rb::connection::Connection as _;
use x11rb::protocol::composite::{ConnectionExt as _, Redirect};
use x11rb::protocol::xproto::{ConnectionExt as _, Window};

use super::connection::{self, failed, X11};
use super::displays::Desktop;
use super::image::Reader;
use super::window_list::describe_with_frame;
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

    fn capture_window(&self, window: WindowId) -> BoxFuture<'static, Result<Image>> {
        blocking::run("window capture", move || {
            let x11 = connection::get()?;
            let closed = || Error::Platform("the window has closed".into());
            let id = u32::try_from(window.0).map_err(|_| closed())?;
            let described = describe_with_frame(x11, id)?.ok_or_else(closed)?;
            let area = described.client.visible_area();
            let mut reader = Reader::new(x11);
            if composited(x11) {
                capture_offscreen(
                    x11,
                    &mut reader,
                    described.frame,
                    described.client.frame,
                    area,
                )
            } else {
                capture_on_screen(x11, &mut reader, area)
            }
        })
    }
}

/// True if a compositing manager runs: then every window is redirected to an
/// offscreen pixmap holding all of its contents, covered or not.
fn composited(x11: &X11) -> bool {
    x11.atom(&format!("_NET_WM_CM_S{}", x11.screen))
        .ok()
        .and_then(|selection| x11.conn.get_selection_owner(selection).ok())
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.owner != x11rb::NONE)
}

/// Reads `area` (root coordinates) of the `frame` window, whose outer bounds
/// are `bounds`, from its Composite pixmap: correct even where other windows
/// cover it.
fn capture_offscreen(
    x11: &X11,
    reader: &mut Reader<'_>,
    frame: Window,
    bounds: PhysicalRect,
    area: PhysicalRect,
) -> Result<Image> {
    let conn = &x11.conn;
    let what = "reading a window with Composite";
    conn.composite_query_version(0, 4)
        .map_err(|e| failed(what, e))?
        .reply()
        .map_err(|e| failed(what, e))?;
    // The compositing manager has redirected the window already; asking
    // again (automatically updated) is harmless and keeps its pixmap alive
    // for us even if the manager lets go meanwhile.
    let redirected = conn
        .composite_redirect_window(frame, Redirect::AUTOMATIC)
        .ok()
        .is_some_and(|cookie| cookie.check().is_ok());
    let visual = conn
        .get_window_attributes(frame)
        .map_err(|e| failed(what, e))?
        .reply()
        .map_err(|e| failed(what, e))?
        .visual;
    let pixmap = conn.generate_id().map_err(|e| failed(what, e))?;
    let named = conn
        .composite_name_window_pixmap(frame, pixmap)
        .map_err(|e| failed(what, e))?
        .check()
        .map_err(|e| failed(what, e));
    let image = named.and_then(|()| {
        // The pixmap covers the frame, border included, from its corner.
        let local = PhysicalRect {
            origin: PhysicalPoint::new(
                area.origin.x - bounds.origin.x,
                area.origin.y - bounds.origin.y,
            ),
            size: area.size,
        };
        reader.read(pixmap, local, visual)
    });
    if let Ok(cookie) = conn.free_pixmap(pixmap) {
        let _ = cookie.check();
    }
    if redirected && let Ok(cookie) = conn.composite_unredirect_window(frame, Redirect::AUTOMATIC) {
        let _ = cookie.check();
    }
    image
}

/// Reads `area` from the root window: what is on screen there, which without
/// a compositing manager is all there is.
fn capture_on_screen(x11: &X11, reader: &mut Reader<'_>, area: PhysicalRect) -> Result<Image> {
    let screen = x11.screen();
    let root = PhysicalRect::new(
        0,
        0,
        u32::from(screen.width_in_pixels),
        u32::from(screen.height_in_pixels),
    );
    let area = area
        .intersection(&root)
        .ok_or_else(|| Error::Platform("the window is off screen".into()))?;
    reader.read(screen.root, area, screen.root_visual)
}

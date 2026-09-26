//! Reading pixels from the X server: MIT-SHM `ShmGetImage` into a segment the
//! server creates and shares with us (no copy through the socket), or plain
//! `GetImage` where the server cannot share memory (remote displays, servers
//! without MIT-SHM 1.2).

use chartreuse_core::geometry::{PhysicalRect, PhysicalSize};
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use memmap2::{Mmap, MmapOptions};
use x11rb::connection::Connection as _;
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt as _, Drawable, ImageFormat, ImageOrder, Visualid};

use super::connection::{failed, X11};
use crate::linux::logic::ximage::{self, PixelFormat};

/// Reads images, reusing one shared-memory segment for a batch.
#[derive(Debug)]
pub struct Reader<'a> {
    x11: &'a X11,
    /// `None` once shared memory has failed; `GetImage` is used from then on.
    shm: Option<Option<Segment>>,
}

#[derive(Debug)]
struct Segment {
    id: shm::Seg,
    map: Mmap,
}

impl<'a> Reader<'a> {
    pub fn new(x11: &'a X11) -> Self {
        let shared = x11
            .conn
            .shm_query_version()
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|version| (version.major_version, version.minor_version) >= (1, 2));
        Self {
            x11,
            shm: shared.then_some(None),
        }
    }

    /// The pixels of `area` of `drawable` (relative to its origin), whose
    /// visual is `visual`. `area` must lie inside the drawable.
    pub fn read(
        &mut self,
        drawable: Drawable,
        area: PhysicalRect,
        visual: Visualid,
    ) -> Result<Image> {
        let size = area.size;
        if size.is_empty() {
            return Err(Error::InvalidImage("the area to capture is empty".into()));
        }
        let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
            i16::try_from(area.origin.x),
            i16::try_from(area.origin.y),
            u16::try_from(size.width),
            u16::try_from(size.height),
        ) else {
            return Err(Error::InvalidImage(format!(
                "{}×{} at ({}, {}) is outside the X11 coordinate range",
                size.width, size.height, area.origin.x, area.origin.y
            )));
        };
        if self.shm.is_some() {
            match self.read_shared(drawable, (x, y, width, height), size, visual) {
                Ok(image) => return Ok(image),
                Err(error) => {
                    tracing::debug!("MIT-SHM capture failed, using GetImage: {error}");
                    self.release();
                    self.shm = None;
                }
            }
        }
        let what = "reading pixels (GetImage)";
        let reply = self
            .x11
            .conn
            .get_image(ImageFormat::Z_PIXMAP, drawable, x, y, width, height, !0)
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))?;
        let format = self.format(reply.depth, visual, size.width)?;
        ximage::to_image(size, &format, &reply.data)
    }

    fn read_shared(
        &mut self,
        drawable: Drawable,
        (x, y, width, height): (i16, i16, u16, u16),
        size: PhysicalSize,
        visual: Visualid,
    ) -> Result<Image> {
        let depth = self.depth(visual)?;
        let format = self.format(depth, visual, size.width)?;
        // The server wants room for every row padded, the last one included,
        // even though parsing needs no padding after the last pixel.
        let len = format.stride * size.height as usize;
        let x11 = self.x11;
        let segment = self.segment(len)?;
        let what = "reading pixels (ShmGetImage)";
        x11.conn
            .shm_get_image(
                drawable,
                x,
                y,
                width,
                height,
                !0,
                ImageFormat::Z_PIXMAP.into(),
                segment.id,
                0,
            )
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))?;
        // The reply means the server has finished writing the segment.
        ximage::to_image(size, &format, &segment.map[..len])
    }

    /// A segment of at least `len` bytes, reusing the last one if it is big
    /// enough.
    fn segment(&mut self, len: usize) -> Result<&Segment> {
        if self
            .shm
            .as_ref()
            .and_then(Option::as_ref)
            .is_none_or(|s| s.map.len() < len)
        {
            self.release();
            let what = "sharing memory with the X server";
            let size = u32::try_from(len)
                .map_err(|_| Error::InvalidImage("the capture is too large".into()))?;
            let id = self.x11.conn.generate_id().map_err(|e| failed(what, e))?;
            let reply = self
                .x11
                .conn
                .shm_create_segment(id, size, false)
                .map_err(|e| failed(what, e))?
                .reply()
                .map_err(|e| failed(what, e))?;
            // SAFETY: the X server created the segment for this connection
            // and only writes it while serving our ShmGetImage requests, each
            // of which completes (with its reply) before we read the mapping.
            let map = unsafe { MmapOptions::new().len(len).map(&reply.shm_fd) }
                .map_err(|e| failed(what, e))?;
            self.shm = Some(Some(Segment { id, map }));
        }
        self.shm
            .as_ref()
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::Platform("X11: no shared memory segment".into()))
    }

    /// Detaches the current segment, if any; the server frees it once no
    /// client uses it, and the mapping goes with the `Segment`.
    fn release(&mut self) {
        if let Some(segment) = self.shm.as_mut().and_then(Option::take)
            && let Ok(cookie) = self.x11.conn.shm_detach(segment.id)
        {
            let _ = cookie.check();
        }
    }

    /// The depth of drawables with `visual`.
    fn depth(&self, visual: Visualid) -> Result<u8> {
        self.x11
            .screen()
            .allowed_depths
            .iter()
            .find(|depth| depth.visuals.iter().any(|v| v.visual_id == visual))
            .map(|depth| depth.depth)
            .ok_or_else(|| Error::Platform(format!("X11: unknown visual {visual:#x}")))
    }

    /// How `width`-pixel rows of a `depth`-bit drawable with `visual` are laid
    /// out. Depth-32 visuals (windows with an alpha channel) carry alpha in the
    /// bits no color uses.
    fn format(&self, depth: u8, visual: Visualid, width: u32) -> Result<PixelFormat> {
        let setup = self.x11.conn.setup();
        let pixmap = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == depth)
            .ok_or_else(|| Error::Platform(format!("X11: no pixmap format for depth {depth}")))?;
        let visual = self
            .x11
            .screen()
            .allowed_depths
            .iter()
            .flat_map(|depth| &depth.visuals)
            .find(|v| v.visual_id == visual)
            .ok_or_else(|| Error::Platform(format!("X11: unknown visual {visual:#x}")))?;
        let format = PixelFormat::new(
            width,
            pixmap.bits_per_pixel,
            pixmap.scanline_pad,
            setup.image_byte_order == ImageOrder::MSB_FIRST,
            [visual.red_mask, visual.green_mask, visual.blue_mask],
        );
        Ok(if depth == 32 {
            format.with_alpha()
        } else {
            format
        })
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

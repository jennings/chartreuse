//! Windows: clipboard access through the Win32 clipboard.
//!
//! Writing empties the clipboard and puts the image on it as a registered `PNG`
//! format and then as `CF_DIBV5` (see [`clipboard_data`](super::clipboard_data)),
//! in that order, since applications take the first format they understand.
//! Reading prefers `PNG`, then the bitmap the source application wrote; if that
//! does not decode, the next choice is tried.
//!
//! Windows requires an owner window for the clipboard to set data on it, so the
//! backend creates a hidden message-only window on first write and keeps it. The
//! data stays on the clipboard after the window is gone.

use std::cell::OnceCell;
use std::thread;
use std::time::Duration;

use ::windows::core::w;
use ::windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use ::windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use ::windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use chartreuse_core::image::Image;
use chartreuse_core::Result;
use chartreuse_imaging::codec::{self, Format};

use super::clipboard_data::{dibv5_from_image, formats_to_read, image_from_dib, CF_DIBV5};
use super::hidden_window::{Handler, HiddenWindow, Kind};
use super::util::platform_error;
use crate::clipboard::Clipboard;

/// How often to try opening the clipboard, and how long to wait in between.
/// Another process (often clipboard history, reading what just changed) may hold
/// it open for a moment.
const OPEN_ATTEMPTS: u32 = 5;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

/// The Windows [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct WindowsClipboard {
    /// The clipboard owner window, created on first write.
    owner: OnceCell<HiddenWindow<Owner>>,
}

impl WindowsClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    fn owner(&self) -> Result<HWND> {
        if let Some(owner) = self.owner.get() {
            return Ok(owner.hwnd());
        }
        let owner = HiddenWindow::new(Kind::MessageOnly, Owner)?;
        let hwnd = owner.hwnd();
        // Nothing else fills the cell: `self` is used on one thread only.
        let _ = self.owner.set(owner);
        Ok(hwnd)
    }
}

impl Clipboard for WindowsClipboard {
    fn write_image(&self, image: &Image) -> Result<()> {
        let png = codec::encode(image, Format::Png)?;
        let dib = dibv5_from_image(image)?;
        let png_format = png_format()?;
        let open = Open::new(Some(self.owner()?))?;
        // SAFETY: the clipboard is open, with our window as its owner.
        unsafe { EmptyClipboard() }.map_err(|e| platform_error("EmptyClipboard", &e))?;
        set_data(&open, png_format, &png)?;
        set_data(&open, CF_DIBV5, &dib)?;
        drop(open);
        tracing::debug!(
            width = image.width(),
            height = image.height(),
            "copied an image"
        );
        Ok(())
    }

    fn read_image(&self) -> Result<Option<Image>> {
        let png_format = png_format()?;
        // Reading needs no owner window.
        let formats = formats_to_read(offered_formats(&Open::new(None)?), png_format);
        let mut last_error = None;
        for format in formats {
            // Decode with the clipboard closed, so other applications can use it.
            let bytes = Open::new(None).and_then(|open| data(&open, format));
            let decoded = bytes.and_then(|bytes| {
                if format == png_format {
                    codec::decode_as(&bytes, Format::Png)
                } else {
                    image_from_dib(&bytes)
                }
            });
            match decoded {
                Ok(image) => return Ok(Some(image)),
                Err(error) => {
                    tracing::warn!(format, %error, "could not read the clipboard image");
                    last_error = Some(error);
                }
            }
        }
        last_error.map_or(Ok(None), Err)
    }
}

/// The registered `PNG` clipboard format.
fn png_format() -> Result<u32> {
    // SAFETY: the name is a NUL-terminated string.
    match unsafe { RegisterClipboardFormatW(w!("PNG")) } {
        0 => Err(platform_error(
            "RegisterClipboardFormatW",
            &::windows::core::Error::from_thread(),
        )),
        format => Ok(format),
    }
}

/// The open clipboard, closed on drop. Functions that need the clipboard open
/// take one.
struct Open;

impl Open {
    /// Opens the clipboard, with `owner` as its owner once emptied.
    fn new(owner: Option<HWND>) -> Result<Self> {
        let mut attempt = 1;
        loop {
            // SAFETY: `owner` is a live window of this thread, or none.
            match unsafe { OpenClipboard(owner) } {
                Ok(()) => return Ok(Self),
                Err(error) if attempt == OPEN_ATTEMPTS => {
                    return Err(platform_error("OpenClipboard", &error));
                }
                Err(_) => {
                    attempt += 1;
                    thread::sleep(OPEN_RETRY_DELAY);
                }
            }
        }
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: this thread opened the clipboard in `Open::new`.
        if let Err(error) = unsafe { CloseClipboard() } {
            tracing::warn!(%error, "CloseClipboard failed");
        }
    }
}

/// The formats on the clipboard, in its order.
fn offered_formats(_: &Open) -> Vec<u32> {
    let mut formats = Vec::new();
    let mut format = 0;
    loop {
        // SAFETY: the clipboard is open. 0 starts the enumeration, and 0 ends it.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            return formats;
        }
        formats.push(format);
    }
}

/// Puts `bytes` on the emptied clipboard as `format`.
fn set_data(_: &Open, format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: allocating has no preconditions. A moveable block must be locked
    // to be written; it is `bytes.len()` long, so the copy stays inside it. Once
    // SetClipboardData succeeds the clipboard owns the block; until then it is
    // ours to free.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| platform_error("GlobalAlloc", &e))?;
        let target = GlobalLock(memory);
        if target.is_null() {
            let error = ::windows::core::Error::from_thread();
            let _ = GlobalFree(Some(memory));
            return Err(platform_error("GlobalLock", &error));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        // Fails with NO_ERROR once the lock count drops to zero.
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(platform_error("SetClipboardData", &error));
        }
    }
    Ok(())
}

/// A copy of the clipboard's `format` data.
fn data(_: &Open, format: u32) -> Result<Vec<u8>> {
    // SAFETY: the clipboard is open. The handle stays valid until it closes, and
    // is a global memory block for every format read here; locking it yields
    // `GlobalSize` readable bytes.
    unsafe {
        let handle =
            GetClipboardData(format).map_err(|e| platform_error("GetClipboardData", &e))?;
        let memory = HGLOBAL(handle.0);
        let source = GlobalLock(memory);
        if source.is_null() {
            return Err(platform_error(
                "GlobalLock",
                &::windows::core::Error::from_thread(),
            ));
        }
        let bytes = std::slice::from_raw_parts(source.cast::<u8>(), GlobalSize(memory)).to_vec();
        let _ = GlobalUnlock(memory);
        Ok(bytes)
    }
}

/// The clipboard owner window's (empty) message handling.
struct Owner;

impl Handler for Owner {
    const CLASS: &'static str = "ChartreuseClipboardOwner";

    fn handle(&self, _: HWND, _: u32, _: WPARAM, _: LPARAM) -> Option<LRESULT> {
        None
    }
}

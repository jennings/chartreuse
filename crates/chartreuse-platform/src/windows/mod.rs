//! The Windows backend: one file per platform trait, so parallel tracks never edit
//! the same file.
//!
//! Each file exports one backend type with a `new()` constructor. Keep that
//! constructor infallible and argument-free so this file never changes; do
//! fallible setup lazily in the trait methods.

mod capture;
mod clipboard;
mod dialogs;
mod displays;
mod hotkeys;
mod overlay_style;
mod permissions;
mod status_item;
mod window_list;

use std::sync::Arc;

use crate::Platform;

/// Every Windows backend.
pub fn platform() -> Platform {
    Platform {
        displays: Box::new(displays::WindowsDisplays::new()),
        capture: Box::new(capture::WindowsCapture::new()),
        window_list: Box::new(window_list::WindowsWindowList::new()),
        hotkeys: Box::new(hotkeys::WindowsHotkeys::new()),
        status_item: Box::new(status_item::WindowsStatusItem::new()),
        clipboard: Box::new(clipboard::WindowsClipboard::new()),
        file_dialogs: Box::new(dialogs::WindowsFileDialogs::new()),
        overlay_style: Arc::new(overlay_style::WindowsOverlayStyle::new()),
        permissions: Box::new(permissions::WindowsPermissions::new()),
    }
}

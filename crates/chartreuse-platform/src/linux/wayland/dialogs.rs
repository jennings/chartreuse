//! Wayland: open and save dialogs, through the FileChooser portal shared with
//! the X11 backend (see [`crate::linux::file_chooser`]).

use std::path::PathBuf;

use chartreuse_core::Result;
use futures::future::BoxFuture;

use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};
use crate::linux::file_chooser;

/// The Wayland [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct WaylandFileDialogs;

impl WaylandFileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for WaylandFileDialogs {
    fn open_image(&self, request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        file_chooser::open_image(request)
    }

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        file_chooser::save_image(request)
    }
}

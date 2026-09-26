//! X11: open and save dialogs, through the FileChooser portal shared with
//! the Wayland backend (see [`crate::linux::file_chooser`]).

use std::path::PathBuf;

use chartreuse_core::Result;
use futures::future::BoxFuture;

use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};
use crate::linux::file_chooser;

/// The X11 [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct X11FileDialogs;

impl X11FileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for X11FileDialogs {
    fn open_image(&self, request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        file_chooser::open_image(request)
    }

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        file_chooser::save_image(request)
    }
}

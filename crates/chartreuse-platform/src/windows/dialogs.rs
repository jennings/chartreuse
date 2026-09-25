//! Windows: open and save dialogs. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use std::path::PathBuf;

use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};

/// The Windows [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct WindowsFileDialogs;

impl WindowsFileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for WindowsFileDialogs {
    fn open_image(
        &self,
        _request: OpenImageRequest,
    ) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        future::ready(Err(Error::Unsupported("the file dialog"))).boxed()
    }

    fn save_image(
        &self,
        _request: SaveImageRequest,
    ) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        future::ready(Err(Error::Unsupported("the file dialog"))).boxed()
    }
}

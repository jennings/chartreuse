//! X11: open and save dialogs. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use std::path::PathBuf;

use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};

/// The X11 [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct X11FileDialogs;

impl X11FileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for X11FileDialogs {
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

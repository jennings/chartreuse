//! X11: on-screen window enumeration. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::window::WindowInfo;
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use crate::window_list::WindowList;

/// The X11 [`WindowList`] backend.
#[derive(Debug, Default)]
pub struct X11WindowList;

impl X11WindowList {
    pub fn new() -> Self {
        Self
    }
}

impl WindowList for X11WindowList {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        future::ready(Err(Error::Unsupported("window enumeration"))).boxed()
    }
}

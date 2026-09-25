//! Wayland: on-screen window enumeration. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::window::WindowInfo;
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use crate::window_list::WindowList;

/// The Wayland [`WindowList`] backend.
#[derive(Debug, Default)]
pub struct WaylandWindowList;

impl WaylandWindowList {
    pub fn new() -> Self {
        Self
    }
}

impl WindowList for WaylandWindowList {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        future::ready(Err(Error::Unsupported("window enumeration"))).boxed()
    }
}

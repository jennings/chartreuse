//! Wayland: display enumeration. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::{Error, Result};

use crate::displays::Displays;

/// The Wayland [`Displays`] backend.
#[derive(Debug, Default)]
pub struct WaylandDisplays;

impl WaylandDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for WaylandDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Err(Error::Unsupported("display enumeration"))
    }
}

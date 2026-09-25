//! macOS: display enumeration. Implemented by track 1D (`NSScreen` / `CGDisplay*`).
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::{Error, Result};

use crate::displays::Displays;

/// The macOS [`Displays`] backend.
#[derive(Debug, Default)]
pub struct MacosDisplays;

impl MacosDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for MacosDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Err(Error::Unsupported("display enumeration"))
    }
}

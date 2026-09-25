//! Windows: display enumeration. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::{Error, Result};

use crate::displays::Displays;

/// The Windows [`Displays`] backend.
#[derive(Debug, Default)]
pub struct WindowsDisplays;

impl WindowsDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for WindowsDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Err(Error::Unsupported("display enumeration"))
    }
}

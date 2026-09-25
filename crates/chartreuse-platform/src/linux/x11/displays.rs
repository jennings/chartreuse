//! X11: display enumeration. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::{Error, Result};

use crate::displays::Displays;

/// The X11 [`Displays`] backend.
#[derive(Debug, Default)]
pub struct X11Displays;

impl X11Displays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for X11Displays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Err(Error::Unsupported("display enumeration"))
    }
}

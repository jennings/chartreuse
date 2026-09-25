//! X11: global hotkeys. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::hotkeys::{HotkeyBinding, HotkeyRegistration, Hotkeys};

/// The X11 [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct X11Hotkeys;

impl X11Hotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for X11Hotkeys {
    fn register(&self, _bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        Err(Error::Unsupported("global hotkey registration"))
    }
}

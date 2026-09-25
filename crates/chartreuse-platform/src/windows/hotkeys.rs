//! Windows: global hotkeys. Implemented by track 4A.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::hotkeys::{HotkeyBinding, HotkeyRegistration, Hotkeys};

/// The Windows [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct WindowsHotkeys;

impl WindowsHotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for WindowsHotkeys {
    fn register(&self, _bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        Err(Error::Unsupported("global hotkey registration"))
    }
}

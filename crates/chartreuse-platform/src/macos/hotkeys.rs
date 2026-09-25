//! macOS: global hotkeys. Implemented by track 1B (Carbon `RegisterEventHotKey`).
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::hotkeys::{HotkeyBinding, HotkeyRegistration, Hotkeys};

/// The macOS [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct MacosHotkeys;

impl MacosHotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for MacosHotkeys {
    fn register(&self, _bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        Err(Error::Unsupported("global hotkey registration"))
    }
}

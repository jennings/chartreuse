//! Wayland: global hotkeys. Implemented by track 4B.
//!
//! Until then every call fails with [`Error::Unsupported`].

use chartreuse_core::{Error, Result};

use crate::hotkeys::{HotkeyBinding, HotkeyRegistration, Hotkeys};

/// The Wayland [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct WaylandHotkeys;

impl WaylandHotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for WaylandHotkeys {
    fn register(&self, _bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        Err(Error::Unsupported("global hotkey registration"))
    }
}

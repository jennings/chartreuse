//! System-wide hotkeys.

use chartreuse_core::capture::CaptureMode;
use chartreuse_core::error::Error;
use chartreuse_core::hotkey::Hotkey;
use chartreuse_core::Result;

use crate::event::{EventReceiver, Registration};

/// One hotkey and the capture it triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyBinding {
    pub mode: CaptureMode,
    pub hotkey: Hotkey,
}

/// A registered hotkey was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyEvent {
    pub mode: CaptureMode,
    pub hotkey: Hotkey,
}

/// The result of [`Hotkeys::register`].
#[derive(Debug)]
pub struct HotkeyRegistration {
    /// Presses of every successfully registered binding.
    pub events: EventReceiver<HotkeyEvent>,
    /// Bindings that could not be registered, typically
    /// [`Error::HotkeyUnavailable`] because another program owns the combination.
    /// The other bindings are active regardless.
    pub failures: Vec<(HotkeyBinding, Error)>,
    /// Keeps the hotkeys registered; drop it (on the main thread) to unregister.
    pub registration: Registration,
}

/// Registers system-wide hotkeys that work whichever application has focus.
pub trait Hotkeys {
    /// Registers `bindings` as one set.
    ///
    /// To change the set (for example after the settings change), drop the previous
    /// [`HotkeyRegistration`] first, so its combinations are free, then register
    /// the new set.
    ///
    /// **Main thread only**: Carbon hotkeys on macOS are tied to the main run loop.
    /// Returns `Err` only if the mechanism as a whole is unavailable; per-binding
    /// problems go into [`HotkeyRegistration::failures`].
    fn register(&self, bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration>;
}

//! Windows: global hotkeys through `RegisterHotKey`.
//!
//! Each [`Hotkeys::register`] call creates a message-only window on the main
//! thread (whose message loop winit runs) and registers every binding for it,
//! with the binding's index as the hotkey id. `WM_HOTKEY` then arrives at that
//! window, which sends the binding's [`HotkeyEvent`]. Dropping the
//! [`Registration`] unregisters the hotkeys and destroys the window, so several
//! registrations can coexist and never see each other's presses.
//!
//! Hotkeys are registered with `MOD_NOREPEAT`. Registration fails with
//! `ERROR_HOTKEY_ALREADY_REGISTERED`, reported as [`Error::HotkeyUnavailable`],
//! when another program (or another Chartreuse registration) holds the
//! combination. Key mapping is described in `keys.rs`.

use ::windows::Win32::Foundation::{
    ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MAPVK_VSC_TO_VK,
};
use ::windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;
use chartreuse_core::hotkey::Hotkey;
use chartreuse_core::{Error, Result};

use super::hidden_window::{Handler, HiddenWindow, Kind};
use super::keys::{key_code, modifier_flags, KeyCode};
use crate::event::{self, EventSender, Registration};
use crate::hotkeys::{HotkeyBinding, HotkeyEvent, HotkeyRegistration, Hotkeys};

/// The Windows [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct WindowsHotkeys;

impl WindowsHotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for WindowsHotkeys {
    fn register(&self, bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        let (sender, events) = event::channel();
        let window = HiddenWindow::new(
            Kind::MessageOnly,
            Presses {
                sender,
                bindings: bindings.to_vec(),
            },
        )?;
        let mut registered = Registered {
            window,
            ids: Vec::new(),
        };
        let mut failures = Vec::new();
        for (id, binding) in (0..).zip(bindings) {
            let hotkey = binding.hotkey;
            // SAFETY: the window belongs to this thread and outlives the
            // registration (see `Registered`'s Drop).
            let result = unsafe {
                RegisterHotKey(
                    Some(registered.window.hwnd()),
                    id,
                    HOT_KEY_MODIFIERS(modifier_flags(hotkey)),
                    u32::from(virtual_key(hotkey)),
                )
            };
            match result {
                Ok(()) => registered.ids.push(id),
                Err(error) => {
                    tracing::warn!(%hotkey, %error, "could not register a hotkey");
                    failures.push((*binding, registration_error(hotkey, &error)));
                }
            }
        }
        Ok(HotkeyRegistration {
            events,
            failures,
            registration: Registration::new(registered),
        })
    }
}

/// The virtual-key code of `hotkey`'s key on the current keyboard layout.
fn virtual_key(hotkey: Hotkey) -> u16 {
    match key_code(hotkey.key) {
        KeyCode::Fixed(vk) => vk,
        KeyCode::Positional { scan, us } => {
            // SAFETY: no preconditions; 0 means the layout has no such key.
            match unsafe { MapVirtualKeyW(u32::from(scan), MAPVK_VSC_TO_VK) } {
                0 => us,
                vk => u16::try_from(vk).unwrap_or(us),
            }
        }
    }
}

/// Why `RegisterHotKey` refused `hotkey`.
fn registration_error(hotkey: Hotkey, error: &::windows::core::Error) -> Error {
    let reason = if error.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() {
        "another app or another Chartreuse hotkey is already using it".to_owned()
    } else {
        format!("Windows refused it: {error}")
    };
    Error::HotkeyUnavailable { hotkey, reason }
}

/// Forwards `WM_HOTKEY` presses of one registration.
struct Presses {
    sender: EventSender<HotkeyEvent>,
    /// Indexed by hotkey id.
    bindings: Vec<HotkeyBinding>,
}

impl Handler for Presses {
    const CLASS: &'static str = "ChartreuseHotkeys";

    fn handle(&self, _: HWND, message: u32, wparam: WPARAM, _: LPARAM) -> Option<LRESULT> {
        if message != WM_HOTKEY {
            return None;
        }
        // wparam is the id passed to RegisterHotKey (negative ids are the system's).
        if let Some(binding) = self.bindings.get(wparam.0) {
            tracing::debug!(hotkey = %binding.hotkey, "hotkey pressed");
            if !self.sender.send(HotkeyEvent {
                mode: binding.mode,
                hotkey: binding.hotkey,
            }) {
                tracing::debug!("nothing is listening for hotkey presses");
            }
        }
        Some(LRESULT(0))
    }
}

/// A registered set of hotkeys. Dropping it unregisters them, then destroys the
/// window.
struct Registered {
    window: HiddenWindow<Presses>,
    ids: Vec<i32>,
}

impl Drop for Registered {
    fn drop(&mut self) {
        for &id in &self.ids {
            // SAFETY: registered for this window by `register`.
            if let Err(error) = unsafe { UnregisterHotKey(Some(self.window.hwnd()), id) } {
                tracing::warn!(%error, "could not unregister a hotkey");
            }
        }
    }
}

//! Linux and other free Unix desktops: an X11 and a Wayland backend, chosen at
//! runtime from the session type.
//!
//! The backends' pure logic lives in [`logic`], which uses no Linux-only crate:
//! test builds on other hosts compile it (and the session detection here), so
//! its unit tests run on every development machine.

#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
mod logic;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

use std::ffi::OsStr;

#[cfg(all(unix, not(target_os = "macos")))]
use crate::Platform;

/// The kind of graphical session Chartreuse runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    X11,
    Wayland,
}

impl Session {
    /// Wayland if `WAYLAND_DISPLAY` is set or `XDG_SESSION_TYPE` says so, else X11
    /// (which also covers XWayland-only setups).
    fn detect(wayland_display: Option<&OsStr>, xdg_session_type: Option<&OsStr>) -> Self {
        let wayland_socket = wayland_display.is_some_and(|value| !value.is_empty());
        let wayland_session =
            xdg_session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"));
        if wayland_socket || wayland_session {
            Self::Wayland
        } else {
            Self::X11
        }
    }
}

/// The backend for the current session.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn platform() -> Platform {
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY");
    let xdg_session_type = std::env::var_os("XDG_SESSION_TYPE");
    match Session::detect(wayland_display.as_deref(), xdg_session_type.as_deref()) {
        Session::X11 => x11::platform(),
        Session::Wayland => wayland::platform(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(wayland_display: Option<&str>, xdg_session_type: Option<&str>) -> Session {
        Session::detect(
            wayland_display.map(OsStr::new),
            xdg_session_type.map(OsStr::new),
        )
    }

    #[test]
    fn wayland_socket_or_session_type_selects_wayland() {
        assert_eq!(detect(Some("wayland-0"), None), Session::Wayland);
        assert_eq!(detect(None, Some("Wayland")), Session::Wayland);
    }

    #[test]
    fn everything_else_falls_back_to_x11() {
        assert_eq!(detect(None, None), Session::X11);
        assert_eq!(detect(Some(""), Some("x11")), Session::X11);
        assert_eq!(detect(None, Some("tty")), Session::X11);
    }
}

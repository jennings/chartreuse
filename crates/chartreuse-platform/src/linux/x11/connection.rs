//! The X server connection the X11 backends share, opened on first use.
//!
//! `x11rb`'s pure-Rust connection is thread-safe, so the display, capture,
//! window list, and overlay backends all send requests through this one;
//! only the hotkey listener opens its own, because it reads events.
//! Requests are checked (their errors come back with the reply or `check()`),
//! so no error events pile up on the shared connection.

use std::fmt::Display;
use std::sync::OnceLock;

use chartreuse_core::{Error, Result};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt as _, Screen, Window};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    /// The atoms the backends use.
    pub Atoms: AtomsCookie {
        UTF8_STRING,
        _GTK_FRAME_EXTENTS,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_CURRENT_DESKTOP,
        _NET_WM_DESKTOP,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_DOCK,
        _XSETTINGS_SETTINGS,
    }
}

/// An open connection and what the backends look up on it.
#[derive(Debug)]
pub struct X11 {
    pub conn: RustConnection,
    /// The default screen's index.
    pub screen: usize,
    pub atoms: Atoms,
}

/// The shared connection, opened on first use. A failure to connect is
/// remembered: `DISPLAY` does not change while Chartreuse runs.
pub fn get() -> Result<&'static X11> {
    static X11: OnceLock<std::result::Result<X11, String>> = OnceLock::new();
    X11.get_or_init(|| X11::connect().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| Error::Platform(error.clone()))
}

impl X11 {
    fn connect() -> Result<Self> {
        let (conn, screen) = x11rb::connect(None)
            .map_err(|e| failed("connecting to the X server (is DISPLAY set?)", e))?;
        let atoms = Atoms::new(&conn)
            .map_err(|e| failed("interning atoms", e))?
            .reply()
            .map_err(|e| failed("interning atoms", e))?;
        Ok(Self {
            conn,
            screen,
            atoms,
        })
    }

    /// The default screen.
    pub fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen]
    }

    /// The default screen's root window.
    pub fn root(&self) -> Window {
        self.screen().root
    }

    /// Interns `name`, for atoms whose name is only known at run time.
    pub fn atom(&self, name: &str) -> Result<Atom> {
        let what = "interning an atom";
        Ok(self
            .conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))?
            .atom)
    }

    /// The 32-bit values of `window`'s `property` of `kind`; empty if unset.
    pub fn property32(
        &self,
        window: Window,
        property: Atom,
        kind: impl Into<Atom>,
    ) -> Result<Vec<u32>> {
        let reply = self.property(window, property, kind.into())?;
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
    }

    /// The bytes of `window`'s `property` of `kind`; empty if unset.
    pub fn property8(
        &self,
        window: Window,
        property: Atom,
        kind: impl Into<Atom>,
    ) -> Result<Vec<u8>> {
        let reply = self.property(window, property, kind.into())?;
        Ok(if reply.format == 8 {
            reply.value
        } else {
            Vec::new()
        })
    }

    fn property(
        &self,
        window: Window,
        property: Atom,
        kind: Atom,
    ) -> Result<xproto::GetPropertyReply> {
        let what = "reading a window property";
        self.conn
            .get_property(false, window, property, kind, 0, u32::MAX / 4)
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))
    }

    /// The window's title: `_NET_WM_NAME` (UTF-8), else `WM_NAME` (Latin-1).
    pub fn title(&self, window: Window) -> Result<Option<String>> {
        let name = self.property8(window, self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING)?;
        if !name.is_empty() {
            return Ok(Some(String::from_utf8_lossy(&name).into_owned()));
        }
        let name = self.property8(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING)?;
        Ok((!name.is_empty()).then(|| name.iter().copied().map(char::from).collect()))
    }
}

/// An X11 request that failed, as an [`Error`].
pub fn failed(what: &str, error: impl Display) -> Error {
    Error::Platform(format!("X11: {what} failed: {error}"))
}

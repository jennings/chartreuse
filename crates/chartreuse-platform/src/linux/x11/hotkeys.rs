//! X11: global hotkeys as passive key grabs (`GrabKey`) on the root window.
//!
//! Each registration opens its own connection, grabs every binding's key (in
//! all Caps Lock / Num Lock combinations, because grabs match the exact
//! modifier state), and reads that connection's events on a thread, which
//! forwards presses of the grabbed keys. The X server tells a client whose
//! grab clashes with another client's (`BadAccess`), which becomes that
//! binding's [`Error::HotkeyUnavailable`]. Dropping the registration releases
//! the grabs and stops the thread.
//!
//! Keys are looked up in the keyboard mapping at registration time: a layout
//! switch afterwards keeps the grabbed physical keys.
//!
//! Holding a hotkey triggers it once, as on the other platforms: the grabs'
//! connection turns on XKB's detectable auto-repeat, and the listener drops
//! presses of a key that has not been released since (see [`HeldKeys`]).

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use chartreuse_core::hotkey::Hotkey;
use chartreuse_core::{Error, Result};
use x11rb::connection::Connection as _;
use x11rb::errors::ReplyError;
use x11rb::protocol::xkb::{self, BoolCtrl, ConnectionExt as _, PerClientFlag};
use x11rb::protocol::xproto::{
    ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode, ModMask, Window,
    WindowClass,
};
use x11rb::protocol::{ErrorKind, Event};
use x11rb::rust_connection::RustConnection;
use x11rb::x11_utils::X11Error;

use super::connection::failed;
use crate::event::{self, EventSender, Registration};
use crate::hotkeys::{HotkeyBinding, HotkeyEvent, HotkeyRegistration, Hotkeys};
use crate::linux::logic::keysym::keysym;
use crate::linux::logic::xgrab::{
    self, hotkey_state, lock_variants, modifier_mask, HeldKeys, KeyboardMapping,
};

/// The keysym of the Num Lock key.
const NUM_LOCK: u32 = 0xff7f;

/// The X11 [`Hotkeys`] backend.
#[derive(Debug, Default)]
pub struct X11Hotkeys;

impl X11Hotkeys {
    pub fn new() -> Self {
        Self
    }
}

impl Hotkeys for X11Hotkeys {
    fn register(&self, bindings: &[HotkeyBinding]) -> Result<HotkeyRegistration> {
        let what = "connecting to the X server for hotkeys";
        let (conn, screen) = x11rb::connect(None).map_err(|e| failed(what, e))?;
        let conn = Arc::new(conn);
        let root = conn.setup().roots[screen].root;
        let keyboard = Keyboard::query(&conn)?;
        match enable_detectable_auto_repeat(&conn) {
            Ok(true) => {}
            Ok(false) => tracing::debug!("the X server has no detectable auto-repeat"),
            Err(error) => tracing::debug!("enabling detectable auto-repeat failed: {error}"),
        }

        let mut grabs: Vec<Grab> = Vec::new();
        let mut failures = Vec::new();
        for binding in bindings {
            let key = keyboard.key(binding.hotkey);
            let grab = match key {
                Some(key) if grabs.iter().any(|grab| grab.key == key) => Err(unavailable(
                    binding.hotkey,
                    "it is bound to another capture".into(),
                )),
                Some(key) => grab(&conn, root, &keyboard.locks, key, *binding),
                None => Err(unavailable(
                    binding.hotkey,
                    "no key on the current keyboard layout types it".into(),
                )),
            };
            match grab {
                Ok(grab) => grabs.push(grab),
                Err(error) => failures.push((*binding, error)),
            }
        }

        // An invisible window of this connection, which the registration
        // sends a message to when it is dropped, to stop the listener.
        let what = "creating the hotkey window";
        let wake = conn.generate_id().map_err(|e| failed(what, e))?;
        conn.create_window(
            0,
            wake,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .map_err(|e| failed(what, e))?
        .check()
        .map_err(|e| failed(what, e))?;

        let (sender, events) = event::channel();
        let listener = {
            let conn = Arc::clone(&conn);
            let grabs = grabs.clone();
            thread::Builder::new()
                .name("chartreuse hotkeys".into())
                .spawn(move || listen(&conn, wake, &grabs, &sender))
                .map_err(|e| failed("starting the hotkey listener", e))?
        };
        Ok(HotkeyRegistration {
            events,
            failures,
            registration: Registration::new(Grabs {
                conn,
                root,
                grabs,
                locks: keyboard.locks,
                wake,
                listener: Some(listener),
            }),
        })
    }
}

/// A grabbed key: its keycode and modifier mask, without the lock variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    keycode: u8,
    modifiers: u16,
}

#[derive(Debug, Clone, Copy)]
struct Grab {
    key: Key,
    binding: HotkeyBinding,
}

/// The keyboard mapping and lock masks when the hotkeys were registered.
struct Keyboard {
    mapping: KeyboardMapping,
    locks: Vec<u16>,
}

impl Keyboard {
    fn query(conn: &RustConnection) -> Result<Self> {
        let what = "reading the keyboard mapping";
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))?;
        let mapping = KeyboardMapping {
            min_keycode: min,
            per_keycode: usize::from(reply.keysyms_per_keycode),
            keysyms: reply.keysyms,
        };
        let modifiers = conn
            .get_modifier_mapping()
            .map_err(|e| failed(what, e))?
            .reply()
            .map_err(|e| failed(what, e))?;
        let num_lock_keys: Vec<u8> = mapping.keycodes(NUM_LOCK).collect();
        let num_lock = xgrab::num_lock_mask(
            usize::from(modifiers.keycodes_per_modifier()),
            &modifiers.keycodes,
            &num_lock_keys,
        );
        Ok(Self {
            mapping,
            locks: lock_variants(num_lock),
        })
    }

    fn key(&self, hotkey: Hotkey) -> Option<Key> {
        Some(Key {
            keycode: self.mapping.keycode(keysym(hotkey.key).0)?,
            modifiers: modifier_mask(hotkey.modifiers),
        })
    }
}

/// Grabs `key` in every lock variant, or none of them.
fn grab(
    conn: &RustConnection,
    root: Window,
    locks: &[u16],
    key: Key,
    binding: HotkeyBinding,
) -> Result<Grab> {
    for (index, lock) in locks.iter().enumerate() {
        let result = conn
            .grab_key(
                false,
                root,
                ModMask::from(key.modifiers | lock),
                key.keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        if let Err(error) = result {
            ungrab(conn, root, &locks[..index], key);
            let reason = match error {
                ReplyError::X11Error(X11Error {
                    error_kind: ErrorKind::Access,
                    ..
                }) => "another application has registered it".into(),
                error => format!("the X server refused the grab: {error}"),
            };
            return Err(unavailable(binding.hotkey, reason));
        }
    }
    Ok(Grab { key, binding })
}

fn ungrab(conn: &RustConnection, root: Window, locks: &[u16], key: Key) {
    for lock in locks {
        if let Ok(cookie) = conn.ungrab_key(key.keycode, root, ModMask::from(key.modifiers | lock))
        {
            let _ = cookie.check();
        }
    }
}

fn unavailable(hotkey: Hotkey, reason: String) -> Error {
    Error::HotkeyUnavailable { hotkey, reason }
}

/// Makes the X server repeat a held key as presses alone on `conn`, with one
/// release when it goes up (XKB's detectable auto-repeat). `false` if the
/// server cannot; [`HeldKeys`] then tells repeats by their timing.
fn enable_detectable_auto_repeat(conn: &RustConnection) -> std::result::Result<bool, ReplyError> {
    if !conn.xkb_use_extension(1, 0)?.reply()?.supported {
        return Ok(false);
    }
    let flag = PerClientFlag::DETECTABLE_AUTO_REPEAT;
    let unchanged = BoolCtrl::from(0u32);
    let flags = conn
        .xkb_per_client_flags(
            xkb::ID::USE_CORE_KBD.into(),
            flag,
            flag,
            unchanged,
            unchanged,
            unchanged,
        )?
        .reply()?;
    Ok(flags.value.contains(flag))
}

/// Forwards presses of `grabs`, but not their auto-repeats, until `wake` gets
/// a message or the connection breaks.
fn listen(conn: &RustConnection, wake: Window, grabs: &[Grab], sender: &EventSender<HotkeyEvent>) {
    let mut held = HeldKeys::default();
    loop {
        match conn.wait_for_event() {
            Ok(Event::KeyPress(press)) => {
                let pressed = Key {
                    keycode: press.detail,
                    modifiers: hotkey_state(u16::from(press.state)),
                };
                if let Some(grab) = grabs.iter().find(|grab| grab.key == pressed)
                    && held.press(press.detail, press.time)
                {
                    sender.send(HotkeyEvent {
                        mode: grab.binding.mode,
                        hotkey: grab.binding.hotkey,
                    });
                }
            }
            // A grabbed key's press grabs the whole keyboard until that key
            // goes up, so its release comes here too.
            Ok(Event::KeyRelease(release)) => held.release(release.detail, release.time),
            Ok(Event::ClientMessage(message)) if message.window == wake => return,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("the hotkey connection to the X server broke: {error}");
                return;
            }
        }
    }
}

/// The registration: releases the grabs and stops the listener when dropped.
struct Grabs {
    conn: Arc<RustConnection>,
    root: Window,
    grabs: Vec<Grab>,
    locks: Vec<u16>,
    wake: Window,
    listener: Option<JoinHandle<()>>,
}

impl Drop for Grabs {
    fn drop(&mut self) {
        for grab in &self.grabs {
            ungrab(&self.conn, self.root, &self.locks, grab.key);
        }
        // With an empty event mask the event goes to the window's creator:
        // this connection, whose listener then returns.
        let stop = ClientMessageEvent::new(32, self.wake, x11rb::NONE, [0u32; 5]);
        let sent = self
            .conn
            .send_event(false, self.wake, EventMask::NO_EVENT, stop)
            .map(drop)
            .and_then(|()| self.conn.flush());
        if let Some(listener) = self.listener.take()
            && sent.is_ok()
        {
            let _ = listener.join();
        }
        if let Ok(cookie) = self.conn.destroy_window(self.wake) {
            let _ = cookie.check();
        }
    }
}

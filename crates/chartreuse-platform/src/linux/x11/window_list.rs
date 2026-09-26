//! X11: the window list, from the window manager's EWMH client list in
//! stacking order (`_NET_CLIENT_LIST_STACKING`), falling back to the root
//! window's children without an EWMH window manager.
//!
//! A window's bounds are its window manager frame (title bar included), or,
//! for client-side decorated windows, the client less its invisible shadows
//! (`_GTK_FRAME_EXTENTS`). Windows on other virtual desktops, minimized
//! windows, desktops and panels, and Chartreuse's own windows are left out.

use std::fs;

use chartreuse_core::geometry::PhysicalRect;
use chartreuse_core::window::WindowInfo;
use chartreuse_core::Result;
use futures::future::BoxFuture;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState, Window};
use x11rb::protocol::ErrorKind;

use super::connection::{self, failed, X11};
use super::displays::Desktop;
use crate::linux::blocking;
use crate::linux::logic::ewmh::{self, ClientWindow};
use crate::window_list::WindowList;

/// The X11 [`WindowList`] backend.
#[derive(Debug, Default)]
pub struct X11WindowList;

impl X11WindowList {
    pub fn new() -> Self {
        Self
    }
}

impl WindowList for X11WindowList {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        blocking::run("window list", || {
            let x11 = connection::get()?;
            let scale = Desktop::query(x11)?.scale;
            let root = x11.root();
            let atoms = &x11.atoms;
            let current = x11
                .property32(root, atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL)?
                .first()
                .copied();
            let mut clients = Vec::new();
            for window in stacking(x11)?.into_iter().rev() {
                if let Some(client) = describe(x11, window)? {
                    clients.push(client);
                }
            }
            Ok(ewmh::window_list(
                &clients,
                current,
                std::process::id(),
                scale,
            ))
        })
    }
}

/// The top-level windows from the bottom of the stack to the top.
fn stacking(x11: &X11) -> Result<Vec<Window>> {
    let root = x11.root();
    let atoms = &x11.atoms;
    for list in [atoms._NET_CLIENT_LIST_STACKING, atoms._NET_CLIENT_LIST] {
        let windows = x11.property32(root, list, AtomEnum::WINDOW)?;
        if !windows.is_empty() {
            return Ok(windows);
        }
    }
    let what = "listing top-level windows";
    Ok(x11
        .conn
        .query_tree(root)
        .map_err(|e| failed(what, e))?
        .reply()
        .map_err(|e| failed(what, e))?
        .children)
}

/// A client window and its frame, the frame being the ancestor that is a
/// child of the root window.
#[derive(Debug, Clone)]
pub struct Described {
    pub client: ClientWindow,
    pub frame: Window,
}

/// What `window` looks like, or `None` if it has been destroyed meanwhile.
pub fn describe(x11: &X11, window: Window) -> Result<Option<ClientWindow>> {
    Ok(describe_with_frame(x11, window)?.map(|described| described.client))
}

/// [`describe`], with the frame window.
pub fn describe_with_frame(x11: &X11, window: Window) -> Result<Option<Described>> {
    match try_describe(x11, window) {
        Ok(described) => Ok(Some(described)),
        Err(ReplyError::X11Error(error))
            if matches!(error.error_kind, ErrorKind::Window | ErrorKind::Drawable) =>
        {
            Ok(None)
        }
        Err(error) => Err(failed("describing a window", error)),
    }
}

fn try_describe(x11: &X11, window: Window) -> std::result::Result<Described, ReplyError> {
    let conn = &x11.conn;
    let root = x11.root();
    let atoms = &x11.atoms;
    let frame = frame_of(x11, window)?;
    let frame_geometry = conn.get_geometry(frame)?.reply()?;
    let border = u32::from(frame_geometry.border_width) * 2;
    let viewable = conn.get_window_attributes(frame)?.reply()?.map_state == MapState::VIEWABLE;
    let client_geometry = conn.get_geometry(window)?.reply()?;
    let client_origin = conn.translate_coordinates(window, root, 0, 0)?.reply()?;

    let property32 = |property, kind: AtomEnum| -> std::result::Result<Vec<u32>, ReplyError> {
        let reply = conn
            .get_property(false, window, property, kind, 0, 1024)?
            .reply()?;
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
    };
    let state = property32(atoms._NET_WM_STATE, AtomEnum::ATOM)?;
    let kinds = property32(atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM)?;
    let desktop = property32(atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL)?;
    let pid = property32(atoms._NET_WM_PID, AtomEnum::CARDINAL)?
        .first()
        .copied();
    let shadow = property32(atoms._GTK_FRAME_EXTENTS, AtomEnum::CARDINAL)?;
    let class = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
        .reply()?
        .value;
    let title = x11.title(window).ok().flatten();

    let client = ClientWindow {
        id: window,
        title,
        owner: owner(&class).or_else(|| pid.and_then(process_name)),
        pid,
        frame: PhysicalRect::new(
            i32::from(frame_geometry.x),
            i32::from(frame_geometry.y),
            u32::from(frame_geometry.width) + border,
            u32::from(frame_geometry.height) + border,
        ),
        client: PhysicalRect::new(
            i32::from(client_origin.dst_x),
            i32::from(client_origin.dst_y),
            u32::from(client_geometry.width),
            u32::from(client_geometry.height),
        ),
        shadow: <[u32; 4]>::try_from(shadow).ok(),
        viewable,
        hidden: state.contains(&atoms._NET_WM_STATE_HIDDEN),
        desktop_or_dock: kinds.iter().any(|kind| {
            *kind == atoms._NET_WM_WINDOW_TYPE_DESKTOP || *kind == atoms._NET_WM_WINDOW_TYPE_DOCK
        }),
        desktop: desktop.first().copied(),
    };
    Ok(Described { client, frame })
}

/// The ancestor of `window` that is a child of the root window.
fn frame_of(x11: &X11, mut window: Window) -> std::result::Result<Window, ReplyError> {
    let root = x11.root();
    loop {
        let tree = x11.conn.query_tree(window)?.reply()?;
        if tree.parent == root || tree.parent == x11rb::NONE {
            return Ok(window);
        }
        window = tree.parent;
    }
}

/// The class half of `WM_CLASS` (`instance\0Class\0`), else the instance.
fn owner(class: &[u8]) -> Option<String> {
    let mut parts = class
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    let instance = parts.next();
    parts
        .next()
        .or(instance)
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

/// The process's command name, from `/proc`.
fn process_name(pid: u32) -> Option<String> {
    let name = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(name.trim_end().to_owned()).filter(|name| !name.is_empty())
}

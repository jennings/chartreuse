//! macOS: on-screen window enumeration for window selection.
//!
//! # Sources
//!
//! Two APIs are combined, because neither is enough alone:
//!
//! - `CGWindowListCopyWindowInfo(OnScreenOnly | ExcludeDesktopElements)`, the
//!   window server's list, returns windows front to back, so its order is the
//!   z-order. Each entry has the window's layer, alpha, owner (name and pid),
//!   title, and bounds. The bounds are global points with the origin at the top-left
//!   corner of the primary display (the one with the menu bar) and y pointing
//!   down: exactly the display model's global logical space, so they are used
//!   as-is.
//! - `SCShareableContent` lists the windows ScreenCaptureKit can capture, in no
//!   particular order. Only windows it lists are kept, so every window in the list
//!   can be passed to [`Capture::capture_window`](crate::Capture::capture_window),
//!   which looks the id up there. It needs the Screen Recording grant, which is
//!   checked up front (`CGPreflightScreenCaptureAccess`) as display capture does;
//!   its absence is [`Error::PermissionDenied`]. A grant that is silently withheld
//!   is detected the same way as for display capture ([`check_not_withheld`]).
//!
//! # Which windows are listed
//!
//! [`select_windows`] keeps a window when all of these hold:
//!
//! - It is on the normal window layer (layer 0), where application windows live.
//!   This drops the menu bar and status items, the Dock, desktop widgets,
//!   notifications, floating panels, and screen-wide overlays, all of which sit on
//!   other layers. Full-screen and Stage Manager windows stay on layer 0.
//! - It is not one of Chartreuse's own windows (same process id).
//! - It is visible: alpha above 0, and at least one point wide and tall.
//! - ScreenCaptureKit lists it.
//!
//! The kept windows are numbered front to back ([`WindowInfo::z_order`] 0 is the
//! frontmost), so [`chartreuse_core::window::topmost_at`] finds the window under a
//! point.

use std::collections::HashSet;

use chartreuse_core::geometry::LogicalRect;
use chartreuse_core::window::{WindowId, WindowInfo, WindowOwner};
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};
use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGRect,
};
use objc2_core_graphics::{
    kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowName,
    kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID, CGPreflightScreenCaptureAccess,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_screen_capture_kit::SCShareableContent;

use super::capture::{check_not_withheld, own_pid, permission_denied, with_shareable_content};
use crate::window_list::WindowList;

/// The window server's layer for ordinary application windows
/// (`kCGNormalWindowLevel`).
const NORMAL_LAYER: i64 = 0;

/// The smallest width and height, in points, of a window worth selecting.
const MIN_SIDE: f64 = 1.0;

/// The macOS [`WindowList`] backend.
#[derive(Debug, Default)]
pub struct MacosWindowList;

impl MacosWindowList {
    pub fn new() -> Self {
        Self
    }
}

impl WindowList for MacosWindowList {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        if !CGPreflightScreenCaptureAccess() {
            return future::ready(Err(permission_denied())).boxed();
        }
        with_shareable_content(list_windows)
    }
}

/// Runs on ScreenCaptureKit's queue: reads the window server's list and keeps
/// the windows [`select_windows`] accepts.
fn list_windows(content: &SCShareableContent) -> Result<Vec<WindowInfo>> {
    let own_pid = own_pid();
    // SAFETY: `windows` and `windowID` are plain property getters on valid objects.
    let shareable: HashSet<u32> = unsafe { content.windows() }
        .iter()
        .map(|window| unsafe { window.windowID() })
        .collect();
    let server = server_windows()?;
    check_not_withheld(content)?;
    Ok(select_windows(server, &shareable, own_pid))
}

/// One entry of the window server's list.
#[derive(Debug, Clone, PartialEq)]
struct ServerWindow {
    /// The `CGWindowID`, also ScreenCaptureKit's `SCWindow.windowID`.
    id: u32,
    layer: i64,
    alpha: f64,
    /// Global logical coordinates (see the module docs).
    bounds: LogicalRect,
    owner_name: String,
    owner_pid: i32,
    /// `None` when the window has no title (or an empty one).
    title: Option<String>,
}

/// Keeps the windows a user can select (see the module docs), in the window
/// server's front-to-back order, numbering them from 0 at the front.
///
/// `server` must be front to back; `shareable` holds the ids ScreenCaptureKit
/// lists; windows of `own_pid` are Chartreuse's own.
fn select_windows(
    server: Vec<ServerWindow>,
    shareable: &HashSet<u32>,
    own_pid: i32,
) -> Vec<WindowInfo> {
    server
        .into_iter()
        .filter(|window| is_selectable(window, own_pid) && shareable.contains(&window.id))
        .zip(0..)
        .map(|(window, z_order)| WindowInfo {
            id: WindowId(u64::from(window.id)),
            title: window.title,
            owner: WindowOwner {
                name: window.owner_name,
                pid: u32::try_from(window.owner_pid).ok(),
            },
            bounds: window.bounds,
            z_order,
        })
        .collect()
}

/// Whether `window` is a visible application window of another process.
fn is_selectable(window: &ServerWindow, own_pid: i32) -> bool {
    window.owner_pid != own_pid
        && window.layer == NORMAL_LAYER
        && window.alpha > 0.0
        && window.bounds.size.width >= MIN_SIDE
        && window.bounds.size.height >= MIN_SIDE
}

/// The window server's on-screen windows, front to back, without desktop
/// elements (wallpaper, desktop icons). Entries missing a required key (id,
/// layer, alpha, bounds, owner pid) are skipped; the owner name and title are
/// optional.
fn server_windows() -> Result<Vec<ServerWindow>> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID)
        .ok_or_else(|| Error::Platform("the window server returned no window list".into()))?;
    // SAFETY: CGWindowListCopyWindowInfo returns an array of dictionaries keyed by
    // CFString (the kCGWindow* keys).
    let list =
        unsafe { CFRetained::cast_unchecked::<CFArray<CFDictionary<CFString, CFType>>>(list) };
    Ok(list
        .iter()
        .filter_map(|entry| server_window(&entry))
        .collect())
}

/// Reads one entry of `CGWindowListCopyWindowInfo`.
fn server_window(entry: &CFDictionary<CFString, CFType>) -> Option<ServerWindow> {
    let number = |key: &CFString| {
        entry
            .get(key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_f64))
    };
    let integer = |key: &CFString| {
        entry
            .get(key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
    };
    let string = |key: &CFString| {
        entry
            .get(key)
            .and_then(|value| value.downcast_ref::<CFString>().map(ToString::to_string))
    };
    // SAFETY: the kCGWindow* keys are immutable CoreGraphics constants.
    let (bounds_key, number_key, layer_key, alpha_key, owner_key, pid_key, name_key) = unsafe {
        (
            kCGWindowBounds,
            kCGWindowNumber,
            kCGWindowLayer,
            kCGWindowAlpha,
            kCGWindowOwnerName,
            kCGWindowOwnerPID,
            kCGWindowName,
        )
    };
    let bounds = entry.get(bounds_key)?;
    let bounds = bounds.downcast_ref::<CFDictionary>()?;
    let mut rect = CGRect::default();
    // SAFETY: `rect` is a valid, writable CGRect for the duration of the call.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut rect) } {
        return None;
    }
    Some(ServerWindow {
        id: u32::try_from(integer(number_key)?).ok()?,
        layer: integer(layer_key)?,
        alpha: number(alpha_key)?,
        bounds: LogicalRect::new(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        ),
        owner_name: string(owner_key).unwrap_or_default(),
        owner_pid: i32::try_from(integer(pid_key)?).ok()?,
        title: string(name_key).filter(|title| !title.is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::LogicalPoint;
    use chartreuse_core::window::topmost_at;

    use super::*;

    const OWN_PID: i32 = 500;

    fn server(
        id: u32,
        layer: i64,
        owner: (&str, i32),
        title: &str,
        bounds: LogicalRect,
    ) -> ServerWindow {
        ServerWindow {
            id,
            layer,
            alpha: 1.0,
            bounds,
            owner_name: owner.0.into(),
            owner_pid: owner.1,
            title: (!title.is_empty()).then(|| title.into()),
        }
    }

    fn all_shareable(windows: &[ServerWindow]) -> HashSet<u32> {
        windows.iter().map(|window| window.id).collect()
    }

    fn ids(windows: &[WindowInfo]) -> Vec<u64> {
        windows.iter().map(|window| window.id.0).collect()
    }

    /// A desktop resembling a real one, front to back as the window server
    /// lists it. Displays: the primary at (0, 0, 1512, 982) and a second one to
    /// its left at (-1920, -200, 1920, 1080).
    fn desktop() -> Vec<ServerWindow> {
        let rect = LogicalRect::new;
        vec![
            // Overlays and system chrome above the normal layer.
            server(90, 101, ("OmniWM", 70), "", rect(600.0, 6.0, 527.0, 24.0)),
            server(
                91,
                20,
                ("Dock", 71),
                "",
                rect(-1920.0, 820.0, 3432.0, 162.0),
            ),
            server(
                92,
                24,
                ("Window Server", 404),
                "Menubar",
                rect(0.0, 0.0, 1512.0, 30.0),
            ),
            // Chartreuse's own overlay, frontmost of the normal layer.
            server(
                93,
                0,
                ("Chartreuse", OWN_PID),
                "",
                rect(0.0, 0.0, 1512.0, 982.0),
            ),
            // The focused terminal, overlapping the browser.
            server(
                10,
                0,
                ("Ghostty", 36),
                "~/src",
                rect(700.0, 100.0, 700.0, 600.0),
            ),
            // A browser spanning both displays, with negative x.
            server(
                11,
                0,
                ("Safari", 14),
                "Docs",
                rect(-500.0, 40.0, 1400.0, 800.0),
            ),
            // A window wholly on the left display, above the primary's top edge.
            server(
                12,
                0,
                ("Code", 21),
                "main.rs",
                rect(-1800.0, -150.0, 1400.0, 900.0),
            ),
            // Desktop widgets sit below the normal layer.
            server(
                94,
                -2147483601,
                ("Notification Center", 76),
                "Up Next",
                rect(8.0, 38.0, 360.0, 360.0),
            ),
        ]
    }

    #[test]
    fn keeps_normal_foreign_windows_front_to_back_and_numbers_them() {
        let desktop = desktop();
        let windows = select_windows(desktop.clone(), &all_shareable(&desktop), OWN_PID);
        assert_eq!(ids(&windows), [10, 11, 12]);
        let z: Vec<u32> = windows.iter().map(|window| window.z_order).collect();
        assert_eq!(z, [0, 1, 2]);

        let terminal = &windows[0];
        assert_eq!(terminal.title.as_deref(), Some("~/src"));
        assert_eq!(
            terminal.owner,
            WindowOwner {
                name: "Ghostty".into(),
                pid: Some(36)
            }
        );
        assert_eq!(
            terminal.bounds,
            LogicalRect::new(700.0, 100.0, 700.0, 600.0)
        );
    }

    #[test]
    fn drops_invisible_windows() {
        let rect = LogicalRect::new(0.0, 0.0, 400.0, 300.0);
        let mut transparent = server(1, 0, ("App", 1), "Hidden", rect);
        transparent.alpha = 0.0;
        let visible = server(2, 0, ("App", 1), "Shown", rect);
        let zero_width = server(
            3,
            0,
            ("App", 1),
            "",
            LogicalRect::new(10.0, 10.0, 0.0, 300.0),
        );
        let sliver = server(
            4,
            0,
            ("App", 1),
            "",
            LogicalRect::new(10.0, 10.0, 400.0, 0.5),
        );
        let mut faint = server(5, 0, ("App", 1), "Faint", rect);
        faint.alpha = 0.05;
        let windows = vec![transparent, visible, zero_width, sliver, faint];
        let kept = select_windows(windows.clone(), &all_shareable(&windows), OWN_PID);
        assert_eq!(ids(&kept), [2, 5]);
        assert_eq!(kept[1].z_order, 1);
    }

    #[test]
    fn drops_windows_screencapturekit_cannot_capture() {
        let desktop = desktop();
        let shareable = HashSet::from([10, 12]);
        let windows = select_windows(desktop, &shareable, OWN_PID);
        assert_eq!(ids(&windows), [10, 12]);
        assert_eq!(windows[1].z_order, 1);
    }

    #[test]
    fn untitled_windows_keep_their_owner() {
        let rect = LogicalRect::new(0.0, 0.0, 400.0, 300.0);
        let windows = vec![server(1, 0, ("Finder", 9), "", rect)];
        let kept = select_windows(windows.clone(), &all_shareable(&windows), OWN_PID);
        assert_eq!(kept[0].title, None);
        assert_eq!(kept[0].owner.name, "Finder");
    }

    #[test]
    fn topmost_window_follows_stacking_across_displays() {
        let desktop = desktop();
        let windows = select_windows(desktop.clone(), &all_shareable(&desktop), OWN_PID);
        let at = |x, y| topmost_at(&windows, LogicalPoint::new(x, y)).map(|window| window.id.0);

        // Where the terminal overlaps the browser, the terminal is in front.
        assert_eq!(at(800.0, 200.0), Some(10));
        // The browser beyond the terminal, on the primary display.
        assert_eq!(at(100.0, 500.0), Some(11));
        // The same browser on the left display (negative x).
        assert_eq!(at(-400.0, 500.0), Some(11));
        // The browser covers the editor where they overlap on the left display.
        assert_eq!(at(-450.0, 100.0), Some(11));
        assert_eq!(at(-600.0, 100.0), Some(12));
        // Above the primary display's top edge: only the editor is there.
        assert_eq!(at(-1000.0, -100.0), Some(12));
        // The menu bar, Dock and Chartreuse's overlay are not selectable, so
        // points on them find the window beneath, or nothing.
        assert_eq!(at(750.0, 10.0), None);
        assert_eq!(at(100.0, 900.0), None);
        assert_eq!(at(100.0, 830.0), Some(11));
        // Half-open bounds: the right edge belongs to the next window over.
        assert_eq!(at(1400.0, 200.0), None);
        assert_eq!(at(1399.5, 200.0), Some(10));
    }
}

//! Windows: which top-level windows window selection offers, and how they are
//! described (the portable part of `window_list.rs`).
//!
//! `EnumWindows` lists every top-level window front to back. [`is_selectable`]
//! keeps a window when all of these hold:
//!
//! - It is visible (`IsWindowVisible`) and not minimized (minimized windows are
//!   parked off screen).
//! - DWM does not cloak it (`DWMWA_CLOAKED`): cloaked windows are on another
//!   virtual desktop, or are suspended or hidden UWP windows.
//! - It is not a tool window (`WS_EX_TOOLWINDOW`: floating palettes, tooltips,
//!   menus, overlays) and not click-through (`WS_EX_TRANSPARENT`, such as game and
//!   GPU overlays, which the pointer passes through anyway).
//! - It is not part of the shell: the desktop (`Progman`, `WorkerW`) or a taskbar
//!   (`Shell_TrayWnd`, `Shell_SecondaryTrayWnd`).
//! - It is not one of Chartreuse's own windows (same process id).
//!
//! [`describe`] then converts the survivors' DWM extended frame bounds (the
//! visible frame, without the invisible resize borders and the shadow) to logical
//! coordinates, drops windows smaller than a logical point, and numbers them front
//! to back.

use chartreuse_core::geometry::PhysicalRect;
use chartreuse_core::window::{WindowId, WindowInfo, WindowOwner};

use super::layout::MonitorLayout;

/// `WS_EX_TRANSPARENT`.
pub(super) const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
/// `WS_EX_TOOLWINDOW`.
pub(super) const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;

/// Window classes of the shell's desktop and taskbars.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// The smallest logical width and height of a window worth selecting.
const MIN_SIDE: f64 = 1.0;

/// The cheap-to-read attributes [`is_selectable`] looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Candidate {
    pub pid: u32,
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: bool,
    /// `GWL_EXSTYLE`.
    pub ex_style: u32,
    pub class: String,
}

/// Whether window selection offers the window (see the module docs).
pub(super) fn is_selectable(candidate: &Candidate, own_pid: u32) -> bool {
    candidate.pid != own_pid
        && candidate.visible
        && !candidate.minimized
        && !candidate.cloaked
        && candidate.ex_style & (WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT) == 0
        && !SHELL_CLASSES.contains(&candidate.class.as_str())
}

/// A selectable window's details, in `EnumWindows` (front-to-back) order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listed {
    /// The `HWND`, which is also the [`WindowId`].
    pub hwnd: u64,
    pub pid: u32,
    /// Empty if the window has no title.
    pub title: String,
    /// The executable's name without its extension, or empty if unknown.
    pub owner_name: String,
    /// DWM extended frame bounds, in physical virtual-screen pixels.
    pub bounds: PhysicalRect,
}

/// Converts `windows` (front to back) to [`WindowInfo`]s in logical coordinates,
/// dropping windows under a logical point in either direction, and numbers the rest
/// from 0 at the front.
pub(super) fn describe(windows: Vec<Listed>, layout: &MonitorLayout) -> Vec<WindowInfo> {
    windows
        .into_iter()
        .filter_map(|window| {
            let bounds = layout.rect_to_logical(window.bounds)?;
            (bounds.size.width >= MIN_SIDE && bounds.size.height >= MIN_SIDE)
                .then_some((window, bounds))
        })
        .zip(0..)
        .map(|((window, bounds), z_order)| WindowInfo {
            id: WindowId(window.hwnd),
            title: (!window.title.is_empty()).then_some(window.title),
            owner: WindowOwner {
                name: window.owner_name,
                pid: Some(window.pid),
            },
            bounds,
            z_order,
        })
        .collect()
}

/// The executable's name without directory or extension, from a full image path
/// (`C:\Program Files\App\app.exe` → `app`).
pub(super) fn owner_name(image_path: &str) -> String {
    let file = image_path.rsplit(['\\', '/']).next().unwrap_or(image_path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => file.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::{LogicalPoint, LogicalRect, ScaleFactor};
    use chartreuse_core::window::topmost_at;

    use super::super::layout::Monitor;
    use super::*;

    const OWN_PID: u32 = 500;

    fn candidate() -> Candidate {
        Candidate {
            pid: 42,
            visible: true,
            minimized: false,
            cloaked: false,
            ex_style: 0x0000_0100, // WS_EX_WINDOWEDGE, as ordinary windows have.
            class: "Chrome_WidgetWin_1".into(),
        }
    }

    #[test]
    fn ordinary_windows_of_other_processes_are_selectable() {
        assert!(is_selectable(&candidate(), OWN_PID));
    }

    #[test]
    fn hidden_cloaked_minimized_and_own_windows_are_not() {
        let base = candidate();
        let cases = [
            (
                "invisible",
                Candidate {
                    visible: false,
                    ..base.clone()
                },
            ),
            (
                "minimized",
                Candidate {
                    minimized: true,
                    ..base.clone()
                },
            ),
            (
                "cloaked",
                Candidate {
                    cloaked: true,
                    ..base.clone()
                },
            ),
            (
                "own",
                Candidate {
                    pid: OWN_PID,
                    ..base.clone()
                },
            ),
            (
                "tool window",
                Candidate {
                    ex_style: base.ex_style | WS_EX_TOOLWINDOW,
                    ..base.clone()
                },
            ),
        ];
        for (name, candidate) in cases {
            assert!(!is_selectable(&candidate, OWN_PID), "{name}");
        }
    }

    #[test]
    fn click_through_overlays_and_the_shell_are_not_selectable() {
        let mut overlay = candidate();
        overlay.ex_style |= WS_EX_TRANSPARENT;
        assert!(!is_selectable(&overlay, OWN_PID));
        for class in SHELL_CLASSES {
            let mut shell = candidate();
            shell.class = class.into();
            assert!(!is_selectable(&shell, OWN_PID), "{class}");
        }
    }

    fn listed(hwnd: u64, title: &str, bounds: PhysicalRect) -> Listed {
        Listed {
            hwnd,
            pid: 7,
            title: title.into(),
            owner_name: "app".into(),
            bounds,
        }
    }

    /// A 2× primary (3840 × 2160 px) with a 1× monitor to its right.
    fn layout() -> MonitorLayout {
        let monitor = |x, width, height, scale, is_primary| Monitor {
            physical: PhysicalRect::new(x, 0, width, height),
            scale: ScaleFactor::new(scale).unwrap(),
            is_primary,
        };
        MonitorLayout::new(vec![
            monitor(0, 3840, 2160, 2.0, true),
            monitor(3840, 1920, 1080, 1.0, false),
        ])
    }

    #[test]
    fn windows_are_numbered_front_to_back_in_logical_coordinates() {
        let windows = describe(
            vec![
                listed(0x10, "Editor", PhysicalRect::new(200, 200, 1600, 1200)),
                listed(0x20, "", PhysicalRect::new(3940, 100, 800, 600)),
            ],
            &layout(),
        );
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, WindowId(0x10));
        assert_eq!(windows[0].title.as_deref(), Some("Editor"));
        assert_eq!(
            windows[0].bounds,
            LogicalRect::new(100.0, 100.0, 800.0, 600.0)
        );
        assert_eq!(windows[0].z_order, 0);
        assert_eq!(
            windows[0].owner,
            WindowOwner {
                name: "app".into(),
                pid: Some(7)
            }
        );
        assert_eq!(windows[1].title, None);
        assert_eq!(
            windows[1].bounds,
            LogicalRect::new(2020.0, 100.0, 800.0, 600.0)
        );
        assert_eq!(windows[1].z_order, 1);
    }

    #[test]
    fn slivers_are_dropped_without_gaps_in_the_numbering() {
        let windows = describe(
            vec![
                // One physical pixel on the 2× display is half a logical point.
                listed(1, "sliver", PhysicalRect::new(0, 0, 1, 400)),
                listed(2, "empty", PhysicalRect::new(10, 10, 0, 0)),
                listed(3, "kept", PhysicalRect::new(0, 0, 400, 400)),
            ],
            &layout(),
        );
        assert_eq!(windows.len(), 1);
        assert_eq!((windows[0].id, windows[0].z_order), (WindowId(3), 0));
    }

    #[test]
    fn the_front_window_wins_hit_tests() {
        let windows = describe(
            vec![
                listed(1, "front", PhysicalRect::new(400, 400, 800, 800)),
                listed(2, "back", PhysicalRect::new(0, 0, 2000, 2000)),
            ],
            &layout(),
        );
        let at = |x, y| topmost_at(&windows, LogicalPoint::new(x, y)).map(|w| w.id.0);
        assert_eq!(at(300.0, 300.0), Some(1));
        assert_eq!(at(100.0, 100.0), Some(2));
        assert_eq!(at(1500.0, 100.0), None);
    }

    #[test]
    fn owner_names_are_executable_stems() {
        assert_eq!(
            owner_name(r"C:\Program Files\Mozilla Firefox\firefox.exe"),
            "firefox"
        );
        assert_eq!(owner_name(r"C:\Windows\explorer.EXE"), "explorer");
        assert_eq!(owner_name(r"\\?\C:\tools\my.app.exe"), "my.app");
        assert_eq!(owner_name("noextension"), "noextension");
        assert_eq!(owner_name(r"C:\dir\.hidden"), ".hidden");
    }
}

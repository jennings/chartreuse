//! Turning EWMH client windows into Chartreuse's window list.

use chartreuse_core::geometry::{PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor};
use chartreuse_core::window::{WindowId, WindowInfo, WindowOwner};

/// `_NET_WM_DESKTOP` of windows shown on every virtual desktop.
pub const ALL_DESKTOPS: u32 = 0xffff_ffff;

/// What the backend reads about one client window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientWindow {
    pub id: u32,
    pub title: Option<String>,
    /// The `WM_CLASS` class, else the process name.
    pub owner: Option<String>,
    pub pid: Option<u32>,
    /// The window manager's frame around the client (the client itself when
    /// the window manager does not reparent), in root coordinates.
    pub frame: PhysicalRect,
    /// The client window, in root coordinates.
    pub client: PhysicalRect,
    /// `_GTK_FRAME_EXTENTS`: the invisible shadow margins (left, right, top,
    /// bottom) client-side decorated windows draw inside the client.
    pub shadow: Option<[u32; 4]>,
    /// Mapped, with every ancestor mapped (`IsViewable`).
    pub viewable: bool,
    /// Minimized (`_NET_WM_STATE_HIDDEN`).
    pub hidden: bool,
    /// A desktop background or a panel, not a window the user would pick.
    pub desktop_or_dock: bool,
    /// `_NET_WM_DESKTOP`.
    pub desktop: Option<u32>,
}

impl ClientWindow {
    /// The part of the root window that shows the window: its frame, or for a
    /// client-side decorated window its client area less the shadows.
    #[must_use]
    pub fn visible_area(&self) -> PhysicalRect {
        let Some([left, right, top, bottom]) = self.shadow else {
            return self.frame;
        };
        let client = self.client;
        let inset = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
        let shrunk = PhysicalRect {
            origin: PhysicalPoint::new(
                client.origin.x.saturating_add(inset(left)),
                client.origin.y.saturating_add(inset(top)),
            ),
            size: PhysicalSize::new(
                client.size.width.saturating_sub(left.saturating_add(right)),
                client
                    .size
                    .height
                    .saturating_sub(top.saturating_add(bottom)),
            ),
        };
        shrunk.intersection(&self.frame).unwrap_or(shrunk)
    }
}

/// The windows the user can pick, front to back, from `clients` in stacking
/// order from the **top**: viewable, not minimized, on the current desktop
/// (`current_desktop`, when known), not a desktop or panel, not Chartreuse's
/// own (`own_pid`), and not empty. Bounds are root pixels divided by `scale`.
#[must_use]
pub fn window_list(
    clients: &[ClientWindow],
    current_desktop: Option<u32>,
    own_pid: u32,
    scale: ScaleFactor,
) -> Vec<WindowInfo> {
    clients
        .iter()
        .filter(|client| client.viewable && !client.hidden && !client.desktop_or_dock)
        .filter(|client| client.pid != Some(own_pid))
        .filter(|client| match (client.desktop, current_desktop) {
            (Some(desktop), Some(current)) => desktop == current || desktop == ALL_DESKTOPS,
            _ => true,
        })
        .filter(|client| !client.visible_area().is_empty())
        .zip(0..)
        .map(|(client, z_order)| WindowInfo {
            id: WindowId(u64::from(client.id)),
            title: client.title.clone().filter(|title| !title.is_empty()),
            owner: WindowOwner {
                name: client.owner.clone().unwrap_or_else(|| "Unknown".into()),
                pid: client.pid,
            },
            bounds: client.visible_area().to_logical(scale),
            z_order,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::LogicalRect;

    use super::*;

    fn client(id: u32, frame: PhysicalRect) -> ClientWindow {
        ClientWindow {
            id,
            title: Some(format!("Window {id}")),
            owner: Some("Test".into()),
            pid: Some(100 + id),
            frame,
            client: frame,
            shadow: None,
            viewable: true,
            hidden: false,
            desktop_or_dock: false,
            desktop: Some(0),
        }
    }

    fn ids(windows: &[WindowInfo]) -> Vec<u64> {
        windows.iter().map(|window| window.id.0).collect()
    }

    #[test]
    fn windows_keep_the_stacking_order_with_consecutive_z_orders() {
        let clients = [
            ClientWindow {
                hidden: true,
                ..client(1, PhysicalRect::new(0, 0, 10, 10))
            },
            client(2, PhysicalRect::new(0, 0, 10, 10)),
            client(3, PhysicalRect::new(5, 5, 10, 10)),
        ];
        let windows = window_list(&clients, Some(0), 1, ScaleFactor::ONE);
        assert_eq!(ids(&windows), [2, 3]);
        let z: Vec<u32> = windows.iter().map(|window| window.z_order).collect();
        assert_eq!(z, [0, 1]);
    }

    #[test]
    fn unpickable_windows_are_left_out() {
        let area = PhysicalRect::new(0, 0, 10, 10);
        let clients = [
            ClientWindow {
                viewable: false,
                ..client(1, area)
            },
            ClientWindow {
                desktop_or_dock: true,
                ..client(2, area)
            },
            ClientWindow {
                desktop: Some(1),
                ..client(3, area)
            },
            ClientWindow {
                desktop: Some(ALL_DESKTOPS),
                ..client(4, area)
            },
            ClientWindow {
                pid: Some(42),
                ..client(5, area)
            },
            client(6, PhysicalRect::new(0, 0, 0, 10)),
            ClientWindow {
                desktop: None,
                ..client(7, area)
            },
        ];
        assert_eq!(
            ids(&window_list(&clients, Some(0), 42, ScaleFactor::ONE)),
            [4, 7]
        );
        // Without a current desktop, every desktop's windows count.
        assert_eq!(
            ids(&window_list(&clients, None, 42, ScaleFactor::ONE)),
            [3, 4, 7]
        );
    }

    #[test]
    fn client_side_shadows_are_not_part_of_the_window() {
        let csd = ClientWindow {
            shadow: Some([10, 20, 5, 15]),
            ..client(1, PhysicalRect::new(100, 100, 300, 200))
        };
        assert_eq!(csd.visible_area(), PhysicalRect::new(110, 105, 270, 180));
        // Server-side decorations: the whole frame, title bar included.
        let ssd = ClientWindow {
            client: PhysicalRect::new(102, 130, 296, 168),
            ..client(2, PhysicalRect::new(100, 100, 300, 200))
        };
        assert_eq!(ssd.visible_area(), PhysicalRect::new(100, 100, 300, 200));
    }

    #[test]
    fn bounds_are_logical_and_names_fall_back() {
        let clients = [ClientWindow {
            title: Some(String::new()),
            owner: None,
            ..client(1, PhysicalRect::new(-300, 150, 600, 300))
        }];
        let window = &window_list(&clients, None, 0, ScaleFactor::new(1.5).unwrap())[0];
        assert_eq!(window.bounds, LogicalRect::new(-200.0, 100.0, 400.0, 200.0));
        assert_eq!(window.title, None);
        assert_eq!(window.owner.name, "Unknown");
    }
}

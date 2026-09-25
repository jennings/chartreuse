//! On-screen windows, used by window-selection mode.

use crate::geometry::{LogicalPoint, LogicalRect};

/// Identifies an on-screen window. Backend-defined and opaque (a `CGWindowID` on
/// macOS, an `HWND` on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// The application that owns a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowOwner {
    /// The application's display name.
    pub name: String,
    /// The owning process, when the platform exposes it.
    pub pid: Option<u32>,
}

/// One on-screen window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    /// The window title, when the platform exposes it (macOS hides titles without
    /// Screen Recording permission).
    pub title: Option<String>,
    pub owner: WindowOwner,
    /// The window's visible frame in the global logical desktop space.
    pub bounds: LogicalRect,
    /// Stacking position: `0` is the frontmost window, larger values are further
    /// back. Unique within one window list.
    pub z_order: u32,
}

/// The frontmost window whose bounds contain `point`, if any.
#[must_use]
pub fn topmost_at(windows: &[WindowInfo], point: LogicalPoint) -> Option<&WindowInfo> {
    windows
        .iter()
        .filter(|window| window.bounds.contains(point))
        .min_by_key(|window| window.z_order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u64, z_order: u32, bounds: LogicalRect) -> WindowInfo {
        WindowInfo {
            id: WindowId(id),
            title: None,
            owner: WindowOwner {
                name: "Test".into(),
                pid: None,
            },
            bounds,
            z_order,
        }
    }

    #[test]
    fn topmost_at_prefers_the_lowest_z_order_regardless_of_list_order() {
        let windows = [
            window(1, 2, LogicalRect::new(0.0, 0.0, 100.0, 100.0)),
            window(2, 0, LogicalRect::new(50.0, 50.0, 100.0, 100.0)),
            window(3, 1, LogicalRect::new(-100.0, 0.0, 400.0, 400.0)),
        ];
        let id = |p| topmost_at(&windows, p).map(|w| w.id);
        assert_eq!(id(LogicalPoint::new(75.0, 75.0)), Some(WindowId(2)));
        assert_eq!(id(LogicalPoint::new(10.0, 10.0)), Some(WindowId(3)));
        assert_eq!(id(LogicalPoint::new(-50.0, 10.0)), Some(WindowId(3)));
        assert_eq!(id(LogicalPoint::new(-150.0, 10.0)), None);
    }
}

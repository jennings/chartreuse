//! Windows: mapping the physical virtual screen to the global logical desktop space.
//!
//! A Per-Monitor DPI Aware (v2) process sees every coordinate — monitor
//! rectangles, window bounds, the cursor — in *physical* pixels of the virtual
//! screen, whose origin is the primary monitor's top-left corner. Each monitor has
//! its own DPI; its scale factor is `dpi / 96` ([`scale_for_dpi`]).
//!
//! Chartreuse's logical space needs each display's logical size to be its
//! physical size divided by its own scale, with the primary at the origin. With
//! mixed scale factors, dividing every physical coordinate by one scale would open
//! gaps or overlaps between neighbours, so [`MonitorLayout`] places the monitors
//! instead:
//!
//! 1. The primary monitor's logical origin is `(0, 0)`.
//! 2. Repeatedly, an unplaced monitor that physically touches a placed one along
//!    an edge (the shared edge has positive length) is put against the same edge in
//!    logical space. Its offset along that edge is the physical offset divided by
//!    the placed neighbour's scale, so top- or left-aligned neighbours stay
//!    aligned. The first touching pair in monitor order wins, which makes the
//!    result deterministic.
//! 3. A monitor touching none (Windows' display settings snap monitors together,
//!    so this is rare) keeps its physical origin divided by its own scale.
//!
//! Neighbours therefore stay adjacent without gaps; where monitors of mixed
//! scale form an L or a ring, logical rectangles can overlap slightly, which the
//! display model tolerates.
//!
//! Physical points and rectangles elsewhere on the desktop (window bounds) are
//! converted through the monitor they are on ([`MonitorLayout::rect_to_logical`]):
//! within a monitor, `logical = logical_origin + (physical - physical_origin) /
//! scale`.

use chartreuse_core::geometry::{
    LogicalPoint, LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor,
};

/// The DPI Windows treats as scale factor 1 (`USER_DEFAULT_SCREEN_DPI`).
pub(super) const BASE_DPI: u32 = 96;

/// The scale factor of a monitor with effective DPI `dpi`, or `None` for 0.
pub(super) fn scale_for_dpi(dpi: u32) -> Option<ScaleFactor> {
    ScaleFactor::new(f64::from(dpi) / f64::from(BASE_DPI))
}

/// One monitor as Windows reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Monitor {
    /// `MONITORINFO::rcMonitor`, in physical virtual-screen pixels.
    pub physical: PhysicalRect,
    pub scale: ScaleFactor,
    pub is_primary: bool,
}

/// Monitors placed in the global logical desktop space (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MonitorLayout {
    monitors: Vec<Monitor>,
    /// The logical origin of each monitor, by index.
    origins: Vec<LogicalPoint>,
}

impl MonitorLayout {
    /// Places `monitors`. If none is marked primary, the first is treated as the
    /// primary.
    pub(super) fn new(monitors: Vec<Monitor>) -> Self {
        let mut origins: Vec<Option<LogicalPoint>> = vec![None; monitors.len()];
        if let Some(primary) = monitors
            .iter()
            .position(|m| m.is_primary)
            .or((!monitors.is_empty()).then_some(0))
        {
            origins[primary] = Some(LogicalPoint::new(0.0, 0.0));
        }
        // Each pass places at least one monitor or stops, so this terminates.
        while let Some((index, origin)) = next_placement(&monitors, &origins) {
            origins[index] = Some(origin);
        }
        let origins = origins
            .into_iter()
            .zip(&monitors)
            .map(|(origin, monitor)| {
                origin.unwrap_or_else(|| monitor.physical.origin.to_logical(monitor.scale))
            })
            .collect();
        Self { monitors, origins }
    }

    /// The logical bounds of monitor `index` (in the order given to [`new`](Self::new)).
    pub(super) fn logical_bounds(&self, index: usize) -> LogicalRect {
        let monitor = &self.monitors[index];
        LogicalRect {
            origin: self.origins[index],
            size: monitor.physical.size.to_logical(monitor.scale),
        }
    }

    /// Converts a physical rectangle (window bounds) to logical coordinates through
    /// the monitor it overlaps most, or the nearest monitor if it overlaps none.
    /// `None` only if there are no monitors.
    pub(super) fn rect_to_logical(&self, rect: PhysicalRect) -> Option<LogicalRect> {
        let index = self.monitor_for(rect)?;
        let top_left = self.point_to_logical(index, rect.origin);
        let size = rect.size.to_logical(self.monitors[index].scale);
        Some(LogicalRect {
            origin: top_left,
            size,
        })
    }

    /// The monitor with the largest overlap with `rect`, else the one closest to
    /// its centre (like `MonitorFromRect` with `MONITOR_DEFAULTTONEAREST`).
    fn monitor_for(&self, rect: PhysicalRect) -> Option<usize> {
        let area = |r: PhysicalRect| u64::from(r.size.width) * u64::from(r.size.height);
        let overlapping = self
            .monitors
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((i, area(m.physical.intersection(&rect)?))))
            // `max_by_key` keeps the last maximum; reverse so the first one wins.
            .rev()
            .max_by_key(|&(_, overlap)| overlap)
            .map(|(i, _)| i);
        overlapping.or_else(|| {
            let cx = rect.min_x() as f64 + f64::from(rect.size.width) / 2.0;
            let cy = rect.min_y() as f64 + f64::from(rect.size.height) / 2.0;
            self.monitors
                .iter()
                .enumerate()
                .map(|(i, m)| (i, distance_squared(m.physical, cx, cy)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        })
    }

    fn point_to_logical(&self, index: usize, point: PhysicalPoint) -> LogicalPoint {
        let monitor = &self.monitors[index];
        let origin = self.origins[index];
        let scale = monitor.scale.get();
        LogicalPoint::new(
            origin.x + (f64::from(point.x) - f64::from(monitor.physical.min_x())) / scale,
            origin.y + (f64::from(point.y) - f64::from(monitor.physical.min_y())) / scale,
        )
    }
}

/// The squared distance from `(x, y)` to the nearest point of `rect`.
fn distance_squared(rect: PhysicalRect, x: f64, y: f64) -> f64 {
    let dx = (rect.min_x() as f64 - x)
        .max(0.0)
        .max(x - rect.max_x() as f64);
    let dy = (rect.min_y() as f64 - y)
        .max(0.0)
        .max(y - rect.max_y() as f64);
    dx * dx + dy * dy
}

/// The first unplaced monitor touching a placed one, and its logical origin.
fn next_placement(
    monitors: &[Monitor],
    origins: &[Option<LogicalPoint>],
) -> Option<(usize, LogicalPoint)> {
    for (index, monitor) in monitors.iter().enumerate() {
        if origins[index].is_some() {
            continue;
        }
        for (placed, origin) in monitors.iter().zip(origins) {
            if let Some(origin) = origin
                && let Some(found) = place_against(monitor, placed, *origin)
            {
                return Some((index, found));
            }
        }
    }
    None
}

/// The logical origin of `monitor` against the edge it shares with `placed`
/// (whose logical origin is `origin`), or `None` if they share no edge.
fn place_against(
    monitor: &Monitor,
    placed: &Monitor,
    origin: LogicalPoint,
) -> Option<LogicalPoint> {
    let (m, p) = (monitor.physical, placed.physical);
    let p_scale = placed.scale.get();
    let p_size = p.size.to_logical(placed.scale);
    let m_size = m.size.to_logical(monitor.scale);
    let overlaps_vertically = i64::from(m.min_y()) < p.max_y() && i64::from(p.min_y()) < m.max_y();
    let overlaps_horizontally =
        i64::from(m.min_x()) < p.max_x() && i64::from(p.min_x()) < m.max_x();
    // The offset along the shared edge, measured on the placed monitor.
    let along_y = origin.y + f64::from(m.min_y() - p.min_y()) / p_scale;
    let along_x = origin.x + f64::from(m.min_x() - p.min_x()) / p_scale;
    if overlaps_vertically && i64::from(m.min_x()) == p.max_x() {
        Some(LogicalPoint::new(origin.x + p_size.width, along_y))
    } else if overlaps_vertically && m.max_x() == i64::from(p.min_x()) {
        Some(LogicalPoint::new(origin.x - m_size.width, along_y))
    } else if overlaps_horizontally && i64::from(m.min_y()) == p.max_y() {
        Some(LogicalPoint::new(along_x, origin.y + p_size.height))
    } else if overlaps_horizontally && m.max_y() == i64::from(p.min_y()) {
        Some(LogicalPoint::new(along_x, origin.y - m_size.height))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::PhysicalSize;

    use super::*;

    fn monitor(x: i32, y: i32, width: u32, height: u32, scale: f64, is_primary: bool) -> Monitor {
        Monitor {
            physical: PhysicalRect::new(x, y, width, height),
            scale: ScaleFactor::new(scale).unwrap(),
            is_primary,
        }
    }

    fn bounds(layout: &MonitorLayout) -> Vec<LogicalRect> {
        (0..layout.monitors.len())
            .map(|i| layout.logical_bounds(i))
            .collect()
    }

    #[test]
    fn dpi_maps_to_scale_relative_to_96() {
        assert_eq!(scale_for_dpi(96).unwrap().get(), 1.0);
        assert_eq!(scale_for_dpi(144).unwrap().get(), 1.5);
        assert_eq!(scale_for_dpi(192).unwrap().get(), 2.0);
        assert_eq!(scale_for_dpi(120).unwrap().get(), 1.25);
        assert_eq!(scale_for_dpi(0), None);
    }

    #[test]
    fn primary_sits_at_the_origin_with_its_size_divided_by_its_scale() {
        let layout = MonitorLayout::new(vec![monitor(0, 0, 3840, 2160, 2.0, true)]);
        assert_eq!(
            bounds(&layout),
            [LogicalRect::new(0.0, 0.0, 1920.0, 1080.0)]
        );
    }

    #[test]
    fn logical_sizes_scale_back_to_the_physical_size() {
        // The display model requires pixel_size == logical size × scale (rounded).
        for (width, height, scale) in [(2560, 1440, 1.25), (2880, 1800, 1.75), (1366, 768, 1.5)] {
            let layout = MonitorLayout::new(vec![monitor(0, 0, width, height, scale, true)]);
            let logical = layout.logical_bounds(0);
            assert_eq!(
                logical.size.to_physical(ScaleFactor::new(scale).unwrap()),
                PhysicalSize::new(width, height),
                "{width}×{height} at {scale}"
            );
        }
    }

    #[test]
    fn mixed_scale_neighbours_stay_adjacent_on_every_side() {
        // A 2× primary with 1× monitors right, left, above and below it.
        let layout = MonitorLayout::new(vec![
            monitor(0, 0, 3840, 2160, 2.0, true),
            monitor(3840, 0, 1920, 1080, 1.0, false),
            monitor(-1920, 200, 1920, 1080, 1.0, false),
            monitor(400, -1080, 1920, 1080, 1.0, false),
            monitor(0, 2160, 1280, 1024, 1.0, false),
        ]);
        assert_eq!(
            bounds(&layout),
            [
                LogicalRect::new(0.0, 0.0, 1920.0, 1080.0),
                // Dividing its physical x by any one scale would leave a gap or overlap.
                LogicalRect::new(1920.0, 0.0, 1920.0, 1080.0),
                LogicalRect::new(-1920.0, 100.0, 1920.0, 1080.0),
                LogicalRect::new(200.0, -1080.0, 1920.0, 1080.0),
                LogicalRect::new(0.0, 1080.0, 1280.0, 1024.0),
            ]
        );
    }

    #[test]
    fn monitors_are_placed_through_chains_of_neighbours() {
        // Primary, then a 1.5× monitor to its right, then a 1× one right of that,
        // listed in an order where the far one comes first.
        let layout = MonitorLayout::new(vec![
            monitor(1920 + 2400, 0, 1920, 1080, 1.0, false),
            monitor(0, 0, 1920, 1080, 1.0, true),
            monitor(1920, 0, 2400, 1350, 1.5, false),
        ]);
        assert_eq!(
            bounds(&layout),
            [
                LogicalRect::new(1920.0 + 1600.0, 0.0, 1920.0, 1080.0),
                LogicalRect::new(0.0, 0.0, 1920.0, 1080.0),
                LogicalRect::new(1920.0, 0.0, 1600.0, 900.0),
            ]
        );
    }

    #[test]
    fn corner_contact_is_not_adjacency() {
        // Touches the primary only at its bottom-right corner, so it falls back to
        // its physical origin over its own scale.
        let layout = MonitorLayout::new(vec![
            monitor(0, 0, 3840, 2160, 2.0, true),
            monitor(3840, 2160, 1920, 1080, 1.0, false),
        ]);
        assert_eq!(
            layout.logical_bounds(1),
            LogicalRect::new(3840.0, 2160.0, 1920.0, 1080.0)
        );
    }

    #[test]
    fn windows_convert_through_the_monitor_they_overlap_most() {
        let layout = MonitorLayout::new(vec![
            monitor(0, 0, 3840, 2160, 2.0, true),
            monitor(3840, 0, 1920, 1080, 1.0, false),
        ]);
        // Wholly on the primary: halved.
        assert_eq!(
            layout.rect_to_logical(PhysicalRect::new(200, 100, 800, 600)),
            Some(LogicalRect::new(100.0, 50.0, 400.0, 300.0))
        );
        // Mostly on the 1× monitor, hanging 100 px over the primary's right edge.
        assert_eq!(
            layout.rect_to_logical(PhysicalRect::new(3740, 10, 1000, 500)),
            Some(LogicalRect::new(1820.0, 10.0, 1000.0, 500.0))
        );
        // Mostly on the primary, hanging over onto the 1× monitor.
        assert_eq!(
            layout.rect_to_logical(PhysicalRect::new(3040, 0, 1000, 400)),
            Some(LogicalRect::new(1520.0, 0.0, 500.0, 200.0))
        );
    }

    #[test]
    fn off_screen_windows_convert_through_the_nearest_monitor() {
        let layout = MonitorLayout::new(vec![
            monitor(0, 0, 3840, 2160, 2.0, true),
            monitor(3840, 0, 1920, 1080, 1.0, false),
        ]);
        // Beyond the 1× monitor's right edge.
        assert_eq!(
            layout.rect_to_logical(PhysicalRect::new(6000, 100, 100, 100)),
            Some(LogicalRect::new(1920.0 + 2160.0, 100.0, 100.0, 100.0))
        );
        assert_eq!(
            MonitorLayout::new(Vec::new()).rect_to_logical(PhysicalRect::new(0, 0, 1, 1)),
            None
        );
    }

    #[test]
    fn without_a_primary_the_first_monitor_is_the_origin() {
        let layout = MonitorLayout::new(vec![
            monitor(100, 100, 1920, 1080, 1.0, false),
            monitor(2020, 100, 1920, 1080, 1.0, false),
        ]);
        assert_eq!(
            bounds(&layout),
            [
                LogicalRect::new(0.0, 0.0, 1920.0, 1080.0),
                LogicalRect::new(1920.0, 0.0, 1920.0, 1080.0),
            ]
        );
    }
}

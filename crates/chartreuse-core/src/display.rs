//! The display model: every display's geometry in logical and physical units.
//!
//! All capture and overlay placement goes through these types so coordinate
//! conversions live in one place.
//!
//! - [`DisplayInfo`] describes one display as a backend reports it.
//! - [`PixelGrid`] maps a logical region of the desktop onto a pixel grid whose
//!   origin `(0, 0)` is the region's top-left corner. A display's own framebuffer
//!   is the grid [`DisplayInfo::pixel_grid`]; a whole-desktop composite is
//!   [`DisplayLayout::desktop_grid`]; a cropped selection is
//!   [`DisplayLayout::capture_grid`].
//! - [`DisplayLayout`] is a validated set of displays: hit testing, per-display
//!   intersections and the desktop's bounding rectangle.
//!
//! # Mixed scale factors
//!
//! When content from displays with different scale factors is combined into one
//! image (a full-desktop composite or a selection spanning displays), the image is
//! rendered at the **maximum** scale factor among the displays involved, and content
//! from lower-scale displays is upscaled into it. The output's pixel size is its
//! logical size times that scale, rounded like every other logical → physical
//! conversion (see [`PixelGrid::pixel_size`]).

use crate::geometry::{
    LogicalPoint, LogicalRect, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};

/// Identifies a display for as long as it stays connected.
///
/// The value is backend-defined and opaque (a `CGDirectDisplayID` on macOS, for
/// example); only compare it for equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DisplayId(pub u64);

/// One connected display.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub id: DisplayId,
    /// A human-readable name, such as "Built-in Retina Display".
    pub name: String,
    /// The display's area in the global logical desktop space (see
    /// [`crate::geometry`]): top-left origin at the primary display's top-left
    /// corner, y pointing down.
    pub logical_bounds: LogicalRect,
    /// The size of the display's framebuffer in physical pixels, which is also the
    /// size of a native-resolution capture of the display.
    ///
    /// Backends report displays whose pixel size equals
    /// `self.pixel_grid().pixel_size()`, i.e. the logical size times the scale
    /// factor, so the conversions of [`PixelGrid`] address the framebuffer exactly.
    pub pixel_size: PhysicalSize,
    /// Physical pixels per logical point on this display.
    pub scale_factor: ScaleFactor,
    /// True for the display that holds the global origin (the menu-bar display on
    /// macOS).
    pub is_primary: bool,
}

impl DisplayInfo {
    /// The display's framebuffer as a pixel grid: display-local physical pixels with
    /// `(0, 0)` at the display's top-left corner, the coordinate space of a capture
    /// of this display.
    #[must_use]
    pub const fn pixel_grid(&self) -> PixelGrid {
        PixelGrid::new(self.logical_bounds, self.scale_factor)
    }

    /// The framebuffer's pixels, `(0, 0, pixel_size)`.
    #[must_use]
    pub const fn pixel_bounds(&self) -> PhysicalRect {
        PhysicalRect {
            origin: PhysicalPoint::new(0, 0),
            size: self.pixel_size,
        }
    }

    /// True if the global logical `point` lies on this display (half-open bounds).
    #[must_use]
    pub fn contains(&self, point: LogicalPoint) -> bool {
        self.logical_bounds.contains(point)
    }
}

/// The largest scale factor among `displays`, or `None` if there are none.
///
/// This is the scale at which content combined from those displays is rendered
/// (see the [module docs](self#mixed-scale-factors)).
#[must_use]
pub fn max_scale_factor<'a>(
    displays: impl IntoIterator<Item = &'a DisplayInfo>,
) -> Option<ScaleFactor> {
    displays
        .into_iter()
        .map(|display| display.scale_factor)
        .reduce(|a, b| if b > a { b } else { a })
}

/// A logical region of the global desktop space mapped onto a pixel grid.
///
/// Physical coordinates are relative to the region: pixel `(0, 0)` is the region's
/// top-left corner and `physical = (logical - origin) × scale`. Points round to
/// the nearest pixel corner and rectangle edges round independently (as in
/// [`crate::geometry`]), so rectangles that touch in logical space touch in the
/// grid too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelGrid {
    logical_bounds: LogicalRect,
    scale: ScaleFactor,
}

impl PixelGrid {
    #[must_use]
    pub const fn new(logical_bounds: LogicalRect, scale: ScaleFactor) -> Self {
        Self {
            logical_bounds,
            scale,
        }
    }

    /// The logical region the grid covers, in global logical coordinates.
    #[must_use]
    pub const fn logical_bounds(&self) -> LogicalRect {
        self.logical_bounds
    }

    /// Physical pixels per logical point.
    #[must_use]
    pub const fn scale(&self) -> ScaleFactor {
        self.scale
    }

    /// The grid's size in pixels: the logical size times the scale, rounded to the
    /// nearest pixel.
    #[must_use]
    pub fn pixel_size(&self) -> PhysicalSize {
        self.logical_bounds.size.to_physical(self.scale)
    }

    /// Every pixel of the grid, `(0, 0, pixel_size)`.
    #[must_use]
    pub fn pixel_bounds(&self) -> PhysicalRect {
        PhysicalRect {
            origin: PhysicalPoint::new(0, 0),
            size: self.pixel_size(),
        }
    }

    /// Converts a global logical point to grid pixels, rounding to the nearest
    /// pixel corner. Points outside the region map outside the grid.
    #[must_use]
    pub fn to_physical(&self, point: LogicalPoint) -> PhysicalPoint {
        self.local(point).to_physical(self.scale)
    }

    /// Converts grid pixels to a global logical point.
    #[must_use]
    pub fn to_logical(&self, point: PhysicalPoint) -> LogicalPoint {
        self.global(point.to_logical(self.scale))
    }

    /// Converts a global logical rectangle to grid pixels. The result is not
    /// clipped to the grid.
    #[must_use]
    pub fn rect_to_physical(&self, rect: &LogicalRect) -> PhysicalRect {
        LogicalRect {
            origin: self.local(rect.origin),
            size: rect.size,
        }
        .to_physical(self.scale)
    }

    /// Converts a rectangle of grid pixels to a global logical rectangle.
    #[must_use]
    pub fn rect_to_logical(&self, rect: &PhysicalRect) -> LogicalRect {
        let local = rect.to_logical(self.scale);
        LogicalRect {
            origin: self.global(local.origin),
            size: local.size,
        }
    }

    fn local(&self, point: LogicalPoint) -> LogicalPoint {
        let origin = self.logical_bounds.origin;
        LogicalPoint::new(point.x - origin.x, point.y - origin.y)
    }

    fn global(&self, point: LogicalPoint) -> LogicalPoint {
        let origin = self.logical_bounds.origin;
        LogicalPoint::new(point.x + origin.x, point.y + origin.y)
    }
}

/// Why a set of displays does not form a valid [`DisplayLayout`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    #[error("no displays are connected")]
    NoDisplays,
    #[error("expected exactly one primary display, found {0}")]
    PrimaryCount(usize),
    #[error("display id {0:?} is reported more than once")]
    DuplicateId(DisplayId),
}

impl From<LayoutError> for crate::Error {
    fn from(error: LayoutError) -> Self {
        Self::Platform(format!("invalid display layout: {error}"))
    }
}

/// A validated set of connected displays in the global logical desktop space.
///
/// Displays keep the order the backend reported them in; where displays overlap
/// (which backends normally avoid) lookups prefer the earlier one.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayLayout {
    displays: Vec<DisplayInfo>,
    primary: usize,
    bounds: LogicalRect,
    max_scale: ScaleFactor,
}

/// The part of a logical rectangle that lies on one display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayIntersection<'a> {
    pub display: &'a DisplayInfo,
    /// The overlap in global logical coordinates; never empty.
    pub logical: LogicalRect,
    /// The overlap in the display's framebuffer pixels (see
    /// [`DisplayInfo::pixel_grid`]), clipped to [`DisplayInfo::pixel_bounds`].
    /// Empty when the overlap is a sliver thinner than half a pixel.
    pub physical: PhysicalRect,
}

impl DisplayLayout {
    /// Validates `displays`: there must be at least one, exactly one primary, and
    /// no repeated ids.
    pub fn new(displays: Vec<DisplayInfo>) -> Result<Self, LayoutError> {
        if displays.is_empty() {
            return Err(LayoutError::NoDisplays);
        }
        for (index, display) in displays.iter().enumerate() {
            if displays[..index].iter().any(|other| other.id == display.id) {
                return Err(LayoutError::DuplicateId(display.id));
            }
        }
        let primary = match displays.iter().filter(|d| d.is_primary).count() {
            1 => displays
                .iter()
                .position(|d| d.is_primary)
                .unwrap_or_default(),
            count => return Err(LayoutError::PrimaryCount(count)),
        };
        let bounds = displays
            .iter()
            .map(|display| display.logical_bounds)
            .reduce(|a, b| union(&a, &b))
            .unwrap_or_default();
        let max_scale = max_scale_factor(&displays).unwrap_or_default();
        Ok(Self {
            displays,
            primary,
            bounds,
            max_scale,
        })
    }

    /// Every display, in the order the backend reported them.
    #[must_use]
    pub fn displays(&self) -> &[DisplayInfo] {
        &self.displays
    }

    /// The display holding the global origin.
    #[must_use]
    pub fn primary(&self) -> &DisplayInfo {
        &self.displays[self.primary]
    }

    /// The display with the given id.
    #[must_use]
    pub fn get(&self, id: DisplayId) -> Option<&DisplayInfo> {
        self.displays.iter().find(|display| display.id == id)
    }

    /// The smallest logical rectangle containing every display. It includes any
    /// gaps between displays, which lie on no display.
    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    /// The largest scale factor of any display.
    #[must_use]
    pub const fn max_scale(&self) -> ScaleFactor {
        self.max_scale
    }

    /// The pixel grid of a composite of the whole desktop: [`Self::bounds`] at
    /// [`Self::max_scale`].
    #[must_use]
    pub const fn desktop_grid(&self) -> PixelGrid {
        PixelGrid::new(self.bounds, self.max_scale)
    }

    /// The pixel grid of an image of `rect`: `rect` at the largest scale factor of
    /// the displays it intersects. `None` if it lies on no display (or is empty).
    #[must_use]
    pub fn capture_grid(&self, rect: &LogicalRect) -> Option<PixelGrid> {
        let touched = self
            .displays
            .iter()
            .filter(|display| display.logical_bounds.intersection(rect).is_some());
        Some(PixelGrid::new(*rect, max_scale_factor(touched)?))
    }

    /// The display containing `point`, or `None` if the point lies in a gap between
    /// displays or outside the desktop. Display bounds are half-open, so a point on
    /// the shared edge of two side-by-side displays belongs to the right/lower one.
    #[must_use]
    pub fn display_at(&self, point: LogicalPoint) -> Option<&DisplayInfo> {
        self.displays.iter().find(|display| display.contains(point))
    }

    /// The display containing `point`, or else the display closest to it (by
    /// Euclidean distance to its bounds). Useful for placing UI for a point in a
    /// gap between displays.
    #[must_use]
    pub fn nearest_display(&self, point: LogicalPoint) -> &DisplayInfo {
        self.display_at(point).unwrap_or_else(|| {
            self.displays
                .iter()
                .map(|display| (distance_squared(&display.logical_bounds, point), display))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(self.primary(), |(_, display)| display)
        })
    }

    /// The parts of `rect` that lie on each display, in display order. Displays the
    /// rectangle only touches along an edge are left out; the result is empty when
    /// `rect` lies entirely in gaps or outside the desktop.
    #[must_use]
    pub fn intersections(&self, rect: &LogicalRect) -> Vec<DisplayIntersection<'_>> {
        self.displays
            .iter()
            .filter_map(|display| {
                let logical = display.logical_bounds.intersection(rect)?;
                let physical = display
                    .pixel_grid()
                    .rect_to_physical(&logical)
                    .intersection(&display.pixel_bounds())
                    .unwrap_or_default();
                Some(DisplayIntersection {
                    display,
                    logical,
                    physical,
                })
            })
            .collect()
    }
}

/// The smallest rectangle containing both `a` and `b`.
fn union(a: &LogicalRect, b: &LogicalRect) -> LogicalRect {
    let x0 = a.min_x().min(b.min_x());
    let y0 = a.min_y().min(b.min_y());
    let x1 = a.max_x().max(b.max_x());
    let y1 = a.max_y().max(b.max_y());
    LogicalRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The squared distance from `point` to the nearest point of `rect` (0 inside).
fn distance_squared(rect: &LogicalRect, point: LogicalPoint) -> f64 {
    let dx = (rect.min_x() - point.x)
        .max(point.x - rect.max_x())
        .max(0.0);
    let dy = (rect.min_y() - point.y)
        .max(point.y - rect.max_y())
        .max(0.0);
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(factor: f64) -> ScaleFactor {
        ScaleFactor::new(factor).unwrap()
    }

    fn display(id: u64, bounds: LogicalRect, factor: f64, is_primary: bool) -> DisplayInfo {
        DisplayInfo {
            id: DisplayId(id),
            name: format!("Display {id}"),
            logical_bounds: bounds,
            pixel_size: bounds.size.to_physical(scale(factor)),
            scale_factor: scale(factor),
            is_primary,
        }
    }

    /// A 2× primary at the origin, a 1× display up and to the left (negative
    /// origin, touching the primary's left edge), and a 1.5× portrait display to
    /// the right, starting 100 points lower (leaving a gap above it).
    fn mixed() -> DisplayLayout {
        DisplayLayout::new(vec![
            display(1, LogicalRect::new(0.0, 0.0, 1512.0, 982.0), 2.0, true),
            display(
                2,
                LogicalRect::new(-1920.0, -400.0, 1920.0, 1080.0),
                1.0,
                false,
            ),
            display(
                3,
                LogicalRect::new(1512.0, 100.0, 800.0, 1280.0),
                1.5,
                false,
            ),
        ])
        .unwrap()
    }

    fn id_at(layout: &DisplayLayout, x: f64, y: f64) -> Option<u64> {
        layout.display_at(LogicalPoint::new(x, y)).map(|d| d.id.0)
    }

    #[test]
    fn display_grid_uses_display_local_pixels() {
        let layout = mixed();
        let external = layout.get(DisplayId(2)).unwrap().pixel_grid();
        assert_eq!(
            external.to_physical(LogicalPoint::new(-1920.0, -400.0)),
            PhysicalPoint::new(0, 0)
        );
        assert_eq!(
            external.to_physical(LogicalPoint::new(-1.0, 0.0)),
            PhysicalPoint::new(1919, 400)
        );
        let portrait = layout.get(DisplayId(3)).unwrap().pixel_grid();
        assert_eq!(
            portrait.to_physical(LogicalPoint::new(1612.0, 200.0)),
            PhysicalPoint::new(150, 150)
        );
        assert_eq!(
            portrait.to_logical(PhysicalPoint::new(150, 150)),
            LogicalPoint::new(1612.0, 200.0)
        );
    }

    #[test]
    fn points_round_trip_through_every_display_grid() {
        let layout = mixed();
        for display in layout.displays() {
            let grid = display.pixel_grid();
            for pixel in [
                PhysicalPoint::new(0, 0),
                PhysicalPoint::new(3, 7),
                PhysicalPoint::new(-6, 9),
                PhysicalPoint::new(1199, 1919),
            ] {
                assert_eq!(
                    grid.to_physical(grid.to_logical(pixel)),
                    pixel,
                    "{}",
                    display.name
                );
            }
        }
    }

    #[test]
    fn rects_round_trip_when_edges_fall_on_pixels() {
        let grid = PixelGrid::new(LogicalRect::new(-1512.0, -100.0, 800.0, 1280.0), scale(1.5));
        let rect = LogicalRect::new(-1500.0, -80.0, 100.0, 50.0);
        let physical = grid.rect_to_physical(&rect);
        assert_eq!(physical, PhysicalRect::new(18, 30, 150, 75));
        assert_eq!(grid.rect_to_logical(&physical), rect);
    }

    #[test]
    fn display_grid_covers_the_framebuffer_exactly() {
        for display in mixed().displays() {
            let grid = display.pixel_grid();
            assert_eq!(grid.pixel_size(), display.pixel_size);
            assert_eq!(
                grid.rect_to_physical(&display.logical_bounds),
                display.pixel_bounds()
            );
        }
    }

    #[test]
    fn adjacent_rects_stay_adjacent_in_a_fractional_grid() {
        let grid = PixelGrid::new(LogicalRect::new(1512.0, 100.0, 800.0, 1280.0), scale(1.5));
        let left = grid.rect_to_physical(&LogicalRect::new(1512.3, 100.0, 10.3, 5.0));
        let right = grid.rect_to_physical(&LogicalRect::new(1522.6, 100.0, 10.3, 5.0));
        assert_eq!(left.max_x(), i64::from(right.min_x()));
    }

    #[test]
    fn bounds_span_negative_origins_and_gaps() {
        let layout = mixed();
        assert_eq!(
            layout.bounds(),
            LogicalRect::new(-1920.0, -400.0, 4232.0, 1780.0)
        );
        assert_eq!(layout.primary().id, DisplayId(1));
    }

    #[test]
    fn desktop_grid_renders_at_the_maximum_scale() {
        let layout = mixed();
        assert_eq!(layout.max_scale(), scale(2.0));
        let grid = layout.desktop_grid();
        assert_eq!(grid.pixel_size(), PhysicalSize::new(8464, 3560));
        // The primary's top-left corner, 1920 × 400 points into the desktop.
        assert_eq!(
            grid.to_physical(LogicalPoint::new(0.0, 0.0)),
            PhysicalPoint::new(3840, 800)
        );
        // Each display lands where its logical bounds say, at the desktop scale.
        let portrait = layout.get(DisplayId(3)).unwrap();
        assert_eq!(
            grid.rect_to_physical(&portrait.logical_bounds),
            PhysicalRect::new(6864, 1000, 1600, 2560)
        );
    }

    #[test]
    fn max_scale_factor_of_a_subset() {
        let layout = mixed();
        let [primary, external, portrait] = layout.displays() else {
            panic!()
        };
        assert_eq!(max_scale_factor([external, portrait]), Some(scale(1.5)));
        assert_eq!(
            max_scale_factor([portrait, primary, external]),
            Some(scale(2.0))
        );
        assert_eq!(max_scale_factor([]), None);
    }

    #[test]
    fn display_at_respects_half_open_edges() {
        let layout = mixed();
        assert_eq!(id_at(&layout, 0.0, 0.0), Some(1));
        assert_eq!(id_at(&layout, -0.001, 0.0), Some(2));
        assert_eq!(id_at(&layout, -1920.0, -400.0), Some(2));
        assert_eq!(id_at(&layout, 1511.999, 500.0), Some(1));
        assert_eq!(id_at(&layout, 1512.0, 500.0), Some(3));
        assert_eq!(id_at(&layout, 0.0, 982.0), None);
        assert_eq!(id_at(&layout, 2312.0, 500.0), None);
    }

    #[test]
    fn points_in_gaps_have_no_display_but_a_nearest_one() {
        let layout = mixed();
        // Above the portrait display, right of the primary.
        let gap = LogicalPoint::new(1600.0, 50.0);
        assert!(layout.display_at(gap).is_none());
        assert_eq!(layout.nearest_display(gap).id, DisplayId(3));
        // Below the external display, left of the primary's bottom edge: 20 points
        // from the external display, 10 from the primary.
        let gap = LogicalPoint::new(-10.0, 700.0);
        assert!(layout.display_at(gap).is_none());
        assert_eq!(layout.nearest_display(gap).id, DisplayId(1));
        let gap = LogicalPoint::new(-30.0, 690.0);
        assert_eq!(layout.nearest_display(gap).id, DisplayId(2));
        // Far outside the desktop.
        assert_eq!(
            layout
                .nearest_display(LogicalPoint::new(-5000.0, -5000.0))
                .id,
            DisplayId(2)
        );
        assert_eq!(
            layout.nearest_display(LogicalPoint::new(100.0, 100.0)).id,
            DisplayId(1)
        );
    }

    #[test]
    fn rect_spanning_three_displays_splits_per_display() {
        let layout = mixed();
        let parts = layout.intersections(&LogicalRect::new(-100.0, 50.0, 1700.0, 200.0));
        let summary: Vec<_> = parts
            .iter()
            .map(|p| (p.display.id.0, p.logical, p.physical))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    1,
                    LogicalRect::new(0.0, 50.0, 1512.0, 200.0),
                    PhysicalRect::new(0, 100, 3024, 400)
                ),
                (
                    2,
                    LogicalRect::new(-100.0, 50.0, 100.0, 200.0),
                    PhysicalRect::new(1820, 450, 100, 200)
                ),
                (
                    3,
                    LogicalRect::new(1512.0, 100.0, 88.0, 150.0),
                    PhysicalRect::new(0, 0, 132, 225)
                ),
            ]
        );
        assert_eq!(
            max_scale_factor(parts.iter().map(|p| p.display)),
            Some(scale(2.0))
        );
    }

    #[test]
    fn rect_spanning_two_displays_and_a_gap() {
        let layout = mixed();
        // From the primary's lower right into the portrait display, crossing the
        // gap above the portrait display.
        let parts = layout.intersections(&LogicalRect::new(1500.0, 0.0, 112.0, 200.0));
        let ids: Vec<_> = parts.iter().map(|p| p.display.id.0).collect();
        assert_eq!(ids, [1, 3]);
        assert_eq!(parts[0].physical, PhysicalRect::new(3000, 0, 24, 400));
        assert_eq!(
            parts[1].logical,
            LogicalRect::new(1512.0, 100.0, 100.0, 100.0)
        );
        assert_eq!(parts[1].physical, PhysicalRect::new(0, 0, 150, 150));
    }

    #[test]
    fn rects_touching_only_an_edge_or_in_a_gap_hit_nothing_there() {
        let layout = mixed();
        // Touches the primary's right edge; lies in the gap above the portrait.
        assert!(layout
            .intersections(&LogicalRect::new(1512.0, 0.0, 50.0, 100.0))
            .is_empty());
        // Touches the primary's bottom edge from below.
        let parts = layout.intersections(&LogicalRect::new(100.0, 982.0, 50.0, 50.0));
        assert!(parts.is_empty());
        assert!(layout
            .intersections(&LogicalRect::new(0.0, 0.0, 0.0, 10.0))
            .is_empty());
    }

    #[test]
    fn capture_grid_uses_the_max_scale_of_the_displays_it_touches() {
        let layout = mixed();
        let on_external = LogicalRect::new(-500.0, 0.0, 100.0, 50.0);
        let grid = layout.capture_grid(&on_external).unwrap();
        assert_eq!(
            (grid.scale(), grid.pixel_size()),
            (scale(1.0), PhysicalSize::new(100, 50))
        );

        let spanning = LogicalRect::new(-50.0, 0.0, 100.25, 50.0);
        let grid = layout.capture_grid(&spanning).unwrap();
        assert_eq!(
            (grid.scale(), grid.pixel_size()),
            (scale(2.0), PhysicalSize::new(201, 100))
        );
        // The part on the 1× display fills the left 100 pixels of the image.
        assert_eq!(
            grid.rect_to_physical(&layout.intersections(&spanning)[1].logical),
            PhysicalRect::new(0, 0, 100, 100)
        );

        assert!(layout
            .capture_grid(&LogicalRect::new(1600.0, 0.0, 10.0, 10.0))
            .is_none());
    }

    #[test]
    fn sliver_intersections_have_empty_physical_rects() {
        let layout = mixed();
        let parts = layout.intersections(&LogicalRect::new(-0.2, 10.0, 10.0, 10.0));
        assert_eq!(parts.len(), 2);
        let external = parts.iter().find(|p| p.display.id == DisplayId(2)).unwrap();
        assert!(!external.logical.is_empty());
        assert!(external.physical.is_empty());
    }

    #[test]
    fn layout_validation() {
        let primary = || display(1, LogicalRect::new(0.0, 0.0, 100.0, 100.0), 1.0, true);
        let other = |id, is_primary| {
            display(
                id,
                LogicalRect::new(100.0, 0.0, 100.0, 100.0),
                1.0,
                is_primary,
            )
        };
        assert_eq!(DisplayLayout::new(vec![]), Err(LayoutError::NoDisplays));
        assert_eq!(
            DisplayLayout::new(vec![other(2, false)]),
            Err(LayoutError::PrimaryCount(0))
        );
        assert_eq!(
            DisplayLayout::new(vec![primary(), other(2, true)]),
            Err(LayoutError::PrimaryCount(2))
        );
        assert_eq!(
            DisplayLayout::new(vec![primary(), other(1, false)]),
            Err(LayoutError::DuplicateId(DisplayId(1)))
        );
        let layout = DisplayLayout::new(vec![other(2, false), primary()]).unwrap();
        assert_eq!(layout.primary().id, DisplayId(1));
    }
}

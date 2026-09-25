//! Logical and physical geometry.
//!
//! Chartreuse uses two coordinate spaces:
//!
//! - **Logical** coordinates are device-independent points (`f64`), the unit that
//!   window placement and UI layout use. The *global logical desktop space* has its
//!   origin at the top-left corner of the primary display, x grows to the right and
//!   y grows **downwards** (backends flip platform conventions such as Cocoa's
//!   bottom-left origin before handing values out). Coordinates may be negative for
//!   displays above or to the left of the primary display.
//! - **Physical** coordinates are integer device pixels (`i32` positions, `u32`
//!   extents), the unit that captured [`Image`](crate::image::Image)s use.
//!
//! A [`ScaleFactor`] converts between the two: `physical = logical × scale`. The
//! per-display and whole-desktop mappings are built on these primitives in
//! [`crate::display`].

/// The ratio of physical pixels to logical points. Always finite and positive.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ScaleFactor(f64);

impl ScaleFactor {
    /// One physical pixel per logical point.
    pub const ONE: Self = Self(1.0);

    /// Returns `None` unless `factor` is finite and greater than zero.
    #[must_use]
    pub fn new(factor: f64) -> Option<Self> {
        (factor.is_finite() && factor > 0.0).then_some(Self(factor))
    }

    /// The raw ratio.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Default for ScaleFactor {
    fn default() -> Self {
        Self::ONE
    }
}

/// Rounds to the nearest integer pixel coordinate, saturating at the `i32` range.
fn round_to_i32(value: f64) -> i32 {
    // `as` saturates for out-of-range floats and maps NaN to 0.
    value.round() as i32
}

/// Rounds to the nearest integer pixel extent, saturating at the `u32` range.
fn round_to_u32(value: f64) -> u32 {
    value.round() as u32
}

/// A point in logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

impl LogicalPoint {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Scales to physical pixels, rounding to the nearest pixel.
    #[must_use]
    pub fn to_physical(self, scale: ScaleFactor) -> PhysicalPoint {
        PhysicalPoint::new(
            round_to_i32(self.x * scale.get()),
            round_to_i32(self.y * scale.get()),
        )
    }
}

/// A size in logical coordinates. Components are expected to be non-negative.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

impl LogicalSize {
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// Scales to physical pixels, rounding to the nearest pixel.
    #[must_use]
    pub fn to_physical(self, scale: ScaleFactor) -> PhysicalSize {
        PhysicalSize::new(
            round_to_u32(self.width * scale.get()),
            round_to_u32(self.height * scale.get()),
        )
    }
}

/// An axis-aligned rectangle in logical coordinates, `origin` being the top-left
/// corner. The rectangle is half-open: it contains its left and top edges but not
/// its right and bottom edges.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogicalRect {
    pub origin: LogicalPoint,
    pub size: LogicalSize,
}

impl LogicalRect {
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: LogicalPoint::new(x, y),
            size: LogicalSize::new(width, height),
        }
    }

    /// The rectangle spanned by two opposite corners, in any order (for example the
    /// press and release points of a drag).
    #[must_use]
    pub fn from_corners(a: LogicalPoint, b: LogicalPoint) -> Self {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    #[must_use]
    pub fn min_x(&self) -> f64 {
        self.origin.x
    }

    #[must_use]
    pub fn min_y(&self) -> f64 {
        self.origin.y
    }

    #[must_use]
    pub fn max_x(&self) -> f64 {
        self.origin.x + self.size.width
    }

    #[must_use]
    pub fn max_y(&self) -> f64 {
        self.origin.y + self.size.height
    }

    /// True if the rectangle covers no area.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.size.width > 0.0 && self.size.height > 0.0)
    }

    /// True if `point` lies inside the half-open rectangle.
    #[must_use]
    pub fn contains(&self, point: LogicalPoint) -> bool {
        point.x >= self.min_x()
            && point.x < self.max_x()
            && point.y >= self.min_y()
            && point.y < self.max_y()
    }

    /// The overlapping area of two rectangles, or `None` if they do not overlap.
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let x0 = self.min_x().max(other.min_x());
        let y0 = self.min_y().max(other.min_y());
        let x1 = self.max_x().min(other.max_x());
        let y1 = self.max_y().min(other.max_y());
        let rect = Self::new(x0, y0, x1 - x0, y1 - y0);
        (!rect.is_empty()).then_some(rect)
    }

    /// Scales to physical pixels. The edges are rounded independently, so
    /// rectangles that touch in logical space also touch in physical space.
    #[must_use]
    pub fn to_physical(&self, scale: ScaleFactor) -> PhysicalRect {
        let s = scale.get();
        let x0 = round_to_i32(self.min_x() * s);
        let y0 = round_to_i32(self.min_y() * s);
        let x1 = round_to_i32(self.max_x() * s);
        let y1 = round_to_i32(self.max_y() * s);
        PhysicalRect::from_edges(x0, y0, x1, y1)
    }
}

/// A point in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

impl PhysicalPoint {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Scales to logical points.
    #[must_use]
    pub fn to_logical(self, scale: ScaleFactor) -> LogicalPoint {
        LogicalPoint::new(
            f64::from(self.x) / scale.get(),
            f64::from(self.y) / scale.get(),
        )
    }
}

/// A size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// True if the size covers no pixels.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Scales to logical points.
    #[must_use]
    pub fn to_logical(self, scale: ScaleFactor) -> LogicalSize {
        LogicalSize::new(
            f64::from(self.width) / scale.get(),
            f64::from(self.height) / scale.get(),
        )
    }
}

/// An axis-aligned, half-open rectangle in physical pixels, `origin` being the
/// top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhysicalRect {
    pub origin: PhysicalPoint,
    pub size: PhysicalSize,
}

impl PhysicalRect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            origin: PhysicalPoint::new(x, y),
            size: PhysicalSize::new(width, height),
        }
    }

    /// The rectangle between the given edges; inverted edges yield an empty rect.
    fn from_edges(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let width = u32::try_from(i64::from(x1) - i64::from(x0)).unwrap_or(0);
        let height = u32::try_from(i64::from(y1) - i64::from(y0)).unwrap_or(0);
        Self::new(x0, y0, width, height)
    }

    #[must_use]
    pub const fn min_x(&self) -> i32 {
        self.origin.x
    }

    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.origin.y
    }

    /// The exclusive right edge. Computed in `i64` so it cannot overflow.
    #[must_use]
    pub const fn max_x(&self) -> i64 {
        self.origin.x as i64 + self.size.width as i64
    }

    /// The exclusive bottom edge. Computed in `i64` so it cannot overflow.
    #[must_use]
    pub const fn max_y(&self) -> i64 {
        self.origin.y as i64 + self.size.height as i64
    }

    /// True if the rectangle covers no pixels.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size.is_empty()
    }

    /// True if the pixel at `point` lies inside the rectangle.
    #[must_use]
    pub fn contains(&self, point: PhysicalPoint) -> bool {
        point.x >= self.min_x()
            && i64::from(point.x) < self.max_x()
            && point.y >= self.min_y()
            && i64::from(point.y) < self.max_y()
    }

    /// The overlapping pixels of two rectangles, or `None` if they do not overlap.
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let x0 = self.min_x().max(other.min_x());
        let y0 = self.min_y().max(other.min_y());
        let x1 = self.max_x().min(other.max_x());
        let y1 = self.max_y().min(other.max_y());
        let width = u32::try_from(x1 - i64::from(x0)).ok()?;
        let height = u32::try_from(y1 - i64::from(y0)).ok()?;
        let rect = Self::new(x0, y0, width, height);
        (!rect.is_empty()).then_some(rect)
    }

    /// Scales to logical points.
    #[must_use]
    pub fn to_logical(&self, scale: ScaleFactor) -> LogicalRect {
        LogicalRect {
            origin: self.origin.to_logical(scale),
            size: self.size.to_logical(scale),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(factor: f64) -> ScaleFactor {
        ScaleFactor::new(factor).unwrap()
    }

    #[test]
    fn scale_factor_rejects_non_positive_and_non_finite_values() {
        assert!(ScaleFactor::new(0.0).is_none());
        assert!(ScaleFactor::new(-2.0).is_none());
        assert!(ScaleFactor::new(f64::NAN).is_none());
        assert!(ScaleFactor::new(f64::INFINITY).is_none());
        assert_eq!(ScaleFactor::new(1.5).map(ScaleFactor::get), Some(1.5));
    }

    #[test]
    fn point_round_trips_through_physical_at_integer_scale() {
        let point = LogicalPoint::new(-960.0, 12.5);
        let physical = point.to_physical(scale(2.0));
        assert_eq!(physical, PhysicalPoint::new(-1920, 25));
        assert_eq!(physical.to_logical(scale(2.0)), point);
    }

    #[test]
    fn from_corners_normalizes_any_drag_direction() {
        let rect =
            LogicalRect::from_corners(LogicalPoint::new(30.0, -5.0), LogicalPoint::new(10.0, 15.0));
        assert_eq!(rect, LogicalRect::new(10.0, -5.0, 20.0, 20.0));
    }

    #[test]
    fn logical_rect_is_half_open() {
        let rect = LogicalRect::new(-10.0, -10.0, 20.0, 20.0);
        assert!(rect.contains(LogicalPoint::new(-10.0, -10.0)));
        assert!(rect.contains(LogicalPoint::new(9.999, 9.999)));
        assert!(!rect.contains(LogicalPoint::new(10.0, 0.0)));
        assert!(!rect.contains(LogicalPoint::new(0.0, 10.0)));
    }

    #[test]
    fn touching_rects_do_not_intersect() {
        let left = LogicalRect::new(0.0, 0.0, 10.0, 10.0);
        let right = LogicalRect::new(10.0, 0.0, 10.0, 10.0);
        assert_eq!(left.intersection(&right), None);
        let overlapping = LogicalRect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(
            left.intersection(&overlapping),
            Some(LogicalRect::new(5.0, 5.0, 5.0, 5.0))
        );
    }

    #[test]
    fn adjacent_logical_rects_stay_adjacent_at_fractional_scale() {
        let s = scale(1.5);
        let left = LogicalRect::new(0.0, 0.0, 10.5, 3.0).to_physical(s);
        let right = LogicalRect::new(10.5, 0.0, 10.5, 3.0).to_physical(s);
        assert_eq!(left.max_x(), i64::from(right.min_x()));
        assert_eq!(i64::from(left.size.width) + i64::from(right.size.width), 32);
    }

    #[test]
    fn physical_rect_edges_do_not_overflow() {
        let rect = PhysicalRect::new(i32::MAX, i32::MAX, u32::MAX, 1);
        assert_eq!(rect.max_x(), i64::from(i32::MAX) + i64::from(u32::MAX));
        assert!(rect.contains(PhysicalPoint::new(i32::MAX, i32::MAX)));
    }

    #[test]
    fn physical_intersection_handles_negative_origins() {
        let a = PhysicalRect::new(-100, -100, 150, 150);
        let b = PhysicalRect::new(0, 0, 100, 100);
        assert_eq!(a.intersection(&b), Some(PhysicalRect::new(0, 0, 50, 50)));
        assert_eq!(a.intersection(&PhysicalRect::new(50, 0, 10, 10)), None);
    }
}

//! The display model: every display's geometry in logical and physical units.
//!
//! All capture and overlay placement goes through these types so coordinate
//! conversions live in one place.

use crate::geometry::{LogicalRect, PhysicalSize, ScaleFactor};

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
    pub pixel_size: PhysicalSize,
    /// Physical pixels per logical point on this display.
    pub scale_factor: ScaleFactor,
    /// True for the display that holds the global origin (the menu-bar display on
    /// macOS).
    pub is_primary: bool,
}

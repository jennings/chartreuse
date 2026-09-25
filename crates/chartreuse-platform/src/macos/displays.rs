//! macOS: display enumeration through `NSScreen`.
//!
//! AppKit reports screen frames in a global space whose origin is the bottom-left
//! corner of the primary screen (`NSScreen.screens[0]`, the one with the menu bar)
//! with y pointing up. [`flip_frame`] converts them to Chartreuse's global logical
//! space: top-left origin at the primary screen's top-left corner, y pointing down.

use chartreuse_core::display::{DisplayId, DisplayInfo};
use chartreuse_core::geometry::{LogicalRect, ScaleFactor};
use chartreuse_core::{Error, Result};
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::CGRect;
use objc2_foundation::{ns_string, NSNumber};

use crate::displays::Displays;

/// The macOS [`Displays`] backend.
#[derive(Debug, Default)]
pub struct MacosDisplays;

impl MacosDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for MacosDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            Error::Platform("display enumeration must run on the main thread".into())
        })?;
        let screens = NSScreen::screens(mtm);
        let primary_height = screens
            .firstObject()
            .ok_or_else(|| Error::Platform("no displays are connected".into()))?
            .frame()
            .size
            .height;
        screens
            .iter()
            .enumerate()
            .map(|(index, screen)| describe(&screen, index == 0, primary_height))
            .collect()
    }
}

/// Converts one `NSScreen` to a [`DisplayInfo`].
fn describe(screen: &NSScreen, is_primary: bool, primary_height: f64) -> Result<DisplayInfo> {
    let backing = screen.backingScaleFactor();
    let scale_factor = ScaleFactor::new(backing)
        .ok_or_else(|| Error::Platform(format!("invalid backing scale factor {backing}")))?;
    let logical_bounds = flip_frame(screen.frame(), primary_height);
    Ok(DisplayInfo {
        id: DisplayId(u64::from(display_id(screen)?)),
        name: screen.localizedName().to_string(),
        logical_bounds,
        // The backing store is the frame times the backing scale factor, which is
        // also the size ScreenCaptureKit captures at natively. (It matches
        // `CGDisplayModeGetPixelWidth/Height` except that the mode is not rotated
        // for rotated displays.)
        pixel_size: logical_bounds.size.to_physical(scale_factor),
        scale_factor,
        is_primary,
    })
}

/// The screen's `CGDirectDisplayID`, from its device description.
fn display_id(screen: &NSScreen) -> Result<u32> {
    screen
        .deviceDescription()
        .objectForKey(ns_string!("NSScreenNumber"))
        .and_then(|number| number.downcast::<NSNumber>().ok())
        .map(|number| number.unsignedIntValue())
        .ok_or_else(|| Error::Platform("screen has no NSScreenNumber".into()))
}

/// Converts an AppKit screen frame (bottom-left origin at the primary screen's
/// bottom-left corner, y up) to the global logical desktop space (top-left origin
/// at the primary screen's top-left corner, y down). `primary_height` is the height
/// of the primary screen's frame.
fn flip_frame(frame: CGRect, primary_height: f64) -> LogicalRect {
    LogicalRect::new(
        frame.origin.x,
        primary_height - (frame.origin.y + frame.size.height),
        frame.size.width,
        frame.size.height,
    )
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGSize};

    use super::*;

    fn frame(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
    }

    #[test]
    fn primary_screen_maps_to_the_origin() {
        assert_eq!(
            flip_frame(frame(0.0, 0.0, 1512.0, 982.0), 982.0),
            LogicalRect::new(0.0, 0.0, 1512.0, 982.0)
        );
    }

    #[test]
    fn screen_above_the_primary_gets_a_negative_y() {
        // AppKit: bottom edge on the primary's top edge.
        assert_eq!(
            flip_frame(frame(-200.0, 982.0, 1920.0, 1080.0), 982.0),
            LogicalRect::new(-200.0, -1080.0, 1920.0, 1080.0)
        );
    }

    #[test]
    fn screen_below_the_primary_starts_at_its_bottom_edge() {
        // AppKit: top edge on the primary's bottom edge (negative y).
        assert_eq!(
            flip_frame(frame(100.0, -1280.0, 800.0, 1280.0), 982.0),
            LogicalRect::new(100.0, 982.0, 800.0, 1280.0)
        );
    }

    #[test]
    fn screen_to_the_left_keeps_x_and_flips_its_top_edge() {
        // AppKit: bottom edge 400 points below the primary's bottom, so the top edge
        // is at y = 680, which is 982 - 680 = 302 points below the primary's top.
        assert_eq!(
            flip_frame(frame(-1920.0, -400.0, 1920.0, 1080.0), 982.0),
            LogicalRect::new(-1920.0, 302.0, 1920.0, 1080.0)
        );
    }
}

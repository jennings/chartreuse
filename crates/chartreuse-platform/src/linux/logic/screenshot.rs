//! Splitting a whole-desktop screenshot (what the Screenshot portal returns)
//! into per-display captures.
//!
//! The portal saves one image of every output together. GNOME and wlroots
//! (grim) render it over the compositor's logical layout at a single scale
//! (the highest output's), so each display's part is its logical bounds
//! scaled by the image's size over the layout's. That part is resampled to the
//! display's pixel size, which leaves it untouched where the scales agree.

use chartreuse_core::display::{DisplayId, DisplayInfo, DisplayLayout};
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};

/// Each display's part of `screenshot`, a picture of the whole `layout`.
///
/// # Errors
///
/// [`Error::InvalidImage`] if the screenshot is empty.
pub fn split(layout: &DisplayLayout, screenshot: &Image) -> Result<Vec<Image>> {
    if screenshot.size().is_empty() {
        return Err(Error::InvalidImage("the screenshot is empty".into()));
    }
    // The screenshot as one display covering the whole layout, stretched over
    // each real display's pixel grid.
    let desktop = DisplayInfo {
        id: DisplayId(u64::MAX),
        name: "Desktop".into(),
        logical_bounds: layout.bounds(),
        pixel_size: screenshot.size(),
        scale_factor: layout.max_scale(),
        is_primary: false,
    };
    layout
        .displays()
        .iter()
        .map(|display| {
            chartreuse_imaging::composite_at([(&desktop, screenshot)], display.pixel_grid())
                .map(|composite| composite.image)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::{LogicalRect, PhysicalSize, ScaleFactor};

    use super::*;

    fn display(id: u64, bounds: LogicalRect, scale: f64) -> DisplayInfo {
        let scale_factor = ScaleFactor::new(scale).unwrap();
        let mut display = DisplayInfo {
            id: DisplayId(id),
            name: format!("Display {id}"),
            logical_bounds: bounds,
            pixel_size: PhysicalSize::new(0, 0),
            scale_factor,
            is_primary: id == 1,
        };
        display.pixel_size = display.pixel_grid().pixel_size();
        display
    }

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const BLUE: Rgba8 = Rgba8::new(0, 0, 255, 255);

    #[test]
    fn each_display_gets_its_part_at_its_own_pixel_size() {
        // A 2× display left of a 1× one, shot at 2×: 200 + 200 pixels wide.
        let layout = DisplayLayout::new(vec![
            display(1, LogicalRect::new(0.0, 0.0, 100.0, 50.0), 2.0),
            display(2, LogicalRect::new(100.0, 0.0, 100.0, 50.0), 1.0),
        ])
        .unwrap();
        let shot = Image::from_fn(PhysicalSize::new(400, 100), |x, y| {
            if x < 200 {
                Rgba8::new((x % 256) as u8, y as u8, 0, 255)
            } else {
                BLUE
            }
        });
        let parts = split(&layout, &shot).unwrap();
        assert_eq!(parts[0].size(), PhysicalSize::new(200, 100));
        assert_eq!(parts[1].size(), PhysicalSize::new(100, 50));
        // The 2× part is copied pixel for pixel.
        for (x, y) in [(0, 0), (37, 81), (199, 99)] {
            assert_eq!(parts[0].pixel(x, y), shot.pixel(x, y));
        }
        assert!(parts[1]
            .pixels()
            .chunks(4)
            .all(|pixel| pixel == [0, 0, 255, 255]));
    }

    #[test]
    fn displays_above_and_left_of_the_primary_are_found_too() {
        let layout = DisplayLayout::new(vec![
            display(1, LogicalRect::new(0.0, 0.0, 10.0, 10.0), 1.0),
            display(2, LogicalRect::new(-10.0, -10.0, 10.0, 10.0), 1.0),
        ])
        .unwrap();
        // The layout spans (-10, -10) to (10, 10): the secondary is the
        // screenshot's top-left quarter.
        let shot = Image::from_fn(PhysicalSize::new(20, 20), |x, y| {
            if x < 10 && y < 10 {
                RED
            } else {
                BLUE
            }
        });
        let parts = split(&layout, &shot).unwrap();
        assert_eq!(parts[1], Image::filled(PhysicalSize::new(10, 10), RED));
        assert_eq!(parts[0].pixel(0, 0), Some(BLUE));
    }

    #[test]
    fn empty_screenshots_are_rejected() {
        let layout = DisplayLayout::new(vec![display(
            1,
            LogicalRect::new(0.0, 0.0, 10.0, 10.0),
            1.0,
        )])
        .unwrap();
        let empty = Image::new(PhysicalSize::new(0, 0), Vec::new()).unwrap();
        assert!(matches!(
            split(&layout, &empty),
            Err(Error::InvalidImage(_))
        ));
    }
}

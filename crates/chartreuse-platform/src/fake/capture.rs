//! Fake [`Capture`]: generated test-pattern images.

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::geometry::{LogicalPoint, ScaleFactor};
use chartreuse_core::image::Image;
use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::window::WindowId;
use chartreuse_core::{Error, Result};
use futures::future::{self, BoxFuture, FutureExt};

use super::{test_pattern, Fake};
use crate::capture::{Capture, DisplayCapture};

/// The blue level that identifies display number `index` in its test pattern.
fn display_tint(index: usize) -> u8 {
    u8::try_from(index * 80 % 256).unwrap_or(0)
}

/// The scale factor of the display under `point`, else of the primary display.
fn scale_at(displays: &[DisplayInfo], point: LogicalPoint) -> ScaleFactor {
    displays
        .iter()
        .find(|display| display.logical_bounds.contains(point))
        .or_else(|| displays.iter().find(|display| display.is_primary))
        .map_or(ScaleFactor::ONE, |display| display.scale_factor)
}

impl Fake {
    fn check_screen_recording(&self) -> Result<()> {
        match self.state.lock().screen_recording {
            PermissionStatus::Granted => Ok(()),
            PermissionStatus::Denied => Err(Error::PermissionDenied(Permission::ScreenRecording)),
        }
    }
}

impl Capture for Fake {
    fn capture_displays(&self) -> BoxFuture<'static, Result<Vec<DisplayCapture>>> {
        if let Err(error) = self.check_screen_recording() {
            return future::ready(Err(error)).boxed();
        }
        let displays = self.state.lock().displays.clone();
        // Generate the pixels on the executor, not on the calling (main) thread.
        future::lazy(move |_| {
            Ok(displays
                .into_iter()
                .enumerate()
                .map(|(index, display)| DisplayCapture {
                    image: test_pattern(
                        display.pixel_size,
                        display.scale_factor,
                        display_tint(index),
                    ),
                    display,
                })
                .collect())
        })
        .boxed()
    }

    fn capture_window(&self, window: WindowId) -> BoxFuture<'static, Result<Image>> {
        if let Err(error) = self.check_screen_recording() {
            return future::ready(Err(error)).boxed();
        }
        let state = self.state.lock();
        let Some(info) = state.windows.iter().find(|info| info.id == window) else {
            return future::ready(Err(Error::Platform(format!(
                "no window with id {}",
                window.0
            ))))
            .boxed();
        };
        let bounds = info.bounds;
        let center = LogicalPoint::new(
            bounds.min_x() + bounds.size.width / 2.0,
            bounds.min_y() + bounds.size.height / 2.0,
        );
        let scale = scale_at(&state.displays, center);
        let tint = u8::try_from(window.0 % 256).unwrap_or(0);
        future::lazy(move |_| Ok(test_pattern(bounds.size.to_physical(scale), scale, tint))).boxed()
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn display_captures_are_native_resolution_with_a_scaled_grid() {
        let captures = block_on(Fake::new().capture_displays()).unwrap();
        assert_eq!(captures.len(), 3);
        for capture in &captures {
            assert_eq!(capture.image.size(), capture.display.pixel_size);
            // Grid lines every 100 logical points land on physical pixels.
            let grid = (100.0 * capture.display.scale_factor.get()) as u32;
            assert_eq!(capture.image.pixel(grid, 1), Some(Rgba8::WHITE));
            assert_ne!(capture.image.pixel(grid + 1, 1), Some(Rgba8::WHITE));
        }
        let tints: Vec<u8> = captures
            .iter()
            .map(|c| c.image.pixel(1, 1).unwrap().b)
            .collect();
        assert!(
            tints.windows(2).all(|pair| pair[0] != pair[1]),
            "displays are distinguishable"
        );
    }

    #[test]
    fn window_captures_use_the_scale_of_the_display_under_the_window() {
        let fake = Fake::new();
        // "Fake Editor" sits on the 1.5× display.
        let image = block_on(fake.capture_window(WindowId(103))).unwrap();
        assert_eq!(image.size(), PhysicalSize::new(900, 1350));
        // "Fake Browser" spans the external and primary displays; its center is on
        // the 2× primary display.
        let image = block_on(fake.capture_window(WindowId(102))).unwrap();
        assert_eq!(image.size(), PhysicalSize::new(2400, 1400));
    }

    #[test]
    fn unknown_windows_fail() {
        assert!(matches!(
            block_on(Fake::new().capture_window(WindowId(9))),
            Err(Error::Platform(_))
        ));
    }

    #[test]
    fn capture_fails_without_screen_recording_permission() {
        let fake = Fake::new();
        fake.set_screen_recording(PermissionStatus::Denied);
        assert!(matches!(
            block_on(fake.capture_displays()),
            Err(Error::PermissionDenied(Permission::ScreenRecording))
        ));
        assert!(matches!(
            block_on(fake.capture_window(WindowId(101))),
            Err(Error::PermissionDenied(Permission::ScreenRecording))
        ));
    }
}

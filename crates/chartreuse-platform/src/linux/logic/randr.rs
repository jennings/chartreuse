//! The X11 desktop as a display model: RandR monitors and the scale factor
//! winit renders Chartreuse's windows at.
//!
//! X11 has one pixel space, the root window, and no per-monitor scaling:
//! desktops scale everything by `Xft.dpi` / 96. So the global logical space
//! is the root window's pixels divided by one scale factor, the same one
//! winit (which iced runs on) applies, so that overlay windows iced places at
//! a display's logical origin land exactly on it. Its origin is the root
//! window's, which is not necessarily the primary monitor's top-left corner.

use chartreuse_core::display::{DisplayId, DisplayInfo};
use chartreuse_core::geometry::{PhysicalRect, ScaleFactor};

/// One RandR monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The first output's XID, stable while the monitor stays connected.
    pub id: u64,
    /// The output name, such as `DP-1`.
    pub name: String,
    /// The monitor's area of the root window.
    pub bounds: PhysicalRect,
    /// Physical width and height in millimetres; 0 when unknown.
    pub millimeters: (u32, u32),
    pub primary: bool,
}

/// Where winit takes its X11 scale factor from, in the order it looks.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScaleSources<'a> {
    /// `WINIT_X11_SCALE_FACTOR`: a factor, or `randr` to derive it from each
    /// monitor's size.
    pub env_override: Option<&'a str>,
    /// `Xft/DPI` from XSETTINGS.
    pub xsettings_dpi: Option<f64>,
    /// `Xft.dpi` from the X resource database.
    pub resource_dpi: Option<f64>,
}

/// The scale factor of the whole desktop, chosen the way winit chooses each
/// window's: the `WINIT_X11_SCALE_FACTOR` override, else the DPI setting / 96,
/// else a factor derived from the monitor's physical size.
///
/// winit derives size-based factors per monitor; a single logical space cannot
/// follow different ones, so the primary monitor's applies to all.
#[must_use]
pub fn desktop_scale(sources: ScaleSources<'_>, monitors: &[Monitor]) -> ScaleFactor {
    let by_size = || {
        primary(monitors)
            .map(|index| size_scale(&monitors[index]))
            .unwrap_or(1.0)
    };
    let override_factor = match sources.env_override.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("randr") => Some(by_size()),
        Some(value) => value.parse().ok(),
        None => None,
    };
    let factor = override_factor
        .or_else(|| sources.xsettings_dpi.map(|dpi| dpi / 96.0))
        .or_else(|| sources.resource_dpi.map(|dpi| dpi / 96.0))
        .unwrap_or_else(by_size);
    ScaleFactor::new(factor).unwrap_or(ScaleFactor::ONE)
}

/// winit's factor for a monitor from its pixel density: the pixels per
/// millimetre relative to 96 DPI, in steps of 1/12, at least 1; 1 when the
/// size is unknown or absurd.
#[must_use]
pub fn size_scale(monitor: &Monitor) -> f64 {
    let (width_mm, height_mm) = monitor.millimeters;
    if width_mm == 0 || height_mm == 0 {
        return 1.0;
    }
    let size = monitor.bounds.size;
    let pixels = f64::from(size.width) * f64::from(size.height);
    let ppmm = (pixels / (f64::from(width_mm) * f64::from(height_mm))).sqrt();
    let factor = ((ppmm * (12.0 * 25.4 / 96.0)).round() / 12.0).max(1.0);
    if factor <= 20.0 {
        factor
    } else {
        1.0
    }
}

/// The monitors as displays at `scale`, in RandR order. The RandR primary
/// monitor is primary; without one, the monitor at the root origin (else the
/// first) is.
#[must_use]
pub fn displays(monitors: &[Monitor], scale: ScaleFactor) -> Vec<DisplayInfo> {
    let primary = primary(monitors);
    monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| DisplayInfo {
            id: DisplayId(monitor.id),
            name: monitor.name.clone(),
            logical_bounds: monitor.bounds.to_logical(scale),
            pixel_size: monitor.bounds.size,
            scale_factor: scale,
            is_primary: Some(index) == primary,
        })
        .collect()
}

fn primary(monitors: &[Monitor]) -> Option<usize> {
    let at_origin =
        |monitor: &Monitor| monitor.bounds.origin.x == 0 && monitor.bounds.origin.y == 0;
    monitors
        .iter()
        .position(|monitor| monitor.primary)
        .or_else(|| monitors.iter().position(at_origin))
        .or((!monitors.is_empty()).then_some(0))
}

#[cfg(test)]
mod tests {
    use chartreuse_core::display::DisplayLayout;

    use super::*;

    fn monitor(id: u64, bounds: PhysicalRect, millimeters: (u32, u32), primary: bool) -> Monitor {
        Monitor {
            id,
            name: format!("DP-{id}"),
            bounds,
            millimeters,
            primary,
        }
    }

    fn scale(factor: f64) -> ScaleFactor {
        ScaleFactor::new(factor).unwrap()
    }

    /// A 27" 4K monitor right of a 24" 1080p one, the 4K one primary.
    fn desk() -> Vec<Monitor> {
        vec![
            monitor(10, PhysicalRect::new(0, 0, 1920, 1080), (531, 299), false),
            monitor(20, PhysicalRect::new(1920, 0, 3840, 2160), (597, 336), true),
        ]
    }

    #[test]
    fn the_override_then_xsettings_then_the_resource_database_decide() {
        let all = ScaleSources {
            env_override: Some("1.75"),
            xsettings_dpi: Some(192.0),
            resource_dpi: Some(120.0),
        };
        assert_eq!(desktop_scale(all, &desk()), scale(1.75));
        let no_override = ScaleSources {
            env_override: None,
            ..all
        };
        assert_eq!(desktop_scale(no_override, &desk()), scale(2.0));
        let resources_only = ScaleSources {
            xsettings_dpi: None,
            ..no_override
        };
        assert_eq!(desktop_scale(resources_only, &desk()), scale(1.25));
    }

    #[test]
    fn without_a_dpi_setting_the_primary_monitors_density_decides() {
        // 3840 px over 597 mm is 163 DPI, 1.7 × 96: 20 twelfths.
        let primary = scale(20.0 / 12.0);
        assert_eq!(desktop_scale(ScaleSources::default(), &desk()), primary);
        let randr = ScaleSources {
            env_override: Some("randr"),
            xsettings_dpi: Some(96.0),
            resource_dpi: None,
        };
        assert_eq!(desktop_scale(randr, &desk()), primary);
    }

    #[test]
    fn size_based_factors_never_go_below_one_and_ignore_unknown_sizes() {
        let desk = desk();
        // 92 DPI rounds to 11/12, raised to 1.
        assert_eq!(size_scale(&desk[0]), 1.0);
        let unknown = monitor(1, PhysicalRect::new(0, 0, 3840, 2160), (0, 0), true);
        assert_eq!(size_scale(&unknown), 1.0);
        // A projector reporting 1 mm: absurd, so 1.
        let tiny = monitor(1, PhysicalRect::new(0, 0, 3840, 2160), (1, 1), true);
        assert_eq!(size_scale(&tiny), 1.0);
    }

    #[test]
    fn invalid_settings_fall_back_to_one() {
        let bad = ScaleSources {
            env_override: Some("fast"),
            xsettings_dpi: Some(0.0),
            resource_dpi: None,
        };
        assert_eq!(desktop_scale(bad, &[]), ScaleFactor::ONE);
    }

    #[test]
    fn displays_share_the_scale_and_address_their_framebuffers_exactly() {
        let monitors = [
            monitor(1, PhysicalRect::new(0, 0, 2561, 1441), (0, 0), false),
            monitor(2, PhysicalRect::new(2561, -7, 1367, 769), (0, 0), true),
        ];
        let displays = displays(&monitors, scale(1.25));
        let layout = DisplayLayout::new(displays.clone()).unwrap();
        assert_eq!(layout.primary().id, DisplayId(2));
        for (display, monitor) in displays.iter().zip(&monitors) {
            assert_eq!(display.pixel_size, display.pixel_grid().pixel_size());
            assert_eq!(
                display.logical_bounds.to_physical(scale(1.25)),
                monitor.bounds
            );
        }
        // Adjacent monitors stay adjacent.
        assert_eq!(
            displays[0].logical_bounds.max_x(),
            displays[1].logical_bounds.min_x()
        );
    }

    #[test]
    fn without_a_randr_primary_the_monitor_at_the_origin_is_primary() {
        let monitors = [
            monitor(1, PhysicalRect::new(1920, 0, 1920, 1080), (0, 0), false),
            monitor(2, PhysicalRect::new(0, 0, 1920, 1080), (0, 0), false),
        ];
        let primaries: Vec<bool> = displays(&monitors, ScaleFactor::ONE)
            .iter()
            .map(|display| display.is_primary)
            .collect();
        assert_eq!(primaries, [false, true]);
        let elsewhere = [monitor(1, PhysicalRect::new(5, 5, 10, 10), (0, 0), false)];
        assert!(displays(&elsewhere, ScaleFactor::ONE)[0].is_primary);
    }
}

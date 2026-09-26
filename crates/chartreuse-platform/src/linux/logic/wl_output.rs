//! Wayland outputs as a display model.
//!
//! The compositor lays outputs out in its own logical space, which
//! `xdg-output` reports (position and size in logical pixels). An output's
//! framebuffer is its current mode, rotated by its transform. With fractional
//! scaling the ratio between the two is the output's scale (1.25, 1.5, …);
//! `wl_output.scale` only carries the integer the compositor rounds it up to,
//! so it is used only without `xdg-output`.
//!
//! Wayland has no primary output. The output at the compositor's origin is
//! primary (else the one nearest to it), and the layout is shifted so that it
//! sits at the global origin.

use chartreuse_core::display::{DisplayId, DisplayInfo};
use chartreuse_core::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalSize, ScaleFactor,
};

/// What the compositor told about one `wl_output`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Output {
    /// The output's registry name, unique while it is connected.
    pub id: u32,
    /// `wl_output.description` or `xdg_output.description`.
    pub description: Option<String>,
    /// `wl_output.name` or `xdg_output.name`, such as `DP-1`.
    pub name: Option<String>,
    /// The current mode, in the output's unrotated orientation.
    pub mode: Option<PhysicalSize>,
    /// The transform turns the output by 90° or 270°.
    pub rotated: bool,
    /// `wl_output.scale`.
    pub integer_scale: i32,
    /// `wl_output.geometry`'s position, in compositor space.
    pub position: (i32, i32),
    /// `xdg_output`'s logical position and size.
    pub logical: Option<((i32, i32), (i32, i32))>,
}

impl Output {
    /// The framebuffer size: the mode, rotated like the output.
    fn pixels(&self) -> Option<PhysicalSize> {
        let mode = self.mode.filter(|mode| !mode.is_empty())?;
        Some(if self.rotated {
            PhysicalSize::new(mode.height, mode.width)
        } else {
            mode
        })
    }

    /// The output's area in compositor space and its scale factor.
    fn geometry(&self) -> Option<(LogicalRect, ScaleFactor)> {
        let integer_scale = f64::from(self.integer_scale.max(1));
        let pixels = self.pixels();
        let (position, size) = match (self.logical, pixels) {
            (Some(((x, y), (width, height))), _) if width > 0 && height > 0 => (
                LogicalPoint::new(f64::from(x), f64::from(y)),
                LogicalSize::new(f64::from(width), f64::from(height)),
            ),
            (_, Some(pixels)) => (
                LogicalPoint::new(f64::from(self.position.0), f64::from(self.position.1)),
                LogicalSize::new(
                    f64::from(pixels.width) / integer_scale,
                    f64::from(pixels.height) / integer_scale,
                ),
            ),
            _ => return None,
        };
        let factor = pixels.map_or(integer_scale, |pixels| f64::from(pixels.width) / size.width);
        let scale = ScaleFactor::new(factor)?;
        Some((
            LogicalRect {
                origin: position,
                size,
            },
            scale,
        ))
    }
}

/// The outputs whose geometry is known, as displays in the order given.
///
/// Each display's pixel size is its logical size times its scale (the
/// framebuffer, give or take the compositor's rounding of the logical size),
/// so that captures resampled to it line up with the display model.
#[must_use]
pub fn displays(outputs: &[Output]) -> Vec<DisplayInfo> {
    let placed: Vec<(&Output, LogicalRect, ScaleFactor)> = outputs
        .iter()
        .filter_map(|output| {
            let (bounds, scale) = output.geometry()?;
            Some((output, bounds, scale))
        })
        .collect();
    let distance = |bounds: &LogicalRect| bounds.origin.x.abs() + bounds.origin.y.abs();
    let Some(primary) = placed
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(&a.1).total_cmp(&distance(&b.1)))
        .map(|(index, _)| index)
    else {
        return Vec::new();
    };
    let origin = placed[primary].1.origin;
    placed
        .iter()
        .enumerate()
        .map(|(index, (output, bounds, scale))| {
            let logical_bounds = LogicalRect {
                origin: LogicalPoint::new(bounds.origin.x - origin.x, bounds.origin.y - origin.y),
                size: bounds.size,
            };
            let name = output
                .description
                .clone()
                .or_else(|| output.name.clone())
                .unwrap_or_else(|| format!("Display {}", index + 1));
            let mut display = DisplayInfo {
                id: DisplayId(u64::from(output.id)),
                name,
                logical_bounds,
                pixel_size: PhysicalSize::new(0, 0),
                scale_factor: *scale,
                is_primary: index == primary,
            };
            display.pixel_size = display.pixel_grid().pixel_size();
            display
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chartreuse_core::display::DisplayLayout;

    use super::*;

    fn output(id: u32, mode: (u32, u32), logical: ((i32, i32), (i32, i32))) -> Output {
        Output {
            id,
            name: Some(format!("DP-{id}")),
            mode: Some(PhysicalSize::new(mode.0, mode.1)),
            integer_scale: 1,
            logical: Some(logical),
            ..Output::default()
        }
    }

    #[test]
    fn fractional_scales_come_from_the_mode_and_the_logical_size() {
        // A 4K panel at 150 % next to a 1080p one at 100 %.
        let outputs = [
            output(1, (3840, 2160), ((0, 0), (2560, 1440))),
            output(2, (1920, 1080), ((2560, 0), (1920, 1080))),
        ];
        let displays = displays(&outputs);
        assert_eq!(displays[0].scale_factor.get(), 1.5);
        assert_eq!(displays[0].pixel_size, PhysicalSize::new(3840, 2160));
        assert_eq!(displays[1].scale_factor.get(), 1.0);
        assert_eq!(
            displays[1].logical_bounds,
            LogicalRect::new(2560.0, 0.0, 1920.0, 1080.0)
        );
        assert!(DisplayLayout::new(displays).is_ok());
    }

    #[test]
    fn rotated_outputs_swap_their_mode() {
        let rotated = Output {
            rotated: true,
            ..output(1, (2560, 1440), ((0, 0), (1440, 2560)))
        };
        let display = &displays(&[rotated])[0];
        assert_eq!(display.pixel_size, PhysicalSize::new(1440, 2560));
        assert_eq!(display.scale_factor, ScaleFactor::ONE);
    }

    #[test]
    fn without_xdg_output_the_integer_scale_and_position_apply() {
        let legacy = Output {
            logical: None,
            integer_scale: 2,
            position: (0, 0),
            ..output(1, (2880, 1800), ((0, 0), (0, 0)))
        };
        let display = &displays(&[legacy])[0];
        assert_eq!(
            display.logical_bounds,
            LogicalRect::new(0.0, 0.0, 1440.0, 900.0)
        );
        assert_eq!(display.scale_factor.get(), 2.0);
    }

    #[test]
    fn the_output_nearest_the_origin_becomes_primary_at_the_origin() {
        let outputs = [
            output(1, (1920, 1080), ((-1920, 200), (1920, 1080))),
            output(2, (1920, 1080), ((100, 0), (1920, 1080))),
            Output {
                mode: None,
                logical: None,
                ..output(3, (0, 0), ((0, 0), (0, 0)))
            },
        ];
        let displays = displays(&outputs);
        assert_eq!(displays.len(), 2, "outputs without geometry are skipped");
        assert!(displays[1].is_primary);
        assert_eq!(
            displays[1].logical_bounds.origin,
            LogicalPoint::new(0.0, 0.0)
        );
        assert_eq!(
            displays[0].logical_bounds.origin,
            LogicalPoint::new(-2020.0, 200.0)
        );
        assert!(!displays[0].is_primary);
    }

    #[test]
    fn pixel_sizes_follow_the_display_model_when_logical_sizes_are_rounded() {
        // 2560×1600 at 1.75: the compositor reports 1463×914 logical pixels.
        let outputs = [output(1, (2560, 1600), ((0, 0), (1463, 914)))];
        let display = &displays(&outputs)[0];
        assert_eq!(display.pixel_size, display.pixel_grid().pixel_size());
        assert_eq!(display.pixel_size.width, 2560);
        assert!(display.pixel_size.height.abs_diff(1600) <= 1);
    }
}

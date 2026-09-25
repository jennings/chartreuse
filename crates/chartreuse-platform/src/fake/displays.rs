//! Fake [`Displays`]: the world's synthetic displays.

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::Result;

use super::Fake;
use crate::displays::Displays;

impl Displays for Fake {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(self.state.lock().displays.clone())
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::LogicalPoint;

    use super::*;

    #[test]
    fn default_world_covers_the_awkward_layouts() {
        let displays = Fake::new().displays().unwrap();
        assert_eq!(displays.iter().filter(|d| d.is_primary).count(), 1);
        let primary = displays.iter().find(|d| d.is_primary).unwrap();
        assert_eq!(primary.logical_bounds.origin, LogicalPoint::new(0.0, 0.0));
        assert!(displays
            .iter()
            .any(|d| d.logical_bounds.min_x() < 0.0 && d.logical_bounds.min_y() < 0.0));
        let mut scales: Vec<f64> = displays.iter().map(|d| d.scale_factor.get()).collect();
        scales.dedup();
        assert_eq!(
            scales.len(),
            displays.len(),
            "every display has its own scale factor"
        );
        assert!(
            scales.iter().any(|s| s.fract() != 0.0),
            "one scale factor is fractional"
        );
    }

    #[test]
    fn pixel_sizes_match_logical_bounds_times_scale() {
        for display in Fake::new().displays().unwrap() {
            assert_eq!(
                display
                    .logical_bounds
                    .size
                    .to_physical(display.scale_factor),
                display.pixel_size,
                "{}",
                display.name
            );
        }
    }

    #[test]
    fn displays_do_not_overlap() {
        let displays = Fake::new().displays().unwrap();
        for (i, a) in displays.iter().enumerate() {
            for b in &displays[i + 1..] {
                assert_eq!(a.logical_bounds.intersection(&b.logical_bounds), None);
            }
        }
    }
}

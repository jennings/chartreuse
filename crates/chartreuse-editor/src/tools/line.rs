//! The line tool: drag from one end to the other. Shift snaps the line to a
//! multiple of 45°.

use std::f32::consts::FRAC_1_SQRT_2;

use super::{DragShape, DragTool, ToolKind};
use crate::model::{self, Point, Shape, Vector};

/// The line tool: drag from one end to the other; Shift snaps it to 45°.
pub type LineTool = DragTool<Line>;

/// The line tool's geometry (see [`DragShape`]).
#[derive(Debug)]
pub struct Line;

impl DragShape for Line {
    const KIND: ToolKind = ToolKind::Line;

    fn shape(start: Point, end: Point, constrain: bool) -> Shape {
        let end = if constrain { snap_45(start, end) } else { end };
        Shape::Line(model::Line { start, end })
    }
}

/// The eight directions a constrained line can take.
const DIRECTIONS: [Vector; 8] = [
    Vector::new(1.0, 0.0),
    Vector::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    Vector::new(0.0, 1.0),
    Vector::new(-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    Vector::new(-1.0, 0.0),
    Vector::new(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    Vector::new(0.0, -1.0),
    Vector::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
];

/// `end` projected onto the nearest ray from `start` at a multiple of 45°, so
/// the pointer's position along that ray still sets the length.
pub(super) fn snap_45(start: Point, end: Point) -> Point {
    let along = end - start;
    let (direction, length) = DIRECTIONS
        .iter()
        .map(|&direction| (direction, direction.dot(along)))
        .fold((Vector::ZERO, 0.0), |best, candidate| {
            if candidate.1 > best.1 {
                candidate
            } else {
                best
            }
        });
    start + direction * length
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconstrained_lines_follow_the_pointer() {
        let (a, b) = (Point::new(1.0, 2.0), Point::new(7.0, 3.0));
        assert_eq!(
            Line::shape(a, b, false),
            Shape::Line(model::Line { start: a, end: b })
        );
    }

    #[test]
    fn shift_snaps_to_the_nearest_45_degrees() {
        let start = Point::new(10.0, 10.0);
        // Nearly horizontal and nearly vertical snap exactly onto the axis.
        assert_eq!(
            snap_45(start, Point::new(50.0, 13.0)),
            Point::new(50.0, 10.0)
        );
        assert_eq!(
            snap_45(start, Point::new(8.0, -30.0)),
            Point::new(10.0, -30.0)
        );
        // Nearly diagonal lands on the diagonal.
        let end = snap_45(start, Point::new(40.0, 38.0));
        assert!((end.x - end.y).abs() < 1e-4, "{end:?}");
        assert!((end.x - 39.0).abs() < 1e-4, "{end:?}");
        assert_eq!(snap_45(start, start), start);
    }
}

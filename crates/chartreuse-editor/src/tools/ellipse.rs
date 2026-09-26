//! The ellipse tool: drag out the ellipse's bounding box from one corner to
//! the opposite one. Shift makes it a circle.

use super::rectangle::square;
use super::{DragShape, DragTool, ToolKind};
use crate::model::{self, Point, Rect, Shape};

/// The ellipse tool: drag out its bounding box; Shift makes it a circle.
pub type EllipseTool = DragTool<Ellipse>;

/// The ellipse tool's geometry (see [`DragShape`]).
#[derive(Debug)]
pub struct Ellipse;

impl DragShape for Ellipse {
    const KIND: ToolKind = ToolKind::Ellipse;

    fn shape(start: Point, end: Point, constrain: bool) -> Shape {
        let end = if constrain { square(start, end) } else { end };
        Shape::Ellipse(model::Ellipse {
            rect: Rect::from_corners(start, end),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_spans_the_bounding_box_and_shift_makes_a_circle() {
        let (a, b) = (Point::new(50.0, 10.0), Point::new(20.0, 30.0));
        assert_eq!(
            Ellipse::shape(a, b, false),
            Shape::Ellipse(model::Ellipse {
                rect: Rect::from_corners(Point::new(20.0, 10.0), Point::new(50.0, 30.0)),
            })
        );
        assert_eq!(
            Ellipse::shape(a, b, true),
            Shape::Ellipse(model::Ellipse {
                rect: Rect::from_corners(Point::new(20.0, 10.0), Point::new(50.0, 40.0)),
            })
        );
    }
}

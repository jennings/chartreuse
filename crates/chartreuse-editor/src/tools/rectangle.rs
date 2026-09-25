//! The rectangle tool: drag from one corner to the opposite one. Shift makes
//! it a square.

use super::{DragShape, DragTool, ToolKind};
use crate::model::{self, Point, Rect, Shape, Vector};

/// The rectangle tool.
pub type RectangleTool = DragTool<Rectangle>;

/// The rectangle tool's geometry (see [`DragShape`]).
#[derive(Debug)]
pub struct Rectangle;

impl DragShape for Rectangle {
    const KIND: ToolKind = ToolKind::Rectangle;

    fn shape(start: Point, end: Point, constrain: bool) -> Shape {
        let end = if constrain { square(start, end) } else { end };
        Shape::Rectangle(model::Rectangle {
            rect: Rect::from_corners(start, end),
        })
    }
}

/// The corner opposite `start` of the square with a side as long as the
/// drag's longer side, in the drag's direction.
pub(super) fn square(start: Point, end: Point) -> Point {
    let along = end - start;
    let side = along.x.abs().max(along.y.abs());
    let signed = |d: f32| if d < 0.0 { -side } else { side };
    start + Vector::new(signed(along.x), signed(along.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_spans_the_rectangle_in_any_direction() {
        let (a, b) = (Point::new(50.0, 10.0), Point::new(20.0, 40.0));
        assert_eq!(
            Rectangle::shape(a, b, false),
            Shape::Rectangle(model::Rectangle {
                rect: Rect::from_corners(Point::new(20.0, 10.0), Point::new(50.0, 40.0)),
            })
        );
    }

    #[test]
    fn shift_makes_a_square_on_the_longer_side() {
        let start = Point::new(10.0, 10.0);
        assert_eq!(
            square(start, Point::new(40.0, 20.0)),
            Point::new(40.0, 40.0)
        );
        assert_eq!(
            square(start, Point::new(5.0, -30.0)),
            Point::new(-30.0, -30.0)
        );
        assert_eq!(square(start, start), start);
    }
}

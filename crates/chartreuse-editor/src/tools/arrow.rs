//! The arrow tool: drag from the tail to the tip. Shift snaps the arrow to a
//! multiple of 45°.

use super::line::snap_45;
use super::{DragShape, DragTool, ToolKind};
use crate::model::{self, Point, Shape};

/// The arrow tool: drag from the tail to the tip; Shift snaps it to 45°.
pub type ArrowTool = DragTool<Arrow>;

/// The arrow tool's geometry (see [`DragShape`]).
#[derive(Debug)]
pub struct Arrow;

impl DragShape for Arrow {
    const KIND: ToolKind = ToolKind::Arrow;

    fn shape(start: Point, end: Point, constrain: bool) -> Shape {
        let end = if constrain { snap_45(start, end) } else { end };
        Shape::Arrow(model::Arrow { start, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tip_is_where_the_drag_ends() {
        let (tail, tip) = (Point::new(5.0, 5.0), Point::new(60.0, 9.0));
        assert_eq!(
            Arrow::shape(tail, tip, false),
            Shape::Arrow(model::Arrow {
                start: tail,
                end: tip
            })
        );
        assert_eq!(
            Arrow::shape(tail, tip, true),
            Shape::Arrow(model::Arrow {
                start: tail,
                end: Point::new(60.0, 5.0),
            })
        );
    }
}

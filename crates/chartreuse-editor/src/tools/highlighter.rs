//! The highlighter: draw a wide, translucent freehand stroke by dragging
//! (see [`FreehandTool`] and [`highlighter`](crate::model::highlighter)).

use super::{FreehandShape, FreehandTool, ToolKind};
use crate::model::{Point, Polyline, Shape};

/// The highlighter tool: press, drag, and release to highlight along a path.
pub type HighlighterTool = FreehandTool<Highlighter>;

/// The highlighter tool's geometry (see [`FreehandShape`]).
#[derive(Debug)]
pub struct Highlighter;

impl FreehandShape for Highlighter {
    const KIND: ToolKind = ToolKind::Highlighter;

    fn shape(points: Vec<Point>) -> Shape {
        Shape::Highlighter(Polyline { points })
    }
}

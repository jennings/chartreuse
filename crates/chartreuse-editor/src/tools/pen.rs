//! The freehand pen: draw a stroke by dragging (see [`FreehandTool`]).

use super::{FreehandShape, FreehandTool, ToolKind};
use crate::model::{Point, Polyline, Shape};

/// The pen tool: press, drag, and release to draw a freehand stroke.
pub type PenTool = FreehandTool<Pen>;

/// The pen tool's geometry (see [`FreehandShape`]).
#[derive(Debug)]
pub struct Pen;

impl FreehandShape for Pen {
    const KIND: ToolKind = ToolKind::Pen;

    fn shape(points: Vec<Point>) -> Shape {
        Shape::Pen(Polyline { points })
    }
}

#[cfg(test)]
mod tests {
    use super::super::drag::testing::{document, send};
    use super::super::{Pointer, Preview, Tool};
    use super::*;

    #[test]
    fn a_stroke_adds_one_smoothed_selected_pen_annotation() {
        let mut document = document();
        let mut tool = PenTool::default();
        let press = Pointer::Press {
            at: Point::new(10.0, 10.0),
            clicks: 1,
        };
        send(&mut tool, &mut document, false, press);
        // Along a right angle, recording a point at least a pixel apart.
        for (x, y) in [(10.5, 10.0), (30.0, 10.0), (50.0, 10.0), (50.0, 30.0)] {
            let at = Point::new(x, y);
            send(&mut tool, &mut document, false, Pointer::Move { at });
        }
        let Preview::New(Shape::Pen(raw)) = tool.preview() else {
            panic!("expected a pen preview");
        };
        assert_eq!(
            raw.points.len(),
            4,
            "the point under a pixel away is skipped"
        );
        assert!(
            document.annotations().is_empty(),
            "nothing added mid-stroke"
        );
        let end = Point::new(50.0, 50.0);
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Release { at: end },
        );

        let [added] = document.annotations() else {
            panic!("expected one annotation");
        };
        let Shape::Pen(pen) = &added.shape else {
            panic!("not a pen stroke: {:?}", added.shape);
        };
        assert_eq!(pen.points.first(), Some(&Point::new(10.0, 10.0)));
        assert_eq!(pen.points.last(), Some(&end));
        // The corner at (50, 10) is rounded off: no point sits on it.
        assert!(!pen.points.contains(&Point::new(50.0, 10.0)));
        assert!(document.is_selected(added.id()));
        assert!(!tool.is_active());
        assert!(document.undo());
        assert!(!document.can_undo());
    }

    #[test]
    fn a_click_leaves_a_dot_and_escape_abandons_a_stroke() {
        let mut document = document();
        let mut tool = PenTool::default();
        let at = Point::new(10.0, 10.0);
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press { at, clicks: 1 },
        );
        send(&mut tool, &mut document, false, Pointer::Release { at });
        assert_eq!(
            document.annotations()[0].shape,
            Shape::Pen(Polyline { points: vec![at] })
        );

        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press { at, clicks: 1 },
        );
        let mut cx = super::super::Context {
            document: &mut document,
            style: crate::model::Style::default(),
            pixel: 1.0,
            shift: false,
        };
        assert!(tool.escape(&mut cx));
        assert_eq!(tool.preview(), Preview::None);
        assert_eq!(document.annotations().len(), 1);
    }
}

//! The step marker tool: click to place the next numbered marker.

use iced::mouse::Interaction;

use super::{Context, Pointer, Preview, Tool, ToolKind};
use crate::model::{Document, Point, Shape, StepMarker};

/// The step marker tool: each click places a [`StepMarker`], numbered one
/// past the markers already in the document.
///
/// Pressing previews the marker under the pointer, which follows it while
/// the button is down; releasing adds it there (one undo step) and selects
/// it. Its number is derived from the document (see
/// [`Document::step_number`]), so it is never stored.
#[derive(Debug, Default)]
pub struct StepTool {
    /// Where the marker being placed is.
    pending: Option<Point>,
}

impl StepTool {
    fn shape(center: Point) -> Shape {
        Shape::Step(StepMarker { center })
    }
}

impl Tool for StepTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Step
    }

    fn pointer(&mut self, pointer: Pointer, cx: &mut Context<'_>) {
        match pointer {
            Pointer::Press { at, .. } => self.pending = Some(at),
            Pointer::Move { at } => {
                if let Some(pending) = &mut self.pending {
                    *pending = at;
                }
            }
            Pointer::Release { at } => {
                self.pointer(Pointer::Move { at }, cx);
                self.finish(cx);
            }
        }
    }

    fn escape(&mut self, _cx: &mut Context<'_>) -> bool {
        self.pending.take().is_some()
    }

    fn finish(&mut self, cx: &mut Context<'_>) {
        if let Some(center) = self.pending.take() {
            let id = cx.document.add(Self::shape(center), cx.style);
            cx.document.set_selection([id]);
        }
    }

    fn is_active(&self) -> bool {
        self.pending.is_some()
    }

    fn preview(&self) -> Preview<'_> {
        match self.pending {
            Some(center) => Preview::New(Self::shape(center)),
            None => Preview::None,
        }
    }

    fn cursor(&self, _document: &Document, _at: Point, _pixel: f32) -> Interaction {
        Interaction::Crosshair
    }
}

#[cfg(test)]
mod tests {
    use super::super::drag::testing::{document, send};
    use super::*;

    fn click(tool: &mut StepTool, document: &mut Document, at: Point) {
        send(tool, document, false, Pointer::Press { at, clicks: 1 });
        send(tool, document, false, Pointer::Release { at });
    }

    #[test]
    fn each_click_places_the_next_number_selected_as_one_undo_step() {
        let mut document = document();
        let mut tool = StepTool::default();
        for x in [10.0, 50.0, 90.0] {
            click(&mut tool, &mut document, Point::new(x, 20.0));
        }
        let numbers: Vec<_> = document
            .annotations()
            .iter()
            .map(|a| document.step_number(a.id()))
            .collect();
        assert_eq!(numbers, [Some(1), Some(2), Some(3)]);
        let last = document.annotations()[2].id();
        assert_eq!(
            document.selection().iter().copied().collect::<Vec<_>>(),
            [last]
        );
        assert!(document.undo());
        assert_eq!(document.annotations().len(), 2);
    }

    #[test]
    fn the_marker_follows_the_pointer_until_released_or_escaped() {
        let mut document = document();
        let mut tool = StepTool::default();
        let (from, to) = (Point::new(10.0, 10.0), Point::new(40.0, 30.0));
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: from,
                clicks: 1,
            },
        );
        send(&mut tool, &mut document, false, Pointer::Move { at: to });
        assert_eq!(tool.preview(), Preview::New(StepTool::shape(to)));
        assert!(document.annotations().is_empty());
        send(&mut tool, &mut document, false, Pointer::Release { at: to });
        assert_eq!(document.annotations()[0].shape, StepTool::shape(to));

        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: from,
                clicks: 1,
            },
        );
        let mut cx = Context {
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

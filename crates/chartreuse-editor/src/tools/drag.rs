//! The press-drag-release state machine shared by tools that draw a shape
//! between two points (line, arrow, rectangle).

use std::fmt;
use std::marker::PhantomData;

use iced::mouse::Interaction;

use super::handles::{handle_at, Reshape};
use super::{Context, Pointer, Preview, Tool, ToolKind};
use crate::model::{Document, Point, Shape};

/// The geometry of a drag tool: which shape a drag draws.
pub trait DragShape: fmt::Debug + 'static {
    const KIND: ToolKind;

    /// The shape for a drag from `start` to `end`. `constrain` is true while
    /// Shift is held.
    fn shape(start: Point, end: Point, constrain: bool) -> Shape;
}

/// A tool that draws one `S` per drag.
///
/// Pressing starts a drag; once the pointer has moved more than
/// [`DRAG_THRESHOLD`](super::DRAG_THRESHOLD) from the press, the shape is
/// previewed, and releasing adds it (one undo step) and selects it. Releasing
/// before that point adds nothing, so clicks and tiny drags are ignored.
///
/// Pressing on a [handle](super::handles) of the lone selected annotation
/// (such as the shape just drawn) drags the handle instead, as the select
/// tool does.
pub struct DragTool<S> {
    gesture: Option<Gesture>,
    shape: PhantomData<S>,
}

#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    Create(Drag),
    Reshape(Reshape),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    start: Point,
    end: Point,
    constrain: bool,
    /// Whether the pointer has left the threshold around `start`.
    moved: bool,
}

impl<S> Default for DragTool<S> {
    fn default() -> Self {
        Self {
            gesture: None,
            shape: PhantomData,
        }
    }
}

impl<S: DragShape> fmt::Debug for DragTool<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragTool")
            .field("kind", &S::KIND)
            .field("gesture", &self.gesture)
            .finish()
    }
}

impl<S: DragShape> DragTool<S> {
    fn shape(drag: &Drag) -> Shape {
        S::shape(drag.start, drag.end, drag.constrain)
    }

    /// Records the gesture: adds the dragged shape if the drag got past the
    /// threshold, or the reshaped annotation.
    fn commit(gesture: Gesture, cx: &mut Context<'_>) {
        match gesture {
            Gesture::Create(drag) if drag.moved => {
                let id = cx.document.add(Self::shape(&drag), cx.style);
                cx.document.set_selection([id]);
            }
            Gesture::Create(_) => {}
            Gesture::Reshape(reshape) => reshape.commit(cx.document),
        }
    }
}

impl<S: DragShape> Tool for DragTool<S> {
    fn kind(&self) -> ToolKind {
        S::KIND
    }

    fn pointer(&mut self, pointer: Pointer, cx: &mut Context<'_>) {
        match pointer {
            Pointer::Press { at, .. } => {
                let reshape = handle_at(cx.document, at, cx.handle_reach())
                    .and_then(|(id, handle)| Reshape::start(cx.document, id, handle, at));
                self.gesture = Some(match reshape {
                    Some(reshape) => Gesture::Reshape(reshape),
                    None => Gesture::Create(Drag {
                        start: at,
                        end: at,
                        constrain: cx.shift,
                        moved: false,
                    }),
                });
            }
            Pointer::Move { at } => match &mut self.gesture {
                Some(Gesture::Create(drag)) => {
                    drag.end = at;
                    drag.constrain = cx.shift;
                    drag.moved |= at.distance(drag.start) > cx.drag_threshold();
                }
                Some(Gesture::Reshape(reshape)) => {
                    reshape.drag_to(at, cx.shift, cx.drag_threshold());
                }
                None => {}
            },
            Pointer::Release { at } => {
                self.pointer(Pointer::Move { at }, cx);
                self.finish(cx);
            }
        }
    }

    fn escape(&mut self, _cx: &mut Context<'_>) -> bool {
        self.gesture.take().is_some()
    }

    fn finish(&mut self, cx: &mut Context<'_>) {
        if let Some(gesture) = self.gesture.take() {
            Self::commit(gesture, cx);
        }
    }

    fn is_active(&self) -> bool {
        self.gesture.is_some()
    }

    fn preview(&self) -> Preview<'_> {
        match &self.gesture {
            Some(Gesture::Create(drag)) if drag.moved => Preview::New(Self::shape(drag)),
            Some(Gesture::Reshape(reshape)) => Preview::Reshaped(reshape.id(), reshape.shape()),
            _ => Preview::None,
        }
    }

    fn cursor(&self, _document: &Document, _at: Point, _pixel: f32) -> Interaction {
        Interaction::Crosshair
    }
}

/// Test helpers shared by the tool modules.
#[cfg(test)]
pub(super) mod testing {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_core::image::Image;

    use super::super::{Context, Pointer, Tool};
    use crate::model::{Document, Point, Style};

    pub fn document() -> Document {
        Document::new(Image::filled(
            PhysicalSize::new(400, 300),
            Rgba8::from_rgb_hex(0xffffff),
        ))
    }

    /// Feeds `pointer` to `tool` at 1:1 zoom, with Shift as given.
    pub fn send(tool: &mut dyn Tool, document: &mut Document, shift: bool, pointer: Pointer) {
        let mut cx = Context {
            document,
            style: Style::default(),
            pixel: 1.0,
            shift,
        };
        tool.pointer(pointer, &mut cx);
    }

    /// A press at `from`, a move to `to`, and a release there.
    pub fn drag(tool: &mut dyn Tool, document: &mut Document, from: Point, to: Point) {
        send(
            tool,
            document,
            false,
            Pointer::Press {
                at: from,
                clicks: 1,
            },
        );
        send(tool, document, false, Pointer::Move { at: to });
        send(tool, document, false, Pointer::Release { at: to });
    }
}

#[cfg(test)]
mod tests {
    use super::super::LineTool;
    use super::testing::{document, drag, send};
    use super::*;
    use crate::model::{Line, Style};

    fn line(start: Point, end: Point) -> Shape {
        Shape::Line(Line { start, end })
    }

    #[test]
    fn a_drag_adds_exactly_one_selected_annotation_as_one_undo_step() {
        let mut document = document();
        let mut tool = LineTool::default();
        let (a, b) = (Point::new(10.0, 10.0), Point::new(90.0, 40.0));
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press { at: a, clicks: 1 },
        );
        for x in [20.0, 50.0, 70.0] {
            send(
                &mut tool,
                &mut document,
                false,
                Pointer::Move {
                    at: Point::new(x, 30.0),
                },
            );
        }
        assert!(document.annotations().is_empty(), "nothing added mid-drag");
        send(&mut tool, &mut document, false, Pointer::Release { at: b });

        let [added] = document.annotations() else {
            panic!("expected one annotation, got {:?}", document.annotations());
        };
        assert_eq!(added.shape, line(a, b));
        assert_eq!(added.style, Style::default());
        assert!(document.is_selected(added.id()));
        assert!(!tool.is_active());
        assert!(document.undo());
        assert!(document.annotations().is_empty());
        assert!(!document.can_undo());
    }

    #[test]
    fn drags_within_the_threshold_add_nothing() {
        let mut document = document();
        let mut tool = LineTool::default();
        let start = Point::new(10.0, 10.0);
        drag(&mut tool, &mut document, start, Point::new(12.0, 12.0));
        drag(&mut tool, &mut document, start, start);
        assert!(document.annotations().is_empty());
        assert!(!document.can_undo());
    }

    #[test]
    fn the_threshold_is_in_canvas_pixels() {
        let mut document = document();
        let mut tool = LineTool::default();
        // Zoomed in 10×: a 2-unit drag is 20 canvas pixels.
        let mut cx = Context {
            document: &mut document,
            style: Style::default(),
            pixel: 0.1,
            shift: false,
        };
        let start = Point::new(10.0, 10.0);
        tool.pointer(
            Pointer::Press {
                at: start,
                clicks: 1,
            },
            &mut cx,
        );
        tool.pointer(
            Pointer::Release {
                at: Point::new(12.0, 10.0),
            },
            &mut cx,
        );
        assert_eq!(document.annotations().len(), 1);
    }

    #[test]
    fn the_preview_appears_once_the_drag_passes_the_threshold() {
        let mut document = document();
        let mut tool = LineTool::default();
        let start = Point::new(10.0, 10.0);
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: start,
                clicks: 1,
            },
        );
        assert!(tool.is_active());
        assert_eq!(tool.preview(), Preview::None);
        let end = Point::new(30.0, 10.0);
        send(&mut tool, &mut document, false, Pointer::Move { at: end });
        assert_eq!(tool.preview(), Preview::New(line(start, end)));
        // Coming back near the start keeps it a drag.
        send(&mut tool, &mut document, false, Pointer::Move { at: start });
        assert_eq!(tool.preview(), Preview::New(line(start, start)));
    }

    #[test]
    fn escape_abandons_the_drag() {
        let mut document = document();
        let mut tool = LineTool::default();
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: Point::ORIGIN,
                clicks: 1,
            },
        );
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Move {
                at: Point::new(50.0, 50.0),
            },
        );
        let mut cx = Context {
            document: &mut document,
            style: Style::default(),
            pixel: 1.0,
            shift: false,
        };
        assert!(tool.escape(&mut cx));
        assert!(!tool.escape(&mut cx), "nothing left to abandon");
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Release {
                at: Point::new(60.0, 60.0),
            },
        );
        assert!(document.annotations().is_empty());
        assert_eq!(tool.preview(), Preview::None);
    }

    #[test]
    fn finish_commits_the_drag_in_progress() {
        let mut document = document();
        let mut tool = LineTool::default();
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: Point::ORIGIN,
                clicks: 1,
            },
        );
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Move {
                at: Point::new(50.0, 0.0),
            },
        );
        let mut cx = Context {
            document: &mut document,
            style: Style::default(),
            pixel: 1.0,
            shift: false,
        };
        tool.finish(&mut cx);
        assert_eq!(document.annotations().len(), 1);
        assert!(!tool.is_active());
    }
}

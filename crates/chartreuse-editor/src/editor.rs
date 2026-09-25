//! The editor widget: one image being annotated, with its tools, style, zoom,
//! and pan.

use chartreuse_core::image::Image;
use iced::widget::image;
use iced::{keyboard, Element};

use crate::canvas::{self, Input, InputKind, View, Viewport, Zoom};
use crate::font;
use crate::model::{Document, Size, Style};
use crate::tools::{Context, Pointer, Tool, ToolKind};

/// Canvas pixels of Cmd-scrolling that double (or halve) the zoom.
const SCROLL_PER_DOUBLING: f32 = 200.0;

/// The editor's messages. The canvas and (later) the toolbar produce them;
/// the app routes them back to [`Editor::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Input from the canvas.
    Canvas(Input),
}

/// Something the editor asks its owner to do, returned by [`Editor::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {}

/// The state of one editor: the [`Document`], the active tool, the style for
/// new annotations, and the zoom and pan.
///
/// Keep it in the app state, show [`view`](Self::view), and feed the
/// messages the view produces to [`update`](Self::update).
#[derive(Debug)]
pub struct Editor {
    document: Document,
    /// The base image for the renderer, uploaded once.
    image: image::Handle,
    tool: Box<dyn Tool>,
    style: Style,
    view: View,
    /// The canvas's size as of its latest input.
    canvas: iced::Size,
    modifiers: keyboard::Modifiers,
    /// The pointer's latest canvas position while the button is down.
    pointer: Option<iced::Point>,
}

impl Editor {
    /// An editor over `image` with no annotations, fitted to the window. Also
    /// loads the annotation font (see [`font::load`]).
    #[must_use]
    pub fn new(image: Image) -> Self {
        font::load();
        let handle =
            image::Handle::from_rgba(image.width(), image.height(), image.pixels().to_vec());
        let canvas = iced::Size::new(image.width() as f32, image.height() as f32);
        Self {
            document: Document::new(image),
            image: handle,
            tool: ToolKind::Line.create(),
            style: Style::default(),
            view: View::default(),
            canvas,
            modifiers: keyboard::Modifiers::default(),
            pointer: None,
        }
    }

    /// The document being edited. A gesture still in progress (a drag) is not
    /// in it yet.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// The active tool.
    #[must_use]
    pub fn tool(&self) -> ToolKind {
        self.tool.kind()
    }

    /// The style for new annotations.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    #[must_use]
    pub const fn zoom(&self) -> Zoom {
        self.view.zoom()
    }

    /// Handles a message from [`view`](Self::view).
    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Canvas(input) => self.canvas_input(input),
        }
        None
    }

    /// The editor's widgets.
    pub fn view(&self) -> Element<'_, Message> {
        canvas::view(self)
    }

    pub(crate) const fn image(&self) -> &image::Handle {
        &self.image
    }

    pub(crate) fn active_tool(&self) -> &dyn Tool {
        self.tool.as_ref()
    }

    pub(crate) const fn canvas_size(&self) -> iced::Size {
        self.canvas
    }

    /// The mapping for a canvas of `size`.
    pub(crate) fn viewport(&self, size: iced::Size) -> Viewport {
        self.view.viewport(size, self.image_size())
    }

    fn image_size(&self) -> Size {
        self.document.bounds().size()
    }

    fn canvas_input(&mut self, input: Input) {
        self.canvas = input.size;
        match input.kind {
            InputKind::Resized => {}
            InputKind::Press { position, clicks } => {
                self.pointer_at(position, |at| Pointer::Press { at, clicks });
            }
            InputKind::Move { position } => self.pointer_at(position, |at| Pointer::Move { at }),
            InputKind::Release { position } => {
                self.pointer_at(position, |at| Pointer::Release { at });
                self.pointer = None;
            }
            InputKind::Scroll { position, delta } => {
                let image = self.image_size();
                if self.modifiers.command() {
                    let factor = (delta.y / SCROLL_PER_DOUBLING).exp2();
                    self.view.zoom_by(factor, position, self.canvas, image);
                } else {
                    self.view.pan_by(delta, self.canvas, image);
                }
            }
            InputKind::Modifiers(modifiers) => {
                self.modifiers = modifiers;
                // Let a drag in progress pick up Shift without waiting for the
                // pointer to move.
                if let Some(position) = self.pointer
                    && self.tool.is_active()
                {
                    self.pointer_at(position, |at| Pointer::Move { at });
                }
            }
        }
    }

    /// Sends the tool a pointer event at canvas `position`.
    fn pointer_at(
        &mut self,
        position: iced::Point,
        event: impl FnOnce(crate::model::Point) -> Pointer,
    ) {
        self.pointer = Some(position);
        let viewport = self.viewport(self.canvas);
        let pointer = event(viewport.to_document(position));
        self.with_tool(|tool, cx| tool.pointer(pointer, cx));
    }

    /// Runs `f` on the active tool with a context for the current state.
    fn with_tool<T>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut Context<'_>) -> T) -> T {
        let pixel = self.viewport(self.canvas).to_document_length(1.0);
        let mut cx = Context {
            document: &mut self.document,
            style: self.style,
            pixel,
            shift: self.modifiers.shift(),
        };
        f(self.tool.as_mut(), &mut cx)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Driving an [`Editor`] the way its canvas does.

    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_core::image::Image;
    use iced::{Point, Vector};

    use super::*;

    /// A 400 × 300 image in a canvas with exactly [`canvas::MARGIN`] around
    /// it, so the fitted view is 1:1 and document point `(x, y)` is at canvas
    /// point `(x + 16, y + 16)`.
    pub const CANVAS: iced::Size = iced::Size::new(432.0, 332.0);

    pub fn editor() -> Editor {
        Editor::new(Image::filled(
            PhysicalSize::new(400, 300),
            Rgba8::from_rgb_hex(0xffffff),
        ))
    }

    /// The canvas point over document point `(x, y)` in the fitted view.
    pub fn at(x: f32, y: f32) -> Point {
        Point::new(x + canvas::MARGIN, y + canvas::MARGIN)
    }

    pub fn input(editor: &mut Editor, kind: InputKind) -> Option<Event> {
        editor.update(Message::Canvas(Input { size: CANVAS, kind }))
    }

    pub fn press(editor: &mut Editor, position: Point, clicks: u8) {
        input(editor, InputKind::Press { position, clicks });
    }

    pub fn drag(editor: &mut Editor, from: Point, to: Point) {
        press(editor, from, 1);
        input(
            editor,
            InputKind::Move {
                position: from + (to - from) * 0.5,
            },
        );
        input(editor, InputKind::Move { position: to });
        input(editor, InputKind::Release { position: to });
    }

    pub fn scroll(editor: &mut Editor, position: Point, delta: Vector) {
        input(editor, InputKind::Scroll { position, delta });
    }

    pub fn modifiers(editor: &mut Editor, modifiers: keyboard::Modifiers) {
        input(editor, InputKind::Modifiers(modifiers));
    }
}

#[cfg(test)]
mod tests {
    use iced::Vector;

    use super::testing::*;
    use super::*;
    use crate::model::{Line, Point, Shape};

    #[test]
    fn a_drag_on_the_canvas_adds_one_annotation_in_document_coordinates() {
        let mut editor = editor();
        drag(&mut editor, at(10.0, 20.0), at(110.0, 70.0));
        let [line] = editor.document().annotations() else {
            panic!("expected one annotation");
        };
        assert_eq!(
            line.shape,
            Shape::Line(Line {
                start: Point::new(10.0, 20.0),
                end: Point::new(110.0, 70.0),
            })
        );
        assert!(editor.document().can_undo());
    }

    #[test]
    fn zoom_and_pan_change_where_drags_land_and_the_drag_threshold() {
        let mut editor = editor();
        modifiers(&mut editor, keyboard::Modifiers::COMMAND);
        // Doubling the zoom around document point (0, 0) keeps it in place.
        scroll(
            &mut editor,
            at(0.0, 0.0),
            Vector::new(0.0, SCROLL_PER_DOUBLING),
        );
        modifiers(&mut editor, keyboard::Modifiers::default());
        assert_eq!(editor.zoom(), Zoom::Scale(2.0));
        scroll(&mut editor, at(0.0, 0.0), Vector::new(-40.0, -10.0));

        // Canvas (16, 16) is now document (20, 5); 1 document unit is 2 pixels.
        let origin = Point::new(20.0, 5.0);
        drag(&mut editor, at(0.0, 0.0), at(100.0, 0.0));
        let [line] = editor.document().annotations() else {
            panic!("expected one annotation");
        };
        assert_eq!(
            line.shape,
            Shape::Line(Line {
                start: origin,
                end: Point::new(70.0, 5.0),
            })
        );

        // 2 canvas pixels (under the 3-pixel threshold) is 1 document unit.
        drag(&mut editor, at(0.0, 50.0), at(2.0, 50.0));
        assert_eq!(editor.document().annotations().len(), 1);
    }

    #[test]
    fn shift_pressed_mid_drag_constrains_without_moving_the_pointer() {
        let mut editor = editor();
        press(&mut editor, at(0.0, 0.0), 1);
        input(
            &mut editor,
            InputKind::Move {
                position: at(100.0, 10.0),
            },
        );
        modifiers(&mut editor, keyboard::Modifiers::SHIFT);
        input(
            &mut editor,
            InputKind::Release {
                position: at(100.0, 10.0),
            },
        );
        let [line] = editor.document().annotations() else {
            panic!("expected one annotation");
        };
        assert_eq!(
            line.shape,
            Shape::Line(Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(100.0, 0.0),
            })
        );
    }
}

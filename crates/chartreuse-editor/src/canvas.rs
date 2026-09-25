//! The editor canvas: draws the base image, the annotations, and the active
//! tool's preview, and turns mouse and keyboard events into editor
//! [`Message`]s.
//!
//! # Layers
//!
//! Within one layer iced draws all meshes (strokes, fills) first, then images,
//! then text, so a single canvas could neither put a shape above text nor an
//! annotation above the base image. The canvas is therefore a stack of canvas
//! widgets drawn bottom to top, each in a renderer layer of its own clipped
//! to the canvas (so a zoomed-in image never spills over the widgets around
//! it):
//!
//! 1. the background and the base image;
//! 2. the annotations, split into runs in z-order such that a run's shapes
//!    all come before its text (see [`runs`]), one layer per run, so every
//!    annotation is drawn above the ones below it;
//! 3. the overlay: the active tool's preview and any selection chrome. This
//!    top layer is also the one that handles input.
//!
//! Annotations and previews are clipped to the image, as they are when
//! flattened; selection chrome is not.
//!
//! Shapes are drawn as described in [`render::shape`]; the zoom and pan are a
//! [`View`], mapped to canvas coordinates by a [`Viewport`].

mod render;
mod viewport;

use std::ops::Range;

use iced::advanced::mouse::{self, click, Interaction};
use iced::widget::canvas::{self as iced_canvas, Action, Event, Frame, Geometry, Program};
use iced::widget::image::FilterMethod;
use iced::widget::{space, stack, Canvas};
use iced::{keyboard, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector};
use smol_str::SmolStr;

use crate::editor::Message;
use crate::font;
use crate::model::{Annotation, AnnotationId, Shape};
use crate::tools::{Preview, TextTarget};
use crate::Editor;

pub use render::color;
pub use viewport::{View, Viewport, Zoom, MARGIN, MAX_SCALE, MIN_SCALE, ZOOM_STEP};

/// Pixels scrolled per line, for mice that scroll by lines.
const SCROLL_LINE: f32 = 40.0;

/// Input from the canvas widget, in canvas coordinates (logical pixels from
/// the canvas's top-left corner).
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// The canvas's size when the input happened.
    pub size: Size,
    pub kind: InputKind,
}

/// The kinds of canvas [`Input`].
#[derive(Debug, Clone, PartialEq)]
pub enum InputKind {
    /// The canvas changed size.
    Resized,
    /// The primary button went down over the canvas; `clicks` is 1 for a
    /// single click, 2 for a double click, and so on.
    Press { position: Point, clicks: u8 },
    /// The pointer moved while the button was down.
    Move { position: Point },
    /// The primary button came up after a press on the canvas.
    Release { position: Point },
    /// A scroll over the canvas, in pixels.
    Scroll { position: Point, delta: Vector },
    /// A key was pressed. `text` is what it types, if anything.
    Key {
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<SmolStr>,
    },
    /// The keyboard modifiers changed.
    Modifiers(keyboard::Modifiers),
}

/// The canvas: its layers stacked bottom to top, clipped to its bounds.
pub(crate) fn view(editor: &Editor) -> Element<'_, Message> {
    let layer = |layer| {
        Canvas::new(Scene { editor, layer })
            .width(Length::Fill)
            .height(Length::Fill)
    };
    let annotations = runs(editor.document().annotations())
        .into_iter()
        .map(|run| layer(Layer::Annotations(run)).into());
    // A stack draws its first child in the enclosing renderer layer and each
    // later one in a new layer clipped to the stack; the empty first child
    // puts every canvas in its own layer. The runs share one nested stack so
    // the overlay stays at the same index however many runs there are: iced
    // matches widget state to children by index, and the overlay's state is
    // its pointer tracking.
    stack![
        space().width(Length::Fill).height(Length::Fill),
        layer(Layer::Base),
        stack(annotations).width(Length::Fill).height(Length::Fill),
        layer(Layer::Overlay),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true)
    .into()
}

/// Splits annotations (bottom to top) into consecutive runs in which no shape
/// comes after text, so drawing each run as one layer (text last) keeps the
/// z-order.
#[must_use]
pub fn runs(annotations: &[Annotation]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    for (index, pair) in annotations.windows(2).enumerate() {
        let text_then_shape =
            matches!(pair[0].shape, Shape::Text(_)) && !matches!(pair[1].shape, Shape::Text(_));
        if text_then_shape {
            runs.push(start..index + 1);
            start = index + 1;
        }
    }
    if start < annotations.len() {
        runs.push(start..annotations.len());
    }
    runs
}

/// One layer of the canvas.
#[derive(Debug, Clone)]
enum Layer {
    Base,
    Annotations(Range<usize>),
    Overlay,
}

#[derive(Debug)]
struct Scene<'a> {
    editor: &'a Editor,
    layer: Layer,
}

/// The overlay layer's pointer tracking.
#[derive(Debug, Default)]
struct Tracking {
    /// Whether the primary button went down on the canvas and is still down.
    pressed: bool,
    last_click: Option<click::Click>,
}

impl Scene<'_> {
    /// The canvas input for `event`, if the editor cares about it.
    fn input(
        &self,
        tracking: &mut Tracking,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<InputKind> {
        let relative = || cursor.position_from(bounds.position());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                let press = click::Click::new(position, mouse::Button::Left, tracking.last_click);
                tracking.last_click = Some(press);
                tracking.pressed = true;
                let clicks = match press.kind() {
                    click::Kind::Single => 1,
                    click::Kind::Double => 2,
                    click::Kind::Triple => 3,
                };
                Some(InputKind::Press { position, clicks })
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if tracking.pressed => {
                Some(InputKind::Move {
                    position: relative()?,
                })
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if tracking.pressed => {
                tracking.pressed = false;
                Some(InputKind::Release {
                    position: relative()?,
                })
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let position = cursor.position_in(bounds)?;
                let delta = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => {
                        Vector::new(x * SCROLL_LINE, y * SCROLL_LINE)
                    }
                    mouse::ScrollDelta::Pixels { x, y } => Vector::new(x, y),
                };
                Some(InputKind::Scroll { position, delta })
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => Some(InputKind::Key {
                key: key.clone(),
                modifiers: *modifiers,
                text: text.clone(),
            }),
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(InputKind::Modifiers(*modifiers))
            }
            _ => (bounds.size() != self.editor.canvas_size()).then_some(InputKind::Resized),
        }
    }

    fn draw_base(&self, frame: &mut Frame, theme: &Theme) {
        let base = theme.extended_palette().background.base.color;
        let backdrop = Color::from_rgb(base.r * 0.6, base.g * 0.6, base.b * 0.6);
        frame.fill_rectangle(Point::ORIGIN, frame.size(), backdrop);

        let viewport = self.editor.viewport(frame.size());
        let image = iced_canvas::Image::new(self.editor.image().clone()).filter_method(
            if viewport.scale() >= 1.0 {
                FilterMethod::Nearest
            } else {
                FilterMethod::Linear
            },
        );
        frame.draw_image(
            viewport.to_canvas_rect(self.editor.document().bounds()),
            image,
        );
    }

    /// The annotation the active tool's preview hides: the text being edited.
    fn hidden(preview: &Preview<'_>) -> Option<AnnotationId> {
        match preview {
            Preview::Text(edit) => match edit.target() {
                TextTarget::Existing(id) => Some(id),
                TextTarget::New => None,
            },
            Preview::None | Preview::New(_) => None,
        }
    }

    fn draw_annotations(&self, frame: &mut Frame, range: Range<usize>) {
        let viewport = self.editor.viewport(frame.size());
        let clip = viewport.to_canvas_rect(self.editor.document().bounds());
        let hidden = Self::hidden(&self.editor.active_tool().preview());
        let annotations = &self.editor.document().annotations()[range];
        frame.with_clip(clip, |frame| {
            for annotation in annotations {
                if Some(annotation.id()) != hidden {
                    render::shape(frame, &viewport, &annotation.shape, &annotation.style);
                }
            }
        });
    }

    fn draw_overlay(&self, frame: &mut Frame, theme: &Theme) {
        let viewport = self.editor.viewport(frame.size());
        let clip = viewport.to_canvas_rect(self.editor.document().bounds());
        let accent = theme.palette().primary;
        match self.editor.active_tool().preview() {
            Preview::None => {}
            Preview::New(shape) => frame.with_clip(clip, |frame| {
                render::shape(frame, &viewport, &shape, &self.editor.style());
            }),
            Preview::Text(edit) => {
                let style = edit.style();
                let position = viewport.to_canvas(edit.position());
                frame.with_clip(clip, |frame| {
                    let text =
                        render::canvas_text(edit.content(), position, &style, viewport.scale());
                    frame.fill_text(text);
                });
                render::text_edit(
                    frame,
                    &viewport,
                    edit.position(),
                    font::measure(edit.content(), style.font_size),
                    font::caret(edit.content(), style.font_size),
                    &style,
                    accent,
                );
            }
        }
    }
}

impl Program<Message> for Scene<'_> {
    type State = Tracking;

    fn update(
        &self,
        tracking: &mut Tracking,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Layer::Overlay = self.layer else {
            return None;
        };
        let kind = self.input(tracking, event, bounds, cursor)?;
        let captures = !matches!(kind, InputKind::Resized | InputKind::Modifiers(_));
        let action = Action::publish(Message::Canvas(Input {
            size: bounds.size(),
            kind,
        }));
        Some(if captures {
            action.and_capture()
        } else {
            action
        })
    }

    fn draw(
        &self,
        _tracking: &Tracking,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        match &self.layer {
            Layer::Base => self.draw_base(&mut frame, theme),
            Layer::Annotations(range) => self.draw_annotations(&mut frame, range.clone()),
            Layer::Overlay => self.draw_overlay(&mut frame, theme),
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _tracking: &Tracking,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Interaction {
        let Layer::Overlay = self.layer else {
            return Interaction::None;
        };
        let Some(position) = cursor.position_in(bounds) else {
            return Interaction::None;
        };
        let viewport = self.editor.viewport(bounds.size());
        self.editor.active_tool().cursor(
            self.editor.document(),
            viewport.to_document(position),
            viewport.to_document_length(1.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_core::image::Image;

    use super::*;
    use crate::model::{Document, Line, Point as DocPoint, Style, Text};

    fn document(kinds: &str) -> Document {
        let mut document = Document::new(Image::filled(
            PhysicalSize::new(10, 10),
            Rgba8::from_rgb_hex(0),
        ));
        for kind in kinds.chars() {
            let shape = match kind {
                't' => Shape::Text(Text::new(DocPoint::ORIGIN, "t")),
                _ => Shape::Line(Line {
                    start: DocPoint::ORIGIN,
                    end: DocPoint::new(1.0, 1.0),
                }),
            };
            document.add(shape, Style::default());
        }
        document
    }

    #[test]
    fn runs_split_only_where_a_shape_follows_text() {
        assert_eq!(runs(document("").annotations()), Vec::<Range<usize>>::new());
        assert_eq!(runs(document("sstt").annotations()), vec![0..4]);
        assert_eq!(runs(document("tsts").annotations()), vec![0..1, 1..3, 3..4]);
        assert_eq!(runs(document("sttsst").annotations()), vec![0..3, 3..6]);
    }
}

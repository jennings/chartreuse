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
//! The base and annotation layers keep their geometry and redraw only when
//! what they show changes: the image, the view, the canvas size, the theme,
//! a run's annotations, or a preview of one of them. A pointer move in a
//! gesture that changes nothing else redraws only the overlay.
//!
//! The zoom and pan are a [`View`], mapped to canvas coordinates by a
//! [`Viewport`].
//!
//! # Drawing
//!
//! Everything is drawn in canvas pixels through the [`Viewport`]: document
//! point `p` is at `origin + p × scale`, and document lengths are multiplied
//! by `scale`. Per annotation, in the annotation's color:
//!
//! - Strokes (a line, an arrow's shaft, a rectangle's or ellipse's outline, a
//!   pen's path) are `stroke_width` wide, centered on the geometry, with
//!   round caps and round joins. A zero-length stroke is a disc
//!   `stroke_width` across.
//! - An arrow is its shaft stroked from `start` to [`ArrowHead::base`], then
//!   the head triangle `[tip, left, right]` filled (never stroked).
//! - A rectangle is the closed outline through [`Rect::corners`].
//! - An ellipse is the closed path of the Béziers of [`Ellipse::curves`];
//!   one of zero size is a dot.
//! - A pen stroke is the open path through its points; one whose points all
//!   coincide is a dot.
//! - Text is iced canvas text: shaped by cosmic-text and rasterized by the
//!   renderer's glyph cache, in [`font::FONT`], at `font_size` with a line
//!   height of `font_size × Text::LINE_HEIGHT` (both × `scale`), the layout
//!   box's top-left corner at the text's `position`, filled with the color
//!   (straight alpha). At `scale` 1 the layout is exactly [`font::layout`];
//!   see [`font`](crate::font#layout) for the baseline and fallback rules.
//!
//! The base image fills its rectangle, filtered nearest-neighbor at a scale
//! of 1 or more (so zoomed-in pixels stay crisp) and bilinearly below.
//!
//! [`ArrowHead::base`]: crate::model::ArrowHead::base
//! [`Rect::corners`]: crate::model::Rect::corners
//! [`Ellipse::curves`]: crate::model::Ellipse::curves
//! [`font::FONT`]: crate::font::FONT
//! [`font::layout`]: crate::font::layout

mod render;
mod viewport;

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;

use iced::advanced::image;
use iced::advanced::mouse::{self, click, Interaction};
use iced::widget::canvas::{self as iced_canvas, Action, Event, Frame, Geometry, Program};
use iced::widget::image::FilterMethod;
use iced::widget::{space, stack, Canvas};
use iced::{keyboard, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector};
use smol_str::SmolStr;

use crate::editor::Message;
use crate::model::{Annotation, Shape};
use crate::tools::{self, Preview, TextTarget};
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

/// A layer's widget state.
#[derive(Debug, Default)]
struct LayerState {
    /// The overlay's pointer tracking.
    tracking: Tracking,
    /// The base or annotation layer's geometry.
    drawing: Drawing,
}

/// A layer's geometry, kept while what the layer shows stays the same.
#[derive(Debug, Default)]
struct Drawing {
    cache: iced_canvas::Cache,
    /// What `cache` holds a drawing of, if anything.
    of: RefCell<Option<Content<'static>>>,
}

/// Everything a cached layer's drawing depends on besides the canvas size
/// (which the cache tracks itself).
#[derive(Debug, Clone, PartialEq)]
enum Content<'a> {
    Base {
        image: image::Id,
        viewport: Viewport,
        backdrop: Color,
    },
    Annotations {
        viewport: Viewport,
        annotations: Cow<'a, [Annotation]>,
    },
}

impl Content<'_> {
    fn into_owned(self) -> Content<'static> {
        match self {
            Content::Base {
                image,
                viewport,
                backdrop,
            } => Content::Base {
                image,
                viewport,
                backdrop,
            },
            Content::Annotations {
                viewport,
                annotations,
            } => Content::Annotations {
                viewport,
                annotations: Cow::Owned(annotations.into_owned()),
            },
        }
    }
}

impl Drawing {
    /// The layer's geometry at `size`: as drawn last time if that was of the
    /// same `content` at the same size, otherwise drawn by `draw`. `None`
    /// content (being previewed) is drawn afresh and not kept.
    fn draw(
        &self,
        renderer: &Renderer,
        size: Size,
        content: Option<Content<'_>>,
        draw: impl FnOnce(&mut Frame),
    ) -> Geometry {
        let mut drawn = self.of.borrow_mut();
        let Some(content) = content else {
            if drawn.take().is_some() {
                self.cache.clear();
            }
            let mut frame = Frame::new(renderer, size);
            draw(&mut frame);
            return frame.into_geometry();
        };
        if drawn.as_ref() != Some(&content) {
            self.cache.clear();
            *drawn = Some(content.into_owned());
        }
        self.cache.draw(renderer, size, draw)
    }
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

    fn draw_base(&self, frame: &mut Frame, viewport: &Viewport, backdrop: Color) {
        frame.fill_rectangle(Point::ORIGIN, frame.size(), backdrop);
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

    fn draw_annotations(
        &self,
        frame: &mut Frame,
        viewport: &Viewport,
        annotations: &[Annotation],
        preview: &Preview<'_>,
    ) {
        let clip = viewport.to_canvas_rect(self.editor.document().bounds());
        frame.with_clip(clip, |frame| {
            for annotation in annotations {
                if let Some(shape) = displayed(annotation, preview) {
                    render::shape(frame, viewport, &shape, &annotation.style);
                }
            }
        });
    }

    fn draw_overlay(&self, frame: &mut Frame, theme: &Theme) {
        let viewport = self.editor.viewport(frame.size());
        let clip = viewport.to_canvas_rect(self.editor.document().bounds());
        let accent = theme.palette().primary;
        let preview = self.editor.active_tool().preview();
        self.draw_selection(frame, &viewport, &preview, accent);
        match preview {
            Preview::None | Preview::Moved(..) | Preview::Reshaped(..) => {}
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
                    edit.size(),
                    edit.caret(),
                    &style,
                    accent,
                );
            }
        }
    }

    /// An outline around each selected annotation as displayed, plus the
    /// handles of a lone selection.
    fn draw_selection(
        &self,
        frame: &mut Frame,
        viewport: &Viewport,
        preview: &Preview<'_>,
        accent: Color,
    ) {
        let selected: Vec<_> = self
            .editor
            .document()
            .selected()
            .filter_map(|annotation| Some((annotation, displayed(annotation, preview)?)))
            .collect();
        for (annotation, shape) in &selected {
            render::selection_outline(frame, viewport, shape.bounds(&annotation.style), accent);
        }
        if let [(_, shape)] = selected.as_slice() {
            for (_, point) in tools::handles(shape) {
                render::handle(frame, viewport.to_canvas(point), accent);
            }
        }
    }
}

/// How `annotation` is displayed while `preview` is in progress: moved,
/// reshaped, or hidden (`None`, the text being edited).
fn displayed<'a>(annotation: &'a Annotation, preview: &Preview<'a>) -> Option<Cow<'a, Shape>> {
    let id = annotation.id();
    match *preview {
        Preview::Text(edit) if edit.target() == TextTarget::Existing(id) => None,
        Preview::Moved(ids, delta) if ids.contains(&id) => {
            let mut shape = annotation.shape.clone();
            shape.translate(delta);
            Some(Cow::Owned(shape))
        }
        Preview::Reshaped(target, shape) if target == id => Some(Cow::Borrowed(shape)),
        _ => Some(Cow::Borrowed(&annotation.shape)),
    }
}

/// Whether `preview` leaves `annotation` displayed as it is.
fn unchanged(annotation: &Annotation, preview: &Preview<'_>) -> bool {
    matches!(
        displayed(annotation, preview),
        Some(Cow::Borrowed(shape)) if std::ptr::eq(shape, &annotation.shape)
    )
}

/// The canvas's backdrop around the image: the theme's background, darkened.
fn backdrop(theme: &Theme) -> Color {
    let base = theme.extended_palette().background.base.color;
    Color::from_rgb(base.r * 0.6, base.g * 0.6, base.b * 0.6)
}

impl Program<Message> for Scene<'_> {
    type State = LayerState;

    fn update(
        &self,
        state: &mut LayerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Layer::Overlay = self.layer else {
            return None;
        };
        let kind = self.input(&mut state.tracking, event, bounds, cursor)?;
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
        state: &LayerState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let viewport = self.editor.viewport(size);
        let geometry = match &self.layer {
            Layer::Base => {
                let backdrop = backdrop(theme);
                let content = Content::Base {
                    image: self.editor.image().id(),
                    viewport,
                    backdrop,
                };
                state.drawing.draw(renderer, size, Some(content), |frame| {
                    self.draw_base(frame, &viewport, backdrop);
                })
            }
            Layer::Annotations(range) => {
                let annotations = &self.editor.document().annotations()[range.clone()];
                let preview = self.editor.active_tool().preview();
                let content = annotations
                    .iter()
                    .all(|annotation| unchanged(annotation, &preview))
                    .then_some(Content::Annotations {
                        viewport,
                        annotations: Cow::Borrowed(annotations),
                    });
                state.drawing.draw(renderer, size, content, |frame| {
                    self.draw_annotations(frame, &viewport, annotations, &preview);
                })
            }
            Layer::Overlay => {
                let mut frame = Frame::new(renderer, size);
                self.draw_overlay(&mut frame, theme);
                frame.into_geometry()
            }
        };
        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &LayerState,
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
    use iced::advanced::{clipboard, renderer};
    use iced::futures::executor::block_on;
    use iced_runtime::user_interface::{self, UserInterface};

    use super::*;
    use crate::editor::testing;
    use crate::model::{Arrow, Document, Line, Point as DocPoint, Style, Text};
    use crate::tools::ToolKind;

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

    #[test]
    fn previews_move_reshape_or_hide_only_their_targets() {
        let document = document("sst");
        let [a, b, t] = document.annotations() else {
            unreachable!()
        };
        let shown =
            |annotation, preview: &Preview<'_>| displayed(annotation, preview).map(Cow::into_owned);
        let delta = crate::model::Vector::new(5.0, 0.0);
        let mut moved = a.shape.clone();
        moved.translate(delta);
        let moving = Preview::Moved(&[a.id()][..], delta);
        assert_eq!(shown(a, &moving), Some(moved));
        assert_eq!(shown(b, &moving), Some(b.shape.clone()));
        assert!(!unchanged(a, &moving) && unchanged(b, &moving));

        let reshaped = Shape::Line(Line {
            start: DocPoint::ORIGIN,
            end: DocPoint::new(9.0, 9.0),
        });
        let reshaping = Preview::Reshaped(b.id(), &reshaped);
        assert_eq!(shown(b, &reshaping), Some(reshaped.clone()));
        assert_eq!(shown(a, &reshaping), Some(a.shape.clone()));
        assert!(!unchanged(b, &reshaping) && unchanged(a, &reshaping));

        let edit = tools::TextEdit::existing(&document, t.id()).unwrap();
        assert_eq!(shown(t, &Preview::Text(&edit)), None);
        assert_eq!(shown(a, &Preview::Text(&edit)), Some(a.shape.clone()));
        assert!(!unchanged(t, &Preview::Text(&edit)) && unchanged(a, &Preview::Text(&edit)));
    }

    fn headless_renderer() -> Renderer {
        block_on(<Renderer as renderer::Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("a tiny-skia renderer")
    }

    #[test]
    fn a_layer_is_redrawn_only_when_its_content_changes() {
        let renderer = headless_renderer();
        let document = document("st");
        let annotations = document.annotations();
        let size = Size::new(40.0, 40.0);
        let view = |canvas| View::default().viewport(canvas, document.bounds().size());
        let shows = |viewport, annotations| {
            Some(Content::Annotations {
                viewport,
                annotations: Cow::Borrowed(annotations),
            })
        };
        let drawing = Drawing::default();
        let draws = std::cell::Cell::new(0);
        let draw = |content| {
            let _ = drawing.draw(&renderer, size, content, |_| draws.set(draws.get() + 1));
            draws.get()
        };

        assert_eq!(draw(shows(view(size), annotations)), 1);
        assert_eq!(draw(shows(view(size), annotations)), 1, "unchanged: kept");
        assert_eq!(draw(shows(view(size), &annotations[..1])), 2);
        assert_eq!(
            draw(shows(view(Size::new(80.0, 80.0)), &annotations[..1])),
            3
        );
        assert_eq!(draw(None), 4);
        assert_eq!(draw(None), 5, "previewed: drawn every time");
        assert_eq!(
            draw(shows(view(Size::new(80.0, 80.0)), &annotations[..1])),
            6,
            "not kept across a preview"
        );
    }

    /// Drives the canvas's widget tree headlessly, as the iced runtime does:
    /// each batch of events goes to a tree built from the editor's current
    /// view, and the messages it publishes go back to the editor. Widget state
    /// carries over from one view to the next.
    struct Headless {
        renderer: Renderer,
        cache: Option<user_interface::Cache>,
    }

    impl Headless {
        fn new() -> Self {
            Self {
                renderer: headless_renderer(),
                cache: None,
            }
        }

        fn events(&mut self, editor: &mut Editor, cursor: Point, events: &[Event]) {
            let mut ui = UserInterface::build(
                view(editor),
                testing::CANVAS,
                self.cache.take().unwrap_or_default(),
                &mut self.renderer,
            );
            let mut messages = Vec::new();
            let _ = ui.update(
                events,
                mouse::Cursor::Available(cursor),
                &mut self.renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            self.cache = Some(ui.into_cache());
            for message in messages {
                editor.update(message);
            }
        }
    }

    #[test]
    fn a_drag_whose_press_changes_the_number_of_layers_still_moves() {
        use testing::{at, click, drag, input, named, press, type_text};

        let mut editor = testing::editor();
        editor.update(Message::Tool(ToolKind::Rectangle));
        drag(&mut editor, at(10.0, 10.0), at(100.0, 100.0));
        editor.update(Message::Tool(ToolKind::Text));
        click(&mut editor, at(200.0, 50.0));
        type_text(&mut editor, "hi");
        editor.update(Message::Tool(ToolKind::Arrow));
        drag(&mut editor, at(10.0, 200.0), at(100.0, 200.0));
        // Open the text and empty it, so committing the edit deletes it.
        editor.update(Message::Tool(ToolKind::Select));
        press(&mut editor, at(205.0, 50.0), 2);
        input(
            &mut editor,
            InputKind::Release {
                position: at(205.0, 50.0),
            },
        );
        named(&mut editor, keyboard::key::Named::Backspace);
        named(&mut editor, keyboard::key::Named::Backspace);
        assert_eq!(runs(editor.document().annotations()).len(), 2);

        // Pressing on the arrow commits the edit, which merges the two runs
        // into one layer, and starts dragging the arrow.
        let mut ui = Headless::new();
        let (from, to) = (at(50.0, 200.0), at(90.0, 230.0));
        let button = mouse::Button::Left;
        ui.events(
            &mut editor,
            from,
            &[Event::Mouse(mouse::Event::ButtonPressed(button))],
        );
        assert_eq!(runs(editor.document().annotations()).len(), 1);
        ui.events(
            &mut editor,
            to,
            &[Event::Mouse(mouse::Event::CursorMoved { position: to })],
        );
        ui.events(
            &mut editor,
            to,
            &[Event::Mouse(mouse::Event::ButtonReleased(button))],
        );

        let [_, arrow] = editor.document().annotations() else {
            panic!("expected the rectangle and the arrow");
        };
        assert_eq!(
            arrow.shape,
            Shape::Arrow(Arrow {
                start: DocPoint::new(50.0, 230.0),
                end: DocPoint::new(140.0, 230.0),
            })
        );
    }
}

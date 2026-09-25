//! The per-display `Canvas` program and its coordinate mapping.

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use chartreuse_core::image::Image;
use iced::advanced::text::Alignment;
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Action, Canvas, Event, Frame, Geometry, Path, Program, Stroke};
use iced::widget::image::Handle;
use iced::widget::{image, stack};
use iced::{
    keyboard, mouse, Color, ContentFit, Element, Fill, Point, Rectangle, Renderer, Size, Theme,
    Vector,
};

use super::selection::{output_grid, Input, Selection};

/// The translucent shade over everything outside the selection.
pub const DIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.45);
/// The width of the selection border, in canvas units, drawn just outside the
/// selected area so it never covers selected content.
const BORDER_WIDTH: f32 = 2.0;
/// The size label's text size, padding, and distance from the selection, in
/// canvas units.
const LABEL_TEXT_SIZE: f32 = 13.0;
const LABEL_PADDING: f32 = 5.0;
const LABEL_GAP: f32 = 6.0;
/// The label's text color; it sits on an accent-colored pill, and both flavor
/// accents are bright.
const LABEL_TEXT: Color = Color::BLACK;

/// Builds the iced image handle for one display's frozen capture, drawn by
/// [`RectangleOverlay`]. Consumes the image without copying its pixels; clone the
/// capture first if it is also needed for cropping.
#[must_use]
pub fn frozen_image(image: Image) -> Handle {
    let size = image.size();
    Handle::from_rgba(size.width, size.height, image.into_pixels())
}

/// Maps between one display's canvas and global logical desktop coordinates.
///
/// The canvas is stretched over the display's logical bounds. In an overlay
/// window, which is placed on the display and sized to it, one canvas unit is
/// one logical point and the window origin is the display's logical origin; a
/// smaller window (the development harness) scales uniformly.
///
/// Positions come in two frames: **window** positions, as reported by mouse
/// events and [`mouse::Cursor`] (relative to the window, so they may lie outside
/// the canvas bounds or the window itself during a drag), and **frame**
/// positions, relative to the canvas's top-left corner, used for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    display: LogicalRect,
    canvas: Rectangle,
}

impl Projection {
    /// The projection for `display` drawn into a canvas occupying `canvas` (its
    /// layout bounds in window coordinates).
    #[must_use]
    pub const fn new(display: &DisplayInfo, canvas: Rectangle) -> Self {
        Self {
            display: display.logical_bounds,
            canvas,
        }
    }

    /// Canvas units per logical point along each axis (1 if either extent is
    /// zero, so conversions stay finite).
    fn scale(&self) -> (f64, f64) {
        let ratio = |canvas: f32, logical: f64| {
            if canvas > 0.0 && logical > 0.0 {
                f64::from(canvas) / logical
            } else {
                1.0
            }
        };
        (
            ratio(self.canvas.width, self.display.size.width),
            ratio(self.canvas.height, self.display.size.height),
        )
    }

    /// Converts a window position to global logical coordinates.
    #[must_use]
    pub fn to_global(&self, window: Point) -> LogicalPoint {
        let (sx, sy) = self.scale();
        LogicalPoint::new(
            self.display.origin.x + f64::from(window.x - self.canvas.x) / sx,
            self.display.origin.y + f64::from(window.y - self.canvas.y) / sy,
        )
    }

    /// Converts a global logical point to a frame position.
    #[must_use]
    pub fn to_frame(&self, point: LogicalPoint) -> Point {
        let (sx, sy) = self.scale();
        Point::new(
            ((point.x - self.display.origin.x) * sx) as f32,
            ((point.y - self.display.origin.y) * sy) as f32,
        )
    }

    /// Converts a global logical rectangle to frame coordinates (not clipped to
    /// the canvas).
    #[must_use]
    pub fn rect_to_frame(&self, rect: &LogicalRect) -> Rectangle {
        let (sx, sy) = self.scale();
        Rectangle::new(
            self.to_frame(rect.origin),
            Size::new(
                (rect.size.width * sx) as f32,
                (rect.size.height * sy) as f32,
            ),
        )
    }

    /// Converts a size in canvas units to logical points.
    #[must_use]
    pub fn size_to_logical(&self, size: Size) -> LogicalSize {
        let (sx, sy) = self.scale();
        LogicalSize::new(f64::from(size.width) / sx, f64::from(size.height) / sy)
    }
}

/// The rectangle-selection overlay for one display: the display's frozen
/// capture, a translucent dim over everything but the selection, the selection
/// border in the accent color, and a label with the output size in pixels.
///
/// Build one per overlay window from the shared [`Selection`] in each `view` and
/// show it with [`RectangleOverlay::view`]. It turns pointer and Escape events
/// into [`Input`] in global logical coordinates and publishes them through
/// `on_input`; route them all to the one [`Selection::apply`]. The window that
/// receives the press keeps receiving the drag (the OS grabs the pointer), even
/// over other displays: its positions simply map outside its own display.
///
/// Only the primary (left) button selects. Escape is heard by the focused
/// overlay window, so give an overlay focus when opening them.
///
/// As a [`Program`] it draws only what goes over the capture: the renderer draws
/// a layer's images above its shapes, so [`RectangleOverlay::view`] puts the
/// capture in a layer of its own underneath the canvas.
pub struct RectangleOverlay<'a, F> {
    selection: &'a Selection,
    display: &'a DisplayInfo,
    image: &'a Handle,
    accent: Color,
    size_label: bool,
    on_input: F,
}

impl<F> std::fmt::Debug for RectangleOverlay<'_, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RectangleOverlay")
            .field("selection", self.selection)
            .field("display", &self.display.id)
            .field("accent", &self.accent)
            .field("size_label", &self.size_label)
            .finish_non_exhaustive()
    }
}

impl<'a, F> RectangleOverlay<'a, F> {
    /// The overlay for `display`, showing `image` (its capture, from
    /// [`frozen_image`]) under `selection`, with the size label shown.
    pub const fn new(
        selection: &'a Selection,
        display: &'a DisplayInfo,
        image: &'a Handle,
        accent: Color,
        on_input: F,
    ) -> Self {
        Self {
            selection,
            display,
            image,
            accent,
            size_label: true,
            on_input,
        }
    }

    /// Shows or hides the output-size label.
    #[must_use]
    pub const fn size_label(mut self, show: bool) -> Self {
        self.size_label = show;
        self
    }

    /// The frozen capture stretched over the whole window, with the selection
    /// canvas on top.
    pub fn view<Message>(self) -> Element<'a, Message>
    where
        F: Fn(Input) -> Message + 'a,
        Message: 'a,
    {
        let capture = image(self.image)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill);
        stack![capture, Canvas::new(self).width(Fill).height(Fill)].into()
    }
}

/// Per-canvas pointer memory (the [`Program::State`] of [`RectangleOverlay`]).
#[derive(Debug, Default)]
pub struct PointerState {
    /// The last window position the pointer was seen at.
    last: Option<Point>,
    /// True between a press on this canvas and the matching release. Kept
    /// locally because a release can arrive in the same event batch as its
    /// press, before the app's [`Selection`] (and this program) is rebuilt.
    pressed: bool,
}

impl<Message, F> Program<Message> for RectangleOverlay<'_, F>
where
    F: Fn(Input) -> Message,
{
    type State = PointerState;

    fn update(
        &self,
        state: &mut PointerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let projection = Projection::new(self.display, bounds);
        let input = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position().filter(|p| bounds.contains(*p))?;
                state.last = Some(position);
                state.pressed = true;
                Input::Press(projection.to_global(position))
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                state.last = Some(*position);
                if !(state.pressed || self.selection.is_active()) {
                    return None;
                }
                Input::Move(projection.to_global(*position))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !(std::mem::take(&mut state.pressed) || self.selection.is_active()) {
                    return None;
                }
                let point = cursor
                    .position()
                    .or(state.last)
                    .map(|position| projection.to_global(position))
                    .or_else(|| self.selection.pointer())?;
                Input::Release(point)
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                state.pressed = false;
                Input::Escape
            }
            _ => return None,
        };
        Some(Action::publish((self.on_input)(input)).and_capture())
    }

    fn draw(
        &self,
        _state: &PointerState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let projection = Projection::new(self.display, bounds);
        let area = Rectangle::with_size(bounds.size());
        let mut frame = Frame::new(renderer, bounds.size());

        let rect = self.selection.rect();
        let hole = rect.map(|rect| projection.rect_to_frame(&rect));
        for region in dim_regions(area, hole) {
            if region.width > 0.0 && region.height > 0.0 {
                frame.fill_rectangle(region.position(), region.size(), DIM);
            }
        }
        if let Some(hole) = hole {
            let inset = BORDER_WIDTH / 2.0;
            frame.stroke_rectangle(
                hole.position() - Vector::new(inset, inset),
                hole.size().expand(Size::new(BORDER_WIDTH, BORDER_WIDTH)),
                Stroke::default()
                    .with_color(self.accent)
                    .with_width(BORDER_WIDTH),
            );
        }
        if self.size_label
            && let Some(rect) = rect
        {
            self.draw_size_label(&mut frame, &projection, &rect);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &PointerState,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}

impl<F> RectangleOverlay<'_, F> {
    /// Draws "W × H" (output pixels) by the selection's bottom-right corner.
    ///
    /// The label is placed in global coordinates, on the display holding that
    /// corner, so every display's canvas draws its part of the same label.
    fn draw_size_label(&self, frame: &mut Frame, projection: &Projection, rect: &LogicalRect) {
        let layout = self.selection.layout();
        let Some(grid) = output_grid(layout, rect) else {
            return;
        };
        let size = grid.pixel_size();
        let content = format!("{} × {}", size.width, size.height);
        // An estimate: iced's canvas has no text measurement, and digits are
        // about 0.6 em wide in the default fonts.
        let text_width = content.chars().count() as f32 * LABEL_TEXT_SIZE * 0.6;
        let pill = Size::new(
            text_width + 2.0 * LABEL_PADDING,
            LABEL_TEXT_SIZE * 1.3 + 2.0 * LABEL_PADDING,
        );
        let corner = LogicalPoint::new(rect.max_x(), rect.max_y());
        let area = layout.nearest_display(corner).logical_bounds;
        let gap = projection
            .size_to_logical(Size::new(LABEL_GAP, LABEL_GAP))
            .height;
        let origin = label_origin(rect, projection.size_to_logical(pill), &area, gap);
        let top_left = projection.to_frame(origin);

        frame.fill(
            &Path::rounded_rectangle(top_left, pill, (pill.height / 2.0).into()),
            self.accent,
        );
        frame.fill_text(canvas::Text {
            content,
            position: top_left + Vector::new(pill.width / 2.0, pill.height / 2.0),
            color: LABEL_TEXT,
            size: LABEL_TEXT_SIZE.into(),
            align_x: Alignment::Center,
            align_y: Vertical::Center,
            ..canvas::Text::default()
        });
    }
}

/// The parts of `area` outside `hole` (clipped to `area`): top, bottom, left and
/// right bands. Bands with nothing to cover are empty.
fn dim_regions(area: Rectangle, hole: Option<Rectangle>) -> [Rectangle; 4] {
    let empty = Rectangle::default();
    let Some(hole) = hole.and_then(|hole| hole.intersection(&area)) else {
        return [area, empty, empty, empty];
    };
    let (right, bottom) = (area.x + area.width, area.y + area.height);
    let (hole_right, hole_bottom) = (hole.x + hole.width, hole.y + hole.height);
    [
        Rectangle::new(area.position(), Size::new(area.width, hole.y - area.y)),
        Rectangle::new(
            Point::new(area.x, hole_bottom),
            Size::new(area.width, bottom - hole_bottom),
        ),
        Rectangle::new(
            Point::new(area.x, hole.y),
            Size::new(hole.x - area.x, hole.height),
        ),
        Rectangle::new(
            Point::new(hole_right, hole.y),
            Size::new(right - hole_right, hole.height),
        ),
    ]
}

/// Where to put a label of `label` size for `selection`, within `area` (the
/// display under the selection's bottom-right corner): right-aligned with the
/// selection, `gap` below it, or inside its bottom edge when there is no room
/// below, and always kept on `area`.
fn label_origin(
    selection: &LogicalRect,
    label: LogicalSize,
    area: &LogicalRect,
    gap: f64,
) -> LogicalPoint {
    let fit = |value: f64, min: f64, max: f64| value.min(max).max(min);
    let x = fit(
        selection.max_x() - label.width,
        area.min_x(),
        area.max_x() - label.width,
    );
    let below = selection.max_y() + gap;
    let y = if below + label.height <= area.max_y() {
        below
    } else {
        selection.max_y() - gap - label.height
    };
    let y = fit(y, area.min_y(), area.max_y() - label.height);
    LogicalPoint::new(x, y)
}

#[cfg(test)]
mod tests {
    use chartreuse_core::display::{DisplayId, DisplayLayout};
    use chartreuse_platform::fake::default_displays;
    use iced::keyboard::key::{Named, Physical};
    use iced::keyboard::{Location, Modifiers};

    use super::*;
    use crate::rectangle::Phase;

    fn layout() -> DisplayLayout {
        DisplayLayout::new(default_displays()).expect("the fake displays form a layout")
    }

    fn display(id: u64) -> DisplayInfo {
        layout()
            .get(DisplayId(id))
            .cloned()
            .expect("a fake display")
    }

    /// A canvas filling a window sized to the display, as in an overlay.
    fn overlay_bounds(display: &DisplayInfo) -> Rectangle {
        let size = display.logical_bounds.size;
        Rectangle::new(
            Point::ORIGIN,
            Size::new(size.width as f32, size.height as f32),
        )
    }

    fn pt(x: f64, y: f64) -> LogicalPoint {
        LogicalPoint::new(x, y)
    }

    #[test]
    fn window_positions_map_to_global_coordinates_per_display() {
        // Primary 2× at the origin, external 1× at (-1920, -400), portrait 1.5×
        // at (1512, 100). The scale factor plays no part: windows are sized in
        // logical points.
        for (id, window, global) in [
            (1, Point::new(10.0, 20.0), pt(10.0, 20.0)),
            (2, Point::new(0.0, 0.0), pt(-1920.0, -400.0)),
            (2, Point::new(1919.5, 400.0), pt(-0.5, 0.0)),
            (3, Point::new(0.0, 0.0), pt(1512.0, 100.0)),
            (3, Point::new(88.5, 1000.25), pt(1600.5, 1100.25)),
        ] {
            let display = display(id);
            let projection = Projection::new(&display, overlay_bounds(&display));
            assert_eq!(projection.to_global(window), global, "display {id}");
            assert_eq!(projection.to_frame(global), window, "display {id}");
        }
    }

    #[test]
    fn positions_outside_the_window_map_onto_neighbouring_displays() {
        // A drag that starts on the external display and continues to the right
        // of its window lands on the primary display.
        let external = display(2);
        let projection = Projection::new(&external, overlay_bounds(&external));
        let global = projection.to_global(Point::new(2020.0, 450.0));
        assert_eq!(global, pt(100.0, 50.0));
        assert_eq!(
            layout().display_at(global).map(|d| d.id),
            Some(DisplayId(1))
        );
        // ...and above it, at negative window coordinates.
        assert_eq!(
            projection.to_global(Point::new(-80.0, -50.0)),
            pt(-2000.0, -450.0)
        );
    }

    #[test]
    fn a_scaled_or_offset_canvas_is_stretched_over_the_display() {
        // The harness shows the portrait display at a quarter size, and a canvas
        // need not start at the window origin.
        let portrait = display(3);
        let bounds = Rectangle::new(Point::new(10.0, 30.0), Size::new(200.0, 320.0));
        let projection = Projection::new(&portrait, bounds);
        assert_eq!(
            projection.to_global(Point::new(10.0, 30.0)),
            pt(1512.0, 100.0)
        );
        assert_eq!(
            projection.to_global(Point::new(60.0, 55.0)),
            pt(1712.0, 200.0)
        );
        assert_eq!(
            projection.rect_to_frame(&LogicalRect::new(1612.0, 500.0, 400.0, 80.0)),
            Rectangle::new(Point::new(25.0, 100.0), Size::new(100.0, 20.0))
        );
        assert_eq!(
            projection.size_to_logical(Size::new(10.0, 5.0)),
            LogicalSize::new(40.0, 20.0)
        );
    }

    #[test]
    fn a_selection_across_displays_projects_onto_each_canvas() {
        // From the external display into the primary: each canvas sees its slice,
        // extending past its edge.
        let rect = LogicalRect::new(-300.0, -100.0, 700.0, 400.0);
        let external = display(2);
        let on_external =
            Projection::new(&external, overlay_bounds(&external)).rect_to_frame(&rect);
        assert_eq!(
            on_external,
            Rectangle::new(Point::new(1620.0, 300.0), Size::new(700.0, 400.0))
        );
        let primary = display(1);
        let on_primary = Projection::new(&primary, overlay_bounds(&primary)).rect_to_frame(&rect);
        assert_eq!(
            on_primary,
            Rectangle::new(Point::new(-300.0, -100.0), Size::new(700.0, 400.0))
        );
    }

    #[test]
    fn dim_covers_everything_but_the_visible_part_of_the_selection() {
        let area = Rectangle::new(Point::ORIGIN, Size::new(100.0, 80.0));
        assert_eq!(dim_regions(area, None)[0], area);

        // A hole running off the right edge of the canvas.
        let hole = Rectangle::new(Point::new(60.0, 20.0), Size::new(100.0, 30.0));
        let regions = dim_regions(area, Some(hole));
        let covered: f32 = regions.iter().map(|r| r.width * r.height).sum();
        assert_eq!(covered, 100.0 * 80.0 - 40.0 * 30.0);
        assert_eq!(regions[3].width, 0.0, "nothing to dim right of the hole");
        for region in regions {
            assert!(
                region.intersection(&hole).is_none(),
                "{region:?} dims the hole"
            );
        }

        // A hole entirely on another display dims the whole canvas.
        let elsewhere = Rectangle::new(Point::new(-500.0, 0.0), Size::new(50.0, 50.0));
        assert_eq!(dim_regions(area, Some(elsewhere))[0], area);
    }

    #[test]
    fn the_label_sits_below_the_selection_and_stays_on_its_display() {
        let area = LogicalRect::new(0.0, 0.0, 1000.0, 800.0);
        let label = LogicalSize::new(100.0, 20.0);
        // Room below: right-aligned, under the bottom edge.
        let rect = LogicalRect::new(200.0, 100.0, 300.0, 200.0);
        assert_eq!(label_origin(&rect, label, &area, 6.0), pt(400.0, 306.0));
        // At the bottom of the display: inside the selection's bottom edge.
        let rect = LogicalRect::new(200.0, 600.0, 300.0, 200.0);
        assert_eq!(label_origin(&rect, label, &area, 6.0), pt(400.0, 774.0));
        // Narrower than the label at the left edge: pinned to the display.
        let rect = LogicalRect::new(0.0, 100.0, 40.0, 40.0);
        assert_eq!(label_origin(&rect, label, &area, 6.0), pt(0.0, 146.0));
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Received(Input);

    /// Runs `event` through the overlay for `display_id`, returning the published
    /// input.
    fn send(
        selection: &Selection,
        display_id: u64,
        state: &mut PointerState,
        event: &Event,
        cursor: Option<Point>,
    ) -> Option<Input> {
        let display = display(display_id);
        let handle = Handle::from_rgba(1, 1, vec![0; 4]);
        let overlay = RectangleOverlay::new(selection, &display, &handle, Color::WHITE, Received);
        let cursor = cursor.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
        let action =
            Program::<Received>::update(&overlay, state, event, overlay_bounds(&display), cursor)?;
        let (message, _, status) = action.into_inner();
        assert_eq!(status, iced::event::Status::Captured);
        message.map(|Received(input)| input)
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    fn moved(x: f32, y: f32) -> Event {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, y),
        })
    }

    #[test]
    fn a_drag_from_one_window_drives_the_shared_selection_across_displays() {
        // Press on the external display, drag right past its window onto the
        // primary, release there. Every event reaches the external window only.
        let mut selection = Selection::new(layout());
        let mut state = PointerState::default();
        let press_at = Point::new(1800.0, 300.0);
        let input = send(&selection, 2, &mut state, &press(), Some(press_at));
        assert_eq!(input, Some(Input::Press(pt(-120.0, -100.0))));
        selection.apply(input.expect("pressed"));

        let input = send(&selection, 2, &mut state, &moved(2120.0, 700.0), None);
        assert_eq!(input, Some(Input::Move(pt(200.0, 300.0))));
        selection.apply(input.expect("moved"));

        let release_at = Point::new(2120.0, 700.0);
        let input = send(&selection, 2, &mut state, &release(), Some(release_at));
        assert_eq!(input, Some(Input::Release(pt(200.0, 300.0))));
        selection.apply(input.expect("released"));
        assert_eq!(
            selection.phase(),
            Phase::Committed(LogicalRect::new(-120.0, -100.0, 320.0, 400.0))
        );
    }

    #[test]
    fn a_release_in_the_same_batch_as_its_press_is_not_lost() {
        // The selection is still idle (the app has not seen the press yet), but
        // this canvas saw the press, so it reports the release.
        let selection = Selection::new(layout());
        let mut state = PointerState::default();
        let at = Some(Point::new(50.0, 60.0));
        assert!(send(&selection, 1, &mut state, &press(), at).is_some());
        assert_eq!(
            send(&selection, 1, &mut state, &release(), at),
            Some(Input::Release(pt(50.0, 60.0)))
        );
    }

    #[test]
    fn a_release_without_a_cursor_uses_the_last_seen_position() {
        let mut selection = Selection::new(layout());
        let mut state = PointerState::default();
        let at = Some(Point::new(50.0, 60.0));
        selection.apply(send(&selection, 1, &mut state, &press(), at).expect("pressed"));
        let _ = send(&selection, 1, &mut state, &moved(400.0, 300.0), None);
        assert_eq!(
            send(&selection, 1, &mut state, &release(), None),
            Some(Input::Release(pt(400.0, 300.0)))
        );
    }

    #[test]
    fn idle_hover_and_presses_elsewhere_publish_nothing() {
        let selection = Selection::new(layout());
        let mut state = PointerState::default();
        assert_eq!(
            send(&selection, 1, &mut state, &moved(10.0, 10.0), None),
            None
        );
        // A press outside this canvas belongs to another window.
        let outside = Some(Point::new(-5.0, 10.0));
        assert_eq!(send(&selection, 1, &mut state, &press(), outside), None);
        assert_eq!(send(&selection, 1, &mut state, &release(), outside), None);
        let right = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        assert_eq!(
            send(
                &selection,
                1,
                &mut state,
                &right,
                Some(Point::new(5.0, 5.0))
            ),
            None
        );
    }

    #[test]
    fn escape_is_published() {
        let selection = Selection::new(layout());
        let mut state = PointerState::default();
        let escape = Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            modified_key: keyboard::Key::Named(Named::Escape),
            physical_key: Physical::Code(keyboard::key::Code::Escape),
            location: Location::Standard,
            modifiers: Modifiers::default(),
            text: None,
            repeat: false,
        });
        assert_eq!(
            send(&selection, 3, &mut state, &escape, None),
            Some(Input::Escape)
        );
    }
}

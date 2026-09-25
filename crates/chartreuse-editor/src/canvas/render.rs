//! Drawing annotations onto a canvas frame, following the model's stroke
//! geometry (see [`Style`]) and the text layout in [`font`].

use chartreuse_core::color::Rgba8;
use iced::advanced::text::{LineHeight, Shaping};
use iced::widget::canvas::{self, Frame, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Pixels, Point as CanvasPoint};

use super::Viewport;
use crate::font;
use crate::model::{Shape, Style, Text};

/// Converts a model color (straight alpha) to an iced color.
#[must_use]
pub fn color(color: Rgba8) -> Color {
    Color::from_rgba8(color.r, color.g, color.b, f32::from(color.a) / 255.0)
}

/// Draws `shape` in `style`, mapped onto the canvas by `viewport`.
///
/// - Strokes are `stroke_width` wide (scaled with the zoom), with round caps
///   and joins; a zero-length stroke is a round dot `stroke_width` across.
/// - An arrow is its shaft stroked from `start` to the head's base, then the
///   head triangle filled.
/// - Text is filled with iced's canvas text (cosmic-text, via the renderer's
///   glyph cache) in [`font::FONT`] at `font_size` × zoom, with a line height
///   of `font_size × Text::LINE_HEIGHT` × zoom and the layout box's top-left
///   corner at the text's position; see [`font`](crate::font#layout).
pub fn shape(frame: &mut Frame, viewport: &Viewport, shape: &Shape, style: &Style) {
    let paint = color(style.color);
    let width = style.stroke_width.max(0.0) * viewport.scale();
    match shape {
        Shape::Line(line) => stroke_segment(
            frame,
            viewport.to_canvas(line.start),
            viewport.to_canvas(line.end),
            width,
            paint,
        ),
        Shape::Arrow(arrow) => {
            let start = viewport.to_canvas(arrow.start);
            match arrow.head(style.stroke_width) {
                Some(head) => {
                    stroke_segment(frame, start, viewport.to_canvas(head.base), width, paint);
                    let [tip, left, right] = head.corners().map(|p| viewport.to_canvas(p));
                    let triangle = Path::new(|path| {
                        path.move_to(tip);
                        path.line_to(left);
                        path.line_to(right);
                        path.close();
                    });
                    frame.fill(&triangle, paint);
                }
                None => dot(frame, start, width, paint),
            }
        }
        Shape::Rectangle(rectangle) => {
            let [a, b, c, d] = rectangle.rect.corners().map(|p| viewport.to_canvas(p));
            if a == c {
                dot(frame, a, width, paint);
            } else {
                let outline = Path::new(|path| {
                    path.move_to(a);
                    path.line_to(b);
                    path.line_to(c);
                    path.line_to(d);
                    path.close();
                });
                frame.stroke(&outline, stroke(width, paint));
            }
        }
        Shape::Text(text) => frame.fill_text(canvas_text(
            &text.content,
            viewport.to_canvas(text.position),
            style,
            viewport.scale(),
        )),
    }
}

/// Annotation text as canvas text at canvas position `position`, `scale`
/// canvas pixels per document unit.
pub fn canvas_text(
    content: &str,
    position: CanvasPoint,
    style: &Style,
    scale: f32,
) -> canvas::Text {
    let size = style.font_size.max(0.0) * scale;
    canvas::Text {
        content: content.to_owned(),
        position,
        color: color(style.color),
        size: Pixels(size),
        line_height: LineHeight::Absolute(Pixels(size * Text::LINE_HEIGHT)),
        font: font::FONT,
        shaping: Shaping::Advanced,
        ..canvas::Text::default()
    }
}

fn stroke(width: f32, paint: Color) -> Stroke<'static> {
    Stroke::default()
        .with_width(width)
        .with_color(paint)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round)
}

fn stroke_segment(frame: &mut Frame, a: CanvasPoint, b: CanvasPoint, width: f32, paint: Color) {
    if a == b {
        dot(frame, a, width, paint);
    } else {
        frame.stroke(&Path::line(a, b), stroke(width, paint));
    }
}

/// A zero-length stroke: a disc `width` across.
fn dot(frame: &mut Frame, center: CanvasPoint, width: f32, paint: Color) {
    if width > 0.0 {
        frame.fill(&Path::circle(center, width / 2.0), paint);
    }
}

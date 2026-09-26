//! Rendering a document into pixels for export, matching the canvas. Owned by
//! track 2F.
//!
//! [`flatten`] draws a [`Document`]'s annotations over a copy of its base
//! image, bottom to top, at the base image's resolution: one document unit is
//! one output pixel, so the result is what the [canvas](crate::canvas#drawing)
//! shows at a scale of 1, clipped to the image.
//!
//! # Rasterizer
//!
//! Shapes are rasterized with [tiny-skia]: anti-aliased, analytic coverage,
//! with real round caps and joins. It is the library iced's software renderer
//! draws canvas geometry with, so the tiny-skia canvas and flatten rasterize
//! the same paths with the same code and agree to within rounding, and iced
//! already builds it, so it adds nothing to the build. Text glyphs come from
//! cosmic-text's [`SwashCache`], the glyph rasterizer behind both of iced's
//! renderers.
//!
//! # Drawing
//!
//! Per annotation, in its [`Style`]'s color (straight alpha), exactly as the
//! canvas docs describe:
//!
//! - Strokes (a line, an arrow's shaft, a rectangle's or ellipse's outline, a
//!   pen's path) are `stroke_width` wide, centered on the geometry, with
//!   round caps and round joins. A zero-length stroke is a disc
//!   `stroke_width` across; a stroke width of zero (or less) draws nothing.
//! - An arrow is its shaft stroked from `start` to [`ArrowHead::base`], then
//!   the head triangle `[tip, left, right]` filled, never stroked.
//! - A rectangle is the closed outline through [`Rect::corners`].
//! - An ellipse is the closed path of the Béziers of [`Ellipse::curves`];
//!   one of zero size is a dot.
//! - A pen stroke is the open path through its points; one whose points all
//!   coincide is a dot.
//! - A highlighter stroke is the same path, [`highlighter::width`] wide,
//!   drawn opaque into a layer of its own (`highlighter_layer`) that is
//!   composited at [`highlighter::alpha`], so it never darkens where it
//!   overlaps itself (see [`highlighter`]).
//! - A step marker is a disc [`StepMarker::radius`] in radius filled in its
//!   color, then its number ([`Document::step_number`]) drawn as text would
//!   be, in [`StepMarker::number_color`], at the layout box position
//!   [`StepMarker::label_origin`] gives for the number's [`font::measure`].
//! - Text is laid out by [`font::layout`] and each glyph rasterized by swash,
//!   placed as iced places canvas text: the glyph's pixel origin is
//!   [`LayoutGlyph::physical`] with the text's position as the offset, moved
//!   down to the line's baseline (`LayoutRun::line_y`, rounded) and by the
//!   glyph image's placement. Mask glyphs are coverage × the color; color
//!   glyphs (emoji from a system fallback font) keep their own colors, with
//!   the style's alpha as their opacity, as in iced.
//!
//! # Compositing
//!
//! Annotations are drawn with source-over blending into a transparent,
//! premultiplied layer the size of the image, which is then composited
//! source-over onto the straight-alpha base image in floating point. Pixels no
//! annotation touches keep their exact bytes, and translucent base pixels
//! (from a pasted or opened image) are blended correctly rather than
//! round-tripped through premultiplied 8-bit color.
//!
//! # Adding a kind (3B)
//!
//! A new [`Shape`] variant needs one arm in the private `Flattener::draw`,
//! built from its helpers:
//!
//! - blur and pixelate regions act on everything below them: call
//!   `Flattener::flush` to composite the layer so far onto the image, then
//!   run the `chartreuse_imaging` kernel on `Flattener::image`;
//! - the document-level crop applies last, to the finished image
//!   (`chartreuse_imaging::crop`).
//!
//! [`Style`]: crate::model::Style
//! [tiny-skia]: https://docs.rs/tiny-skia/0.11
//! [`SwashCache`]: iced::advanced::graphics::text::cosmic_text::SwashCache
//! [`LayoutGlyph::physical`]: iced::advanced::graphics::text::cosmic_text::LayoutGlyph::physical
//! [`ArrowHead::base`]: crate::model::ArrowHead::base
//! [`Rect::corners`]: crate::model::Rect::corners
//! [`Ellipse::curves`]: crate::model::Ellipse::curves
//! [`font::layout`]: crate::font::layout
//! [`font::measure`]: crate::font::measure
//! [`StepMarker::radius`]: crate::model::StepMarker::radius
//! [`StepMarker::number_color`]: crate::model::StepMarker::number_color
//! [`StepMarker::label_origin`]: crate::model::StepMarker::label_origin
//! [`Document::step_number`]: crate::model::Document::step_number

mod text;

use chartreuse_core::color::Rgba8;
use chartreuse_core::error::{Error, Result};
use chartreuse_core::image::Image;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Stroke, Transform,
};

use crate::font;
use crate::model::{
    highlighter, Annotation, Document, Point, Polyline, Shape, StepMarker, Style, Text,
};

/// The document's base image with every annotation drawn over it, bottom to
/// top, at the base image's size (see the [module docs](self)). The document
/// is unchanged.
///
/// Draws text with iced's shared font system (after [`font::load`]), so it
/// uses the same fonts, including fallbacks, as the canvas; it holds that
/// system's lock while it draws each text annotation.
///
/// # Errors
///
/// [`Error::InvalidImage`] if the image is too large for the rasterizer
/// (2²⁹ pixels wide or more).
///
/// [`font::load`]: crate::font::load
pub fn flatten(document: &Document) -> Result<Image> {
    let base = document.base();
    let annotations = document.annotations();
    if annotations.is_empty() || base.width() == 0 || base.height() == 0 {
        return Ok(base.clone());
    }
    let mut flattener = Flattener::new(base.clone())?;
    for annotation in annotations {
        flattener.draw(annotation, document.step_number(annotation.id()));
    }
    flattener.flush();
    Ok(flattener.image)
}

/// An image being flattened: the pixels so far, and the annotations drawn
/// since the last [`flush`](Self::flush), not yet composited onto them.
struct Flattener {
    /// Straight alpha.
    image: Image,
    /// Premultiplied, the image's size.
    layer: Pixmap,
    text: text::Rasterizer,
}

impl Flattener {
    fn new(image: Image) -> Result<Self> {
        let layer = Pixmap::new(image.width(), image.height()).ok_or_else(|| {
            Error::InvalidImage(format!(
                "{}×{} is too large to flatten",
                image.width(),
                image.height()
            ))
        })?;
        Ok(Self {
            image,
            layer,
            text: text::Rasterizer::new(),
        })
    }

    /// Draws `annotation` into the layer, above everything drawn so far.
    /// `number` is a step marker's number.
    fn draw(&mut self, annotation: &Annotation, number: Option<usize>) {
        let style = &annotation.style;
        let paint = paint(style.color);
        let width = style.stroke_width.max(0.0);
        let layer = &mut self.layer;
        let identity = Transform::identity();
        match &annotation.shape {
            Shape::Line(line) => polyline(layer, &[line.start, line.end], width, &paint, identity),
            Shape::Arrow(arrow) => match arrow.head(style.stroke_width) {
                Some(head) => {
                    polyline(layer, &[arrow.start, head.base], width, &paint, identity);
                    let [tip, left, right] = head.corners();
                    let mut path = PathBuilder::new();
                    path.move_to(tip.x, tip.y);
                    path.line_to(left.x, left.y);
                    path.line_to(right.x, right.y);
                    path.close();
                    fill(layer, path.finish(), &paint, identity);
                }
                None => dot(layer, arrow.start, width, &paint, identity),
            },
            Shape::Rectangle(rectangle) => {
                let [a, b, c, d] = rectangle.rect.corners();
                if a == c {
                    dot(layer, a, width, &paint, identity);
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(a.x, a.y);
                    for corner in [b, c, d] {
                        path.line_to(corner.x, corner.y);
                    }
                    path.close();
                    stroke(layer, path.finish(), width, &paint, identity);
                }
            }
            Shape::Ellipse(ellipse) => {
                let (start, curves) = ellipse.curves();
                if ellipse.rect.width() == 0.0 && ellipse.rect.height() == 0.0 {
                    dot(layer, start, width, &paint, identity);
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(start.x, start.y);
                    for [a, b, to] in curves {
                        path.cubic_to(a.x, a.y, b.x, b.y, to.x, to.y);
                    }
                    path.close();
                    stroke(layer, path.finish(), width, &paint, identity);
                }
            }
            Shape::Pen(pen) => polyline(layer, &pen.points, width, &paint, identity),
            Shape::Highlighter(stroke) => self.highlighter(stroke, style),
            Shape::Step(step) => {
                let diameter = 2.0 * StepMarker::radius(style.font_size);
                dot(layer, step.center, diameter, &paint, identity);
                if let Some(number) = number {
                    let label = number.to_string();
                    let size = font::measure(&label, style.font_size);
                    let text = Text::new(step.label_origin(size), label);
                    let style = Style {
                        color: StepMarker::number_color(style.color),
                        ..*style
                    };
                    self.text.draw(layer, &text, &style);
                }
            }
            Shape::Text(text) => self.text.draw(layer, text, style),
        }
    }

    /// A highlighter stroke: its own layer (see [`highlighter_layer`]),
    /// covering the whole pixels the stroke touches, composited onto the
    /// annotation layer at [`highlighter::alpha`].
    fn highlighter(&mut self, stroke: &Polyline, style: &Style) {
        let reach = highlighter::width(style) / 2.0;
        let bounds = stroke.path_bounds().expand(reach);
        let clamp = |value: f32, size: u32| {
            // Clamped to the image first, so the casts are exact.
            value.clamp(0.0, size as f32) as i32
        };
        let (x0, y0) = (
            clamp(bounds.min().x.floor(), self.image.width()),
            clamp(bounds.min().y.floor(), self.image.height()),
        );
        let (x1, y1) = (
            clamp(bounds.max().x.ceil(), self.image.width()),
            clamp(bounds.max().y.ceil(), self.image.height()),
        );
        let (Ok(width), Ok(height)) = (u32::try_from(x1 - x0), u32::try_from(y1 - y0)) else {
            return;
        };
        let transform = Transform::from_translate(-x0 as f32, -y0 as f32);
        if let Some(layer) = highlighter_layer(stroke, style, transform, width, height) {
            let paint = PixmapPaint {
                opacity: highlighter::alpha(style),
                quality: FilterQuality::Nearest,
                ..PixmapPaint::default()
            };
            self.layer
                .draw_pixmap(x0, y0, layer.as_ref(), &paint, Transform::identity(), None);
        }
    }

    /// Composites the layer onto the image and clears it.
    fn flush(&mut self) {
        let layer = self.layer.data_mut();
        for (dst, src) in self
            .image
            .pixels_mut()
            .chunks_exact_mut(4)
            .zip(layer.chunks_exact_mut(4))
        {
            if src[3] != 0 {
                source_over(dst, src);
                src.fill(0);
            }
        }
    }
}

/// A highlighter stroke's own layer (see [`highlighter`]): the stroke drawn
/// at full opacity, in its color with the alpha left out, into a transparent
/// premultiplied pixmap `width` × `height` pixels big, with `transform`
/// mapping document coordinates onto the pixmap. Compositing it at
/// [`highlighter::alpha`] finishes the job. The canvas draws highlighters
/// with this too, so they look the same there.
///
/// `None` if the pixmap would be empty.
pub(crate) fn highlighter_layer(
    stroke: &Polyline,
    style: &Style,
    transform: Transform,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    let mut layer = Pixmap::new(width, height)?;
    let opaque = Rgba8 {
        a: u8::MAX,
        ..style.color
    };
    polyline(
        &mut layer,
        &stroke.points,
        highlighter::width(style),
        &paint(opaque),
        transform,
    );
    Some(layer)
}

/// A stroke through `points`, or a disc if they all coincide.
fn polyline(
    target: &mut Pixmap,
    points: &[Point],
    width: f32,
    paint: &Paint<'_>,
    transform: Transform,
) {
    let [first, rest @ ..] = points else {
        return;
    };
    if rest.iter().all(|p| p == first) {
        dot(target, *first, width, paint, transform);
    } else {
        let mut path = PathBuilder::new();
        path.move_to(first.x, first.y);
        for p in rest {
            path.line_to(p.x, p.y);
        }
        stroke(target, path.finish(), width, paint, transform);
    }
}

/// A zero-length stroke: a disc `width` across.
fn dot(target: &mut Pixmap, center: Point, width: f32, paint: &Paint<'_>, transform: Transform) {
    if width > 0.0 {
        fill(
            target,
            PathBuilder::from_circle(center.x, center.y, width / 2.0),
            paint,
            transform,
        );
    }
}

/// Strokes `path` (if it was valid) `width` wide with round caps and joins.
/// A width of zero draws nothing; tiny-skia would draw a hairline.
fn stroke(
    target: &mut Pixmap,
    path: Option<Path>,
    width: f32,
    paint: &Paint<'_>,
    transform: Transform,
) {
    if let Some(path) = path
        && width > 0.0
    {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        target.stroke_path(&path, paint, &stroke, transform, None);
    }
}

/// Fills `path` (if it was valid) with the nonzero rule, as iced's canvas
/// does by default.
fn fill(target: &mut Pixmap, path: Option<Path>, paint: &Paint<'_>, transform: Transform) {
    if let Some(path) = path {
        target.fill_path(&path, paint, FillRule::Winding, transform, None);
    }
}

/// An anti-aliased solid paint in `color` (straight alpha).
fn paint(color: Rgba8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}

/// Blends the premultiplied pixel `src` over the straight-alpha pixel `dst`.
fn source_over(dst: &mut [u8], src: &[u8]) {
    let unit = |value: u8| f32::from(value) / 255.0;
    let src_alpha = unit(src[3]);
    let dst_alpha = unit(dst[3]);
    // How much of the destination shows through, premultiplied by its alpha.
    let under = dst_alpha * (1.0 - src_alpha);
    let alpha = src_alpha + under;
    for channel in 0..3 {
        let premultiplied = unit(src[channel]) + unit(dst[channel]) * under;
        dst[channel] = to_byte(premultiplied / alpha);
    }
    dst[3] = to_byte(alpha);
}

/// `value` (0 to 1) as the nearest byte.
fn to_byte(value: f32) -> u8 {
    // In range after the clamp, so the cast is exact.
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests;

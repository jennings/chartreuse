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
//! - Strokes (a line, an arrow's shaft, a rectangle's or ellipse's outline)
//!   are `stroke_width` wide, centered on the geometry, with round caps and
//!   round joins. A zero-length stroke is a disc `stroke_width` across; a
//!   stroke width of zero (or less) draws nothing.
//! - An arrow is its shaft stroked from `start` to [`ArrowHead::base`], then
//!   the head triangle `[tip, left, right]` filled, never stroked.
//! - A rectangle is the closed outline through [`Rect::corners`].
//! - An ellipse is the closed path of the Béziers of [`Ellipse::curves`];
//!   one of zero size is a dot.
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
//! - pen and highlighter strokes are paths: build a tiny-skia path and pass
//!   it to `Flattener::stroke` or `Flattener::fill` (the highlighter with
//!   its translucent color, like any other);
//! - step markers are a filled disc plus text, from the same helpers and the
//!   text rasterizer;
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

mod text;

use chartreuse_core::color::Rgba8;
use chartreuse_core::error::{Error, Result};
use chartreuse_core::image::Image;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::model::{Annotation, Document, Point, Shape};

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
        flattener.draw(annotation);
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
    fn draw(&mut self, annotation: &Annotation) {
        let style = &annotation.style;
        let paint = paint(style.color);
        let width = style.stroke_width.max(0.0);
        match &annotation.shape {
            Shape::Line(line) => self.stroke_segment(line.start, line.end, width, &paint),
            Shape::Arrow(arrow) => match arrow.head(style.stroke_width) {
                Some(head) => {
                    self.stroke_segment(arrow.start, head.base, width, &paint);
                    let [tip, left, right] = head.corners();
                    let mut path = PathBuilder::new();
                    path.move_to(tip.x, tip.y);
                    path.line_to(left.x, left.y);
                    path.line_to(right.x, right.y);
                    path.close();
                    self.fill(path.finish(), &paint);
                }
                None => self.dot(arrow.start, width, &paint),
            },
            Shape::Rectangle(rectangle) => {
                let [a, b, c, d] = rectangle.rect.corners();
                if a == c {
                    self.dot(a, width, &paint);
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(a.x, a.y);
                    for corner in [b, c, d] {
                        path.line_to(corner.x, corner.y);
                    }
                    path.close();
                    self.stroke(path.finish(), width, &paint);
                }
            }
            Shape::Ellipse(ellipse) => {
                let (start, curves) = ellipse.curves();
                if ellipse.rect.width() == 0.0 && ellipse.rect.height() == 0.0 {
                    self.dot(start, width, &paint);
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(start.x, start.y);
                    for [a, b, to] in curves {
                        path.cubic_to(a.x, a.y, b.x, b.y, to.x, to.y);
                    }
                    path.close();
                    self.stroke(path.finish(), width, &paint);
                }
            }
            Shape::Text(text) => self.text.draw(&mut self.layer, text, style),
        }
    }

    /// A stroke from `a` to `b`, or a disc if they coincide.
    fn stroke_segment(&mut self, a: Point, b: Point, width: f32, paint: &Paint<'_>) {
        if a == b {
            self.dot(a, width, paint);
        } else {
            let mut path = PathBuilder::new();
            path.move_to(a.x, a.y);
            path.line_to(b.x, b.y);
            self.stroke(path.finish(), width, paint);
        }
    }

    /// A zero-length stroke: a disc `width` across.
    fn dot(&mut self, center: Point, width: f32, paint: &Paint<'_>) {
        if width > 0.0 {
            self.fill(
                PathBuilder::from_circle(center.x, center.y, width / 2.0),
                paint,
            );
        }
    }

    /// Strokes `path` (if it was valid) `width` wide with round caps and joins.
    /// A width of zero draws nothing; tiny-skia would draw a hairline.
    fn stroke(&mut self, path: Option<Path>, width: f32, paint: &Paint<'_>) {
        if let Some(path) = path
            && width > 0.0
        {
            let stroke = Stroke {
                width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            self.layer
                .stroke_path(&path, paint, &stroke, Transform::identity(), None);
        }
    }

    /// Fills `path` (if it was valid) with the nonzero rule, as iced's canvas
    /// does by default.
    fn fill(&mut self, path: Option<Path>, paint: &Paint<'_>) {
        if let Some(path) = path {
            self.layer
                .fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
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

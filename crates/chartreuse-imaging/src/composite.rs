//! Combining per-display captures into one image laid out in the global logical
//! desktop space.
//!
//! [`composite_at`] renders the captures onto a [`PixelGrid`]: the output is
//! [`PixelGrid::pixel_size`] pixels and every display is placed at
//! [`PixelGrid::rect_to_physical`] of its [`DisplayInfo::logical_bounds`], so the
//! image lines up exactly with overlays and hit testing that use the same grid.
//! The grid comes from the [`DisplayLayout`] the captures were taken from:
//!
//! - the whole desktop (capture all displays): [`DisplayLayout::desktop_grid`];
//! - a selection (a rectangle in global logical coordinates):
//!   [`DisplayLayout::capture_grid`], which is `None` when the selection lies on
//!   no display.
//!
//! ```
//! # use chartreuse_core::color::Rgba8;
//! # use chartreuse_core::display::{DisplayId, DisplayInfo, DisplayLayout};
//! # use chartreuse_core::geometry::{LogicalRect, ScaleFactor};
//! # use chartreuse_core::image::Image;
//! # let bounds = LogicalRect::new(0.0, 0.0, 4.0, 2.0);
//! # let display = DisplayInfo {
//! #     id: DisplayId(1),
//! #     name: "Built-in".into(),
//! #     logical_bounds: bounds,
//! #     pixel_size: bounds.size.to_physical(ScaleFactor::ONE),
//! #     scale_factor: ScaleFactor::ONE,
//! #     is_primary: true,
//! # };
//! # let captures = [(display.clone(), Image::filled(display.pixel_size, Rgba8::WHITE))];
//! let layout = DisplayLayout::new(captures.iter().map(|(d, _)| d.clone()).collect())?;
//! let pairs = || captures.iter().map(|(d, i)| (d, i));
//! let desktop = chartreuse_imaging::composite_at(pairs(), layout.desktop_grid())?;
//! let selection = LogicalRect::new(1.0, 0.0, 2.0, 1.0);
//! if let Some(grid) = layout.capture_grid(&selection) {
//!     let cropped = chartreuse_imaging::composite_at(pairs(), grid)?;
//! #   assert_eq!(cropped.image.size().width, 2);
//! }
//! # assert_eq!(desktop.image.size().width, 4);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Both grids use the **largest** scale factor among the displays involved (see
//! [`chartreuse_core::display`]), so no display loses detail. Captures from
//! lower-scale displays are upscaled into place:
//!
//! - when the output size is an exact integer multiple of the capture size along
//!   an axis (a 1× display in a 2× composite, say) each source pixel is repeated,
//!   which keeps text crisp and loses nothing;
//! - otherwise (a 1.5× display in a 2× composite) the capture is resampled
//!   bilinearly, on premultiplied colors so transparent pixels do not darken or
//!   tint their neighbours.
//!
//! Areas that no display covers (gaps between displays of different sizes) are
//! transparent. Where displays overlap (mirroring), later captures in the input
//! are drawn over earlier ones.
//!
//! Inputs are `(display, image)` pairs, for example
//! `captures.iter().map(|c| (&c.display, &c.image))` over the platform crate's
//! `DisplayCapture`s. The image is stretched over the display's logical bounds, so
//! it need not be exactly `display.pixel_size`.

#[cfg(doc)]
use chartreuse_core::display::DisplayLayout;
use chartreuse_core::display::{DisplayInfo, PixelGrid};
use chartreuse_core::geometry::PhysicalRect;
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};

use crate::region::{bounds, clip, copy_region, transparent};

/// A composited image and the pixel grid it is drawn on.
#[derive(Debug, Clone, PartialEq)]
pub struct Composite {
    /// The pixels, [`PixelGrid::pixel_size`] of `grid` in size.
    pub image: Image,
    /// Maps between the image's pixels and global logical coordinates.
    pub grid: PixelGrid,
}

/// Renders `captures` onto `grid`.
///
/// The image is `grid.pixel_size()` pixels and each display lands at
/// `grid.rect_to_physical(&display.logical_bounds)`. Captures outside the grid
/// are skipped; with none inside it, the image is fully transparent.
///
/// # Errors
///
/// [`Error::InvalidImage`] if `grid` covers no pixels, or the result would be too
/// large to allocate.
pub fn composite_at<'a>(
    captures: impl IntoIterator<Item = (&'a DisplayInfo, &'a Image)>,
    grid: PixelGrid,
) -> Result<Composite> {
    let size = grid.pixel_size();
    if size.is_empty() {
        return Err(Error::InvalidImage(
            "the area to capture covers no pixels".into(),
        ));
    }
    let mut image = transparent(size)?;
    for (display, capture) in captures {
        draw_scaled(
            capture,
            grid.rect_to_physical(&display.logical_bounds),
            &mut image,
        );
    }
    Ok(Composite { image, grid })
}

/// Stretches all of `src` over `dst_rect` (in `dst`'s pixel coordinates, possibly
/// extending past its edges), replacing the covered pixels of `dst`.
fn draw_scaled(src: &Image, dst_rect: PhysicalRect, dst: &mut Image) {
    if src.size().is_empty() {
        return;
    }
    if dst_rect.size == src.size() {
        copy_region(src, bounds(src), dst, dst_rect.origin);
        return;
    }
    let Some(visible) = clip(dst, dst_rect) else {
        return;
    };
    // `visible` lies inside `dst_rect`, so these offsets are non-negative.
    let columns = axis_samples(
        (i64::from(visible.min_x()) - i64::from(dst_rect.min_x())) as u32,
        visible.size.width,
        dst_rect.size.width,
        src.width(),
    );
    let rows = axis_samples(
        (i64::from(visible.min_y()) - i64::from(dst_rect.min_y())) as u32,
        visible.size.height,
        dst_rect.size.height,
        src.height(),
    );
    let src_stride = src.width() as usize * 4;
    let dst_stride = dst.width() as usize * 4;
    let (vx, vy) = (visible.min_x() as usize, visible.min_y() as usize);
    let src_pixels = src.pixels();
    let dst_pixels = dst.pixels_mut();
    for (j, row) in rows.iter().enumerate() {
        let top = row.first as usize * src_stride;
        let bottom = row.second as usize * src_stride;
        let out_row = (vy + j) * dst_stride + vx * 4;
        for (i, column) in columns.iter().enumerate() {
            let left = column.first as usize * 4;
            let right = column.second as usize * 4;
            let out = &mut dst_pixels[out_row + i * 4..out_row + i * 4 + 4];
            if row.weight == 0.0 && column.weight == 0.0 {
                out.copy_from_slice(&src_pixels[top + left..top + left + 4]);
                continue;
            }
            let (wx, wy) = (column.weight, row.weight);
            let taps = [
                (top + left, (1.0 - wx) * (1.0 - wy)),
                (top + right, wx * (1.0 - wy)),
                (bottom + left, (1.0 - wx) * wy),
                (bottom + right, wx * wy),
            ];
            out.copy_from_slice(&blend_premultiplied(src_pixels, taps));
        }
    }
}

/// Where one output pixel samples along an axis: `first` and `second` are source
/// indices, `weight` is the share of `second` (0 means "exactly `first`").
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    first: u32,
    second: u32,
    weight: f32,
}

/// The samples for output pixels `start..start + count` along an axis where
/// `src_len` source pixels are stretched over `dst_len` output pixels.
///
/// Exact integer upscales repeat pixels; other ratios interpolate linearly between
/// the two source pixels nearest each output pixel's centre, clamped at the edges.
fn axis_samples(start: u32, count: u32, dst_len: u32, src_len: u32) -> Vec<Sample> {
    let nearest = dst_len.is_multiple_of(src_len);
    let ratio = f64::from(src_len) / f64::from(dst_len);
    let last = src_len - 1;
    (start..start + count)
        .map(|i| {
            if nearest {
                let index = (u64::from(i) * u64::from(src_len) / u64::from(dst_len)) as u32;
                return Sample {
                    first: index,
                    second: index,
                    weight: 0.0,
                };
            }
            let centre = ((f64::from(i) + 0.5) * ratio - 0.5).clamp(0.0, f64::from(last));
            let first = centre.floor() as u32;
            Sample {
                first,
                second: (first + 1).min(last),
                weight: (centre - f64::from(first)) as f32,
            }
        })
        .collect()
}

/// The weighted average of the pixels at the given byte offsets, computed on
/// premultiplied colors and returned with straight alpha.
fn blend_premultiplied(pixels: &[u8], taps: [(usize, f32); 4]) -> [u8; 4] {
    let mut sum = [0.0_f32; 4];
    for (offset, weight) in taps {
        let [r, g, b, a] = [0, 1, 2, 3].map(|k| f32::from(pixels[offset + k]));
        let alpha = a * weight;
        sum[0] += r * alpha;
        sum[1] += g * alpha;
        sum[2] += b * alpha;
        sum[3] += alpha;
    }
    let alpha = sum[3];
    if alpha <= 0.0 {
        return [0; 4];
    }
    let channel = |value: f32| (value / alpha).round().clamp(0.0, 255.0) as u8;
    [
        channel(sum[0]),
        channel(sum[1]),
        channel(sum[2]),
        alpha.round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::display::{DisplayId, DisplayLayout};
    use chartreuse_core::geometry::{LogicalRect, PhysicalSize, ScaleFactor};

    use super::*;

    const RED: Rgba8 = Rgba8::rgb(255, 0, 0);
    const BLUE: Rgba8 = Rgba8::rgb(0, 0, 255);

    fn display(id: u64, bounds: LogicalRect, scale: f64) -> DisplayInfo {
        let scale_factor = ScaleFactor::new(scale).unwrap();
        DisplayInfo {
            id: DisplayId(id),
            name: format!("Display {id}"),
            logical_bounds: bounds,
            pixel_size: bounds.size.to_physical(scale_factor),
            scale_factor,
            is_primary: id == 1,
        }
    }

    /// A capture of `display` whose pixels encode their own coordinates.
    fn numbered(display: &DisplayInfo) -> Image {
        Image::from_fn(display.pixel_size, |x, y| {
            Rgba8::new(x as u8, y as u8, display.id.0 as u8, 255)
        })
    }

    fn pairs(captures: &[(DisplayInfo, Image)]) -> impl Iterator<Item = (&DisplayInfo, &Image)> {
        captures.iter().map(|(d, i)| (d, i))
    }

    fn layout(captures: &[(DisplayInfo, Image)]) -> DisplayLayout {
        DisplayLayout::new(captures.iter().map(|(d, _)| d.clone()).collect()).unwrap()
    }

    /// Composites the whole desktop, as capturing every display does.
    fn desktop(captures: &[(DisplayInfo, Image)]) -> Composite {
        composite_at(pairs(captures), layout(captures).desktop_grid()).unwrap()
    }

    /// Composites a selection of the desktop.
    fn selection(captures: &[(DisplayInfo, Image)], rect: LogicalRect) -> Composite {
        let grid = layout(captures).capture_grid(&rect).unwrap();
        composite_at(pairs(captures), grid).unwrap()
    }

    fn px(image: &Image, x: u32, y: u32) -> Rgba8 {
        image.pixel(x, y).unwrap()
    }

    #[test]
    fn displays_are_placed_by_logical_bounds_including_negative_origins() {
        // Primary 4×2 at the origin; a 2×2 display up and to the left, which leaves
        // gaps at the bottom-left and top-right of the union.
        let primary = display(1, LogicalRect::new(0.0, 0.0, 4.0, 2.0), 1.0);
        let left = display(2, LogicalRect::new(-2.0, -1.0, 2.0, 2.0), 1.0);
        let captures = [
            (primary.clone(), Image::filled(primary.pixel_size, RED)),
            (left.clone(), Image::filled(left.pixel_size, BLUE)),
        ];
        let out = desktop(&captures);
        assert_eq!(
            out.grid.logical_bounds(),
            LogicalRect::new(-2.0, -1.0, 6.0, 3.0)
        );
        assert_eq!(out.grid.scale(), ScaleFactor::ONE);
        assert_eq!(out.image.size(), PhysicalSize::new(6, 3));
        assert_eq!(px(&out.image, 0, 0), BLUE, "global (-2, -1)");
        assert_eq!(px(&out.image, 1, 1), BLUE, "global (-1, 0)");
        assert_eq!(px(&out.image, 2, 1), RED, "global (0, 0)");
        assert_eq!(px(&out.image, 5, 2), RED, "global (3, 1)");
        assert_eq!(px(&out.image, 0, 2), Rgba8::TRANSPARENT, "gap below left");
        assert_eq!(
            px(&out.image, 5, 0),
            Rgba8::TRANSPARENT,
            "gap above primary"
        );
    }

    #[test]
    fn mixed_scales_render_at_the_maximum_and_repeat_integer_upscales() {
        // A 2× display of 2×2 points, and a 1× display of 2×1 points to its right.
        let retina = display(1, LogicalRect::new(0.0, 0.0, 2.0, 2.0), 2.0);
        let external = display(2, LogicalRect::new(2.0, 0.0, 2.0, 1.0), 1.0);
        let captures = [
            (retina.clone(), numbered(&retina)),
            (external.clone(), numbered(&external)),
        ];
        let out = desktop(&captures);
        assert_eq!(out.grid.scale().get(), 2.0);
        assert_eq!(out.image.size(), PhysicalSize::new(8, 4));
        // The 2× capture is copied pixel for pixel.
        assert_eq!(px(&out.image, 3, 3), Rgba8::new(3, 3, 1, 255));
        // Each 1× pixel becomes a 2×2 block.
        for (x, y, source_x) in [(4, 0, 0), (5, 1, 0), (6, 0, 1), (7, 1, 1)] {
            assert_eq!(px(&out.image, x, y), Rgba8::new(source_x, 0, 2, 255));
        }
        assert_eq!(
            px(&out.image, 4, 2),
            Rgba8::TRANSPARENT,
            "below the 1× display"
        );
    }

    #[test]
    fn fractional_upscales_interpolate_between_pixel_centres() {
        // A 1.5× display 2 points wide (3 pixels) next to a 2× display: its capture
        // is stretched from 3 to 4 pixels per row.
        let retina = display(1, LogicalRect::new(0.0, 0.0, 1.0, 2.0), 2.0);
        let mid = display(2, LogicalRect::new(1.0, 0.0, 2.0, 2.0 / 3.0), 1.5);
        let ramp = Image::from_fn(PhysicalSize::new(3, 1), |x, _| {
            Rgba8::rgb(x as u8 * 80, 0, 0)
        });
        let captures = [
            (retina.clone(), Image::filled(retina.pixel_size, BLUE)),
            (mid, ramp),
        ];
        let out = desktop(&captures);
        let reds: Vec<u8> = (2..6).map(|x| px(&out.image, x, 0).r).collect();
        // Centres map to source x = 0 (clamped), 0.625, 1.375, 2 (clamped).
        assert_eq!(reds, [0, 50, 110, 160]);
    }

    #[test]
    fn interpolation_does_not_bleed_color_from_transparent_pixels() {
        let retina = display(1, LogicalRect::new(0.0, 0.0, 1.0, 1.0), 2.0);
        let mid = display(2, LogicalRect::new(1.0, 0.0, 1.5, 1.0), 4.0 / 3.0);
        // 2 source pixels stretched over 3: the middle one is a 50/50 blend.
        let src = Image::from_fn(PhysicalSize::new(2, 1), |x, _| {
            if x == 0 {
                RED
            } else {
                Rgba8::new(0, 255, 0, 0)
            }
        });
        let captures = [(retina.clone(), numbered(&retina)), (mid, src)];
        let out = desktop(&captures);
        assert_eq!(px(&out.image, 3, 0), Rgba8::new(255, 0, 0, 128));
        assert_eq!(px(&out.image, 4, 0).a, 0);
    }

    #[test]
    fn region_on_one_display_is_an_exact_crop_at_its_own_scale() {
        let retina = display(1, LogicalRect::new(0.0, 0.0, 4.0, 4.0), 2.0);
        let external = display(2, LogicalRect::new(-4.0, 0.0, 4.0, 4.0), 1.0);
        let captures = [
            (retina.clone(), numbered(&retina)),
            (external.clone(), numbered(&external)),
        ];
        let region = LogicalRect::new(-3.0, 1.0, 2.0, 1.0);
        let out = selection(&captures, region);
        assert_eq!(
            out.grid.scale(),
            ScaleFactor::ONE,
            "only the 1× display is involved"
        );
        assert_eq!(out.grid.logical_bounds(), region);
        assert_eq!(out.image.size(), PhysicalSize::new(2, 1));
        assert_eq!(px(&out.image, 0, 0), Rgba8::new(1, 1, 2, 255));
        assert_eq!(px(&out.image, 1, 0), Rgba8::new(2, 1, 2, 255));
    }

    #[test]
    fn region_across_displays_uses_their_maximum_scale_and_leaves_gaps_clear() {
        let retina = display(1, LogicalRect::new(0.0, 0.0, 4.0, 4.0), 2.0);
        let external = display(2, LogicalRect::new(-4.0, 0.0, 4.0, 2.0), 1.0);
        let captures = [
            (retina.clone(), numbered(&retina)),
            (external.clone(), numbered(&external)),
        ];
        // From 1 point left of the boundary to 1 point right, and down past the
        // bottom of the 1× display.
        let region = LogicalRect::new(-1.0, 1.0, 2.0, 2.0);
        let out = selection(&captures, region);
        assert_eq!(out.grid.scale().get(), 2.0);
        assert_eq!(out.image.size(), PhysicalSize::new(4, 4));
        // 1× pixel (3, 1) doubled.
        assert_eq!(px(&out.image, 0, 0), Rgba8::new(3, 1, 2, 255));
        assert_eq!(px(&out.image, 1, 1), Rgba8::new(3, 1, 2, 255));
        // 2× pixel (0, 2) and on, copied.
        assert_eq!(px(&out.image, 2, 0), Rgba8::new(0, 2, 1, 255));
        assert_eq!(px(&out.image, 3, 3), Rgba8::new(1, 5, 1, 255));
        // Below the 1× display: nothing.
        assert_eq!(px(&out.image, 1, 2), Rgba8::TRANSPARENT);
    }

    #[test]
    fn later_captures_draw_over_earlier_ones() {
        let a = display(1, LogicalRect::new(0.0, 0.0, 2.0, 1.0), 1.0);
        let b = display(2, LogicalRect::new(1.0, 0.0, 2.0, 1.0), 1.0);
        let captures = [
            (a.clone(), Image::filled(a.pixel_size, RED)),
            (b.clone(), Image::filled(b.pixel_size, BLUE)),
        ];
        let out = desktop(&captures);
        let row: Vec<Rgba8> = (0..3).map(|x| px(&out.image, x, 0)).collect();
        assert_eq!(row, [RED, BLUE, BLUE]);
    }

    #[test]
    fn placement_is_relative_to_the_area_origin_then_rounded() {
        // At 1.5×, an area starting at x = 0.4 and 1 point wide is 2 pixels wide
        // (1.5 rounded), wherever it starts; the display edge at x = 0 lands at
        // (0 − 0.4) × 1.5 = −0.6, rounded to −1.
        let mid = display(1, LogicalRect::new(0.0, 0.0, 4.0, 4.0), 1.5);
        let captures = [(mid.clone(), numbered(&mid))];
        let area = LogicalRect::new(0.4, 0.0, 1.0, 2.0);
        let out = selection(&captures, area);
        assert_eq!(out.image.size(), PhysicalSize::new(2, 3));
        assert_eq!(px(&out.image, 0, 0), Rgba8::new(1, 0, 1, 255));
        assert_eq!(px(&out.image, 1, 2), Rgba8::new(2, 2, 1, 255));
    }

    #[test]
    fn composite_at_renders_any_area_at_the_requested_scale() {
        let two = ScaleFactor::new(2.0).unwrap();
        let area = LogicalRect::new(-1.0, 0.0, 2.0, 1.0);
        let blank = composite_at(std::iter::empty(), PixelGrid::new(area, two)).unwrap();
        assert_eq!(
            blank.image,
            Image::filled(PhysicalSize::new(4, 2), Rgba8::TRANSPARENT)
        );
        let tiny = LogicalRect::new(0.0, 0.0, 0.2, 5.0);
        assert!(matches!(
            composite_at(std::iter::empty(), PixelGrid::new(tiny, two)),
            Err(Error::InvalidImage(_))
        ));
    }
}

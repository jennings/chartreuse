//! Cropping and copying rectangular regions, in physical pixels.
//!
//! Regions are [`PhysicalRect`]s in the image's own pixel coordinates: `(0, 0)` is
//! the top-left pixel. Rectangles that extend past the image are clipped to it
//! rather than rejected, so callers can pass a selection that was dragged past an
//! edge without clamping it first.

use chartreuse_core::geometry::{PhysicalPoint, PhysicalRect, PhysicalSize};
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};

/// The rectangle covering every pixel of `image`.
#[must_use]
pub fn bounds(image: &Image) -> PhysicalRect {
    PhysicalRect::new(0, 0, image.width(), image.height())
}

/// The part of `rect` that lies inside `image`, or `None` if they do not overlap.
#[must_use]
pub fn clip(image: &Image, rect: PhysicalRect) -> Option<PhysicalRect> {
    bounds(image).intersection(&rect)
}

/// A new image holding the pixels of `image` inside `rect`.
///
/// `rect` is clipped to the image first, so the result can be smaller than `rect`.
///
/// # Errors
///
/// [`Error::InvalidImage`] if `rect` does not overlap the image at all (including
/// when `rect` is empty), because the crop would have no pixels.
pub fn crop(image: &Image, rect: PhysicalRect) -> Result<Image> {
    let clipped = clip(image, rect).ok_or_else(|| {
        Error::InvalidImage(format!(
            "the crop {}×{} at ({}, {}) lies outside the {}×{} image",
            rect.size.width,
            rect.size.height,
            rect.origin.x,
            rect.origin.y,
            image.width(),
            image.height()
        ))
    })?;
    let mut out = transparent(clipped.size)?;
    copy_region(image, clipped, &mut out, PhysicalPoint::new(0, 0));
    Ok(out)
}

/// Copies the pixels of `src` inside `src_rect` into `dst`, placing the top-left
/// corner of `src_rect` at `dst_origin`.
///
/// Pixels are replaced, not blended. Whatever part of the region falls outside
/// `src` or outside `dst` is skipped. Returns the rectangle of `dst` that was
/// written, or `None` if nothing was.
pub fn copy_region(
    src: &Image,
    src_rect: PhysicalRect,
    dst: &mut Image,
    dst_origin: PhysicalPoint,
) -> Option<PhysicalRect> {
    let src_clipped = clip(src, src_rect)?;
    // Where the clipped source lands in `dst`, before clipping to `dst`. Computed in
    // i64: origins near the i32 limits must not overflow.
    let dx = i64::from(dst_origin.x) + i64::from(src_clipped.min_x()) - i64::from(src_rect.min_x());
    let dy = i64::from(dst_origin.y) + i64::from(src_clipped.min_y()) - i64::from(src_rect.min_y());
    let placed = PhysicalRect::new(
        i32::try_from(dx).ok()?,
        i32::try_from(dy).ok()?,
        src_clipped.size.width,
        src_clipped.size.height,
    );
    let written = clip(dst, placed)?;
    // `written` lies inside `placed`, which maps onto `src_clipped` pixel for pixel,
    // so these source offsets are in range.
    let sx = (i64::from(src_clipped.min_x()) + i64::from(written.min_x()) - dx) as usize;
    let sy = (i64::from(src_clipped.min_y()) + i64::from(written.min_y()) - dy) as usize;
    let (wx, wy) = (written.min_x() as usize, written.min_y() as usize);
    let row_bytes = written.size.width as usize * 4;
    let src_stride = src.width() as usize * 4;
    let dst_stride = dst.width() as usize * 4;
    let src_pixels = src.pixels();
    let dst_pixels = dst.pixels_mut();
    for row in 0..written.size.height as usize {
        let s = (sy + row) * src_stride + sx * 4;
        let d = (wy + row) * dst_stride + wx * 4;
        dst_pixels[d..d + row_bytes].copy_from_slice(&src_pixels[s..s + row_bytes]);
    }
    Some(written)
}

/// A fully transparent image of `size`.
///
/// # Errors
///
/// [`Error::InvalidImage`] if the buffer size overflows.
pub(crate) fn transparent(size: PhysicalSize) -> Result<Image> {
    let len = usize::try_from(size.width)
        .ok()
        .and_then(|w| w.checked_mul(usize::try_from(size.height).ok()?))
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| {
            Error::InvalidImage(format!("{}×{} is too large", size.width, size.height))
        })?;
    Image::new(size, vec![0; len])
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;

    use super::*;

    /// A 4×3 image whose pixel at `(x, y)` encodes its own coordinates.
    fn numbered() -> Image {
        Image::from_fn(PhysicalSize::new(4, 3), |x, y| {
            Rgba8::new(x as u8, y as u8, 0, 255)
        })
    }

    fn coords(image: &Image, x: u32, y: u32) -> (u8, u8) {
        let p = image.pixel(x, y).unwrap();
        (p.r, p.g)
    }

    #[test]
    fn crop_inside_the_image_takes_exactly_the_region() {
        let cropped = crop(&numbered(), PhysicalRect::new(1, 1, 2, 2)).unwrap();
        assert_eq!(cropped.size(), PhysicalSize::new(2, 2));
        assert_eq!(coords(&cropped, 0, 0), (1, 1));
        assert_eq!(coords(&cropped, 1, 1), (2, 2));
    }

    #[test]
    fn crop_of_the_whole_image_is_identical() {
        let image = numbered();
        assert_eq!(crop(&image, bounds(&image)).unwrap(), image);
    }

    #[test]
    fn crop_past_the_edges_is_clipped() {
        let cropped = crop(&numbered(), PhysicalRect::new(-2, 2, 10, 10)).unwrap();
        assert_eq!(cropped.size(), PhysicalSize::new(4, 1));
        assert_eq!(coords(&cropped, 0, 0), (0, 2));
        assert_eq!(coords(&cropped, 3, 0), (3, 2));
    }

    #[test]
    fn crop_outside_or_empty_is_an_error() {
        let image = numbered();
        for rect in [
            PhysicalRect::new(4, 0, 1, 1),
            PhysicalRect::new(0, 3, 1, 1),
            PhysicalRect::new(-1, -1, 1, 1),
            PhysicalRect::new(1, 1, 0, 2),
        ] {
            assert!(
                matches!(crop(&image, rect), Err(Error::InvalidImage(_))),
                "{rect:?}"
            );
        }
    }

    #[test]
    fn copy_region_clips_against_source_and_destination() {
        let src = numbered();
        let mut dst = Image::filled(PhysicalSize::new(3, 3), Rgba8::WHITE);
        // The source rect starts one pixel left of `src`, and lands one pixel above
        // `dst`: only src (0..2, 1..3) → dst (1..3, 0..2) survives.
        let written = copy_region(
            &src,
            PhysicalRect::new(-1, 0, 3, 3),
            &mut dst,
            PhysicalPoint::new(0, -1),
        );
        assert_eq!(written, Some(PhysicalRect::new(1, 0, 2, 2)));
        assert_eq!(coords(&dst, 1, 0), (0, 1));
        assert_eq!(coords(&dst, 2, 1), (1, 2));
        assert_eq!(dst.pixel(0, 0), Some(Rgba8::WHITE), "left column untouched");
        assert_eq!(dst.pixel(1, 2), Some(Rgba8::WHITE), "bottom row untouched");
    }

    #[test]
    fn copy_region_replaces_rather_than_blends() {
        let src = Image::filled(PhysicalSize::new(1, 1), Rgba8::TRANSPARENT);
        let mut dst = Image::filled(PhysicalSize::new(1, 1), Rgba8::WHITE);
        copy_region(&src, bounds(&src), &mut dst, PhysicalPoint::new(0, 0));
        assert_eq!(dst.pixel(0, 0), Some(Rgba8::TRANSPARENT));
    }

    #[test]
    fn copy_region_entirely_off_the_destination_writes_nothing() {
        let src = numbered();
        let mut dst = Image::filled(PhysicalSize::new(2, 2), Rgba8::WHITE);
        let before = dst.clone();
        let far = PhysicalPoint::new(i32::MAX, i32::MAX);
        assert_eq!(copy_region(&src, bounds(&src), &mut dst, far), None);
        assert_eq!(dst, before);
    }
}

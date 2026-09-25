//! macOS: `CGImage` → [`Image`] conversion, shared by the backends that receive
//! Core Graphics images.

use chartreuse_core::geometry::PhysicalSize;
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo,
};

/// Draws `image` into a `size` sRGB RGBA8 bitmap and returns it as an [`Image`],
/// scaling if the sizes differ.
///
/// CoreGraphics bitmap contexts only support premultiplied alpha for 8-bit RGBA,
/// so the pixels are un-premultiplied afterwards ([`unpremultiply`]).
pub(super) fn image_from_cg(image: &CGImage, size: PhysicalSize) -> Result<Image> {
    let width = size.width as usize;
    let height = size.height as usize;
    let too_large = || Error::InvalidImage(format!("{}×{} is too large", size.width, size.height));
    let stride = width.checked_mul(4).ok_or_else(too_large)?;
    let mut pixels = vec![0; stride.checked_mul(height).ok_or_else(too_large)?];
    // SAFETY: `kCGColorSpaceSRGB` is an immutable CoreGraphics constant.
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or_else(|| Error::Platform("the sRGB color space is unavailable".into()))?;
    // Byte order 32-big with alpha last: memory holds r, g, b, a.
    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    {
        // SAFETY: `pixels` holds `stride × height` bytes and outlives the context,
        // which is dropped at the end of this block.
        let context = unsafe {
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                8,
                stride,
                Some(&srgb),
                bitmap_info,
            )
        }
        .ok_or_else(|| {
            Error::Platform(format!(
                "could not create a {}×{} bitmap context",
                size.width, size.height
            ))
        })?;
        let bounds = CGRect::new(CGPoint::ZERO, CGSize::new(width as f64, height as f64));
        CGContext::draw_image(Some(&context), bounds, Some(image));
    }
    unpremultiply(&mut pixels);
    Image::new(size, pixels)
}

/// Converts RGBA8 pixels from premultiplied to straight alpha in place, rounding
/// to nearest. Fully transparent pixels become transparent black; color values
/// above their alpha (invalid premultiplied data) saturate at 255.
fn unpremultiply(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        match alpha {
            255 => {}
            0 => pixel[..3].fill(0),
            _ => {
                for channel in &mut pixel[..3] {
                    let straight = (u16::from(*channel) * 255 + alpha / 2) / alpha;
                    *channel = straight.min(255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::CFRetained;

    use super::*;

    #[test]
    fn unpremultiply_restores_straight_alpha() {
        let mut pixels = [
            10, 20, 30, 255, // opaque: unchanged
            40, 50, 60, 0, // transparent: becomes transparent black
            64, 32, 0, 128, // half alpha: doubled (rounded)
            200, 100, 50, 100, // color above alpha: saturates
            1, 1, 1, 3, // rounds to nearest: 1 × 255 / 3 = 85
        ];
        unpremultiply(&mut pixels);
        assert_eq!(
            pixels,
            [
                10, 20, 30, 255, //
                0, 0, 0, 0, //
                128, 64, 0, 128, //
                255, 255, 128, 100, //
                85, 85, 85, 3,
            ]
        );
    }

    #[test]
    fn unpremultiply_round_trips_premultiplied_colors() {
        for alpha in 1..=255u16 {
            for straight in [0u16, 1, 77, 128, 254, 255] {
                let premultiplied = (straight * alpha + 127) / 255;
                let mut pixel = [premultiplied as u8, 0, 0, alpha as u8];
                unpremultiply(&mut pixel);
                let restored = u16::from(pixel[0]);
                // Premultiplying loses precision at low alpha; the round trip is
                // within one premultiplied step.
                let tolerance = 255 / alpha / 2 + 1;
                assert!(
                    restored.abs_diff(straight) <= tolerance,
                    "alpha {alpha}: {straight} → {premultiplied} → {restored}"
                );
            }
        }
    }

    /// A 2×2 sRGB `CGImage` from premultiplied RGBA bytes, top row first.
    fn cg_image(premultiplied: [u8; 16]) -> CFRetained<CGImage> {
        use objc2_core_graphics::{
            CGBitmapContextCreateImage, CGBitmapContextGetBytesPerRow, CGBitmapContextGetData,
        };

        // SAFETY: `kCGColorSpaceSRGB` is an immutable CoreGraphics constant.
        let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap();
        // SAFETY: a null buffer makes CoreGraphics allocate and own the memory, so
        // the image made from the context stays valid on its own.
        let context = unsafe {
            CGBitmapContextCreate(
                std::ptr::null_mut(),
                2,
                2,
                8,
                0,
                Some(&srgb),
                CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
            )
        }
        .unwrap();
        let data = CGBitmapContextGetData(Some(&context)).cast::<u8>();
        let stride = CGBitmapContextGetBytesPerRow(Some(&context));
        for (row, bytes) in premultiplied.chunks_exact(8).enumerate() {
            // SAFETY: the context's buffer has 2 rows of `stride` ≥ 8 bytes.
            unsafe {
                data.add(row * stride)
                    .copy_from_nonoverlapping(bytes.as_ptr(), 8)
            };
        }
        CGBitmapContextCreateImage(Some(&context)).unwrap()
    }

    #[test]
    fn cg_images_convert_to_straight_rgba_top_row_first() {
        let source = cg_image([
            255, 0, 0, 255, /**/ 0, 255, 0, 255, // top: red, green
            0, 0, 255, 255, /**/ 128, 128, 128,
            128, // bottom: blue, half-transparent white
        ]);
        let size = PhysicalSize::new(2, 2);
        let image = image_from_cg(&source, size).unwrap();
        assert_eq!(image.size(), size);
        assert_eq!(
            image.pixels(),
            [
                255, 0, 0, 255, /**/ 0, 255, 0, 255, //
                0, 0, 255, 255, /**/ 255, 255, 255, 128,
            ]
        );
    }
}

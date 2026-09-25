//! macOS: clipboard access through the general `NSPasteboard`.
//!
//! Writing puts one pasteboard item on the clipboard with both a PNG and a TIFF
//! representation of the image, whose size in points equals its size in pixels.
//! Reading accepts anything `NSImage` can read from a pasteboard (PNG, TIFF, PDF,
//! images copied from Preview or a browser) and converts it to straight-alpha
//! sRGB RGBA8 at the image's pixel size.

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapFormat, NSBitmapImageFileType, NSBitmapImageRep, NSColorSpace, NSDeviceRGBColorSpace,
    NSImage, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF,
};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSDictionary, NSSize};

use super::cgimage::cg_image_to_image;
use crate::clipboard::Clipboard;

/// The macOS [`Clipboard`] backend.
#[derive(Debug, Default)]
pub struct MacosClipboard;

impl MacosClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl Clipboard for MacosClipboard {
    fn write_image(&self, image: &Image) -> Result<()> {
        write_image_to(&NSPasteboard::generalPasteboard(), image)
    }

    fn read_image(&self) -> Result<Option<Image>> {
        read_image_from(&NSPasteboard::generalPasteboard())
    }
}

/// Replaces the contents of `pasteboard` with `image` as PNG and TIFF.
fn write_image_to(pasteboard: &NSPasteboard, image: &Image) -> Result<()> {
    let rep = bitmap_rep(image)?;
    // SAFETY: an empty properties dictionary is valid for every file type.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or_else(|| Error::Encode("NSBitmapImageRep produced no PNG data".into()))?;
    let tiff = rep
        .TIFFRepresentation()
        .ok_or_else(|| Error::Encode("NSBitmapImageRep produced no TIFF data".into()))?;

    pasteboard.clearContents();
    // SAFETY: the type names are AppKit's own constants.
    let (png_type, tiff_type) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
    if !pasteboard.setData_forType(Some(&png), png_type)
        || !pasteboard.setData_forType(Some(&tiff), tiff_type)
    {
        return Err(Error::Platform(
            "NSPasteboard refused the image data".into(),
        ));
    }
    Ok(())
}

/// An sRGB, straight-alpha `NSBitmapImageRep` holding a copy of `image`, sized so
/// one point is one pixel.
fn bitmap_rep(image: &Image) -> Result<Retained<NSBitmapImageRep>> {
    let invalid = || {
        Error::InvalidImage(format!(
            "cannot copy a {}×{} image",
            image.width(),
            image.height()
        ))
    };
    if image.width() == 0 || image.height() == 0 {
        return Err(invalid());
    }
    let width = isize::try_from(image.width()).map_err(|_| invalid())?;
    let height = isize::try_from(image.height()).map_err(|_| invalid())?;
    let bytes_per_row = width.checked_mul(4).ok_or_else(invalid)?;
    // SAFETY: null planes make the rep allocate (and own) its pixel buffer; the
    // other arguments describe meshed 8-bit RGBA with straight alpha.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            NSBitmapFormat::AlphaNonpremultiplied,
            bytes_per_row,
            32,
        )
    }
    .ok_or_else(invalid)?;
    let pixels = image.pixels();
    let buffer = rep.bitmapData();
    if buffer.is_null() || rep.bytesPerRow() != bytes_per_row {
        return Err(Error::Platform(
            "NSBitmapImageRep allocated an unexpected buffer".into(),
        ));
    }
    // SAFETY: the rep's buffer is `bytes_per_row × height` bytes (checked above),
    // which is exactly `pixels.len()`, and it cannot overlap our own buffer.
    unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), buffer, pixels.len()) };

    // Our pixels are sRGB; the initializer only accepts a color space *name*, so
    // tag the rep with sRGB afterwards (this reinterprets, it does not convert).
    let rep = rep
        .bitmapImageRepByRetaggingWithColorSpace(&NSColorSpace::sRGBColorSpace())
        .ok_or_else(|| Error::Platform("could not tag the image as sRGB".into()))?;
    // One point per pixel (72 dpi), so consumers paste the image at pixel size.
    rep.setSize(NSSize::new(
        f64::from(image.width()),
        f64::from(image.height()),
    ));
    Ok(rep)
}

/// The image on `pasteboard`, or `None` if it holds nothing `NSImage` can read.
fn read_image_from(pasteboard: &NSPasteboard) -> Result<Option<Image>> {
    if !NSImage::canInitWithPasteboard(pasteboard) {
        return Ok(None);
    }
    // `canInitWithPasteboard` only looks at the types on offer; a file URL, for
    // example, may still not point at an image.
    let Some(image) = NSImage::initWithPasteboard(NSImage::alloc(), pasteboard) else {
        return Ok(None);
    };
    let cg_image = largest_bitmap(&image)
        .or_else(|| {
            // Not a bitmap (e.g. PDF): rasterize it at one pixel per point.
            // SAFETY: a null rect means "the image's own size"; no context or hints.
            unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
        })
        .ok_or_else(|| Error::Decode("the clipboard image could not be rasterized".into()))?;
    cg_image_to_image(&cg_image).map(Some)
}

/// The `CGImage` of the bitmap representation with the most pixels, which is the
/// image's full resolution regardless of its size in points.
fn largest_bitmap(image: &NSImage) -> Option<Retained<CGImage>> {
    image
        .representations()
        .iter()
        .filter_map(|rep| rep.downcast::<NSBitmapImageRep>().ok())
        .max_by_key(|rep| rep.pixelsWide().saturating_mul(rep.pixelsHigh()))
        .and_then(|rep| rep.CGImage())
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use objc2_app_kit::NSPasteboardTypeString;
    use objc2_foundation::ns_string;

    use super::*;

    /// A private, uniquely named pasteboard released on drop, so the tests exercise
    /// real `NSPasteboard` I/O without touching the user's clipboard or racing
    /// other test processes.
    struct ScratchPasteboard(Retained<NSPasteboard>);

    impl ScratchPasteboard {
        fn new() -> Self {
            Self(NSPasteboard::pasteboardWithUniqueName())
        }
    }

    impl Drop for ScratchPasteboard {
        fn drop(&mut self) {
            // objc2-app-kit does not bind `-releaseGlobally`.
            // SAFETY: `releaseGlobally` takes no arguments and returns void.
            let () = unsafe { objc2::msg_send![&*self.0, releaseGlobally] };
        }
    }

    /// An odd-sized image mixing opaque, translucent and transparent pixels.
    fn sample() -> Image {
        Image::from_fn(PhysicalSize::new(7, 5), |x, y| match (x + y) % 4 {
            0 => Rgba8::new(0xff, 0x80, 0x00, 0xff),
            1 => Rgba8::new(x as u8 * 30, y as u8 * 50, 0x40, 0xff),
            2 => Rgba8::new(0x37, 0x91, 0xc3, 0x55),
            _ => Rgba8::new(0, 0, 0, 0),
        })
    }

    #[test]
    fn written_image_reads_back_pixel_for_pixel() {
        let pasteboard = ScratchPasteboard::new();
        let image = sample();
        write_image_to(&pasteboard.0, &image).unwrap();
        assert_eq!(read_image_from(&pasteboard.0).unwrap(), Some(image));
    }

    #[test]
    fn written_image_offers_png_and_tiff_at_pixel_size() {
        let pasteboard = ScratchPasteboard::new();
        let image = sample();
        write_image_to(&pasteboard.0, &image).unwrap();
        // SAFETY: AppKit constants.
        let types = unsafe { [NSPasteboardTypePNG, NSPasteboardTypeTIFF] };
        for kind in types {
            let data = pasteboard
                .0
                .dataForType(kind)
                .unwrap_or_else(|| panic!("no {kind}"));
            let rep = NSBitmapImageRep::imageRepWithData(&data).unwrap();
            assert_eq!((rep.pixelsWide(), rep.pixelsHigh()), (7, 5), "{kind}");
            assert_eq!(rep.size(), NSSize::new(7.0, 5.0), "{kind}");
            let decoded = cg_image_to_image(&rep.CGImage().unwrap()).unwrap();
            assert_eq!(decoded, image, "{kind}");
        }
    }

    #[test]
    fn writing_replaces_the_previous_contents() {
        let pasteboard = ScratchPasteboard::new();
        // SAFETY: an AppKit constant.
        let string_type = unsafe { NSPasteboardTypeString };
        pasteboard.0.clearContents();
        pasteboard
            .0
            .setString_forType(ns_string!("old"), string_type);
        write_image_to(&pasteboard.0, &sample()).unwrap();
        assert!(pasteboard.0.stringForType(string_type).is_none());
    }

    #[test]
    fn reading_picks_the_largest_representation() {
        // A 2× image: 4×2 points backed by 4×2 and 8×4 pixel representations.
        let small = bitmap_rep(&Image::filled(
            PhysicalSize::new(4, 2),
            Rgba8::new(1, 2, 3, 255),
        ))
        .unwrap();
        let large = bitmap_rep(&Image::filled(
            PhysicalSize::new(8, 4),
            Rgba8::new(4, 5, 6, 255),
        ))
        .unwrap();
        large.setSize(NSSize::new(4.0, 2.0));
        let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(4.0, 2.0));
        image.addRepresentation(&small);
        image.addRepresentation(&large);
        let converted = cg_image_to_image(&largest_bitmap(&image).unwrap()).unwrap();
        assert_eq!(
            converted,
            Image::filled(PhysicalSize::new(8, 4), Rgba8::new(4, 5, 6, 255))
        );
    }

    #[test]
    fn pasteboard_without_an_image_reads_none() {
        let pasteboard = ScratchPasteboard::new();
        assert_eq!(read_image_from(&pasteboard.0).unwrap(), None);
        pasteboard.0.clearContents();
        // SAFETY: an AppKit constant.
        pasteboard
            .0
            .setString_forType(ns_string!("not an image"), unsafe {
                NSPasteboardTypeString
            });
        assert_eq!(read_image_from(&pasteboard.0).unwrap(), None);
    }

    #[test]
    fn empty_images_cannot_be_copied() {
        let pasteboard = ScratchPasteboard::new();
        let empty = Image::new(PhysicalSize::new(0, 3), Vec::new()).unwrap();
        assert!(matches!(
            write_image_to(&pasteboard.0, &empty),
            Err(Error::InvalidImage(_))
        ));
    }
}

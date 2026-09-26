//! Windows: the clipboard's image formats (the portable part of `clipboard.rs`).
//!
//! Chartreuse writes a registered `PNG` format (what Office, browsers, and image
//! editors read, with alpha) and `CF_DIBV5`: a packed device-independent bitmap,
//! which every other application reads, and from which Windows synthesizes
//! `CF_DIB` and `CF_BITMAP`. It reads either back, and `CF_DIB`.

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::codec::{decode_as, Format};

/// The standard `CF_DIB` clipboard format.
pub(super) const CF_DIB: u32 = 8;
/// The standard `CF_DIBV5` clipboard format.
pub(super) const CF_DIBV5: u32 = 17;

/// The formats to try reading, best first, from those on offer (in clipboard
/// order): `png` (the registered `PNG` format's id) wherever it is, then the first
/// of `CF_DIBV5` and `CF_DIB`. Windows lists the formats it synthesizes after the
/// ones an application wrote, so that is the bitmap the source actually provided.
pub(super) fn formats_to_read(offered: impl IntoIterator<Item = u32>, png: u32) -> Vec<u32> {
    let mut has_png = false;
    let mut bitmap = None;
    for format in offered {
        if format == png {
            has_png = true;
        } else if bitmap.is_none() && (format == CF_DIBV5 || format == CF_DIB) {
            bitmap = Some(format);
        }
    }
    has_png.then_some(png).into_iter().chain(bitmap).collect()
}

/// `BITMAPINFOHEADER`, `BITMAPV4HEADER`, and `BITMAPV5HEADER` sizes.
const INFO_HEADER: u32 = 40;
const V4_HEADER: u32 = 108;
const V5_HEADER: u32 = 124;
/// `BITMAPCOREHEADER`'s size.
const CORE_HEADER: u32 = 12;
/// `BITMAPV2INFOHEADER` and `BITMAPV3INFOHEADER` (Adobe) sizes.
const V2_HEADER: u32 = 52;
const V3_HEADER: u32 = 56;

/// `biCompression` values.
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

/// The masks of 32-bit little-endian BGRA pixels.
const RED_MASK: u32 = 0x00FF_0000;
const GREEN_MASK: u32 = 0x0000_FF00;
const BLUE_MASK: u32 = 0x0000_00FF;
const ALPHA_MASK: u32 = 0xFF00_0000;

/// `LCS_sRGB` (`'sRGB'`) and `LCS_GM_IMAGES`.
const LCS_SRGB: u32 = 0x7352_4742;
const LCS_GM_IMAGES: u32 = 4;

/// `image` as a `CF_DIBV5` packed DIB: a `BITMAPV5HEADER` for 32-bit sRGB pixels
/// with straight alpha (`BI_BITFIELDS`), the red, green, and blue masks again
/// (the documented layout puts them after the header even though the V5 header
/// holds them too), then bottom-up BGRA rows, the orientation readers handle
/// best.
///
/// # Errors
///
/// [`Error::InvalidImage`] if the image is empty or too large for a DIB.
pub(super) fn dibv5_from_image(image: &Image) -> Result<Vec<u8>> {
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
    // The header stores the size as signed 32-bit numbers.
    if i32::try_from(image.width()).is_err() || i32::try_from(image.height()).is_err() {
        return Err(invalid());
    }
    let pixel_bytes = image
        .width()
        .checked_mul(image.height())
        .and_then(|n| n.checked_mul(4));
    let pixel_bytes = pixel_bytes.ok_or_else(invalid)?;

    let mut dib = Vec::with_capacity(V5_HEADER as usize + 12 + image.pixels().len());
    let mut u32s = |values: &[u32]| {
        for value in values {
            dib.extend(value.to_le_bytes());
        }
    };
    u32s(&[V5_HEADER, image.width(), image.height()]);
    // Planes 1 and 32 bits per pixel, as one little-endian u32.
    u32s(&[1 | (32 << 16), BI_BITFIELDS, pixel_bytes]);
    // Resolution unspecified, no color table.
    u32s(&[0, 0, 0, 0]);
    u32s(&[RED_MASK, GREEN_MASK, BLUE_MASK, ALPHA_MASK, LCS_SRGB]);
    // Endpoints (a CIEXYZTRIPLE) and gamma, unused for sRGB.
    u32s(&[0; 12]);
    // Intent, then no profile, and the reserved field.
    u32s(&[LCS_GM_IMAGES, 0, 0, 0]);
    u32s(&[RED_MASK, GREEN_MASK, BLUE_MASK]);
    debug_assert_eq!(dib.len(), V5_HEADER as usize + 12);

    let row_bytes = image.width() as usize * 4;
    for row in image.pixels().chunks_exact(row_bytes).rev() {
        for &[r, g, b, a] in row.as_chunks::<4>().0 {
            dib.extend([b, g, r, a]);
        }
    }
    Ok(dib)
}

/// Decodes a `CF_DIB` or `CF_DIBV5` packed DIB (any header, bit depth, color
/// table, or orientation the BMP decoder supports) into a straight-alpha image.
///
/// A 32-bit DIB whose alpha is zero everywhere is treated as opaque: that is what
/// a screen bitmap (whose fourth byte is unused) looks like after Windows converts
/// it to a DIB with an alpha mask, and a fully transparent image is never what
/// the user copied.
///
/// # Errors
///
/// [`Error::Decode`] if the data is malformed or in an unsupported format.
pub(super) fn image_from_dib(dib: &[u8]) -> Result<Image> {
    let file = bmp_file(dib)
        .ok_or_else(|| Error::Decode("the clipboard bitmap has a malformed header".into()))?;
    let mut image = decode_as(&file, Format::Bmp)?;
    let pixels = image.pixels_mut();
    if pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = u8::MAX;
        }
    }
    Ok(image)
}

/// `dib` with a `BITMAPFILEHEADER` in front, making it a `.bmp` file. The file
/// header records where the pixels start: after the header, the color masks
/// (if they follow it), and the color table.
fn bmp_file(dib: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(dib.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let header = u32_at(0)?;
    let (bit_count, compression, colors_used, color_bytes) = match header {
        CORE_HEADER => (u16_at(10)?, BI_RGB, 0, 3),
        INFO_HEADER | V2_HEADER | V3_HEADER | V4_HEADER | V5_HEADER => {
            (u16_at(14)?, u32_at(16)?, u32_at(32)?, 4)
        }
        _ => return None,
    };
    let repeated_masks = || {
        let after = header as usize;
        dib.get(40..52)
            .is_some_and(|masks| dib.get(after..after + 12) == Some(masks))
    };
    let masks = match header {
        _ if compression != BI_BITFIELDS => 0,
        INFO_HEADER => 12,
        // V4 and V5 headers hold the masks, and should repeat them after the
        // header, but not every application does: they are there if they match.
        V4_HEADER | V5_HEADER if repeated_masks() => 12,
        _ => 0,
    };
    // Up to 8 bits per pixel, 0 colors used means all of them.
    let colors: u32 = match (bit_count, colors_used) {
        (1 | 2 | 4 | 8, 0) => 1 << bit_count,
        (_, colors) => colors,
    };
    let pixels = colors
        .checked_mul(color_bytes)?
        .checked_add(header)?
        .checked_add(masks)?;
    if pixels as usize > dib.len() {
        return None;
    }
    const FILE_HEADER: u32 = 14;
    let size = u32::try_from(dib.len()).ok()?.checked_add(FILE_HEADER)?;
    let mut file = Vec::with_capacity(size as usize);
    file.extend(b"BM");
    file.extend(size.to_le_bytes());
    file.extend([0; 4]);
    file.extend((FILE_HEADER + pixels).to_le_bytes());
    file.extend(dib);
    Some(file)
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;

    use super::*;

    fn sample() -> Image {
        Image::from_fn(PhysicalSize::new(3, 2), |x, y| {
            Rgba8::new(x as u8 * 80, y as u8 * 200, 7, [255, 128, 0][x as usize])
        })
    }

    /// A DIB with `header` (whose size field is set here), `extra` bytes after it
    /// (masks, a color table), and `pixels`.
    fn dib(mut header: Vec<u8>, extra: &[u8], pixels: &[u8]) -> Vec<u8> {
        let size = header.len() as u32;
        header[..4].copy_from_slice(&size.to_le_bytes());
        header.extend(extra);
        header.extend(pixels);
        header
    }

    /// A `BITMAPINFOHEADER`.
    fn info_header(width: i32, height: i32, bits: u16, compression: u32, colors: u32) -> Vec<u8> {
        let mut header = vec![0; 40];
        header[4..8].copy_from_slice(&width.to_le_bytes());
        header[8..12].copy_from_slice(&height.to_le_bytes());
        header[12..14].copy_from_slice(&1u16.to_le_bytes());
        header[14..16].copy_from_slice(&bits.to_le_bytes());
        header[16..20].copy_from_slice(&compression.to_le_bytes());
        header[32..36].copy_from_slice(&colors.to_le_bytes());
        header
    }

    fn masks(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn dibv5_round_trips_with_straight_alpha() {
        let image = sample();
        let dib = dibv5_from_image(&image).unwrap();
        assert_eq!(image_from_dib(&dib).unwrap(), image);
    }

    #[test]
    fn dibv5_is_a_bottom_up_32_bit_bitfields_bitmap_with_repeated_masks() {
        let image = sample();
        let dib = dibv5_from_image(&image).unwrap();
        let u32_at = |at: usize| u32::from_le_bytes(dib[at..at + 4].try_into().unwrap());
        assert_eq!(u32_at(0), 124);
        assert_eq!(
            (u32_at(4), u32_at(8)),
            (3, 2),
            "a positive height is bottom-up"
        );
        assert_eq!(u32_at(12), 1 | (32 << 16), "one plane, 32 bits per pixel");
        assert_eq!(u32_at(16), BI_BITFIELDS);
        assert_eq!(u32_at(20), 3 * 2 * 4);
        let header_masks = [u32_at(40), u32_at(44), u32_at(48), u32_at(52)];
        assert_eq!(header_masks, [RED_MASK, GREEN_MASK, BLUE_MASK, ALPHA_MASK]);
        assert_eq!(u32_at(56), LCS_SRGB);
        assert_eq!(
            [u32_at(124), u32_at(128), u32_at(132)],
            [RED_MASK, GREEN_MASK, BLUE_MASK]
        );
        // The first stored pixel is the bottom-left one, in BGRA order.
        let bottom_left = image.pixel(0, 1).unwrap();
        assert_eq!(
            dib[136..140],
            [bottom_left.b, bottom_left.g, bottom_left.r, bottom_left.a]
        );
        assert_eq!(dib.len(), 136 + 3 * 2 * 4);
    }

    #[test]
    fn empty_images_cannot_be_copied() {
        let empty = Image::new(PhysicalSize::new(0, 0), Vec::new()).unwrap();
        assert!(matches!(
            dibv5_from_image(&empty),
            Err(Error::InvalidImage(_))
        ));
    }

    #[test]
    fn reads_padded_bottom_up_24_bit_rows_as_opaque() {
        // 1×2: each 3-byte row is padded to 4. Bottom row blue, top row red.
        let pixels = [255, 0, 0, 0, 0, 0, 255, 0];
        let image = image_from_dib(&dib(info_header(1, 2, 24, BI_RGB, 0), &[], &pixels)).unwrap();
        assert_eq!(image.pixel(0, 0), Some(Rgba8::new(255, 0, 0, 255)));
        assert_eq!(image.pixel(0, 1), Some(Rgba8::new(0, 0, 255, 255)));
    }

    #[test]
    fn reads_top_down_bitfields_with_masks_after_an_info_header() {
        // 1×2, top-down, 32-bit RGBA byte order (masks unlike Chartreuse's own).
        let header = info_header(1, -2, 32, BI_BITFIELDS, 0);
        let order = masks(&[0x0000_00FF, 0x0000_FF00, 0x00FF_0000]);
        let pixels = [10, 20, 30, 40, 50, 60, 70, 80];
        let image = image_from_dib(&dib(header, &order, &pixels)).unwrap();
        // No alpha mask in a BITMAPINFOHEADER: opaque.
        assert_eq!(image.pixel(0, 0), Some(Rgba8::new(10, 20, 30, 255)));
        assert_eq!(image.pixel(0, 1), Some(Rgba8::new(50, 60, 70, 255)));
    }

    #[test]
    fn reads_a_v5_header_with_or_without_repeated_masks() {
        let image = sample();
        let with = dibv5_from_image(&image).unwrap();
        let mut without = with.clone();
        without.drain(124..136);
        assert_eq!(image_from_dib(&without).unwrap(), image);
        assert_eq!(image_from_dib(&with).unwrap(), image);
    }

    #[test]
    fn reads_past_the_color_table() {
        // 8 bits per pixel with a two-color table: 1×1 of color 1 (green).
        let header = info_header(1, 1, 8, BI_RGB, 2);
        let table = [0, 0, 255, 0, 0, 255, 0, 0];
        let image = image_from_dib(&dib(header, &table, &[1, 0, 0, 0])).unwrap();
        assert_eq!(image.pixel(0, 0), Some(Rgba8::new(0, 255, 0, 255)));
    }

    #[test]
    fn all_zero_alpha_means_opaque() {
        let transparent =
            Image::from_fn(PhysicalSize::new(2, 1), |x, _| Rgba8::new(x as u8, 2, 3, 0));
        let read = image_from_dib(&dibv5_from_image(&transparent).unwrap()).unwrap();
        assert_eq!(read.pixel(0, 0), Some(Rgba8::new(0, 2, 3, 255)));
        assert_eq!(read.pixel(1, 0), Some(Rgba8::new(1, 2, 3, 255)));

        // Any visible pixel keeps the alpha channel as it is.
        let partly = Image::from_fn(PhysicalSize::new(2, 1), |x, _| Rgba8::new(1, 2, 3, x as u8));
        let read = image_from_dib(&dibv5_from_image(&partly).unwrap()).unwrap();
        assert_eq!(read, partly);
    }

    #[test]
    fn malformed_bitmaps_are_decode_errors() {
        let decode_error = |dib: &[u8]| matches!(image_from_dib(dib), Err(Error::Decode(_)));
        assert!(decode_error(&[]));
        // An unknown header size.
        assert!(decode_error(&dib(vec![0; 20], &[], &[0; 16])));
        // A color table running past the end.
        assert!(decode_error(&dib(
            info_header(1, 1, 8, BI_RGB, 0),
            &[0; 8],
            &[]
        )));
        // Pixels missing.
        let header = info_header(4, 4, 32, BI_RGB, 0);
        assert!(decode_error(&dib(header, &[], &[0; 8])));
    }

    #[test]
    fn png_wins_then_the_first_bitmap_on_offer() {
        const PNG: u32 = 0xC0DE;
        const TEXT: u32 = 1;
        assert_eq!(
            formats_to_read([TEXT, CF_DIB, CF_DIBV5, PNG], PNG),
            [PNG, CF_DIB]
        );
        assert_eq!(formats_to_read([CF_DIBV5, CF_DIB], PNG), [CF_DIBV5]);
        assert_eq!(formats_to_read([PNG], PNG), [PNG]);
        assert_eq!(formats_to_read([TEXT], PNG), Vec::<u32>::new());
    }
}

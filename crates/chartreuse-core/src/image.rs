//! In-memory images.

use crate::color::Rgba8;
use crate::error::Error;
use crate::geometry::PhysicalSize;

/// An RGBA8 image with its size in physical pixels.
///
/// Pixels are stored row-major, top row first, 4 bytes per pixel in `r, g, b, a`
/// order, sRGB with straight (non-premultiplied) alpha, and no row padding: the
/// buffer is exactly `width × height × 4` bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    size: PhysicalSize,
    pixels: Vec<u8>,
}

/// The byte length of an RGBA8 buffer for `size`, or `None` on overflow.
fn buffer_len(size: PhysicalSize) -> Option<usize> {
    usize::try_from(size.width)
        .ok()?
        .checked_mul(usize::try_from(size.height).ok()?)?
        .checked_mul(4)
}

impl Image {
    /// Wraps an existing buffer, which must be exactly `width × height × 4` bytes.
    pub fn new(size: PhysicalSize, pixels: Vec<u8>) -> Result<Self, Error> {
        let expected = buffer_len(size).ok_or_else(|| {
            Error::InvalidImage(format!("{}×{} is too large", size.width, size.height))
        })?;
        if pixels.len() != expected {
            return Err(Error::InvalidImage(format!(
                "{}×{} RGBA8 needs {expected} bytes, got {}",
                size.width,
                size.height,
                pixels.len()
            )));
        }
        Ok(Self { size, pixels })
    }

    /// An image filled with one color.
    ///
    /// # Panics
    ///
    /// If the buffer size overflows `usize`.
    #[must_use]
    pub fn filled(size: PhysicalSize, color: Rgba8) -> Self {
        Self::from_fn(size, |_, _| color)
    }

    /// An image whose pixel at `(x, y)` is `f(x, y)`.
    ///
    /// # Panics
    ///
    /// If the buffer size overflows `usize`.
    #[must_use]
    pub fn from_fn(size: PhysicalSize, mut f: impl FnMut(u32, u32) -> Rgba8) -> Self {
        let len = buffer_len(size).expect("image buffer size overflows usize");
        let mut pixels = Vec::with_capacity(len);
        for y in 0..size.height {
            for x in 0..size.width {
                pixels.extend_from_slice(&f(x, y).to_array());
            }
        }
        Self { size, pixels }
    }

    #[must_use]
    pub const fn size(&self) -> PhysicalSize {
        self.size
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.size.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.size.height
    }

    /// The raw RGBA8 bytes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The raw RGBA8 bytes, mutably. The length cannot change.
    #[must_use]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Takes the raw RGBA8 bytes.
    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.size.width || y >= self.size.height {
            return None;
        }
        // In range: the full buffer length was checked at construction.
        Some((y as usize * self.size.width as usize + x as usize) * 4)
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba8> {
        let i = self.offset(x, y)?;
        let [r, g, b, a] = self.pixels[i..i + 4] else {
            unreachable!("offset is 4-byte aligned within the buffer")
        };
        Some(Rgba8::new(r, g, b, a))
    }

    /// Sets the pixel at `(x, y)`. Returns `false` (and changes nothing) outside the
    /// image.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Rgba8) -> bool {
        match self.offset(x, y) {
            Some(i) => {
                self.pixels[i..i + 4].copy_from_slice(&color.to_array());
                true
            }
            None => false,
        }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never dump megabytes of pixels into logs.
        f.debug_struct("Image")
            .field("width", &self.size.width)
            .field("height", &self.size.height)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_buffers_of_the_wrong_length() {
        let size = PhysicalSize::new(3, 2);
        assert!(Image::new(size, vec![0; 24]).is_ok());
        assert!(matches!(
            Image::new(size, vec![0; 23]),
            Err(Error::InvalidImage(_))
        ));
        assert!(matches!(
            Image::new(size, vec![0; 25]),
            Err(Error::InvalidImage(_))
        ));
    }

    #[test]
    fn new_rejects_sizes_that_overflow() {
        let size = PhysicalSize::new(u32::MAX, u32::MAX);
        assert!(matches!(
            Image::new(size, Vec::new()),
            Err(Error::InvalidImage(_))
        ));
    }

    #[test]
    fn pixels_are_row_major_rgba() {
        let image = Image::from_fn(PhysicalSize::new(2, 2), |x, y| {
            Rgba8::new(x as u8, y as u8, 7, 255)
        });
        assert_eq!(
            &image.pixels()[8..12],
            &[0, 1, 7, 255],
            "third pixel is (0, 1)"
        );
        assert_eq!(image.pixel(1, 1), Some(Rgba8::new(1, 1, 7, 255)));
    }

    #[test]
    fn out_of_bounds_access_is_rejected() {
        let mut image = Image::filled(PhysicalSize::new(2, 1), Rgba8::BLACK);
        assert_eq!(image.pixel(2, 0), None);
        assert_eq!(image.pixel(0, 1), None);
        assert!(!image.set_pixel(0, 1, Rgba8::WHITE));
        assert!(image.set_pixel(1, 0, Rgba8::WHITE));
        assert_eq!(image.pixel(1, 0), Some(Rgba8::WHITE));
        assert_eq!(image.pixel(0, 0), Some(Rgba8::BLACK));
    }
}

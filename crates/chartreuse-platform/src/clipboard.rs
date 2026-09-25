//! The system clipboard.

use chartreuse_core::image::Image;
use chartreuse_core::Result;

/// Reads and writes images on the system clipboard.
///
/// Call from the main thread (iced `update`).
pub trait Clipboard {
    /// Replaces the clipboard contents with `image`, in formats other applications
    /// can paste (PNG and TIFF on macOS).
    fn write_image(&self, image: &Image) -> Result<()>;

    /// The image on the clipboard, or `Ok(None)` if it holds no image.
    fn read_image(&self) -> Result<Option<Image>>;
}

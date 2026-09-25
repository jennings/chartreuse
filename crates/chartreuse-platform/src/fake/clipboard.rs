//! Fake [`Clipboard`]: an in-memory image slot.

use chartreuse_core::image::Image;
use chartreuse_core::Result;

use super::Fake;
use crate::clipboard::Clipboard;

impl Clipboard for Fake {
    fn write_image(&self, image: &Image) -> Result<()> {
        self.state.lock().clipboard = Some(image.clone());
        Ok(())
    }

    fn read_image(&self) -> Result<Option<Image>> {
        Ok(self.state.lock().clipboard.clone())
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;

    use super::*;

    #[test]
    fn empty_clipboard_reads_as_none_until_written() {
        let fake = Fake::new();
        assert_eq!(fake.read_image().unwrap(), None);
        let image = Image::filled(PhysicalSize::new(2, 2), Rgba8::WHITE);
        fake.write_image(&image).unwrap();
        assert_eq!(fake.read_image().unwrap(), Some(image));
    }
}

//! Windows: picking one image out of an `.ico` file, for the tray icon.
//!
//! An `.ico` is an `ICONDIR` header (reserved 0, type 1, entry count) followed by
//! 16-byte `ICONDIRENTRY`s, each pointing at one image's bytes. Those bytes are
//! what `CreateIconFromResourceEx` takes.

/// The tray icon of this build's flavor (see `assets/icon/README.md`).
pub(super) fn tray_icon() -> &'static [u8] {
    use chartreuse_core::flavor::Flavor;
    match Flavor::CURRENT {
        Flavor::Release => include_bytes!("../../../../assets/icon/generated/tray-release.ico"),
        Flavor::Development => {
            include_bytes!("../../../../assets/icon/generated/tray-development.ico")
        }
    }
}

/// One image of an `.ico`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Entry<'a> {
    /// Width and height in pixels.
    pub size: u32,
    /// The image data (a PNG, or a BMP without its file header).
    pub bytes: &'a [u8],
}

/// The entries of `ico`, or `None` if it is not a well-formed icon file.
pub(super) fn entries(ico: &[u8]) -> Option<Vec<Entry<'_>>> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    (0..usize::from(u16_at(4)?))
        .map(|index| {
            let entry = 6 + index * 16;
            // A stored width of 0 means 256.
            let width = match *ico.get(entry)? {
                0 => 256,
                width => u32::from(width),
            };
            let length = usize::try_from(u32_at(entry + 8)?).ok()?;
            let offset = usize::try_from(u32_at(entry + 12)?).ok()?;
            Some(Entry {
                size: width,
                bytes: ico.get(offset..offset.checked_add(length)?)?,
            })
        })
        .collect()
}

/// The entry to show at `wanted` pixels: the smallest one at least that large (so
/// Windows only ever scales down), else the largest.
pub(super) fn best_entry<'a>(entries: &[Entry<'a>], wanted: u32) -> Option<Entry<'a>> {
    entries
        .iter()
        .filter(|entry| entry.size >= wanted)
        .min_by_key(|entry| entry.size)
        .or_else(|| entries.iter().max_by_key(|entry| entry.size))
        .copied()
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_imaging::codec::{decode_as, Format};

    use super::*;

    #[test]
    fn the_tray_icon_has_a_png_for_every_small_icon_size() {
        let entries = entries(tray_icon()).expect("the tray icon parses");
        let sizes: Vec<u32> = entries.iter().map(|entry| entry.size).collect();
        // SM_CXSMICON from 100% to 300% scale.
        assert_eq!(sizes, [16, 20, 24, 32, 40, 48]);
        for entry in entries {
            let image = decode_as(entry.bytes, Format::Png).expect("each entry is a PNG");
            assert_eq!(image.size(), PhysicalSize::new(entry.size, entry.size));
        }
    }

    #[test]
    fn best_entry_scales_down_rather_than_up() {
        let data = [0u8; 4];
        let entry = |size| Entry { size, bytes: &data };
        let entries = [entry(16), entry(32), entry(24)];
        let pick = |wanted| best_entry(&entries, wanted).map(|entry| entry.size);
        assert_eq!(pick(16), Some(16));
        assert_eq!(pick(20), Some(24));
        assert_eq!(pick(25), Some(32));
        assert_eq!(pick(8), Some(16));
        assert_eq!(pick(64), Some(32));
        assert_eq!(best_entry(&[], 16), None);
    }

    #[test]
    fn malformed_icons_are_rejected() {
        assert_eq!(entries(&[]), None);
        // A cursor file (type 2), not an icon.
        assert_eq!(entries(&[0, 0, 2, 0, 0, 0]), None);
        // One entry whose image runs past the end of the file.
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend([16, 16, 0, 0, 1, 0, 32, 0]);
        ico.extend(100u32.to_le_bytes());
        ico.extend(22u32.to_le_bytes());
        ico.extend([0; 10]);
        assert_eq!(entries(&ico), None);
        // The same entry, now complete; a width byte of 0 means 256.
        ico.extend([0; 90]);
        assert_eq!(entries(&ico).unwrap()[0].size, 16);
        ico[6] = 0;
        assert_eq!(entries(&ico).unwrap()[0].size, 256);
    }
}

//! Reading the `Xft/DPI` setting from an XSETTINGS block, the
//! `_XSETTINGS_SETTINGS` property the desktop's settings daemon publishes
//! (GNOME, Xfce, Cinnamon, MATE, and KDE all run one).
//!
//! The format (freedesktop XSETTINGS specification): a byte-order byte, three
//! pad bytes, a serial, the number of settings, then each setting as its type,
//! a pad byte, a 16-bit name length, the name padded to 4 bytes, a serial, and
//! the value: a 32-bit integer, a length-prefixed string padded to 4 bytes, or
//! four 16-bit color channels.

/// The `Xft/DPI` setting in dots per inch, if `block` holds it as an integer.
/// XSETTINGS stores it in 1024ths of a DPI.
///
/// Parsing stops at the first malformed setting; a truncated block yields the
/// DPI only if it came before the damage.
#[must_use]
pub fn xft_dpi(block: &[u8]) -> Option<f64> {
    let mut reader = Reader::new(block)?;
    let count = reader.u32()?;
    for _ in 0..count {
        let kind = reader.bytes(1)?[0];
        reader.bytes(1)?;
        let name_len = usize::from(reader.u16()?);
        let name = reader.bytes(name_len)?;
        reader.pad(name_len)?;
        let _serial = reader.u32()?;
        match kind {
            0 => {
                let value = reader.u32()? as i32;
                if name == b"Xft/DPI" {
                    return (value > 0).then(|| f64::from(value) / 1024.0);
                }
            }
            1 => {
                let len = reader.u32()? as usize;
                reader.bytes(len)?;
                reader.pad(len)?;
            }
            2 => {
                reader.bytes(8)?;
            }
            _ => return None,
        }
    }
    None
}

struct Reader<'a> {
    bytes: &'a [u8],
    big_endian: bool,
}

impl<'a> Reader<'a> {
    fn new(block: &'a [u8]) -> Option<Self> {
        // The specification says 0 (LSBFirst) or 1 (MSBFirst); settings
        // daemons write `'l'` or `'B'`, as Xlib's byte-order bytes are.
        let big_endian = match *block.first()? {
            0 | b'l' => false,
            1 | b'B' => true,
            _ => cfg!(target_endian = "big"),
        };
        // Byte order, 3 pad bytes, and the serial.
        let bytes = block.get(8..)?;
        Some(Self { bytes, big_endian })
    }

    fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(len)?;
        self.bytes = rest;
        Some(taken)
    }

    /// Skips the padding after `len` bytes of data.
    fn pad(&mut self, len: usize) -> Option<()> {
        self.bytes(len.next_multiple_of(4) - len).map(drop)
    }

    fn u16(&mut self) -> Option<u16> {
        let bytes = self.bytes(2)?.try_into().ok()?;
        Some(if self.big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        })
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes = self.bytes(4)?.try_into().ok()?;
        Some(if self.big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a settings block in either byte order.
    struct Block {
        big_endian: bool,
        settings: Vec<u8>,
        count: u32,
    }

    impl Block {
        fn new(big_endian: bool) -> Self {
            Self {
                big_endian,
                settings: Vec::new(),
                count: 0,
            }
        }

        fn u16(&self, value: u16) -> [u8; 2] {
            if self.big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }
        }

        fn u32(&self, value: u32) -> [u8; 4] {
            if self.big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }
        }

        fn setting(mut self, kind: u8, name: &str, value: &[u8]) -> Self {
            let mut bytes = vec![kind, 0];
            bytes.extend(self.u16(name.len() as u16));
            bytes.extend(name.as_bytes());
            bytes.resize(bytes.len().next_multiple_of(4), 0);
            bytes.extend(self.u32(7));
            bytes.extend(value);
            self.settings.extend(bytes);
            self.count += 1;
            self
        }

        fn int(self, name: &str, value: i32) -> Self {
            let value = self.u32(value as u32);
            self.setting(0, name, &value)
        }

        fn string(self, name: &str, value: &str) -> Self {
            let mut bytes = self.u32(value.len() as u32).to_vec();
            bytes.extend(value.as_bytes());
            bytes.resize(bytes.len().next_multiple_of(4), 0);
            self.setting(1, name, &bytes)
        }

        fn build(&self) -> Vec<u8> {
            let order = if self.big_endian { b'B' } else { b'l' };
            let mut block = vec![order, 0, 0, 0];
            block.extend(self.u32(42));
            block.extend(self.u32(self.count));
            block.extend(&self.settings);
            block
        }
    }

    #[test]
    fn the_dpi_is_found_after_settings_of_every_type_in_either_byte_order() {
        for big_endian in [false, true] {
            let block = Block::new(big_endian)
                .string("Net/ThemeName", "Adwaita")
                .setting(2, "Gtk/Color", &[0; 8])
                .int("Xft/Antialias", 1)
                .int("Xft/DPI", 144 * 1024)
                .build();
            assert_eq!(xft_dpi(&block), Some(144.0), "big endian: {big_endian}");
        }
    }

    #[test]
    fn fractional_dpis_keep_their_fraction() {
        let block = Block::new(false).int("Xft/DPI", 98_304 + 512).build();
        assert_eq!(xft_dpi(&block), Some(96.5));
    }

    #[test]
    fn missing_unset_or_damaged_dpis_are_none() {
        assert_eq!(
            xft_dpi(&Block::new(false).int("Xft/Hinting", 1).build()),
            None
        );
        // Settings daemons write -1 for "not set".
        assert_eq!(xft_dpi(&Block::new(false).int("Xft/DPI", -1).build()), None);
        let mut truncated = Block::new(false).int("Xft/DPI", 96 * 1024).build();
        truncated.truncate(truncated.len() - 1);
        assert_eq!(xft_dpi(&truncated), None);
        assert_eq!(xft_dpi(&[]), None);
    }
}

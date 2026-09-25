//! Colors.

use std::fmt;
use std::str::FromStr;

/// An sRGB color with straight (non-premultiplied) 8-bit alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// An opaque color from a `0xRRGGBB` literal.
    #[must_use]
    pub const fn from_rgb_hex(hex: u32) -> Self {
        let [_, r, g, b] = hex.to_be_bytes();
        Self::rgb(r, g, b)
    }

    /// The color as `[r, g, b, a]`, the byte order of [`Image`](crate::image::Image) pixels.
    #[must_use]
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Formats as `#rrggbb`, or `#rrggbbaa` when not fully opaque.
impl fmt::Display for Rgba8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if self.a != 255 {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

/// The error returned when parsing an [`Rgba8`] fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color {0:?}: expected #rrggbb or #rrggbbaa")]
pub struct ParseColorError(String);

/// Parses `#rrggbb` or `#rrggbbaa` (case-insensitive).
impl FromStr for Rgba8 {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseColorError(s.to_owned());
        let hex = s.strip_prefix('#').ok_or_else(error)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| error());
        let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Self::new(byte(0)?, byte(2)?, byte(4)?, alpha))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_literal_splits_channels() {
        assert_eq!(Rgba8::from_rgb_hex(0xf0cc00), Rgba8::rgb(0xf0, 0xcc, 0x00));
    }

    #[test]
    fn display_and_parse_round_trip() {
        for color in [Rgba8::rgb(0x80, 0xff, 0x00), Rgba8::new(1, 2, 3, 4)] {
            assert_eq!(color.to_string().parse::<Rgba8>(), Ok(color));
        }
        assert_eq!(Rgba8::rgb(0x80, 0xff, 0x00).to_string(), "#80ff00");
        assert_eq!(
            "#F0CC00".parse::<Rgba8>(),
            Ok(Rgba8::from_rgb_hex(0xf0cc00))
        );
    }

    #[test]
    fn parse_rejects_malformed_input() {
        for input in [
            "f0cc00",
            "#f0cc0",
            "#f0cc0000ff",
            "#gg0000",
            "#",
            "",
            "#+1+2+3",
        ] {
            assert!(
                input.parse::<Rgba8>().is_err(),
                "{input:?} should not parse"
            );
        }
    }
}

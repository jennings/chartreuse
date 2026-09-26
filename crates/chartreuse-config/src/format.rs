//! The image format captures are saved in.

use std::fmt;

use chartreuse_imaging::Format;
use serde::{Deserialize, Serialize};

/// A format Chartreuse can save captures in: the encodable subset of
/// [`chartreuse_imaging::Format`].
///
/// In the settings file it is `"png"`, `"jpeg"` (or `"jpg"`) or `"webp"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveFormat {
    #[default]
    Png,
    #[serde(alias = "jpg")]
    Jpeg,
    WebP,
}

impl SaveFormat {
    /// Every save format, in the order a settings window lists them.
    pub const ALL: [Self; 3] = [Self::Png, Self::Jpeg, Self::WebP];

    /// The codec format that encodes it.
    #[must_use]
    pub const fn format(self) -> Format {
        match self {
            Self::Png => Format::Png,
            Self::Jpeg => Format::Jpeg,
            Self::WebP => Format::WebP,
        }
    }

    /// The extension of new files, without the dot, such as `"png"`.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        self.format().extension()
    }
}

/// The format's usual name, such as `PNG`.
impl fmt::Display for SaveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.format().name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_formats_are_exactly_the_encodable_ones() {
        let formats: Vec<Format> = SaveFormat::ALL.iter().map(|f| f.format()).collect();
        assert_eq!(formats, Format::ENCODABLE);
    }
}

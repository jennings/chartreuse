//! The build flavor: development or release.
//!
//! The flavor is fixed at compile time by the `release-flavor` cargo feature of this
//! crate (forwarded by the app crate's feature of the same name, and enabled only by
//! `cargo xtask release`). It is independent of the Cargo profile: an optimized
//! local build is still a development build.
//!
//! The flavor determines the bundle identifier, the display name, and the accent
//! color, so development and release builds can be installed side by side with
//! separate permissions and settings.

use crate::color::Rgba8;

/// A build flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Development,
    Release,
}

impl Flavor {
    /// The flavor this crate was compiled as.
    pub const CURRENT: Self = if cfg!(feature = "release-flavor") {
        Self::Release
    } else {
        Self::Development
    };

    /// The reverse-DNS bundle identifier. The configuration directory is derived
    /// from it.
    #[must_use]
    pub const fn bundle_id(self) -> &'static str {
        match self {
            Self::Development => "io.jennings.chartreuse.dev",
            Self::Release => "io.jennings.chartreuse",
        }
    }

    /// The user-facing application name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Development => "Chartreuse Dev",
            Self::Release => "Chartreuse",
        }
    }

    /// The UI accent color that identifies the flavor at a glance.
    #[must_use]
    pub const fn accent(self) -> Rgba8 {
        match self {
            Self::Development => Rgba8::from_rgb_hex(0xf0cc00),
            Self::Release => Rgba8::from_rgb_hex(0x80ff00),
        }
    }
}

/// [`Flavor::CURRENT`]'s bundle identifier.
pub const BUNDLE_ID: &str = Flavor::CURRENT.bundle_id();
/// [`Flavor::CURRENT`]'s display name.
pub const DISPLAY_NAME: &str = Flavor::CURRENT.display_name();
/// [`Flavor::CURRENT`]'s accent color.
pub const ACCENT: Rgba8 = Flavor::CURRENT.accent();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavors_have_the_documented_identities() {
        assert_eq!(
            Flavor::Development.bundle_id(),
            "io.jennings.chartreuse.dev"
        );
        assert_eq!(Flavor::Development.display_name(), "Chartreuse Dev");
        assert_eq!(Flavor::Development.accent().to_string(), "#f0cc00");
        assert_eq!(Flavor::Release.bundle_id(), "io.jennings.chartreuse");
        assert_eq!(Flavor::Release.display_name(), "Chartreuse");
        assert_eq!(Flavor::Release.accent().to_string(), "#80ff00");
    }
}

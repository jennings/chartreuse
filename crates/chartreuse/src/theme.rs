//! The iced theme, built around the flavor's accent color.

use chartreuse_core::color::Rgba8;
use chartreuse_core::flavor;
use iced::theme::Palette;
use iced::{Color, Theme};

/// Converts a core color to an iced color.
#[must_use]
pub fn to_iced(color: Rgba8) -> Color {
    Color::from_rgba8(color.r, color.g, color.b, f32::from(color.a) / 255.0)
}

/// A dark theme whose primary color (buttons, selections, focus) is `accent`.
#[must_use]
pub fn theme(accent: Rgba8) -> Theme {
    Theme::custom(
        flavor::DISPLAY_NAME,
        Palette {
            primary: to_iced(accent),
            ..Palette::DARK
        },
    )
}

#[cfg(test)]
mod tests {
    use chartreuse_core::flavor::Flavor;

    use super::*;

    #[test]
    fn primary_color_is_the_accent() {
        for flavor in [Flavor::Development, Flavor::Release] {
            let accent = flavor.accent();
            let primary = theme(accent).palette().primary.into_rgba8();
            assert_eq!(primary, accent.to_array(), "{flavor:?}");
        }
    }
}

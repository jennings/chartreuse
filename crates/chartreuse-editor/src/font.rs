//! The annotation font: Inter Bold, bundled so text annotations look the same
//! on every machine, in the editor and in exported images.
//!
//! The font file and its license (SIL Open Font License 1.1) live in
//! `crates/chartreuse-editor/assets/fonts/`.
//!
//! # Loading
//!
//! [`load`] registers the font with iced's global text system: the one
//! cosmic-text `FontSystem` that iced's renderers, canvas text, and [`layout`]
//! all share. It is synchronous and idempotent, and
//! [`Editor::new`](crate::Editor::new) calls it ([`measure`] and [`caret`] do
//! too), so an app that shows an editor needs no wiring of its own. Passing
//! [`BYTES`] to `iced::daemon(..).font(..)` as well is harmless: iced skips
//! bytes it has already loaded.
//!
//! `load` also drops any other copy of the same face from that font system
//! (for example an installed Inter Bold of another version), so text
//! annotations always use exactly the bundled file.
//!
//! # Layout
//!
//! Text annotations use only [`FONT`], plus cosmic-text's per-script system
//! fallback for glyphs Inter lacks. Every piece of code that lays out
//! annotation text (the canvas, measurements reported to the model, and
//! flattening) goes through [`layout`], which fixes the parameters:
//!
//! - font [`FONT`]: family [`FAMILY`], weight bold (700), normal style and
//!   stretch, [`Shaping::Advanced`] (full shaping and font fallback);
//! - `Metrics::new(font_size, font_size × Text::LINE_HEIGHT)`, where
//!   `font_size` is the annotation's [`Style::font_size`] in document units;
//! - no wrapping and no width limit: lines break only at `\n`;
//! - left-aligned (cosmic-text's default alignment, which puts right-to-left
//!   paragraphs flush right within the widest line, as iced does).
//!
//! Each line is one `line_height` tall, stacked from the layout box's top
//! edge; cosmic-text puts a line's baseline at `LayoutRun::line_y`, which is
//! `(line_height − (ascent + descent)) / 2 + ascent` below the line's top, with
//! the ascent and descent of the line's fonts at `font_size`. The layout box's
//! top-left corner is the annotation's [`Text::position`].
//!
//! [`Style::font_size`]: crate::model::Style::font_size
//! [`Text::position`]: crate::model::Text::position

use std::borrow::Cow;
use std::sync::{Once, PoisonError};

use iced::advanced::graphics::text::cosmic_text::{
    fontdb, Attrs, Buffer, Family, FontSystem, Metrics, Shaping,
};
use iced::advanced::graphics::text::font_system;
use iced::font;
use iced::Font;

use crate::model::{Size, Text, Vector};

/// The bundled font file (Inter 4.1, Bold).
pub const BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");

/// The bundled font's license (SIL Open Font License 1.1), which must travel
/// with redistributed copies of the font.
pub const LICENSE: &str = include_str!("../assets/fonts/Inter-LICENSE.txt");

/// The family name inside [`BYTES`].
pub const FAMILY: &str = "Inter";

/// The iced font for annotation text: [`FAMILY`] in bold.
pub const FONT: Font = Font {
    family: font::Family::Name(FAMILY),
    weight: font::Weight::Bold,
    stretch: font::Stretch::Normal,
    style: font::Style::Normal,
};

/// The face within the family that annotations use; must agree with [`FONT`].
const WEIGHT: fontdb::Weight = fontdb::Weight::BOLD;

/// Registers the bundled font with iced's global font system and removes any
/// competing copy of the same face. Safe to call any number of times, from any
/// thread; only the first call does work.
pub fn load() {
    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        let mut system = font_system()
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        system.load_font(Cow::Borrowed(BYTES));
        remove_other_copies(system.raw().db_mut());
    });
}

/// Removes every face that a query for [`FONT`] would weigh equally with the
/// bundled one (same family, weight, style, and stretch) unless its data is
/// byte-identical to [`BYTES`], so the bundled face is the one chosen.
fn remove_other_copies(db: &mut fontdb::Database) {
    let competitors: Vec<fontdb::ID> = db
        .faces()
        .filter(|face| {
            face.weight == WEIGHT
                && face.style == fontdb::Style::Normal
                && face.stretch == fontdb::Stretch::Normal
                && face.families.iter().any(|(name, _)| name == FAMILY)
        })
        .map(|face| face.id)
        .collect();
    for id in competitors {
        let bundled = db
            .with_face_data(id, |data, _| data == BYTES)
            .unwrap_or(false);
        if !bundled {
            db.remove_face(id);
        }
    }
}

/// The cosmic-text attributes for annotation text.
#[must_use]
pub fn attrs() -> Attrs<'static> {
    Attrs::new().family(Family::Name(FAMILY)).weight(WEIGHT)
}

/// The cosmic-text metrics for annotation text of `font_size`.
#[must_use]
pub fn metrics(font_size: f32) -> Metrics {
    let font_size = font_size.max(f32::MIN_POSITIVE);
    Metrics::new(font_size, font_size * Text::LINE_HEIGHT)
}

/// Lays out annotation text exactly as the editor draws it (see
/// [Layout](self#layout)), in document units with the origin at the text's
/// position. Call [`load`] first when using iced's font system.
#[must_use]
pub fn layout(font_system: &mut FontSystem, content: &str, font_size: f32) -> Buffer {
    let mut buffer = Buffer::new(font_system, metrics(font_size));
    buffer.set_text(font_system, content, &attrs(), Shaping::Advanced, None);
    let (size, rtl) = extent(&buffer);
    if rtl {
        // Like iced: align right-to-left paragraphs within the widest line.
        buffer.set_size(font_system, Some(size.width), Some(size.height));
    }
    buffer
}

/// The laid-out size: the widest line by the total line height.
fn extent(buffer: &Buffer) -> (Size, bool) {
    buffer
        .layout_runs()
        .fold((Size::default(), false), |(size, rtl), run| {
            (
                Size::new(size.width.max(run.line_w), size.height + run.line_height),
                rtl || run.rtl,
            )
        })
}

/// Runs `f` with iced's global font system, the bundled font loaded.
fn with_font_system<T>(f: impl FnOnce(&mut FontSystem) -> T) -> T {
    load();
    let mut system = font_system()
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    f(system.raw())
}

/// The layout size of `content` at `font_size`: the widest line's advance by
/// the number of lines × the line height. This is what the editor reports to
/// the model with [`Document::set_text_size`](crate::model::Document::set_text_size).
#[must_use]
pub fn measure(content: &str, font_size: f32) -> Size {
    with_font_system(|system| extent(&layout(system, content, font_size)).0)
}

/// Where a caret after the last character of `content` goes, relative to the
/// text's position: the top of its line box, at the end of the last line.
#[must_use]
pub fn caret(content: &str, font_size: f32) -> Vector {
    with_font_system(|system| {
        let buffer = layout(system, content, font_size);
        buffer.layout_runs().last().map_or(Vector::ZERO, |run| {
            let end = if run.rtl { 0.0 } else { run.line_w };
            Vector::new(end, run.line_top)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_face(db: &fontdb::Database) -> Option<fontdb::ID> {
        db.query(&fontdb::Query {
            families: &[Family::Name(FAMILY)],
            weight: WEIGHT,
            ..fontdb::Query::default()
        })
    }

    #[test]
    fn the_bundled_file_is_the_face_the_font_names() {
        let mut db = fontdb::Database::new();
        db.load_font_data(BYTES.to_vec());
        let face = db.faces().next().expect("the font parses");
        assert!(face.families.iter().any(|(name, _)| name == FAMILY));
        assert_eq!(face.weight, WEIGHT);
        assert_eq!(face.style, fontdb::Style::Normal);
        assert_eq!(face.weight.0, 700, "FONT asks for Weight::Bold");
    }

    #[test]
    fn other_copies_of_the_face_lose_to_the_bundled_one() {
        // An "installed" Inter Bold that differs from the bundled file (here by
        // trailing padding, which font parsers ignore), loaded first as system
        // fonts are, so it would win the query.
        let path =
            std::env::temp_dir().join(format!("chartreuse-font-test-{}.ttf", std::process::id()));
        let mut other = BYTES.to_vec();
        other.extend_from_slice(&[0; 4]);
        std::fs::write(&path, other).expect("write the test font");

        let mut db = fontdb::Database::new();
        db.load_font_file(&path).expect("load the test font");
        db.load_font_data(BYTES.to_vec());
        let is_bundled =
            |db: &fontdb::Database, id| db.with_face_data(id, |data, _| data == BYTES).unwrap();
        let before = bundled_face(&db).unwrap();
        assert!(!is_bundled(&db, before), "precondition: the copy wins");

        remove_other_copies(&mut db);
        let _ = std::fs::remove_file(&path);

        let after = bundled_face(&db).unwrap();
        assert!(is_bundled(&db, after));
        assert_eq!(db.len(), 1);
    }

    #[test]
    fn height_is_one_line_height_per_line() {
        let size = 20.0;
        let line = size * Text::LINE_HEIGHT;
        assert_eq!(measure("Hello", size).height, line);
        assert_eq!(measure("Hello\nworld", size).height, 2.0 * line);
        // A trailing newline starts an empty last line; empty text is one line.
        assert_eq!(measure("Hello\n", size).height, 2.0 * line);
        assert_eq!(measure("", size), Size::new(0.0, line));
    }

    #[test]
    fn width_is_the_widest_line_and_scales_with_font_size() {
        let narrow = measure("iii", 24.0).width;
        let wide = measure("WWW", 24.0).width;
        assert!(0.0 < narrow && narrow < wide, "{narrow} vs {wide}");
        assert_eq!(measure("WWW\niii", 24.0).width, wide);
        let double = measure("WWW", 48.0).width;
        assert!((double - 2.0 * wide).abs() < 0.5, "{double} vs 2 × {wide}");
    }

    #[test]
    fn the_caret_follows_the_last_line() {
        let size = 20.0;
        let line = size * Text::LINE_HEIGHT;
        assert_eq!(caret("", size), Vector::ZERO);
        let end = caret("abc\nde", size);
        assert_eq!(end, Vector::new(measure("de", size).width, line));
        assert_eq!(caret("abc\n", size), Vector::new(0.0, line));
    }
}

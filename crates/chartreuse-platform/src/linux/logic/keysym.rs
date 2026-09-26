//! X keysyms for [`Key`]s: the numeric values X11 key grabs look up, and
//! their xkb names.
//!
//! Chartreuse keys name positions on a US layout; each maps to the keysym that
//! position produces unshifted there (`Key::Minus` is `minus`, `Key::A` is `a`).

use chartreuse_core::hotkey::Key;

/// The keysym value and xkb name `key` produces.
#[must_use]
pub const fn keysym(key: Key) -> (u32, &'static str) {
    match key {
        Key::A => (0x61, "a"),
        Key::B => (0x62, "b"),
        Key::C => (0x63, "c"),
        Key::D => (0x64, "d"),
        Key::E => (0x65, "e"),
        Key::F => (0x66, "f"),
        Key::G => (0x67, "g"),
        Key::H => (0x68, "h"),
        Key::I => (0x69, "i"),
        Key::J => (0x6a, "j"),
        Key::K => (0x6b, "k"),
        Key::L => (0x6c, "l"),
        Key::M => (0x6d, "m"),
        Key::N => (0x6e, "n"),
        Key::O => (0x6f, "o"),
        Key::P => (0x70, "p"),
        Key::Q => (0x71, "q"),
        Key::R => (0x72, "r"),
        Key::S => (0x73, "s"),
        Key::T => (0x74, "t"),
        Key::U => (0x75, "u"),
        Key::V => (0x76, "v"),
        Key::W => (0x77, "w"),
        Key::X => (0x78, "x"),
        Key::Y => (0x79, "y"),
        Key::Z => (0x7a, "z"),
        Key::Digit0 => (0x30, "0"),
        Key::Digit1 => (0x31, "1"),
        Key::Digit2 => (0x32, "2"),
        Key::Digit3 => (0x33, "3"),
        Key::Digit4 => (0x34, "4"),
        Key::Digit5 => (0x35, "5"),
        Key::Digit6 => (0x36, "6"),
        Key::Digit7 => (0x37, "7"),
        Key::Digit8 => (0x38, "8"),
        Key::Digit9 => (0x39, "9"),
        Key::F1 => (0xffbe, "F1"),
        Key::F2 => (0xffbf, "F2"),
        Key::F3 => (0xffc0, "F3"),
        Key::F4 => (0xffc1, "F4"),
        Key::F5 => (0xffc2, "F5"),
        Key::F6 => (0xffc3, "F6"),
        Key::F7 => (0xffc4, "F7"),
        Key::F8 => (0xffc5, "F8"),
        Key::F9 => (0xffc6, "F9"),
        Key::F10 => (0xffc7, "F10"),
        Key::F11 => (0xffc8, "F11"),
        Key::F12 => (0xffc9, "F12"),
        Key::F13 => (0xffca, "F13"),
        Key::F14 => (0xffcb, "F14"),
        Key::F15 => (0xffcc, "F15"),
        Key::F16 => (0xffcd, "F16"),
        Key::F17 => (0xffce, "F17"),
        Key::F18 => (0xffcf, "F18"),
        Key::F19 => (0xffd0, "F19"),
        Key::F20 => (0xffd1, "F20"),
        Key::Space => (0x20, "space"),
        Key::Tab => (0xff09, "Tab"),
        Key::Return => (0xff0d, "Return"),
        Key::Escape => (0xff1b, "Escape"),
        Key::Backspace => (0xff08, "BackSpace"),
        Key::Delete => (0xffff, "Delete"),
        Key::Insert => (0xff63, "Insert"),
        Key::Home => (0xff50, "Home"),
        Key::End => (0xff57, "End"),
        Key::PageUp => (0xff55, "Prior"),
        Key::PageDown => (0xff56, "Next"),
        Key::Left => (0xff51, "Left"),
        Key::Right => (0xff53, "Right"),
        Key::Up => (0xff52, "Up"),
        Key::Down => (0xff54, "Down"),
        Key::Minus => (0x2d, "minus"),
        Key::Equal => (0x3d, "equal"),
        Key::LeftBracket => (0x5b, "bracketleft"),
        Key::RightBracket => (0x5d, "bracketright"),
        Key::Backslash => (0x5c, "backslash"),
        Key::Semicolon => (0x3b, "semicolon"),
        Key::Quote => (0x27, "apostrophe"),
        Key::Comma => (0x2c, "comma"),
        Key::Period => (0x2e, "period"),
        Key::Slash => (0x2f, "slash"),
        Key::Grave => (0x60, "grave"),
        Key::PrintScreen => (0xff61, "Print"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_key_has_a_distinct_keysym_and_name() {
        let values: HashSet<u32> = Key::ALL.iter().map(|&key| keysym(key).0).collect();
        let names: HashSet<&str> = Key::ALL.iter().map(|&key| keysym(key).1).collect();
        assert_eq!(values.len(), Key::ALL.len());
        assert_eq!(names.len(), Key::ALL.len());
    }

    #[test]
    fn printable_keys_use_their_latin1_code_points() {
        // Latin-1 keysyms equal the character's code point.
        for (key, character) in [
            (Key::A, 'a'),
            (Key::Z, 'z'),
            (Key::Digit0, '0'),
            (Key::Digit9, '9'),
            (Key::Space, ' '),
            (Key::Grave, '`'),
            (Key::Quote, '\''),
        ] {
            assert_eq!(keysym(key).0, u32::from(character), "{key:?}");
        }
    }

    #[test]
    fn function_keys_are_consecutive() {
        let f = |n: usize| keysym(Key::from_name(&format!("F{n}")).unwrap()).0;
        assert_eq!(f(1), 0xffbe);
        for n in 1..20 {
            assert_eq!(f(n + 1), f(n) + 1, "F{}", n + 1);
        }
    }
}

//! Windows: mapping [`Hotkey`]s to `RegisterHotKey` arguments (the portable part
//! of `hotkeys.rs`).
//!
//! [`Key`] names a key by its position on a US keyboard. Keys whose meaning does
//! not depend on the layout (function, editing and navigation keys) map to a
//! fixed virtual-key code. Character keys (letters, digits, punctuation) map to
//! their scan code: `MapVirtualKeyW` turns it into the current layout's
//! virtual-key code for that position, so a hotkey stays on the same physical key
//! whatever the layout, as on macOS. The US virtual-key code is the fallback
//! when the layout has no key there.

use chartreuse_core::hotkey::{Hotkey, Key, Modifiers};

/// `MOD_ALT`.
const MOD_ALT: u32 = 0x0001;
/// `MOD_CONTROL`.
const MOD_CONTROL: u32 = 0x0002;
/// `MOD_SHIFT`.
const MOD_SHIFT: u32 = 0x0004;
/// `MOD_WIN`.
const MOD_WIN: u32 = 0x0008;
/// `MOD_NOREPEAT`: holding the combination down does not repeat the press.
const MOD_NOREPEAT: u32 = 0x4000;

/// How to find the virtual-key code of a [`Key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyCode {
    /// A layout-independent virtual-key code.
    Fixed(u16),
    /// A character key: its set-1 scan code, and its virtual-key code on a US
    /// layout.
    Positional { scan: u16, us: u16 },
}

/// The `RegisterHotKey` modifier flags for `hotkey`, including `MOD_NOREPEAT`.
/// `Super` is the Windows key.
pub(super) fn modifier_flags(hotkey: Hotkey) -> u32 {
    [
        (Modifiers::CONTROL, MOD_CONTROL),
        (Modifiers::ALT, MOD_ALT),
        (Modifiers::SHIFT, MOD_SHIFT),
        (Modifiers::SUPER, MOD_WIN),
    ]
    .into_iter()
    .filter(|&(modifier, _)| hotkey.modifiers.contains(modifier))
    .fold(MOD_NOREPEAT, |flags, (_, flag)| flags | flag)
}

/// Where the virtual-key code of `key` comes from (see the module docs).
pub(super) fn key_code(key: Key) -> KeyCode {
    use KeyCode::{Fixed, Positional};
    let positional = |scan, us: u8| Positional {
        scan,
        us: u16::from(us),
    };
    match key {
        // Letters: `VK_A`..`VK_Z` are the ASCII capitals.
        Key::Q => positional(0x10, b'Q'),
        Key::W => positional(0x11, b'W'),
        Key::E => positional(0x12, b'E'),
        Key::R => positional(0x13, b'R'),
        Key::T => positional(0x14, b'T'),
        Key::Y => positional(0x15, b'Y'),
        Key::U => positional(0x16, b'U'),
        Key::I => positional(0x17, b'I'),
        Key::O => positional(0x18, b'O'),
        Key::P => positional(0x19, b'P'),
        Key::A => positional(0x1E, b'A'),
        Key::S => positional(0x1F, b'S'),
        Key::D => positional(0x20, b'D'),
        Key::F => positional(0x21, b'F'),
        Key::G => positional(0x22, b'G'),
        Key::H => positional(0x23, b'H'),
        Key::J => positional(0x24, b'J'),
        Key::K => positional(0x25, b'K'),
        Key::L => positional(0x26, b'L'),
        Key::Z => positional(0x2C, b'Z'),
        Key::X => positional(0x2D, b'X'),
        Key::C => positional(0x2E, b'C'),
        Key::V => positional(0x2F, b'V'),
        Key::B => positional(0x30, b'B'),
        Key::N => positional(0x31, b'N'),
        Key::M => positional(0x32, b'M'),
        // Digits: `VK_0`..`VK_9` are the ASCII digits.
        Key::Digit1 => positional(0x02, b'1'),
        Key::Digit2 => positional(0x03, b'2'),
        Key::Digit3 => positional(0x04, b'3'),
        Key::Digit4 => positional(0x05, b'4'),
        Key::Digit5 => positional(0x06, b'5'),
        Key::Digit6 => positional(0x07, b'6'),
        Key::Digit7 => positional(0x08, b'7'),
        Key::Digit8 => positional(0x09, b'8'),
        Key::Digit9 => positional(0x0A, b'9'),
        Key::Digit0 => positional(0x0B, b'0'),
        // Punctuation: the `VK_OEM_*` codes of a US layout.
        Key::Minus => positional(0x0C, 0xBD),
        Key::Equal => positional(0x0D, 0xBB),
        Key::LeftBracket => positional(0x1A, 0xDB),
        Key::RightBracket => positional(0x1B, 0xDD),
        Key::Semicolon => positional(0x27, 0xBA),
        Key::Quote => positional(0x28, 0xDE),
        Key::Grave => positional(0x29, 0xC0),
        Key::Backslash => positional(0x2B, 0xDC),
        Key::Comma => positional(0x33, 0xBC),
        Key::Period => positional(0x34, 0xBE),
        Key::Slash => positional(0x35, 0xBF),
        // `VK_F1`..`VK_F20` are consecutive.
        Key::F1 => Fixed(0x70),
        Key::F2 => Fixed(0x71),
        Key::F3 => Fixed(0x72),
        Key::F4 => Fixed(0x73),
        Key::F5 => Fixed(0x74),
        Key::F6 => Fixed(0x75),
        Key::F7 => Fixed(0x76),
        Key::F8 => Fixed(0x77),
        Key::F9 => Fixed(0x78),
        Key::F10 => Fixed(0x79),
        Key::F11 => Fixed(0x7A),
        Key::F12 => Fixed(0x7B),
        Key::F13 => Fixed(0x7C),
        Key::F14 => Fixed(0x7D),
        Key::F15 => Fixed(0x7E),
        Key::F16 => Fixed(0x7F),
        Key::F17 => Fixed(0x80),
        Key::F18 => Fixed(0x81),
        Key::F19 => Fixed(0x82),
        Key::F20 => Fixed(0x83),
        Key::Space => Fixed(0x20),
        Key::Tab => Fixed(0x09),
        Key::Return => Fixed(0x0D),
        Key::Escape => Fixed(0x1B),
        Key::Backspace => Fixed(0x08),
        Key::Delete => Fixed(0x2E),
        Key::Insert => Fixed(0x2D),
        Key::Home => Fixed(0x24),
        Key::End => Fixed(0x23),
        Key::PageUp => Fixed(0x21),
        Key::PageDown => Fixed(0x22),
        Key::Left => Fixed(0x25),
        Key::Up => Fixed(0x26),
        Key::Right => Fixed(0x27),
        Key::Down => Fixed(0x28),
        Key::PrintScreen => Fixed(0x2C),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_key_has_a_distinct_code() {
        let mut vks = HashSet::new();
        let mut scans = HashSet::new();
        for &key in Key::ALL {
            let vk = match key_code(key) {
                KeyCode::Fixed(vk) => vk,
                KeyCode::Positional { scan, us } => {
                    assert!(scans.insert(scan), "{key:?} shares scan code {scan:#x}");
                    us
                }
            };
            assert!(vks.insert(vk), "{key:?} shares virtual key {vk:#x}");
        }
    }

    #[test]
    fn letters_and_digits_use_their_ascii_virtual_keys() {
        for &key in Key::ALL {
            let name = key.name();
            if let [c] = name.as_bytes()
                && c.is_ascii_alphanumeric()
            {
                assert!(
                    matches!(key_code(key), KeyCode::Positional { us, .. } if us == u16::from(*c)),
                    "{key:?}"
                );
            }
        }
    }

    #[test]
    fn scan_codes_follow_the_us_rows() {
        // The top letter row runs Q (0x10) to P (0x19), the home row A (0x1E) to
        // L (0x26), the bottom row Z (0x2C) to M (0x32).
        let rows: [(&[Key], u16); 3] = [
            (
                &[
                    Key::Q,
                    Key::W,
                    Key::E,
                    Key::R,
                    Key::T,
                    Key::Y,
                    Key::U,
                    Key::I,
                    Key::O,
                    Key::P,
                ],
                0x10,
            ),
            (
                &[
                    Key::A,
                    Key::S,
                    Key::D,
                    Key::F,
                    Key::G,
                    Key::H,
                    Key::J,
                    Key::K,
                    Key::L,
                ],
                0x1E,
            ),
            (
                &[Key::Z, Key::X, Key::C, Key::V, Key::B, Key::N, Key::M],
                0x2C,
            ),
        ];
        for (keys, first) in rows {
            for (offset, &key) in (0..).zip(keys) {
                assert!(
                    matches!(key_code(key), KeyCode::Positional { scan, .. } if scan == first + offset),
                    "{key:?}"
                );
            }
        }
    }

    #[test]
    fn modifiers_map_to_register_hotkey_flags_without_repeat() {
        let flags = |modifiers| modifier_flags(Hotkey::new(modifiers, Key::A));
        assert_eq!(flags(Modifiers::NONE), MOD_NOREPEAT);
        assert_eq!(
            flags(Modifiers::CONTROL | Modifiers::SHIFT),
            MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT
        );
        assert_eq!(
            flags(Modifiers::ALT | Modifiers::SUPER),
            MOD_ALT | MOD_WIN | MOD_NOREPEAT
        );
    }

    /// The hand-written codes match the Windows SDK's.
    #[cfg(windows)]
    #[test]
    fn codes_match_the_sdk() {
        use ::windows::Win32::UI::Input::KeyboardAndMouse as sdk;
        let fixed = |key| match key_code(key) {
            KeyCode::Fixed(vk) => vk,
            KeyCode::Positional { us, .. } => us,
        };
        assert_eq!(fixed(Key::F1), sdk::VK_F1.0);
        assert_eq!(fixed(Key::F20), sdk::VK_F20.0);
        assert_eq!(fixed(Key::PageUp), sdk::VK_PRIOR.0);
        assert_eq!(fixed(Key::PageDown), sdk::VK_NEXT.0);
        assert_eq!(fixed(Key::PrintScreen), sdk::VK_SNAPSHOT.0);
        assert_eq!(fixed(Key::Delete), sdk::VK_DELETE.0);
        assert_eq!(fixed(Key::Minus), sdk::VK_OEM_MINUS.0);
        assert_eq!(fixed(Key::Equal), sdk::VK_OEM_PLUS.0);
        assert_eq!(fixed(Key::Grave), sdk::VK_OEM_3.0);
        assert_eq!(fixed(Key::Backslash), sdk::VK_OEM_5.0);
        assert_eq!(MOD_NOREPEAT, sdk::MOD_NOREPEAT.0);
        assert_eq!(MOD_WIN, sdk::MOD_WIN.0);
    }
}

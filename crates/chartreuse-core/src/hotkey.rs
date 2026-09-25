//! Global hotkeys: a set of modifiers plus one key.
//!
//! The text form (used by the config file and the settings UI) is the modifiers in
//! the fixed order `Ctrl+Alt+Shift+Super`, then the key, joined by `+`, for example
//! `Ctrl+Shift+4`. Parsing is case-insensitive and also accepts common aliases
//! (`Control`, `Option`/`Opt`, `Cmd`/`Command`/`Meta`/`Win`, `Enter`, `Esc`).
//! `Super` is the Command key on macOS and the Windows key on Windows.

use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::str::FromStr;

/// A set of modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CONTROL: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const SHIFT: Self = Self(1 << 2);
    /// Command on macOS, the Windows key on Windows.
    pub const SUPER: Self = Self(1 << 3);

    /// Every modifier with its canonical name, in text-form order.
    const NAMED: [(Self, &'static str); 4] = [
        (Self::CONTROL, "Ctrl"),
        (Self::ALT, "Alt"),
        (Self::SHIFT, "Shift"),
        (Self::SUPER, "Super"),
    ];

    /// True if every modifier in `other` is also in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Self::CONTROL,
            "alt" | "option" | "opt" => Self::ALT,
            "shift" => Self::SHIFT,
            "super" | "cmd" | "command" | "meta" | "win" => Self::SUPER,
            _ => return None,
        })
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

macro_rules! keys {
    ($($variant:ident => $name:literal $(| $alias:literal)*),+ $(,)?) => {
        /// A non-modifier key, identified by its position on a US layout.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($variant),+
        }

        impl Key {
            /// Every key, in declaration order.
            pub const ALL: &'static [Key] = &[$(Key::$variant),+];

            /// The canonical name used in the text form.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Key::$variant => $name),+
                }
            }

            /// Looks a key up by canonical name or alias, case-insensitively.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Key> {
                $(
                    if name.eq_ignore_ascii_case($name) $(|| name.eq_ignore_ascii_case($alias))* {
                        return Some(Key::$variant);
                    }
                )+
                None
            }
        }
    };
}

keys! {
    A => "A", B => "B", C => "C", D => "D", E => "E", F => "F", G => "G", H => "H",
    I => "I", J => "J", K => "K", L => "L", M => "M", N => "N", O => "O", P => "P",
    Q => "Q", R => "R", S => "S", T => "T", U => "U", V => "V", W => "W", X => "X",
    Y => "Y", Z => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
    F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
    F13 => "F13", F14 => "F14", F15 => "F15", F16 => "F16", F17 => "F17",
    F18 => "F18", F19 => "F19", F20 => "F20",
    Space => "Space",
    Tab => "Tab",
    Return => "Return" | "Enter",
    Escape => "Escape" | "Esc",
    Backspace => "Backspace",
    Delete => "Delete" | "Del",
    Insert => "Insert" | "Ins",
    Home => "Home",
    End => "End",
    PageUp => "PageUp",
    PageDown => "PageDown",
    Left => "Left",
    Right => "Right",
    Up => "Up",
    Down => "Down",
    Minus => "Minus" | "-",
    Equal => "Equal" | "=",
    LeftBracket => "LeftBracket" | "[",
    RightBracket => "RightBracket" | "]",
    Backslash => "Backslash" | "\\",
    Semicolon => "Semicolon" | ";",
    Quote => "Quote" | "'",
    Comma => "Comma" | ",",
    Period => "Period" | ".",
    Slash => "Slash" | "/",
    Grave => "Grave" | "`",
    PrintScreen => "PrintScreen" | "PrtSc",
}

/// A global hotkey: modifiers plus one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Hotkey {
    #[must_use]
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, name) in Modifiers::NAMED {
            if self.modifiers.contains(modifier) {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.name())
    }
}

/// The error returned when parsing a [`Hotkey`] fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseHotkeyError {
    #[error("hotkey is empty")]
    Empty,
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    #[error("unknown modifier {0:?}")]
    UnknownModifier(String),
    #[error("modifier {0:?} appears more than once")]
    DuplicateModifier(String),
}

impl FromStr for Hotkey {
    type Err = ParseHotkeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseHotkeyError::Empty);
        }
        // The key follows the last `+`; a trailing `+` means the key is missing.
        let (modifier_part, key_name) = match s.rsplit_once('+') {
            Some((modifiers, key)) => (Some(modifiers), key.trim()),
            None => (None, s),
        };
        if key_name.is_empty() {
            return Err(ParseHotkeyError::UnknownKey(String::new()));
        }
        let key = Key::from_name(key_name)
            .ok_or_else(|| ParseHotkeyError::UnknownKey(key_name.to_owned()))?;
        let mut modifiers = Modifiers::NONE;
        for name in modifier_part.into_iter().flat_map(|part| part.split('+')) {
            let name = name.trim();
            let modifier = Modifiers::from_name(name)
                .ok_or_else(|| ParseHotkeyError::UnknownModifier(name.to_owned()))?;
            if modifiers.contains(modifier) {
                return Err(ParseHotkeyError::DuplicateModifier(name.to_owned()));
            }
            modifiers |= modifier;
        }
        Ok(Self { modifiers, key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_canonical_modifier_order() {
        let hotkey = Hotkey::new(
            Modifiers::SUPER | Modifiers::SHIFT | Modifiers::CONTROL,
            Key::Digit4,
        );
        assert_eq!(hotkey.to_string(), "Ctrl+Shift+Super+4");
    }

    #[test]
    fn every_key_round_trips_through_text() {
        for &key in Key::ALL {
            let hotkey = Hotkey::new(Modifiers::ALT | Modifiers::SUPER, key);
            assert_eq!(hotkey.to_string().parse::<Hotkey>(), Ok(hotkey), "{key:?}");
        }
    }

    #[test]
    fn parse_accepts_aliases_case_and_whitespace() {
        let expected = Hotkey::new(Modifiers::SUPER | Modifiers::ALT, Key::Return);
        assert_eq!(" cmd + OPTION + enter ".parse::<Hotkey>(), Ok(expected));
        assert_eq!(
            "PrintScreen".parse::<Hotkey>(),
            Ok(Hotkey::new(Modifiers::NONE, Key::PrintScreen))
        );
        assert_eq!(
            "Ctrl+-".parse::<Hotkey>(),
            Ok(Hotkey::new(Modifiers::CONTROL, Key::Minus))
        );
    }

    #[test]
    fn parse_reports_what_is_wrong() {
        assert_eq!("".parse::<Hotkey>(), Err(ParseHotkeyError::Empty));
        assert_eq!(
            "Ctrl+".parse::<Hotkey>(),
            Err(ParseHotkeyError::UnknownKey(String::new()))
        );
        assert_eq!(
            "Ctrl+Hyper".parse::<Hotkey>(),
            Err(ParseHotkeyError::UnknownKey("Hyper".into()))
        );
        assert_eq!(
            "Hyper+A".parse::<Hotkey>(),
            Err(ParseHotkeyError::UnknownModifier("Hyper".into()))
        );
        assert_eq!(
            "Cmd+Super+A".parse::<Hotkey>(),
            Err(ParseHotkeyError::DuplicateModifier("Super".into()))
        );
        assert_eq!(
            "Ctrl+Shift".parse::<Hotkey>(),
            Err(ParseHotkeyError::UnknownKey("Shift".into()))
        );
    }
}

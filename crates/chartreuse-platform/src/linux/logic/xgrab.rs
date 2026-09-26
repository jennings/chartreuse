//! What X11 key grabs need: the keycode that produces a keysym, the modifier
//! mask of a [`Modifiers`] set, the lock-key variants to grab alongside, and
//! telling a grabbed key's presses from its auto-repeats.

use chartreuse_core::hotkey::Modifiers;

pub const SHIFT: u16 = 1;
pub const LOCK: u16 = 1 << 1;
pub const CONTROL: u16 = 1 << 2;
/// `Mod1`, Alt on every common layout.
pub const ALT: u16 = 1 << 3;
/// `Mod4`, Super on every common layout.
pub const SUPER: u16 = 1 << 6;

/// The X modifier mask of `modifiers`.
#[must_use]
pub fn modifier_mask(modifiers: Modifiers) -> u16 {
    [
        (Modifiers::SHIFT, SHIFT),
        (Modifiers::CONTROL, CONTROL),
        (Modifiers::ALT, ALT),
        (Modifiers::SUPER, SUPER),
    ]
    .into_iter()
    .filter(|&(modifier, _)| modifiers.contains(modifier))
    .fold(0, |mask, (_, bit)| mask | bit)
}

/// The part of a key event's state that selects a hotkey: without the lock
/// modifiers, mouse buttons, and keyboard group.
#[must_use]
pub fn hotkey_state(state: u16) -> u16 {
    state & (SHIFT | CONTROL | ALT | SUPER)
}

/// The masks to grab a hotkey with in addition to its own, so that it works
/// with Caps Lock and Num Lock on: X matches grabs on the exact modifier
/// state, locks included.
#[must_use]
pub fn lock_variants(num_lock: u16) -> Vec<u16> {
    let mut variants = vec![0, LOCK];
    if num_lock != 0 && num_lock != LOCK {
        variants.extend([num_lock, num_lock | LOCK]);
    }
    variants
}

/// The modifier bit Num Lock is mapped to, from the `GetModifierMapping`
/// table (eight modifiers × `per_modifier` keycodes, 0 for unused slots);
/// 0 if Num Lock is not a modifier.
#[must_use]
pub fn num_lock_mask(per_modifier: usize, table: &[u8], num_lock_keycodes: &[u8]) -> u16 {
    if per_modifier == 0 {
        return 0;
    }
    table
        .chunks(per_modifier)
        .take(8)
        .position(|keycodes| {
            keycodes
                .iter()
                .any(|keycode| *keycode != 0 && num_lock_keycodes.contains(keycode))
        })
        .map_or(0, |index| 1 << index)
}

/// The keyboard mapping from `GetKeyboardMapping`: `per_keycode` keysyms for
/// each keycode from `min_keycode` up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardMapping {
    pub min_keycode: u8,
    pub per_keycode: usize,
    pub keysyms: Vec<u32>,
}

impl KeyboardMapping {
    /// Every keycode that produces `keysym` at any level.
    pub fn keycodes(&self, keysym: u32) -> impl Iterator<Item = u8> + '_ {
        self.rows()
            .filter(move |(_, row)| row.contains(&keysym))
            .map(|(keycode, _)| keycode)
    }

    /// The keycode to grab for `keysym`: one that produces it unshifted if
    /// there is one (so `a` is the A key, not a key that makes `a` at some
    /// other level), else the first that produces it at all.
    #[must_use]
    pub fn keycode(&self, keysym: u32) -> Option<u8> {
        self.rows()
            .find(|(_, row)| row.first() == Some(&keysym))
            .map(|(keycode, _)| keycode)
            .or_else(|| self.keycodes(keysym).next())
    }

    fn rows(&self) -> impl Iterator<Item = (u8, &[u32])> + '_ {
        let per_keycode = self.per_keycode.max(1);
        (self.min_keycode..=u8::MAX).zip(self.keysyms.chunks(per_keycode))
    }
}

/// How soon after a key's release, in milliseconds of server time, a press
/// of the same key is still an auto-repeat. Without detectable auto-repeat
/// the server repeats a held key as a release and a press created together.
const REPEAT_GAP: u32 = 20;

/// Tells a key's first press from the presses the X server repeats while it
/// is held, so that holding a hotkey triggers it once.
///
/// With XKB's detectable auto-repeat a held key repeats as presses alone,
/// and its one release comes when it goes up. Without it, every repeat is a
/// release immediately followed by a press.
#[derive(Debug, Default)]
pub struct HeldKeys {
    held: Vec<u8>,
    /// The keycode and server time of the latest release.
    released: Option<(u8, u32)>,
}

impl HeldKeys {
    /// Records a press of `keycode` at server `time`: `true` if the key went
    /// down, `false` if the press is an auto-repeat.
    pub fn press(&mut self, keycode: u8, time: u32) -> bool {
        if self.held.contains(&keycode) {
            return false;
        }
        self.held.push(keycode);
        !self.released.is_some_and(|(released, at)| {
            released == keycode && time.wrapping_sub(at) <= REPEAT_GAP
        })
    }

    /// Records a release of `keycode` at server `time`.
    pub fn release(&mut self, keycode: u8, time: u32) {
        self.held.retain(|&held| held != keycode);
        self.released = Some((keycode, time));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUM_LOCK: u32 = 0xff7f;

    /// Keycodes 8…: `a A`, `1 exclam`, Num_Lock, and a key with `a` shifted.
    fn mapping() -> KeyboardMapping {
        KeyboardMapping {
            min_keycode: 8,
            per_keycode: 2,
            keysyms: vec![0x61, 0x41, 0x31, 0x21, NUM_LOCK, 0, 0x62, 0x61],
        }
    }

    #[test]
    fn keysyms_map_to_the_key_that_produces_them_unshifted() {
        let mapping = mapping();
        assert_eq!(mapping.keycode(0x61), Some(8));
        assert_eq!(mapping.keycode(0x21), Some(9));
        assert_eq!(mapping.keycode(NUM_LOCK), Some(10));
        assert_eq!(mapping.keycode(0xffbe), None);
        assert_eq!(mapping.keycodes(0x61).collect::<Vec<_>>(), [8, 11]);
    }

    #[test]
    fn modifiers_map_to_their_x_bits() {
        assert_eq!(modifier_mask(Modifiers::NONE), 0);
        assert_eq!(
            modifier_mask(
                Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT | Modifiers::SUPER
            ),
            SHIFT | CONTROL | ALT | SUPER
        );
    }

    #[test]
    fn events_match_regardless_of_locks_and_buttons() {
        let mod2 = 1 << 4;
        let button1 = 1 << 8;
        let group = 1 << 13;
        assert_eq!(
            hotkey_state(CONTROL | LOCK | mod2 | button1 | group),
            CONTROL
        );
        assert_eq!(hotkey_state(SHIFT | SUPER), SHIFT | SUPER);
    }

    #[test]
    fn locks_are_grabbed_in_every_combination() {
        let mod2 = 1 << 4;
        assert_eq!(lock_variants(mod2), [0, LOCK, mod2, mod2 | LOCK]);
        assert_eq!(lock_variants(0), [0, LOCK]);
    }

    #[test]
    fn num_lock_is_found_in_the_modifier_table() {
        // Two keycodes per modifier: Shift, Lock, Control, Mod1, Mod2 = 10.
        let table = [50, 62, 66, 0, 37, 105, 64, 108, 10, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(num_lock_mask(2, &table, &[10]), 1 << 4);
        assert_eq!(num_lock_mask(2, &table, &[99]), 0);
        assert_eq!(num_lock_mask(0, &[], &[10]), 0);
    }

    #[test]
    fn detectable_auto_repeat_presses_count_until_the_release() {
        let mut keys = HeldKeys::default();
        assert!(keys.press(38, 1000));
        assert!(!keys.press(38, 1660));
        assert!(!keys.press(38, 1700));
        keys.release(38, 1720);
        assert!(keys.press(38, 2500));
    }

    #[test]
    fn a_release_and_press_together_are_a_repeat() {
        let mut keys = HeldKeys::default();
        assert!(keys.press(38, 1000));
        keys.release(38, 1660);
        assert!(!keys.press(38, 1660));
        keys.release(38, 1700);
        assert!(!keys.press(38, 1701));
        keys.release(38, 1740);
        // Pressed again by the user.
        assert!(keys.press(38, 1900));
    }

    #[test]
    fn keys_are_held_and_repeated_independently() {
        let mut keys = HeldKeys::default();
        assert!(keys.press(38, 1000));
        assert!(keys.press(39, 1010));
        keys.release(39, 1100);
        // Releasing 39 leaves 38 down, and 38's release makes no press of 39
        // a repeat.
        assert!(!keys.press(38, 1100));
        keys.release(38, 1200);
        assert!(keys.press(39, 1200));
    }

    #[test]
    fn server_time_wraps_around() {
        let mut keys = HeldKeys::default();
        assert!(keys.press(38, u32::MAX - 100));
        keys.release(38, u32::MAX - 5);
        assert!(!keys.press(38, 4));
    }
}

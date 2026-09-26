//! Windows: the tray icon's menu and callback decoding (the portable part of
//! `status_item.rs`).

use crate::status_item::{MenuAction, MenuEntry, MENU};

/// The menu command id of the [`MENU`] entry at `index`. `TrackPopupMenu` returns
/// 0 when the menu is dismissed, so ids start at 1.
pub(super) fn command_id(index: usize) -> usize {
    index + 1
}

/// The action of menu command `id`, if it is one of [`command_id`]'s.
pub(super) fn action_for_command(id: usize) -> Option<MenuAction> {
    match MENU.get(id.checked_sub(1)?)? {
        MenuEntry::Action(action) => Some(*action),
        MenuEntry::Separator => None,
    }
}

/// `label` as menu item text: `&` marks a mnemonic, so literal ones are doubled.
pub(super) fn menu_text(label: &str) -> String {
    label.replace('&', "&&")
}

/// The notification in a `NOTIFYICON_VERSION_4` callback's `lParam` (its low
/// word, such as `WM_CONTEXTMENU` or `NIN_SELECT`).
pub(super) fn notification(lparam: isize) -> u32 {
    (lparam as usize & 0xFFFF) as u32
}

/// The anchor point in a `NOTIFYICON_VERSION_4` callback's `wParam`: signed x and
/// y screen coordinates in its low and high words.
pub(super) fn anchor(wparam: usize) -> (i32, i32) {
    let word = |shift: u32| i32::from((wparam >> shift) as u16 as i16);
    (word(0), word(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_maps_back_to_its_action() {
        for (index, entry) in MENU.iter().enumerate() {
            let expected = match entry {
                MenuEntry::Action(action) => Some(*action),
                MenuEntry::Separator => None,
            };
            assert_eq!(action_for_command(command_id(index)), expected, "{index}");
        }
    }

    #[test]
    fn dismissal_and_unknown_commands_map_to_no_action() {
        for id in [0, command_id(MENU.len()), usize::MAX] {
            assert_eq!(action_for_command(id), None, "{id}");
        }
    }

    #[test]
    fn ampersands_are_shown_literally() {
        assert_eq!(menu_text("Open File…"), "Open File…");
        assert_eq!(menu_text("Quit R&D Tool"), "Quit R&&D Tool");
    }

    #[test]
    fn callbacks_decode_notification_and_signed_anchor() {
        // WM_CONTEXTMENU (0x007B) for icon id 1 in the high word.
        assert_eq!(notification(0x0001_007B), 0x007B);
        // An icon on a monitor left of and above the primary: (-100, -20).
        let wparam = (usize::from((-20i16) as u16) << 16) | usize::from((-100i16) as u16);
        assert_eq!(anchor(wparam), (-100, -20));
        assert_eq!(anchor((900 << 16) | 1800), (1800, 900));
    }
}

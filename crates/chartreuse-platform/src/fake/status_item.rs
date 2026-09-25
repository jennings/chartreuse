//! Fake [`StatusItem`]: menu choices made with [`Fake::choose_menu_action`].

use std::sync::{Arc, Weak};

use chartreuse_core::Result;
use parking_lot::Mutex;

use super::{Fake, State};
use crate::event::{self, Registration};
use crate::status_item::{StatusItem, StatusItemHandle};

/// Removes the status item from the world when dropped, unless it was replaced.
struct Uninstall {
    state: Weak<Mutex<State>>,
    generation: u64,
}

impl Drop for Uninstall {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state.lock();
            if state
                .menu
                .as_ref()
                .is_some_and(|(generation, _)| *generation == self.generation)
            {
                state.menu = None;
            }
        }
    }
}

impl StatusItem for Fake {
    fn install(&self) -> Result<StatusItemHandle> {
        let mut state = self.state.lock();
        let (sender, actions) = event::channel();
        let generation = state.next_generation();
        state.menu = Some((generation, sender));
        Ok(StatusItemHandle {
            actions,
            registration: Registration::new(Uninstall {
                state: Arc::downgrade(&self.state),
                generation,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_item::MenuAction;

    #[test]
    fn menu_actions_reach_the_installed_item_only() {
        let fake = Fake::new();
        assert!(!fake.choose_menu_action(MenuAction::Quit));
        let handle = fake.install().unwrap();
        assert!(fake.choose_menu_action(MenuAction::Settings));
        assert_eq!(handle.actions.try_recv(), Some(MenuAction::Settings));
        drop(handle);
        assert!(!fake.status_item_installed());
        assert!(!fake.choose_menu_action(MenuAction::Quit));
    }

    #[test]
    fn dropping_a_replaced_handle_keeps_the_new_item() {
        let fake = Fake::new();
        let old = fake.install().unwrap();
        let new = fake.install().unwrap();
        drop(old);
        assert!(fake.choose_menu_action(MenuAction::OpenFromFile));
        assert_eq!(new.actions.try_recv(), Some(MenuAction::OpenFromFile));
    }
}

//! The status item: installs it through the platform [`StatusItem`](chartreuse_platform::StatusItem) trait and handles its menu actions. Owned by track 1A.
//!
//! [`boot`] only asks for [`Message::Install`]: iced runs `boot` before the AppKit
//! run loop starts, and the status item must be created once it runs. [`update`]
//! installs the item and keeps its handle in [`State`]; while the handle is there,
//! [`subscription`] delivers menu choices as [`Message::Menu`].

use chartreuse_platform::{MenuAction, StatusItemHandle};
use iced::{Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::events;

/// This feature's part of the app state ([`App::tray`]).
#[derive(Debug, Default)]
pub struct State {
    /// The installed status item; dropping it removes the item.
    handle: Option<StatusItemHandle>,
}

/// This feature's messages ([`AppMessage::Tray`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Install the status item (sent by [`boot`]). Replaces an installed one.
    Install,
    /// The user chose an item from the status item menu.
    Menu(MenuAction),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::done(AppMessage::Tray(Message::Install))
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Install => install(app),
        Message::Menu(action) => menu_action(action),
    }
}

pub fn subscription(app: &App) -> Subscription<AppMessage> {
    match &app.tray.handle {
        Some(handle) => events::subscription(&handle.actions)
            .map(|action| AppMessage::Tray(Message::Menu(action))),
        None => Subscription::none(),
    }
}

fn install(app: &mut App) -> Task<AppMessage> {
    match app.platform.status_item.install() {
        Ok(handle) => {
            app.tray.handle = Some(handle);
            Task::none()
        }
        Err(error) => alert::report_error(
            app,
            Notice::from_error("The menu bar icon is unavailable", &error),
        ),
    }
}

fn menu_action(action: MenuAction) -> Task<AppMessage> {
    match action {
        MenuAction::Quit => {
            tracing::info!("quitting from the status item menu");
            iced::exit()
        }
        MenuAction::Capture(_)
        | MenuAction::OpenFromClipboard
        | MenuAction::OpenFromFile
        | MenuAction::Settings => {
            tracing::info!(?action, "status item menu action (not wired up yet)");
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::capture::CaptureMode;
    use chartreuse_core::{Error, Result};
    use chartreuse_platform::StatusItem;
    use futures::executor::block_on;
    use futures::StreamExt;
    use iced::advanced::subscription::into_recipes;
    use iced_runtime::Action;

    use super::*;
    use crate::windows::WindowKind;

    /// Everything `task` does, in order.
    fn actions(task: Task<AppMessage>) -> Vec<Action<AppMessage>> {
        iced_runtime::task::into_stream(task)
            .map(|stream| block_on(stream.collect()))
            .unwrap_or_default()
    }

    /// Chooses `action` in the status item menu and returns the message it
    /// arrives as through [`subscription`].
    fn choose(app: &App, fake: &chartreuse_platform::fake::Fake, action: MenuAction) -> Message {
        assert!(fake.choose_menu_action(action));
        let mut recipes = into_recipes(subscription(app));
        assert_eq!(recipes.len(), 1);
        let events = recipes
            .pop()
            .unwrap()
            .stream(futures::stream::empty().boxed());
        match block_on(events.into_future()).0 {
            Some(AppMessage::Tray(message)) => message,
            other => panic!("expected a tray message, got {other:?}"),
        }
    }

    #[test]
    fn boot_installs_through_a_message() {
        let (mut app, fake) = App::for_test();
        let boot = actions(boot(&mut app));
        assert!(!fake.status_item_installed(), "boot must not install");
        let [Action::Output(AppMessage::Tray(Message::Install))] = boot.as_slice() else {
            panic!("boot should ask for Install, got {boot:?}");
        };

        let _ = app.update(AppMessage::Tray(Message::Install));
        assert!(fake.status_item_installed());
    }

    #[test]
    fn menu_choices_arrive_as_messages_once_installed() {
        let (mut app, fake) = App::for_test();
        assert!(into_recipes(subscription(&app)).is_empty());
        let _ = app.update(AppMessage::Tray(Message::Install));

        let action = MenuAction::Capture(CaptureMode::Window);
        assert!(matches!(choose(&app, &fake, action), Message::Menu(chosen) if chosen == action));
    }

    #[test]
    fn quit_exits_and_other_actions_do_nothing_yet() {
        let (mut app, _fake) = App::for_test();
        let quit = actions(app.update(AppMessage::Tray(Message::Menu(MenuAction::Quit))));
        assert!(matches!(quit.as_slice(), [Action::Exit]), "{quit:?}");

        for action in [
            MenuAction::Capture(CaptureMode::Display),
            MenuAction::OpenFromClipboard,
            MenuAction::OpenFromFile,
            MenuAction::Settings,
        ] {
            let task = app.update(AppMessage::Tray(Message::Menu(action)));
            assert!(actions(task).is_empty(), "{action:?}");
        }
    }

    #[test]
    fn install_failures_are_reported_to_the_user() {
        struct Broken;
        impl StatusItem for Broken {
            fn install(&self) -> Result<StatusItemHandle> {
                Err(Error::Unsupported("the status item"))
            }
        }

        let (mut app, _fake) = App::for_test();
        app.platform.status_item = Box::new(Broken);
        let _ = app.update(AppMessage::Tray(Message::Install));
        assert_eq!(app.windows.of_kind(WindowKind::Alert).count(), 1);
        assert!(into_recipes(subscription(&app)).is_empty());
    }
}

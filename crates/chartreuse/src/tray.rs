//! The status item: installs it through the platform [`StatusItem`](chartreuse_platform::StatusItem) trait and handles its menu actions. Owned by track 1A.
//!
//! Install the status item in [`update`], not [`boot`]: iced runs `boot` before the AppKit run loop starts. At startup, have `boot` return `Task::done(AppMessage::Tray(Message::Install))` (or similar) and install when `update` handles it.

use iced::{Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::tray`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Tray`]).
#[derive(Debug, Clone)]
pub enum Message {}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(_app: &mut App, message: Message) -> Task<AppMessage> {
    match message {}
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

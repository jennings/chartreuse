//! Global hotkeys: registers the configured hotkeys through the platform [`Hotkeys`](chartreuse_platform::Hotkeys) trait and turns presses into messages. Owned by track 1B; re-registration on settings changes by 3A.

use iced::{Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::hotkeys`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Hotkeys`]).
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

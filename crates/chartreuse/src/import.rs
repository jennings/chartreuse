//! Opening an image from the clipboard or a file in the editor. Owned by integration task I4.

use iced::{Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::import`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Import`]).
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

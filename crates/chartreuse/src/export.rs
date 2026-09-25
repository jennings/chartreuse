//! Saving an edited image to a file or copying it to the clipboard. Owned by integration tasks I2 and I4.

use iced::{Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::export`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Export`]).
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

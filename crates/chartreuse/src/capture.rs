//! The capture flow: captures every display, then goes to the editor directly or through a selection overlay. Owned by integration tasks I2, I3, and I5.

use iced::{Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::capture`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Capture`]).
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

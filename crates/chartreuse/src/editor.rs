//! Editor windows (`WindowKind::Editor`), driven by `chartreuse-editor`. The editor window view ([`view`]) is owned by track 2E; the rest by integration task I4.

use iced::widget::space;
use iced::{window, Element, Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::editor`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Editor`]).
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

pub fn view(_app: &App, _window: window::Id) -> Element<'_, AppMessage> {
    space().into()
}

pub fn window_closed(_app: &mut App, _window: window::Id) -> Task<AppMessage> {
    Task::none()
}

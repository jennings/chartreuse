//! The Stage 0 placeholder window.
//!
//! Until the status item exists, this blank window proves that the daemon opens
//! and closes windows and shows the flavor's accent. Closing it quits the app,
//! since nothing else can. Once the status item's Quit works, an integration
//! pass (not a Stage 1 track) removes this module and `WindowKind::Placeholder`.

use chartreuse_core::flavor;
use iced::widget::{button, column, container, space, text};
use iced::{window, Element, Length, Size, Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::windows::WindowKind;

#[derive(Debug, Default)]
pub struct State {
    window: Option<window::Id>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ShowSampleAlert,
    Quit,
}

pub fn boot(app: &mut App) -> Task<AppMessage> {
    let (id, open) = app.windows.open(
        WindowKind::Placeholder,
        window::Settings {
            size: Size::new(480.0, 300.0),
            ..window::Settings::default()
        },
    );
    app.placeholder.window = Some(id);
    open.discard()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::ShowSampleAlert => alert::report_error(
            app,
            Notice::new(
                "Sample notice",
                "This is how Chartreuse reports problems to the user.",
            ),
        ),
        Message::Quit => iced::exit(),
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

pub fn view(_app: &App, _id: window::Id) -> Element<'_, AppMessage> {
    let accent_stripe =
        container(space().height(8))
            .width(Length::Fill)
            .style(|theme: &iced::Theme| {
                container::Style::default().background(theme.palette().primary)
            });
    let content = column![
        text(flavor::DISPLAY_NAME).size(28),
        text("Nothing to see yet: this placeholder window stands in for the status item."),
        button(text("Show sample alert"))
            .style(button::secondary)
            .on_press(AppMessage::Placeholder(Message::ShowSampleAlert)),
        button(text(format!("Quit {}", flavor::DISPLAY_NAME)))
            .style(button::primary)
            .on_press(AppMessage::Placeholder(Message::Quit)),
    ]
    .spacing(16);
    column![
        accent_stripe,
        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .into()
}

pub fn window_closed(app: &mut App, id: window::Id) -> Task<AppMessage> {
    if app.placeholder.window == Some(id) {
        tracing::info!("placeholder window closed; quitting");
        return iced::exit();
    }
    Task::none()
}

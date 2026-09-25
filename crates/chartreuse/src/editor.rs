//! Editor windows (`WindowKind::Editor`), driven by `chartreuse-editor`. The editor window view ([`view`]) is owned by track 2E; the rest by integration task I4.
//!
//! Each editor window shows one [`Editor`] widget, kept in [`State`] by
//! window id. Whoever opens an editor window (I4) inserts its editor with
//! [`State::insert`]; the widget's messages come back as
//! [`Message::Widget`], and closing the window drops its editor.

use std::collections::HashMap;

use chartreuse_editor::Editor;
use iced::widget::space;
use iced::{window, Element, Subscription, Task};

use crate::app::{App, Message as AppMessage};

/// This feature's part of the app state ([`App::editor`]).
#[derive(Debug, Default)]
pub struct State {
    /// The editor in each open editor window.
    editors: HashMap<window::Id, Editor>,
}

impl State {
    /// Shows `editor` in editor window `window` (replacing any editor it had).
    pub fn insert(&mut self, window: window::Id, editor: Editor) {
        self.editors.insert(window, editor);
    }

    /// The editor in `window`, if it is an open editor window.
    #[must_use]
    pub fn get(&self, window: window::Id) -> Option<&Editor> {
        self.editors.get(&window)
    }

    #[must_use]
    pub fn get_mut(&mut self, window: window::Id) -> Option<&mut Editor> {
        self.editors.get_mut(&window)
    }
}

/// This feature's messages ([`AppMessage::Editor`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// A message from the editor widget in a window.
    Widget(window::Id, chartreuse_editor::Message),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Widget(window, message) => {
            let Some(editor) = app.editor.get_mut(window) else {
                return Task::none();
            };
            if let Some(event) = editor.update(message) {
                // Export (Cmd+S, Cmd+C) is wired up by integration task I4.
                tracing::info!(?window, ?event, "editor event");
            }
            Task::none()
        }
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

/// The editor window's contents: its [`Editor`] widget.
pub fn view(app: &App, window: window::Id) -> Element<'_, AppMessage> {
    match app.editor.get(window) {
        Some(editor) => editor
            .view()
            .map(move |message| AppMessage::Editor(Message::Widget(window, message))),
        None => space().into(),
    }
}

pub fn window_closed(app: &mut App, window: window::Id) -> Task<AppMessage> {
    app.editor.editors.remove(&window);
    Task::none()
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_core::image::Image;
    use chartreuse_editor::tools::ToolKind;

    use super::*;

    fn editor() -> Editor {
        Editor::new(Image::filled(
            PhysicalSize::new(8, 8),
            Rgba8::from_rgb_hex(0xffffff),
        ))
    }

    fn widget(window: window::Id, message: chartreuse_editor::Message) -> Message {
        Message::Widget(window, message)
    }

    #[test]
    fn widget_messages_reach_the_editor_of_their_window_only() {
        let (mut app, _fake) = App::for_test();
        let (first, second) = (window::Id::unique(), window::Id::unique());
        app.editor.insert(first, editor());
        app.editor.insert(second, editor());

        let _ = update(
            &mut app,
            widget(first, chartreuse_editor::Message::Tool(ToolKind::Text)),
        );
        let _ = update(
            &mut app,
            widget(window::Id::unique(), chartreuse_editor::Message::Undo),
        );
        assert_eq!(app.editor.get(first).unwrap().tool(), ToolKind::Text);
        assert_eq!(app.editor.get(second).unwrap().tool(), ToolKind::Select);
    }

    #[test]
    fn closing_a_window_drops_its_editor() {
        let (mut app, _fake) = App::for_test();
        let window = window::Id::unique();
        app.editor.insert(window, editor());
        let _ = window_closed(&mut app, window);
        assert!(app.editor.get(window).is_none());
    }
}

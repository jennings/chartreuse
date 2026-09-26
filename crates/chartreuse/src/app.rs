//! The app core: state, the top-level [`Message`], and dispatch to the feature
//! modules.
//!
//! # Feature modules
//!
//! Every feature lives in its own module, so parallel work never edits the same
//! file: `hotkeys`, `tray`, `permission`, `capture`, `overlay`, `editor`,
//! `settings`, `import`, `export`, plus the shared `alert` (user notices). The
//! daemon starts with no windows; the status item is the app's only UI until a
//! feature opens a window. Each module exports:
//!
//! - `State`: its part of the app state, a field of [`App`] named after the module.
//! - `Message`: its messages, wrapped in the [`Message`] variant named after the
//!   module. Other modules talk to it by returning
//!   `Task::done(Message::Module(module::Message::…))` from their `update`.
//! - `boot(&mut App) -> Task<Message>`: startup work, run once on the main thread
//!   before the event loop starts (see [Threading](#threading)).
//! - `update(&mut App, module::Message) -> Task<Message>`.
//! - `subscription(&App) -> Subscription<Message>`.
//! - For modules that own a [`WindowKind`]: `view(&App, window::Id)` and
//!   `window_closed(&mut App, window::Id) -> Task<Message>`.
//!
//! This file only dispatches; it should not need to change when a feature grows.
//!
//! # Windows
//!
//! Open windows through [`App::windows`] ([`WindowRegistry::open`]) so every
//! window has a [`WindowKind`] that routes its `view` and close event to the
//! owning module. Report problems to the user with
//! [`alert::report_error`](crate::alert::report_error).
//!
//! # Threading
//!
//! `boot`, `update`, `view`, and `subscription` run on the main thread; call
//! platform traits from there (see `chartreuse_platform`'s crate docs). `boot`
//! runs before the event loop starts, so AppKit UI setup (the status item, and
//! anything else that needs the running `NSApplication` run loop) belongs in
//! `update`: have `boot` return `Task::done` with the module's install message.

use chartreuse_core::flavor;
use chartreuse_platform::{fake, Platform};
use iced::widget::space;
use iced::{window, Element, Subscription, Task, Theme};

use crate::windows::{WindowKind, WindowRegistry};
use crate::{
    alert, capture, editor, export, hotkeys, import, overlay, permission, settings, theme, tray,
};

/// Selects the platform backend: `fake` for the synthetic backend, anything else
/// (or unset) for the real one.
const BACKEND_ENV: &str = "CHARTREUSE_BACKEND";

/// The whole app state.
#[derive(Debug)]
pub struct App {
    pub platform: Platform,
    pub windows: WindowRegistry,
    pub theme: Theme,
    pub alert: alert::State,
    pub hotkeys: hotkeys::State,
    pub tray: tray::State,
    pub permission: permission::State,
    pub capture: capture::State,
    pub overlay: overlay::State,
    pub editor: editor::State,
    pub settings: settings::State,
    pub import: import::State,
    pub export: export::State,
}

/// Every message, grouped by the module that handles it.
#[derive(Debug, Clone)]
pub enum Message {
    Hotkeys(hotkeys::Message),
    Tray(tray::Message),
    Permission(permission::Message),
    Capture(capture::Message),
    Overlay(overlay::Message),
    Editor(editor::Message),
    Settings(settings::Message),
    Import(import::Message),
    Export(export::Message),
    Alert(alert::Message),
    /// A window closed (by the user or by `window::close`).
    WindowClosed(window::Id),
}

impl App {
    fn new(platform: Platform) -> Self {
        Self {
            platform,
            windows: WindowRegistry::default(),
            theme: theme::theme(flavor::ACCENT),
            alert: alert::State::default(),
            hotkeys: hotkeys::State::default(),
            tray: tray::State::default(),
            permission: permission::State::default(),
            capture: capture::State::default(),
            overlay: overlay::State::default(),
            editor: editor::State::default(),
            settings: settings::State::default(),
            import: import::State::default(),
            export: export::State::default(),
        }
    }

    /// Creates the state and runs every module's `boot`. Called by iced on the
    /// main thread.
    pub fn boot() -> (Self, Task<Message>) {
        let platform = match std::env::var(BACKEND_ENV).as_deref() {
            Ok("fake") => {
                tracing::info!("using the fake platform backend");
                fake::Fake::new().platform()
            }
            _ => chartreuse_platform::current(),
        };
        let mut app = Self::new(platform);
        let boots: [fn(&mut Self) -> Task<Message>; 10] = [
            permission::boot,
            tray::boot,
            hotkeys::boot,
            capture::boot,
            overlay::boot,
            editor::boot,
            settings::boot,
            import::boot,
            export::boot,
            alert::boot,
        ];
        let tasks: Vec<Task<Message>> = boots.into_iter().map(|boot| boot(&mut app)).collect();
        (app, Task::batch(tasks))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Hotkeys(message) => hotkeys::update(self, message),
            Message::Tray(message) => tray::update(self, message),
            Message::Permission(message) => permission::update(self, message),
            Message::Capture(message) => capture::update(self, message),
            Message::Overlay(message) => overlay::update(self, message),
            Message::Editor(message) => editor::update(self, message),
            Message::Settings(message) => settings::update(self, message),
            Message::Import(message) => import::update(self, message),
            Message::Export(message) => export::update(self, message),
            Message::Alert(message) => alert::update(self, message),
            Message::WindowClosed(id) => self.window_closed(id),
        }
    }

    fn window_closed(&mut self, id: window::Id) -> Task<Message> {
        let Some(kind) = self.windows.remove(id) else {
            return Task::none();
        };
        tracing::debug!(?id, ?kind, "window closed");
        match kind {
            WindowKind::Overlay => overlay::window_closed(self, id),
            WindowKind::Editor => editor::window_closed(self, id),
            WindowKind::Settings => settings::window_closed(self, id),
            WindowKind::Alert => alert::window_closed(self, id),
            WindowKind::Permission => permission::window_closed(self, id),
        }
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message> {
        match self.windows.kind(id) {
            Some(WindowKind::Overlay) => overlay::view(self, id),
            Some(WindowKind::Editor) => editor::view(self, id),
            Some(WindowKind::Settings) => settings::view(self, id),
            Some(WindowKind::Alert) => alert::view(self, id),
            Some(WindowKind::Permission) => permission::view(self, id),
            // Not ours, or already closed.
            None => space().into(),
        }
    }

    pub fn title(&self, id: window::Id) -> String {
        self.windows
            .kind(id)
            .map_or_else(|| flavor::DISPLAY_NAME.to_owned(), WindowKind::title)
    }

    pub fn theme(&self, _id: window::Id) -> Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::close_events().map(Message::WindowClosed),
            hotkeys::subscription(self),
            tray::subscription(self),
            permission::subscription(self),
            capture::subscription(self),
            overlay::subscription(self),
            editor::subscription(self),
            settings::subscription(self),
            import::subscription(self),
            export::subscription(self),
            alert::subscription(self),
        ])
    }
}

#[cfg(test)]
impl App {
    /// An app on the fake backend, for module tests. Does not run `boot`.
    pub fn for_test() -> (Self, fake::Fake) {
        let fake = fake::Fake::new();
        (Self::new(fake.platform()), fake)
    }

    /// Handles `message`, then every message its tasks produce, depth first,
    /// until nothing is left to do. Futures run to completion on this thread.
    /// Closing a window is answered with [`Message::WindowClosed`], as the
    /// runtime does. Other actions (opening or focusing a window, …) are
    /// dropped as they arrive, so tasks waiting on their answer end. Returns
    /// every message handled, in order, starting with `message`.
    pub fn settle(&mut self, message: Message) -> Vec<Message> {
        use futures::executor::block_on;
        use futures::StreamExt;
        use iced_runtime::{window, Action};

        let mut pending = vec![message];
        let mut handled = Vec::new();
        while let Some(message) = pending.pop() {
            handled.push(message.clone());
            let task = self.update(message);
            let outputs: Vec<Message> = iced_runtime::task::into_stream(task)
                .map(|stream| {
                    block_on(
                        stream
                            .filter_map(|action| {
                                futures::future::ready(match action {
                                    Action::Output(message) => Some(message),
                                    Action::Window(window::Action::Close(id)) => {
                                        Some(Message::WindowClosed(id))
                                    }
                                    _ => None,
                                })
                            })
                            .collect(),
                    )
                })
                .unwrap_or_default();
            // Depth first: the first output is handled next.
            pending.extend(outputs.into_iter().rev());
        }
        handled
    }
}

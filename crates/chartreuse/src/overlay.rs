//! Selection overlay windows (`WindowKind::Overlay`), one per display, driven by
//! `chartreuse-overlay`. Owned by integration tasks I3 and I5.
//!
//! # Flow
//!
//! 1. The capture flow sends [`Message::OpenRectangle`] with the frozen
//!    [`Snapshot`]. Its captures become image handles off the main thread
//!    ([`Message::Frozen`]), then one overlay window opens per captured display
//!    ([`setup::open`]), each drawing its display's capture under one shared
//!    [`Selection`] in global logical coordinates ([`RectangleOverlay`]).
//! 2. Every overlay's pointer and Escape input goes to that selection
//!    ([`Message::Selection`]). Escape reaches the focused overlay only: the
//!    platform style activates the app and makes each overlay key as it is
//!    shown, and the primary display's overlay is focused as well
//!    ([`window::gain_focus`]), for backends whose style cannot.
//! 3. When the selection ends, every overlay closes and the capture flow hears
//!    how: `capture::Message::Selected` with the rectangle on commit,
//!    `capture::Message::SelectionCancelled` on Escape. An overlay closed any
//!    other way (by the OS) mid-selection cancels too.
//!
//! There is one session at a time: the capture flow starts no capture while one
//! is in progress. A finished session lingers only until its windows have
//! closed, so they keep showing the capture meanwhile; a new session replaces it.

use chartreuse_core::display::{DisplayId, DisplayLayout};
use chartreuse_core::flavor;
use chartreuse_overlay::rectangle::{Input, Outcome, RectangleOverlay, Selection};
use chartreuse_overlay::setup::{self, OverlayWindows, Styled};
use chartreuse_overlay::shared;
use iced::widget::image::Handle;
use iced::widget::space;
use iced::{window, Element, Subscription, Task};

use crate::app::{App, Message as AppMessage};
use crate::capture::{self, Snapshot};
use crate::theme;
use crate::windows::WindowKind;

/// This feature's part of the app state ([`App::overlay`]).
#[derive(Debug, Default)]
pub struct State {
    session: Option<Session>,
}

impl State {
    /// The display an open overlay window covers.
    #[must_use]
    pub fn display(&self, window: window::Id) -> Option<DisplayId> {
        self.session.as_ref()?.windows.display(window)
    }
}

/// One selection across every display's overlay window.
#[derive(Debug)]
struct Session {
    /// The selection, over the snapshot's layout.
    selection: Selection,
    /// Each display's frozen capture, in the layout's display order.
    images: Vec<Handle>,
    /// The overlay windows still open.
    windows: OverlayWindows,
}

/// This feature's messages ([`AppMessage::Overlay`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Open the rectangle-selection overlays over a capture.
    OpenRectangle(Snapshot),
    /// A capture's images are ready to draw, one per display in `layout`'s
    /// order: open its overlays.
    Frozen(DisplayLayout, Vec<Handle>),
    /// An overlay window was styled and shown.
    Styled(Styled),
    /// Pointer or Escape input from an overlay window.
    Selection(Input),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::OpenRectangle(snapshot) => freeze(snapshot),
        Message::Frozen(layout, images) => open_rectangle(app, layout, images),
        Message::Styled(styled) => shown(app, styled),
        Message::Selection(input) => select(app, input),
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

pub fn view(app: &App, window: window::Id) -> Element<'_, AppMessage> {
    let overlay = app.overlay.session.as_ref().and_then(|session| {
        let display = session.windows.display(window)?;
        let (info, image) = session
            .selection
            .layout()
            .displays()
            .iter()
            .zip(&session.images)
            .find(|(info, _)| info.id == display)?;
        Some(RectangleOverlay::new(
            &session.selection,
            info,
            image,
            theme::to_iced(flavor::ACCENT),
            |input| AppMessage::Overlay(Message::Selection(input)),
        ))
    });
    overlay.map_or_else(|| space().into(), RectangleOverlay::view)
}

pub fn window_closed(app: &mut App, window: window::Id) -> Task<AppMessage> {
    let Some(session) = &mut app.overlay.session else {
        return Task::none();
    };
    if session.windows.remove(window).is_none() {
        // One of a replaced session's windows.
        return Task::none();
    }
    // Closed from outside while still selecting: give up on the selection.
    let cancelled = session
        .selection
        .escape()
        .map(|_| end(session, Outcome::Cancel));
    if session.windows.is_empty() {
        app.overlay.session = None;
    }
    cancelled.unwrap_or_else(Task::none)
}

/// Makes image handles of `snapshot`'s captures off the main thread, then
/// reports them as [`Message::Frozen`]. Each handle takes its display's pixels
/// by value, so every capture is copied: the snapshot keeps its own for cropping.
fn freeze(snapshot: Snapshot) -> Task<AppMessage> {
    Task::perform(
        async move {
            let images = snapshot
                .captures()
                .iter()
                .map(|capture| shared::frozen_image(capture.image.clone()))
                .collect();
            (snapshot.layout().clone(), images)
        },
        |(layout, images)| AppMessage::Overlay(Message::Frozen(layout, images)),
    )
}

fn open_rectangle(app: &mut App, layout: DisplayLayout, images: Vec<Handle>) -> Task<AppMessage> {
    let (windows, styled) = setup::open(&layout, &app.platform.overlay_style, |settings| {
        app.windows.open(WindowKind::Overlay, settings)
    });
    tracing::debug!(overlays = windows.len(), "opened the rectangle overlays");
    let session = Session {
        selection: Selection::new(layout),
        images,
        windows,
    };
    // A replaced session has ended and its windows are already closing.
    app.overlay.session = Some(session);
    styled.map(|styled| AppMessage::Overlay(Message::Styled(styled)))
}

/// An overlay was styled and shown: focus it if it covers the primary display.
fn shown(app: &App, Styled { window, result }: Styled) -> Task<AppMessage> {
    if let Err(error) = result {
        tracing::warn!(?window, %error, "could not style an overlay window");
    }
    let Some(session) = &app.overlay.session else {
        return Task::none();
    };
    let primary = session.selection.layout().primary().id;
    if session.windows.display(window) == Some(primary) {
        window::gain_focus(window)
    } else {
        Task::none()
    }
}

fn select(app: &mut App, input: Input) -> Task<AppMessage> {
    let Some(session) = &mut app.overlay.session else {
        return Task::none();
    };
    session
        .selection
        .apply(input)
        .map_or_else(Task::none, |outcome| end(session, outcome))
}

/// Closes every overlay and tells the capture flow how the selection ended.
fn end(session: &Session, outcome: Outcome) -> Task<AppMessage> {
    let reply = match outcome {
        Outcome::Commit(rect) => capture::Message::Selected(rect),
        Outcome::Cancel => capture::Message::SelectionCancelled,
    };
    Task::batch([
        session.windows.close_all(),
        Task::done(AppMessage::Capture(reply)),
    ])
}

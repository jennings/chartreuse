//! Selection overlay windows (`WindowKind::Overlay`), one per display, driven by
//! `chartreuse-overlay`. Owned by integration tasks I3 and I5.
//!
//! # Flow
//!
//! 1. The capture flow sends [`Message::OpenRectangle`] with the frozen
//!    [`Snapshot`]. Its captures become image handles off the main thread
//!    ([`Message::Frozen`]), then one overlay window opens per captured display
//!    ([`setup::open`]), each drawing its display's capture under one shared
//!    [`Selector`] in global logical coordinates: a [`Selection`] drawn by
//!    [`RectangleOverlay`].
//! 2. Every overlay's pointer and Escape input goes to that selector
//!    ([`Message::Rectangle`]). Escape reaches the focused overlay only: the
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
use chartreuse_overlay::rectangle::{self, RectangleOverlay, Selection};
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

/// The selection gesture shared by every display's overlay, in global logical
/// coordinates.
#[derive(Debug, Clone)]
pub enum Selector {
    /// Drag out a rectangle.
    Rectangle(Selection),
}

impl Selector {
    /// The displays the selection spans.
    const fn layout(&self) -> &DisplayLayout {
        match self {
            Self::Rectangle(selection) => selection.layout(),
        }
    }

    /// Feeds rectangle input to a rectangle selection, returning what to tell
    /// the capture flow if it ended the selection.
    fn rectangle(&mut self, input: rectangle::Input) -> Option<capture::Message> {
        let Self::Rectangle(selection) = self;
        selection.apply(input).map(|outcome| match outcome {
            rectangle::Outcome::Commit(rect) => capture::Message::Selected(rect),
            rectangle::Outcome::Cancel => capture::Message::SelectionCancelled,
        })
    }

    /// Cancels the selection if it has not ended yet, returning what to tell
    /// the capture flow if so.
    fn cancel(&mut self) -> Option<capture::Message> {
        let Self::Rectangle(selection) = self;
        selection
            .escape()
            .map(|_| capture::Message::SelectionCancelled)
    }
}

/// One selection across every display's overlay window.
#[derive(Debug)]
struct Session {
    /// The selection, over the snapshot's layout.
    selector: Selector,
    /// Each display's frozen capture, in the layout's display order.
    images: Vec<Handle>,
    /// The overlay windows still open.
    windows: OverlayWindows,
}

impl Session {
    /// The overlay for `window`, if it is one of this session's.
    fn view(&self, window: window::Id) -> Option<Element<'_, AppMessage>> {
        let display = self.windows.display(window)?;
        let (info, image) = self
            .selector
            .layout()
            .displays()
            .iter()
            .zip(&self.images)
            .find(|(info, _)| info.id == display)?;
        let accent = theme::to_iced(flavor::ACCENT);
        Some(match &self.selector {
            Selector::Rectangle(selection) => {
                RectangleOverlay::new(selection, info, image, accent, |input| {
                    AppMessage::Overlay(Message::Rectangle(input))
                })
                .view()
            }
        })
    }
}

/// This feature's messages ([`AppMessage::Overlay`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Open the rectangle-selection overlays over a capture.
    OpenRectangle(Snapshot),
    /// A capture's images are ready to draw, one per display in the selector's
    /// layout order: open its overlays.
    Frozen(Selector, Vec<Handle>),
    /// An overlay window was styled and shown.
    Styled(Styled),
    /// Pointer or Escape input from a rectangle overlay.
    Rectangle(rectangle::Input),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::OpenRectangle(snapshot) => freeze(
            Selector::Rectangle(Selection::new(snapshot.layout().clone())),
            snapshot,
        ),
        Message::Frozen(selector, images) => open(app, selector, images),
        Message::Styled(styled) => shown(app, styled),
        Message::Rectangle(input) => select(app, |selector| selector.rectangle(input)),
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

pub fn view(app: &App, window: window::Id) -> Element<'_, AppMessage> {
    app.overlay
        .session
        .as_ref()
        .and_then(|session| session.view(window))
        .unwrap_or_else(|| space().into())
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
    let cancelled = session.selector.cancel().map(|reply| end(session, reply));
    if session.windows.is_empty() {
        app.overlay.session = None;
    }
    cancelled.unwrap_or_else(Task::none)
}

/// Makes image handles of `snapshot`'s captures off the main thread, then
/// reports them with `selector` as [`Message::Frozen`]. Each handle takes its
/// display's pixels by value, so every capture is copied: the snapshot keeps
/// its own for cropping.
fn freeze(selector: Selector, snapshot: Snapshot) -> Task<AppMessage> {
    Task::perform(
        async move {
            snapshot
                .captures()
                .iter()
                .map(|capture| shared::frozen_image(capture.image.clone()))
                .collect()
        },
        move |images| AppMessage::Overlay(Message::Frozen(selector, images)),
    )
}

fn open(app: &mut App, selector: Selector, images: Vec<Handle>) -> Task<AppMessage> {
    let (windows, styled) =
        setup::open(selector.layout(), &app.platform.overlay_style, |settings| {
            app.windows.open(WindowKind::Overlay, settings)
        });
    tracing::debug!(overlays = windows.len(), "opened the overlays");
    let session = Session {
        selector,
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
    let primary = session.selector.layout().primary().id;
    if session.windows.display(window) == Some(primary) {
        window::gain_focus(window)
    } else {
        Task::none()
    }
}

/// Feeds input to the session's selector with `apply`, ending the session if
/// the selection ended.
fn select(
    app: &mut App,
    apply: impl FnOnce(&mut Selector) -> Option<capture::Message>,
) -> Task<AppMessage> {
    let Some(session) = &mut app.overlay.session else {
        return Task::none();
    };
    apply(&mut session.selector).map_or_else(Task::none, |reply| end(session, reply))
}

/// Closes every overlay and tells the capture flow how the selection ended.
fn end(session: &Session, reply: capture::Message) -> Task<AppMessage> {
    Task::batch([
        session.windows.close_all(),
        Task::done(AppMessage::Capture(reply)),
    ])
}

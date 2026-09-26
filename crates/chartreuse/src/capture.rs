//! The capture flow: captures every display first, then hands the result on.
//! Owned by integration tasks I2, I3, and I5.
//!
//! # Flow
//!
//! 1. [`Message::Start`] (from the status item menu or a hotkey) passes the
//!    Screen Recording gate ([`permission::ensure_screen_recording`]), then
//!    captures every display through the platform
//!    [`Capture`](chartreuse_platform::Capture) trait, off the main thread.
//! 2. The captures arrive frozen, with the [`DisplayLayout`] they were taken
//!    from, as a [`Snapshot`] in the [`Scene`] of [`Message::Captured`].
//! 3. By mode:
//!    - **Display**: the snapshot is composited onto the whole desktop
//!      ([`Snapshot::desktop`], at the largest scale factor, off the main
//!      thread) and the image goes to `export::Message::CopyAndSave`.
//!    - **Rectangle**: the snapshot is kept while the selection overlays show
//!      it (`overlay::Message::OpenRectangle`). A committed rectangle
//!      ([`Message::Selected`]) is cropped at the largest scale factor of the
//!      displays it covers ([`rectangle::output_grid`], then
//!      [`Snapshot::composite`] off the main thread) and goes to
//!      `export::Message::CopyAndSave` too. A cancelled selection
//!      ([`Message::SelectionCancelled`]) discards the snapshot.
//!    - **Window** is not implemented yet: `Start` logs and returns (I5).
//!
//! One capture runs at a time, selection included: a `Start` while one is in
//! progress is ignored.
//!
//! # Failures
//!
//! A capture that fails with `Error::PermissionDenied(Permission::ScreenRecording)`
//! (which the platform also reports for captures that come back blank) opens the
//! permission guidance ([`permission::show_guidance`]). Any other failure is
//! reported with [`alert::report_error`].

use std::sync::Arc;

use chartreuse_core::capture::CaptureMode;
use chartreuse_core::display::{DisplayLayout, PixelGrid};
use chartreuse_core::geometry::LogicalRect;
use chartreuse_core::image::Image;
use chartreuse_core::permission::Permission;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::Composite;
use chartreuse_overlay::rectangle;
use chartreuse_platform::DisplayCapture;
use iced::{Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::{export, overlay, permission};

/// This feature's part of the app state ([`App::capture`]).
#[derive(Debug, Default)]
pub struct State {
    /// The mode of the capture in progress, from `Start` until its image is
    /// handed on, it fails, or its selection is cancelled.
    in_progress: Option<CaptureMode>,
    /// The capture being selected from while the overlays show it.
    selecting: Option<Snapshot>,
}

impl State {
    /// The mode of the capture in progress, if any.
    #[must_use]
    pub const fn in_progress(&self) -> Option<CaptureMode> {
        self.in_progress
    }
}

/// Every display's contents at one moment, with the layout they were taken in.
///
/// Cloning is cheap: the captures are shared.
#[derive(Debug, Clone)]
pub struct Snapshot {
    layout: DisplayLayout,
    captures: Arc<[DisplayCapture]>,
}

impl Snapshot {
    /// Freezes `captures`, validating their displays as a [`DisplayLayout`].
    ///
    /// # Errors
    ///
    /// [`Error::Platform`] if the captured displays do not form a valid layout
    /// (none, no single primary, or a repeated id).
    pub fn new(captures: Vec<DisplayCapture>) -> Result<Self> {
        let layout = DisplayLayout::new(
            captures
                .iter()
                .map(|capture| capture.display.clone())
                .collect(),
        )?;
        Ok(Self {
            layout,
            captures: captures.into(),
        })
    }

    /// The displays as they were when captured.
    #[must_use]
    pub const fn layout(&self) -> &DisplayLayout {
        &self.layout
    }

    /// One capture per display, in the layout's order.
    #[must_use]
    pub fn captures(&self) -> &[DisplayCapture] {
        &self.captures
    }

    /// The captures rendered onto `grid`, e.g. [`DisplayLayout::capture_grid`] of
    /// a selection. CPU-heavy for large grids: run it off the main thread.
    ///
    /// # Errors
    ///
    /// As [`chartreuse_imaging::composite_at`].
    pub fn composite(&self, grid: PixelGrid) -> Result<Composite> {
        chartreuse_imaging::composite_at(
            self.captures
                .iter()
                .map(|capture| (&capture.display, &capture.image)),
            grid,
        )
    }

    /// The whole desktop: [`Self::composite`] on [`DisplayLayout::desktop_grid`].
    ///
    /// # Errors
    ///
    /// As [`chartreuse_imaging::composite_at`].
    pub fn desktop(&self) -> Result<Composite> {
        self.composite(self.layout.desktop_grid())
    }
}

/// What a capture took, by mode, before the mode's next step.
#[derive(Debug, Clone)]
pub enum Scene {
    /// Every display, for a display capture.
    Display(Snapshot),
    /// Every display, to select a rectangle from.
    Rectangle(Snapshot),
}

/// This feature's messages ([`AppMessage::Capture`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Start a capture: from the status item menu or a hotkey.
    Start(CaptureMode),
    /// The displays were captured for the capture in progress.
    Captured(Result<Scene>),
    /// The user committed this rectangle (global logical coordinates) over the
    /// capture being selected from.
    Selected(LogicalRect),
    /// The user cancelled the selection.
    SelectionCancelled,
    /// The image of the capture in progress is ready.
    Finished(Result<Arc<Image>>),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Start(mode) => start(app, mode),
        Message::Captured(Ok(scene)) => captured(app, scene),
        Message::Selected(rect) => selected(app, &rect),
        Message::SelectionCancelled => {
            tracing::info!("selection cancelled");
            app.capture.selecting = None;
            app.capture.in_progress = None;
            Task::none()
        }
        Message::Captured(Err(error)) | Message::Finished(Err(error)) => {
            app.capture.in_progress = None;
            failed(app, &error)
        }
        Message::Finished(Ok(image)) => {
            let mode = app.capture.in_progress.take();
            tracing::info!(
                mode = ?mode,
                width = image.size().width,
                height = image.size().height,
                "captured"
            );
            Task::done(AppMessage::Export(export::Message::CopyAndSave(image)))
        }
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

fn start(app: &mut App, mode: CaptureMode) -> Task<AppMessage> {
    if let Some(running) = app.capture.in_progress {
        tracing::info!(%mode, %running, "a capture is already in progress; ignoring");
        return Task::none();
    }
    let scene: fn(Snapshot) -> Scene = match mode {
        CaptureMode::Display => Scene::Display,
        CaptureMode::Rectangle => Scene::Rectangle,
        CaptureMode::Window => {
            tracing::info!("{mode}: not implemented yet");
            return Task::none();
        }
    };
    if let Err(guidance) = permission::ensure_screen_recording(app) {
        return guidance;
    }
    app.capture.in_progress = Some(mode);
    let capture = app.platform.capture.capture_displays();
    Task::perform(
        async move { capture.await.and_then(Snapshot::new).map(scene) },
        |scene| AppMessage::Capture(Message::Captured(scene)),
    )
}

fn captured(app: &mut App, scene: Scene) -> Task<AppMessage> {
    match scene {
        Scene::Display(snapshot) => {
            tracing::debug!(displays = snapshot.captures().len(), "displays captured");
            composite(move || snapshot.desktop())
        }
        Scene::Rectangle(snapshot) => {
            tracing::debug!(
                displays = snapshot.captures().len(),
                "displays captured to select from"
            );
            app.capture.selecting = Some(snapshot.clone());
            Task::done(AppMessage::Overlay(overlay::Message::OpenRectangle(
                snapshot,
            )))
        }
    }
}

/// Crops the committed `rect` out of the capture being selected from.
fn selected(app: &mut App, rect: &LogicalRect) -> Task<AppMessage> {
    let Some(snapshot) = app.capture.selecting.take() else {
        return Task::none();
    };
    match rectangle::output_grid(snapshot.layout(), rect) {
        Some(grid) => composite(move || snapshot.composite(grid)),
        None => {
            // Committed selections always cover a display.
            tracing::warn!(?rect, "the selection covers no display; nothing captured");
            app.capture.in_progress = None;
            Task::none()
        }
    }
}

/// Runs `render` off the main thread, then reports its image as
/// [`Message::Finished`].
fn composite(render: impl FnOnce() -> Result<Composite> + Send + 'static) -> Task<AppMessage> {
    Task::perform(
        async move { render().map(|composite| Arc::new(composite.image)) },
        |image| AppMessage::Capture(Message::Finished(image)),
    )
}

fn failed(app: &mut App, error: &Error) -> Task<AppMessage> {
    match error {
        Error::PermissionDenied(Permission::ScreenRecording) => {
            tracing::info!("capture withheld for lack of Screen Recording permission");
            permission::show_guidance(app)
        }
        error => alert::report_error(app, Notice::from_error("Capture failed", error)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use chartreuse_core::display::{DisplayId, DisplayInfo};
    use chartreuse_core::geometry::{LogicalPoint, LogicalRect, PhysicalSize, ScaleFactor};
    use chartreuse_core::permission::PermissionStatus;
    use chartreuse_core::window::WindowId;
    use chartreuse_overlay::rectangle::Input;
    use chartreuse_platform::fake::Fake;
    use chartreuse_platform::{Capture, MenuAction};
    use futures::executor::block_on;
    use futures::future::{self, BoxFuture, FutureExt};
    use futures::StreamExt;
    use iced::advanced::subscription::into_recipes;

    use super::*;
    use crate::windows::WindowKind;
    use crate::{hotkeys, tray};

    /// A test app whose saves go to a fresh temporary directory.
    fn app() -> (App, Fake, tempfile::TempDir) {
        let (mut app, fake) = App::for_test();
        let saves = tempfile::tempdir().unwrap();
        app.export.directory = Some(saves.path().to_owned());
        (app, fake, saves)
    }

    /// A small desktop, to keep compositing fast: a 2× primary display with its
    /// top-left at the global origin, and a 1× display up and to the left of it.
    /// Together they span (-4, -2) to (8, 6) in logical points, leaving a gap
    /// below the 1× display.
    fn small_desktop() -> Fake {
        let display = |id, bounds: LogicalRect, scale: f64, is_primary| {
            let scale_factor = ScaleFactor::new(scale).unwrap();
            DisplayInfo {
                id: DisplayId(id),
                name: format!("Display {id}"),
                logical_bounds: bounds,
                pixel_size: bounds.size.to_physical(scale_factor),
                scale_factor,
                is_primary,
            }
        };
        Fake::with_world(
            vec![
                display(1, LogicalRect::new(0.0, 0.0, 8.0, 6.0), 2.0, true),
                display(2, LogicalRect::new(-4.0, -2.0, 4.0, 4.0), 1.0, false),
            ],
            Vec::new(),
        )
    }

    fn start(app: &mut App, mode: CaptureMode) -> Vec<AppMessage> {
        app.settle(AppMessage::Capture(Message::Start(mode)))
    }

    fn windows(app: &App, kind: WindowKind) -> usize {
        app.windows.of_kind(kind).count()
    }

    fn saved(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    /// The width and height in a PNG file's header.
    fn png_size(path: &Path) -> PhysicalSize {
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            chartreuse_imaging::Format::detect(&bytes),
            Some(chartreuse_imaging::Format::Png)
        );
        let field = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        PhysicalSize::new(field(16), field(20))
    }

    /// A capture backend that always answers `result`.
    struct Scripted(Result<Vec<DisplayCapture>>);

    impl Capture for Scripted {
        fn capture_displays(&self) -> BoxFuture<'static, Result<Vec<DisplayCapture>>> {
            future::ready(self.0.clone()).boxed()
        }

        fn capture_window(&self, _window: WindowId) -> BoxFuture<'static, Result<Image>> {
            future::ready(Err(Error::Unsupported("window capture"))).boxed()
        }
    }

    #[test]
    fn a_display_capture_composites_the_desktop_then_copies_and_saves_it() {
        let (mut app, _default_desktop, saves) = app();
        let fake = small_desktop();
        app.platform = fake.platform();
        let _ = start(&mut app, CaptureMode::Display);

        // The desktop spans 12 × 8 logical points, composited at the largest
        // scale factor, 2×.
        let desktop = fake.clipboard().expect("the capture was copied");
        assert_eq!(desktop.size(), PhysicalSize::new(24, 16));

        // The 2× primary display lands unscaled with its top-left, the global
        // origin, at pixel (4, 2) × 2.
        let captures = block_on(fake.platform().capture.capture_displays()).unwrap();
        let primary = captures.iter().find(|c| c.display.is_primary).unwrap();
        for (x, y) in [(0, 0), (5, 3), (15, 11)] {
            assert_eq!(
                desktop.pixel(8 + x, 4 + y),
                primary.image.pixel(x, y),
                "primary pixel ({x}, {y})"
            );
        }
        // Logical (-1, 4), below the 1× display and left of the primary, lies
        // on no display.
        let gap = desktop.pixel((4 - 1) * 2, (2 + 4) * 2).unwrap();
        assert_eq!(gap.a, 0, "gaps between displays are transparent");

        let files = saved(saves.path());
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(png_size(&files[0]), desktop.size());
        assert_eq!(app.capture.in_progress(), None);
        assert_eq!(windows(&app, WindowKind::Alert), 0);
    }

    /// The first message `subscription` delivers.
    fn next_message(subscription: Subscription<AppMessage>) -> AppMessage {
        let mut recipes = into_recipes(subscription);
        assert_eq!(recipes.len(), 1);
        let events = recipes
            .pop()
            .unwrap()
            .stream(futures::stream::empty().boxed());
        block_on(events.into_future()).0.expect("a message")
    }

    #[test]
    fn the_menu_and_the_display_hotkey_capture_the_desktop() {
        let (mut app, _default_desktop, saves) = app();
        let fake = small_desktop();
        app.platform = fake.platform();
        let _ = app.settle(AppMessage::Tray(tray::Message::Install));
        let _ = app.settle(AppMessage::Hotkeys(hotkeys::Message::Register));
        let desktop = PhysicalSize::new(24, 16);

        assert!(fake.choose_menu_action(MenuAction::Capture(CaptureMode::Display)));
        let chosen = next_message(tray::subscription(&app));
        let _ = app.settle(chosen);
        assert_eq!(fake.clipboard().map(|image| image.size()), Some(desktop));
        assert_eq!(saved(saves.path()).len(), 1);

        fake.set_clipboard(None);
        let hotkey = hotkeys::default_bindings()
            .into_iter()
            .find(|binding| binding.mode == CaptureMode::Display)
            .unwrap()
            .hotkey;
        assert!(fake.press_hotkey(hotkey));
        let pressed = next_message(hotkeys::subscription(&app));
        let _ = app.settle(pressed);
        assert_eq!(fake.clipboard().map(|image| image.size()), Some(desktop));
        assert_eq!(saved(saves.path()).len(), 2);
        assert_eq!(windows(&app, WindowKind::Alert), 0);
    }

    #[test]
    fn a_denied_permission_opens_the_guidance_and_captures_nothing() {
        let (mut app, fake, saves) = app();
        fake.set_screen_recording(PermissionStatus::Denied);

        for mode in [CaptureMode::Display, CaptureMode::Rectangle] {
            let handled = start(&mut app, mode);
            assert!(
                !handled
                    .iter()
                    .any(|message| matches!(message, AppMessage::Capture(Message::Captured(..)))),
                "{mode}: {handled:?}"
            );
            assert_eq!(windows(&app, WindowKind::Permission), 1, "{mode}");
            assert_eq!(windows(&app, WindowKind::Overlay), 0, "{mode}");
            assert_eq!(app.capture.in_progress(), None, "{mode}");
        }
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
    }

    #[test]
    fn a_capture_withheld_by_the_os_opens_the_guidance() {
        // The status says granted, but the capture itself is refused (or came
        // back blank, which the platform reports the same way).
        let (mut app, fake, saves) = app();
        app.platform.capture = Box::new(Scripted(Err(Error::PermissionDenied(
            Permission::ScreenRecording,
        ))));

        let _ = start(&mut app, CaptureMode::Display);
        assert_eq!(windows(&app, WindowKind::Permission), 1);
        assert_eq!(windows(&app, WindowKind::Alert), 0);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
    }

    #[test]
    fn capture_errors_are_reported_and_the_next_capture_can_start() {
        let (mut app, fake, saves) = app();
        app.platform.capture = Box::new(Scripted(Err(Error::Platform(
            "SCShareableContent failed".into(),
        ))));
        let _ = start(&mut app, CaptureMode::Display);
        // No displays at all do not form a layout.
        app.platform.capture = Box::new(Scripted(Ok(Vec::new())));
        let _ = start(&mut app, CaptureMode::Display);

        assert_eq!(windows(&app, WindowKind::Alert), 2);
        assert_eq!(windows(&app, WindowKind::Permission), 0);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
        assert_eq!(app.capture.in_progress(), None);
    }

    #[test]
    fn a_start_during_a_capture_is_ignored() {
        let (mut app, _fake, _saves) = app();
        let first = app.update(AppMessage::Capture(Message::Start(CaptureMode::Display)));
        assert!(iced_runtime::task::into_stream(first).is_some());
        assert_eq!(app.capture.in_progress(), Some(CaptureMode::Display));

        let second = app.update(AppMessage::Capture(Message::Start(CaptureMode::Display)));
        assert!(iced_runtime::task::into_stream(second).is_none());
    }

    #[test]
    fn window_captures_do_not_capture_yet() {
        let (mut app, fake, saves) = app();
        app.platform.capture = Box::new(Scripted(Err(Error::Platform("captured".into()))));
        assert_eq!(start(&mut app, CaptureMode::Window).len(), 1);
        assert_eq!(app.windows.len(), 0);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
    }

    fn overlays(app: &App) -> Vec<iced::window::Id> {
        app.windows.of_kind(WindowKind::Overlay).collect()
    }

    fn select(app: &mut App, input: Input) {
        let _ = app.settle(AppMessage::Overlay(overlay::Message::Rectangle(input)));
    }

    /// Starts a rectangle capture of the small desktop.
    fn start_rectangle() -> (App, Fake, tempfile::TempDir) {
        let (mut app, _default_desktop, saves) = app();
        let fake = small_desktop();
        app.platform = fake.platform();
        let _ = start(&mut app, CaptureMode::Rectangle);
        (app, fake, saves)
    }

    #[test]
    fn a_rectangle_capture_opens_one_overlay_per_display() {
        let (mut app, fake, saves) = start_rectangle();

        let covered: HashSet<DisplayId> = overlays(&app)
            .into_iter()
            .map(|window| app.overlay.display(window).expect("an overlay's display"))
            .collect();
        assert_eq!(overlays(&app).len(), 2);
        assert_eq!(covered, HashSet::from([DisplayId(1), DisplayId(2)]));
        assert_eq!(app.capture.in_progress(), Some(CaptureMode::Rectangle));
        assert_eq!(
            fake.clipboard(),
            None,
            "nothing is exported before a commit"
        );

        // One selection at a time.
        let _ = start(&mut app, CaptureMode::Rectangle);
        let _ = start(&mut app, CaptureMode::Display);
        assert_eq!(overlays(&app).len(), 2);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
    }

    #[test]
    fn a_committed_rectangle_is_cropped_at_the_largest_scale_then_copied_and_saved() {
        let (mut app, fake, saves) = start_rectangle();

        // From (-3, -1) on the 1× display to (2, 2) on the 2× primary: 5 × 3
        // logical points, output at 2×.
        let (from, to) = (LogicalPoint::new(-3.0, -1.0), LogicalPoint::new(2.0, 2.0));
        select(&mut app, Input::Press(from));
        select(&mut app, Input::Move(to));
        select(&mut app, Input::Release(to));

        assert!(overlays(&app).is_empty(), "the overlays closed");
        let image = fake.clipboard().expect("the selection was copied");
        assert_eq!(image.size(), PhysicalSize::new(10, 6));

        let captures = block_on(fake.platform().capture.capture_displays()).unwrap();
        let capture = |id| &captures.iter().find(|c| c.display.id == id).unwrap().image;
        // The primary's top-left, the global origin, is 3 × 1 points into the
        // selection, and lands unscaled.
        for (x, y) in [(0, 0), (1, 1), (3, 3)] {
            assert_eq!(
                image.pixel(6 + x, 2 + y),
                capture(DisplayId(1)).pixel(x, y),
                "primary pixel ({x}, {y})"
            );
        }
        // The selection's top-left is pixel (1, 1) of the 1× display, doubled.
        for (x, y) in [(0, 0), (1, 1)] {
            assert_eq!(image.pixel(x, y), capture(DisplayId(2)).pixel(1, 1));
        }
        // (1, -0.5) is above the primary and right of the 1× display.
        assert_eq!(image.pixel(8, 1).unwrap().a, 0, "gaps are transparent");

        let files = saved(saves.path());
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(png_size(&files[0]), image.size());
        assert_eq!(app.capture.in_progress(), None);
        assert_eq!(windows(&app, WindowKind::Alert), 0);
    }

    #[test]
    fn escape_closes_the_overlays_and_exports_nothing() {
        let (mut app, fake, saves) = start_rectangle();
        select(&mut app, Input::Press(LogicalPoint::new(1.0, 1.0)));
        select(&mut app, Input::Move(LogicalPoint::new(6.0, 5.0)));
        select(&mut app, Input::Escape);

        assert!(overlays(&app).is_empty());
        assert_eq!(app.capture.in_progress(), None);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());

        // Input after the session ends goes nowhere.
        select(&mut app, Input::Release(LogicalPoint::new(6.0, 5.0)));
        assert_eq!(fake.clipboard(), None);

        let _ = start(&mut app, CaptureMode::Rectangle);
        assert_eq!(overlays(&app).len(), 2, "a new capture can start");
    }

    #[test]
    fn an_overlay_closed_from_outside_cancels_the_selection() {
        let (mut app, fake, saves) = start_rectangle();
        let closed = overlays(&app)[0];
        let _ = app.settle(AppMessage::WindowClosed(closed));

        assert!(overlays(&app).is_empty(), "the other overlays closed too");
        assert_eq!(app.capture.in_progress(), None);
        assert_eq!(fake.clipboard(), None);
        assert!(saved(saves.path()).is_empty());
    }
}

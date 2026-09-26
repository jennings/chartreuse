//! Development harness for the overlay windows: real displays and the real
//! platform overlay style, but generated test patterns instead of a capture (no
//! ScreenCaptureKit, so no Screen Recording permission).
//!
//! ```text
//! cargo run -p chartreuse-overlay --example overlay_harness [-- --close-after <seconds>]
//! ```
//!
//! Enumerates the displays through `chartreuse_platform::current()` (`NSScreen`
//! on macOS), installs the status item so the process runs as an accessory app
//! the way Chartreuse does, and opens one overlay per display with
//! [`chartreuse_overlay::setup::open`], each showing that display's test pattern
//! under the rectangle selection. Drag to select; committing or pressing Escape
//! closes every overlay and quits. `--close-after` cancels on its own after the
//! given number of seconds, for checking the windows from a script.

use std::time::Duration;

use chartreuse_core::display::{DisplayId, DisplayInfo, DisplayLayout};
use chartreuse_core::flavor;
use chartreuse_core::geometry::LogicalRect;
use chartreuse_overlay::rectangle::{output_grid, Input, Outcome, RectangleOverlay, Selection};
use chartreuse_overlay::setup::{self, OverlayWindows, Styled};
use chartreuse_overlay::shared::frozen_image;
use chartreuse_platform::fake::test_pattern;
use chartreuse_platform::StatusItemHandle;
use iced::widget::image::Handle;
use iced::{window, Color, Element, Subscription, Task};

fn main() -> iced::Result {
    iced::daemon(Harness::boot, Harness::update, Harness::view)
        .subscription(Harness::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Start,
    Styled(Styled),
    Selection(Input),
    CloseAfter,
    Closed(window::Id),
}

/// One display with its test pattern.
struct Pattern {
    display: DisplayInfo,
    image: Handle,
}

#[derive(Default)]
struct Harness {
    close_after: Option<Duration>,
    /// Keeps the status item (and with it the accessory activation policy).
    _status_item: Option<StatusItemHandle>,
    selection: Option<Selection>,
    patterns: Vec<Pattern>,
    windows: OverlayWindows,
}

impl Harness {
    fn boot() -> (Self, Task<Message>) {
        let harness = Self {
            close_after: parse_close_after(),
            ..Self::default()
        };
        // AppKit setup waits for the event loop, so it happens in `update`.
        (harness, Task::done(Message::Start))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Start => self.start(),
            Message::Styled(Styled { window, result }) => {
                let name = self.display(window).map_or("?", |d| d.name.as_str());
                match result {
                    Ok(()) => println!("styled the overlay on {name} ({window:?})"),
                    Err(error) => eprintln!("could not style the overlay on {name}: {error}"),
                }
                Task::none()
            }
            Message::Selection(input) => {
                let Some(selection) = &mut self.selection else {
                    return Task::none();
                };
                match selection.apply(input) {
                    Some(Outcome::Commit(rect)) => {
                        describe_commit(selection.layout(), &rect);
                        self.windows.close_all()
                    }
                    Some(Outcome::Cancel) => {
                        println!("cancelled");
                        self.windows.close_all()
                    }
                    None => Task::none(),
                }
            }
            Message::CloseAfter => {
                println!("closing after the --close-after delay");
                self.windows.close_all()
            }
            Message::Closed(window) => {
                self.windows.remove(window);
                if self.windows.is_empty() {
                    iced::exit()
                } else {
                    Task::none()
                }
            }
        }
    }

    fn start(&mut self) -> Task<Message> {
        let platform = chartreuse_platform::current();
        match platform.status_item.install() {
            Ok(handle) => self._status_item = Some(handle),
            Err(error) => eprintln!("no status item ({error}); running as a regular app"),
        }
        let layout = match platform
            .displays
            .displays()
            .and_then(|displays| DisplayLayout::new(displays).map_err(Into::into))
        {
            Ok(layout) => layout,
            Err(error) => {
                eprintln!("could not list the displays: {error}");
                return iced::exit();
            }
        };

        println!("overlay harness: {} displays", layout.displays().len());
        self.patterns = layout
            .displays()
            .iter()
            .zip(0u8..)
            .map(|(display, index)| {
                println!(
                    "  {} (id {}): {} at {}×, {}×{} px{}",
                    display.name,
                    display.id.0,
                    describe(&display.logical_bounds),
                    display.scale_factor.get(),
                    display.pixel_size.width,
                    display.pixel_size.height,
                    if display.is_primary { ", primary" } else { "" }
                );
                let tint = index.wrapping_mul(85);
                Pattern {
                    display: display.clone(),
                    image: frozen_image(test_pattern(
                        display.pixel_size,
                        display.scale_factor,
                        tint,
                    )),
                }
            })
            .collect();
        println!("drag to select; commit or Escape closes the overlays and quits");

        let (windows, styled) = setup::open(&layout, &platform.overlay_style, window::open);
        for (window, display) in windows.iter() {
            println!("  overlay {window:?} covers display {}", display.0);
        }
        self.windows = windows;
        self.selection = Some(Selection::new(layout));

        let styled = styled.map(Message::Styled);
        match self.close_after {
            Some(delay) => Task::batch([
                styled,
                Task::future(after(delay)).map(|()| Message::CloseAfter),
            ]),
            None => styled,
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        let overlay = self.windows.display(window).and_then(|display| {
            let pattern = self.pattern(display)?;
            let selection = self.selection.as_ref()?;
            Some((pattern, selection))
        });
        match overlay {
            Some((pattern, selection)) => RectangleOverlay::new(
                selection,
                &pattern.display,
                &pattern.image,
                accent(),
                Message::Selection,
            )
            .view(),
            None => iced::widget::space().into(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        window::close_events().map(Message::Closed)
    }

    fn pattern(&self, display: DisplayId) -> Option<&Pattern> {
        self.patterns
            .iter()
            .find(|pattern| pattern.display.id == display)
    }

    fn display(&self, window: window::Id) -> Option<&DisplayInfo> {
        let display = self.windows.display(window)?;
        self.pattern(display).map(|pattern| &pattern.display)
    }
}

fn accent() -> Color {
    let accent = flavor::ACCENT;
    Color::from_rgba8(accent.r, accent.g, accent.b, f32::from(accent.a) / 255.0)
}

fn describe_commit(layout: &DisplayLayout, rect: &LogicalRect) {
    match output_grid(layout, rect) {
        Some(grid) => {
            let size = grid.pixel_size();
            println!(
                "committed {}: {}×{} px at {}×",
                describe(rect),
                size.width,
                size.height,
                grid.scale().get()
            );
        }
        None => println!("committed {} (outside every display)", describe(rect)),
    }
}

/// Resolves after `delay`, timed on a thread of its own so the executor, which
/// also drives the window tasks, never blocks.
async fn after(delay: Duration) {
    let (done, wait) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let _ = done.send(());
    });
    let _ = wait.await;
}

/// Parses `--close-after <seconds>`.
fn parse_close_after() -> Option<Duration> {
    let mut close_after = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--close-after" => {
                close_after = args
                    .next()
                    .and_then(|value| value.parse::<f64>().ok())
                    .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok());
                if close_after.is_none() {
                    eprintln!("--close-after needs a number of seconds; staying open");
                }
            }
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    close_after
}

fn describe(rect: &LogicalRect) -> String {
    format!(
        "({}, {}) {}×{} pt",
        rect.min_x(),
        rect.min_y(),
        rect.size.width,
        rect.size.height
    )
}

//! Development harness for the rectangle-selection overlay, on the `fake`
//! platform backend (no real capture, no Screen Recording permission).
//!
//! ```text
//! cargo run -p chartreuse-overlay --example rectangle_harness [-- --scale 0.5] [--demo]
//! ```
//!
//! Opens one ordinary window per fake display, laid out like the fake desktop and
//! scaled down to fit (or by `--scale`), each showing that display's generated
//! test-pattern capture. Drag in any window to select; the selection is shared
//! across the windows, and a drag can continue past its window onto the others.
//! On commit the harness prints the rectangle and its output size and writes the
//! cropped composite as a PNG to a temporary directory; Escape cancels. Either
//! way a new selection starts. Close any window to quit.
//!
//! `--demo` starts mid-drag on a sample selection spanning all three displays,
//! writes its crop as a commit would, and saves each window's rendering (iced's
//! own window screenshot, which needs no Screen Recording permission) to the
//! same directory.
//!
//! Windows are decorated and normal-level (overlay window setup belongs to
//! `chartreuse_overlay::setup`). Every window has the same title bar, so their
//! contents keep the desktop's relative layout.

use std::path::PathBuf;
use std::time::Duration;

use chartreuse_core::display::DisplayLayout;
use chartreuse_core::flavor;
use chartreuse_core::geometry::{LogicalPoint, LogicalRect, PhysicalSize};
use chartreuse_core::image::Image;
use chartreuse_imaging::{composite_at, encode, Format};
use chartreuse_overlay::rectangle::{output_grid, Input, Outcome, RectangleOverlay, Selection};
use chartreuse_overlay::shared::frozen_image;
use chartreuse_platform::fake::Fake;
use chartreuse_platform::DisplayCapture;
use iced::widget::image::Handle;
use iced::{window, Color, Element, Point, Size, Subscription, Task};

/// The screen area (logical points) the scaled-down desktop is fitted into.
const FIT: Size = Size::new(1400.0, 800.0);
/// Where the desktop's top-left corner goes on the screen.
const MARGIN: Point = Point::new(40.0, 60.0);

fn main() -> iced::Result {
    iced::daemon(Harness::boot, Harness::update, Harness::view)
        .title(Harness::title)
        .subscription(Harness::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Selection(Input),
    Screenshot(window::Id, window::Screenshot),
    WindowClosed,
}

/// Command-line options.
#[derive(Debug, Default)]
struct Args {
    scale: Option<f64>,
    demo: bool,
}

#[derive(Debug)]
struct Overlay {
    window: window::Id,
    capture: DisplayCapture,
    image: Handle,
}

#[derive(Debug)]
struct Harness {
    selection: Selection,
    overlays: Vec<Overlay>,
    accent: Color,
    output_dir: PathBuf,
    commits: usize,
}

impl Harness {
    fn boot() -> (Self, Task<Message>) {
        let platform = Fake::new().platform();
        let displays = platform
            .displays
            .displays()
            .expect("the fake backend lists displays");
        let layout = DisplayLayout::new(displays).expect("the fake displays form a layout");
        let captures = futures::executor::block_on(platform.capture.capture_displays())
            .expect("the fake backend captures");

        let args = parse_args();
        let desktop = layout.bounds();
        let scale = args.scale.unwrap_or_else(|| {
            let fit = (f64::from(FIT.width) / desktop.size.width)
                .min(f64::from(FIT.height) / desktop.size.height);
            fit.min(1.0)
        });
        let output_dir = std::env::temp_dir().join("chartreuse-rectangle-harness");
        println!(
            "rectangle harness: {} displays, shown at {scale:.2}×",
            captures.len()
        );
        for capture in &captures {
            let d = &capture.display;
            println!(
                "  {}: {} at {}× ({}×{} px)",
                d.name,
                describe(&d.logical_bounds),
                d.scale_factor.get(),
                d.pixel_size.width,
                d.pixel_size.height
            );
        }
        println!("drag to select, Escape cancels, close a window to quit");
        println!("commits are written to {}", output_dir.display());

        let mut tasks = Vec::new();
        let mut overlays = Vec::new();
        for capture in captures {
            let bounds = capture.display.logical_bounds;
            let (window, open) = window::open(window::Settings {
                size: Size::new(
                    (bounds.size.width * scale) as f32,
                    (bounds.size.height * scale) as f32,
                ),
                position: window::Position::Specific(Point::new(
                    MARGIN.x + ((bounds.origin.x - desktop.origin.x) * scale) as f32,
                    MARGIN.y + ((bounds.origin.y - desktop.origin.y) * scale) as f32,
                )),
                resizable: false,
                ..window::Settings::default()
            });
            tasks.push(if args.demo {
                open.then(|window| {
                    // A screenshot reads back the last frame drawn; give the new
                    // window time to draw its first one.
                    Task::future(async { std::thread::sleep(Duration::from_secs(2)) }).then(
                        move |()| {
                            window::screenshot(window)
                                .map(move |shot| Message::Screenshot(window, shot))
                        },
                    )
                })
            } else {
                open.discard()
            });
            overlays.push(Overlay {
                window,
                image: frozen_image(capture.image.clone()),
                capture,
            });
        }

        let accent = flavor::ACCENT;
        let mut selection = Selection::new(layout);
        if args.demo {
            // From the external display, across the primary, onto the portrait.
            selection.apply(Input::Press(LogicalPoint::new(-300.0, -100.0)));
            selection.apply(Input::Move(LogicalPoint::new(1700.0, 500.0)));
        }
        let mut harness = Self {
            selection,
            overlays,
            accent: Color::from_rgba8(accent.r, accent.g, accent.b, f32::from(accent.a) / 255.0),
            output_dir,
            commits: 0,
        };
        if let Some(rect) = harness.selection.rect() {
            // Crop the sample too, as a commit would, leaving it on screen.
            harness.commit(&rect);
        }
        (harness, Task::batch(tasks))
    }

    fn title(&self, window: window::Id) -> String {
        self.overlay(window).map_or_else(String::new, |overlay| {
            format!("Rectangle harness: {}", overlay.capture.display.name)
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Selection(input) => {
                match self.selection.apply(input) {
                    Some(Outcome::Commit(rect)) => self.commit(&rect),
                    Some(Outcome::Cancel) => println!("cancelled"),
                    None => return Task::none(),
                }
                self.selection = Selection::new(self.selection.layout().clone());
                Task::none()
            }
            Message::Screenshot(window, shot) => {
                let Some(overlay) = self.overlay(window) else {
                    return Task::none();
                };
                let name = format!("window-{}.png", overlay.capture.display.id.0);
                let size = PhysicalSize::new(shot.size.width, shot.size.height);
                match Image::new(size, shot.rgba.to_vec()) {
                    Ok(image) => self.save(&name, &image),
                    Err(error) => eprintln!("unusable screenshot of {name}: {error}"),
                }
                Task::none()
            }
            Message::WindowClosed => iced::exit(),
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        match self.overlay(window) {
            Some(overlay) => RectangleOverlay::new(
                &self.selection,
                &overlay.capture.display,
                &overlay.image,
                self.accent,
                Message::Selection,
            )
            .view(),
            None => iced::widget::space().into(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        window::close_events().map(|_| Message::WindowClosed)
    }

    fn overlay(&self, window: window::Id) -> Option<&Overlay> {
        self.overlays
            .iter()
            .find(|overlay| overlay.window == window)
    }

    fn commit(&mut self, rect: &LogicalRect) {
        let layout = self.selection.layout();
        let grid = output_grid(layout, rect).expect("committed selections have an output grid");
        let size = grid.pixel_size();
        let covered: Vec<&str> = layout
            .intersections(rect)
            .iter()
            .map(|part| part.display.name.as_str())
            .collect();
        println!(
            "committed {}: {}×{} px at {}× (on {})",
            describe(rect),
            size.width,
            size.height,
            grid.scale().get(),
            covered.join(", ")
        );

        let captures = self
            .overlays
            .iter()
            .map(|overlay| (&overlay.capture.display, &overlay.capture.image));
        match composite_at(captures, grid) {
            Ok(composite) => {
                self.commits += 1;
                self.save(&format!("selection-{}.png", self.commits), &composite.image);
            }
            Err(error) => eprintln!("  could not crop: {error}"),
        }
    }

    /// Writes `image` as a PNG named `name` in the output directory.
    fn save(&self, name: &str, image: &Image) {
        let path = self.output_dir.join(name);
        let written = encode(image, Format::Png)
            .map_err(|error| error.to_string())
            .and_then(|png| {
                std::fs::create_dir_all(&self.output_dir).map_err(|error| error.to_string())?;
                std::fs::write(&path, png).map_err(|error| error.to_string())
            });
        match written {
            Ok(()) => println!("  wrote {}", path.display()),
            Err(error) => eprintln!("  could not write {}: {error}", path.display()),
        }
    }
}

/// Parses `--scale <factor>` and `--demo`.
fn parse_args() -> Args {
    let mut parsed = Args::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => parsed.demo = true,
            "--scale" => {
                parsed.scale = args
                    .next()
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|scale| *scale > 0.0 && scale.is_finite());
                if parsed.scale.is_none() {
                    eprintln!("--scale needs a positive number; fitting to the screen");
                }
            }
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    parsed
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

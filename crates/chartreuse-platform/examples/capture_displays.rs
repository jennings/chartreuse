//! Captures every display with the current platform backend and writes one PNG per
//! display, for checking real captures by eye.
//!
//! ```sh
//! cargo run -p chartreuse-platform --example capture_displays [OUTPUT_DIR]
//! ```
//!
//! `OUTPUT_DIR` defaults to a fresh directory under the system temp dir. On macOS
//! the process that runs this (the terminal app) needs the Screen Recording
//! permission; without it the example reports `PermissionDenied` and exits with
//! status 2.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use chartreuse_core::Error;
use chartreuse_imaging::codec::{self, Format};
use futures::executor::block_on;

fn main() -> ExitCode {
    let dir = std::env::args_os().nth(1).map_or_else(
        || {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            std::env::temp_dir().join(format!("chartreuse-capture-{stamp}"))
        },
        PathBuf::from,
    );

    // Called on the main thread, as the Capture trait requires; the future is then
    // driven here while ScreenCaptureKit answers on its own queues.
    let platform = chartreuse_platform::current();
    let captures = match block_on(platform.capture.capture_displays()) {
        Ok(captures) => captures,
        Err(error @ Error::PermissionDenied(_)) => {
            eprintln!("capture failed: {error}");
            eprintln!("grant Screen Recording to the app running this example and retry");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("capture failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!("could not create {}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    for capture in &captures {
        let display = &capture.display;
        let path = dir.join(format!("display-{}.png", display.id.0));
        let png = match codec::encode(&capture.image, Format::Png) {
            Ok(png) => png,
            Err(error) => {
                eprintln!("could not encode display {}: {error}", display.id.0);
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = fs::write(&path, png) {
            eprintln!("could not write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!(
            "{} ({}{}): {}×{} px at {}× → {}",
            display.name,
            display.id.0,
            if display.is_primary { ", primary" } else { "" },
            capture.image.size().width,
            capture.image.size().height,
            display.scale_factor.get(),
            path.display()
        );
    }
    ExitCode::SUCCESS
}

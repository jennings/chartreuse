//! Chartreuse: a screenshot and annotation tool that lives in the menu bar.
//!
//! The app is an [`iced::daemon`]: it runs with no windows and opens them on
//! demand. See [`app`] for how state, messages, and windows are organized.
//!
//! The app is a library plus a thin `main.rs`, so the cross-module contracts
//! defined here ahead of their users are checked without dead-code noise.

pub mod alert;
pub mod app;
pub mod capture;
pub mod editor;
pub mod events;
pub mod export;
pub mod hotkeys;
pub mod import;
pub mod ipc;
pub mod overlay;
pub mod permission;
pub mod settings;
pub mod theme;
pub mod tray;
pub mod windows;

use chartreuse_core::flavor;
use tracing_subscriber::EnvFilter;

/// The log filter when `RUST_LOG` is unset: our own logs at `info`, and only
/// warnings from the chatty graphics stack.
const DEFAULT_LOG_FILTER: &str =
    "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,iced_wgpu=warn,iced_winit=warn";

/// Initializes logging and runs the daemon until the app exits.
pub fn run() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        bundle_id = flavor::BUNDLE_ID,
        "starting {}",
        flavor::DISPLAY_NAME
    );

    iced::daemon(
        || app::App::boot(ipc::State::default()),
        app::App::update,
        app::App::view,
    )
    .title(app::App::title)
    .theme(app::App::theme)
    .subscription(app::App::subscription)
    .run()
}

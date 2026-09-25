//! Selection overlays: one borderless, top-most window per display showing the
//! frozen capture, drawn with iced `Canvas` programs.
//!
//! Each module has its own owner, so tracks never edit the same file.

pub mod rectangle;
pub mod setup;
pub mod window;

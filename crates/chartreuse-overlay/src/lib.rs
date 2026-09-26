//! Selection overlays: one borderless, top-most window per display showing the
//! frozen capture, drawn with iced `Canvas` programs.
//!
//! Each module has its own owner, so tracks never edit the same file; `shared`
//! holds what the selection canvases have in common.

pub mod rectangle;
pub mod setup;
pub mod shared;
pub mod window;

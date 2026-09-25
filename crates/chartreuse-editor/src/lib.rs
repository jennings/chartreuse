//! The annotation editor: document model, tools, canvas, and flattening.
//!
//! [`Editor`] is the widget an editor window shows: it owns a
//! [`model::Document`], the active [tool](tools), the style for new
//! annotations, and the zoom and pan, and draws them with the [`canvas`].
//!
//! Each module has its own owner, so tracks never edit the same file.

pub mod canvas;
mod editor;
pub mod flatten;
pub mod font;
pub mod model;
pub mod tools;

pub use editor::{Editor, Event, Message};

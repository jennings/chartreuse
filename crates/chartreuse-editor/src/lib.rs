//! The annotation editor: document model, tools, canvas, and flattening.
//!
//! Each module has its own owner, so tracks never edit the same file.

pub mod canvas;
pub mod flatten;
pub mod font;
pub mod model;
pub mod tools;

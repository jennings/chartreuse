//! Pixel operations, image encoding, and decoding, on
//! [`chartreuse_core::image::Image`] buffers.
//!
//! Owned by track 1E.
//!
//! - [`region`]: crop and copy rectangles in physical pixels.
//! - [`composite`]: combine per-display captures into one image on a
//!   [`PixelGrid`](chartreuse_core::display::PixelGrid) of the desktop or of a
//!   selection.

pub mod composite;
pub mod region;

pub use composite::{composite_at, Composite};
pub use region::{copy_region, crop};

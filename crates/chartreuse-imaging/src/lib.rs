//! Pixel operations, image encoding, and decoding, on
//! [`chartreuse_core::image::Image`] buffers.
//!
//! Owned by track 1E.
//!
//! - [`region`]: crop and copy rectangles in physical pixels.

pub mod region;

pub use region::{copy_region, crop};

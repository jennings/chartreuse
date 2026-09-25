//! The document model: base image, ordered annotations, selection, and
//! command-based undo/redo. Pure logic. Owned by track 1G.
//!
//! # Coordinate space
//!
//! Everything in the model is in **base-image pixel coordinates**, as `f32`:
//! the origin is the top-left corner of the base image, `x` grows right, `y`
//! grows down, and one unit is one pixel of the base image (not a logical
//! point, and independent of the editor's zoom). Coordinates are continuous and
//! may fall outside the image; flattening clips to the image.
//!
//! The canvas maps between widget and document space with its own zoom/pan
//! transform, and must scale screen-space quantities (such as a hit-test
//! tolerance of a few logical points) into document units before calling the
//! model.

mod geometry;
mod style;

pub use geometry::{distance_to_segment, distance_to_triangle, Point, Rect, Size, Vector};
pub use style::{Style, StylePatch};

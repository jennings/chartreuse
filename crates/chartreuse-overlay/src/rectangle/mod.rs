//! Rectangle selection across every display. Owned by track 2D.
//!
//! # Design
//!
//! There is one overlay window per display (see [`crate::setup`]), but only one
//! selection: a [`Selection`] in **global logical desktop coordinates**, owned by
//! the app, which every display's overlay feeds [`Input`] into. A drag may start
//! on one display and end on another; the selection simply spans both.
//!
//! # Mixed scale factors
//!
//! A selection's output image is cropped at the **largest** scale factor among
//! the displays it covers ([`output_grid`], built on
//! `DisplayLayout::capture_grid`): a selection spanning a 1× and a 2× display is
//! output at 2×, the 1× part upscaled.

mod selection;

pub use selection::{output_grid, Input, Outcome, Phase, Selection, DRAG_THRESHOLD};

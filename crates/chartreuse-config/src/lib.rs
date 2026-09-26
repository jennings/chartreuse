//! Settings schema and persistence: a hand-editable TOML file in the platform
//! config directory derived from [`chartreuse_core::flavor::BUNDLE_ID`].
//!
//! Owned by track 1F.

pub mod format;
pub mod pattern;

pub use format::SaveFormat;
pub use pattern::{candidate_names, unique_file_name, FileNamePattern, PatternError};

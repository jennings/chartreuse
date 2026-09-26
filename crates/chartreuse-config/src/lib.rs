//! Settings schema and persistence: a hand-editable TOML file in the platform
//! config directory derived from [`chartreuse_core::flavor::BUNDLE_ID`].
//!
//! Owned by track 1F.

pub mod format;
pub mod pattern;
pub mod settings;

pub use format::SaveFormat;
pub use pattern::{candidate_names, unique_file_name, FileNamePattern, PatternError};
pub use settings::{
    default_save_directory, AfterCapture, DuplicateHotkey, Hotkeys, RelativeSaveDirectory,
    SaveDirectory, Settings,
};

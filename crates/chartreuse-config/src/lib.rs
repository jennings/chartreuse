//! Settings schema and persistence: a hand-editable TOML file in the platform
//! config directory derived from [`chartreuse_core::flavor::BUNDLE_ID`].
//!
//! - [`settings`]: [`Settings`] and its parts, with their defaults.
//! - [`file`](mod@file): where the file lives, [`load`] and [`save`].
//! - [`pattern`]: file name patterns and collision-free names.
//! - [`format`](mod@format): the save format.
//!
//! Owned by track 1F.

pub mod file;
pub mod format;
pub mod pattern;
pub mod settings;

pub use file::{config_dir, load, save, settings_path};
pub use format::SaveFormat;
pub use pattern::{candidate_names, unique_file_name, FileNamePattern, PatternError};
pub use settings::{
    default_save_directory, AfterCapture, DuplicateHotkey, Hotkeys, RelativeSaveDirectory,
    SaveDirectory, Settings,
};

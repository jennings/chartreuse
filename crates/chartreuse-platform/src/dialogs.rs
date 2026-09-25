//! Open and save dialogs.

use std::path::PathBuf;

use chartreuse_core::Result;
use futures::future::BoxFuture;

/// What [`FileDialogs::open_image`] shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenImageRequest {
    pub title: String,
    /// Selectable file extensions, lowercase and without the dot (`"png"`).
    pub extensions: Vec<String>,
}

/// What [`FileDialogs::save_image`] shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveImageRequest {
    pub title: String,
    /// The initial directory; `None` lets the OS choose.
    pub directory: Option<PathBuf>,
    /// The suggested file name, including its extension.
    pub file_name: String,
    /// Allowed file extensions, lowercase and without the dot.
    pub extensions: Vec<String>,
}

/// The platform's native open and save dialogs.
///
/// Call on the main thread (iced `update`). The dialog is shown without blocking
/// the event loop; the returned `Send + 'static` future (for `Task::perform`)
/// resolves with the chosen path, or `Ok(None)` if the user cancelled.
pub trait FileDialogs {
    fn open_image(&self, request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>>;

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>>;
}

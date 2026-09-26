//! The open and save dialogs of both Linux backends: the FileChooser portal,
//! which shows the desktop's own dialog (GTK or Qt) and works from a Flatpak
//! sandbox as well.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use chartreuse_core::{Error, Result};
use futures::future::{BoxFuture, FutureExt};

use super::logic::file_chooser::{extension_glob, file_uri_path};
use super::portal;
use crate::dialogs::{OpenImageRequest, SaveImageRequest};

const WHAT: &str = "the file dialog";

/// Asks for one image file to open.
pub fn open_image(request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
    async move {
        portal::register_app().await;
        let response = SelectedFiles::open_file()
            .title(request.title.as_str())
            .modal(true)
            .multiple(false)
            .filter(filter(&request.extensions))
            .send()
            .await
            .and_then(|request| request.response());
        chosen(response)
    }
    .boxed()
}

/// Asks where to save an image.
pub fn save_image(request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
    async move {
        portal::register_app().await;
        let dialog = SelectedFiles::save_file()
            .title(request.title.as_str())
            .modal(true)
            .current_name(request.file_name.as_str())
            .current_folder::<&Path>(request.directory.as_deref())
            .map_err(|error| portal::error(WHAT, &error))?
            .filter(filter(&request.extensions));
        let response = dialog.send().await.and_then(|request| request.response());
        chosen(response)
    }
    .boxed()
}

/// One filter offering every file with one of `extensions`.
fn filter(extensions: &[String]) -> FileFilter {
    extensions
        .iter()
        .fold(FileFilter::new("Images"), |filter, extension| {
            filter.glob(&extension_glob(extension))
        })
}

/// The file the user chose, `None` if they cancelled.
fn chosen(response: ashpd::Result<SelectedFiles>) -> Result<Option<PathBuf>> {
    let files = match response {
        Ok(files) => files,
        Err(error) if portal::cancelled(&error) => return Ok(None),
        Err(error) => return Err(portal::error(WHAT, &error)),
    };
    let Some(uri) = files.uris().first() else {
        return Ok(None);
    };
    let path = file_uri_path(uri.as_str())
        .ok_or_else(|| Error::Platform(format!("{WHAT} chose {uri}, which is not a local file")))?;
    Ok(Some(PathBuf::from(OsString::from_vec(path))))
}

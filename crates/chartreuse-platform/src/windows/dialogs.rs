//! Windows: open and save dialogs through `IFileOpenDialog` and `IFileSaveDialog`.
//!
//! Each dialog runs on a thread of its own, in a single-threaded COM apartment
//! (which the shell dialogs require), and resolves the returned future through a
//! oneshot channel. The call returns at once and winit's event loop on the main
//! thread keeps running. The dialog has no owner window, so Chartreuse's windows
//! stay usable while it is open, like the macOS panels.
//!
//! The requested extensions become one "Images" filter. The save dialog only
//! accepts those extensions: a name without one gets the suggested name's
//! extension (or the first allowed one) appended.

use std::ffi::{c_void, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::thread;

use ::windows::core::{HRESULT, PCWSTR};
use ::windows::Win32::Foundation::ERROR_CANCELLED;
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use ::windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use ::windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SHCreateItemFromParsingName, FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_STRICTFILETYPES, SIGDN_FILESYSPATH,
};
use chartreuse_core::{Error, Result};
use futures::channel::oneshot;
use futures::future::{self, BoxFuture, FutureExt};

use super::file_types::{default_extension, image_filter};
use super::util::{platform_error, wide};
use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};

/// The Windows [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct WindowsFileDialogs;

impl WindowsFileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for WindowsFileDialogs {
    fn open_image(&self, request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        on_dialog_thread(move || show_open(&request))
    }

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        on_dialog_thread(move || show_save(&request))
    }
}

/// Runs `show` on a new thread in a single-threaded COM apartment, resolving
/// with its result.
fn on_dialog_thread(
    show: impl FnOnce() -> Result<Option<PathBuf>> + Send + 'static,
) -> BoxFuture<'static, Result<Option<PathBuf>>> {
    let (sender, receiver) = oneshot::channel();
    let spawned = thread::Builder::new()
        .name("file dialog".into())
        .spawn(move || {
            let result = in_apartment(show);
            if let Err(error) = &result {
                tracing::warn!(%error, "the file dialog failed");
            }
            // The receiver is gone if the app stopped waiting; nothing to do.
            let _ = sender.send(result);
        });
    if let Err(error) = spawned {
        return future::ready(Err(Error::io("starting the file dialog thread", error))).boxed();
    }
    receiver
        .map(|answer| {
            answer.unwrap_or_else(|_| {
                Err(Error::Platform(
                    "the file dialog thread ended without an answer".into(),
                ))
            })
        })
        .boxed()
}

/// Runs `f` with COM initialized for this thread as a single-threaded apartment.
fn in_apartment<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    // SAFETY: called once on a fresh thread; balanced by CoUninitialize below.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|e| platform_error("CoInitializeEx", &e))?;
    let result = f();
    // SAFETY: balances the successful CoInitializeEx; the COM objects `f` used
    // were its locals, so they are already released.
    unsafe { CoUninitialize() };
    result
}

fn show_open(request: &OpenImageRequest) -> Result<Option<PathBuf>> {
    // SAFETY: COM is initialized on this thread (see `in_apartment`).
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| platform_error("CoCreateInstance(FileOpenDialog)", &e))?;
    configure(
        &dialog,
        &request.title,
        &request.extensions,
        FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM,
    )?;
    show(&dialog)
}

fn show_save(request: &SaveImageRequest) -> Result<Option<PathBuf>> {
    // SAFETY: COM is initialized on this thread (see `in_apartment`).
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| platform_error("CoCreateInstance(FileSaveDialog)", &e))?;
    let mut options = FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM;
    if !request.extensions.is_empty() {
        options |= FOS_STRICTFILETYPES;
    }
    configure(&dialog, &request.title, &request.extensions, options)?;

    let file_name = wide(&request.file_name);
    // SAFETY: the dialog is live and copies the NUL-terminated name.
    unsafe { dialog.SetFileName(PCWSTR(file_name.as_ptr())) }
        .map_err(|e| platform_error("IFileDialog::SetFileName", &e))?;
    if let Some(extension) = default_extension(&request.file_name, &request.extensions) {
        let extension = wide(extension);
        // SAFETY: as above.
        unsafe { dialog.SetDefaultExtension(PCWSTR(extension.as_ptr())) }
            .map_err(|e| platform_error("IFileDialog::SetDefaultExtension", &e))?;
    }
    if let Some(directory) = &request.directory {
        // A directory that no longer exists leaves the choice to Windows.
        match folder(directory) {
            // SAFETY: the dialog and the folder item are live.
            Ok(folder) => unsafe { dialog.SetFolder(&folder) }
                .map_err(|e| platform_error("IFileDialog::SetFolder", &e))?,
            Err(error) => {
                tracing::debug!(%error, directory = %directory.display(), "no initial folder");
            }
        }
    }
    show(&dialog)
}

/// Sets what both dialogs share: extra `options`, the title, and the filter.
fn configure(
    dialog: &IFileDialog,
    title: &str,
    extensions: &[String],
    options: FILEOPENDIALOGOPTIONS,
) -> Result<()> {
    let title = wide(title);
    // SAFETY: the dialog is live; it copies the NUL-terminated title.
    unsafe {
        let current = dialog
            .GetOptions()
            .map_err(|e| platform_error("IFileDialog::GetOptions", &e))?;
        dialog
            .SetOptions(current | options)
            .map_err(|e| platform_error("IFileDialog::SetOptions", &e))?;
        dialog
            .SetTitle(PCWSTR(title.as_ptr()))
            .map_err(|e| platform_error("IFileDialog::SetTitle", &e))?;
    }
    if let Some(filter) = image_filter(extensions) {
        let (name, spec) = (wide(&filter.name), wide(&filter.spec));
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        }];
        // SAFETY: the dialog is live; it copies the filter strings, which
        // outlive the call.
        unsafe { dialog.SetFileTypes(&filters) }
            .map_err(|e| platform_error("IFileDialog::SetFileTypes", &e))?;
    }
    Ok(())
}

/// Shows `dialog` modally on this thread and returns the chosen file system path,
/// or `None` if the user cancelled.
fn show(dialog: &IFileDialog) -> Result<Option<PathBuf>> {
    // SAFETY: the dialog is live and configured; with no owner it is modal to
    // nothing but this thread.
    match unsafe { dialog.Show(None) } {
        Ok(()) => {}
        Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => return Ok(None),
        Err(error) => return Err(platform_error("IFileDialog::Show", &error)),
    }
    // SAFETY: Show succeeded, so there is a result; FOS_FORCEFILESYSTEM makes it
    // a file system item, which has a SIGDN_FILESYSPATH name. That name is a
    // NUL-terminated string the caller frees with CoTaskMemFree.
    unsafe {
        let item = dialog
            .GetResult()
            .map_err(|e| platform_error("IFileDialog::GetResult", &e))?;
        let name = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| platform_error("IShellItem::GetDisplayName", &e))?;
        let path = OsString::from_wide(name.as_wide());
        CoTaskMemFree(Some(name.0.cast_const().cast::<c_void>()));
        Ok(Some(PathBuf::from(path)))
    }
}

/// The shell item of the folder at `directory`.
fn folder(directory: &Path) -> ::windows::core::Result<IShellItem> {
    let path: Vec<u16> = directory
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `path` is NUL-terminated and outlives the call; COM is initialized.
    unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
}

//! macOS: open and save dialogs through `NSOpenPanel` and `NSSavePanel`.
//!
//! Panels are shown modelessly with `beginWithCompletionHandler:`, so the call
//! returns at once and the event loop keeps running; the completion handler
//! resolves the returned future through a oneshot channel. Chartreuse is an
//! accessory (menu bar) app, so the app is activated first to bring the panel to
//! the front.

use std::cell::Cell;
use std::path::PathBuf;

use block2::RcBlock;
use chartreuse_core::{Error, Result};
use futures::channel::oneshot;
use futures::future::{self, BoxFuture, FutureExt};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{NSApplication, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSObjectProtocol, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};

/// The macOS [`FileDialogs`] backend.
#[derive(Debug, Default)]
pub struct MacosFileDialogs;

impl MacosFileDialogs {
    pub fn new() -> Self {
        Self
    }
}

impl FileDialogs for MacosFileDialogs {
    fn open_image(&self, request: OpenImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        let Some(mtm) = MainThreadMarker::new() else {
            return future::ready(Err(not_on_main_thread())).boxed();
        };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        configure(&panel, &request.title, &request.extensions);
        present(mtm, &panel)
    }

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        let Some(mtm) = MainThreadMarker::new() else {
            return future::ready(Err(not_on_main_thread())).boxed();
        };
        let panel = NSSavePanel::savePanel(mtm);
        panel.setCanCreateDirectories(true);
        panel.setNameFieldStringValue(&NSString::from_str(&request.file_name));
        if let Some(directory) = &request.directory {
            panel.setDirectoryURL(NSURL::from_directory_path(directory).as_deref());
        }
        configure(&panel, &request.title, &request.extensions);
        present(mtm, &panel)
    }
}

fn not_on_main_thread() -> Error {
    Error::Platform("file dialogs must be opened on the main thread".into())
}

/// Sets the title and the selectable file types shared by both panels.
fn configure(panel: &NSSavePanel, title: &str, extensions: &[String]) {
    let title = NSString::from_str(title);
    // Panels stopped showing their title bar in macOS 11; the message is what
    // the user sees, and the title still names the window (e.g. in Mission Control).
    panel.setTitle(Some(&title));
    panel.setMessage(Some(&title));
    panel.setAllowedContentTypes(&NSArray::from_retained_slice(&content_types(extensions)));
}

/// The uniform types for `extensions`, in order and without duplicates (`jpg` and
/// `jpeg` are both `public.jpeg`). Extensions the system does not know map to a
/// dynamic type that still matches files with that extension. An empty list
/// allows every file type.
fn content_types(extensions: &[String]) -> Vec<Retained<UTType>> {
    let mut types: Vec<Retained<UTType>> = Vec::with_capacity(extensions.len());
    for extension in extensions {
        let Some(found) = UTType::typeWithFilenameExtension(&NSString::from_str(extension)) else {
            tracing::warn!(extension, "no uniform type for file extension");
            continue;
        };
        if !types.iter().any(|known| known.isEqual(Some(&found))) {
            types.push(found);
        }
    }
    types
}

/// Shows `panel` modelessly and resolves with its chosen file once it closes.
fn present(
    mtm: MainThreadMarker,
    panel: &NSSavePanel,
) -> BoxFuture<'static, Result<Option<PathBuf>>> {
    let (sender, receiver) = oneshot::channel();
    // The handler is an `Fn` block but runs once; `Cell` lets it take the sender.
    let sender = Cell::new(Some(sender));
    // The handler owns the panel until it runs: that keeps the panel alive while
    // it is on screen, and AppKit releases the handler (and so the panel) after
    // calling it.
    let owned_panel = panel.retain();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        if let Some(sender) = sender.take() {
            // The receiver is gone only if the caller dropped the future.
            let _ = sender.send(chosen_path(&owned_panel, response));
        }
    });
    NSApplication::sharedApplication(mtm).activate();
    panel.beginWithCompletionHandler(&handler);
    receiver
        .map(|answer| {
            answer.unwrap_or_else(|_| {
                Err(Error::Platform(
                    "the file dialog closed without an answer".into(),
                ))
            })
        })
        .boxed()
}

/// The file chosen in a closed panel, or `None` if the user cancelled.
fn chosen_path(panel: &NSSavePanel, response: NSModalResponse) -> Result<Option<PathBuf>> {
    if response != NSModalResponseOK {
        return Ok(None);
    }
    panel
        .URL()
        .and_then(|url| url.to_file_path())
        .map(Some)
        .ok_or_else(|| Error::Platform("the file dialog returned no file path".into()))
}

#[cfg(test)]
mod tests {
    use objc2_uniform_type_identifiers::UTTypeImage;

    use super::*;

    fn identifiers(extensions: &[&str]) -> Vec<String> {
        let extensions: Vec<String> = extensions.iter().map(|&e| e.to_owned()).collect();
        content_types(&extensions)
            .iter()
            .map(|t| t.identifier().to_string())
            .collect()
    }

    #[test]
    fn extensions_map_to_their_image_types_in_order() {
        assert_eq!(
            identifiers(&["png", "jpg", "tiff", "gif", "heic"]),
            [
                "public.png",
                "public.jpeg",
                "public.tiff",
                "com.compuserve.gif",
                "public.heic"
            ]
        );
    }

    #[test]
    fn extensions_of_the_same_type_are_listed_once() {
        assert_eq!(
            identifiers(&["jpg", "png", "jpeg", "tif", "tiff"]),
            ["public.jpeg", "public.png", "public.tiff"]
        );
    }

    #[test]
    fn supported_image_extensions_conform_to_public_image() {
        let extensions =
            ["png", "jpg", "jpeg", "gif", "bmp", "tiff", "heic", "webp"].map(String::from);
        // SAFETY: a UniformTypeIdentifiers constant.
        let image = unsafe { UTTypeImage };
        for found in content_types(&extensions) {
            assert!(found.conformsToType(image), "{}", found.identifier());
        }
    }

    #[test]
    fn unknown_extensions_still_restrict_by_extension() {
        let found = content_types(&["chartreuse-test-unknown".to_owned()]);
        assert_eq!(found.len(), 1);
        assert!(found[0].isDynamic());
        assert_eq!(
            found[0]
                .preferredFilenameExtension()
                .map(|e| e.to_string())
                .as_deref(),
            Some("chartreuse-test-unknown")
        );
    }

    #[test]
    fn no_extensions_means_no_restriction() {
        assert!(content_types(&[]).is_empty());
    }
}

//! Opening an image from the clipboard or a file in an editor window. Owned by
//! integration task I4.
//!
//! The status item's "Open from Clipboard" and "Open File…" items arrive as
//! [`Message::FromClipboard`] and [`Message::FromFile`]; a file named on the
//! command line (`chartreuse open <file>`) arrives as [`Message::OpenPath`].
//!
//! - **Clipboard**: the image on the clipboard opens in a new editor window. A
//!   clipboard without an image, or one that cannot be read, is reported to the
//!   user.
//! - **File**: the platform's open dialog offers every format Chartreuse
//!   decodes ([`Format::ALL`]); the chosen file is decoded off the main thread
//!   and opens in a new editor window. A file that cannot be read or decoded is
//!   reported, naming the file. Cancelling the dialog does nothing. A file
//!   opened by path skips the dialog and is decoded and reported the same way.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::Format;
use chartreuse_platform::OpenImageRequest;
use iced::{Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::editor;

/// This feature's part of the app state ([`App::import`]).
#[derive(Debug, Default)]
pub struct State {}

/// This feature's messages ([`AppMessage::Import`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Open the image on the clipboard.
    FromClipboard,
    /// Ask for an image file to open.
    FromFile,
    /// The user answered the open dialog: the chosen file, or `None` if they
    /// cancelled.
    FileChosen(Result<Option<PathBuf>>),
    /// Open the image file at this path, without asking.
    OpenPath(PathBuf),
    /// The chosen file was decoded, or failed to be.
    Decoded(PathBuf, Result<Arc<Image>>),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::FromClipboard => from_clipboard(app),
        Message::FromFile => {
            let dialog = app.platform.file_dialogs.open_image(OpenImageRequest {
                title: "Open Image".into(),
                extensions: Format::ALL
                    .iter()
                    .flat_map(|format| format.extensions())
                    .map(|&extension| extension.to_owned())
                    .collect(),
            });
            Task::perform(dialog, |chosen| {
                AppMessage::Import(Message::FileChosen(chosen))
            })
        }
        Message::FileChosen(Ok(Some(path))) | Message::OpenPath(path) => Task::perform(
            async move {
                let image = chartreuse_imaging::decode_file(&path).map(Arc::new);
                (path, image)
            },
            |(path, image)| AppMessage::Import(Message::Decoded(path, image)),
        ),
        Message::FileChosen(Ok(None)) => {
            tracing::info!("open cancelled");
            Task::none()
        }
        Message::FileChosen(Err(error)) => alert::report_error(
            app,
            Notice::from_error("Could not choose a file to open", &error),
        ),
        Message::Decoded(path, Ok(image)) => {
            tracing::info!(path = %path.display(), "opened an image file");
            Task::done(AppMessage::Editor(editor::Message::Open(image)))
        }
        Message::Decoded(path, Err(error)) => alert::report_error(app, undecodable(&path, &error)),
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

/// The notice for a file at `path` that could not be opened: it names the
/// file.
fn undecodable(path: &Path, error: &Error) -> Notice {
    let name = path.file_name().unwrap_or(path.as_os_str());
    Notice::from_error(format!("Could not open “{}”", name.display()), error)
}

fn from_clipboard(app: &mut App) -> Task<AppMessage> {
    match app.platform.clipboard.read_image() {
        Ok(Some(image)) => Task::done(AppMessage::Editor(editor::Message::Open(Arc::new(image)))),
        Ok(None) => alert::report_error(
            app,
            Notice::from_error("Nothing to open", &Error::ClipboardEmpty),
        ),
        Err(error) => alert::report_error(
            app,
            Notice::from_error("Could not read the clipboard", &error),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_platform::MenuAction;

    use super::*;
    use crate::tray;
    use crate::windows::WindowKind;

    fn sample() -> Image {
        Image::from_fn(PhysicalSize::new(5, 4), |x, y| {
            Rgba8::new(
                u8::try_from(x * 50).unwrap(),
                u8::try_from(y * 60).unwrap(),
                200,
                u8::try_from(100 + x * 30).unwrap(),
            )
        })
    }

    /// Chooses `action` from the status item menu.
    fn choose(app: &mut App, action: MenuAction) {
        let _ = app.settle(AppMessage::Tray(tray::Message::Menu(action)));
    }

    /// The images of the open editor windows.
    fn editor_images(app: &App) -> Vec<Image> {
        app.windows
            .of_kind(WindowKind::Editor)
            .map(|window| app.editor.get(window).unwrap().document().base().clone())
            .collect()
    }

    fn alerts(app: &App) -> usize {
        app.windows.of_kind(WindowKind::Alert).count()
    }

    /// A clipboard that cannot be read.
    struct Unreadable;

    impl chartreuse_platform::Clipboard for Unreadable {
        fn write_image(&self, _image: &Image) -> Result<()> {
            Ok(())
        }
        fn read_image(&self) -> Result<Option<Image>> {
            Err(Error::Platform("the pasteboard server is gone".into()))
        }
    }

    #[test]
    fn the_image_on_the_clipboard_opens_in_an_editor() {
        let (mut app, fake) = App::for_test();
        fake.set_clipboard(Some(sample()));
        choose(&mut app, MenuAction::OpenFromClipboard);
        assert_eq!(editor_images(&app), [sample()]);
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn a_clipboard_without_an_image_is_reported() {
        let (mut app, fake) = App::for_test();
        fake.set_clipboard(None);
        choose(&mut app, MenuAction::OpenFromClipboard);
        assert_eq!(alerts(&app), 1);

        app.platform.clipboard = Box::new(Unreadable);
        choose(&mut app, MenuAction::OpenFromClipboard);
        assert_eq!(alerts(&app), 2);
        assert!(editor_images(&app).is_empty());
    }

    #[test]
    fn the_chosen_file_opens_in_an_editor() {
        let (mut app, fake) = App::for_test();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("picture.png");
        fs::write(
            &path,
            chartreuse_imaging::encode(&sample(), Format::Png).unwrap(),
        )
        .unwrap();
        fake.set_open_answer(Some(path));

        choose(&mut app, MenuAction::OpenFromFile);
        assert_eq!(editor_images(&app), [sample()]);
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn a_cancelled_open_does_nothing() {
        let (mut app, fake) = App::for_test();
        fake.set_open_answer(None);
        choose(&mut app, MenuAction::OpenFromFile);
        assert!(editor_images(&app).is_empty());
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_reported_by_name() {
        let (mut app, fake) = App::for_test();
        let temp = tempfile::tempdir().unwrap();
        let garbled = temp.path().join("notes.png");
        fs::write(&garbled, b"not an image at all").unwrap();

        fake.set_open_answer(Some(garbled));
        choose(&mut app, MenuAction::OpenFromFile);
        fake.set_open_answer(Some(temp.path().join("missing.png")));
        choose(&mut app, MenuAction::OpenFromFile);

        assert_eq!(alerts(&app), 2);
        assert!(editor_images(&app).is_empty());
        let mut titles: Vec<&str> = app
            .alert
            .notices()
            .map(|notice| notice.title.as_str())
            .collect();
        titles.sort_unstable();
        assert_eq!(
            titles,
            ["Could not open “missing.png”", "Could not open “notes.png”"]
        );
    }

    #[test]
    fn a_file_opened_by_path_opens_without_the_dialog_or_is_reported() {
        let (mut app, fake) = App::for_test();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("picture.png");
        fs::write(
            &path,
            chartreuse_imaging::encode(&sample(), Format::Png).unwrap(),
        )
        .unwrap();
        // The dialog would cancel: only the path given opens.
        fake.set_open_answer(None);

        let _ = app.settle(AppMessage::Import(Message::OpenPath(path)));
        assert_eq!(editor_images(&app), [sample()]);
        assert_eq!(alerts(&app), 0);

        let missing = temp.path().join("missing.png");
        let _ = app.settle(AppMessage::Import(Message::OpenPath(missing)));
        assert_eq!(editor_images(&app).len(), 1);
        let titles: Vec<&str> = app
            .alert
            .notices()
            .map(|notice| notice.title.as_str())
            .collect();
        assert_eq!(titles, ["Could not open “missing.png”"]);
    }
}

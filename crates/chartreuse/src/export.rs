//! Exporting an image: saving it as a file or copying it to the clipboard.
//! Owned by integration tasks I2 and I4.
//!
//! An editor window, or the capture flow, sends [`Message::Export`] with a
//! [`Target`] and a [`Request`]: the image (flattened, from an editor), when
//! it was taken, and the window to close once the export succeeds, if any.
//! Failures are reported to the user ([`alert::report_error`]) and leave the
//! window open, as does cancelling the save dialog.
//!
//! # Saving
//!
//! 1. The save directory from the settings ([`App::config`],
//!    [`Settings::save_directory_path`]: `~/Pictures/Chartreuse` by default) is
//!    created if missing, off the main thread, so the save dialog can start
//!    there. If it cannot be, the dialog starts where the platform chooses.
//! 2. The platform's save dialog ([`FileDialogs::save_image`]) asks where to
//!    save, suggesting the settings' file name pattern expanded with the time the
//!    image was taken, such as `Chartreuse 2026-09-25 at 14.03.07.png`. The
//!    dialog asks before replacing an existing file.
//! 3. The image is encoded and written to the chosen file off the main thread;
//!    the outcome arrives as [`Message::Saved`], and a successful save is logged
//!    with its path.
//!
//! # Saving without asking
//!
//! [`Target::SaveToDirectory`] (the capture flow's "save and copy" behavior)
//! skips the dialog: off the main thread, the save directory is created if
//! missing and the image is written into it, named by the file name pattern.
//! A file is never replaced: if the name is taken, ` (2)`, ` (3)`, … is added
//! ([`candidate_names`], with each file created exclusively). The outcome
//! arrives as [`Message::Saved`] too.
//!
//! Saves are PNG ([`SAVE_FORMAT`]); choosing the format (the settings'
//! `save_format`) is track 3D.
//!
//! # Copying
//!
//! The image is written to the clipboard in `update`: the platform clipboard is
//! main-thread only.
//!
//! [`FileDialogs::save_image`]: chartreuse_platform::FileDialogs::save_image
//! [`Settings::save_directory_path`]: chartreuse_config::Settings::save_directory_path

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chartreuse_config::{candidate_names, SaveFormat};
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use chartreuse_platform::SaveImageRequest;
use chrono::NaiveDateTime;
use iced::{window, Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};

/// The format images are saved in.
pub const SAVE_FORMAT: SaveFormat = SaveFormat::Png;

/// This feature's part of the app state ([`App::export`]).
#[derive(Debug, Default)]
pub struct State {}

/// Where an exported image goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// A file the user chooses in the save dialog.
    Save,
    /// A new file in the save directory, named by the file name pattern,
    /// without asking (see [Saving without asking](self#saving-without-asking)).
    SaveToDirectory,
    /// The clipboard.
    Copy,
}

impl Target {
    /// The headline of the notice reporting a failed export.
    #[must_use]
    pub const fn failure(self) -> &'static str {
        match self {
            Self::Save | Self::SaveToDirectory => "Could not save the image",
            Self::Copy => "Could not copy the image",
        }
    }
}

/// An image to export.
#[derive(Debug, Clone)]
pub struct Request {
    /// The image, with any annotations flattened into it.
    pub image: Arc<Image>,
    /// The local time the image was taken (or opened): names the saved file.
    pub taken: NaiveDateTime,
    /// A window to close once the export succeeds.
    pub then_close: Option<window::Id>,
}

/// This feature's messages ([`AppMessage::Export`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Export an image.
    Export(Target, Request),
    /// Ask where to save the image, starting in this directory (`None`: where
    /// the platform chooses).
    ChooseFile(Request, Option<PathBuf>),
    /// The user answered the save dialog: the chosen file, or `None` if they
    /// cancelled.
    FileChosen(Request, Result<Option<PathBuf>>),
    /// A save finished: the file written, or why it failed. The window, if any,
    /// closes on success.
    Saved(Option<window::Id>, Result<PathBuf>),
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Export(Target::Copy, request) => copy(app, &request.image, request.then_close),
        Message::Export(Target::Save, request) => {
            let directory = app.config.save_directory_path();
            Task::perform(
                async move { create_directory(directory) },
                move |directory| AppMessage::Export(Message::ChooseFile(request, directory)),
            )
        }
        Message::Export(Target::SaveToDirectory, request) => save_to_directory(app, request),
        Message::ChooseFile(request, directory) => choose_file(app, request, directory),
        Message::FileChosen(request, Ok(Some(path))) => {
            let then_close = request.then_close;
            Task::perform(
                async move { write(&request.image, &path).map(|()| path) },
                move |saved| AppMessage::Export(Message::Saved(then_close, saved)),
            )
        }
        Message::FileChosen(_, Ok(None)) => {
            tracing::info!("save cancelled");
            Task::none()
        }
        Message::Saved(then_close, Ok(path)) => {
            tracing::info!(path = %path.display(), "saved the image");
            close(then_close)
        }
        Message::FileChosen(_, Err(error)) | Message::Saved(_, Err(error)) => {
            report(app, Target::Save, &error)
        }
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

/// Copies `image` to the clipboard, then closes `then_close`, if any.
fn copy(app: &mut App, image: &Image, then_close: Option<window::Id>) -> Task<AppMessage> {
    match app.platform.clipboard.write_image(image) {
        Ok(()) => {
            tracing::info!("copied the image to the clipboard");
            close(then_close)
        }
        Err(error) => report(app, Target::Copy, &error),
    }
}

/// Creates `directory` if missing. Returns it, or `None` if there is none or
/// it cannot be created. Blocking: run it off the main thread.
fn create_directory(directory: Option<PathBuf>) -> Option<PathBuf> {
    let directory = directory?;
    match fs::create_dir_all(&directory) {
        Ok(()) => Some(directory),
        Err(error) => {
            tracing::warn!(
                directory = %directory.display(),
                %error,
                "could not create the save directory"
            );
            None
        }
    }
}

/// Shows the save dialog for `request`, starting in `directory`.
fn choose_file(app: &App, request: Request, directory: Option<PathBuf>) -> Task<AppMessage> {
    let dialog = app.platform.file_dialogs.save_image(SaveImageRequest {
        title: "Save Image".into(),
        directory,
        file_name: app.config.file_name.file_name(request.taken, SAVE_FORMAT),
        extensions: vec![SAVE_FORMAT.extension().into()],
    });
    Task::perform(dialog, move |chosen| {
        AppMessage::Export(Message::FileChosen(request, chosen))
    })
}

/// Encodes `image` in [`SAVE_FORMAT`] and writes it to `path`, replacing any
/// file there. Blocking: run it off the main thread.
///
/// # Errors
///
/// [`Error::Encode`] if encoding fails; [`Error::Io`] if the file cannot be
/// written.
fn write(image: &Image, path: &Path) -> Result<()> {
    let bytes = chartreuse_imaging::encode(image, SAVE_FORMAT.format())?;
    fs::write(path, bytes).map_err(|error| Error::io(format!("writing {}", path.display()), error))
}

/// Saves `request`'s image into the save directory without asking (see
/// [Saving without asking](self#saving-without-asking)), off the main thread.
fn save_to_directory(app: &App, request: Request) -> Task<AppMessage> {
    let directory = app.config.save_directory_path();
    let stem = app.config.file_name.expand(request.taken);
    let then_close = request.then_close;
    Task::perform(
        async move { write_new(&request.image, directory, &stem) },
        move |saved| AppMessage::Export(Message::Saved(then_close, saved)),
    )
}

/// Encodes `image` in [`SAVE_FORMAT`] and writes it into `directory`,
/// creating the directory if missing, as the first of [`candidate_names`] of
/// `stem` that is free. Never replaces a file. Returns the file written.
/// Blocking: run it off the main thread.
///
/// # Errors
///
/// [`Error::Config`] if there is no save directory (the default needs a home
/// folder); [`Error::Encode`] if encoding fails; [`Error::Io`] if the
/// directory or file cannot be written, or every candidate name is taken.
fn write_new(image: &Image, directory: Option<PathBuf>, stem: &str) -> Result<PathBuf> {
    let directory = directory.ok_or_else(|| {
        Error::Config("there is no folder to save in; choose one in the settings".into())
    })?;
    fs::create_dir_all(&directory)
        .map_err(|error| Error::io(format!("creating {}", directory.display()), error))?;
    let bytes = chartreuse_imaging::encode(image, SAVE_FORMAT.format())?;
    for name in candidate_names(stem, SAVE_FORMAT.extension()) {
        let path = directory.join(name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::io(format!("creating {}", path.display()), error)),
        };
        return match file.write_all(&bytes) {
            Ok(()) => Ok(path),
            Err(error) => {
                drop(file);
                // Do not leave a truncated image behind.
                let _ = fs::remove_file(&path);
                Err(Error::io(format!("writing {}", path.display()), error))
            }
        };
    }
    Err(Error::io(
        format!("saving in {}", directory.display()),
        io::Error::new(io::ErrorKind::AlreadyExists, "every file name is taken"),
    ))
}

/// Closes `window`, if any.
fn close(window: Option<window::Id>) -> Task<AppMessage> {
    window.map_or_else(Task::none, window::close)
}

fn report(app: &mut App, target: Target, error: &Error) -> Task<AppMessage> {
    alert::report_error(app, Notice::from_error(target.failure(), error))
}

#[cfg(test)]
mod tests {
    use chartreuse_config::SaveDirectory;
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chartreuse_imaging::Format;
    use chrono::NaiveDate;

    use super::*;
    use crate::windows::WindowKind;

    /// A test app whose saves go to `directory`.
    fn app_saving_to(directory: &Path) -> (App, chartreuse_platform::fake::Fake) {
        let (mut app, fake) = App::for_test();
        app.config.save_directory = Some(SaveDirectory::new(directory).unwrap());
        (app, fake)
    }

    fn sample() -> Image {
        Image::from_fn(PhysicalSize::new(7, 3), |x, y| {
            Rgba8::new(
                u8::try_from(x * 30).unwrap(),
                u8::try_from(y * 80).unwrap(),
                9,
                255,
            )
        })
    }

    /// A request to export [`sample`], taken on 2 January 2026 at 03:04:05.
    fn request(then_close: Option<window::Id>) -> Request {
        Request {
            image: Arc::new(sample()),
            taken: NaiveDate::from_ymd_opt(2026, 1, 2)
                .unwrap()
                .and_hms_opt(3, 4, 5)
                .unwrap(),
            then_close,
        }
    }

    fn save(app: &mut App, request: Request) {
        let _ = app.settle(AppMessage::Export(Message::Export(Target::Save, request)));
    }

    fn alerts(app: &App) -> usize {
        app.windows.of_kind(WindowKind::Alert).count()
    }

    /// The files in `directory`, sorted by name.
    fn files(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_save_dialog_starts_in_the_save_directory_with_the_patterned_name() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("Pictures").join("Chartreuse");
        let (mut app, fake) = app_saving_to(&directory);
        app.config.file_name = "Shot {yyyy}{MM}{dd}-{HH}{mm}{ss}".parse().unwrap();
        fake.set_save_answer(None);

        save(&mut app, request(None));

        assert_eq!(
            fake.save_requests(),
            [SaveImageRequest {
                title: "Save Image".into(),
                directory: Some(directory.clone()),
                file_name: "Shot 20260102-030405.png".into(),
                extensions: vec!["png".into()],
            }]
        );
        assert!(directory.is_dir(), "the save directory was created");
        assert!(files(&directory).is_empty(), "cancelled: nothing saved");
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn an_uncreatable_save_directory_leaves_the_start_to_the_platform() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("in the way");
        fs::write(&file, b"").unwrap();
        let (mut app, fake) = app_saving_to(&file.join("inside"));
        let chosen = temp.path().join("chosen.png");
        fake.set_save_answer(Some(chosen.clone()));

        save(&mut app, request(None));

        assert_eq!(fake.save_requests()[0].directory, None);
        assert_eq!(chartreuse_imaging::decode_file(&chosen).unwrap(), sample());
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn saving_replaces_the_chosen_file_with_a_png_then_closes_the_window() {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, fake) = app_saving_to(temp.path());
        let chosen = temp.path().join("existing.png");
        fs::write(&chosen, b"the dialog confirmed replacing this").unwrap();
        fake.set_save_answer(Some(chosen.clone()));
        let (window, _) = app
            .windows
            .open(WindowKind::Editor, window::Settings::default());

        save(&mut app, request(Some(window)));

        let bytes = fs::read(&chosen).unwrap();
        assert_eq!(Format::detect(&bytes), Some(Format::Png));
        assert_eq!(chartreuse_imaging::decode(&bytes).unwrap(), sample());
        assert_eq!(app.windows.kind(window), None, "the window closed");
    }

    fn save_without_asking(app: &mut App, request: Request) {
        let _ = app.settle(AppMessage::Export(Message::Export(
            Target::SaveToDirectory,
            request,
        )));
    }

    #[test]
    fn saving_to_the_directory_names_the_file_by_the_pattern_and_never_replaces_one() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("not yet there");
        let (mut app, fake) = app_saving_to(&directory);
        app.config.file_name = "Shot {HH}{mm}".parse().unwrap();

        save_without_asking(&mut app, request(None));
        fs::write(directory.join("Shot 0304 (2).png"), b"someone else's").unwrap();
        save_without_asking(&mut app, request(None));

        assert_eq!(
            files(&directory),
            ["Shot 0304 (2).png", "Shot 0304 (3).png", "Shot 0304.png"]
        );
        for saved in ["Shot 0304.png", "Shot 0304 (3).png"] {
            let decoded = chartreuse_imaging::decode_file(&directory.join(saved)).unwrap();
            assert_eq!(decoded, sample(), "{saved}");
        }
        assert_eq!(
            fs::read(directory.join("Shot 0304 (2).png")).unwrap(),
            b"someone else's"
        );
        assert!(fake.save_requests().is_empty(), "no dialog");
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn a_failed_save_to_the_directory_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("in the way");
        fs::write(&file, b"").unwrap();
        let (mut app, _fake) = app_saving_to(&file.join("inside"));

        save_without_asking(&mut app, request(None));

        assert_eq!(alerts(&app), 1);
        let notice = app.alert.notices().next().unwrap();
        assert_eq!(notice.title, "Could not save the image");
    }
}

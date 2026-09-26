//! Exporting an image: saving it as a file or copying it to the clipboard.
//! Owned by integration tasks I2 and I4.
//!
//! An editor window sends [`Message::Export`] with a [`Target`] and a
//! [`Request`]: the flattened image, when it was taken, and the window to close
//! once the export succeeds, if any. Failures are reported to the user
//! ([`alert::report_error`]) and leave the window open, as does cancelling the
//! save dialog.
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
//! Saves are PNG ([`SAVE_FORMAT`]); choosing the format (the settings'
//! `save_format`) is track 3D.
//!
//! # Copying
//!
//! The image is written to the clipboard in `update`: the platform clipboard is
//! main-thread only.
//!
//! # Quick export
//!
//! Until captures open in the editor, a finished capture arrives as
//! [`Message::CopyAndSave`]: the image is copied to the clipboard and saved as a
//! PNG in the save directory, with no dialog. Failures of either half are
//! reported to the user; the other half still happens.
//!
//! [`save_target`] decides the directory and the file name from the settings:
//! the save directory, created if missing, and the file name pattern expanded
//! with the local time, plus `.png`. An existing file is never overwritten: the
//! name gets a ` (2)`, ` (3)`, … suffix ([`file_name`]) until it is free. Files
//! are created exclusively, so two saves in the same second cannot pick the same
//! name.
//!
//! [`FileDialogs::save_image`]: chartreuse_platform::FileDialogs::save_image

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chartreuse_config::{SaveFormat, Settings};
use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::Format;
use chartreuse_platform::SaveImageRequest;
use chrono::NaiveDateTime;
use iced::{window, Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};

/// The format images are saved in.
pub const SAVE_FORMAT: SaveFormat = SaveFormat::Png;

/// How many ` (n)` suffixes to try before giving up on a file name.
const MAX_NAME_ATTEMPTS: u32 = 10_000;

/// This feature's part of the app state ([`App::export`]).
#[derive(Debug, Default)]
pub struct State {}

/// Where an exported image goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// A file the user chooses in the save dialog.
    Save,
    /// The clipboard.
    Copy,
}

impl Target {
    /// The headline of the notice reporting a failed export.
    #[must_use]
    pub const fn failure(self) -> &'static str {
        match self {
            Self::Save => "Could not save the image",
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
    /// Copy the image to the clipboard and save it as a PNG in the save
    /// directory: the post-capture action while captures do not open in the
    /// editor.
    CopyAndSave(Arc<Image>),
}

/// The file name for the `attempt`th try at saving as `stem`: `stem.ext` first,
/// then `stem (2).ext`, `stem (3).ext`, ….
#[must_use]
pub fn file_name(stem: &str, extension: &str, attempt: u32) -> String {
    if attempt <= 1 {
        format!("{stem}.{extension}")
    } else {
        format!("{stem} ({attempt}).{extension}")
    }
}

/// Where a save made at local time `now` goes, by `config`: the directory and
/// the file stem.
///
/// # Errors
///
/// [`Error::Config`] if no directory is configured and the platform has no
/// Pictures or home folder to default to.
pub fn save_target(config: &Settings, now: NaiveDateTime) -> Result<(PathBuf, String)> {
    let directory = config
        .save_directory_path()
        .ok_or_else(|| Error::Config("there is no Pictures or home folder to save into".into()))?;
    Ok((directory, config.file_name.expand(now)))
}

/// Encodes `image` as PNG and writes it to a new file named after `stem` in
/// `directory` (created if missing), never replacing an existing file. Returns
/// the new file's path. Blocking: run it off the main thread.
///
/// # Errors
///
/// [`Error::Encode`] if encoding fails; [`Error::Io`] if the directory or the
/// file cannot be created or written (a partly written file is removed).
pub fn save_png(image: &Image, directory: &Path, stem: &str) -> Result<PathBuf> {
    let bytes = chartreuse_imaging::encode(image, Format::Png)?;
    fs::create_dir_all(directory)
        .map_err(|error| Error::io(format!("creating {}", directory.display()), error))?;
    let (path, mut file) = create_unique(directory, stem, Format::Png.extension())?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        // Best effort: a truncated image is worse than none.
        let _ = fs::remove_file(&path);
        return Err(Error::io(format!("writing {}", path.display()), error));
    }
    Ok(path)
}

/// Creates the first free [`file_name`] for `stem` in `directory`.
fn create_unique(directory: &Path, stem: &str, extension: &str) -> Result<(PathBuf, File)> {
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let path = directory.join(file_name(stem, extension, attempt));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Error::io(format!("creating {}", path.display()), error)),
        }
    }
    Err(Error::io(
        format!(
            "choosing a file name for {stem:?} in {}",
            directory.display()
        ),
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{MAX_NAME_ATTEMPTS} files with that name already exist"),
        ),
    ))
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
        Message::CopyAndSave(image) => {
            let copied = copy(app, &image, None);
            Task::batch([copied, quick_save(app, image)])
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

/// Closes `window`, if any.
fn close(window: Option<window::Id>) -> Task<AppMessage> {
    window.map_or_else(Task::none, window::close)
}

fn report(app: &mut App, target: Target, error: &Error) -> Task<AppMessage> {
    alert::report_error(app, Notice::from_error(target.failure(), error))
}

/// Starts saving `image` to the save directory, with no dialog; the outcome
/// arrives as [`Message::Saved`].
fn quick_save(app: &mut App, image: Arc<Image>) -> Task<AppMessage> {
    let (directory, stem) = match save_target(&app.config, chrono::Local::now().naive_local()) {
        Ok(target) => target,
        Err(error) => return report(app, Target::Save, &error),
    };
    Task::perform(
        async move { save_png(&image, &directory, &stem) },
        |saved| AppMessage::Export(Message::Saved(None, saved)),
    )
}

#[cfg(test)]
mod tests {
    use chartreuse_config::SaveDirectory;
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
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

    #[test]
    fn later_attempts_get_a_numbered_suffix() {
        assert_eq!(file_name("Shot", "png", 1), "Shot.png");
        assert_eq!(file_name("Shot", "png", 2), "Shot (2).png");
        assert_eq!(file_name("Shot", "png", 13), "Shot (13).png");
    }

    #[test]
    fn saving_creates_the_directory_and_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("Pictures").join("Chartreuse");
        let image = sample();

        let first = save_png(&image, &directory, "Shot").unwrap();
        let other = Image::filled(PhysicalSize::new(2, 2), Rgba8::WHITE);
        let second = save_png(&other, &directory, "Shot").unwrap();
        fs::write(directory.join("Shot (3).png"), b"taken").unwrap();
        let fourth = save_png(&image, &directory, "Shot").unwrap();

        assert_eq!(first, directory.join("Shot.png"));
        assert_eq!(second, directory.join("Shot (2).png"));
        assert_eq!(fourth, directory.join("Shot (4).png"));
        assert_eq!(chartreuse_imaging::decode_file(&first).unwrap(), image);
        assert_eq!(chartreuse_imaging::decode_file(&second).unwrap(), other);
        assert_eq!(fs::read(directory.join("Shot (3).png")).unwrap(), b"taken");
    }

    #[test]
    fn saving_into_an_uncreatable_directory_fails() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("not a directory");
        fs::write(&file, b"").unwrap();
        let error = save_png(&sample(), &file.join("inside"), "Shot").unwrap_err();
        assert!(matches!(error, Error::Io { .. }), "{error:?}");
    }

    #[test]
    fn the_target_follows_the_configured_directory_and_file_name_pattern() {
        let now = NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap();
        let config = Settings {
            save_directory: Some(SaveDirectory::new("/somewhere").unwrap()),
            file_name: "Shot {yyyy}{MM}{dd}-{HH}{mm}{ss}".parse().unwrap(),
            ..Settings::default()
        };
        let (directory, stem) = save_target(&config, now).unwrap();
        assert_eq!(directory, Path::new("/somewhere"));
        assert_eq!(stem, "Shot 20260102-030405");
    }

    #[test]
    fn copy_and_save_copies_and_writes_a_png() {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, fake) = app_saving_to(temp.path());
        let image = sample();

        let handled = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(
            image.clone(),
        ))));

        assert_eq!(fake.clipboard(), Some(image.clone()));
        let saved = handled.iter().find_map(|message| match message {
            AppMessage::Export(Message::Saved(None, saved)) => Some(saved.clone()),
            _ => None,
        });
        let path = saved.expect("a save finished").unwrap();
        assert_eq!(path.parent(), Some(temp.path()));
        assert_eq!(files(temp.path()).len(), 1);
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("Chartreuse ") && name.ends_with(".png"),
            "{name}"
        );
        assert_eq!(chartreuse_imaging::decode_file(&path).unwrap(), image);
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn a_failed_copy_is_reported_and_the_image_still_saved() {
        struct Broken;
        impl chartreuse_platform::Clipboard for Broken {
            fn write_image(&self, _image: &Image) -> Result<()> {
                Err(Error::Platform("the pasteboard refused".into()))
            }
            fn read_image(&self) -> Result<Option<Image>> {
                Ok(None)
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let (mut app, _fake) = app_saving_to(temp.path());
        app.platform.clipboard = Box::new(Broken);

        let _ = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(sample()))));
        assert_eq!(alerts(&app), 1);
        assert_eq!(files(temp.path()).len(), 1);
    }

    #[test]
    fn a_failed_save_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("in the way");
        fs::write(&file, b"").unwrap();
        let (mut app, fake) = app_saving_to(&file.join("inside"));

        let _ = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(sample()))));
        assert_eq!(alerts(&app), 1);
        assert_eq!(fake.clipboard(), Some(sample()), "the copy still happens");
    }
}

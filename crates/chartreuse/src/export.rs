//! Exporting an image: copying it to the clipboard and saving it as a file.
//! Owned by integration tasks I2 and I4.
//!
//! # Quick export
//!
//! Until the editor (I4) exists, a finished capture arrives as
//! [`Message::CopyAndSave`]: the image is copied to the clipboard and saved as a
//! PNG in the save directory. Failures of either half are reported to the user
//! ([`alert::report_error`]); the other half still happens. A successful save is
//! logged with its path.
//!
//! # Where files go
//!
//! [`save_target`] decides the directory and the file name, and is where the
//! settings (1F's config, 3A's settings window) plug in:
//!
//! - the directory is [`State::directory`] if set, else [`default_directory`]
//!   (`~/Pictures/Chartreuse`); it is created if missing;
//! - the file name is [`file_stem`] of the local time, such as
//!   `Chartreuse 2026-09-25 at 14.03.07`, plus `.png`.
//!
//! An existing file is never overwritten: the name gets a ` (2)`, ` (3)`, …
//! suffix ([`file_name`]) until it is free. Files are created exclusively, so two
//! saves in the same second cannot pick the same name.
//!
//! # Threading
//!
//! The clipboard write happens in `update` (the platform clipboard is main-thread
//! only). PNG encoding and the file write run in a [`Task`] on the executor's
//! thread pool, so a large composite does not stall the UI; the outcome arrives
//! as [`Message::Saved`].

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chartreuse_core::image::Image;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::Format;
use chrono::NaiveDateTime;
use iced::{Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};

/// The folder inside the user's Pictures folder that saves go to by default.
const DEFAULT_FOLDER: &str = "Chartreuse";

/// How many ` (n)` suffixes to try before giving up on a file name.
const MAX_NAME_ATTEMPTS: u32 = 10_000;

/// This feature's part of the app state ([`App::export`]).
#[derive(Debug, Default)]
pub struct State {
    /// The directory saves go to, overriding [`default_directory`]. Set from the
    /// settings once they exist (1F/3A), and by tests.
    pub directory: Option<PathBuf>,
}

/// This feature's messages ([`AppMessage::Export`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Copy the image to the clipboard and save it as a PNG in the save
    /// directory: the post-capture action while there is no editor.
    CopyAndSave(Arc<Image>),
    /// A save finished: the new file, or why it failed.
    Saved(Result<PathBuf>),
}

/// The default save directory: `Chartreuse` in the user's Pictures folder
/// (`~/Pictures/Chartreuse` on macOS). `None` if the platform reports no
/// Pictures or home folder.
#[must_use]
pub fn default_directory() -> Option<PathBuf> {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Pictures")))
        .map(|pictures| pictures.join(DEFAULT_FOLDER))
}

/// The file name, without extension, of an image saved at local time `time`:
/// `Chartreuse 2026-09-25 at 14.03.07`. Dots separate the time because macOS
/// Finder shows `:` in file names as `/`.
#[must_use]
pub fn file_stem(time: NaiveDateTime) -> String {
    time.format("Chartreuse %Y-%m-%d at %H.%M.%S").to_string()
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

/// Where a save made at local time `now` goes: the directory and the file stem.
///
/// # Errors
///
/// [`Error::Config`] if no directory is configured and the platform has no
/// Pictures or home folder to default to.
pub fn save_target(state: &State, now: NaiveDateTime) -> Result<(PathBuf, String)> {
    let directory = match &state.directory {
        Some(directory) => directory.clone(),
        None => default_directory().ok_or_else(|| {
            Error::Config("there is no Pictures or home folder to save into".into())
        })?,
    };
    Ok((directory, file_stem(now)))
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
        Message::CopyAndSave(image) => {
            let copied = copy(app, &image);
            Task::batch([copied, save(app, image)])
        }
        Message::Saved(Ok(path)) => {
            tracing::info!(path = %path.display(), "saved the capture");
            Task::none()
        }
        Message::Saved(Err(error)) => alert::report_error(
            app,
            Notice::from_error("Could not save the capture", &error),
        ),
    }
}

pub fn subscription(_app: &App) -> Subscription<AppMessage> {
    Subscription::none()
}

/// Copies `image` to the clipboard; returns an alert task if that failed.
fn copy(app: &mut App, image: &Image) -> Task<AppMessage> {
    match app.platform.clipboard.write_image(image) {
        Ok(()) => {
            tracing::info!("copied the capture to the clipboard");
            Task::none()
        }
        Err(error) => alert::report_error(
            app,
            Notice::from_error("Could not copy the capture", &error),
        ),
    }
}

/// Starts saving `image` to the save directory; the outcome arrives as
/// [`Message::Saved`].
fn save(app: &mut App, image: Arc<Image>) -> Task<AppMessage> {
    let (directory, stem) = match save_target(&app.export, chrono::Local::now().naive_local()) {
        Ok(target) => target,
        Err(error) => {
            return alert::report_error(
                app,
                Notice::from_error("Could not save the capture", &error),
            );
        }
    };
    Task::perform(
        async move { save_png(&image, &directory, &stem) },
        |saved| AppMessage::Export(Message::Saved(saved)),
    )
}

#[cfg(test)]
mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;
    use chrono::NaiveDate;

    use super::*;
    use crate::windows::WindowKind;

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
    fn file_names_are_timestamped_and_zero_padded() {
        let time = NaiveDate::from_ymd_opt(2026, 9, 5)
            .unwrap()
            .and_hms_opt(4, 3, 7)
            .unwrap();
        assert_eq!(file_stem(time), "Chartreuse 2026-09-05 at 04.03.07");
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
    fn a_configured_directory_overrides_the_default() {
        let now = NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap();
        let state = State {
            directory: Some(PathBuf::from("/somewhere")),
        };
        let (directory, stem) = save_target(&state, now).unwrap();
        assert_eq!(directory, Path::new("/somewhere"));
        assert_eq!(stem, file_stem(now));
    }

    #[test]
    fn copy_and_save_copies_and_writes_a_png() {
        let (mut app, fake) = App::for_test();
        let temp = tempfile::tempdir().unwrap();
        app.export.directory = Some(temp.path().to_owned());
        let image = sample();

        let handled = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(
            image.clone(),
        ))));

        assert_eq!(fake.clipboard(), Some(image.clone()));
        let saved = handled.iter().find_map(|message| match message {
            AppMessage::Export(Message::Saved(saved)) => Some(saved.clone()),
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

        let (mut app, _fake) = App::for_test();
        app.platform.clipboard = Box::new(Broken);
        let temp = tempfile::tempdir().unwrap();
        app.export.directory = Some(temp.path().to_owned());

        let _ = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(sample()))));
        assert_eq!(alerts(&app), 1);
        assert_eq!(files(temp.path()).len(), 1);
    }

    #[test]
    fn a_failed_save_is_reported() {
        let (mut app, fake) = App::for_test();
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("in the way");
        fs::write(&file, b"").unwrap();
        app.export.directory = Some(file.join("inside"));

        let _ = app.settle(AppMessage::Export(Message::CopyAndSave(Arc::new(sample()))));
        assert_eq!(alerts(&app), 1);
        assert_eq!(fake.clipboard(), Some(sample()), "the copy still happens");
    }
}

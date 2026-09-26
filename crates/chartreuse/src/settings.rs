//! The settings window (`WindowKind::Settings`), and keeping [`App::config`]
//! in step with the settings file. Owned by track 3A.
//!
//! # The window
//!
//! The status item's Settings item ([`Message::Open`]) opens the window, or
//! brings it to the front if it is already open: there is at most one. Every
//! change applies at once, to [`App::config`], and is saved.
//!
//! - **After a capture**: open the editor, copy the capture, or save and copy
//!   it ([`AfterCapture`], applied by the capture flow).
//! - **Saving**: the folder saves go to, typed as a full path or one starting
//!   with `~` ([`SaveDirectory`]), or left empty for the default; the
//!   platform's file dialogs cannot choose a folder. Then the file name
//!   pattern ([`FileNamePattern`]), with a preview of the name a capture taken
//!   now would get. Text that is not valid says why under its field and
//!   changes nothing until it is.
//!
//! ## Adding a setting
//!
//! Each row of the window is a `setting` (a label and a control) in a
//! `section` of [`view`], and every change goes through `change`, which
//! applies it and saves it. A change is a function of the settings, so that
//! it can be made again on top of a file edited by hand (see
//! [Saving](self#saving)). So a new setting takes a [`Message`] variant, an
//! [`update`] arm, and a row. For example, a toggle in a new section:
//!
//! ```ignore
//! // In `update`:
//! Message::LaunchAtLogin(on) => change(app, move |config| config.launch_at_login = on),
//! // In `view`:
//! section("General", [setting(
//!     "Launch at login",
//!     toggler(app.config.launch_at_login)
//!         .on_toggle(|on| AppMessage::Settings(Message::LaunchAtLogin(on))),
//! )]),
//! ```
//!
//! # Saving
//!
//! Changes are saved off the main thread by syncing with the file: it is
//! loaded, the changes made in the window since the last sync are made again
//! on top of what it holds, the result is written
//! ([`chartreuse_config::save`]), and it becomes [`App::config`], with the
//! changes made meanwhile on top. So a hand edit the watcher has not seen yet
//! is kept, not overwritten, and the settings in memory are always those in
//! the file plus the changes still to save. Loading the file after a hand
//! edit (below) is a sync with no changes, and there is one sync at a time: a
//! change or hand edit that comes during a sync is synced once it ends.
//!
//! A sync that fails (say, the file was edited into invalid TOML) is reported
//! once per problem, like a hand edit that does not load, and shown at the
//! bottom of the window. The changes still apply, and are saved by the next
//! sync: the next change, or the hand edit that fixes the file.
//!
//! # Picking up hand edits
//!
//! [`App::boot`] loads the settings file ([`chartreuse_config::settings_path`])
//! into [`App::config`]. The file is meant to be edited by hand too, so while
//! the app runs [`subscription`] checks it every [`POLL_INTERVAL`] on a thread
//! of its own. When it changed, it is loaded again off the main thread and
//! applied: [`App::config`] is replaced, and the hotkeys are re-registered if
//! they changed (failures are reported as at startup). A file that no longer
//! loads (invalid TOML or an invalid setting) leaves the settings as they
//! were and is reported, once per problem: the same problem is not reported
//! again until the file loads or a different problem turns up. Deleting the
//! file means the defaults, as at startup.
//!
//! The check compares the file's modification time and size with those it
//! had when the app last read or wrote it. Polling, rather than file system
//! notifications (the `notify` crate), because saves replace the file by
//! renaming another over it (ours, and many editors'), which a watch on the
//! file itself loses track of, and its directory may not exist yet: one
//! `stat` every two seconds handles all of that, on every platform, without a
//! dependency.

use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use chartreuse_config::pattern::DEFAULT_PATTERN;
use chartreuse_config::{
    default_save_directory, AfterCapture, FileNamePattern, RelativeSaveDirectory, SaveDirectory,
    Settings,
};
use chartreuse_core::{Error, Result};
use chrono::NaiveDateTime;
use iced::futures::channel::mpsc;
use iced::widget::{button, column, container, radio, row, scrollable, space, text, text_input};
use iced::{window, Element, Length, Size, Subscription, Task};
use parking_lot::Mutex;

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::export::SAVE_FORMAT;
use crate::hotkeys;
use crate::windows::WindowKind;

/// How often the settings file is checked for hand edits.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The size the settings window opens at.
const WINDOW_SIZE: Size = Size::new(640.0, 560.0);

/// The width of the labels down the left of the settings window.
const LABEL_WIDTH: f32 = 150.0;

/// The hint under the file name field.
const TOKENS_HINT: &str =
    "{date} {time} {yyyy} {MM} {dd} {HH} {mm} {ss} become the capture's date and time";

/// This feature's part of the app state ([`App::settings`]).
#[derive(Debug, Default)]
pub struct State {
    /// The settings file; `None` if the platform has no place for it, and
    /// then nothing is saved or watched.
    file: Option<SettingsFile>,
    /// The last problem reported with the settings file, so that it is
    /// reported once.
    problem: Option<String>,
    /// The open settings window.
    window: Option<SettingsWindow>,
    /// Changes made in the window and not yet in the file: made to
    /// [`App::config`] at once, and to the file by the next sync.
    pending: Vec<Edit>,
    /// The changes the running sync is saving; `None` if none runs.
    syncing: Option<Vec<Edit>>,
    /// The file changed during a sync: load it once the sync ends.
    reload: bool,
}

/// This feature's messages ([`AppMessage::Settings`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Open the settings window, or bring it to the front (the status item's
    /// Settings item).
    Open,
    /// The folder field was edited.
    SaveDirectory(String),
    /// The folder's Default button.
    DefaultSaveDirectory,
    /// The file name field was edited.
    FileName(String),
    /// The file name's Default button.
    DefaultFileName,
    /// A post-capture behavior was chosen.
    AfterCapture(AfterCapture),
    /// The settings file changed on disk, other than by the app itself.
    FileChanged,
    /// A sync with the settings file ended: the settings it holds now, or why
    /// it could not be loaded or written.
    Synced(Result<Settings>),
}

/// A change to the settings, kept until the file has it.
#[derive(Clone)]
struct Edit(Arc<dyn Fn(&mut Settings) + Send + Sync>);

impl Edit {
    fn apply(&self, settings: &mut Settings) {
        (self.0)(settings);
    }
}

impl fmt::Debug for Edit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Edit")
    }
}

/// The open settings window: what its text fields hold.
#[derive(Debug)]
struct SettingsWindow {
    id: window::Id,
    /// The save directory, as written; empty for the default.
    directory: Field,
    file_name: Field,
    /// The time the file name preview is for: when the window opened or the
    /// file name last changed, so that the preview holds still in between.
    previewed: NaiveDateTime,
    /// The default save directory, shown in the empty folder field.
    default_directory: String,
}

impl SettingsWindow {
    fn new(id: window::Id, config: &Settings) -> Self {
        let mut shown = Self {
            id,
            directory: Field::default(),
            file_name: Field::default(),
            previewed: chrono::Local::now().naive_local(),
            default_directory: default_save_directory()
                .map(|directory| directory.display().to_string())
                .unwrap_or_default(),
        };
        shown.show(config);
        shown
    }

    /// Shows `config` in the text fields, dropping text that was not valid.
    fn show(&mut self, config: &Settings) {
        self.directory = Field::new(
            config
                .save_directory
                .as_ref()
                .map(|directory| directory.as_path().display().to_string())
                .unwrap_or_default(),
        );
        self.file_name = Field::new(config.file_name.to_string());
        self.previewed = chrono::Local::now().naive_local();
    }
}

/// A text field whose text applies whenever it is valid.
#[derive(Debug, Default)]
struct Field {
    text: String,
    /// Why `text` does not apply, if it does not.
    error: Option<String>,
}

impl Field {
    fn new(text: String) -> Self {
        Self { text, error: None }
    }

    /// Takes `text` as typed, returning the value to apply if `parse` accepts
    /// it; otherwise remembers why not.
    fn edit<T, E: ToString>(
        &mut self,
        text: String,
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Option<T> {
        let parsed = parse(&text);
        self.text = text;
        match parsed {
            Ok(value) => {
                self.error = None;
                Some(value)
            }
            Err(error) => {
                self.error = Some(alert::capitalize(&error.to_string()));
                None
            }
        }
    }
}

/// The folder field's text as a save directory: empty (or blank) for the
/// default.
fn parse_directory(text: &str) -> Result<Option<SaveDirectory>, RelativeSaveDirectory> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        SaveDirectory::new(text).map(Some)
    }
}

/// The settings file, shared with the thread that watches it.
#[derive(Debug, Clone)]
struct SettingsFile {
    path: Arc<Path>,
    /// The file's [`Stamp`] when the app last read or wrote it; a different
    /// one means someone else changed it.
    known: Arc<Mutex<Option<Stamp>>>,
    /// How often the watcher checks.
    interval: Duration,
}

/// What tells one version of a file from another: its modification time and
/// size. `None` for a missing (or unreadable) file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        fs::metadata(path).ok().map(|metadata| Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

impl SettingsFile {
    /// The settings file at `path`, checked every [`POLL_INTERVAL`]. Only
    /// changes from now on count.
    fn new(path: PathBuf) -> Self {
        Self::with_interval(path, POLL_INTERVAL)
    }

    fn with_interval(path: PathBuf, interval: Duration) -> Self {
        let known = Stamp::of(&path);
        Self {
            path: path.into(),
            known: Arc::new(Mutex::new(known)),
            interval,
        }
    }

    /// Whether `stamp` is the file's as the app last read or wrote it.
    fn is_known(&self, stamp: Option<Stamp>) -> bool {
        *self.known.lock() == stamp
    }

    /// Loads the file and makes `edits` on top of what it holds, writing the
    /// result ([`chartreuse_config::save`]) if there are any; returns the
    /// settings the file then holds. All under the lock, and what is read or
    /// written becomes known, so the watcher reports neither. Blocking.
    fn sync(&self, edits: &[Edit]) -> Result<Settings> {
        let mut known = self.known.lock();
        // Known even if it does not load, so that the watcher reports it just
        // once; stamped before reading, so that an edit made meanwhile shows.
        *known = Stamp::of(&self.path);
        let mut settings = chartreuse_config::load(&self.path)?;
        if edits.is_empty() {
            return Ok(settings);
        }
        for edit in edits {
            edit.apply(&mut settings);
        }
        chartreuse_config::save(&self.path, &settings)?;
        *known = Stamp::of(&self.path);
        Ok(settings)
    }
}

/// Identifies the watcher's subscription: one per file and interval.
impl Hash for SettingsFile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.interval.hash(state);
    }
}

/// Checks `file` every interval on a thread of its own, yielding once per
/// change. The thread ends soon after the stream is dropped.
fn watch(file: &SettingsFile) -> mpsc::Receiver<()> {
    let (mut changes, stream) = mpsc::channel(1);
    let file = file.clone();
    let spawned = thread::Builder::new()
        .name("settings watcher".into())
        .spawn(move || {
            // The version last reported, so that it is reported once: the app
            // reads it soon, and then it is known.
            let mut reported = *file.known.lock();
            while !changes.is_closed() {
                thread::sleep(file.interval);
                let now = Stamp::of(&file.path);
                if now == reported || file.is_known(now) {
                    continue;
                }
                reported = now;
                // A full channel already holds a change for the app to load.
                if changes
                    .try_send(())
                    .is_err_and(|error| error.is_disconnected())
                {
                    break;
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "cannot watch the settings file for changes");
    }
    stream
}

pub fn boot(app: &mut App) -> Task<AppMessage> {
    app.settings.file = chartreuse_config::settings_path()
        .ok()
        .map(SettingsFile::new);
    Task::none()
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Open => open(app),
        Message::SaveDirectory(text) => {
            let Some(shown) = &mut app.settings.window else {
                return Task::none();
            };
            match shown.directory.edit(text, parse_directory) {
                Some(directory) => change(app, move |config| {
                    config.save_directory.clone_from(&directory);
                }),
                None => Task::none(),
            }
        }
        Message::DefaultSaveDirectory => {
            if let Some(shown) = &mut app.settings.window {
                shown.directory = Field::default();
            }
            change(app, |config| config.save_directory = None)
        }
        Message::FileName(text) => {
            let Some(shown) = &mut app.settings.window else {
                return Task::none();
            };
            shown.previewed = chrono::Local::now().naive_local();
            match shown
                .file_name
                .edit(text, |text| FileNamePattern::new(text))
            {
                Some(pattern) => change(app, move |config| config.file_name = pattern.clone()),
                None => Task::none(),
            }
        }
        Message::DefaultFileName => {
            if let Some(shown) = &mut app.settings.window {
                shown.file_name = Field::new(DEFAULT_PATTERN.to_owned());
                shown.previewed = chrono::Local::now().naive_local();
            }
            change(app, |config| config.file_name = FileNamePattern::default())
        }
        Message::AfterCapture(after) => change(app, move |config| config.after_capture = after),
        Message::FileChanged => {
            app.settings.reload = true;
            sync(app)
        }
        Message::Synced(synced) => synced_with(app, synced),
    }
}

pub fn subscription(app: &App) -> Subscription<AppMessage> {
    match &app.settings.file {
        Some(file) => Subscription::run_with(file.clone(), watch)
            .map(|()| AppMessage::Settings(Message::FileChanged)),
        None => Subscription::none(),
    }
}

pub fn view(app: &App, window: window::Id) -> Element<'_, AppMessage> {
    let Some(shown) = app
        .settings
        .window
        .as_ref()
        .filter(|shown| shown.id == window)
    else {
        return space().into();
    };
    let config = &app.config;
    let after_capture = column(AfterCapture::ALL.map(|after| {
        radio(
            after.to_string(),
            after,
            Some(config.after_capture),
            |after| AppMessage::Settings(Message::AfterCapture(after)),
        )
        .into()
    }))
    .spacing(8);
    let folder = column![
        row![
            text_input(&shown.default_directory, &shown.directory.text)
                .on_input(|text| AppMessage::Settings(Message::SaveDirectory(text))),
            default_button(
                config.save_directory.is_some() || !shown.directory.text.is_empty(),
                Message::DefaultSaveDirectory,
            ),
        ]
        .spacing(8),
        note(directory_note(config, &shown.directory)),
    ]
    .spacing(4);
    let file_name = column![
        row![
            text_input(DEFAULT_PATTERN, &shown.file_name.text)
                .on_input(|text| AppMessage::Settings(Message::FileName(text))),
            default_button(
                config.file_name != FileNamePattern::default() || shown.file_name.error.is_some(),
                Message::DefaultFileName,
            ),
        ]
        .spacing(8),
        note(file_name_note(config, &shown.file_name, shown.previewed)),
        note(Note::Hint(TOKENS_HINT.to_owned())),
    ]
    .spacing(4);

    let mut content = column![
        section("Capturing", [setting("After a capture", after_capture)]),
        section(
            "Saving",
            [setting("Folder", folder), setting("File name", file_name)]
        ),
    ]
    .spacing(24);
    if let Some(problem) = &app.settings.problem {
        content = content.push(note(Note::Problem(alert::capitalize(problem))));
    }
    scrollable(container(content).padding(24).width(Length::Fill)).into()
}

pub fn window_closed(app: &mut App, window: window::Id) -> Task<AppMessage> {
    if app
        .settings
        .window
        .as_ref()
        .is_some_and(|shown| shown.id == window)
    {
        app.settings.window = None;
    }
    Task::none()
}

/// Opens the settings window, or brings it to the front if it is open.
fn open(app: &mut App) -> Task<AppMessage> {
    if let Some(shown) = &app.settings.window {
        return window::gain_focus(shown.id);
    }
    let (id, open) = app.windows.open(
        WindowKind::Settings,
        window::Settings {
            size: WINDOW_SIZE,
            resizable: false,
            minimizable: false,
            position: window::Position::Centered,
            ..window::Settings::default()
        },
    );
    app.settings.window = Some(SettingsWindow::new(id, &app.config));
    // The app has no Dock icon, so bring the window forward explicitly.
    open.discard().chain(window::gain_focus(id))
}

/// Applies `edit` to the settings and saves it, if that changed anything.
fn change(app: &mut App, edit: impl Fn(&mut Settings) + Send + Sync + 'static) -> Task<AppMessage> {
    let edit = Edit(Arc::new(edit));
    let before = app.config.clone();
    edit.apply(&mut app.config);
    if app.config == before {
        return Task::none();
    }
    app.settings.pending.push(edit);
    sync(app)
}

/// Syncs with the settings file off the main thread, or once the running
/// sync ends (see [Saving](self#saving)).
fn sync(app: &mut App) -> Task<AppMessage> {
    let Some(file) = app.settings.file.clone() else {
        app.settings.pending.clear();
        return Task::none();
    };
    if app.settings.syncing.is_some() {
        return Task::none();
    }
    if std::mem::take(&mut app.settings.reload) {
        tracing::info!(path = %file.path.display(), "the settings file changed; loading it");
    }
    let edits = std::mem::take(&mut app.settings.pending);
    app.settings.syncing = Some(edits.clone());
    Task::perform(async move { file.sync(&edits) }, |synced| {
        AppMessage::Settings(Message::Synced(synced))
    })
}

/// A sync ended: takes on the settings the file holds, and starts the next
/// sync if one is due.
fn synced_with(app: &mut App, synced: Result<Settings>) -> Task<AppMessage> {
    let saved = app.settings.syncing.take().unwrap_or_default();
    let due = app.settings.reload || !app.settings.pending.is_empty();
    let outcome = match synced {
        Ok(settings) => {
            app.settings.problem = None;
            adopt(app, settings)
        }
        Err(error) => {
            let title = if saved.is_empty() {
                "Keeping the current settings"
            } else {
                "Could not save the settings"
            };
            // They still apply: the next sync saves them.
            app.settings.pending.splice(0..0, saved);
            report_once(app, title, &error)
        }
    };
    if due {
        Task::batch([outcome, sync(app)])
    } else {
        outcome
    }
}

/// Makes `settings`, from the file, the app's, with the changes still to
/// save on top.
fn adopt(app: &mut App, mut settings: Settings) -> Task<AppMessage> {
    for edit in &app.settings.pending {
        edit.apply(&mut settings);
    }
    if settings == app.config {
        return Task::none();
    }
    tracing::info!("applying the changed settings file");
    let hotkeys_changed = settings.hotkeys != app.config.hotkeys;
    app.config = settings;
    if let Some(shown) = &mut app.settings.window {
        shown.show(&app.config);
    }
    if hotkeys_changed {
        hotkeys::reregister(app, hotkeys::bindings(&app.config.hotkeys))
    } else {
        Task::none()
    }
}

/// Reports a problem with the settings file, unless it is the problem last
/// reported.
fn report_once(app: &mut App, title: &str, error: &Error) -> Task<AppMessage> {
    let problem = error.to_string();
    if app.settings.problem.as_ref() == Some(&problem) {
        tracing::debug!(%problem, "already reported");
        return Task::none();
    }
    app.settings.problem = Some(problem);
    alert::report_error(app, Notice::from_error(title, error))
}

/// A line under a control.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Note {
    Hint(String),
    Problem(String),
}

/// The line under the folder field: where saves go, or why the text is not
/// a folder.
fn directory_note(config: &Settings, field: &Field) -> Note {
    if let Some(error) = &field.error {
        return Note::Problem(error.clone());
    }
    match config.save_directory_path() {
        Some(directory) => Note::Hint(format!("Saves go to {}", directory.display())),
        None => Note::Problem("There is no Pictures folder to save in: enter a folder".into()),
    }
}

/// The line under the file name field: the name of a capture taken at `time`,
/// or why the text is not a pattern.
fn file_name_note(config: &Settings, field: &Field, time: NaiveDateTime) -> Note {
    match &field.error {
        Some(error) => Note::Problem(error.clone()),
        None => Note::Hint(format!(
            "For example: {}",
            config.file_name.file_name(time, SAVE_FORMAT)
        )),
    }
}

/// A titled group of [`setting`]s.
fn section<'a>(
    title: &'a str,
    rows: impl IntoIterator<Item = Element<'a, AppMessage>>,
) -> Element<'a, AppMessage> {
    column![text(title).size(18), column(rows).spacing(16)]
        .spacing(12)
        .into()
}

/// One setting: its label on the left, its control on the right.
fn setting<'a>(
    label: &'a str,
    control: impl Into<Element<'a, AppMessage>>,
) -> Element<'a, AppMessage> {
    row![
        container(text(label)).width(LABEL_WIDTH).padding([5, 0]),
        container(control).width(Length::Fill),
    ]
    .spacing(12)
    .into()
}

/// A button that resets a setting to its default; disabled if it is.
fn default_button<'a>(enabled: bool, message: Message) -> Element<'a, AppMessage> {
    button(text("Default"))
        .style(button::secondary)
        .on_press_maybe(enabled.then_some(AppMessage::Settings(message)))
        .into()
}

fn note<'a>(note: Note) -> Element<'a, AppMessage> {
    let (line, style): (String, fn(&iced::Theme) -> text::Style) = match note {
        Note::Hint(line) => (line, text::secondary),
        Note::Problem(line) => (line, text::danger),
    };
    text(line).size(13).style(style).into()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc as std_mpsc;

    use chartreuse_core::capture::CaptureMode;
    use chartreuse_core::hotkey::Hotkey;
    use chartreuse_platform::fake::Fake;
    use chartreuse_platform::MenuAction;
    use chrono::NaiveDate;
    use futures::executor::block_on;
    use futures::StreamExt;
    use iced::advanced::subscription::into_recipes;
    use iced_runtime::Action;
    use tempfile::TempDir;

    use super::*;
    use crate::tray;

    /// A test app keeping its settings in a file in a new temporary
    /// directory, with the hotkeys registered.
    fn app_with_file() -> (App, Fake, TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, fake) = App::for_test();
        app.settings.file = Some(SettingsFile::new(temp.path().join("settings.toml")));
        let _ = app.settle(AppMessage::Hotkeys(hotkeys::Message::Register));
        (app, fake, temp)
    }

    fn file(app: &App) -> &SettingsFile {
        app.settings.file.as_ref().unwrap()
    }

    /// Edits the settings file by hand, then does what the watcher does when
    /// it sees the change.
    fn edit(app: &mut App, text: &str) {
        let file = file(app).clone();
        fs::write(&file.path, text).unwrap();
        assert!(changed(&file), "a hand edit is a change");
        let _ = app.settle(AppMessage::Settings(Message::FileChanged));
    }

    /// Whether the watcher would report the file.
    fn changed(file: &SettingsFile) -> bool {
        !file.is_known(Stamp::of(&file.path))
    }

    /// Runs `task`, a sync, to its end, and hands its outcome to the app.
    fn finish(app: &mut App, task: Task<AppMessage>) {
        for action in actions(task) {
            let Action::Output(synced) = action else {
                panic!("expected the sync's outcome, got {action:?}");
            };
            let _ = app.settle(synced);
        }
    }

    fn alerts(app: &App) -> usize {
        app.windows.of_kind(WindowKind::Alert).count()
    }

    fn hotkey(text: &str) -> Hotkey {
        text.parse().unwrap()
    }

    fn send(app: &mut App, message: Message) {
        let _ = app.settle(AppMessage::Settings(message));
    }

    /// Everything `task` does, in order.
    fn actions(task: Task<AppMessage>) -> Vec<Action<AppMessage>> {
        iced_runtime::task::into_stream(task)
            .map(|stream| block_on(stream.collect()))
            .unwrap_or_default()
    }

    fn shown(app: &App) -> &SettingsWindow {
        app.settings
            .window
            .as_ref()
            .expect("the settings window is open")
    }

    fn settings_windows(app: &App) -> Vec<window::Id> {
        app.windows.of_kind(WindowKind::Settings).collect()
    }

    /// The settings in the settings file.
    fn on_disk(app: &App) -> Settings {
        chartreuse_config::load(&file(app).path).unwrap()
    }

    /// 25 September 2026 at 14:03:07.
    fn afternoon() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, 25)
            .unwrap()
            .and_hms_opt(14, 3, 7)
            .unwrap()
    }

    #[test]
    fn the_status_item_opens_one_settings_window() {
        let (mut app, fake) = App::for_test();
        let _ = app.settle(AppMessage::Tray(tray::Message::Install));
        assert!(fake.choose_menu_action(MenuAction::Settings));
        let mut recipes = into_recipes(tray::subscription(&app));
        let chosen = block_on(
            recipes
                .pop()
                .unwrap()
                .stream(futures::stream::empty().boxed())
                .into_future(),
        )
        .0
        .unwrap();
        let _ = app.settle(chosen);
        let opened = settings_windows(&app);
        assert_eq!(opened.len(), 1);

        // Chosen again, it brings the open window to the front.
        let again = app.update(AppMessage::Tray(tray::Message::Menu(MenuAction::Settings)));
        let again = actions(again);
        let [Action::Output(open)] = again.as_slice() else {
            panic!("expected Open, got {again:?}");
        };
        let focus = actions(app.update(open.clone()));
        assert!(
            matches!(
                focus.as_slice(),
                [Action::Window(iced_runtime::window::Action::GainFocus(id))] if *id == opened[0]
            ),
            "{focus:?}"
        );
        assert_eq!(settings_windows(&app), opened);

        // Closed, it opens anew.
        let _ = app.settle(AppMessage::WindowClosed(opened[0]));
        assert!(app.settings.window.is_none());
        send(&mut app, Message::Open);
        let reopened = settings_windows(&app);
        assert_eq!(reopened.len(), 1);
        assert_ne!(reopened, opened);
    }

    #[test]
    fn changes_apply_at_once_and_are_saved_without_counting_as_hand_edits() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);

        send(&mut app, Message::AfterCapture(AfterCapture::SaveAndCopy));

        assert_eq!(app.config.after_capture, AfterCapture::SaveAndCopy);
        assert_eq!(on_disk(&app), app.config);
        assert!(!changed(file(&app)), "the app's own save");
    }

    #[test]
    fn the_file_name_field_previews_valid_patterns_and_explains_invalid_ones() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);

        send(&mut app, Message::FileName("Shot {HH}{mm}".into()));
        assert_eq!(app.config.file_name.as_str(), "Shot {HH}{mm}");
        assert_eq!(
            file_name_note(&app.config, &shown(&app).file_name, afternoon()),
            Note::Hint("For example: Shot 1403.png".into())
        );

        send(&mut app, Message::FileName("Shot {hour}".into()));
        assert_eq!(
            app.config.file_name.as_str(),
            "Shot {HH}{mm}",
            "not applied"
        );
        assert_eq!(shown(&app).file_name.text, "Shot {hour}");
        let note = file_name_note(&app.config, &shown(&app).file_name, afternoon());
        assert!(
            matches!(&note, Note::Problem(problem) if problem.contains("{hour} is not a file name token")),
            "{note:?}"
        );
        assert_eq!(on_disk(&app).file_name.as_str(), "Shot {HH}{mm}");

        send(&mut app, Message::DefaultFileName);
        assert_eq!(app.config.file_name, FileNamePattern::default());
        assert_eq!(shown(&app).file_name.text, DEFAULT_PATTERN);
        assert_eq!(
            file_name_note(&app.config, &shown(&app).file_name, afternoon()),
            Note::Hint("For example: Chartreuse 2026-09-25 at 14.03.07.png".into())
        );
        assert_eq!(on_disk(&app).file_name, FileNamePattern::default());
    }

    #[test]
    fn the_folder_field_takes_full_paths_and_empty_means_the_default() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);

        send(&mut app, Message::SaveDirectory("Screenshots".into()));
        assert_eq!(app.config.save_directory, None, "not applied");
        let note = directory_note(&app.config, &shown(&app).directory);
        assert!(
            matches!(&note, Note::Problem(problem) if problem.contains("is not a full path")),
            "{note:?}"
        );

        send(&mut app, Message::SaveDirectory("~/Screenshots".into()));
        let written = app.config.save_directory.clone().unwrap();
        assert_eq!(written.as_path(), Path::new("~/Screenshots"));
        assert_eq!(on_disk(&app).save_directory, Some(written.clone()));
        if let Some(resolved) = written.resolve() {
            assert_eq!(
                directory_note(&app.config, &shown(&app).directory),
                Note::Hint(format!("Saves go to {}", resolved.display()))
            );
        }

        send(&mut app, Message::SaveDirectory(" ".into()));
        assert_eq!(app.config.save_directory, None);
        assert_eq!(on_disk(&app).save_directory, None);

        send(&mut app, Message::SaveDirectory("~/Elsewhere".into()));
        send(&mut app, Message::DefaultSaveDirectory);
        assert_eq!(app.config.save_directory, None);
        assert_eq!(shown(&app).directory.text, "");
    }

    #[test]
    fn a_change_made_during_a_save_is_saved_after_it() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);

        let first = app.update(AppMessage::Settings(Message::AfterCapture(
            AfterCapture::Copy,
        )));
        let second = app.update(AppMessage::Settings(Message::AfterCapture(
            AfterCapture::SaveAndCopy,
        )));
        assert!(
            iced_runtime::task::into_stream(second).is_none(),
            "waits for the running save"
        );

        finish(&mut app, first);
        assert_eq!(on_disk(&app).after_capture, AfterCapture::SaveAndCopy);
        assert!(app.settings.syncing.is_none());
    }

    #[test]
    fn a_failing_save_is_reported_once_and_the_change_still_applies() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);
        // Broken by hand, before the watcher looked.
        fs::write(&file(&app).path, "after_capture = [\n").unwrap();

        send(&mut app, Message::AfterCapture(AfterCapture::Copy));
        send(&mut app, Message::AfterCapture(AfterCapture::SaveAndCopy));

        assert_eq!(app.config.after_capture, AfterCapture::SaveAndCopy);
        assert_eq!(alerts(&app), 1);
        let notice = app.alert.notices().next().unwrap();
        assert_eq!(notice.title, "Could not save the settings");
        assert!(app.settings.problem.is_some(), "shown in the window");
        assert_eq!(
            fs::read_to_string(&file(&app).path).unwrap(),
            "after_capture = [\n",
            "left for the user to fix"
        );

        // Fixed by hand: the changes are saved on top of the fix.
        edit(&mut app, "file_name = \"Fixed {date}\"\n");
        assert_eq!(on_disk(&app).file_name.as_str(), "Fixed {date}");
        assert_eq!(on_disk(&app).after_capture, AfterCapture::SaveAndCopy);
        assert_eq!(on_disk(&app), app.config);
        assert_eq!(alerts(&app), 1);
    }

    #[test]
    fn a_change_keeps_a_hand_edit_the_watcher_has_not_seen() {
        let (mut app, fake, _temp) = app_with_file();
        send(&mut app, Message::Open);
        fs::write(
            &file(&app).path,
            "after_capture = \"copy\"\n[hotkeys]\ndisplay = \"Super+Shift+F1\"\n",
        )
        .unwrap();

        send(&mut app, Message::FileName("Mine {date}".into()));

        let saved = on_disk(&app);
        assert_eq!(saved.file_name.as_str(), "Mine {date}");
        assert_eq!(saved.after_capture, AfterCapture::Copy);
        assert_eq!(
            saved.hotkeys.get(CaptureMode::Display),
            hotkey("Super+Shift+F1")
        );
        assert_eq!(app.config, saved);
        assert_eq!(shown(&app).file_name.text, "Mine {date}");
        assert_eq!(
            fake.registered_hotkeys(),
            hotkeys::bindings(&app.config.hotkeys)
        );
        assert!(fake.press_hotkey(hotkey("Super+Shift+F1")));
        assert!(!changed(file(&app)), "the hand edit was read");
    }

    #[test]
    fn a_hand_edit_and_a_change_that_overlap_end_up_both_in_memory_and_on_disk() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);

        // Edited by hand while a save runs.
        let save = app.update(AppMessage::Settings(Message::AfterCapture(
            AfterCapture::Copy,
        )));
        fs::write(&file(&app).path, "file_name = \"Edited {date}\"\n").unwrap();
        let reload = app.update(AppMessage::Settings(Message::FileChanged));
        assert!(
            iced_runtime::task::into_stream(reload).is_none(),
            "waits for the save"
        );
        finish(&mut app, save);
        assert_eq!(app.config.after_capture, AfterCapture::Copy);
        assert_eq!(app.config.file_name.as_str(), "Edited {date}");
        assert_eq!(on_disk(&app), app.config);

        // Changed while the hand edit loads.
        fs::write(
            &file(&app).path,
            "after_capture = \"save_and_copy\"\nfile_name = \"Again {date}\"\n",
        )
        .unwrap();
        let reload = app.update(AppMessage::Settings(Message::FileChanged));
        let save = app.update(AppMessage::Settings(Message::FileName(
            "Mine {time}".into(),
        )));
        assert!(
            iced_runtime::task::into_stream(save).is_none(),
            "waits for the load"
        );
        finish(&mut app, reload);
        assert_eq!(app.config.after_capture, AfterCapture::SaveAndCopy);
        assert_eq!(app.config.file_name.as_str(), "Mine {time}");
        assert_eq!(on_disk(&app), app.config);
        assert!(app.settings.syncing.is_none());
        assert!(!changed(file(&app)));
    }

    #[test]
    fn a_hand_edit_shows_in_the_open_window() {
        let (mut app, _fake, _temp) = app_with_file();
        send(&mut app, Message::Open);
        send(&mut app, Message::FileName("Half {typed".into()));

        edit(&mut app, "file_name = \"Edited {date}\"\n");

        assert_eq!(app.config.file_name.as_str(), "Edited {date}");
        assert_eq!(shown(&app).file_name.text, "Edited {date}");
        assert_eq!(shown(&app).file_name.error, None);
    }

    #[test]
    fn a_hand_edit_is_applied_and_changed_hotkeys_are_re_registered() {
        let (mut app, fake, _temp) = app_with_file();

        edit(
            &mut app,
            "after_capture = \"copy\"\n[hotkeys]\ndisplay = \"Super+Shift+F1\"\n",
        );

        assert_eq!(app.config.after_capture, AfterCapture::Copy);
        assert_eq!(
            app.config.hotkeys.get(CaptureMode::Display),
            hotkey("Super+Shift+F1")
        );
        assert_eq!(
            fake.registered_hotkeys(),
            hotkeys::bindings(&app.config.hotkeys)
        );
        assert!(fake.press_hotkey(hotkey("Super+Shift+F1")));
        assert!(!fake.press_hotkey(hotkey("Ctrl+Alt+Shift+3")));
        assert_eq!(alerts(&app), 0);
        assert!(!changed(file(&app)), "loading is not a change");
    }

    #[test]
    fn an_invalid_hand_edit_keeps_the_settings_and_is_reported_once() {
        let (mut app, fake, _temp) = app_with_file();
        edit(&mut app, "after_capture = \"copy\"\n");
        let applied = app.config.clone();
        let registered = fake.registered_hotkeys();

        edit(&mut app, "after_capture = \"sometimes\"\n");
        assert_eq!(app.config, applied);
        assert_eq!(fake.registered_hotkeys(), registered);
        assert_eq!(alerts(&app), 1);
        let notice = app.alert.notices().next().unwrap().clone();
        assert_eq!(notice.title, "Keeping the current settings");
        assert!(notice.body.contains("after_capture"), "{}", notice.body);

        // Saved again with the same problem: not reported again.
        edit(
            &mut app,
            "after_capture = \"sometimes\"\n# still thinking\n",
        );
        assert_eq!(alerts(&app), 1);

        // Fixed, then broken again: a new report.
        edit(&mut app, "after_capture = \"save_and_copy\"\n");
        assert_eq!(app.config.after_capture, AfterCapture::SaveAndCopy);
        edit(&mut app, "after_capture = \"sometimes\"\n");
        assert_eq!(alerts(&app), 2);
        assert_eq!(app.config.after_capture, AfterCapture::SaveAndCopy);
    }

    #[test]
    fn a_hand_edit_to_a_hotkey_another_program_holds_is_reported() {
        let (mut app, fake, _temp) = app_with_file();
        fake.reserve_hotkey(hotkey("Super+F5"));

        edit(&mut app, "[hotkeys]\nwindow = \"Super+F5\"\n");

        assert_eq!(
            app.config.hotkeys.get(CaptureMode::Window),
            hotkey("Super+F5")
        );
        assert_eq!(alerts(&app), 1);
        let active: Vec<CaptureMode> = fake
            .registered_hotkeys()
            .iter()
            .map(|binding| binding.mode)
            .collect();
        assert_eq!(active, [CaptureMode::Display, CaptureMode::Rectangle]);
    }

    #[test]
    fn the_watcher_notices_a_hand_edit_on_its_own_thread() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.toml");
        let (mut app, _fake) = App::for_test();
        app.settings.file = Some(SettingsFile::with_interval(
            path.clone(),
            Duration::from_millis(5),
        ));
        let mut recipes = into_recipes(subscription(&app));
        assert_eq!(recipes.len(), 1);
        let mut changes = recipes
            .pop()
            .unwrap()
            .stream(futures::stream::empty().boxed());

        fs::write(&path, "launch_at_login = true\n").unwrap();
        let (sender, received) = std_mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(block_on(changes.next()));
        });
        let message = received
            .recv_timeout(Duration::from_secs(10))
            .expect("the watcher reports the change");
        assert!(
            matches!(message, Some(AppMessage::Settings(Message::FileChanged))),
            "{message:?}"
        );
    }
}

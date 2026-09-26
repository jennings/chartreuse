//! The settings: keeping [`App::config`] in step with the settings file, and
//! the settings window (`WindowKind::Settings`). Owned by track 3A.
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

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use chartreuse_config::Settings;
use chartreuse_core::{Error, Result};
use iced::futures::channel::mpsc;
use iced::widget::space;
use iced::{window, Element, Subscription, Task};
use parking_lot::Mutex;

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::hotkeys;

/// How often the settings file is checked for hand edits.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// This feature's part of the app state ([`App::settings`]).
#[derive(Debug, Default)]
pub struct State {
    /// The settings file; `None` if the platform has no place for it, and
    /// then nothing is watched.
    file: Option<SettingsFile>,
    /// The last problem reported with the settings file, so that it is
    /// reported once.
    problem: Option<String>,
}

/// This feature's messages ([`AppMessage::Settings`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// The settings file changed on disk, other than by the app itself.
    FileChanged,
    /// The settings file was loaded again after it changed.
    Reloaded(Result<Settings>),
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

    /// Whether the file changed since the app last read or wrote it, or since
    /// this last returned `true`. Blocking.
    fn changed(&self) -> bool {
        let mut known = self.known.lock();
        let now = Stamp::of(&self.path);
        if *known == now {
            false
        } else {
            *known = now;
            true
        }
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
            while !changes.is_closed() {
                thread::sleep(file.interval);
                // A full channel already holds a change for the app to load.
                if file.changed()
                    && changes
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
        Message::FileChanged => {
            let Some(file) = &app.settings.file else {
                return Task::none();
            };
            tracing::info!(path = %file.path.display(), "the settings file changed; loading it");
            let path = Arc::clone(&file.path);
            Task::perform(async move { chartreuse_config::load(&path) }, |loaded| {
                AppMessage::Settings(Message::Reloaded(loaded))
            })
        }
        Message::Reloaded(Ok(settings)) => reloaded(app, settings),
        Message::Reloaded(Err(error)) => report_once(app, "Keeping the current settings", &error),
    }
}

pub fn subscription(app: &App) -> Subscription<AppMessage> {
    match &app.settings.file {
        Some(file) => Subscription::run_with(file.clone(), watch)
            .map(|()| AppMessage::Settings(Message::FileChanged)),
        None => Subscription::none(),
    }
}

pub fn view(_app: &App, _window: window::Id) -> Element<'_, AppMessage> {
    space().into()
}

pub fn window_closed(_app: &mut App, _window: window::Id) -> Task<AppMessage> {
    Task::none()
}

/// Applies settings loaded from the file after it changed.
fn reloaded(app: &mut App, settings: Settings) -> Task<AppMessage> {
    app.settings.problem = None;
    if settings == app.config {
        return Task::none();
    }
    tracing::info!("applying the changed settings file");
    let hotkeys_changed = settings.hotkeys != app.config.hotkeys;
    app.config = settings;
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc as std_mpsc;

    use chartreuse_config::AfterCapture;
    use chartreuse_core::capture::CaptureMode;
    use chartreuse_core::hotkey::Hotkey;
    use chartreuse_platform::fake::Fake;
    use futures::executor::block_on;
    use futures::StreamExt;
    use iced::advanced::subscription::into_recipes;
    use tempfile::TempDir;

    use super::*;
    use crate::windows::WindowKind;

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
        assert!(file.changed(), "a hand edit is a change");
        let _ = app.settle(AppMessage::Settings(Message::FileChanged));
    }

    fn alerts(app: &App) -> usize {
        app.windows.of_kind(WindowKind::Alert).count()
    }

    fn hotkey(text: &str) -> Hotkey {
        text.parse().unwrap()
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
        assert!(!file(&app).changed(), "loading is not a change");
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

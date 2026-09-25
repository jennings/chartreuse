//! Global hotkeys: registers the hotkey set through the platform
//! [`Hotkeys`](chartreuse_platform::Hotkeys) trait and turns presses into
//! [`Message::Pressed`]. Owned by track 1B; settings (3A) re-register through
//! [`reregister`].
//!
//! # Startup
//!
//! `boot` schedules [`Message::Register`] (Carbon needs the running event loop),
//! which registers [`default_bindings`], one hotkey per capture mode:
//!
//! | Hotkey             | Mode              |
//! |--------------------|-------------------|
//! | Ctrl+Alt+Shift+3   | Capture display   |
//! | Ctrl+Alt+Shift+4   | Capture rectangle |
//! | Ctrl+Alt+Shift+5   | Capture window    |
//!
//! They echo the system screenshot shortcuts (Shift+Command+3/4/5) without
//! clashing with them, since macOS keeps those for itself. Settings (3A) will make
//! them configurable. Pressing one starts that capture
//! (`capture::Message::Start`).
//!
//! # Failures
//!
//! Bindings that cannot be registered (typically because another app already uses
//! the combination) are reported in one alert listing every failing combination;
//! the other bindings stay active. If hotkeys are unavailable altogether, that is
//! reported instead.

use chartreuse_core::capture::CaptureMode;
use chartreuse_core::hotkey::{Hotkey, Key, Modifiers};
use chartreuse_core::Error;
use chartreuse_platform::{HotkeyBinding, HotkeyEvent, HotkeyRegistration};
use iced::{Subscription, Task};

use crate::alert::{self, Notice};
use crate::app::{App, Message as AppMessage};
use crate::{capture, events};

/// This feature's part of the app state ([`App::hotkeys`]).
#[derive(Debug, Default)]
pub struct State {
    /// The active set; its receiver drives [`subscription`].
    registration: Option<HotkeyRegistration>,
}

/// This feature's messages ([`AppMessage::Hotkeys`]).
#[derive(Debug, Clone)]
pub enum Message {
    /// Register the startup set ([`default_bindings`]). Sent by `boot`.
    Register,
    /// A registered hotkey was pressed.
    Pressed(HotkeyEvent),
}

/// The startup hotkey set: Ctrl+Alt+Shift+3, 4, and 5 capture a display, a
/// rectangle, and a window.
#[must_use]
pub fn default_bindings() -> Vec<HotkeyBinding> {
    let hotkey = |key| Hotkey::new(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT, key);
    vec![
        HotkeyBinding {
            mode: CaptureMode::Display,
            hotkey: hotkey(Key::Digit3),
        },
        HotkeyBinding {
            mode: CaptureMode::Rectangle,
            hotkey: hotkey(Key::Digit4),
        },
        HotkeyBinding {
            mode: CaptureMode::Window,
            hotkey: hotkey(Key::Digit5),
        },
    ]
}

/// Replaces the registered hotkeys with `bindings` and returns the task to
/// return from `update` (an alert if some or all of them failed).
///
/// The previous set is unregistered first, so a combination that stays in the
/// set is free to register again. Presses of the new set arrive as
/// [`Message::Pressed`]. Main thread only: call it from `update`.
pub fn reregister(app: &mut App, bindings: Vec<HotkeyBinding>) -> Task<AppMessage> {
    app.hotkeys.registration = None;
    match app.platform.hotkeys.register(&bindings) {
        Ok(registration) => {
            tracing::info!(
                registered = bindings.len() - registration.failures.len(),
                failed = registration.failures.len(),
                "hotkeys registered"
            );
            let notice = failure_notice(&registration.failures);
            app.hotkeys.registration = Some(registration);
            notice.map_or_else(Task::none, |notice| alert::report_error(app, notice))
        }
        Err(error) => alert::report_error(app, Notice::from_error("Hotkeys unavailable", &error)),
    }
}

/// The alert for bindings that failed to register: one line per combination, or
/// `None` if all of them registered.
fn failure_notice(failures: &[(HotkeyBinding, Error)]) -> Option<Notice> {
    let title = match failures.len() {
        0 => return None,
        1 => "Hotkey unavailable",
        _ => "Hotkeys unavailable",
    };
    let lines: Vec<String> = failures
        .iter()
        .map(|(binding, error)| {
            let reason = match error {
                Error::HotkeyUnavailable { reason, .. } => reason.clone(),
                other => other.to_string(),
            };
            format!("{} ({}): {reason}.", binding.hotkey, binding.mode)
        })
        .collect();
    Some(Notice::new(title, lines.join("\n")))
}

pub fn boot(_app: &mut App) -> Task<AppMessage> {
    Task::done(AppMessage::Hotkeys(Message::Register))
}

pub fn update(app: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Register => reregister(app, default_bindings()),
        Message::Pressed(event) => {
            tracing::info!(hotkey = %event.hotkey, "hotkey pressed: {}", event.mode);
            Task::done(AppMessage::Capture(capture::Message::Start(event.mode)))
        }
    }
}

pub fn subscription(app: &App) -> Subscription<AppMessage> {
    match &app.hotkeys.registration {
        Some(registration) => events::subscription(&registration.events)
            .map(|event| AppMessage::Hotkeys(Message::Pressed(event))),
        None => Subscription::none(),
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_platform::Hotkeys;
    use futures::executor::block_on;
    use futures::StreamExt;
    use iced::advanced::subscription::into_recipes;

    use super::*;
    use crate::windows::WindowKind;

    fn alerts(app: &App) -> usize {
        app.windows.of_kind(WindowKind::Alert).count()
    }

    fn binding(mode: CaptureMode, key: Key) -> HotkeyBinding {
        HotkeyBinding {
            mode,
            hotkey: Hotkey::new(Modifiers::SUPER | Modifiers::SHIFT, key),
        }
    }

    #[test]
    fn boot_leaves_registration_to_update() {
        let (mut app, fake) = App::for_test();
        let _ = boot(&mut app);
        assert!(fake.registered_hotkeys().is_empty());

        let _ = app.update(AppMessage::Hotkeys(Message::Register));
        assert_eq!(fake.registered_hotkeys(), default_bindings());
        assert_eq!(alerts(&app), 0);
    }

    #[test]
    fn the_defaults_bind_every_mode_to_its_own_hotkey() {
        let bindings = default_bindings();
        let modes: Vec<CaptureMode> = bindings.iter().map(|binding| binding.mode).collect();
        assert_eq!(modes.len(), CaptureMode::ALL.len());
        for mode in CaptureMode::ALL {
            assert!(modes.contains(&mode), "{mode} has no hotkey");
        }
        for (index, binding) in bindings.iter().enumerate() {
            assert!(
                bindings[..index]
                    .iter()
                    .all(|other| other.hotkey != binding.hotkey),
                "{} is bound twice",
                binding.hotkey
            );
        }
    }

    #[test]
    fn presses_start_the_bound_capture() {
        let (mut app, fake) = App::for_test();
        let _ = update(&mut app, Message::Register);
        let mut recipes = into_recipes(subscription(&app));
        assert_eq!(recipes.len(), 1);
        let mut presses = recipes
            .pop()
            .unwrap()
            .stream(futures::stream::empty().boxed());

        for binding in default_bindings() {
            assert!(fake.press_hotkey(binding.hotkey));
            let message = block_on(presses.next());
            let Some(AppMessage::Hotkeys(Message::Pressed(event))) = message else {
                panic!("expected a press, got {message:?}");
            };
            assert_eq!(
                event,
                HotkeyEvent {
                    mode: binding.mode,
                    hotkey: binding.hotkey
                }
            );

            let started: Vec<_> =
                iced_runtime::task::into_stream(update(&mut app, Message::Pressed(event)))
                    .map(|stream| block_on(stream.collect()))
                    .unwrap_or_default();
            assert!(
                matches!(
                    started.as_slice(),
                    [iced_runtime::Action::Output(AppMessage::Capture(
                        capture::Message::Start(mode)
                    ))] if *mode == binding.mode
                ),
                "{}: {started:?}",
                binding.mode
            );
        }
    }

    #[test]
    fn taken_hotkeys_are_reported_in_one_alert() {
        let (mut app, fake) = App::for_test();
        let display = binding(CaptureMode::Display, Key::Digit1);
        let window = binding(CaptureMode::Window, Key::Digit2);
        let rectangle = binding(CaptureMode::Rectangle, Key::Digit3);
        fake.reserve_hotkey(display.hotkey);
        fake.reserve_hotkey(window.hotkey);

        let _ = reregister(&mut app, vec![display, window, rectangle]);
        assert_eq!(alerts(&app), 1);
        assert_eq!(fake.registered_hotkeys(), [rectangle]);
        assert!(fake.press_hotkey(rectangle.hotkey));
    }

    #[test]
    fn the_failure_notice_lists_every_failing_combination() {
        let fake = chartreuse_platform::fake::Fake::new();
        let display = binding(CaptureMode::Display, Key::Digit1);
        let window = binding(CaptureMode::Window, Key::Digit2);
        fake.reserve_hotkey(display.hotkey);
        fake.reserve_hotkey(window.hotkey);
        let registration = fake.register(&[display, window]).unwrap();

        let notice = failure_notice(&registration.failures).unwrap();
        let lines: Vec<&str> = notice.body.lines().collect();
        assert_eq!(lines.len(), 2, "{}", notice.body);
        for (line, failed) in lines.iter().zip([display, window]) {
            assert!(line.contains(&failed.hotkey.to_string()), "{line}");
            assert!(line.contains(&failed.mode.to_string()), "{line}");
            assert!(line.contains("another program has registered it"), "{line}");
        }
        assert_eq!(failure_notice(&[]), None);
    }

    #[test]
    fn reregistering_replaces_the_set() {
        let (mut app, fake) = App::for_test();
        let _ = update(&mut app, Message::Register);
        let old_events = app.hotkeys.registration.as_ref().unwrap().events.clone();

        // The same set again: the old registration is gone first, so no clash.
        let _ = reregister(&mut app, default_bindings());
        assert_eq!(fake.registered_hotkeys(), default_bindings());
        assert_eq!(alerts(&app), 0);

        let window = binding(CaptureMode::Window, Key::W);
        let _ = reregister(&mut app, vec![window]);
        assert_eq!(fake.registered_hotkeys(), [window]);
        assert!(!fake.press_hotkey(default_bindings()[0].hotkey));
        assert!(fake.press_hotkey(window.hotkey));
        let registration = app.hotkeys.registration.as_ref().unwrap();
        assert_ne!(registration.events, old_events, "a new subscription");
        assert_eq!(
            registration.events.try_recv().map(|event| event.mode),
            Some(CaptureMode::Window)
        );
    }

    #[derive(Debug)]
    struct Unavailable;

    impl Hotkeys for Unavailable {
        fn register(
            &self,
            _bindings: &[HotkeyBinding],
        ) -> chartreuse_core::Result<HotkeyRegistration> {
            Err(Error::Unsupported("global hotkey registration"))
        }
    }

    #[test]
    fn unavailable_hotkeys_are_reported_and_the_old_set_is_released() {
        let (mut app, fake) = App::for_test();
        let _ = update(&mut app, Message::Register);
        app.platform.hotkeys = Box::new(Unavailable);

        let _ = reregister(&mut app, default_bindings());
        assert_eq!(alerts(&app), 1);
        assert!(app.hotkeys.registration.is_none());
        assert!(fake.registered_hotkeys().is_empty());
    }
}

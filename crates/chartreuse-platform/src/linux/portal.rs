//! What every XDG desktop portal call of the Linux backends shares.
//!
//! The portals run in `xdg-desktop-portal` and the desktop's backend for it
//! (GNOME, KDE, wlroots, …), reached over the session bus with `ashpd`, whose
//! requests are plain futures: zbus drives the connection on its own thread,
//! so they run on iced's executor without another async runtime.

use chartreuse_core::flavor;
use chartreuse_core::Error;
use futures::lock::Mutex;

/// Registers Chartreuse's app id with the portal once per process, before its
/// first request.
///
/// Unsandboxed apps have no app id the portal can see, and some portals
/// (GlobalShortcuts, and the Screenshot permission) key what the user allowed
/// on it. Registering is best-effort: portals before version 1.19 lack the
/// registry, and then the portal falls back to guessing the id.
pub async fn register_app() {
    static REGISTERED: Mutex<bool> = Mutex::new(false);
    let mut registered = REGISTERED.lock().await;
    if *registered {
        return;
    }
    *registered = true;
    let registration = match ashpd::AppID::try_from(flavor::BUNDLE_ID) {
        Ok(app_id) => ashpd::register_host_app(app_id).await,
        Err(error) => Err(error),
    };
    if let Err(error) = registration {
        tracing::debug!("could not register the app id with the portal: {error}");
    }
}

/// A failed portal request as an [`Error`], naming what was asked (`"the file
/// dialog"`).
pub fn error(what: &str, error: &ashpd::Error) -> Error {
    match error {
        ashpd::Error::PortalNotFound(_) => Error::Platform(format!(
            "{what} needs the XDG desktop portal (xdg-desktop-portal and a backend for \
             this desktop), which is not running: {error}"
        )),
        _ => Error::Platform(format!("{what} failed: {error}")),
    }
}

/// True if the user dismissed the portal's dialog.
pub fn cancelled(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)
    )
}

//! macOS: privacy permissions.
//!
//! Screen Recording goes through `CGPreflightScreenCaptureAccess` (status, never
//! prompts) and `CGRequestScreenCaptureAccess` (prompts once per app; later calls
//! only report). macOS may keep reporting the old status to a running process
//! after the user changes it in System Settings, so the app tells the user that a
//! relaunch may be needed.

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::{Error, Result};
use objc2_app_kit::NSWorkspace;
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use objc2_foundation::{NSString, NSURL};

use crate::permissions::Permissions;

/// The macOS [`Permissions`] backend.
#[derive(Debug, Default)]
pub struct MacosPermissions;

impl MacosPermissions {
    pub fn new() -> Self {
        Self
    }
}

impl Permissions for MacosPermissions {
    fn status(&self, permission: Permission) -> Result<PermissionStatus> {
        match permission {
            Permission::ScreenRecording => Ok(status_from(CGPreflightScreenCaptureAccess())),
        }
    }

    fn request(&self, permission: Permission) -> Result<PermissionStatus> {
        match permission {
            Permission::ScreenRecording => Ok(status_from(CGRequestScreenCaptureAccess())),
        }
    }

    fn open_settings(&self, permission: Permission) -> Result<()> {
        let url = settings_url(permission);
        let ns_url = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| Error::Platform(format!("invalid System Settings URL {url}")))?;
        if NSWorkspace::sharedWorkspace().openURL(&ns_url) {
            Ok(())
        } else {
            Err(Error::Platform(format!("could not open {url}")))
        }
    }
}

fn status_from(granted: bool) -> PermissionStatus {
    if granted {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

/// The System Settings pane where the user grants `permission` (Privacy &
/// Security › Screen & System Audio Recording for Screen Recording).
fn settings_url(permission: Permission) -> &'static str {
    match permission {
        Permission::ScreenRecording => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_url_is_a_valid_system_settings_url() {
        let url = NSURL::URLWithString(&NSString::from_str(settings_url(
            Permission::ScreenRecording,
        )))
        .expect("the settings URL parses");
        assert_eq!(
            url.scheme().map(|scheme| scheme.to_string()).as_deref(),
            Some("x-apple.systempreferences")
        );
    }
}

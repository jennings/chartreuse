//! macOS: privacy permissions.
//!
//! Screen Recording goes through `CGPreflightScreenCaptureAccess` (status, never
//! prompts) and `CGRequestScreenCaptureAccess` (may prompt). macOS may keep
//! reporting the old status to a running process after the user changes it in
//! System Settings, so the app tells the user that a relaunch may be needed.
//!
//! # When macOS prompts
//!
//! TCC keeps one Screen Recording record per bundle identifier. The record holds
//! the designated requirement of the build that first asked. A build that
//! satisfies it gets the recorded answer and is never prompted again. A build that
//! does not is treated as never asked: each request prompts again, and the record,
//! with any grant in it, keeps pointing at the old build.
//!
//! An ad-hoc signature's designated requirement is the build's `cdhash`. Every
//! rebuild of an ad-hoc signed build therefore fails the recorded requirement, so
//! every launch would prompt, and a grant would only ever help the one build that
//! asked first. [`MacosPermissions::request`] asks macOS to prompt only when this
//! copy's designated requirement survives a rebuild (it names a certificate, as
//! with `cargo xtask dev-cert` or a real identity). Other builds only report the
//! status, and the app's guidance window takes over.
//!
//! ScreenCaptureKit also prompts when called without the permission, so capture
//! code checks [`Permissions::status`] first. Only this module asks for a prompt.

use std::ptr;

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::{Error, Result};
use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{CFRetained, CFString, CFType, Type};
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
            Permission::ScreenRecording => match own_designated_requirement() {
                Ok(requirement) if !requirement_pins_build(&requirement) => {
                    Ok(status_from(CGRequestScreenCaptureAccess()))
                }
                signature => {
                    tracing::warn!(
                        ?signature,
                        "not asking macOS for Screen Recording: this build is not signed with \
                         a certificate, so macOS would prompt on every launch and forget the \
                         grant on the next rebuild (see `cargo xtask dev-cert`)"
                    );
                    self.status(permission)
                }
            },
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

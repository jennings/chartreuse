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
//! satisfies it gets the recorded answer and is not prompted again. A build that
//! does not is treated as never asked: each request prompts again, while the
//! record, and any grant in it, keeps pointing at the old build.
//!
//! An ad-hoc signature's designated requirement is the build's `cdhash`, so no
//! rebuild satisfies a record made by an earlier ad-hoc build: each launch would
//! prompt, and a grant would only ever help the one build that asked first.
//! [`MacosPermissions::request`] therefore asks macOS to prompt only when this
//! copy's designated requirement names a certificate (`cargo xtask dev-cert` or a
//! real identity), and otherwise only reports the status, leaving the user to
//! the app's guidance window.
//!
//! ScreenCaptureKit prompts too when called without the permission, so capture
//! code checks [`Permissions::status`] first. Only this module asks for a prompt.

use std::ptr::{self, NonNull};

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
                requirement => {
                    tracing::warn!(
                        designated_requirement = ?requirement,
                        "not asking macOS for Screen Recording: this build is not signed with \
                         a certificate, so macOS would ask again on every launch and forget \
                         the answer on the next rebuild (see `cargo xtask dev-cert`)"
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

/// True if a designated requirement pins one exact build, as ad-hoc signatures'
/// `cdhash H"…"` requirements do, so that no rebuild satisfies it.
fn requirement_pins_build(requirement: &str) -> bool {
    requirement.contains("cdhash")
}

/// Security.framework's result code; 0 (`errSecSuccess`) is success.
type OsStatus = i32;

/// `kSecCSDefaultFlags`.
const SEC_CS_DEFAULT_FLAGS: u32 = 0;

// Security.framework has no binding among the workspace's objc2 crates. Code and
// requirement objects are CoreFoundation objects, so `CFType` stands in for them.
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    /// Returns (+1) the `SecCodeRef` of the calling process.
    fn SecCodeCopySelf(flags: u32, code: *mut *mut CFType) -> OsStatus;
    /// Returns (+1) the designated requirement of a `SecCodeRef` or
    /// `SecStaticCodeRef`; fails with `errSecCSUnsigned` for unsigned code.
    fn SecCodeCopyDesignatedRequirement(
        code: &CFType,
        flags: u32,
        requirement: *mut *mut CFType,
    ) -> OsStatus;
    /// Returns (+1) the text of a `SecRequirementRef` in the requirement
    /// language, as `codesign -d -r-` prints it.
    fn SecRequirementCopyString(
        requirement: &CFType,
        flags: u32,
        text: *mut *const CFString,
    ) -> OsStatus;
}

/// This process's designated requirement, e.g. `identifier "io.jennings.chartreuse.dev"
/// and certificate leaf = H"…"` or, for ad-hoc code, `cdhash H"…"`. Fails with the
/// Security.framework status (`errSecCSUnsigned` for unsigned code).
fn own_designated_requirement() -> Result<String, OsStatus> {
    let mut code = ptr::null_mut();
    // SAFETY: `code` is a valid out-pointer for the returned SecCodeRef.
    let status = unsafe { SecCodeCopySelf(SEC_CS_DEFAULT_FLAGS, &mut code) };
    // SAFETY: on success SecCodeCopySelf returns a +1 object.
    let code = unsafe { owned(status, code) }?;

    let mut requirement = ptr::null_mut();
    // SAFETY: `code` is a live SecCodeRef, which Security.framework accepts
    // wherever it takes a SecStaticCodeRef; `requirement` is a valid out-pointer.
    let status =
        unsafe { SecCodeCopyDesignatedRequirement(&code, SEC_CS_DEFAULT_FLAGS, &mut requirement) };
    // SAFETY: on success SecCodeCopyDesignatedRequirement returns a +1 object.
    let requirement = unsafe { owned(status, requirement) }?;

    let mut text = ptr::null();
    // SAFETY: `requirement` is a live SecRequirementRef; `text` is a valid
    // out-pointer for the returned CFString.
    let status = unsafe { SecRequirementCopyString(&requirement, SEC_CS_DEFAULT_FLAGS, &mut text) };
    // SAFETY: on success SecRequirementCopyString returns a +1 CFString.
    let text = unsafe { owned(status, text.cast_mut()) }?;
    Ok(text.to_string())
}

/// Takes ownership of an object that a Security.framework copy function returned
/// through an out-pointer.
///
/// # Safety
///
/// If `status` is 0, `object` must be null or a +1 reference to a live `T`.
unsafe fn owned<T: Type>(status: OsStatus, object: *mut T) -> Result<CFRetained<T>, OsStatus> {
    match NonNull::new(object) {
        // SAFETY: the caller guarantees a +1 reference on success.
        Some(object) if status == 0 => Ok(unsafe { CFRetained::from_raw(object) }),
        _ => Err(status),
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

    #[test]
    fn only_cdhash_requirements_pin_a_build() {
        assert!(requirement_pins_build(
            r#"cdhash H"16c1b61cb5b3bece6427e1142e9b0c92b79c2aa9""#
        ));
        assert!(!requirement_pins_build(
            r#"identifier "io.jennings.chartreuse.dev" and certificate leaf = H"0fce8b6e9b27c1d054a35307c43ce095e24ed2cb""#
        ));
        assert!(!requirement_pins_build(
            r#"identifier "io.jennings.chartreuse" and anchor apple generic and certificate leaf[subject.OU] = "ABC123""#
        ));
    }

    // The linker ad-hoc signs every binary on Apple silicon, test binaries
    // included (Intel binaries are unsigned unless codesign runs).
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn a_linker_signed_build_reads_as_pinned() {
        let requirement = own_designated_requirement().expect("test binaries are signed");
        assert!(requirement.starts_with("cdhash H\""), "{requirement}");
        assert!(requirement_pins_build(&requirement));
    }
}

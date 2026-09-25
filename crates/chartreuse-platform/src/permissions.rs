//! Operating-system privacy permissions.

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::Result;

/// Checks and requests privacy permissions.
///
/// Call from the main thread (iced `boot`/`update`). Platforms without the
/// permission report it as [`PermissionStatus::Granted`].
pub trait Permissions {
    /// The current status, without prompting.
    fn status(&self, permission: Permission) -> Result<PermissionStatus>;

    /// Asks the OS to prompt the user, if it still will (macOS prompts for Screen
    /// Recording only once per app), and returns the status without waiting for the
    /// user's answer.
    fn request(&self, permission: Permission) -> Result<PermissionStatus>;

    /// Opens the system settings page where the user grants `permission`.
    fn open_settings(&self, permission: Permission) -> Result<()>;
}

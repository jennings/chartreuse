//! Fake [`Permissions`]: a settable Screen Recording status.

use chartreuse_core::permission::{Permission, PermissionStatus};
use chartreuse_core::Result;

use super::Fake;
use crate::permissions::Permissions;

impl Permissions for Fake {
    fn status(&self, permission: Permission) -> Result<PermissionStatus> {
        match permission {
            Permission::ScreenRecording => Ok(self.state.lock().screen_recording),
        }
    }

    fn request(&self, permission: Permission) -> Result<PermissionStatus> {
        let mut state = self.state.lock();
        match permission {
            Permission::ScreenRecording => {
                if state.grant_on_request {
                    state.screen_recording = PermissionStatus::Granted;
                }
                Ok(state.screen_recording)
            }
        }
    }

    fn open_settings(&self, permission: Permission) -> Result<()> {
        self.state.lock().opened_settings.push(permission);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_grants_only_when_the_user_would_accept() {
        let fake = Fake::new();
        fake.set_screen_recording(PermissionStatus::Denied);
        fake.set_grant_on_request(false);
        assert_eq!(
            fake.request(Permission::ScreenRecording).unwrap(),
            PermissionStatus::Denied
        );
        fake.set_grant_on_request(true);
        assert_eq!(
            fake.request(Permission::ScreenRecording).unwrap(),
            PermissionStatus::Granted
        );
        assert_eq!(
            fake.status(Permission::ScreenRecording).unwrap(),
            PermissionStatus::Granted
        );
    }
}

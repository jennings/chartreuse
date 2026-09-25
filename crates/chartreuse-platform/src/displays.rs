//! Display enumeration.

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::Result;

/// Enumerates connected displays.
pub trait Displays {
    /// Every connected display, in the global logical desktop space (see
    /// [`chartreuse_core::geometry`]). Exactly one display has `is_primary` set.
    ///
    /// **Main thread only** (`NSScreen` is main-thread-only on macOS): call it from
    /// iced `boot`/`update`.
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
}

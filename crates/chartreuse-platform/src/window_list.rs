//! On-screen window enumeration, for window-selection mode.

use chartreuse_core::window::WindowInfo;
use chartreuse_core::Result;
use futures::future::BoxFuture;

/// Lists on-screen windows.
pub trait WindowList {
    /// The visible, normal-level windows sorted front to back (ascending
    /// `z_order`), excluding Chartreuse's own windows. Bounds are in the global
    /// logical desktop space.
    ///
    /// Call on the main thread; the future is `Send + 'static` for `Task::perform`.
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>>;
}

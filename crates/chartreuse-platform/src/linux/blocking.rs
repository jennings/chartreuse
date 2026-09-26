//! Running blocking system calls off iced's executor.

use std::thread;

use chartreuse_core::{Error, Result};
use futures::channel::oneshot;
use futures::future::{BoxFuture, FutureExt};

/// Runs `work` on a new thread and resolves with its result, so that X11
/// round trips and Wayland dispatching never stall iced's thread pool.
pub fn run<T: Send + 'static>(
    what: &'static str,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> BoxFuture<'static, Result<T>> {
    let (sender, receiver) = oneshot::channel();
    let spawned = thread::Builder::new()
        .name(format!("chartreuse {what}"))
        .spawn(move || {
            // The receiver is gone only if the app dropped the task.
            let _ = sender.send(work());
        });
    async move {
        if let Err(error) = spawned {
            return Err(Error::Platform(format!("{what}: no thread: {error}")));
        }
        receiver
            .await
            .unwrap_or_else(|_| Err(Error::Platform(format!("{what} panicked"))))
    }
    .boxed()
}

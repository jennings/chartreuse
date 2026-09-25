//! Delivering platform events (hotkey presses, menu actions) to the app.
//!
//! OS callbacks fire on the main thread, inside the run loop that iced/winit owns.
//! A backend forwards each event through an [`EventSender`]; the app turns the
//! matching [`EventReceiver`] into an iced `Subscription` (see
//! `crates/chartreuse/src/events.rs`), so every event arrives as a `Message` in
//! `update`.
//!
//! Channels are unbounded: senders never block the main thread, and the volume
//! (human key presses and clicks) is tiny.

use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::channel::mpsc;
use futures::stream::{FusedStream, Stream};
use parking_lot::Mutex;

/// Creates a connected sender/receiver pair.
#[must_use]
pub fn channel<T>() -> (EventSender<T>, EventReceiver<T>) {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let (tx, rx) = mpsc::unbounded();
    let receiver = EventReceiver {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        rx: Arc::new(Mutex::new(Some(rx))),
    };
    (EventSender { tx }, receiver)
}

/// The sending half, held by a backend. Cheap to clone; `Send`, so it can be moved
/// into OS callbacks.
#[derive(Debug)]
pub struct EventSender<T> {
    tx: mpsc::UnboundedSender<T>,
}

impl<T> Clone for EventSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T> EventSender<T> {
    /// Sends an event without blocking. Returns `false` if the receiving side (the
    /// app's subscription) is gone; backends may ignore that.
    pub fn send(&self, event: T) -> bool {
        self.tx.unbounded_send(event).is_ok()
    }
}

/// The receiving half, handed to the app.
///
/// It is a cheap, cloneable *identity* for one event source: clones compare and
/// hash equal, which is what iced's `Subscription::run_with` needs to keep one
/// subscription alive across `subscription()` calls. The events themselves can be
/// taken out exactly once, with [`stream`](Self::stream).
#[derive(Debug)]
pub struct EventReceiver<T> {
    id: u64,
    rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<T>>>>,
}

impl<T> Clone for EventReceiver<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            rx: Arc::clone(&self.rx),
        }
    }
}

impl<T> PartialEq for EventReceiver<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for EventReceiver<T> {}

impl<T> Hash for EventReceiver<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> EventReceiver<T> {
    /// Takes the event stream. The first call (on this receiver or any clone) gets
    /// every event; later calls get a stream that ends immediately.
    #[must_use]
    pub fn stream(&self) -> EventStream<T> {
        EventStream {
            rx: self.rx.lock().take(),
        }
    }

    /// Returns the next queued event without waiting, or `None` if there is none or
    /// the stream has been taken. For tests.
    #[must_use]
    pub fn try_recv(&self) -> Option<T> {
        self.rx.lock().as_mut()?.try_recv().ok()
    }
}

/// The events of one [`EventReceiver`]. Ends when every sender is dropped.
#[derive(Debug)]
pub struct EventStream<T> {
    rx: Option<mpsc::UnboundedReceiver<T>>,
}

impl<T> Stream for EventStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        match self.rx.as_mut() {
            Some(rx) => Pin::new(rx).poll_next(cx),
            None => Poll::Ready(None),
        }
    }
}

impl<T> FusedStream for EventStream<T> {
    fn is_terminated(&self) -> bool {
        self.rx.as_ref().is_none_or(FusedStream::is_terminated)
    }
}

/// Keeps an OS registration (a status item, a set of hotkeys) alive. Dropping it
/// undoes the registration.
///
/// Deliberately not `Send`: registrations are made on the main thread and must be
/// dropped there, which holds automatically when they live in the app state.
pub struct Registration {
    _resources: Box<dyn std::any::Any>,
}

impl Registration {
    /// Wraps whatever the backend needs to keep alive; its `Drop` implementation
    /// performs the unregistration.
    pub fn new(resources: impl std::any::Any) -> Self {
        Self {
            _resources: Box::new(resources),
        }
    }
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use futures::StreamExt;

    use super::*;

    #[test]
    fn events_arrive_in_order_and_the_stream_ends_with_the_senders() {
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        assert!(tx.send(1));
        assert!(tx2.send(2));
        drop((tx, tx2));
        let events: Vec<i32> = block_on(rx.stream().collect());
        assert_eq!(events, [1, 2]);
    }

    #[test]
    fn the_stream_can_be_taken_only_once_across_clones() {
        let (tx, rx) = channel();
        let clone = rx.clone();
        assert_eq!(rx, clone);
        let mut first = clone.stream();
        let second = rx.stream();
        tx.send("event");
        assert!(second.is_terminated());
        assert_eq!(block_on(first.next()), Some("event"));
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn distinct_channels_have_distinct_identities() {
        let (_, a) = channel::<()>();
        let (_, b) = channel::<()>();
        assert_ne!(a, b);
    }

    #[test]
    fn send_reports_a_dropped_receiver() {
        let (tx, rx) = channel();
        drop(rx);
        assert!(!tx.send(()));
    }

    #[test]
    fn dropping_a_registration_runs_its_cleanup() {
        struct Flag(Arc<Mutex<bool>>);
        impl Drop for Flag {
            fn drop(&mut self) {
                *self.0.lock() = true;
            }
        }
        let dropped = Arc::new(Mutex::new(false));
        let registration = Registration::new(Flag(Arc::clone(&dropped)));
        assert!(!*dropped.lock());
        drop(registration);
        assert!(*dropped.lock());
    }
}

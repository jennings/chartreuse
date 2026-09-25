//! Turning platform event sources into iced subscriptions.
//!
//! A feature that installs a platform event source (the status item, a hotkey
//! registration) keeps the returned [`EventReceiver`] in its state and, in its
//! `subscription()`, maps [`subscription`] of it into its own messages:
//!
//! ```ignore
//! pub fn subscription(app: &App) -> Subscription<AppMessage> {
//!     match &app.tray.handle {
//!         Some(handle) => events::subscription(&handle.actions)
//!             .map(|action| AppMessage::Tray(Message::Menu(action))),
//!         None => Subscription::none(),
//!     }
//! }
//! ```
//!
//! The subscription is identified by the receiver, so it keeps running across
//! `subscription()` calls for as long as the receiver stays in the state, and a
//! new receiver (after re-registering) starts a new one.

use chartreuse_platform::EventReceiver;
use iced::Subscription;

/// The events of `receiver` as a subscription.
pub fn subscription<T: Send + 'static>(receiver: &EventReceiver<T>) -> Subscription<T> {
    Subscription::run_with(receiver.clone(), EventReceiver::stream)
}

#[cfg(test)]
mod tests {
    use std::hash::Hasher as _;

    use chartreuse_platform::event;
    use futures::executor::block_on;
    use futures::StreamExt;
    use iced::advanced::subscription::{into_recipes, Hasher, Recipe};

    use super::*;

    fn only_recipe<T: Send + 'static>(receiver: &EventReceiver<T>) -> Box<dyn Recipe<Output = T>> {
        let mut recipes = into_recipes(subscription(receiver));
        assert_eq!(recipes.len(), 1);
        recipes.pop().unwrap()
    }

    fn identity<T: Send + 'static>(receiver: &EventReceiver<T>) -> u64 {
        let recipe = only_recipe(receiver);
        let mut hasher = Hasher::default();
        recipe.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn identity_follows_the_receiver() {
        let (_, a) = event::channel::<u8>();
        let (_, b) = event::channel::<u8>();
        assert_eq!(identity(&a), identity(&a.clone()));
        assert_ne!(identity(&a), identity(&b));
    }

    #[test]
    fn the_subscription_streams_the_sent_events() {
        let (sender, receiver) = event::channel();
        sender.send(7);
        sender.send(8);
        drop(sender);
        let recipe = only_recipe(&receiver);
        let events: Vec<i32> = block_on(recipe.stream(futures::stream::empty().boxed()).collect());
        assert_eq!(events, [7, 8]);
    }
}

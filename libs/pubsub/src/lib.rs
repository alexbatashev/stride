//! In-process, global pub/sub.
//!
//! Topics are identified by a name and an event type. Calling [`topic`] with the
//! same name and type anywhere in the process yields handles to the same
//! channel, so producers and consumers do not need to share state explicitly.
//! Each subscriber receives its own copy of every event published after it
//! subscribed.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use thiserror::Error;
use tokio::sync::broadcast;

/// Buffered events kept per topic before slow subscribers start lagging.
const DEFAULT_CAPACITY: usize = 256;

type Registry = Mutex<HashMap<(TypeId, String), Box<dyn Any + Send + Sync>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Error)]
pub enum RecvError {
    /// The topic was removed and no producers remain.
    #[error("topic closed")]
    Closed,
    /// The subscriber fell behind and skipped `0` events; recv can continue.
    #[error("lagged behind, skipped {0} events")]
    Lagged(u64),
}

/// Handle to a global topic. Cheap to clone; clones share the same channel.
pub struct Topic<T> {
    tx: broadcast::Sender<T>,
}

impl<T> Clone for Topic<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T: Clone + Send + 'static> Topic<T> {
    /// Send an event to every current subscriber. Returns how many subscribers
    /// received it; publishing to a topic with no subscribers is not an error.
    pub fn publish(&self, event: T) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// Start receiving events published from now on.
    pub fn subscribe(&self) -> Subscriber<T> {
        Subscriber {
            rx: self.tx.subscribe(),
        }
    }

    /// Number of live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// Receiving end of a topic.
pub struct Subscriber<T> {
    rx: broadcast::Receiver<T>,
}

impl<T: Clone + Send + 'static> Subscriber<T> {
    /// Await the next event. Returns [`RecvError::Lagged`] if events were missed
    /// (further calls keep working) or [`RecvError::Closed`] once the topic is
    /// gone and drained.
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        match self.rx.recv().await {
            Ok(event) => Ok(event),
            Err(broadcast::error::RecvError::Closed) => Err(RecvError::Closed),
            Err(broadcast::error::RecvError::Lagged(n)) => Err(RecvError::Lagged(n)),
        }
    }
}

/// Get a handle to the global topic `name` carrying events of type `T`,
/// creating it if it does not exist yet. Names are scoped per event type.
pub fn topic<T: Clone + Send + 'static>(name: &str) -> Topic<T> {
    let key = (TypeId::of::<T>(), name.to_string());
    let mut reg = registry().lock().unwrap();
    let entry = reg.entry(key).or_insert_with(|| {
        let (tx, _) = broadcast::channel::<T>(DEFAULT_CAPACITY);
        Box::new(tx)
    });
    let tx = entry
        .downcast_ref::<broadcast::Sender<T>>()
        .expect("topic type mismatch")
        .clone();
    Topic { tx }
}

/// Drop the global topic `name` for event type `T`. Returns `true` if it
/// existed. Outstanding [`Topic`] handles keep the channel alive until dropped;
/// subscribers then observe [`RecvError::Closed`]. A later [`topic`] call with
/// the same name creates a fresh, independent channel.
pub fn remove<T: 'static>(name: &str) -> bool {
    let key = (TypeId::of::<T>(), name.to_string());
    registry().lock().unwrap().remove(&key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_subscriber() {
        let t = topic::<i32>("publish_reaches_subscriber");
        let mut sub = t.subscribe();
        assert_eq!(t.publish(7), 1);
        assert_eq!(sub.recv().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn every_subscriber_gets_a_copy() {
        let t = topic::<String>("every_subscriber_gets_a_copy");
        let mut a = t.subscribe();
        let mut b = t.subscribe();
        assert_eq!(t.publish("hi".to_string()), 2);
        assert_eq!(a.recv().await.unwrap(), "hi");
        assert_eq!(b.recv().await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn handles_share_one_global_channel() {
        let mut sub = topic::<u8>("handles_share_one_global_channel").subscribe();
        // A separately acquired handle to the same name publishes to the same channel.
        topic::<u8>("handles_share_one_global_channel").publish(42);
        assert_eq!(sub.recv().await.unwrap(), 42);
    }

    #[tokio::test]
    async fn publish_without_subscribers_is_ok() {
        let t = topic::<i32>("publish_without_subscribers_is_ok");
        assert_eq!(t.publish(1), 0);
    }

    #[tokio::test]
    async fn same_name_different_type_are_distinct() {
        let mut ints = topic::<i32>("dup").subscribe();
        topic::<String>("dup").publish("text".to_string());
        topic::<i32>("dup").publish(9);
        assert_eq!(ints.recv().await.unwrap(), 9);
    }

    #[tokio::test]
    async fn remove_drops_the_topic() {
        let name = "remove_drops_the_topic";
        let _t = topic::<i32>(name);
        assert!(remove::<i32>(name));
        assert!(!remove::<i32>(name));

        // A fresh topic of the same name is an independent channel.
        let mut sub = topic::<i32>(name).subscribe();
        topic::<i32>(name).publish(5);
        assert_eq!(sub.recv().await.unwrap(), 5);
    }

    #[tokio::test]
    async fn subscriber_sees_closed_after_all_senders_drop() {
        let name = "subscriber_sees_closed_after_all_senders_drop";
        let t = topic::<i32>(name);
        let mut sub = t.subscribe();
        remove::<i32>(name); // drop registry's sender
        drop(t); // drop the last remaining sender
        assert!(matches!(sub.recv().await, Err(RecvError::Closed)));
    }
}

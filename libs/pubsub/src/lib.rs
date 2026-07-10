//! In-process, global pub/sub over a stream-shaped [`EventTransport`].
//!
//! Topics are identified by a name. Calling [`topic`] with the same name
//! anywhere in the process yields handles to the same channel, so producers and
//! consumers do not need to share state explicitly.
//!
//! Each topic assigns every published event a monotonic [`Offset`] and keeps a
//! bounded backlog of its most recent events. A new subscriber first replays a
//! window (the whole backlog, or everything past a cursor), then receives live
//! events — so a late joiner does not miss what happened just before it
//! subscribed, and a reconnecting one can resume from its last offset.
//!
//! ## Delivery contract
//!
//! - **Offsets are per-topic and monotonic.** `publish` returns the offset it
//!   assigned; offsets carry no meaning across topics.
//! - **Ordering is per-topic only.** No ordering is guaranteed between topics.
//! - **Live delivery is at-most-once.** A subscriber that falls behind the live
//!   buffer, or resumes from a cursor older than the retained window, observes
//!   [`RecvError::Lagged`] instead of silently losing events.
//! - **The replayable window equals retention.** Events evicted from the backlog
//!   cannot be replayed.
//! - **Publish after [`remove`] is an error.** A handle whose topic was removed
//!   returns [`PublishError::Closed`]; it does not silently re-create the topic.

mod memory;
mod transport;

use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

pub use transport::{
    EventTransport, Offset, RawDelivery, RawSubscription, TopicHandle, TransportError,
};

pub use memory::{DEFAULT_CAPACITY, InMemoryTransport};

/// Failure publishing a typed event.
#[derive(Debug, Error)]
pub enum PublishError {
    /// The topic was removed; this handle can no longer publish.
    #[error("topic closed")]
    Closed,
    /// The event could not be serialized.
    #[error("failed to encode payload: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Failure receiving a typed event.
#[derive(Debug, Error)]
pub enum RecvError {
    /// The topic was removed and its retained window is drained.
    #[error("topic closed")]
    Closed,
    /// The subscriber fell behind and skipped `0` events; recv can continue.
    #[error("lagged behind, skipped {0} events")]
    Lagged(u64),
    /// A delivered payload could not be deserialized as `T`.
    #[error("failed to decode payload: {0}")]
    Decode(serde_json::Error),
}

/// Handle to a global topic carrying events of type `T`. Cheap to clone; clones
/// share the same channel. Serialization to the byte transport is JSON.
pub struct Topic<T> {
    handle: Arc<dyn TopicHandle>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Topic<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> Topic<T> {
    /// Subscribe, replaying the whole current backlog before live events.
    pub fn subscribe(&self) -> Subscriber<T> {
        self.subscribe_from(None)
    }

    /// Subscribe from a cursor: replay events with offset greater than `from`
    /// (or the whole backlog when `from` is `None`) before live events. A
    /// cursor older than the retained window yields [`RecvError::Lagged`] first.
    pub fn subscribe_from(&self, from: Option<Offset>) -> Subscriber<T> {
        Subscriber {
            inner: self.handle.subscribe(from),
            _marker: PhantomData,
        }
    }
}

impl<T: Serialize> Topic<T> {
    /// Serialize and publish `event`, returning its transport-assigned offset.
    /// Errors if the topic was removed or the event fails to serialize.
    pub fn publish(&self, event: &T) -> Result<Offset, PublishError> {
        let payload = serde_json::to_vec(event)?;
        self.handle.publish(payload).map_err(|e| match e {
            TransportError::Closed => PublishError::Closed,
        })
    }
}

/// Receiving end of a topic.
pub struct Subscriber<T> {
    inner: Box<dyn RawSubscription>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Subscriber<T> {
    /// Await the next event, draining any replay window first. Returns
    /// [`RecvError::Lagged`] if events were skipped (further calls keep working)
    /// or [`RecvError::Closed`] once the topic is gone and drained.
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        self.recv_with_offset().await.map(|(_, event)| event)
    }

    /// Like [`recv`](Self::recv) but also yields the event's offset, for
    /// callers that persist a resume cursor.
    pub async fn recv_with_offset(&mut self) -> Result<(Offset, T), RecvError> {
        match self.inner.recv().await {
            RawDelivery::Event { offset, payload } => serde_json::from_slice(&payload)
                .map(|event| (offset, event))
                .map_err(RecvError::Decode),
            RawDelivery::Lagged { skipped } => Err(RecvError::Lagged(skipped)),
            RawDelivery::Closed => Err(RecvError::Closed),
        }
    }
}

/// Get a handle to the global topic `name` carrying events of type `T`, creating
/// it if it does not exist yet. A given name must be used with one payload type.
pub fn topic<T>(name: &str) -> Topic<T> {
    Topic {
        handle: memory::global().topic(name),
        _marker: PhantomData,
    }
}

/// Drop the global topic `name`, discarding its backlog. Returns `true` if it
/// existed. Handles held across this call observe [`PublishError::Closed`] on
/// publish; a later [`topic`] call with the same name creates a fresh channel.
pub fn remove(name: &str) -> bool {
    memory::global().remove(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_subscriber() {
        let t = topic::<i32>("publish_reaches_subscriber");
        let mut sub = t.subscribe();
        t.publish(&7).unwrap();
        assert_eq!(sub.recv().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn every_subscriber_gets_a_copy() {
        let t = topic::<String>("every_subscriber_gets_a_copy");
        let mut a = t.subscribe();
        let mut b = t.subscribe();
        t.publish(&"hi".to_string()).unwrap();
        assert_eq!(a.recv().await.unwrap(), "hi");
        assert_eq!(b.recv().await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn handles_share_one_global_channel() {
        let mut sub = topic::<u8>("handles_share_one_global_channel").subscribe();
        topic::<u8>("handles_share_one_global_channel")
            .publish(&42)
            .unwrap();
        assert_eq!(sub.recv().await.unwrap(), 42);
    }

    #[tokio::test]
    async fn late_subscriber_replays_backlog_then_live() {
        let t = topic::<i32>("late_subscriber_replays_backlog_then_live");
        t.publish(&1).unwrap();
        t.publish(&2).unwrap();
        let mut sub = t.subscribe();
        assert_eq!(sub.recv().await.unwrap(), 1);
        assert_eq!(sub.recv().await.unwrap(), 2);
        t.publish(&3).unwrap();
        assert_eq!(sub.recv().await.unwrap(), 3);
    }

    // -- Contract tests over the transport trait ---------------------------

    fn fresh() -> InMemoryTransport {
        InMemoryTransport::with_capacity(4)
    }

    async fn next(sub: &mut Box<dyn RawSubscription>) -> RawDelivery {
        sub.recv().await
    }

    #[tokio::test]
    async fn publish_returns_increasing_offsets_per_topic() {
        let bus = fresh();
        let a = bus.topic("a");
        let b = bus.topic("b");
        assert_eq!(a.publish(b"1".to_vec()).unwrap(), 0);
        assert_eq!(a.publish(b"2".to_vec()).unwrap(), 1);
        assert_eq!(a.publish(b"3".to_vec()).unwrap(), 2);
        assert_eq!(b.publish(b"x".to_vec()).unwrap(), 0);
    }

    #[tokio::test]
    async fn per_topic_ordering_is_preserved() {
        let bus = fresh();
        let t = bus.topic("order");
        for i in 0..4u8 {
            t.publish(vec![i]).unwrap();
        }
        let mut sub = t.subscribe(None);
        for i in 0..4u8 {
            assert_eq!(
                next(&mut sub).await,
                RawDelivery::Event {
                    offset: i as Offset,
                    payload: vec![i],
                }
            );
        }
    }

    #[tokio::test]
    async fn replay_from_offset_is_exact() {
        let bus = fresh();
        let t = bus.topic("replay");
        for i in 0..4u8 {
            t.publish(vec![i]).unwrap();
        }
        let mut sub = t.subscribe(Some(1));
        assert_eq!(
            next(&mut sub).await,
            RawDelivery::Event {
                offset: 2,
                payload: vec![2],
            }
        );
        assert_eq!(
            next(&mut sub).await,
            RawDelivery::Event {
                offset: 3,
                payload: vec![3],
            }
        );
    }

    #[tokio::test]
    async fn overflow_past_retention_reports_lag() {
        let bus = fresh(); // capacity 4
        let t = bus.topic("overflow");
        for i in 0..8u8 {
            t.publish(vec![i]).unwrap();
        }
        let mut sub = t.subscribe(Some(0));
        assert_eq!(next(&mut sub).await, RawDelivery::Lagged { skipped: 3 });
        assert_eq!(
            next(&mut sub).await,
            RawDelivery::Event {
                offset: 4,
                payload: vec![4],
            }
        );
    }

    #[tokio::test]
    async fn backlog_trims_to_capacity() {
        let bus = fresh(); // capacity 4
        let t = bus.topic("trim");
        for i in 0..6u8 {
            t.publish(vec![i]).unwrap();
        }
        let mut sub = t.subscribe(None);
        assert_eq!(
            next(&mut sub).await,
            RawDelivery::Event {
                offset: 2,
                payload: vec![2],
            }
        );
    }

    #[tokio::test]
    async fn publish_after_remove_is_an_error() {
        let bus = fresh();
        let t = bus.topic("closable");
        t.publish(b"before".to_vec()).unwrap();
        assert!(bus.remove("closable"));
        assert_eq!(t.publish(b"after".to_vec()), Err(TransportError::Closed));
        let t2 = bus.topic("closable");
        assert_eq!(t2.publish(b"new".to_vec()).unwrap(), 0);
    }

    #[tokio::test]
    async fn removing_absent_topic_is_false() {
        let bus = fresh();
        assert!(!bus.remove("never-created"));
    }

    #[tokio::test]
    async fn subscriber_sees_closed_after_topic_dropped() {
        let bus = fresh();
        let mut sub = bus.topic("dropme").subscribe(None);
        bus.remove("dropme");
        assert_eq!(next(&mut sub).await, RawDelivery::Closed);
    }

    #[tokio::test]
    async fn typed_publish_after_remove_reports_closed() {
        let t = topic::<i32>("typed_publish_after_remove_reports_closed");
        t.publish(&1).unwrap();
        remove("typed_publish_after_remove_reports_closed");
        assert!(matches!(t.publish(&2), Err(PublishError::Closed)));
    }

    #[tokio::test]
    async fn typed_recv_with_offset_tracks_cursor() {
        let t = topic::<i32>("typed_recv_with_offset_tracks_cursor");
        t.publish(&10).unwrap();
        t.publish(&11).unwrap();
        let mut sub = t.subscribe();
        assert_eq!(sub.recv_with_offset().await.unwrap(), (0, 10));
        assert_eq!(sub.recv_with_offset().await.unwrap(), (1, 11));
    }
}

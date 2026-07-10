//! Transport boundary for the event bus.
//!
//! The trait is modeled on stream transports (Redis Streams, Kafka): every
//! publish is assigned a per-topic monotonic [`Offset`], and a subscriber can
//! replay from an arbitrary cursor before tailing live events. Payloads are
//! opaque bytes so a topic can cross a process boundary; the typed [`crate::Topic`]
//! wrapper above this layer owns (de)serialization.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

/// Per-topic, transport-assigned position of an event. Monotonically increasing
/// within one topic; carries no meaning across topics. Maps to a Redis Stream
/// entry id or a Kafka partition offset.
pub type Offset = u64;

/// Failure returned by [`TopicHandle::publish`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    /// The topic this handle points at was removed; publishing is no longer
    /// possible. A fresh handle from [`EventTransport::topic`] opens a new,
    /// independent topic under the same name.
    #[error("topic closed")]
    Closed,
}

/// One item yielded by a [`RawSubscription`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawDelivery {
    /// A live or replayed event and its offset.
    Event { offset: Offset, payload: Vec<u8> },
    /// The subscriber's requested cursor, or its live buffer, fell outside the
    /// retained window; `skipped` events were lost and cannot be replayed.
    Lagged { skipped: u64 },
    /// The topic was removed and its retained window is drained.
    Closed,
}

/// Receiving end of a subscription. Backend-specific; obtained from
/// [`TopicHandle::subscribe`].
#[async_trait]
pub trait RawSubscription: Send {
    /// Await the next delivery. After [`RawDelivery::Closed`] no further events
    /// arrive.
    async fn recv(&mut self) -> RawDelivery;
}

/// Handle to a single topic. Cheap to clone via `Arc`; every handle to the same
/// topic instance shares its offsets and retained window. This is the
/// object-safe realization of H1's `publish(topic, ...)` / `subscribe(topic, ...)`
/// primitives: the topic is resolved once by [`EventTransport::topic`], so
/// publishing through a handle whose topic was removed is an observable error
/// rather than a silent re-create.
pub trait TopicHandle: Send + Sync {
    /// Append `payload`, returning its assigned offset. Errors if the topic was
    /// removed.
    fn publish(&self, payload: Vec<u8>) -> Result<Offset, TransportError>;

    /// Subscribe. `from == None` replays the whole retained window before live
    /// events; `from == Some(o)` replays only offsets greater than `o`. If `o`
    /// predates the retained window the subscription's first delivery is
    /// [`RawDelivery::Lagged`].
    fn subscribe(&self, from: Option<Offset>) -> Box<dyn RawSubscription>;
}

/// A pub/sub transport. The in-memory implementation ships in [`crate::memory`];
/// a Redis/Kafka backend would implement the same trait so swapping is a config
/// change.
pub trait EventTransport: Send + Sync {
    /// Resolve `name` to its current topic instance, creating it if absent.
    fn topic(&self, name: &str) -> Arc<dyn TopicHandle>;

    /// Remove `name`. Handles already held by callers observe
    /// [`TransportError::Closed`] on publish; drained subscribers observe
    /// [`RawDelivery::Closed`]. Returns whether a topic existed.
    fn remove(&self, name: &str) -> bool;
}

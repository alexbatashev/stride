//! In-memory [`EventTransport`]: a bounded per-topic backlog plus a broadcast
//! live tail, honoring the stream contract (per-topic offsets, replay-from-cursor,
//! detectable overflow, publish-after-remove error).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::transport::{
    EventTransport, Offset, RawDelivery, RawSubscription, TopicHandle, TransportError,
};

/// Events retained per topic for replay, and the live broadcast buffer size.
pub const DEFAULT_CAPACITY: usize = 256;

type Frame = (Offset, Vec<u8>);

/// Process-wide transport backing the free functions in [`crate`].
pub fn global() -> &'static InMemoryTransport {
    static GLOBAL: OnceLock<InMemoryTransport> = OnceLock::new();
    GLOBAL.get_or_init(InMemoryTransport::default)
}

/// In-memory transport. Topics are created lazily and keyed by name only, so a
/// name identifies one payload type per process.
pub struct InMemoryTransport {
    topics: Mutex<HashMap<String, Arc<MemoryTopic>>>,
    capacity: usize,
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl InMemoryTransport {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            capacity,
        }
    }
}

impl EventTransport for InMemoryTransport {
    fn topic(&self, name: &str) -> Arc<dyn TopicHandle> {
        let mut topics = self.topics.lock().unwrap();
        let topic = topics
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(MemoryTopic::new(self.capacity)))
            .clone();
        topic as Arc<dyn TopicHandle>
    }

    fn remove(&self, name: &str) -> bool {
        let Some(topic) = self.topics.lock().unwrap().remove(name) else {
            return false;
        };
        topic.close();
        true
    }
}

/// One topic instance: its offset counter, retained backlog and live sender,
/// updated together under `inner` so a subscriber observes each event once.
struct MemoryTopic {
    inner: Mutex<Inner>,
    tx: broadcast::Sender<Frame>,
    capacity: usize,
}

struct Inner {
    next_offset: Offset,
    backlog: VecDeque<Frame>,
    closed: bool,
}

impl MemoryTopic {
    fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel::<Frame>(capacity);
        Self {
            inner: Mutex::new(Inner {
                next_offset: 0,
                backlog: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            tx,
            capacity,
        }
    }

    fn close(&self) {
        self.inner.lock().unwrap().closed = true;
    }
}

impl TopicHandle for MemoryTopic {
    fn publish(&self, payload: Vec<u8>) -> Result<Offset, TransportError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(TransportError::Closed);
        }
        let offset = inner.next_offset;
        inner.next_offset += 1;
        if inner.backlog.len() == self.capacity {
            inner.backlog.pop_front();
        }
        inner.backlog.push_back((offset, payload.clone()));
        let _ = self.tx.send((offset, payload));
        Ok(offset)
    }

    fn subscribe(&self, from: Option<Offset>) -> Box<dyn RawSubscription> {
        let inner = self.inner.lock().unwrap();
        let rx = self.tx.subscribe();
        let replay: VecDeque<Frame> = match from {
            None => inner.backlog.clone(),
            Some(cursor) => inner
                .backlog
                .iter()
                .filter(|(offset, _)| *offset > cursor)
                .cloned()
                .collect(),
        };
        let lag = overflow_gap(from, inner.backlog.front().map(|(offset, _)| *offset));
        Box::new(MemorySubscription { lag, replay, rx })
    }

    fn subscribe_live(&self) -> Box<dyn RawSubscription> {
        let rx = self.tx.subscribe();
        Box::new(MemorySubscription {
            lag: None,
            replay: VecDeque::new(),
            rx,
        })
    }
}

/// Events lost between the requested cursor and the oldest retained offset.
fn overflow_gap(from: Option<Offset>, earliest: Option<Offset>) -> Option<u64> {
    match (from, earliest) {
        (Some(cursor), Some(earliest)) if earliest > cursor + 1 => Some(earliest - (cursor + 1)),
        _ => None,
    }
}

struct MemorySubscription {
    lag: Option<u64>,
    replay: VecDeque<Frame>,
    rx: broadcast::Receiver<Frame>,
}

#[async_trait]
impl RawSubscription for MemorySubscription {
    async fn recv(&mut self) -> RawDelivery {
        if let Some(skipped) = self.lag.take() {
            return RawDelivery::Lagged { skipped };
        }
        if let Some((offset, payload)) = self.replay.pop_front() {
            return RawDelivery::Event { offset, payload };
        }
        match self.rx.recv().await {
            Ok((offset, payload)) => RawDelivery::Event { offset, payload },
            Err(broadcast::error::RecvError::Closed) => RawDelivery::Closed,
            Err(broadcast::error::RecvError::Lagged(skipped)) => RawDelivery::Lagged { skipped },
        }
    }
}

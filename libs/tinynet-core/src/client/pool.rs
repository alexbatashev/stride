use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::protocol::AnyTransport;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct PoolKey {
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: u16,
}

struct Idle {
    transport: AnyTransport,
    since: Instant,
}

#[derive(Default)]
pub(crate) struct Pool {
    idle: Mutex<HashMap<PoolKey, VecDeque<Idle>>>,
}

impl Pool {
    pub(crate) fn checkout(&self, key: &PoolKey, idle_timeout: Duration) -> Option<AnyTransport> {
        let now = Instant::now();
        let mut all = self.idle.lock().unwrap();
        let connections = all.get_mut(key)?;
        connections.retain(|idle| now.duration_since(idle.since) < idle_timeout);
        let transport = connections.pop_back().map(|idle| idle.transport);
        if connections.is_empty() {
            all.remove(key);
        }
        transport
    }

    pub(crate) fn checkin(&self, key: PoolKey, transport: AnyTransport, max_idle: usize) {
        if max_idle == 0 {
            return;
        }
        let mut all = self.idle.lock().unwrap();
        let connections = all.entry(key).or_default();
        while connections.len() >= max_idle {
            connections.pop_front();
        }
        connections.push_back(Idle {
            transport,
            since: Instant::now(),
        });
    }
}

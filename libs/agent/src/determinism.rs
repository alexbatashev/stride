use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

/// Injectable source of time. Production code depends on this instead of calling
/// [`SystemTime::now`]/[`Instant::now`] directly so that time-dependent logic
/// (scheduling, rate limiting, retention stamps) can be driven deterministically
/// in tests.
pub trait Clock: Send + Sync {
    /// Current wall-clock time.
    fn now_system(&self) -> SystemTime;

    /// Monotonic instant, used for measuring elapsed durations.
    fn now_instant(&self) -> Instant;

    /// Current wall-clock time as whole seconds since the Unix epoch.
    fn now_unix_secs(&self) -> i64 {
        self.now_system()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Current wall-clock time as milliseconds since the Unix epoch.
    fn now_unix_millis(&self) -> i64 {
        self.now_system()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}

/// Injectable identifier generator. Production code depends on this instead of
/// calling [`Uuid::now_v7`]/[`Uuid::new_v4`] directly so that generated ids are
/// reproducible in tests.
pub trait IdGen: Send + Sync {
    /// Time-ordered UUID (version 7).
    fn new_uuid_v7(&self) -> Uuid;

    /// Random UUID (version 4).
    fn new_uuid_v4(&self) -> Uuid;
}

/// Wall clock backed by the operating system.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_system(&self) -> SystemTime {
        SystemTime::now()
    }

    fn now_instant(&self) -> Instant {
        Instant::now()
    }
}

/// Identifier generator backed by the `uuid` crate's real generators.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemIdGen;

impl IdGen for SystemIdGen {
    fn new_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }

    fn new_uuid_v4(&self) -> Uuid {
        Uuid::new_v4()
    }
}

/// Deterministic clock for tests. Wall-clock time starts at a fixed value and
/// only advances when explicitly told to (or by a fixed step per read); the
/// monotonic instant advances the same way, keeping durations reproducible.
pub struct TestClock {
    millis: AtomicI64,
    step_millis: i64,
    base_instant: Instant,
    instant_nanos: AtomicU64,
    instant_step_nanos: u64,
}

impl TestClock {
    /// Clock frozen at `start_unix_millis`.
    pub fn new(start_unix_millis: i64) -> Self {
        Self {
            millis: AtomicI64::new(start_unix_millis),
            step_millis: 0,
            base_instant: Instant::now(),
            instant_nanos: AtomicU64::new(0),
            instant_step_nanos: 0,
        }
    }

    /// Clock starting at `start_unix_millis` that advances by `step_millis`
    /// (both wall-clock and monotonic) on every read.
    pub fn with_step(start_unix_millis: i64, step_millis: i64) -> Self {
        Self {
            millis: AtomicI64::new(start_unix_millis),
            step_millis,
            base_instant: Instant::now(),
            instant_nanos: AtomicU64::new(0),
            instant_step_nanos: (step_millis.max(0) as u64) * 1_000_000,
        }
    }

    /// Move wall-clock time forward by `delta_millis`.
    pub fn advance_millis(&self, delta_millis: i64) {
        self.millis.fetch_add(delta_millis, Ordering::SeqCst);
    }

    /// Set wall-clock time to an absolute value.
    pub fn set_unix_millis(&self, unix_millis: i64) {
        self.millis.store(unix_millis, Ordering::SeqCst);
    }

    /// Move the monotonic instant forward.
    pub fn advance_instant(&self, delta: Duration) {
        self.instant_nanos
            .fetch_add(delta.as_nanos() as u64, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_system(&self) -> SystemTime {
        let ms = self.millis.fetch_add(self.step_millis, Ordering::SeqCst);
        UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64)
    }

    fn now_instant(&self) -> Instant {
        let nanos = self
            .instant_nanos
            .fetch_add(self.instant_step_nanos, Ordering::SeqCst);
        self.base_instant + Duration::from_nanos(nanos)
    }
}

/// Deterministic id generator for tests. Produces a reproducible, strictly
/// increasing sequence of UUIDs (with valid v4/v7 version and variant bits) from
/// a seed and a monotonic counter.
pub struct SeededIdGen {
    seed: u64,
    v7_counter: AtomicU64,
    v4_counter: AtomicU64,
}

impl SeededIdGen {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            v7_counter: AtomicU64::new(0),
            v4_counter: AtomicU64::new(0),
        }
    }

    fn build(&self, counter: u64, version: u8) -> Uuid {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&counter.to_be_bytes());
        bytes[8..16].copy_from_slice(&self.seed.to_be_bytes());
        bytes[6] = (bytes[6] & 0x0F) | (version << 4);
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Uuid::from_bytes(bytes)
    }
}

impl IdGen for SeededIdGen {
    fn new_uuid_v7(&self) -> Uuid {
        let counter = self.v7_counter.fetch_add(1, Ordering::SeqCst);
        self.build(counter, 7)
    }

    fn new_uuid_v4(&self) -> Uuid {
        let counter = self.v4_counter.fetch_add(1, Ordering::SeqCst);
        self.build(counter, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_reports_recent_time() {
        let clock = SystemClock;
        assert!(clock.now_unix_secs() > 1_700_000_000);
    }

    #[test]
    fn test_clock_is_frozen_by_default() {
        let clock = TestClock::new(1_000);
        assert_eq!(clock.now_unix_millis(), 1_000);
        assert_eq!(clock.now_unix_millis(), 1_000);
        assert_eq!(clock.now_unix_secs(), 1);
    }

    #[test]
    fn test_clock_steps_and_advances() {
        let clock = TestClock::with_step(0, 5);
        assert_eq!(clock.now_unix_millis(), 0);
        assert_eq!(clock.now_unix_millis(), 5);
        clock.advance_millis(90);
        assert_eq!(clock.now_unix_millis(), 100);
    }

    #[test]
    fn test_clock_instant_monotonic() {
        let clock = TestClock::new(0);
        let a = clock.now_instant();
        clock.advance_instant(Duration::from_secs(3));
        let b = clock.now_instant();
        assert_eq!(b.duration_since(a), Duration::from_secs(3));
    }

    #[test]
    fn seeded_idgen_is_deterministic_and_ordered() {
        let a = SeededIdGen::new(42);
        let b = SeededIdGen::new(42);
        let first = a.new_uuid_v7();
        let second = a.new_uuid_v7();
        assert_eq!(first, b.new_uuid_v7());
        assert_eq!(second, b.new_uuid_v7());
        assert_ne!(first, second);
        assert!(first < second);
        assert_eq!(first.get_version_num(), 7);
    }

    #[test]
    fn seeded_idgen_v4_has_version_bits() {
        let idgen = SeededIdGen::new(1);
        assert_eq!(idgen.new_uuid_v4().get_version_num(), 4);
    }
}

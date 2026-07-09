use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    future::Future,
    pin::Pin,
    sync::{Condvar, Mutex},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use super::REACTOR;

struct State {
    next_id: u64,
    heap: BinaryHeap<Reverse<(Instant, u64)>>,
    wakers: HashMap<u64, Waker>,
}

pub(crate) struct TimerQueue {
    state: Mutex<State>,
    changed: Condvar,
}

impl TimerQueue {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next_id: 0,
                heap: BinaryHeap::new(),
                wakers: HashMap::new(),
            }),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn register(&self, deadline: Instant, id: Option<u64>, waker: Waker) -> u64 {
        let mut state = self.state.lock().unwrap();
        let id = id.unwrap_or_else(|| {
            let id = state.next_id;
            state.next_id = state.next_id.wrapping_add(1);
            state.heap.push(Reverse((deadline, id)));
            id
        });
        let wake_driver = state
            .heap
            .peek()
            .is_some_and(|head| head.0 == (deadline, id));
        state.wakers.insert(id, waker);
        drop(state);
        if wake_driver {
            self.changed.notify_one();
        }
        id
    }

    pub(crate) fn cancel(&self, id: u64) {
        self.state.lock().unwrap().wakers.remove(&id);
        self.changed.notify_one();
    }

    pub(crate) fn run_loop(&self) {
        let mut state = self.state.lock().unwrap();
        loop {
            while let Some(Reverse((_, id))) = state.heap.peek() {
                if state.wakers.contains_key(id) {
                    break;
                }
                state.heap.pop();
            }

            let Some(Reverse((deadline, _))) = state.heap.peek().copied() else {
                state = self.changed.wait(state).unwrap();
                continue;
            };
            let now = Instant::now();
            if deadline > now {
                let (next, _) = self.changed.wait_timeout(state, deadline - now).unwrap();
                state = next;
                continue;
            }

            let mut ready = Vec::new();
            while let Some(Reverse((deadline, id))) = state.heap.peek().copied() {
                if deadline > Instant::now() {
                    break;
                }
                state.heap.pop();
                ready.extend(state.wakers.remove(&id));
            }
            drop(state);
            for waker in ready {
                waker.wake();
            }
            state = self.state.lock().unwrap();
        }
    }
}

pub struct Sleep {
    deadline: Instant,
    id: Option<u64>,
}

pub fn sleep(duration: Duration) -> Sleep {
    deadline(Instant::now() + duration)
}

pub fn deadline(at: Instant) -> Sleep {
    Sleep {
        deadline: at,
        id: None,
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() >= self.deadline {
            if let Some(id) = self.id.take() {
                REACTOR.cancel_timer(id);
            }
            return Poll::Ready(());
        }
        self.id = Some(REACTOR.register_timer(self.deadline, self.id, context.waker().clone()));
        Poll::Pending
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            REACTOR.cancel_timer(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sleeps_finish_in_deadline_order() {
        let start = Instant::now();
        let late = tokio::spawn(async move {
            sleep(Duration::from_millis(30)).await;
            2
        });
        let early = tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            1
        });

        assert_eq!(early.await.unwrap(), 1);
        assert!(start.elapsed() >= Duration::from_millis(10));
        assert_eq!(late.await.unwrap(), 2);
    }

    #[tokio::test]
    async fn cancelled_sleep_does_not_fire() {
        let sleep = sleep(Duration::from_millis(10));
        drop(sleep);
        super::sleep(Duration::from_millis(20)).await;
    }
}

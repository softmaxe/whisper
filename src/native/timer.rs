//! Injectable clock and one-shot timers.
//!
//! The timer-driven state machines (hotkey gesture, push-to-talk, listener
//! restart policy, lid monitor) take an `Arc<dyn Timers>` so production uses one shared timer thread and tests
//! drive a manual clock.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Identifies a scheduled callback so it can be cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

pub type TimerCallback = Box<dyn FnOnce() + Send + 'static>;

pub trait Timers: Send + Sync {
    /// Monotonic milliseconds since an arbitrary origin.
    fn now_ms(&self) -> u64;
    /// Runs `callback` once after `delay`, on a timer thread.
    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId;
    /// Cancels a pending callback; a no-op if it already ran.
    fn cancel(&self, id: TimerId);
}

/// Locks a mutex, recovering the data if a previous holder panicked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct RealState {
    next_id: u64,
    // Ordered by (deadline, id) so equal deadlines fire in scheduling order.
    queue: BTreeMap<(Instant, u64), TimerCallback>,
    deadlines: HashMap<u64, Instant>,
}

/// Production timers: one background thread runs every callback.
pub struct RealTimers {
    origin: Instant,
    state: Mutex<RealState>,
    wake: Condvar,
}

impl RealTimers {
    /// The process-wide timer thread.
    pub fn shared() -> Arc<dyn Timers> {
        static SHARED: OnceLock<Arc<RealTimers>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                let timers = Arc::new(RealTimers {
                    origin: Instant::now(),
                    state: Mutex::new(RealState {
                        next_id: 1,
                        queue: BTreeMap::new(),
                        deadlines: HashMap::new(),
                    }),
                    wake: Condvar::new(),
                });
                let worker = Arc::clone(&timers);
                let spawned = std::thread::Builder::new()
                    .name("whisper-timers".into())
                    .spawn(move || worker.run());
                if let Err(error) = spawned {
                    log::error!("[timers] failed to start timer thread: {error}");
                }
                timers
            })
            .clone()
    }

    fn run(&self) {
        let mut state = lock(&self.state);
        loop {
            let now = Instant::now();
            let due = state
                .queue
                .first_key_value()
                .map(|(key, _)| *key)
                .filter(|(deadline, _)| *deadline <= now);
            if let Some(key) = due {
                if let Some(callback) = state.queue.remove(&key) {
                    state.deadlines.remove(&key.1);
                    drop(state);
                    // A panicking callback must not take the timer thread down.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
                    state = lock(&self.state);
                }
                continue;
            }
            let next = state
                .queue
                .first_key_value()
                .map(|((deadline, _), _)| *deadline);
            state = match next {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(now);
                    self.wake
                        .wait_timeout(state, wait)
                        .map(|(guard, _)| guard)
                        .unwrap_or_else(|poisoned| poisoned.into_inner().0)
                }
                None => self
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            };
        }
    }
}

impl Timers for RealTimers {
    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId {
        let mut state = lock(&self.state);
        let id = state.next_id;
        state.next_id += 1;
        let deadline = Instant::now() + delay;
        state.queue.insert((deadline, id), callback);
        state.deadlines.insert(id, deadline);
        drop(state);
        self.wake.notify_one();
        TimerId(id)
    }

    fn cancel(&self, id: TimerId) {
        let mut state = lock(&self.state);
        if let Some(deadline) = state.deadlines.remove(&id.0) {
            state.queue.remove(&(deadline, id.0));
        }
    }
}

/// Manual clock for tests: callbacks run only from [`FakeTimers::advance`].
#[cfg(test)]
pub struct FakeTimers {
    state: Mutex<FakeState>,
}

#[cfg(test)]
struct FakeState {
    now: u64,
    next_id: u64,
    pending: BTreeMap<(u64, u64), TimerCallback>,
    delays: Vec<(u64, Duration)>,
}

#[cfg(test)]
impl FakeTimers {
    pub fn new() -> Arc<Self> {
        Self::starting_at(1000)
    }

    pub fn starting_at(now: u64) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeState {
                now,
                next_id: 1,
                pending: BTreeMap::new(),
                delays: Vec::new(),
            }),
        })
    }

    /// Moves the clock forward, running due callbacks in deadline order.
    pub fn advance(&self, ms: u64) {
        self.advance_inner(ms, true);
    }

    /// Moves the clock without running callbacks (a timer delayed by sleep).
    pub fn advance_without_running(&self, ms: u64) {
        self.advance_inner(ms, false);
    }

    fn advance_inner(&self, ms: u64, run: bool) {
        let target = lock(&self.state).now + ms;
        if run {
            loop {
                let mut state = lock(&self.state);
                let due = state
                    .pending
                    .first_key_value()
                    .map(|(key, _)| *key)
                    .filter(|(at, _)| *at <= target);
                let Some(key) = due else { break };
                let callback = state.pending.remove(&key);
                state.now = state.now.max(key.0);
                drop(state);
                if let Some(callback) = callback {
                    callback();
                }
            }
        }
        lock(&self.state).now = target;
    }

    /// Runs every pending callback whose deadline has passed at the current time.
    pub fn run_due(&self) {
        self.advance(0);
    }

    pub fn pending(&self) -> usize {
        lock(&self.state).pending.len()
    }

    /// Delays of every timer scheduled so far, as (id, delay).
    pub fn scheduled_delays(&self) -> Vec<Duration> {
        lock(&self.state).delays.iter().map(|(_, d)| *d).collect()
    }
}

#[cfg(test)]
impl Timers for FakeTimers {
    fn now_ms(&self) -> u64 {
        lock(&self.state).now
    }

    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId {
        let mut state = lock(&self.state);
        let id = state.next_id;
        state.next_id += 1;
        let at = state.now + delay.as_millis() as u64;
        state.pending.insert((at, id), callback);
        state.delays.push((id, delay));
        TimerId(id)
    }

    fn cancel(&self, id: TimerId) {
        let mut state = lock(&self.state);
        let key = state.pending.keys().find(|(_, k)| *k == id.0).copied();
        if let Some(key) = key {
            state.pending.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn real_timers_fire_and_cancel() {
        let timers = RealTimers::shared();
        let fired = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel();
        let f = Arc::clone(&fired);
        timers.schedule(
            Duration::from_millis(10),
            Box::new(move || {
                f.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            }),
        );
        let f2 = Arc::clone(&fired);
        let cancelled = timers.schedule(
            Duration::from_millis(5),
            Box::new(move || {
                f2.fetch_add(10, Ordering::SeqCst);
            }),
        );
        timers.cancel(cancelled);
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn fake_timers_run_in_deadline_order() {
        let timers = FakeTimers::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        for (delay, label) in [(30u64, "c"), (10, "a"), (20, "b")] {
            let log = Arc::clone(&log);
            timers.schedule(
                Duration::from_millis(delay),
                Box::new(move || lock(&log).push(label)),
            );
        }
        timers.advance(15);
        assert_eq!(*lock(&log), vec!["a"]);
        timers.advance(100);
        assert_eq!(*lock(&log), vec!["a", "b", "c"]);
    }
}

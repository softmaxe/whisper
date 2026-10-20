//! The laptop lid (clamshell) state, watched through
//! `macos-mic-listener --watch-lid-state`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use super::super::helper::resolve_helper;
use super::super::hotkeys::globe::{
    ChildEvent, ChildSink, ListenerChild, ListenerSpawner, ProcessSpawner,
};
use super::super::timer::{RealTimers, TimerId, Timers, lock};

const LID_INITIAL_WAIT_MS: u64 = 1500;
const LID_STALE_STATE_MS: u64 = 5000;
const LID_RESTART_DELAY_MS: u64 = 2000;
const LID_MAX_BUFFER: usize = 4096;
const MIC_LISTENER_BINARY: &str = "macos-mic-listener";

/// `{"lidClosed":true|false|null}` -> Some(state); anything else -> None.
pub fn parse_lid_state(line: &str) -> Option<Option<bool>> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    match value.get("lidClosed")? {
        serde_json::Value::Null => Some(None),
        serde_json::Value::Bool(closed) => Some(Some(*closed)),
        _ => None,
    }
}

pub struct LidHooks {
    pub spawner: Arc<dyn ListenerSpawner>,
    pub resolve_binary: Arc<dyn Fn() -> Option<std::path::PathBuf> + Send + Sync>,
    pub timers: Arc<dyn Timers>,
    /// Called with every change of the known lid state.
    pub on_change: Arc<dyn Fn(Option<bool>) + Send + Sync>,
}

struct Waiter {
    id: u64,
    tx: mpsc::Sender<Option<bool>>,
    timer: Option<TimerId>,
}

#[derive(Default)]
struct LidState {
    running: bool,
    child: Option<(u64, Box<dyn ListenerChild>)>,
    generation: u64,
    restart_timer: Option<TimerId>,
    stale_timer: Option<TimerId>,
    state: Option<bool>,
    received: bool,
    last_sample_at: u64,
    buffer: String,
    waiters: Vec<Waiter>,
    next_waiter: u64,
}

struct LidInner {
    state: Mutex<LidState>,
    hooks: LidHooks,
    this: Weak<LidInner>,
}

/// Watches the clamshell state through the mic listener helper, which emits
/// a heartbeat every second; a silent helper is replaced.
#[derive(Clone)]
pub struct LaptopLidMonitor {
    inner: Arc<LidInner>,
}

impl LaptopLidMonitor {
    pub fn new(hooks: LidHooks) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| LidInner {
                state: Mutex::new(LidState::default()),
                hooks,
                this: this.clone(),
            }),
        }
    }

    /// The process-wide monitor used by [`resolve_preferred_microphone`].
    pub fn shared() -> LaptopLidMonitor {
        static SHARED: OnceLock<LaptopLidMonitor> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                LaptopLidMonitor::new(LidHooks {
                    spawner: Arc::new(ProcessSpawner),
                    resolve_binary: Arc::new(|| resolve_helper(MIC_LISTENER_BINARY)),
                    timers: RealTimers::shared(),
                    on_change: Arc::new(|_| {}),
                })
            })
            .clone()
    }

    #[cfg(test)]
    pub fn start(&self) {
        let mut state = lock(&self.inner.state);
        if state.running {
            return;
        }
        state.running = true;
        self.inner.launch(&mut state);
    }

    /// The current lid state: answered at once from a fresh sample, otherwise
    /// after the next sample or a 1.5 s bound (then `None`).
    pub fn request_state(&self) -> mpsc::Receiver<Option<bool>> {
        let (tx, rx) = mpsc::channel();
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        if !state.running {
            state.running = true;
            inner.launch(&mut state);
        }
        let now = inner.hooks.timers.now_ms();
        if state.received && now.saturating_sub(state.last_sample_at) < LID_STALE_STATE_MS {
            let _ = tx.send(state.state);
            return rx;
        }
        // Timers can be delayed by sleep; never answer with a pre-sleep value.
        if state.received {
            let generation = state.child.as_ref().map(|(g, _)| *g);
            if let Some(generation) = generation {
                inner.retire(
                    &mut state,
                    generation,
                    Some("Cached lid state expired"),
                    true,
                );
            }
        }
        state.next_waiter += 1;
        let id = state.next_waiter;
        let weak = inner.this.clone();
        let timer = inner.hooks.timers.schedule(
            Duration::from_millis(LID_INITIAL_WAIT_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    if let Some(index) = state.waiters.iter().position(|w| w.id == id) {
                        let waiter = state.waiters.remove(index);
                        let _ = waiter.tx.send(None);
                    }
                }
            }),
        );
        state.waiters.push(Waiter {
            id,
            tx,
            timer: Some(timer),
        });
        rx
    }

    /// Blocking form of [`LaptopLidMonitor::request_state`].
    pub fn state(&self) -> Option<bool> {
        self.request_state()
            .recv_timeout(Duration::from_millis(LID_INITIAL_WAIT_MS + 500))
            .ok()
            .flatten()
    }

    #[cfg(test)]
    pub fn stop(&self) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        state.running = false;
        if let Some(timer) = state.restart_timer.take() {
            inner.hooks.timers.cancel(timer);
        }
        let previous = state.child.take();
        inner.invalidate(&mut state);
        inner.finish_waiters(&mut state, None);
        drop(state);
        if let Some((_, mut child)) = previous {
            child.kill(libc::SIGTERM);
        }
    }
}

impl LidInner {
    fn update_state(&self, state: &mut LidState, value: Option<bool>) {
        if state.state != value {
            state.state = value;
            (self.hooks.on_change)(value);
        }
    }

    fn invalidate(&self, state: &mut LidState) {
        state.received = false;
        state.last_sample_at = 0;
        if let Some(timer) = state.stale_timer.take() {
            self.hooks.timers.cancel(timer);
        }
        self.update_state(state, None);
    }

    fn finish_waiters(&self, state: &mut LidState, value: Option<bool>) {
        for waiter in state.waiters.drain(..) {
            if let Some(timer) = waiter.timer {
                self.hooks.timers.cancel(timer);
            }
            let _ = waiter.tx.send(value);
        }
    }

    fn schedule_restart(&self, state: &mut LidState) {
        if !state.running || state.restart_timer.is_some() {
            return;
        }
        let weak = self.this.clone();
        state.restart_timer = Some(self.hooks.timers.schedule(
            Duration::from_millis(LID_RESTART_DELAY_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.restart_timer = None;
                    inner.launch(&mut state);
                }
            }),
        ));
    }

    fn retire(&self, state: &mut LidState, generation: u64, error: Option<&str>, kill: bool) {
        if !state.child.as_ref().is_some_and(|(g, _)| *g == generation) {
            return;
        }
        let child = state.child.take();
        self.invalidate(state);
        self.finish_waiters(state, None);
        if let Some(error) = error {
            log::warn!("[lid] laptop lid monitor unavailable: {error}");
        }
        if let (true, Some((_, mut child))) = (kill, child) {
            child.kill(libc::SIGTERM);
        }
        self.schedule_restart(state);
    }

    fn arm_stale_timer(&self, state: &mut LidState, generation: u64) {
        if let Some(timer) = state.stale_timer.take() {
            self.hooks.timers.cancel(timer);
        }
        let weak = self.this.clone();
        state.stale_timer = Some(self.hooks.timers.schedule(
            Duration::from_millis(LID_STALE_STATE_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.stale_timer = None;
                    inner.retire(
                        &mut state,
                        generation,
                        Some("Lid state heartbeat timed out"),
                        true,
                    );
                }
            }),
        ));
    }

    fn launch(&self, state: &mut LidState) {
        if !state.running || state.child.is_some() {
            return;
        }
        let Some(binary) = (self.hooks.resolve_binary)() else {
            self.schedule_restart(state);
            return;
        };
        state.generation += 1;
        let generation = state.generation;
        let weak = self.this.clone();
        let sink: ChildSink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_child_event(generation, event);
            }
        });
        match self
            .hooks
            .spawner
            .spawn(&binary, &["--watch-lid-state".to_string()], sink)
        {
            Ok(child) => {
                state.child = Some((generation, child));
                state.buffer.clear();
                self.arm_stale_timer(state, generation);
            }
            Err(error) => {
                log::warn!("[lid] cannot start the lid monitor: {error}");
                self.schedule_restart(state);
            }
        }
    }

    fn on_child_event(&self, generation: u64, event: ChildEvent) {
        let mut state = lock(&self.state);
        let current = state.child.as_ref().is_some_and(|(g, _)| *g == generation);
        if !current || !state.running {
            return;
        }
        match event {
            ChildEvent::Stdout(chunk) => {
                state.buffer.push_str(&chunk);
                if state.buffer.len() > LID_MAX_BUFFER {
                    self.retire(
                        &mut state,
                        generation,
                        Some("Invalid lid state output"),
                        true,
                    );
                    return;
                }
                while let Some(newline) = state.buffer.find('\n') {
                    let line: String = state.buffer.drain(..=newline).collect();
                    let Some(value) = parse_lid_state(line.trim()) else {
                        continue;
                    };
                    state.last_sample_at = self.hooks.timers.now_ms();
                    state.received = true;
                    self.update_state(&mut state, value);
                    self.finish_waiters(&mut state, value);
                    self.arm_stale_timer(&mut state, generation);
                }
            }
            ChildEvent::Stderr(_) => {}
            ChildEvent::Error(error) => self.retire(&mut state, generation, Some(&error), true),
            ChildEvent::Exit { .. } => self.retire(&mut state, generation, None, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::hotkeys::globe::tests::FakeSpawner;
    use crate::native::timer::FakeTimers;
    use std::path::PathBuf;

    struct Lid {
        monitor: LaptopLidMonitor,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
        broadcasts: Arc<Mutex<Vec<Option<bool>>>>,
    }

    fn lid(binary: Option<&'static str>) -> (Lid, Arc<std::sync::atomic::AtomicUsize>) {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::starting_at(100);
        let broadcasts = Arc::new(Mutex::new(Vec::new()));
        let lookups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (b, l) = (broadcasts.clone(), lookups.clone());
        let monitor = LaptopLidMonitor::new(LidHooks {
            spawner: spawner.clone(),
            resolve_binary: Arc::new(move || {
                l.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                binary.map(PathBuf::from)
            }),
            timers: timers.clone(),
            on_change: Arc::new(move |v| lock(&b).push(v)),
        });
        (
            Lid {
                monitor,
                spawner,
                timers,
                broadcasts,
            },
            lookups,
        )
    }

    fn helper() -> (Lid, Arc<std::sync::atomic::AtomicUsize>) {
        lid(Some("/helper/macos-mic-listener"))
    }

    impl Lid {
        fn out(&self, index: usize, text: &str) {
            self.spawner.emit(index, ChildEvent::Stdout(text.into()));
        }
        fn broadcasts(&self) -> Vec<Option<bool>> {
            lock(&self.broadcasts).clone()
        }
        fn kills(&self, index: usize) -> usize {
            lock(&self.spawner.child(index)).kills.len()
        }
    }

    #[test]
    fn parses_only_explicit_states() {
        assert_eq!(parse_lid_state("{\"lidClosed\":true}"), Some(Some(true)));
        assert_eq!(parse_lid_state("{\"lidClosed\":false}"), Some(Some(false)));
        assert_eq!(parse_lid_state("{\"lidClosed\":null}"), Some(None));
        for value in [
            "{\"lidClosed\":0}",
            "{\"lidClosed\":\"true\"}",
            "{}",
            "null",
            "bad",
        ] {
            assert_eq!(parse_lid_state(value), None, "{value}");
        }
    }

    #[test]
    fn concurrent_lookups_share_a_helper_and_parse_partial_lines() {
        let (h, _) = helper();
        let first = h.monitor.request_state();
        let second = h.monitor.request_state();
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(h.spawner.args(0), vec!["--watch-lid-state".to_string()]);
        h.out(0, "{\"lidClo");
        h.out(0, "sed\":false}\n{\"lidClosed\":false}\n");
        assert_eq!(first.try_recv().unwrap(), Some(false));
        assert_eq!(second.try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(false)]);
        h.out(0, "{\"lidClosed\":true}\n{\"lidClosed\":null}\n");
        assert_eq!(h.broadcasts(), vec![Some(false), Some(true), None]);
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), None);
        h.monitor.stop();
    }

    #[test]
    fn initial_lookup_is_bounded() {
        let (h, _) = helper();
        let result = h.monitor.request_state();
        h.out(0, "CAPABILITY PID\n{\"lidClosed\":1}\n");
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.monitor.stop();
    }

    #[test]
    fn helper_failure_clears_state_and_restarts_once() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(true));
        h.spawner
            .emit(0, ChildEvent::Error("process failed".into()));
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(1),
                signal: None,
            },
        );
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.out(0, "{\"lidClosed\":true}\n");
        h.out(1, "{\"lidClosed\":false}\n");
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(true), None, Some(false)]);
        h.monitor.stop();
    }

    #[test]
    fn heartbeats_refresh_without_duplicate_broadcasts() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(false)]);
        h.timers.advance(1000);
        assert_eq!(h.broadcasts(), vec![Some(false), None]);
        assert_eq!(h.kills(0), 1);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }

    #[test]
    fn delayed_timer_after_sleep_cannot_expose_stale_state() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        h.timers.advance_without_running(10_000);
        let result = h.monitor.request_state();
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        assert_eq!(h.kills(0), 1);
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.timers.advance(500);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }

    #[test]
    fn stop_resolves_pending_reads_and_cancels_timers() {
        let (h, _) = helper();
        let result = h.monitor.request_state();
        h.monitor.stop();
        assert_eq!(result.try_recv().unwrap(), None);
        assert_eq!(h.kills(0), 1);
        assert_eq!(h.timers.pending(), 0);
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        h.timers.advance(10_000);
        assert_eq!(h.spawner.count(), 1);
    }

    #[test]
    fn missing_helper_is_retried_without_blocking() {
        let (h, lookups) = lid(None);
        let result = h.monitor.request_state();
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.timers.advance(500);
        assert_eq!(lookups.load(std::sync::atomic::Ordering::SeqCst), 2);
        h.monitor.stop();
        assert_eq!(h.timers.pending(), 0);
    }

    #[test]
    fn excessive_output_retires_the_helper() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        h.out(0, &"x".repeat(4097));
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        assert_eq!(h.kills(0), 1);
        h.monitor.stop();
    }
}

//! Supervises `macos-globe-listener`, the helper that reports Globe/Fn,
//! right-side modifiers, modifier releases and side mouse buttons, and that
//! holds the macOS Globe action at "Do Nothing" while the app owns the key.
//! Covers the spawn/restart policy, stdin JSON reconfiguration and recovery
//! of a preference left behind by a crash.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::Serialize;

use super::super::helper::{arch_mismatch, resolve_helper, signal_pid};
use super::super::timer::{TimerId, Timers, lock};
use super::slots::ListenerConfig;

/// Rapid automatic restarts before giving up.
const MAX_RESTART_ATTEMPTS: u32 = 3;
const RESTART_DELAY_MS: u64 = 1000;
/// Sustained uptime after which the restart counter resets (sleep/wake).
const RESTART_RESET_MS: u64 = 10_000;
const PREFERENCE_RECOVERY_TIMEOUT: Duration = Duration::from_millis(2000);
const PREFERENCE_RECOVERY_TERMINATION_TIMEOUT: Duration = Duration::from_millis(1000);

pub const LISTENER_BINARY: &str = "macos-globe-listener";

/// What the listener reports, one stdout line each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobeEvent {
    GlobeDown,
    GlobeUp,
    /// Another key or click during a hold: the held key was a modifier.
    Interrupted,
    RightModifierDown(String),
    RightModifierUp(String),
    /// "command", "control", "option" or "shift" went up.
    ModifierUp(String),
    MouseButtonDown(String),
    MouseButtonUp(String),
    /// The listener went away; keys it saw held will never report release.
    ListenerStopped,
    Error(String),
}

/// Output and lifecycle of a spawned helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildEvent {
    Stdout(String),
    Stderr(String),
    Exit {
        code: Option<i32>,
        signal: Option<i32>,
    },
    Error(String),
}

pub type ChildSink = Arc<dyn Fn(ChildEvent) + Send + Sync>;

pub trait ListenerChild: Send {
    /// Writes one line to stdin; false when stdin is no longer writable.
    fn write_line(&mut self, line: &str) -> bool;
    fn kill(&mut self, signal: i32);
}

pub trait ListenerSpawner: Send + Sync {
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        sink: ChildSink,
    ) -> std::io::Result<Box<dyn ListenerChild>>;
}

/// Spawns real processes; output and exit arrive on reader threads.
pub struct ProcessSpawner;

struct ProcessChild {
    pid: u32,
    stdin: Option<ChildStdin>,
}

impl ListenerChild for ProcessChild {
    fn write_line(&mut self, line: &str) -> bool {
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        let ok = stdin
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|_| stdin.flush())
            .is_ok();
        if !ok {
            self.stdin = None;
        }
        ok
    }

    fn kill(&mut self, signal: i32) {
        signal_pid(self.pid, signal);
    }
}

fn forward_lines<R: Read + Send + 'static>(
    reader: R,
    sink: ChildSink,
    wrap: fn(String) -> ChildEvent,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => sink(wrap(format!("{line}\n"))),
                Err(_) => break,
            }
        }
    })
}

impl ListenerSpawner for ProcessSpawner {
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        sink: ChildSink,
    ) -> std::io::Result<Box<dyn ListenerChild>> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .map(|out| forward_lines(out, Arc::clone(&sink), ChildEvent::Stdout));
        if let Some(err) = child.stderr.take() {
            forward_lines(err, Arc::clone(&sink), ChildEvent::Stderr);
        }
        std::thread::spawn(move || {
            let status = child.wait();
            // Deliver remaining output before the exit, like a "close".
            if let Some(reader) = stdout {
                let _ = reader.join();
            }
            match status {
                Ok(status) => {
                    use std::os::unix::process::ExitStatusExt;
                    sink(ChildEvent::Exit {
                        code: status.code(),
                        signal: status.signal(),
                    });
                }
                Err(error) => sink(ChildEvent::Error(error.to_string())),
            }
        });
        Ok(Box::new(ProcessChild { pid, stdin }))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireConfig<'a> {
    mouse_buttons: &'a [String],
    suppress_globe_action: bool,
}

fn wire_line(config: &ListenerConfig) -> String {
    serde_json::to_string(&WireConfig {
        mouse_buttons: &config.mouse_buttons,
        suppress_globe_action: config.suppress_globe_action,
    })
    .unwrap_or_default()
}

pub type BinaryResolver = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

pub struct GlobeOptions {
    pub preference_state_path: Option<PathBuf>,
    pub spawner: Arc<dyn ListenerSpawner>,
    pub resolve_binary: BinaryResolver,
    pub timers: Arc<dyn Timers>,
    pub recovery_timeout: Duration,
    pub recovery_termination_timeout: Duration,
}

impl GlobeOptions {
    pub fn production(preference_state_path: Option<PathBuf>, timers: Arc<dyn Timers>) -> Self {
        Self {
            preference_state_path,
            spawner: Arc::new(ProcessSpawner),
            resolve_binary: Arc::new(|| resolve_helper(LISTENER_BINARY)),
            timers,
            recovery_timeout: PREFERENCE_RECOVERY_TIMEOUT,
            recovery_termination_timeout: PREFERENCE_RECOVERY_TERMINATION_TIMEOUT,
        }
    }
}

#[derive(Default)]
struct State {
    child: Option<(u64, Box<dyn ListenerChild>)>,
    generation: u64,
    stdout_buffer: String,
    has_reported_error: bool,
    is_stopping: bool,
    restart_count: u32,
    restart_reset_timer: Option<TimerId>,
    recovery_blocked: bool,
    config: ListenerConfig,
}

struct Inner {
    state: Mutex<State>,
    options: GlobeOptions,
    sink: Arc<dyn Fn(GlobeEvent) + Send + Sync>,
    this: Weak<Inner>,
}

#[derive(Clone)]
pub struct GlobeListener {
    inner: Arc<Inner>,
}

impl GlobeListener {
    pub fn new(options: GlobeOptions, sink: Arc<dyn Fn(GlobeEvent) + Send + Sync>) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                state: Mutex::new(State::default()),
                options,
                sink,
                this: this.clone(),
            }),
        }
    }

    pub fn is_running(&self) -> bool {
        lock(&self.inner.state).child.is_some()
    }

    pub fn start(&self) {
        let events = {
            let mut state = lock(&self.inner.state);
            self.inner.start_locked(&mut state)
        };
        self.inner.dispatch(events);
    }

    pub fn stop(&self) {
        let events = {
            let mut state = lock(&self.inner.state);
            self.inner.stop_locked(&mut state)
        };
        self.inner.dispatch(events);
    }

    /// Replaces the listener's whole state. A running listener is reconfigured
    /// over stdin rather than restarted: a restart drops events and briefly
    /// hands the Globe key back to macOS.
    pub fn set_configuration(&self, config: ListenerConfig) {
        let mut buttons: Vec<String> = config
            .mouse_buttons
            .into_iter()
            .filter(|b| b == "MouseButton4" || b == "MouseButton5")
            .collect();
        buttons.sort();
        buttons.dedup();
        let next = ListenerConfig {
            mouse_buttons: buttons,
            suppress_globe_action: config.suppress_globe_action,
        };
        let events = {
            let mut state = lock(&self.inner.state);
            if state.config == next {
                return;
            }
            state.config = next;
            let line = wire_line(&state.config);
            let sent = match state.child.as_mut() {
                None => return,
                Some((_, child)) => child.write_line(&line),
            };
            if sent {
                Vec::new()
            } else {
                let mut events = self.inner.stop_locked(&mut state);
                events.extend(self.inner.start_locked(&mut state));
                events
            }
        };
        self.inner.dispatch(events);
    }

    /// Restores a Globe preference marker left by a crashed run, using the
    /// helper's one-shot mode. Blocks for at most the recovery timeouts; a
    /// helper that cannot be terminated blocks the long-lived listener.
    pub fn restore_leftover_system_preference(&self) {
        let options = &self.inner.options;
        let Some(state_path) = options.preference_state_path.clone() else {
            return;
        };
        if !state_path.exists() {
            return;
        }
        let Some(binary) = (options.resolve_binary)() else {
            log::warn!("[globe] preference recovery skipped: binary not found");
            return;
        };
        if let Some(mismatch) = arch_mismatch(&binary, "Globe listener") {
            log::warn!("[globe] preference recovery skipped: {mismatch}");
            return;
        }

        let (tx, rx) = mpsc::channel::<ChildEvent>();
        let tx = Mutex::new(tx);
        let sink: ChildSink = Arc::new(move |event| {
            if matches!(event, ChildEvent::Exit { .. } | ChildEvent::Error(_)) {
                let _ = lock(&tx).send(event);
            }
        });
        let args = vec![
            "--globe-preference-state".to_string(),
            state_path.display().to_string(),
            "--restore-leftover-globe-preference".to_string(),
        ];
        let mut child = match options.spawner.spawn(&binary, &args, sink) {
            Ok(child) => child,
            Err(error) => {
                log::warn!("[globe] preference recovery failed: {error}");
                return;
            }
        };
        match rx.recv_timeout(options.recovery_timeout) {
            Ok(ChildEvent::Exit { code, .. }) => {
                if code != Some(0) {
                    log::warn!("[globe] preference recovery exited with {code:?}");
                }
            }
            Ok(other) => log::warn!("[globe] preference recovery failed: {other:?}"),
            Err(_) => {
                child.kill(libc::SIGKILL);
                match rx.recv_timeout(options.recovery_termination_timeout) {
                    Ok(ChildEvent::Exit { .. }) => {
                        log::warn!(
                            "[globe] preference recovery helper was terminated after timing out"
                        );
                    }
                    _ => {
                        log::warn!("[globe] preference recovery helper did not exit after SIGKILL");
                        lock(&self.inner.state).recovery_blocked = true;
                    }
                }
            }
        }
    }
}

impl Inner {
    fn dispatch(&self, events: Vec<GlobeEvent>) {
        for event in events {
            (self.sink)(event);
        }
    }

    fn listener_args(&self, state: &State) -> Vec<String> {
        let mut args = Vec::new();
        if !state.config.mouse_buttons.is_empty() {
            args.push(state.config.mouse_buttons.join(","));
        }
        if let Some(path) = &self.options.preference_state_path {
            args.push("--globe-preference-state".into());
            args.push(path.display().to_string());
        }
        if state.config.suppress_globe_action {
            args.push("--suppress-system-globe-action".into());
        }
        args
    }

    fn start_locked(&self, state: &mut State) -> Vec<GlobeEvent> {
        if state.recovery_blocked {
            return self.report_error(
                state,
                "Globe listener blocked because preference recovery could not be terminated".into(),
            );
        }
        if state.child.is_some() {
            return Vec::new();
        }
        state.is_stopping = false;

        let Some(binary) = (self.options.resolve_binary)() else {
            return self.report_error(
                state,
                "macOS Globe listener binary not found. Build the helpers (cargo build) before packaging."
                    .into(),
            );
        };
        if let Some(mismatch) = arch_mismatch(&binary, "Globe listener") {
            return self.report_error(state, mismatch);
        }

        state.has_reported_error = false;
        state.generation += 1;
        let generation = state.generation;
        let weak = self.this.clone();
        let sink: ChildSink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_child_event(generation, event);
            }
        });
        let args = self.listener_args(state);
        match self.options.spawner.spawn(&binary, &args, sink) {
            Ok(child) => {
                log::info!("[globe] listener spawned with {args:?}");
                state.child = Some((generation, child));
                state.stdout_buffer.clear();
            }
            Err(error) => return self.report_error(state, error.to_string()),
        }

        if let Some(timer) = state.restart_reset_timer.take() {
            self.options.timers.cancel(timer);
        }
        let weak = self.this.clone();
        state.restart_reset_timer = Some(self.options.timers.schedule(
            Duration::from_millis(RESTART_RESET_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.restart_reset_timer = None;
                    if state.restart_count > 0 {
                        log::info!("[globe] sustained uptime, restart counter reset");
                        state.restart_count = 0;
                    }
                }
            }),
        ));
        Vec::new()
    }

    fn stop_locked(&self, state: &mut State) -> Vec<GlobeEvent> {
        state.is_stopping = true;
        if let Some(timer) = state.restart_reset_timer.take() {
            self.options.timers.cancel(timer);
        }
        match state.child.take() {
            Some((_, mut child)) => {
                child.kill(libc::SIGTERM);
                vec![GlobeEvent::ListenerStopped]
            }
            None => Vec::new(),
        }
    }

    // Keys held when the listener goes away never report their release.
    fn drop_process(&self, state: &mut State) -> Vec<GlobeEvent> {
        match state.child.take() {
            Some(_) => vec![GlobeEvent::ListenerStopped],
            None => Vec::new(),
        }
    }

    fn report_error(&self, state: &mut State, message: String) -> Vec<GlobeEvent> {
        if state.has_reported_error {
            return Vec::new();
        }
        state.has_reported_error = true;
        let mut events = Vec::new();
        if let Some((_, child)) = state.child.as_mut() {
            child.kill(libc::SIGTERM);
            events.extend(self.drop_process(state));
        }
        log::error!("[globe] {message}");
        events.push(GlobeEvent::Error(message));
        events
    }

    fn is_current(state: &State, generation: u64) -> bool {
        state.child.as_ref().is_some_and(|(g, _)| *g == generation)
    }

    fn on_child_event(&self, generation: u64, event: ChildEvent) {
        let events = {
            let mut state = lock(&self.state);
            match event {
                ChildEvent::Stdout(chunk) => {
                    if !Self::is_current(&state, generation) {
                        return;
                    }
                    state.stdout_buffer.push_str(&chunk);
                    let mut events = Vec::new();
                    while let Some(newline) = state.stdout_buffer.find('\n') {
                        let line: String = state.stdout_buffer.drain(..=newline).collect();
                        if let Some(event) = parse_line(line.trim()) {
                            events.push(event);
                        }
                    }
                    events
                }
                ChildEvent::Stderr(text) => {
                    let message = text.trim().to_string();
                    if message.is_empty() || !Self::is_current(&state, generation) {
                        return;
                    }
                    if message.contains("Failed to create event monitor") {
                        self.report_error(&mut state, message)
                    } else {
                        log::warn!("[globe] listener stderr: {message}");
                        Vec::new()
                    }
                }
                ChildEvent::Error(message) => {
                    if !Self::is_current(&state, generation) {
                        return;
                    }
                    let mut events = self.report_error(&mut state, message);
                    events.extend(self.drop_process(&mut state));
                    events
                }
                ChildEvent::Exit { code, signal } => {
                    self.on_exit(&mut state, generation, code, signal)
                }
            }
        };
        self.dispatch(events);
    }

    fn on_exit(
        &self,
        state: &mut State,
        generation: u64,
        code: Option<i32>,
        signal: Option<i32>,
    ) -> Vec<GlobeEvent> {
        log::info!("[globe] listener exited: code {code:?} signal {signal:?}");
        // A replaced listener's late exit must not touch the current one.
        if !Self::is_current(state, generation) {
            return Vec::new();
        }
        let mut events = self.drop_process(state);
        if let Some(timer) = state.restart_reset_timer.take() {
            self.options.timers.cancel(timer);
        }
        if state.is_stopping || matches!(signal, Some(libc::SIGINT | libc::SIGTERM)) {
            return events;
        }
        if code != Some(0) {
            let describe = |v: Option<i32>| v.map_or("null".to_string(), |v| v.to_string());
            events.extend(self.report_error(
                state,
                format!(
                    "Globe key listener exited with code {} signal {}",
                    describe(code),
                    describe(signal)
                ),
            ));
            return events;
        }
        // Exit 0 without a stop request: the event tap was likely invalidated
        // (sleep/wake). Restart a few times.
        if state.restart_count < MAX_RESTART_ATTEMPTS {
            state.restart_count += 1;
            log::warn!(
                "[globe] unexpected exit, restarting (attempt {}/{MAX_RESTART_ATTEMPTS})",
                state.restart_count
            );
            let weak = self.this.clone();
            self.options.timers.schedule(
                Duration::from_millis(RESTART_DELAY_MS),
                Box::new(move || {
                    if let Some(inner) = weak.upgrade() {
                        let events = {
                            let mut state = lock(&inner.state);
                            if state.is_stopping || state.child.is_some() {
                                return;
                            }
                            inner.start_locked(&mut state)
                        };
                        inner.dispatch(events);
                    }
                }),
            );
        } else {
            events.extend(self.report_error(
                state,
                format!(
                    "Globe key listener keeps exiting unexpectedly ({MAX_RESTART_ATTEMPTS} restarts failed). Try restarting Whisper."
                ),
            ));
        }
        events
    }
}

fn parse_line(line: &str) -> Option<GlobeEvent> {
    let value = |prefix: &str| {
        line.strip_prefix(prefix)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    match line {
        "FN_DOWN" => Some(GlobeEvent::GlobeDown),
        "FN_UP" => Some(GlobeEvent::GlobeUp),
        "HOTKEY_INTERRUPTED" => Some(GlobeEvent::Interrupted),
        _ => value("RIGHT_MOD_DOWN:")
            .map(GlobeEvent::RightModifierDown)
            .or_else(|| value("RIGHT_MOD_UP:").map(GlobeEvent::RightModifierUp))
            .or_else(|| value("MODIFIER_UP:").map(|m| GlobeEvent::ModifierUp(m.to_lowercase())))
            .or_else(|| value("MOUSE_BUTTON_DOWN:").map(GlobeEvent::MouseButtonDown))
            .or_else(|| value("MOUSE_BUTTON_UP:").map(GlobeEvent::MouseButtonUp)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::native::timer::FakeTimers;

    #[derive(Default)]
    pub struct FakeChildState {
        pub writes: Vec<String>,
        pub writable: bool,
        pub kills: Vec<i32>,
    }

    pub struct SpawnCall {
        pub program: PathBuf,
        pub args: Vec<String>,
        pub sink: ChildSink,
        pub child: Arc<Mutex<FakeChildState>>,
    }

    #[derive(Default)]
    pub struct FakeSpawner {
        pub calls: Mutex<Vec<SpawnCall>>,
    }

    struct FakeChild(Arc<Mutex<FakeChildState>>);

    impl ListenerChild for FakeChild {
        fn write_line(&mut self, line: &str) -> bool {
            let mut state = lock(&self.0);
            if !state.writable {
                return false;
            }
            state.writes.push(format!("{line}\n"));
            true
        }
        fn kill(&mut self, signal: i32) {
            lock(&self.0).kills.push(signal);
        }
    }

    impl ListenerSpawner for FakeSpawner {
        fn spawn(
            &self,
            program: &Path,
            args: &[String],
            sink: ChildSink,
        ) -> std::io::Result<Box<dyn ListenerChild>> {
            let child = Arc::new(Mutex::new(FakeChildState {
                writable: true,
                ..Default::default()
            }));
            lock(&self.calls).push(SpawnCall {
                program: program.to_path_buf(),
                args: args.to_vec(),
                sink,
                child: Arc::clone(&child),
            });
            Ok(Box::new(FakeChild(child)))
        }
    }

    impl FakeSpawner {
        pub fn count(&self) -> usize {
            lock(&self.calls).len()
        }
        pub fn args(&self, index: usize) -> Vec<String> {
            lock(&self.calls)[index].args.clone()
        }
        pub fn emit(&self, index: usize, event: ChildEvent) {
            let sink = Arc::clone(&lock(&self.calls)[index].sink);
            sink(event);
        }
        pub fn child(&self, index: usize) -> Arc<Mutex<FakeChildState>> {
            Arc::clone(&lock(&self.calls)[index].child)
        }
    }

    pub const LISTENER_PATH: &str = "/fake/resources/bin/macos-globe-listener";

    pub struct Harness {
        pub listener: GlobeListener,
        pub spawner: Arc<FakeSpawner>,
        pub timers: Arc<FakeTimers>,
        pub events: Arc<Mutex<Vec<GlobeEvent>>>,
        _dir: tempfile::TempDir,
        pub state_path: PathBuf,
    }

    pub fn harness(with_state: bool, marker_exists: bool) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("globe-preference-state.json");
        if marker_exists {
            std::fs::write(&state_path, "{}").unwrap();
        }
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let listener = GlobeListener::new(
            GlobeOptions {
                preference_state_path: with_state.then(|| state_path.clone()),
                spawner: spawner.clone(),
                // A missing file skips the best-effort architecture check.
                resolve_binary: Arc::new(|| Some(PathBuf::from(LISTENER_PATH))),
                timers: timers.clone(),
                recovery_timeout: Duration::from_millis(30),
                recovery_termination_timeout: Duration::from_millis(30),
            },
            Arc::new(move |event| lock(&sink_events).push(event)),
        );
        Harness {
            listener,
            spawner,
            timers,
            events,
            _dir: dir,
            state_path,
        }
    }

    fn state_arg(h: &Harness) -> String {
        h.state_path.display().to_string()
    }

    #[test]
    fn spawn_args_carry_state_path_and_omit_suppression_by_default() {
        let h = harness(true, true);
        h.listener.start();
        h.listener.stop();
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(
            lock(&h.spawner.calls)[0].program,
            PathBuf::from(LISTENER_PATH)
        );
        assert_eq!(
            h.spawner.args(0),
            vec!["--globe-preference-state".to_string(), state_arg(&h)]
        );
    }

    #[test]
    fn leftover_recovery_uses_one_shot_helper() {
        let h = harness(true, true);
        let listener = h.listener.clone();
        let worker = std::thread::spawn(move || listener.restore_leftover_system_preference());
        while h.spawner.count() == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            h.spawner.args(0),
            vec![
                "--globe-preference-state".to_string(),
                state_arg(&h),
                "--restore-leftover-globe-preference".to_string()
            ]
        );
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        worker.join().unwrap();
        assert!(!h.listener.is_running());
    }

    #[test]
    fn timed_out_recovery_kills_and_waits_then_allows_start() {
        let h = harness(true, true);
        let listener = h.listener.clone();
        let spawner = Arc::clone(&h.spawner);
        let worker = std::thread::spawn(move || listener.restore_leftover_system_preference());
        // Report the exit only after the SIGKILL.
        loop {
            if h.spawner.count() > 0 && !lock(&h.spawner.child(0)).kills.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(lock(&h.spawner.child(0)).kills, vec![libc::SIGKILL]);
        spawner.emit(
            0,
            ChildEvent::Exit {
                code: None,
                signal: Some(libc::SIGKILL),
            },
        );
        worker.join().unwrap();
        h.listener.start();
        assert_eq!(h.spawner.count(), 2);
        h.listener.stop();
    }

    #[test]
    fn unconfirmed_recovery_termination_blocks_listener() {
        let h = harness(true, true);
        h.listener.restore_leftover_system_preference();
        h.listener.start();
        assert_eq!(
            h.spawner.count(),
            1,
            "no second helper may race the wedged recovery"
        );
        let events = lock(&h.events).clone();
        assert!(
            matches!(&events[0], GlobeEvent::Error(m) if m.contains("preference recovery could not be terminated"))
        );
        assert!(!h.listener.is_running());
    }

    #[test]
    fn recovery_skipped_without_path_or_marker() {
        let h = harness(false, true);
        h.listener.restore_leftover_system_preference();
        assert_eq!(h.spawner.count(), 0);
        let h = harness(true, false);
        h.listener.restore_leftover_system_preference();
        assert_eq!(h.spawner.count(), 0);
    }

    #[test]
    fn spawn_args_include_buttons_spaced_path_and_suppression() {
        let h = harness(true, true);
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec![
                "MouseButton5".into(),
                "MouseButton4".into(),
                "MouseButton4".into(),
            ],
            suppress_globe_action: true,
        });
        h.listener.start();
        h.listener.stop();
        assert_eq!(
            h.spawner.args(0),
            vec![
                "MouseButton4,MouseButton5".to_string(),
                "--globe-preference-state".to_string(),
                state_arg(&h),
                "--suppress-system-globe-action".to_string()
            ]
        );
    }

    #[test]
    fn configuration_change_reconfigures_running_listener() {
        let h = harness(true, true);
        h.listener.start();
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec!["MouseButton4".into()],
            suppress_globe_action: true,
        });
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(
            lock(&h.spawner.child(0)).writes,
            vec!["{\"mouseButtons\":[\"MouseButton4\"],\"suppressGlobeAction\":true}\n"]
        );
        h.listener.stop();
    }

    #[test]
    fn unchanged_configuration_writes_nothing() {
        let h = harness(true, true);
        let config = ListenerConfig {
            mouse_buttons: vec!["MouseButton4".into()],
            suppress_globe_action: true,
        };
        h.listener.set_configuration(config.clone());
        h.listener.start();
        h.listener.set_configuration(config);
        assert!(lock(&h.spawner.child(0)).writes.is_empty());
        h.listener.stop();
    }

    #[test]
    fn successive_changes_each_send_full_state() {
        let h = harness(true, true);
        h.listener.start();
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec![],
            suppress_globe_action: true,
        });
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec!["MouseButton5".into()],
            suppress_globe_action: true,
        });
        h.listener.set_configuration(ListenerConfig::default());
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(
            lock(&h.spawner.child(0)).writes,
            vec![
                "{\"mouseButtons\":[],\"suppressGlobeAction\":true}\n",
                "{\"mouseButtons\":[\"MouseButton5\"],\"suppressGlobeAction\":true}\n",
                "{\"mouseButtons\":[],\"suppressGlobeAction\":false}\n",
            ]
        );
        h.listener.stop();
    }

    #[test]
    fn dead_stdin_falls_back_to_respawning() {
        let h = harness(true, true);
        h.listener.start();
        lock(&h.spawner.child(0)).writable = false;
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec![],
            suppress_globe_action: true,
        });
        assert_eq!(h.spawner.count(), 2);
        // The old listener must be terminated so it restores the preference.
        assert_eq!(lock(&h.spawner.child(0)).kills, vec![libc::SIGTERM]);
        assert_eq!(
            h.spawner.args(1),
            vec![
                "--globe-preference-state".to_string(),
                state_arg(&h),
                "--suppress-system-globe-action".to_string()
            ]
        );
        h.listener.stop();
    }

    #[test]
    fn restart_picks_up_latest_configuration() {
        let h = harness(true, true);
        h.listener.start();
        h.listener.stop();
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec!["MouseButton4".into()],
            suppress_globe_action: true,
        });
        h.listener.start();
        h.listener.stop();
        assert_eq!(
            h.spawner.args(1),
            vec![
                "MouseButton4".to_string(),
                "--globe-preference-state".to_string(),
                state_arg(&h),
                "--suppress-system-globe-action".to_string()
            ]
        );
    }

    #[test]
    fn no_state_path_means_no_state_argument() {
        let h = harness(false, false);
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec![],
            suppress_globe_action: true,
        });
        h.listener.start();
        h.listener.stop();
        assert_eq!(
            h.spawner.args(0),
            vec!["--suppress-system-globe-action".to_string()]
        );
    }

    #[test]
    fn listener_output_becomes_events() {
        let h = harness(false, false);
        h.listener.start();
        h.spawner.emit(
            0,
            ChildEvent::Stdout(
                "RIGHT_MOD_DOWN:RightCommand\nHOTKEY_INTERRUPTED\nRIGHT_MOD_".into(),
            ),
        );
        h.spawner.emit(
            0,
            ChildEvent::Stdout("UP:RightCommand\nMODIFIER_UP:Control\n".into()),
        );
        h.listener.stop();
        assert_eq!(
            lock(&h.events).clone(),
            vec![
                GlobeEvent::RightModifierDown("RightCommand".into()),
                GlobeEvent::Interrupted,
                GlobeEvent::RightModifierUp("RightCommand".into()),
                GlobeEvent::ModifierUp("control".into()),
                GlobeEvent::ListenerStopped,
            ]
        );
    }

    #[test]
    fn losing_listener_reported_once_but_replaced_late_exit_ignored() {
        let h = harness(false, false);
        let stopped = || {
            lock(&h.events)
                .iter()
                .filter(|e| **e == GlobeEvent::ListenerStopped)
                .count()
        };
        h.listener.start();
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        assert_eq!(stopped(), 1);

        h.listener.start();
        lock(&h.spawner.child(1)).writable = false;
        h.listener.set_configuration(ListenerConfig {
            mouse_buttons: vec![],
            suppress_globe_action: true,
        });
        assert_eq!(
            stopped(),
            2,
            "a respawn drops what the old listener saw held"
        );
        h.spawner.emit(
            1,
            ChildEvent::Exit {
                code: None,
                signal: Some(libc::SIGTERM),
            },
        );
        assert_eq!(stopped(), 2);
        h.listener.stop();
        assert_eq!(stopped(), 3);
    }

    #[test]
    fn unexpected_clean_exits_restart_until_the_limit() {
        let h = harness(false, false);
        h.listener.start();
        for attempt in 0..MAX_RESTART_ATTEMPTS as usize {
            h.spawner.emit(
                attempt,
                ChildEvent::Exit {
                    code: Some(0),
                    signal: None,
                },
            );
            h.timers.advance(RESTART_DELAY_MS);
            assert_eq!(h.spawner.count(), attempt + 2);
        }
        h.spawner.emit(
            MAX_RESTART_ATTEMPTS as usize,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        h.timers.advance(RESTART_DELAY_MS);
        assert_eq!(h.spawner.count(), MAX_RESTART_ATTEMPTS as usize + 1);
        assert!(
            lock(&h.events)
                .iter()
                .any(|e| matches!(e, GlobeEvent::Error(m) if m.contains("keeps exiting")))
        );
    }

    #[test]
    fn crash_reports_error_without_restart() {
        let h = harness(false, false);
        h.listener.start();
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(1),
                signal: None,
            },
        );
        h.timers.advance(5000);
        assert_eq!(h.spawner.count(), 1);
        assert!(
            lock(&h.events)
                .iter()
                .any(|e| matches!(e, GlobeEvent::Error(m) if m.contains("code 1")))
        );
    }
}

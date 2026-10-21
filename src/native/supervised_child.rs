//! Supervises the long-running helper processes: the Globe listener, the lid
//! monitor and post-paste edit monitoring.
//!
//! A [`SupervisedChild`] runs at most one child at a time. Every spawn starts
//! a new generation, so output or an exit from a replaced child is ignored.
//! Stdout is framed into lines of bounded length, an optional heartbeat
//! retires a silent child, and a [`RestartPolicy`] decides whether a child
//! that went away is started again.
//!
//! A helper supplies a [`Protocol`]: its arguments, what its lines mean and
//! what to do when the child goes away. Protocol callbacks run under the
//! supervisor's lock and must not call back into it; the events they emit are
//! delivered after the lock is released, so an event sink may.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::helper::{arch_mismatch, signal_pid};
use super::timer::{TimerId, Timers, lock};

/// Largest piece of output a reader thread delivers at once; a longer line
/// arrives in several pieces.
const READ_CHUNK: u64 = 64 * 1024;

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

pub type BinaryResolver = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

pub trait HelperChild: Send {
    /// Writes one line to stdin; false when stdin is no longer writable.
    fn write_line(&mut self, line: &str) -> bool;
    fn kill(&mut self, signal: i32);
}

/// Starts helper processes; the seam tests replace.
pub trait HelperSpawner: Send + Sync {
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        sink: ChildSink,
    ) -> std::io::Result<Box<dyn HelperChild>>;
}

/// Spawns real processes; output and exit arrive on reader threads.
pub struct ProcessSpawner;

struct ProcessChild {
    pid: u32,
    stdin: Option<ChildStdin>,
}

impl HelperChild for ProcessChild {
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

/// Forwards output a line (or [`READ_CHUNK`] bytes) at a time.
fn forward_output<R: Read + Send + 'static>(
    reader: R,
    sink: ChildSink,
    wrap: fn(String) -> ChildEvent,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut chunk = Vec::new();
        loop {
            chunk.clear();
            match (&mut reader).take(READ_CHUNK).read_until(b'\n', &mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(_) => sink(wrap(String::from_utf8_lossy(&chunk).into_owned())),
            }
        }
    })
}

impl HelperSpawner for ProcessSpawner {
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        sink: ChildSink,
    ) -> std::io::Result<Box<dyn HelperChild>> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        let stdin = child.stdin.take();
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|out| forward_output(out, Arc::clone(&sink), ChildEvent::Stdout)),
            child
                .stderr
                .take()
                .map(|err| forward_output(err, Arc::clone(&sink), ChildEvent::Stderr)),
        ]
        .into_iter()
        .flatten()
        .collect();
        std::thread::spawn(move || {
            let status = child.wait();
            // Deliver remaining output before the exit, like a "close".
            for reader in readers {
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

/// Why the current child went away, or why none could be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Down {
    /// [`SupervisedChild::stop`] terminated it.
    Stopped,
    /// The binary was not found.
    Missing,
    /// The binary has the wrong architecture or could not be spawned.
    NotStarted(String),
    /// It was terminated after an I/O error, an overlong line, a missed
    /// heartbeat or [`Control::fail`].
    Failed(String),
    /// It exited by itself.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
}

/// What the [`RestartPolicy`] decided after a [`Down`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// A restart is scheduled; `attempt` counts restarts since the restart
    /// budget was last reset.
    Restarting {
        attempt: u32,
    },
    /// The restart budget is spent.
    GaveUp,
    NotRestarting,
}

#[derive(Debug, Clone, Copy)]
pub enum RestartPolicy {
    Never,
    /// Restarts after a clean exit (status 0) only, at most `max_attempts`
    /// times until a child stays up for `reset_after`.
    CleanExit {
        delay: Duration,
        max_attempts: u32,
        reset_after: Duration,
    },
    /// Restarts after every end but [`Down::Stopped`], indefinitely.
    Always {
        delay: Duration,
    },
}

pub struct ChildOptions {
    /// Names the helper in messages, e.g. "Globe listener".
    pub label: &'static str,
    pub resolve_binary: BinaryResolver,
    pub spawner: Arc<dyn HelperSpawner>,
    pub timers: Arc<dyn Timers>,
    pub restart: RestartPolicy,
    /// Retires a child that goes this long without [`Control::heartbeat`].
    pub heartbeat: Option<Duration>,
    /// Longest accepted stdout line in bytes; longer output retires the child.
    pub max_line: usize,
}

/// A helper's side of the conversation. Callbacks run under the supervisor's
/// lock; they talk to the child and report events through [`Control`].
pub trait Protocol: Send + 'static {
    type Event: Send + 'static;

    /// Arguments for the next spawn.
    fn args(&self) -> Vec<String>;

    /// A child was spawned.
    fn on_start(&mut self, _cx: &mut Control<'_, Self::Event>) {}

    /// One stdout line, without its line ending.
    fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, Self::Event>);

    /// Stderr output, normally one line with its line ending.
    fn on_stderr(&mut self, _text: &str, _cx: &mut Control<'_, Self::Event>) {}

    /// The current child went away, or none could be started.
    fn on_down(&mut self, down: &Down, recovery: Recovery, cx: &mut Control<'_, Self::Event>);
}

/// The running child and the event queue, as seen from a [`Protocol`].
pub struct Control<'a, E> {
    child: Option<&'a mut Box<dyn HelperChild>>,
    events: &'a mut Vec<E>,
    heartbeat: bool,
    failure: Option<String>,
}

impl<E> Control<'_, E> {
    /// Queues an event for the sink.
    pub fn emit(&mut self, event: E) {
        self.events.push(event);
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Writes one line to the running child's stdin; false without a child
    /// or once its stdin is closed.
    pub fn write_line(&mut self, line: &str) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| child.write_line(line))
    }

    /// Pushes back the heartbeat deadline of the running child.
    pub fn heartbeat(&mut self) {
        self.heartbeat = true;
    }

    /// Terminates the running child as [`Down::Failed`] once the callback
    /// returns.
    pub fn fail(&mut self, reason: impl Into<String>) {
        self.failure.get_or_insert_with(|| reason.into());
    }
}

#[derive(Default)]
struct Core {
    /// Between `start` and `stop`.
    started: bool,
    child: Option<(u64, Box<dyn HelperChild>)>,
    generation: u64,
    stdout: String,
    restart_count: u32,
    restart_timer: Option<TimerId>,
    reset_timer: Option<TimerId>,
    heartbeat_timer: Option<TimerId>,
}

impl Core {
    fn is_current(&self, generation: u64) -> bool {
        self.child.as_ref().is_some_and(|(g, _)| *g == generation)
    }
}

struct State<P> {
    protocol: P,
    core: Core,
}

struct Inner<P: Protocol> {
    state: Mutex<State<P>>,
    options: ChildOptions,
    sink: Arc<dyn Fn(P::Event) + Send + Sync>,
    this: Weak<Inner<P>>,
}

/// One supervised helper; clones share it.
pub struct SupervisedChild<P: Protocol> {
    inner: Arc<Inner<P>>,
}

impl<P: Protocol> Clone for SupervisedChild<P> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<P: Protocol> SupervisedChild<P> {
    pub fn new(
        options: ChildOptions,
        protocol: P,
        sink: Arc<dyn Fn(P::Event) + Send + Sync>,
    ) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                state: Mutex::new(State {
                    protocol,
                    core: Core::default(),
                }),
                options,
                sink,
                this: this.clone(),
            }),
        }
    }

    /// Spawns a child unless one is running; a pending restart happens now.
    pub fn start(&self) {
        let inner = &self.inner;
        inner.run(|state, events| {
            state.core.started = true;
            if state.core.child.is_some() {
                return;
            }
            if let Some(timer) = state.core.restart_timer.take() {
                inner.options.timers.cancel(timer);
            }
            inner.launch(state, events);
        });
    }

    /// Terminates the child (SIGTERM) and cancels any pending restart.
    pub fn stop(&self) {
        let inner = &self.inner;
        inner.run(|state, events| {
            state.core.started = false;
            if let Some(timer) = state.core.restart_timer.take() {
                inner.options.timers.cancel(timer);
            }
            inner.retire(state, events, Down::Stopped, true);
        });
    }

    /// Runs `f` on the protocol state under the supervisor's lock.
    pub fn update<R>(&self, f: impl FnOnce(&mut P, &mut Control<'_, P::Event>) -> R) -> R {
        let inner = &self.inner;
        inner.run(|state, events| inner.call(state, events, f))
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        lock(&self.inner.state).core.child.is_some()
    }
}

impl<P: Protocol> Inner<P> {
    /// Runs `f` under the lock, then delivers the events it queued.
    fn run<R>(&self, f: impl FnOnce(&mut State<P>, &mut Vec<P::Event>) -> R) -> R {
        let mut events = Vec::new();
        let result = f(&mut lock(&self.state), &mut events);
        for event in events {
            (self.sink)(event);
        }
        result
    }

    /// Calls into the protocol, then applies what it asked of the child.
    fn call<R>(
        &self,
        state: &mut State<P>,
        events: &mut Vec<P::Event>,
        f: impl FnOnce(&mut P, &mut Control<'_, P::Event>) -> R,
    ) -> R {
        let State { protocol, core } = &mut *state;
        let mut cx = Control {
            child: core.child.as_mut().map(|(_, child)| child),
            events: &mut *events,
            heartbeat: false,
            failure: None,
        };
        let result = f(protocol, &mut cx);
        let Control {
            heartbeat, failure, ..
        } = cx;
        if heartbeat {
            self.arm_heartbeat(&mut state.core);
        }
        if let Some(reason) = failure {
            self.retire(state, events, Down::Failed(reason), true);
        }
        result
    }

    fn launch(&self, state: &mut State<P>, events: &mut Vec<P::Event>) {
        let Some(binary) = (self.options.resolve_binary)() else {
            return self.went_down(state, events, Down::Missing);
        };
        if let Some(mismatch) = arch_mismatch(&binary, self.options.label) {
            return self.went_down(state, events, Down::NotStarted(mismatch));
        }
        state.core.generation += 1;
        let generation = state.core.generation;
        let weak = self.this.clone();
        let sink: ChildSink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_child_event(generation, event);
            }
        });
        let args = state.protocol.args();
        let child = match self.options.spawner.spawn(&binary, &args, sink) {
            Ok(child) => child,
            Err(error) => {
                return self.went_down(state, events, Down::NotStarted(error.to_string()));
            }
        };
        let core = &mut state.core;
        core.child = Some((generation, child));
        core.stdout.clear();
        if let RestartPolicy::CleanExit { reset_after, .. } = self.options.restart {
            self.arm_reset(core, reset_after);
        }
        self.arm_heartbeat(core);
        self.call(state, events, |protocol, cx| protocol.on_start(cx));
    }

    /// Takes the running child, terminating it with SIGTERM if `kill`.
    fn retire(&self, state: &mut State<P>, events: &mut Vec<P::Event>, down: Down, kill: bool) {
        let Some((_, mut child)) = state.core.child.take() else {
            return;
        };
        if kill {
            child.kill(libc::SIGTERM);
        }
        self.went_down(state, events, down);
    }

    fn went_down(&self, state: &mut State<P>, events: &mut Vec<P::Event>, down: Down) {
        let core = &mut state.core;
        for timer in [core.heartbeat_timer.take(), core.reset_timer.take()]
            .into_iter()
            .flatten()
        {
            self.options.timers.cancel(timer);
        }
        core.stdout.clear();
        let recovery = self.recovery(core, &down);
        if let (Recovery::Restarting { .. }, Some(delay)) = (recovery, self.restart_delay()) {
            self.schedule_restart(core, delay);
        }
        self.call(state, events, |protocol, cx| {
            protocol.on_down(&down, recovery, cx)
        });
    }

    fn recovery(&self, core: &mut Core, down: &Down) -> Recovery {
        if !core.started || *down == Down::Stopped {
            return Recovery::NotRestarting;
        }
        match self.options.restart {
            RestartPolicy::Never => Recovery::NotRestarting,
            RestartPolicy::CleanExit { max_attempts, .. } => {
                if !matches!(down, Down::Exited { code: Some(0), .. }) {
                    Recovery::NotRestarting
                } else if core.restart_count < max_attempts {
                    core.restart_count += 1;
                    Recovery::Restarting {
                        attempt: core.restart_count,
                    }
                } else {
                    Recovery::GaveUp
                }
            }
            RestartPolicy::Always { .. } => {
                core.restart_count = core.restart_count.saturating_add(1);
                Recovery::Restarting {
                    attempt: core.restart_count,
                }
            }
        }
    }

    fn restart_delay(&self) -> Option<Duration> {
        match self.options.restart {
            RestartPolicy::Never => None,
            RestartPolicy::CleanExit { delay, .. } | RestartPolicy::Always { delay } => Some(delay),
        }
    }

    fn schedule_restart(&self, core: &mut Core, delay: Duration) {
        if let Some(timer) = core.restart_timer.take() {
            self.options.timers.cancel(timer);
        }
        let weak = self.this.clone();
        core.restart_timer = Some(self.options.timers.schedule(
            delay,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.run(|state, events| {
                        state.core.restart_timer = None;
                        if state.core.started && state.core.child.is_none() {
                            inner.launch(state, events);
                        }
                    });
                }
            }),
        ));
    }

    /// Sustained uptime resets the restart budget (sleep/wake can end a
    /// healthy helper now and then).
    fn arm_reset(&self, core: &mut Core, after: Duration) {
        if let Some(timer) = core.reset_timer.take() {
            self.options.timers.cancel(timer);
        }
        let weak = self.this.clone();
        let label = self.options.label;
        core.reset_timer = Some(self.options.timers.schedule(
            after,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.core.reset_timer = None;
                    if state.core.restart_count > 0 {
                        log::info!("[helper] {label}: sustained uptime, restart counter reset");
                        state.core.restart_count = 0;
                    }
                }
            }),
        ));
    }

    fn arm_heartbeat(&self, core: &mut Core) {
        let Some(timeout) = self.options.heartbeat else {
            return;
        };
        let Some(generation) = core.child.as_ref().map(|(g, _)| *g) else {
            return;
        };
        if let Some(timer) = core.heartbeat_timer.take() {
            self.options.timers.cancel(timer);
        }
        let weak = self.this.clone();
        core.heartbeat_timer = Some(self.options.timers.schedule(
            timeout,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.run(|state, events| {
                        if !state.core.is_current(generation) {
                            return;
                        }
                        state.core.heartbeat_timer = None;
                        let reason = "heartbeat timed out".to_string();
                        inner.retire(state, events, Down::Failed(reason), true);
                    });
                }
            }),
        ));
    }

    fn on_child_event(&self, generation: u64, event: ChildEvent) {
        self.run(|state, events| {
            // A replaced child's late output or exit must not touch the current one.
            if !state.core.is_current(generation) {
                return;
            }
            match event {
                ChildEvent::Stdout(chunk) => self.on_stdout(state, events, generation, &chunk),
                ChildEvent::Stderr(text) => {
                    self.call(state, events, |protocol, cx| protocol.on_stderr(&text, cx))
                }
                ChildEvent::Error(message) => {
                    self.retire(state, events, Down::Failed(message), true)
                }
                ChildEvent::Exit { code, signal } => {
                    self.retire(state, events, Down::Exited { code, signal }, false)
                }
            }
        });
    }

    fn on_stdout(
        &self,
        state: &mut State<P>,
        events: &mut Vec<P::Event>,
        generation: u64,
        chunk: &str,
    ) {
        let max_line = self.options.max_line;
        state.core.stdout.push_str(chunk);
        while let Some(newline) = state.core.stdout.find('\n') {
            if newline > max_line {
                break;
            }
            let mut line: String = state.core.stdout.drain(..=newline).collect();
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            self.call(state, events, |protocol, cx| protocol.on_stdout(&line, cx));
            if !state.core.is_current(generation) {
                return;
            }
        }
        let pending = state
            .core
            .stdout
            .find('\n')
            .unwrap_or(state.core.stdout.len());
        if pending > max_line {
            let reason = format!("output line longer than {max_line} bytes");
            self.retire(state, events, Down::Failed(reason), true);
        }
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

    impl HelperChild for FakeChild {
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

    impl HelperSpawner for FakeSpawner {
        fn spawn(
            &self,
            program: &Path,
            args: &[String],
            sink: ChildSink,
        ) -> std::io::Result<Box<dyn HelperChild>> {
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
        pub fn kills(&self, index: usize) -> Vec<i32> {
            lock(&self.child(index)).kills.clone()
        }
    }

    /// Records every callback as a string; "beat" lines are heartbeats and
    /// "fail" lines ask for the child to be terminated.
    struct Recorder;

    impl Protocol for Recorder {
        type Event = String;

        fn args(&self) -> Vec<String> {
            vec!["--watch".into()]
        }
        fn on_start(&mut self, cx: &mut Control<'_, String>) {
            cx.emit("start".into());
        }
        fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, String>) {
            match line {
                "beat" => cx.heartbeat(),
                "fail" => cx.fail("asked to fail"),
                _ => {}
            }
            cx.emit(format!("line {line}"));
        }
        fn on_stderr(&mut self, text: &str, cx: &mut Control<'_, String>) {
            cx.emit(format!("stderr {}", text.trim()));
        }
        fn on_down(&mut self, down: &Down, recovery: Recovery, cx: &mut Control<'_, String>) {
            cx.emit(format!("down {down:?} {recovery:?}"));
        }
    }

    struct Harness {
        child: SupervisedChild<Recorder>,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
        events: Arc<Mutex<Vec<String>>>,
    }

    fn harness(restart: RestartPolicy, heartbeat: Option<u64>) -> Harness {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let child = SupervisedChild::new(
            ChildOptions {
                label: "Test helper",
                // A missing file skips the best-effort architecture check.
                resolve_binary: Arc::new(|| Some(PathBuf::from("/fake/helper"))),
                spawner: spawner.clone(),
                timers: timers.clone(),
                restart,
                heartbeat: heartbeat.map(Duration::from_millis),
                max_line: 16,
            },
            Recorder,
            Arc::new(move |event| lock(&sink_events).push(event)),
        );
        Harness {
            child,
            spawner,
            timers,
            events,
        }
    }

    const ALWAYS: RestartPolicy = RestartPolicy::Always {
        delay: Duration::from_millis(2000),
    };

    impl Harness {
        fn out(&self, index: usize, text: &str) {
            self.spawner.emit(index, ChildEvent::Stdout(text.into()));
        }
        fn exit(&self, index: usize, code: i32) {
            self.spawner.emit(
                index,
                ChildEvent::Exit {
                    code: Some(code),
                    signal: None,
                },
            );
        }
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *lock(&self.events))
        }
    }

    #[test]
    fn stdout_is_framed_into_lines_across_chunks() {
        let h = harness(RestartPolicy::Never, None);
        h.child.start();
        assert_eq!(h.spawner.args(0), vec!["--watch".to_string()]);
        h.out(0, "fir");
        h.out(0, "st\r\nsecond\n\nthi");
        h.out(0, "rd\n");
        h.spawner.emit(0, ChildEvent::Stderr("warning\n".into()));
        assert_eq!(
            h.take(),
            vec![
                "start",
                "line first",
                "line second",
                "line ",
                "line third",
                "stderr warning"
            ]
        );
    }

    #[test]
    fn overlong_output_terminates_the_child() {
        let h = harness(ALWAYS, None);
        h.child.start();
        h.out(0, "ok\n");
        h.out(0, &"x".repeat(17));
        assert_eq!(h.spawner.kills(0), vec![libc::SIGTERM]);
        h.timers.advance(2000);
        h.out(1, &format!("{}\nok\n", "y".repeat(17)));
        assert_eq!(h.spawner.kills(1), vec![libc::SIGTERM]);
        let failed = "down Failed(\"output line longer than 16 bytes\")";
        assert_eq!(
            h.take(),
            vec![
                "start".to_string(),
                "line ok".into(),
                format!("{failed} Restarting {{ attempt: 1 }}"),
                "start".into(),
                format!("{failed} Restarting {{ attempt: 2 }}"),
            ]
        );
    }

    #[test]
    fn replaced_child_output_and_exit_are_ignored() {
        let h = harness(ALWAYS, None);
        h.child.start();
        h.out(0, "fail\n");
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.take();
        h.out(0, "late\n");
        h.exit(0, 0);
        h.spawner.emit(0, ChildEvent::Error("late failure".into()));
        assert!(h.take().is_empty());
        assert!(h.child.is_running());
        h.out(1, "current\n");
        assert_eq!(h.take(), vec!["line current"]);
    }

    #[test]
    fn stop_terminates_cancels_timers_and_ignores_the_late_exit() {
        let h = harness(
            RestartPolicy::CleanExit {
                delay: Duration::from_millis(1000),
                max_attempts: 3,
                reset_after: Duration::from_millis(10_000),
            },
            Some(5000),
        );
        h.child.start();
        h.child.stop();
        assert_eq!(h.spawner.kills(0), vec![libc::SIGTERM]);
        assert_eq!(h.timers.pending(), 0);
        h.exit(0, 0);
        h.timers.advance(20_000);
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(h.take(), vec!["start", "down Stopped NotRestarting"]);
    }

    #[test]
    fn missed_heartbeat_terminates_and_restarts() {
        let h = harness(ALWAYS, Some(5000));
        h.child.start();
        h.timers.advance(4000);
        h.out(0, "beat\n");
        h.timers.advance(4000);
        h.out(0, "noise\n");
        assert!(h.spawner.kills(0).is_empty());
        h.timers.advance(1000);
        assert_eq!(h.spawner.kills(0), vec![libc::SIGTERM]);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        assert!(h.take().contains(
            &"down Failed(\"heartbeat timed out\") Restarting { attempt: 1 }".to_string()
        ));
    }

    #[test]
    fn clean_exits_restart_within_a_budget_that_uptime_resets() {
        let h = harness(
            RestartPolicy::CleanExit {
                delay: Duration::from_millis(1000),
                max_attempts: 2,
                reset_after: Duration::from_millis(10_000),
            },
            None,
        );
        h.child.start();
        for index in 0..3 {
            h.exit(index, 0);
            h.timers.advance(1000);
        }
        assert_eq!(h.spawner.count(), 3);
        let downs: Vec<String> = h
            .take()
            .into_iter()
            .filter(|e| e.starts_with("down"))
            .collect();
        assert_eq!(
            downs,
            vec![
                "down Exited { code: Some(0), signal: None } Restarting { attempt: 1 }",
                "down Exited { code: Some(0), signal: None } Restarting { attempt: 2 }",
                "down Exited { code: Some(0), signal: None } GaveUp",
            ]
        );

        h.child.start();
        h.timers.advance(10_000);
        h.exit(3, 0);
        h.timers.advance(1000);
        h.spawner.emit(
            4,
            ChildEvent::Exit {
                code: None,
                signal: Some(libc::SIGKILL),
            },
        );
        h.timers.advance(1000);
        assert_eq!(h.spawner.count(), 5);
        let downs: Vec<String> = h
            .take()
            .into_iter()
            .filter(|e| e.starts_with("down"))
            .collect();
        assert_eq!(
            downs,
            vec![
                "down Exited { code: Some(0), signal: None } Restarting { attempt: 1 }",
                "down Exited { code: None, signal: Some(9) } NotRestarting",
            ]
        );
    }

    #[test]
    fn missing_binary_is_retried_under_always() {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let lookups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&lookups);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let child = SupervisedChild::new(
            ChildOptions {
                label: "Test helper",
                resolve_binary: Arc::new(move || {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    None
                }),
                spawner: spawner.clone(),
                timers: timers.clone(),
                restart: ALWAYS,
                heartbeat: None,
                max_line: 16,
            },
            Recorder,
            Arc::new(move |event| lock(&sink_events).push(event)),
        );
        child.start();
        timers.advance(2000);
        assert_eq!(lookups.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(spawner.count(), 0);
        assert_eq!(
            lock(&events)[0],
            "down Missing Restarting { attempt: 1 }".to_string()
        );
        child.stop();
        assert_eq!(timers.pending(), 0);
    }

    #[test]
    fn process_spawner_delivers_output_before_the_exit() {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: ChildSink = Arc::new(move |event| {
            let _ = lock(&tx).send(event);
        });
        let script = "read line; echo \"got $line\"; echo oops >&2; exit 3";
        let mut child = ProcessSpawner
            .spawn(
                Path::new("/bin/sh"),
                &["-c".to_string(), script.to_string()],
                sink,
            )
            .expect("spawn /bin/sh");
        assert!(child.write_line("hello"));
        let mut events = Vec::new();
        loop {
            let event = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("child event");
            let done = matches!(event, ChildEvent::Exit { .. });
            events.push(event);
            if done {
                break;
            }
        }
        assert!(events.contains(&ChildEvent::Stdout("got hello\n".into())));
        assert!(events.contains(&ChildEvent::Stderr("oops\n".into())));
        assert_eq!(
            events.last(),
            Some(&ChildEvent::Exit {
                code: Some(3),
                signal: None
            })
        );
    }
}

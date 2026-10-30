//! Target-app capture and post-paste edit monitoring via `macos-text-monitor`.
//!
//! - [`TextEditMonitor::capture_target_pid`] names the app holding keyboard
//!   focus at a hotkey press (the Target app), before the overlay steals focus.
//! - [`TextEditMonitor::activate_pid`] / [`TextEditMonitor::can_paste_at_target`]
//!   gate automatic paste.
//! - [`TextEditMonitor::start_monitoring`] watches the focused field after an
//!   automatic paste. The session ends when the field empties (the message was
//!   sent), focus leaves it, another paste starts or [`MONITOR_TIMEOUT`] runs
//!   out, and reports the field's last text as one [`SessionReport`] for the
//!   correction learner, so half-typed fixes are never learned.
//!
//! Monitoring is strictly read-only: never write AX attributes such as
//! AXEnhancedUserInterface on the target app (it blurs Chromium editors).
//! All methods block (helper spawns, short sleeps); call them off the main
//! thread.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use base64::Engine;

use super::helper::{probe_first_line, resolve_helper};
use super::supervised_child::{
    ChildOptions, Control, Down, HelperSpawner, ProcessSpawner, Protocol, Recovery, RestartPolicy,
    SupervisedChild,
};
use super::timer::{RealTimers, TimerId, Timers, lock};

const ACTIVATE_CONFIRM_RETRIES: u32 = 6;
const ACTIVATE_CONFIRM_DELAY: Duration = Duration::from_millis(25);
const TARGET_PROBE_TIMEOUT: Duration = Duration::from_millis(700);
/// One Dictation captures its target from several call sites within a few
/// hundred ms; reuse the result across that burst only.
const TARGET_CAPTURE_FRESHNESS: Duration = Duration::from_millis(250);
/// Long enough to read a dictation back and fix it.
pub const MONITOR_TIMEOUT: Duration = Duration::from_secs(90);
/// Wait for the paste to settle in the target app before monitoring.
pub const MONITOR_START_DELAY: Duration = Duration::from_secs(1);
/// Dictating several times in a row and then fixing an earlier part is normal,
/// so a session accepts edits to the app's recent pastes.
const RECENT_PASTES: usize = 5;
const RECENT_PASTE_WINDOW: Duration = Duration::from_secs(5 * 60);

pub const MONITOR_BINARY: &str = "macos-text-monitor";
/// The helper caps a reported value at 10240 characters.
const MAX_LINE_BYTES: usize = 64 * 1024;
/// Enough for the helper's diagnostics, which only feed
/// [`is_persistent_ax_no_value_failure`].
const MAX_STDERR_BYTES: usize = 16 * 1024;

/// The field right after the paste and its last text before the session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub baseline: String,
    pub edited: String,
    /// The transcripts this session may have corrected, oldest first.
    pub pasted: Vec<String>,
}

/// How one monitored paste went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    /// Bundle identifier of the target app.
    pub app: Option<String>,
    /// Whether the field's text could be read at all.
    pub readable: bool,
    pub edit: Option<TextEdit>,
}

/// How to invoke the helper; overridable for tests.
#[derive(Debug, Clone)]
pub struct HelperCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

type PidSource = Arc<dyn Fn() -> Option<i32> + Send + Sync>;
type Activator = Arc<dyn Fn(i32) -> bool + Send + Sync>;
type HelperResolver = Arc<dyn Fn() -> Option<HelperCommand> + Send + Sync>;

/// Seams for tests; production uses the bundled helper and AppKit.
#[derive(Clone)]
pub struct MonitorHooks {
    pub resolve_helper: HelperResolver,
    pub frontmost_pid: PidSource,
    /// Requests activation; false for apps without a regular Dock presence.
    pub activate_app: Activator,
    /// Replaces the helper-then-frontmost focus lookup when set.
    pub keyboard_focus_pid: Option<PidSource>,
    /// Starts the long-running monitoring helper.
    pub spawner: Arc<dyn HelperSpawner>,
}

impl Default for MonitorHooks {
    fn default() -> Self {
        Self {
            resolve_helper: Arc::new(|| {
                resolve_helper(MONITOR_BINARY).map(|program| HelperCommand {
                    program,
                    args: Vec::new(),
                })
            }),
            frontmost_pid: Arc::new(appkit::frontmost_pid),
            activate_app: Arc::new(appkit::activate_regular_app),
            keyboard_focus_pid: None,
            spawner: Arc::new(ProcessSpawner),
        }
    }
}

/// A lookup in progress: its result (once set) and a wake-up for waiters.
type SharedLookup = Arc<(Mutex<Option<Option<i32>>>, Condvar)>;

#[derive(Default)]
struct Capture {
    in_flight: Option<SharedLookup>,
    last_pid: Option<i32>,
    last_capture_at: Option<Instant>,
}

struct RecentPaste {
    pid: Option<i32>,
    text: String,
    at: Instant,
}

/// One monitored paste.
struct Session {
    app: Option<String>,
    pasted: Vec<String>,
    baseline: Option<String>,
    /// The last non-empty text after the baseline: pressing Send clears the
    /// field right after a fix, and the fix must survive that.
    latest: Option<String>,
    report: async_channel::Sender<SessionReport>,
}

impl Session {
    fn finish(self) {
        let readable = self.baseline.is_some();
        let edit = match (self.baseline, self.latest) {
            (Some(baseline), Some(edited)) if baseline != edited => Some(TextEdit {
                baseline,
                edited,
                pasted: self.pasted,
            }),
            _ => None,
        };
        let _ = self.report.try_send(SessionReport {
            app: self.app,
            readable,
            edit,
        });
    }

    fn on_baseline(&mut self, value: String) {
        self.baseline.get_or_insert(value);
    }

    /// A slow app takes the paste only after the baseline was read; its text
    /// then becomes the baseline. Returns false when the field was emptied.
    fn on_changed(&mut self, value: String) -> bool {
        let Some(baseline) = &self.baseline else {
            return true;
        };
        if value.trim().is_empty() {
            return baseline.trim().is_empty();
        }
        let paste_landed = self
            .pasted
            .last()
            .is_some_and(|text| !baseline.contains(text.as_str()) && value.contains(text.as_str()));
        if paste_landed {
            self.baseline = Some(value);
            self.latest = None;
        } else {
            self.latest = Some(value);
        }
        true
    }
}

#[derive(Default)]
struct Monitoring {
    session: Option<Session>,
    run: Option<SupervisedChild<EditWatch>>,
    timeout: Option<TimerId>,
    recent: VecDeque<RecentPaste>,
}

impl Monitoring {
    /// Records a paste and returns the app's recent ones, oldest first.
    fn remember_paste(&mut self, pid: Option<i32>, text: &str) -> Vec<String> {
        self.recent
            .retain(|paste| paste.pid == pid && paste.at.elapsed() < RECENT_PASTE_WINDOW);
        if self.recent.len() == RECENT_PASTES {
            self.recent.pop_front();
        }
        self.recent.push_back(RecentPaste {
            pid,
            text: text.to_string(),
            at: Instant::now(),
        });
        self.recent.iter().map(|p| p.text.clone()).collect()
    }
}

struct Inner {
    hooks: MonitorHooks,
    timers: Arc<dyn Timers>,
    capture: Mutex<Capture>,
    monitoring: Mutex<Monitoring>,
    // Bumped by every start/stop; a run whose generation is stale gives up.
    generation: AtomicU64,
    // PIDs whose AX tree never yields a focused element this session.
    unsupported_pids: Mutex<HashSet<i32>>,
    this: Weak<Inner>,
}

#[derive(Clone)]
pub struct TextEditMonitor {
    inner: Arc<Inner>,
}

impl TextEditMonitor {
    /// The process-wide monitor used by [`super::paste`].
    pub fn shared() -> TextEditMonitor {
        static SHARED: OnceLock<TextEditMonitor> = OnceLock::new();
        SHARED
            .get_or_init(|| TextEditMonitor::with_hooks(MonitorHooks::default()))
            .clone()
    }

    pub fn with_hooks(hooks: MonitorHooks) -> Self {
        Self::build(hooks, RealTimers::shared())
    }

    fn build(hooks: MonitorHooks, timers: Arc<dyn Timers>) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                hooks,
                timers,
                capture: Mutex::new(Capture::default()),
                monitoring: Mutex::new(Monitoring::default()),
                generation: AtomicU64::new(0),
                unsupported_pids: Mutex::new(HashSet::new()),
                this: this.clone(),
            }),
        }
    }

    /// The app holding keyboard focus (a floating launcher such as Raycast
    /// holds it over the menu bar app). Call at hotkey press time, before the
    /// dictation panel shows. Concurrent calls share one lookup and a fresh
    /// result is reused for 250 ms.
    pub fn capture_target_pid(&self) -> Option<i32> {
        let inner = &self.inner;
        let shared = {
            let mut capture = lock(&inner.capture);
            if let Some(in_flight) = &capture.in_flight {
                Some(Arc::clone(in_flight))
            } else {
                if capture.last_pid.is_some()
                    && capture
                        .last_capture_at
                        .is_some_and(|at| at.elapsed() < TARGET_CAPTURE_FRESHNESS)
                {
                    return capture.last_pid;
                }
                capture.last_pid = None;
                capture.in_flight = Some(Arc::new((Mutex::new(None), Condvar::new())));
                None
            }
        };
        if let Some(shared) = shared {
            let (slot, ready) = &*shared;
            let mut result = lock(slot);
            loop {
                if let Some(pid) = *result {
                    return pid;
                }
                result = ready
                    .wait(result)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }

        let pid = inner.read_keyboard_focus_pid();
        let mut capture = lock(&inner.capture);
        if let Some(shared) = capture.in_flight.take() {
            *lock(&shared.0) = Some(pid);
            shared.1.notify_all();
        }
        capture.last_capture_at = Some(Instant::now());
        capture.last_pid = pid;
        log::debug!("[text-monitor] captured target pid {pid:?}");
        pid
    }

    /// The pid captured most recently, if any.
    #[cfg(test)]
    pub fn last_target_pid(&self) -> Option<i32> {
        lock(&self.inner.capture).last_pid
    }

    /// Gives the Target app keyboard focus before pasting. True once it holds
    /// focus. An app already holding focus is left alone (re-activating a
    /// Chromium app drops its field's first responder); an app without a
    /// regular Dock presence (a closed launcher) is never activated.
    pub fn activate_pid(&self, pid: Option<i32>) -> bool {
        let Some(pid) = pid.filter(|p| *p > 0) else {
            return false;
        };
        let inner = &self.inner;
        if inner.read_keyboard_focus_pid() == Some(pid) {
            return true;
        }
        if !(inner.hooks.activate_app)(pid) {
            log::debug!("[text-monitor] target {pid} cannot be activated");
            return false;
        }
        for _ in 0..ACTIVATE_CONFIRM_RETRIES {
            std::thread::sleep(ACTIVATE_CONFIRM_DELAY);
            if inner.read_keyboard_focus_pid() == Some(pid) {
                return true;
            }
        }
        log::debug!("[text-monitor] target {pid} did not take keyboard focus");
        false
    }

    /// Whether the focused element of `pid` accepts a paste: `Some(true)`
    /// pasteable, `Some(false)` explicitly not, `None` unknown/unavailable.
    pub fn can_paste_at_target(&self, pid: Option<i32>) -> Option<bool> {
        self.can_paste_at_target_within(pid, TARGET_PROBE_TIMEOUT)
    }

    pub fn can_paste_at_target_within(&self, pid: Option<i32>, timeout: Duration) -> Option<bool> {
        let pid = pid?;
        let helper = (self.inner.hooks.resolve_helper)()?;
        let mut args: Vec<String> = helper.args.clone();
        args.push("--paste-target".into());
        args.push(pid.to_string());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match probe_first_line(&helper.program, &refs, timeout)?.as_str() {
            "PASTEABLE" => Some(true),
            "NOT_PASTEABLE" => Some(false),
            _ => None,
        }
    }

    /// Waits [`MONITOR_START_DELAY`] (on a background thread) and then
    /// monitors, as automatic paste does after a successful paste. The
    /// receiver gets one report when the session ends.
    pub fn start_monitoring_after_paste(
        &self,
        original_text: &str,
        target_pid: Option<i32>,
    ) -> async_channel::Receiver<SessionReport> {
        let (report, receiver) = async_channel::bounded(1);
        let monitor = self.clone();
        let text = original_text.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(MONITOR_START_DELAY);
            monitor.start_monitoring(&text, MONITOR_TIMEOUT, target_pid, report);
        });
        receiver
    }

    /// Watches the focused field of `target_pid` for `timeout`. Ends any
    /// previous session first. Returns immediately; `report` always receives
    /// the outcome.
    pub fn start_monitoring(
        &self,
        original_text: &str,
        timeout: Duration,
        target_pid: Option<i32>,
        report: async_channel::Sender<SessionReport>,
    ) {
        self.stop_monitoring();
        let inner = &self.inner;
        let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let app = target_pid.and_then(appkit::bundle_identifier);
        {
            let mut monitoring = lock(&inner.monitoring);
            let pasted = monitoring.remember_paste(target_pid, original_text);
            monitoring.session = Some(Session {
                app,
                pasted,
                baseline: None,
                latest: None,
                report,
            });
        }

        let Some(pid) = target_pid.filter(|p| *p > 0) else {
            log::debug!("[text-monitor] no target pid");
            self.stop_monitoring();
            return;
        };
        let Some(helper) = (inner.hooks.resolve_helper)() else {
            log::debug!("[text-monitor] helper unavailable");
            self.stop_monitoring();
            return;
        };
        if lock(&inner.unsupported_pids).contains(&pid) {
            log::debug!("[text-monitor] AX reads unsupported for {pid} this session");
            self.stop_monitoring();
            return;
        }

        let weak = inner.this.clone();
        inner.arm_timeout(generation, timeout);
        std::thread::spawn(move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.run_native(generation, pid, helper, timeout);
        });
    }

    /// Ends the session, reporting what it saw. A paste ends the previous
    /// session before it lands, so the new text is not read as an edit.
    pub fn stop_monitoring(&self) {
        self.inner.stop(None);
    }

    /// True while a monitoring run is in progress.
    #[cfg(test)]
    pub fn is_monitoring(&self) -> bool {
        lock(&self.inner.monitoring).session.is_some()
    }
}

impl Inner {
    fn read_keyboard_focus_pid(&self) -> Option<i32> {
        if let Some(source) = &self.hooks.keyboard_focus_pid {
            return source();
        }
        if let Some(helper) = (self.hooks.resolve_helper)() {
            let mut args: Vec<String> = helper.args.clone();
            args.push("--focused-app".into());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let pid = probe_first_line(&helper.program, &refs, TARGET_PROBE_TIMEOUT)
                .and_then(|line| line.strip_prefix("FOCUSED_PID:")?.parse::<i32>().ok())
                .filter(|pid| *pid > 0);
            if pid.is_some() {
                return pid;
            }
        }
        (self.hooks.frontmost_pid)()
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
            && lock(&self.monitoring).session.is_some()
    }

    /// Stops monitoring; with `Some(generation)` only if that run is current.
    fn stop(&self, generation: Option<u64>) {
        if let Some(generation) = generation
            && self.generation.load(Ordering::SeqCst) != generation
        {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        let session = {
            let mut monitoring = lock(&self.monitoring);
            if let Some(timer) = monitoring.timeout.take() {
                self.timers.cancel(timer);
            }
            if let Some(run) = monitoring.run.take() {
                run.stop();
            }
            monitoring.session.take()
        };
        if let Some(session) = session {
            session.finish();
        }
    }

    fn arm_timeout(&self, generation: u64, timeout: Duration) {
        let weak = self.this.clone();
        let timer = self.timers.schedule(
            timeout,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.stop(Some(generation));
                }
            }),
        );
        lock(&self.monitoring).timeout = Some(timer);
    }

    fn update_session(&self, generation: u64, update: impl FnOnce(&mut Session) -> bool) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let keep_going = lock(&self.monitoring).session.as_mut().is_none_or(update);
        if !keep_going {
            log::debug!("[text-monitor] field emptied");
            self.stop(Some(generation));
        }
    }

    fn handle_line(&self, generation: u64, line: &str) {
        if let Some(value) = payload(line, "CHANGED") {
            self.update_session(generation, |session| session.on_changed(value));
        } else if let Some(value) = payload(line, "BASELINE") {
            self.update_session(generation, |session| {
                session.on_baseline(value);
                true
            });
        } else if line == "NO_ELEMENT" || line == "NO_VALUE" {
            // The helper exits right after; its end stops monitoring.
            log::debug!("[text-monitor] no target element ({line})");
        }
    }

    fn on_watch_event(&self, generation: u64, pid: i32, event: WatchEvent) {
        match event {
            WatchEvent::Line(line) => self.handle_line(generation, &line),
            WatchEvent::Ended { ax_unsupported } => {
                // NO_ELEMENT after a uniform -25212 run is terminal for this app.
                if ax_unsupported {
                    lock(&self.unsupported_pids).insert(pid);
                }
                self.stop(Some(generation));
            }
        }
    }

    /// Event-based monitoring through the helper's AXObserver.
    fn run_native(&self, generation: u64, pid: i32, helper: HelperCommand, timeout: Duration) {
        let program = helper.program;
        let mut args = helper.args;
        args.push(pid.to_string());
        args.push(timeout.as_secs().to_string());
        let weak = self.this.clone();
        let run = SupervisedChild::new(
            ChildOptions {
                label: "Text monitor",
                resolve_binary: Arc::new(move || Some(program.clone())),
                spawner: Arc::clone(&self.hooks.spawner),
                timers: Arc::clone(&self.timers),
                restart: RestartPolicy::Never,
                heartbeat: None,
                max_line: MAX_LINE_BYTES,
            },
            EditWatch {
                args,
                stderr: String::new(),
                no_element: false,
            },
            Arc::new(move |event| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_watch_event(generation, pid, event);
                }
            }),
        );
        {
            let mut monitoring = lock(&self.monitoring);
            if self.generation.load(Ordering::SeqCst) != generation || monitoring.session.is_none()
            {
                return;
            }
            monitoring.run = Some(run.clone());
        }
        run.start();
        // A stop between storing the run and starting it had nothing to end.
        if !self.is_current(generation) {
            run.stop();
        }
    }
}

enum WatchEvent {
    Line(String),
    /// The helper ended; `ax_unsupported` once the target app's AX tree
    /// proved permanently unreadable.
    Ended {
        ax_unsupported: bool,
    },
}

/// The text of a `<prefix>:` or, for multi-line text, `<prefix>_B64:` line.
fn payload(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    if let Some(encoded) = rest.strip_prefix("_B64:") {
        return match base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
            Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) => {
                log::debug!("[text-monitor] bad base64 payload: {error}");
                None
            }
        };
    }
    rest.strip_prefix(':').map(str::to_string)
}

/// One `macos-text-monitor <pid>` run: forwards the helper's lines and
/// judges its diagnostics when it ends.
struct EditWatch {
    args: Vec<String>,
    stderr: String,
    no_element: bool,
}

impl Protocol for EditWatch {
    type Event = WatchEvent;

    fn args(&self) -> Vec<String> {
        self.args.clone()
    }

    fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, WatchEvent>) {
        if line == "NO_ELEMENT" {
            self.no_element = true;
        }
        cx.emit(WatchEvent::Line(line.to_string()));
    }

    fn on_stderr(&mut self, text: &str, _cx: &mut Control<'_, WatchEvent>) {
        if self.stderr.len() < MAX_STDERR_BYTES {
            self.stderr.push_str(text);
        }
    }

    fn on_down(&mut self, down: &Down, _recovery: Recovery, cx: &mut Control<'_, WatchEvent>) {
        match down {
            Down::Stopped => {}
            Down::Missing | Down::NotStarted(_) => {
                log::debug!("[text-monitor] helper failed to start ({down:?})");
                cx.emit(WatchEvent::Ended {
                    ax_unsupported: false,
                });
            }
            Down::Failed(_) | Down::Exited { .. } => cx.emit(WatchEvent::Ended {
                ax_unsupported: self.no_element && is_persistent_ax_no_value_failure(&self.stderr),
            }),
        }
    }
}

/// AXError -25212 (kAXErrorNoValue) on every attempt, and the element never
/// resolved: a Chromium/Electron app whose AX tree stays dormant.
fn is_persistent_ax_no_value_failure(stderr: &str) -> bool {
    if stderr.contains("Got focused element") {
        return false;
    }
    let mut codes = Vec::new();
    let mut rest = stderr;
    while let Some(start) = rest.find("(error: ") {
        let after = &rest[start + 8..];
        let Some(end) = after.find(')') else { break };
        let code = &after[..end];
        if code
            .trim_start_matches('-')
            .chars()
            .all(|c| c.is_ascii_digit())
            && !code.is_empty()
        {
            codes.push(code.to_string());
        }
        rest = &after[end..];
    }
    !codes.is_empty() && codes.iter().all(|code| code == "-25212")
}

/// In-process AppKit lookups.
pub(crate) mod appkit {
    use objc2_app_kit::{
        NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication,
        NSWorkspace,
    };

    pub fn frontmost_pid() -> Option<i32> {
        let workspace = NSWorkspace::sharedWorkspace();
        workspace
            .frontmostApplication()
            .map(|app| app.processIdentifier())
            .filter(|pid| *pid > 0)
    }

    pub fn bundle_identifier(pid: i32) -> Option<String> {
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
            .bundleIdentifier()
            .map(|id| id.to_string())
    }

    /// Activates `pid` with all its windows, unless it has no regular Dock
    /// presence (a floating launcher that closed could reopen and take the
    /// paste).
    #[allow(deprecated)]
    pub fn activate_regular_app(pid: i32) -> bool {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return false;
        };
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            return false;
        }
        app.activateWithOptions(
            NSApplicationActivationOptions::ActivateAllWindows
                | NSApplicationActivationOptions::ActivateIgnoringOtherApps,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    //! Capture target and monitor target.
    use super::*;
    use crate::native::supervised_child::ChildEvent;
    use crate::native::supervised_child::tests::FakeSpawner;
    use crate::native::timer::FakeTimers;
    use std::sync::atomic::AtomicUsize;

    fn sh(script: &str) -> HelperResolver {
        let script = script.to_string();
        Arc::new(move || {
            Some(HelperCommand {
                program: PathBuf::from("/bin/sh"),
                args: vec!["-c".into(), script.clone()],
            })
        })
    }

    fn hooks(resolve: HelperResolver, frontmost: PidSource) -> MonitorHooks {
        MonitorHooks {
            resolve_helper: resolve,
            frontmost_pid: frontmost,
            activate_app: Arc::new(|_| false),
            keyboard_focus_pid: None,
            spawner: Arc::new(ProcessSpawner),
        }
    }

    /// Monitoring runs go to `spawner`; one-shot probes find no binary at
    /// the fake path.
    fn fake_hooks(spawner: &Arc<FakeSpawner>) -> MonitorHooks {
        MonitorHooks {
            resolve_helper: Arc::new(|| {
                Some(HelperCommand {
                    program: PathBuf::from("/fake/macos-text-monitor"),
                    args: Vec::new(),
                })
            }),
            frontmost_pid: Arc::new(|| None),
            activate_app: Arc::new(|_| false),
            keyboard_focus_pid: None,
            spawner: spawner.clone(),
        }
    }

    /// Returns once the run spawned; its first event then waits for the
    /// spawn to finish.
    fn wait_for_spawn(spawner: &FakeSpawner) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while spawner.count() == 0 {
            assert!(Instant::now() < deadline, "monitor never spawned");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn no_helper() -> HelperResolver {
        Arc::new(|| None)
    }

    fn watch(
        monitor: &TextEditMonitor,
        text: &str,
        pid: Option<i32>,
    ) -> async_channel::Receiver<SessionReport> {
        let (report, receiver) = async_channel::bounded(1);
        monitor.start_monitoring(text, MONITOR_TIMEOUT, pid, report);
        receiver
    }

    fn counting(values: Vec<Option<i32>>, delay: Duration) -> (PidSource, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let source: PidSource = Arc::new(move || {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(delay);
            values.get(n).or(values.last()).copied().flatten()
        });
        (source, calls)
    }

    #[test]
    fn concurrent_captures_share_one_lookup() {
        let (frontmost, calls) = counting(vec![Some(4242)], Duration::from_millis(100));
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), frontmost));
        let other = monitor.clone();
        let first = std::thread::spawn(move || other.capture_target_pid());
        std::thread::sleep(Duration::from_millis(20));
        let second = monitor.capture_target_pid();
        assert_eq!(first.join().unwrap(), Some(4242));
        assert_eq!(second, Some(4242));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(monitor.last_target_pid(), Some(4242));
    }

    #[test]
    fn fresh_capture_is_reused_and_failure_is_retried() {
        let (frontmost, calls) = counting(vec![Some(4242)], Duration::ZERO);
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), frontmost));
        assert_eq!(monitor.capture_target_pid(), Some(4242));
        assert_eq!(monitor.capture_target_pid(), Some(4242));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let (frontmost, calls) = counting(vec![None, Some(4242)], Duration::ZERO);
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), frontmost));
        assert_eq!(monitor.capture_target_pid(), None);
        assert_eq!(monitor.capture_target_pid(), Some(4242));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn captures_refresh_after_the_reuse_window() {
        let (frontmost, _) = counting(vec![Some(1111), Some(2222)], Duration::ZERO);
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), frontmost));
        assert_eq!(monitor.capture_target_pid(), Some(1111));
        lock(&monitor.inner.capture).last_capture_at =
            Some(Instant::now() - Duration::from_secs(10));
        assert_eq!(monitor.capture_target_pid(), Some(2222));
        assert_eq!(monitor.last_target_pid(), Some(2222));
    }

    #[test]
    fn captures_the_keyboard_focus_owner_and_falls_back_to_frontmost() {
        let monitor = TextEditMonitor::with_hooks(hooks(
            sh("[ \"$0\" = \"--focused-app\" ] && echo 'FOCUSED_PID:4242'"),
            Arc::new(|| Some(1111)),
        ));
        assert_eq!(monitor.capture_target_pid(), Some(4242));
        for output in ["UNKNOWN", "NO_ELEMENT"] {
            let monitor = TextEditMonitor::with_hooks(hooks(
                sh(&format!("echo '{output}'")),
                Arc::new(|| Some(1111)),
            ));
            assert_eq!(monitor.capture_target_pid(), Some(1111), "{output}");
        }
    }

    #[test]
    fn activation_rules() {
        let activations = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&activations);
        let monitor = TextEditMonitor::with_hooks(MonitorHooks {
            resolve_helper: no_helper(),
            frontmost_pid: Arc::new(|| Some(1111)),
            activate_app: Arc::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                true
            }),
            keyboard_focus_pid: Some(Arc::new(|| Some(4242))),
            spawner: Arc::new(ProcessSpawner),
        });
        assert!(monitor.activate_pid(Some(4242)));
        assert_eq!(
            activations.load(Ordering::SeqCst),
            0,
            "focused target is not re-activated"
        );
        assert!(!monitor.activate_pid(None));

        let focus = Arc::new(Mutex::new(1111));
        let read = Arc::clone(&focus);
        let write = Arc::clone(&focus);
        let monitor = TextEditMonitor::with_hooks(MonitorHooks {
            resolve_helper: no_helper(),
            frontmost_pid: Arc::new(|| None),
            activate_app: Arc::new(move |pid| {
                *lock(&write) = pid;
                true
            }),
            keyboard_focus_pid: Some(Arc::new(move || Some(*lock(&read)))),
            spawner: Arc::new(ProcessSpawner),
        });
        assert!(monitor.activate_pid(Some(4242)));

        let lookups = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&lookups);
        let monitor = TextEditMonitor::with_hooks(MonitorHooks {
            resolve_helper: no_helper(),
            frontmost_pid: Arc::new(|| None),
            activate_app: Arc::new(|_| false),
            keyboard_focus_pid: Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Some(1111)
            })),
            spawner: Arc::new(ProcessSpawner),
        });
        assert!(!monitor.activate_pid(Some(4242)));
        assert_eq!(lookups.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[ignore = "activates real applications"]
    fn activate_unmapped_pid_is_false() {
        assert!(!TextEditMonitor::shared().activate_pid(Some(99_999_999)));
    }

    #[test]
    fn paste_target_verdicts() {
        for (output, expected) in [
            ("PASTEABLE", Some(true)),
            ("NOT_PASTEABLE", Some(false)),
            ("UNKNOWN", None),
            ("NO_ELEMENT", None),
            ("CHANGED:stale", None),
        ] {
            let monitor = TextEditMonitor::with_hooks(hooks(
                sh(&format!("echo '{output}'")),
                Arc::new(|| None),
            ));
            assert_eq!(monitor.can_paste_at_target(Some(42)), expected, "{output}");
        }
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), Arc::new(|| None)));
        assert_eq!(monitor.can_paste_at_target(None), None);
        assert_eq!(monitor.can_paste_at_target(Some(42)), None);
        let missing = TextEditMonitor::with_hooks(hooks(
            Arc::new(|| {
                Some(HelperCommand {
                    program: PathBuf::from("/nonexistent/paste-probe"),
                    args: vec![],
                })
            }),
            Arc::new(|| None),
        ));
        assert_eq!(missing.can_paste_at_target(Some(42)), None);
        let slow = TextEditMonitor::with_hooks(hooks(sh("exec sleep 2"), Arc::new(|| None)));
        assert_eq!(
            slow.can_paste_at_target_within(Some(42), Duration::from_millis(25)),
            None
        );
    }

    #[test]
    fn paste_target_probes_the_pid_and_takes_the_first_verdict() {
        let monitor = TextEditMonitor::with_hooks(hooks(
            sh(
                "[ \"$0\" = \"--paste-target\" ] && [ \"$1\" = \"42\" ] && printf 'NOT_PASTEABLE\\nPASTEABLE\\n'",
            ),
            Arc::new(|| None),
        ));
        assert_eq!(monitor.can_paste_at_target(Some(42)), Some(false));
    }

    #[test]
    fn monitoring_without_target_or_for_cached_pid_stops_at_once() {
        let monitor = TextEditMonitor::with_hooks(hooks(sh("echo"), Arc::new(|| None)));
        let reports = watch(&monitor, "pasted text", None);
        assert!(!monitor.is_monitoring());
        assert!(!reports.try_recv().unwrap().readable);
        lock(&monitor.inner.unsupported_pids).insert(42);
        let reports = watch(&monitor, "pasted text", Some(42));
        assert!(!monitor.is_monitoring());
        assert!(!reports.try_recv().unwrap().readable);
    }

    #[test]
    fn monitoring_without_the_helper_reports_an_unreadable_field() {
        let monitor = TextEditMonitor::with_hooks(hooks(no_helper(), Arc::new(|| None)));
        let reports = watch(&monitor, "pasted text", Some(42));
        assert!(!monitor.is_monitoring());
        assert!(!reports.try_recv().unwrap().readable);
    }

    #[test]
    fn no_element_with_uniform_no_value_teaches_the_cache() {
        let spawner = Arc::new(FakeSpawner::default());
        let monitor = TextEditMonitor::with_hooks(fake_hooks(&spawner));
        let _reports = watch(&monitor, "pasted text", Some(4242));
        wait_for_spawn(&spawner);
        assert_eq!(spawner.args(0), ["4242", "90"]);
        spawner.emit(
            0,
            ChildEvent::Stderr(
                "Attempt 5/5: Cannot get focused element for PID 4242 (error: -25212)\n".into(),
            ),
        );
        spawner.emit(0, ChildEvent::Stdout("NO_ELEMENT\n".into()));
        assert!(monitor.is_monitoring(), "monitoring ends with the helper");
        spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(1),
                signal: None,
            },
        );
        assert!(!monitor.is_monitoring());
        assert!(lock(&monitor.inner.unsupported_pids).contains(&4242));
    }

    #[test]
    fn persistent_failure_detection() {
        assert!(is_persistent_ax_no_value_failure(
            "Attempt 1/5: x (error: -25212)\nAttempt 2/5: x (error: -25212)"
        ));
        assert!(!is_persistent_ax_no_value_failure(
            "Attempt 1/5: x (error: -25212)\nGot focused element on attempt 2"
        ));
        assert!(!is_persistent_ax_no_value_failure(
            "Attempt 1/5: x (error: -25204)"
        ));
        assert!(!is_persistent_ax_no_value_failure(""));
    }

    /// A session on a fake helper, with its first lines already delivered.
    fn session(
        lines: &str,
    ) -> (
        TextEditMonitor,
        Arc<FakeSpawner>,
        Arc<FakeTimers>,
        async_channel::Receiver<SessionReport>,
    ) {
        let timers = FakeTimers::new();
        let spawner = Arc::new(FakeSpawner::default());
        let monitor = TextEditMonitor::build(fake_hooks(&spawner), timers.clone());
        let reports = watch(&monitor, "hello world", Some(42));
        wait_for_spawn(&spawner);
        spawner.emit(0, ChildEvent::Stdout(lines.into()));
        (monitor, spawner, timers, reports)
    }

    fn edit(baseline: &str, edited: &str) -> Option<TextEdit> {
        Some(TextEdit {
            baseline: baseline.into(),
            edited: edited.into(),
            pasted: vec!["hello world".into()],
        })
    }

    #[test]
    fn a_session_reports_the_last_text_once_it_ends() {
        let (monitor, _spawner, _timers, reports) =
            session("BASELINE:hello world\nCHANGED:hello wrld\nCHANGED_B64:aGVsbG8Kd29ybGQ=\n");
        assert!(reports.try_recv().is_err(), "nothing before the end");
        monitor.stop_monitoring();
        assert_eq!(
            reports.try_recv().unwrap(),
            SessionReport {
                app: None,
                readable: true,
                edit: edit("hello world", "hello\nworld"),
            }
        );
    }

    #[test]
    fn a_session_with_no_edit_reports_none() {
        let (monitor, _spawner, _timers, reports) = session("BASELINE:hello world\n");
        monitor.stop_monitoring();
        let report = reports.try_recv().unwrap();
        assert!(report.readable);
        assert_eq!(report.edit, None);
    }

    #[test]
    fn typing_the_text_back_is_no_edit() {
        let (monitor, _spawner, _timers, reports) =
            session("BASELINE:hello world\nCHANGED:hello\nCHANGED:hello world\n");
        monitor.stop_monitoring();
        assert_eq!(reports.try_recv().unwrap().edit, None);
    }

    #[test]
    fn the_timeout_ends_a_session() {
        let (monitor, _spawner, timers, reports) =
            session("BASELINE:hello world\nCHANGED:hello there\n");
        timers.advance(90_000);
        assert!(!monitor.is_monitoring());
        assert_eq!(
            reports.try_recv().unwrap().edit,
            edit("hello world", "hello there")
        );
    }

    #[test]
    fn emptying_the_field_keeps_the_fix_made_just_before() {
        let (monitor, _spawner, _timers, reports) =
            session("BASELINE:hello world\nCHANGED:hello there\nCHANGED:\n");
        assert!(!monitor.is_monitoring(), "sending ends the session");
        assert_eq!(
            reports.try_recv().unwrap().edit,
            edit("hello world", "hello there")
        );
    }

    #[test]
    fn a_paste_landing_after_the_baseline_becomes_the_baseline() {
        let (monitor, _spawner, _timers, reports) =
            session("BASELINE:\nCHANGED:hello world\nCHANGED:hello there\n");
        assert!(monitor.is_monitoring());
        monitor.stop_monitoring();
        assert_eq!(
            reports.try_recv().unwrap().edit,
            edit("hello world", "hello there")
        );
    }

    #[test]
    fn the_helper_ending_ends_the_session() {
        let (monitor, spawner, _timers, reports) =
            session("BASELINE:hello world\nCHANGED:hello there\n");
        spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        assert!(!monitor.is_monitoring());
        assert!(reports.try_recv().unwrap().edit.is_some());
    }

    #[test]
    fn a_new_session_ends_the_last_and_remembers_its_paste() {
        let (monitor, spawner, _timers, first) =
            session("BASELINE:hello world\nCHANGED:hello there\n");
        let second = watch(&monitor, "next part", Some(42));
        assert!(first.try_recv().unwrap().edit.is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while spawner.count() < 2 {
            assert!(Instant::now() < deadline, "monitor never spawned");
            std::thread::sleep(Duration::from_millis(10));
        }
        spawner.emit(
            1,
            ChildEvent::Stdout(
                "BASELINE:hello there next part\nCHANGED:hello you next part\n".into(),
            ),
        );
        monitor.stop_monitoring();
        let report = second.try_recv().unwrap();
        assert_eq!(report.edit.unwrap().pasted, ["hello world", "next part"]);
    }

    #[test]
    fn recent_pastes_are_kept_per_app_and_capped() {
        let mut monitoring = Monitoring::default();
        for i in 0..7 {
            monitoring.remember_paste(Some(1), &i.to_string());
        }
        assert_eq!(
            monitoring.remember_paste(Some(1), "7"),
            ["3", "4", "5", "6", "7"]
        );
        assert_eq!(monitoring.remember_paste(Some(2), "other"), ["other"]);
    }

    #[test]
    fn payloads_decode_plain_and_base64_lines() {
        assert_eq!(payload("CHANGED:a: b", "CHANGED").as_deref(), Some("a: b"));
        assert_eq!(
            payload("BASELINE_B64:aGVsbG8Kd29ybGQ=", "BASELINE").as_deref(),
            Some("hello\nworld")
        );
        assert_eq!(payload("CHANGED_B64:not base64!", "CHANGED"), None);
        assert_eq!(payload("OTHER:x", "CHANGED"), None);
    }
}

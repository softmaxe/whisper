//! Target-app capture and post-paste edit monitoring via `macos-text-monitor`.
//!
//! - [`TextEditMonitor::capture_target_pid`] names the app holding keyboard
//!   focus at a hotkey press (the Target app), before the overlay steals focus.
//! - [`TextEditMonitor::activate_pid`] / [`TextEditMonitor::can_paste_at_target`]
//!   gate automatic paste.
//! - [`TextEditMonitor::start_monitoring`] watches the focused field for 30 s
//!   after an automatic paste and reports the user's edits, debounced, as
//!   [`TextEdit`]s for the correction learner.
//!
//! Monitoring is strictly read-only: never write AX attributes such as
//! AXEnhancedUserInterface on the target app (it blurs Chromium editors).
//! All methods block (helper spawns, short sleeps); call them off the main
//! thread.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use base64::Engine;

use super::helper::{probe_first_line, resolve_helper, run_with_timeout, signal_pid};
use super::timer::{RealTimers, TimerId, Timers, lock};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Wait for the paste to settle in the target app.
const INITIAL_QUERY_DELAY: Duration = Duration::from_millis(500);
const INITIAL_QUERY_RETRIES: u32 = 4;
const INITIAL_QUERY_RETRY_DELAY: Duration = Duration::from_millis(300);
const ACTIVATE_CONFIRM_RETRIES: u32 = 6;
const ACTIVATE_CONFIRM_DELAY: Duration = Duration::from_millis(25);
const TARGET_PROBE_TIMEOUT: Duration = Duration::from_millis(700);
/// One Dictation captures its target from several call sites within a few
/// hundred ms; reuse the result across that burst only.
const TARGET_CAPTURE_FRESHNESS: Duration = Duration::from_millis(250);
pub const MONITOR_TIMEOUT: Duration = Duration::from_secs(30);
/// Delay between a paste and the start of monitoring.
pub const MONITOR_START_DELAY: Duration = Duration::from_millis(500);
/// Edits are reported once the field has been quiet this long.
pub const EDIT_DEBOUNCE: Duration = Duration::from_millis(1500);

pub const MONITOR_BINARY: &str = "macos-text-monitor";

/// The pasted transcript and the field's text after the user edited it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub original_text: String,
    pub edited_text: String,
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

#[derive(Default)]
struct Monitoring {
    original_text: Option<String>,
    child_pid: Option<u32>,
    timeout: Option<TimerId>,
}

#[derive(Default)]
struct Debounce {
    latest: Option<TextEdit>,
    timer: Option<TimerId>,
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
    debounce: Mutex<Debounce>,
    debounce_delay: Duration,
    subscribers: Mutex<Vec<async_channel::Sender<TextEdit>>>,
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
        Self::build(hooks, RealTimers::shared(), EDIT_DEBOUNCE)
    }

    fn build(hooks: MonitorHooks, timers: Arc<dyn Timers>, debounce_delay: Duration) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                hooks,
                timers,
                capture: Mutex::new(Capture::default()),
                monitoring: Mutex::new(Monitoring::default()),
                generation: AtomicU64::new(0),
                unsupported_pids: Mutex::new(HashSet::new()),
                debounce: Mutex::new(Debounce::default()),
                debounce_delay,
                subscribers: Mutex::new(Vec::new()),
                this: this.clone(),
            }),
        }
    }

    /// Debounced `(original, edited)` pairs from monitoring runs.
    pub fn subscribe(&self) -> async_channel::Receiver<TextEdit> {
        let (tx, rx) = async_channel::unbounded();
        lock(&self.inner.subscribers).push(tx);
        rx
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
    /// monitors, as automatic paste does after a successful paste.
    pub fn start_monitoring_after_paste(&self, original_text: &str, target_pid: Option<i32>) {
        let monitor = self.clone();
        let text = original_text.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(MONITOR_START_DELAY);
            monitor.start_monitoring(&text, MONITOR_TIMEOUT, target_pid);
        });
    }

    /// Watches the focused field of `target_pid` for edits for `timeout`.
    /// Stops any previous run first. Returns immediately.
    pub fn start_monitoring(
        &self,
        original_text: &str,
        timeout: Duration,
        target_pid: Option<i32>,
    ) {
        self.stop_monitoring();
        let inner = &self.inner;
        let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        lock(&inner.monitoring).original_text = Some(original_text.to_string());

        let Some(pid) = target_pid.filter(|p| *p > 0) else {
            log::debug!("[text-monitor] no target pid");
            self.stop_monitoring();
            return;
        };
        let helper = (inner.hooks.resolve_helper)();
        if helper.is_some() && lock(&inner.unsupported_pids).contains(&pid) {
            log::debug!("[text-monitor] AX reads unsupported for {pid} this session");
            self.stop_monitoring();
            return;
        }

        let weak = inner.this.clone();
        inner.arm_timeout(generation, timeout);
        let text = original_text.to_string();
        std::thread::spawn(move || {
            let Some(inner) = weak.upgrade() else { return };
            match helper {
                Some(helper) => inner.run_native(generation, &text, pid, helper),
                None => inner.run_polling(generation, pid),
            }
        });
    }

    pub fn stop_monitoring(&self) {
        self.inner.stop(None);
    }

    /// True while a monitoring run is in progress.
    #[cfg(test)]
    pub fn is_monitoring(&self) -> bool {
        lock(&self.inner.monitoring).original_text.is_some()
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
            && lock(&self.monitoring).original_text.is_some()
    }

    /// Stops monitoring; with `Some(generation)` only if that run is current.
    fn stop(&self, generation: Option<u64>) {
        if let Some(generation) = generation
            && self.generation.load(Ordering::SeqCst) != generation
        {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        let mut monitoring = lock(&self.monitoring);
        if let Some(timer) = monitoring.timeout.take() {
            self.timers.cancel(timer);
        }
        if let Some(pid) = monitoring.child_pid.take() {
            signal_pid(pid, libc::SIGTERM);
        }
        monitoring.original_text = None;
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

    fn emit_edit(&self, generation: u64, value: String) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let Some(original_text) = lock(&self.monitoring).original_text.clone() else {
            return;
        };
        log::debug!("[text-monitor] text changed");
        let mut debounce = lock(&self.debounce);
        debounce.latest = Some(TextEdit {
            original_text,
            edited_text: value,
        });
        if let Some(timer) = debounce.timer.take() {
            self.timers.cancel(timer);
        }
        let weak = self.this.clone();
        debounce.timer = Some(self.timers.schedule(
            self.debounce_delay,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.flush_edit();
                }
            }),
        ));
    }

    fn flush_edit(&self) {
        let edit = {
            let mut debounce = lock(&self.debounce);
            debounce.timer = None;
            debounce.latest.take()
        };
        if let Some(edit) = edit {
            lock(&self.subscribers).retain(|tx| tx.try_send(edit.clone()).is_ok());
        }
    }

    fn handle_line(&self, generation: u64, line: &str) {
        if let Some(encoded) = line.strip_prefix("CHANGED_B64:") {
            match base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
                Ok(bytes) => {
                    self.emit_edit(generation, String::from_utf8_lossy(&bytes).into_owned())
                }
                Err(error) => log::debug!("[text-monitor] bad base64 payload: {error}"),
            }
        } else if let Some(value) = line.strip_prefix("CHANGED:") {
            self.emit_edit(generation, value.to_string());
        } else if line == "NO_ELEMENT" || line == "NO_VALUE" {
            log::debug!("[text-monitor] no target element ({line})");
            self.stop(Some(generation));
        }
    }

    /// Event-based monitoring through the helper's AXObserver.
    fn run_native(&self, generation: u64, original_text: &str, pid: i32, helper: HelperCommand) {
        std::thread::sleep(INITIAL_QUERY_DELAY);
        if !self.is_current(generation) {
            return;
        }
        let mut child = match Command::new(&helper.program)
            .args(&helper.args)
            .arg(pid.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                log::debug!("[text-monitor] native monitor failed ({error}), polling instead");
                self.run_polling_after_delay(generation, pid);
                return;
            }
        };
        {
            let mut monitoring = lock(&self.monitoring);
            if self.generation.load(Ordering::SeqCst) != generation {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            monitoring.child_pid = Some(child.id());
        }
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(format!("{original_text}\n").as_bytes());
        }
        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                text
            })
        });
        let mut no_element = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.is_empty() {
                    continue;
                }
                if line.contains("NO_ELEMENT") {
                    no_element = true;
                }
                self.handle_line(generation, &line);
            }
        }
        let _ = child.wait();
        let stderr = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
        {
            let mut monitoring = lock(&self.monitoring);
            if monitoring.child_pid == Some(child.id()) {
                monitoring.child_pid = None;
            }
        }
        // NO_ELEMENT after a uniform -25212 run is terminal for this app.
        if no_element && is_persistent_ax_no_value_failure(&stderr) {
            lock(&self.unsupported_pids).insert(pid);
        }
    }

    fn run_polling_after_delay(&self, generation: u64, pid: i32) {
        self.poll(generation, pid);
    }

    fn run_polling(&self, generation: u64, pid: i32) {
        std::thread::sleep(INITIAL_QUERY_DELAY);
        self.poll(generation, pid);
    }

    /// osascript polling fallback when the helper is unavailable.
    fn poll(&self, generation: u64, pid: i32) {
        let mut last_value = None;
        for attempt in 1..=INITIAL_QUERY_RETRIES {
            if !self.is_current(generation) {
                return;
            }
            match query_focused_value(pid) {
                None => {
                    self.stop(Some(generation));
                    return;
                }
                Some(value) if !value.is_empty() => {
                    last_value = Some(value);
                    break;
                }
                Some(_) if attempt < INITIAL_QUERY_RETRIES => {
                    std::thread::sleep(INITIAL_QUERY_RETRY_DELAY)
                }
                Some(_) => {}
            }
        }
        let Some(mut last_value) = last_value else {
            self.stop(Some(generation));
            return;
        };
        loop {
            std::thread::sleep(POLL_INTERVAL);
            if !self.is_current(generation) {
                return;
            }
            let Some(current) = query_focused_value(pid) else {
                self.stop(Some(generation));
                return;
            };
            if current != last_value {
                last_value = current.clone();
                self.emit_edit(generation, current);
            }
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

/// Reads the focused field of `pid` through System Events. `None` on error.
fn query_focused_value(pid: i32) -> Option<String> {
    let script = format!(
        "tell application \"System Events\"\n\
         \tset targetProc to first application process whose unix id is {pid}\n\
         \tset focAttr to value of attribute \"AXFocusedUIElement\" of targetProc\n\
         \tif focAttr is missing value then return \"\"\n\
         \ttry\n\
         \t\tset val to value of attribute \"AXValue\" of focAttr\n\
         \t\tif val is not missing value and val is not \"\" then return val\n\
         \tend try\n\
         \ttry\n\
         \t\tset charCount to value of attribute \"AXNumberOfCharacters\" of focAttr\n\
         \t\tif charCount is greater than 0 then\n\
         \t\t\treturn value of attribute \"AXSelectedText\" of focAttr\n\
         \t\tend if\n\
         \tend try\n\
         \treturn \"\"\n\
         end tell"
    );
    let output = run_with_timeout(
        std::path::Path::new("/usr/bin/osascript"),
        &["-e", &script],
        None,
        Duration::from_secs(3),
    );
    output.success().then(|| {
        output
            .stdout
            .strip_suffix('\n')
            .unwrap_or(&output.stdout)
            .to_string()
    })
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
        }
    }

    fn no_helper() -> HelperResolver {
        Arc::new(|| None)
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
            ("INITIAL_VALUE:stale", None),
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
        monitor.start_monitoring("pasted text", Duration::from_secs(5), None);
        assert!(!monitor.is_monitoring());
        lock(&monitor.inner.unsupported_pids).insert(42);
        monitor.start_monitoring("pasted text", Duration::from_secs(5), Some(42));
        assert!(!monitor.is_monitoring());
    }

    #[test]
    fn no_element_with_uniform_no_value_teaches_the_cache() {
        let monitor = TextEditMonitor::with_hooks(hooks(
            sh(
                "echo 'Attempt 5/5: Cannot get focused element for PID 4242 (error: -25212)' >&2; echo NO_ELEMENT",
            ),
            Arc::new(|| None),
        ));
        monitor.start_monitoring("pasted text", Duration::from_secs(4), Some(4242));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !lock(&monitor.inner.unsupported_pids).contains(&4242) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
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

    #[test]
    fn edits_are_reported_debounced_with_the_pasted_text() {
        let timers = FakeTimers::new();
        let monitor = TextEditMonitor::build(
            hooks(
                sh(
                    "read line; echo 'INITIAL_VALUE:x'; echo 'CHANGED:hello wrld'; echo \"CHANGED_B64:$(printf 'hello\\nworld' | base64)\"; sleep 1",
                ),
                Arc::new(|| None),
            ),
            timers.clone(),
            Duration::from_millis(1500),
        );
        let edits = monitor.subscribe();
        monitor.start_monitoring("hello world", Duration::from_secs(30), Some(42));
        std::thread::sleep(Duration::from_millis(900));
        timers.advance(1500);
        let edit = edits.try_recv().expect("debounced edit");
        assert_eq!(
            edit,
            TextEdit {
                original_text: "hello world".into(),
                edited_text: "hello\nworld".into()
            }
        );
        assert!(
            edits.try_recv().is_err(),
            "only the latest edit is reported"
        );
        monitor.stop_monitoring();
    }
}

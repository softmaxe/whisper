//! Target-app capture and post-paste edit monitoring via `macos-text-monitor`.
//!
//! - [`TextEditMonitor::capture_target_pid`] names the app holding keyboard
//!   focus at a hotkey press (the Target app), before the overlay steals focus.
//! - [`TextEditMonitor::activate_pid`] / [`TextEditMonitor::can_paste_at_target`]
//!   gate automatic paste.
//! - [`TextEditMonitor::prepare_paste`] anchors a field before Command-V.
//! - [`TextEditMonitor::subscribe`] streams completed edits and read health
//!   while each field remains eligible for five minutes after delivery.
//!
//! Monitoring is strictly read-only: never write AX attributes such as
//! AXEnhancedUserInterface on the target app (it blurs Chromium editors).
//! All methods block (helper spawns, short sleeps); call them off the main
//! thread.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use super::helper::{probe_first_line, resolve_helper};
use super::supervised_child::{
    ChildOptions, Control, Down, HelperSpawner, ProcessSpawner, Protocol, Recovery, RestartPolicy,
    SupervisedChild,
};
use super::timer::{RealTimers, Timers, lock};

const ACTIVATE_CONFIRM_RETRIES: u32 = 6;
const ACTIVATE_CONFIRM_DELAY: Duration = Duration::from_millis(25);
const TARGET_PROBE_TIMEOUT: Duration = Duration::from_millis(700);
/// One Dictation captures its target from several call sites within a few
/// hundred ms; reuse the result across that burst only.
const TARGET_CAPTURE_FRESHNESS: Duration = Duration::from_millis(250);
/// Long enough to read a dictation back and fix it.
pub const MONITOR_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MONITOR_BINARY: &str = "macos-text-monitor";
/// The original dictated field text and its latest completed revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub baseline: String,
    pub edited: String,
    /// Exact delivered UTF-8 byte ranges in the cumulative original baseline.
    pub pasted_ranges: Vec<Range<usize>>,
}

/// A completed field revision or a change in observation availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    pub outcome: ObservationOutcome,
    pub diagnostic: Option<String>,
    pub field: Option<String>,
    /// Stable across repeated dictation in one eligible field lifecycle.
    pub session_id: String,
    /// Increases only for completed edits; an empty diff revokes prior learning.
    pub revision: u64,
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

struct Inner {
    lifecycle: Mutex<()>,
    observations: Mutex<Observations>,
    hooks: MonitorHooks,
    timers: Arc<dyn Timers>,
    capture: Mutex<Capture>,
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
                lifecycle: Mutex::new(()),
                observations: Mutex::new(Observations::default()),
                hooks,
                timers,
                capture: Mutex::new(Capture::default()),
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

    /// Discards all observation state and stops helpers when learning is disabled.
    #[cfg(test)]
    pub fn stop_monitoring(&self) {
        let _lifecycle = lock(&self.inner.lifecycle);
        self.stop_observers();
    }

    /// Gates all future preparations and invalidates tickets already in flight.
    pub fn set_enabled(&self, enabled: bool) {
        let _lifecycle = lock(&self.inner.lifecycle);
        let changed = {
            let mut observations = lock(&self.inner.observations);
            let changed = observations.enabled != enabled;
            observations.enabled = enabled;
            changed
        };
        if changed || !enabled {
            self.stop_observers();
        }
    }

    fn stop_observers(&self) {
        let runs = {
            let mut observations = lock(&self.inner.observations);
            observations.epoch += 1;
            observations.fields.clear();
            observations.pending.clear();
            observations.failures.clear();
            std::mem::take(&mut observations.runs)
        };
        for (_, run) in runs {
            run.stop();
        }
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
}

/// The observation result is independent from whether an edit was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObservationOutcome {
    #[default]
    Readable,
    Unreadable,
    Recovered,
    CompletedEdit,
}

struct Observations {
    enabled: bool,
    epoch: u64,
    runs: std::collections::HashMap<i32, SupervisedChild<ObservationProtocol>>,
    fields: std::collections::HashMap<(i32, String), ObservedField>,
    pending: std::collections::HashMap<u64, async_channel::Sender<NativeObservation>>,
    subscribers: Vec<async_channel::Sender<SessionReport>>,
    next_ticket: u64,
    failures: std::collections::HashSet<(i32, String)>,
}

impl Default for Observations {
    fn default() -> Self {
        Self {
            enabled: true,
            epoch: 0,
            runs: Default::default(),
            fields: Default::default(),
            pending: Default::default(),
            subscribers: Vec::new(),
            next_ticket: 0,
            failures: Default::default(),
        }
    }
}

struct ObservedField {
    app: Option<String>,
    session_id: String,
    revision: u64,
    baseline: String,
    latest: String,
    reported: String,
    pasted_ranges: Vec<Range<usize>>,
    composing: bool,
    health_only: bool,
    alignment_blocked: bool,
    empty: bool,
    unreadable: bool,
    change: u64,
    last_change_at: u64,
    expiry: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct NativeObservation {
    kind: String,
    #[serde(default)]
    ticket: u64,
    #[serde(default)]
    field: String,
    value: Option<String>,
    #[serde(default)]
    start: usize,
    #[serde(default)]
    length: usize,
    #[serde(default)]
    composing: bool,
    reason: Option<String>,
}

/// A target field anchored before Command-V. Dropping an uncommitted ticket
/// cancels it. A failed preparation never prevents the ordinary paste.
pub struct PreparedPaste {
    monitor: TextEditMonitor,
    pid: i32,
    ticket: u64,
    epoch: u64,
    prepared: NativeObservation,
    text: String,
    finished: bool,
}

impl PreparedPaste {
    pub fn commit(mut self, pasted: bool) {
        let committed = pasted
            && self.monitor.inner.commit_observation(
                self.pid,
                self.ticket,
                self.epoch,
                &self.prepared,
                &self.text,
            );
        self.monitor.inner.observation_command(
            self.pid,
            serde_json::json!({
                "command": if committed { "commit" } else { "cancel" }, "ticket": self.ticket
            }),
        );
        self.finished = true;
    }
}

impl Drop for PreparedPaste {
    fn drop(&mut self) {
        if !self.finished {
            self.monitor.inner.observation_command(
                self.pid,
                serde_json::json!({
                    "command": "cancel", "ticket": self.ticket
                }),
            );
        }
    }
}

impl TextEditMonitor {
    /// Each subscriber receives all reports; receivers do not steal each
    /// other's messages. Install the application subscriber before pasting.
    pub fn subscribe(&self) -> async_channel::Receiver<SessionReport> {
        let (sender, receiver) = async_channel::unbounded();
        lock(&self.inner.observations).subscribers.push(sender);
        receiver
    }

    /// Binds the intended AX element and selection before the paste event.
    /// This bounded operation belongs inside the serialized paste queue.
    pub fn prepare_paste(&self, text: &str, pid: Option<i32>) -> Option<PreparedPaste> {
        let pid = pid.filter(|pid| *pid > 0)?;
        let epoch = {
            let observations = lock(&self.inner.observations);
            if !observations.enabled {
                return None;
            }
            observations.epoch
        };
        if self.inner.ensure_observer(pid, epoch).is_none() {
            self.inner
                .observation_failure(pid, None, "helper_unavailable");
            return None;
        }
        let (sender, receiver) = async_channel::bounded(1);
        let ticket = {
            let mut observations = lock(&self.inner.observations);
            observations.next_ticket += 1;
            let ticket = observations.next_ticket;
            observations.pending.insert(ticket, sender);
            ticket
        };
        if !self.inner.observation_command(
            pid,
            serde_json::json!({"command":"prepare", "ticket":ticket}),
        ) {
            lock(&self.inner.observations).pending.remove(&ticket);
            self.inner
                .observation_failure(pid, None, "helper_command_unavailable");
            return None;
        }
        // recv_blocking has no deadline; use a condition-independent bounded
        // wait so an unresponsive AX server cannot hold the paste queue.
        let deadline = Instant::now() + TARGET_PROBE_TIMEOUT;
        let prepared = loop {
            match receiver.try_recv() {
                Ok(prepared) => break Some(prepared),
                Err(async_channel::TryRecvError::Closed) => break None,
                Err(async_channel::TryRecvError::Empty) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break None,
            }
        };
        lock(&self.inner.observations).pending.remove(&ticket);
        {
            let observations = lock(&self.inner.observations);
            if !observations.enabled || observations.epoch != epoch {
                return None;
            }
        }
        let Some(prepared) = prepared else {
            self.inner.observation_command(
                pid,
                serde_json::json!({"command":"cancel", "ticket":ticket}),
            );
            self.inner
                .observation_failure(pid, None, "ax_prepare_timeout");
            return None;
        };
        if prepared.value.is_none() {
            self.inner.observation_failure(
                pid,
                (!prepared.field.is_empty()).then_some(prepared.field.clone()),
                prepared.reason.as_deref().unwrap_or("ax_text_unavailable"),
            );
            self.inner.observation_command(
                pid,
                serde_json::json!({"command":"health", "ticket":ticket}),
            );
            return None;
        }
        Some(PreparedPaste {
            monitor: self.clone(),
            pid,
            ticket,
            epoch,
            prepared,
            text: text.into(),
            finished: false,
        })
    }

    /// Synchronizes helper output before the next dictation reads learned
    /// data. This does not turn unfinished composition into a correction.
    pub fn flush_completed_edits(&self) {
        let runs: Vec<_> = lock(&self.inner.observations)
            .runs
            .keys()
            .copied()
            .collect();
        for pid in runs {
            let (sender, receiver) = async_channel::bounded(1);
            let ticket = {
                let mut observations = lock(&self.inner.observations);
                observations.next_ticket += 1;
                let ticket = observations.next_ticket;
                observations.pending.insert(ticket, sender);
                ticket
            };
            self.inner.observation_command(
                pid,
                serde_json::json!({"command":"snapshot", "ticket":ticket}),
            );
            let deadline = Instant::now() + Duration::from_millis(250);
            while receiver.try_recv().is_err() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            lock(&self.inner.observations).pending.remove(&ticket);
        }
    }
}

impl Inner {
    fn observation_failure(&self, pid: i32, field: Option<String>, reason: &str) {
        if !lock(&self.observations).enabled {
            return;
        }
        let key = (pid, field.clone().unwrap_or_default());
        let notify = {
            let mut observations = lock(&self.observations);
            if field.is_some() && !observations.fields.contains_key(&key) {
                observations.fields.insert(
                    key.clone(),
                    ObservedField {
                        app: appkit::bundle_identifier(pid),
                        session_id: uuid::Uuid::new_v4().to_string(),
                        revision: 0,
                        baseline: String::new(),
                        latest: String::new(),
                        reported: String::new(),
                        pasted_ranges: Vec::new(),
                        composing: false,
                        health_only: true,
                        alignment_blocked: false,
                        empty: false,
                        unreadable: true,
                        change: 0,
                        last_change_at: self.timers.now_ms(),
                        expiry: 0,
                    },
                );
            }
            if let Some(observed) = observations.fields.get_mut(&key) {
                observed.unreadable = true;
            }
            observations.failures.insert(key.clone())
        };
        if !notify {
            return;
        }
        if field.is_none() {
            let epoch = lock(&self.observations).epoch;
            self.retry_observation_health(
                pid,
                epoch,
                self.timers
                    .now_ms()
                    .saturating_add(MONITOR_TIMEOUT.as_millis() as u64),
            );
        }
        let report = self
            .field_report(
                &key,
                ObservationOutcome::Unreadable,
                Some(reason.into()),
                false,
            )
            .unwrap_or(SessionReport {
                app: appkit::bundle_identifier(pid),
                readable: false,
                edit: None,
                outcome: ObservationOutcome::Unreadable,
                diagnostic: Some(reason.into()),
                field,
                session_id: String::new(),
                revision: 0,
            });
        self.publish_observation(report);
    }

    fn retry_observation_health(&self, pid: i32, epoch: u64, deadline: u64) {
        let weak = self.this.clone();
        self.timers.schedule(
            Duration::from_secs(1),
            Box::new(move || {
                let Some(inner) = weak.upgrade() else { return };
                {
                    let observations = lock(&inner.observations);
                    if !observations.enabled
                        || observations.epoch != epoch
                        || !observations.failures.contains(&(pid, String::new()))
                        || inner.timers.now_ms() >= deadline
                    {
                        return;
                    }
                }
                if inner.ensure_observer(pid, epoch).is_some()
                    && inner.observation_command(
                        pid,
                        serde_json::json!({"command":"health", "ticket":0}),
                    )
                {
                    return;
                }
                inner.retry_observation_health(pid, epoch, deadline);
            }),
        );
    }

    fn publish_observation(&self, report: SessionReport) {
        lock(&self.observations)
            .subscribers
            .retain(|sender| sender.try_send(report.clone()).is_ok());
    }

    fn observation_command(&self, pid: i32, command: serde_json::Value) -> bool {
        let run = lock(&self.observations).runs.get(&pid).cloned();
        run.is_some_and(|run| run.update(|_, cx| cx.write_line(&command.to_string())))
    }

    fn ensure_observer(&self, pid: i32, epoch: u64) -> Option<()> {
        let _lifecycle = lock(&self.lifecycle);
        {
            let observations = lock(&self.observations);
            if !observations.enabled || observations.epoch != epoch {
                return None;
            }
        }
        if lock(&self.observations).runs.contains_key(&pid) {
            return Some(());
        }
        let helper = (self.hooks.resolve_helper)()?;
        let mut args = helper.args;
        args.extend(["--observe".into(), pid.to_string()]);
        let weak = self.this.clone();
        let run = SupervisedChild::new(
            ChildOptions {
                label: "Field observer",
                resolve_binary: Arc::new(move || Some(helper.program.clone())),
                spawner: self.hooks.spawner.clone(),
                timers: self.timers.clone(),
                restart: RestartPolicy::Never,
                heartbeat: None,
                max_line: 8 * 1024 * 1024,
            },
            ObservationProtocol { args },
            Arc::new(move |event| {
                if let Some(inner) = weak.upgrade() {
                    let current = {
                        let observations = lock(&inner.observations);
                        observations.enabled && observations.epoch == epoch
                    };
                    if current {
                        inner.observe_native(pid, event);
                    }
                }
            }),
        );
        lock(&self.observations).runs.insert(pid, run.clone());
        run.start();
        Some(())
    }

    fn commit_observation(
        &self,
        pid: i32,
        ticket: u64,
        epoch: u64,
        prepared: &NativeObservation,
        text: &str,
    ) -> bool {
        let _lifecycle = lock(&self.lifecycle);
        {
            let observations = lock(&self.observations);
            if !observations.enabled || observations.epoch != epoch {
                return false;
            }
        }
        let key = (pid, prepared.field.clone());
        let pre = prepared.value.as_deref().unwrap_or_default();
        let Some(selected) = utf16_range(pre, prepared.start, prepared.length) else {
            self.observation_failure(
                pid,
                Some(key.1.clone()),
                "ax_selection_not_on_text_boundaries",
            );
            return false;
        };
        let predicted = pre[..selected.start].to_string() + text + &pre[selected.end..];
        let (expiry, change, recovered) = {
            let mut observations = lock(&self.observations);
            if observations
                .fields
                .get(&key)
                .is_some_and(|field| field.health_only)
            {
                observations.fields.remove(&key);
            }
            let recovered = observations.failures.remove(&key)
                | observations.failures.remove(&(pid, String::new()));
            let field = observations
                .fields
                .entry(key.clone())
                .or_insert_with(|| ObservedField {
                    app: appkit::bundle_identifier(pid),
                    session_id: uuid::Uuid::new_v4().to_string(),
                    revision: 0,
                    baseline: pre.into(),
                    latest: pre.into(),
                    reported: pre.into(),
                    pasted_ranges: Vec::new(),
                    composing: false,
                    health_only: false,
                    alignment_blocked: false,
                    empty: false,
                    unreadable: false,
                    change: 0,
                    last_change_at: self.timers.now_ms(),
                    expiry: 0,
                });
            if field.alignment_blocked {
                return false;
            }
            let selections = aligned_range(pre, &field.baseline, selected.clone())
                .zip(aligned_range(pre, &field.reported, selected.clone()));
            let Some((baseline_range, reported_range)) = selections else {
                // Preserve the preceding scope instead of inventing a location
                // for this delivered text. Further values cannot safely be
                // attributed until the field lifecycle is reset.
                field.alignment_blocked = true;
                field.change += 1;
                drop(observations);
                self.observation_failure(
                    pid,
                    Some(key.1.clone()),
                    "text_alignment_budget_or_ambiguity",
                );
                return false;
            };
            field.pasted_ranges =
                replaced_regions(&field.pasted_ranges, baseline_range.clone(), text.len());
            if !text.is_empty() {
                field
                    .pasted_ranges
                    .push(baseline_range.start..baseline_range.start + text.len());
            }
            field.baseline.replace_range(baseline_range, text);
            field.reported.replace_range(reported_range, text);
            let recovered = recovered || field.unreadable;
            field.unreadable = false;
            field.latest = predicted;
            field.empty = false;
            field.change += 1;
            field.last_change_at = self.timers.now_ms();
            field.expiry = ticket;
            (field.expiry, field.change, recovered)
        };
        let stable_key = key.clone();
        let weak = self.this.clone();
        self.timers.schedule(
            Duration::from_secs(1),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.complete_observation(&stable_key, Some(change));
                }
            }),
        );
        self.publish_observation(
            self.field_report(
                &key,
                if recovered {
                    ObservationOutcome::Recovered
                } else {
                    ObservationOutcome::Readable
                },
                None,
                false,
            )
            .unwrap(),
        );
        let weak = self.this.clone();
        self.timers.schedule(
            MONITOR_TIMEOUT,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut observations = lock(&inner.observations);
                    if observations
                        .fields
                        .get(&key)
                        .is_some_and(|field| field.expiry == expiry)
                    {
                        observations.fields.remove(&key);
                        drop(observations);
                        inner.observation_command(
                            pid,
                            serde_json::json!({"command":"forget", "field":key.1}),
                        );
                    }
                }
            }),
        );
        true
    }

    fn field_report(
        &self,
        key: &(i32, String),
        outcome: ObservationOutcome,
        diagnostic: Option<String>,
        edited: bool,
    ) -> Option<SessionReport> {
        let observations = lock(&self.observations);
        let field = observations.fields.get(key)?;
        Some(SessionReport {
            app: field.app.clone(),
            readable: !field.unreadable,
            edit: edited.then(|| TextEdit {
                baseline: field.baseline.clone(),
                edited: field.latest.clone(),
                pasted_ranges: field.pasted_ranges.clone(),
            }),
            outcome,
            diagnostic,
            field: Some(key.1.clone()),
            session_id: field.session_id.clone(),
            revision: field.revision,
        })
    }

    fn complete_observation(&self, key: &(i32, String), expected_change: Option<u64>) {
        let report = {
            let mut observations = lock(&self.observations);
            let Some(field) = observations.fields.get_mut(key) else {
                return;
            };
            if field.health_only
                || field.alignment_blocked
                || field.composing
                || (field.empty && expected_change.is_some())
                || field.unreadable
                || field.latest.trim().is_empty()
                || expected_change.is_some_and(|change| field.change != change)
                || field.reported == field.latest
            {
                return;
            }
            field.reported = field.latest.clone();
            field.revision += 1;
            // Capture exactly the completed revision under the same lock: a
            // new composing value must never replace the text being reported.
            SessionReport {
                app: field.app.clone(),
                readable: true,
                outcome: ObservationOutcome::CompletedEdit,
                diagnostic: None,
                field: Some(key.1.clone()),
                session_id: field.session_id.clone(),
                revision: field.revision,
                edit: Some(TextEdit {
                    baseline: field.baseline.clone(),
                    edited: field.latest.clone(),
                    pasted_ranges: field.pasted_ranges.clone(),
                }),
            }
        };
        // An empty diff revokes the preceding contribution after a restoration.
        self.publish_observation(report);
    }

    fn observe_native(&self, pid: i32, event: NativeObservation) {
        if event.kind == "health_recovered" {
            if lock(&self.observations)
                .failures
                .remove(&(pid, String::new()))
            {
                self.publish_observation(SessionReport {
                    app: appkit::bundle_identifier(pid),
                    readable: true,
                    edit: None,
                    outcome: ObservationOutcome::Recovered,
                    diagnostic: None,
                    field: None,
                    session_id: String::new(),
                    revision: 0,
                });
            }
            return;
        }
        if event.kind == "helper_ended" {
            lock(&self.observations).runs.remove(&pid);
            let keys: Vec<_> = lock(&self.observations)
                .fields
                .keys()
                .filter(|key| key.0 == pid)
                .cloned()
                .collect();
            for key in keys {
                self.observe_native(
                    pid,
                    NativeObservation {
                        kind: "unreadable".into(),
                        field: key.1,
                        ..event.clone()
                    },
                );
            }
            return;
        }
        if event.kind == "synced" {
            let stable: Vec<_> = lock(&self.observations)
                .fields
                .iter()
                .filter(|(key, field)| {
                    key.0 == pid
                        && self.timers.now_ms().saturating_sub(field.last_change_at) >= 1_000
                })
                .map(|(key, field)| (key.clone(), field.change))
                .collect();
            for (key, change) in stable {
                self.complete_observation(&key, Some(change));
            }
        }
        if event.kind == "prepared" || event.kind == "synced" {
            if let Some(sender) = lock(&self.observations).pending.remove(&event.ticket) {
                let _ = sender.try_send(event);
            }
            return;
        }
        let key = (pid, event.field.clone());
        if event.kind == "sent" {
            self.complete_observation(&key, None);
            lock(&self.observations).fields.remove(&key);
            return;
        }
        if event.kind == "closed" {
            lock(&self.observations).fields.remove(&key);
            return;
        }
        if event.kind == "unreadable" {
            let notify = {
                let mut observations = lock(&self.observations);
                if let Some(field) = observations.fields.get_mut(&key) {
                    let notify = !field.unreadable;
                    field.unreadable = true;
                    notify
                } else {
                    false
                }
            };
            if notify
                && let Some(report) =
                    self.field_report(&key, ObservationOutcome::Unreadable, event.reason, false)
            {
                self.publish_observation(report);
            }
            return;
        }
        let Some(value) = event.value else { return };
        let (change, recovered) = {
            let mut observations = lock(&self.observations);
            let Some(field) = observations.fields.get_mut(&key) else {
                return;
            };
            if field.alignment_blocked {
                return;
            }
            let recovered = field.unreadable;
            field.unreadable = false;
            // Empty values are never evidence of submission. The helper sends
            // a separate sent event after a key/mouse send gesture and clear.
            if value.trim().is_empty() {
                if !field.empty {
                    field.change += 1;
                    field.last_change_at = self.timers.now_ms();
                }
                field.empty = true;
                (None, recovered)
            } else {
                let was_empty = field.empty;
                field.empty = false;
                if value == field.latest
                    && event.composing == field.composing
                    && !recovered
                    && !was_empty
                {
                    return;
                }
                field.latest = value;
                field.composing = event.composing;
                field.change += 1;
                field.last_change_at = self.timers.now_ms();
                (Some(field.change), recovered)
            }
        };
        if recovered {
            lock(&self.observations).failures.remove(&key);
        }
        if recovered
            && let Some(report) =
                self.field_report(&key, ObservationOutcome::Recovered, None, false)
        {
            self.publish_observation(report);
        }
        let Some(change) = change else { return };
        let weak = self.this.clone();
        self.timers.schedule(
            Duration::from_secs(1),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.complete_observation(&key, Some(change));
                }
            }),
        );
    }
}

fn utf16_range(value: &str, start: usize, length: usize) -> Option<Range<usize>> {
    fn offset(value: &str, requested: usize) -> Option<usize> {
        let mut units = 0;
        for (byte, character) in value.char_indices() {
            if units == requested {
                return Some(byte);
            }
            units += character.len_utf16();
            if units > requested {
                return None;
            }
        }
        (units == requested).then_some(value.len())
    }
    Some(offset(value, start)?..offset(value, start.checked_add(length)?)?)
}

fn replaced_regions(
    regions: &[Range<usize>],
    replaced: Range<usize>,
    inserted: usize,
) -> Vec<Range<usize>> {
    let mut updated = Vec::new();
    for region in regions {
        let left_end = region.end.min(replaced.start);
        if region.start < left_end {
            updated.push(region.start..left_end);
        }
        let right_start = region.start.max(replaced.end);
        if right_start < region.end {
            let start = replaced.start + inserted + right_start - replaced.end;
            let end = replaced.start + inserted + region.end - replaced.end;
            updated.push(start..end);
        }
    }
    updated
}

/// Map boundaries through every optimal character edit alignment. A bounded
/// band makes small corrections in long editors cheap without greedily
/// anchoring repeated prefixes/suffixes to an arbitrary occurrence.
fn aligned_range(from: &str, to: &str, selected: Range<usize>) -> Option<Range<usize>> {
    const CELL_BUDGET: usize = 1_000_000;
    const WORK_BUDGET: usize = 2 * CELL_BUDGET;
    const UNREACHABLE: u32 = u32::MAX / 4;
    struct Grid {
        rows: usize,
        columns: usize,
        width: usize,
        band: usize,
        costs: Vec<u32>,
    }
    impl Grid {
        fn index(&self, row: usize, column: usize) -> Option<usize> {
            if row >= self.rows || column >= self.columns || row.abs_diff(column) > self.band {
                return None;
            }
            let offset = if self.width == self.columns {
                column
            } else {
                column + self.band - row
            };
            Some(row * self.width + offset)
        }
        fn cost(&self, row: usize, column: usize) -> u32 {
            self.index(row, column)
                .map_or(UNREACHABLE, |index| self.costs[index])
        }
    }
    fn characters(value: &str) -> (Vec<char>, Vec<usize>) {
        let (characters, mut offsets): (Vec<_>, Vec<_>) = value
            .char_indices()
            .map(|(offset, character)| (character, offset))
            .unzip();
        offsets.push(value.len());
        (characters, offsets)
    }
    if selected.start > selected.end
        || selected.end > from.len()
        || !from.is_char_boundary(selected.start)
        || !from.is_char_boundary(selected.end)
    {
        return None;
    }
    if from == to {
        return Some(selected);
    }
    // An emptied field supplies no anchor for a position in its old text.
    if from.is_empty() {
        return None;
    }
    // Physical field edges do not depend on the alignment of interior edits.
    // This also keeps appending to a long corrected field within paste latency.
    let edge = |offset| {
        if offset == 0 {
            Some(0)
        } else if offset == from.len() {
            Some(to.len())
        } else {
            None
        }
    };
    if let (Some(start), Some(end)) = (edge(selected.start), edge(selected.end)) {
        return Some(start..end);
    }
    let (source, source_offsets) = characters(from);
    let (target, target_offsets) = characters(to);
    let boundaries = [
        source_offsets.binary_search(&selected.start).ok()?,
        source_offsets.binary_search(&selected.end).ok()?,
    ];
    let mut band = source.len().abs_diff(target.len()).max(1);
    let mut work = WORK_BUDGET;
    let grid = loop {
        let rows = source.len().checked_add(1)?;
        let columns = target.len().checked_add(1)?;
        let width = band.checked_mul(2)?.checked_add(1)?.min(columns);
        let cells = rows.checked_mul(width)?;
        if cells > CELL_BUDGET {
            return None;
        }
        work = work.checked_sub(cells)?;
        let mut grid = Grid {
            rows,
            columns,
            width,
            band,
            costs: vec![UNREACHABLE; cells],
        };
        for row in 0..rows {
            let first = row.saturating_sub(band);
            let last = row.saturating_add(band).min(target.len());
            for column in first..=last {
                let cost = if row == 0 {
                    column as u32
                } else if column == 0 {
                    row as u32
                } else {
                    (grid.cost(row - 1, column) + 1)
                        .min(grid.cost(row, column - 1) + 1)
                        .min(
                            grid.cost(row - 1, column - 1)
                                + u32::from(source[row - 1] != target[column - 1]),
                        )
                };
                let index = grid.index(row, column)?;
                grid.costs[index] = cost;
            }
        }
        // Every path of cost D stays within a band of width D. Once D fits,
        // this matrix includes all globally optimal paths, including ties.
        if grid.cost(source.len(), target.len()) as usize <= band {
            break grid;
        }
        band = band.checked_mul(2)?.min(source.len().max(target.len()));
    };
    let mut mapped = [None, None];
    for (index, &boundary) in boundaries.iter().enumerate() {
        if boundary == 0 {
            mapped[index] = Some(0);
        } else if boundary == source.len() {
            mapped[index] = Some(target.len());
        }
    }
    let mut visited = vec![false; grid.costs.len()];
    let mut pending = vec![(source.len(), target.len())];
    visited[grid.index(source.len(), target.len())?] = true;
    while let Some((row, column)) = pending.pop() {
        for index in 0..2 {
            if boundaries[index] > 0 && boundaries[index] < source.len() && row == boundaries[index]
            {
                if mapped[index].is_some_and(|previous| previous != column) {
                    return None;
                }
                mapped[index] = Some(column);
            }
        }
        let cost = grid.cost(row, column);
        let mut visit = |row: usize, column: usize| {
            if let Some(index) = grid.index(row, column)
                && !visited[index]
            {
                visited[index] = true;
                pending.push((row, column));
            }
        };
        if row > 0 && cost == grid.cost(row - 1, column) + 1 {
            visit(row - 1, column);
        }
        if column > 0 && cost == grid.cost(row, column - 1) + 1 {
            visit(row, column - 1);
        }
        if row > 0
            && column > 0
            && cost
                == grid.cost(row - 1, column - 1) + u32::from(source[row - 1] != target[column - 1])
        {
            visit(row - 1, column - 1);
        }
    }
    let start = target_offsets[mapped[0]?];
    let end = target_offsets[mapped[1]?];
    (start <= end).then_some(start..end)
}

struct ObservationProtocol {
    args: Vec<String>,
}
impl Protocol for ObservationProtocol {
    type Event = NativeObservation;
    fn args(&self) -> Vec<String> {
        self.args.clone()
    }
    fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, NativeObservation>) {
        if let Ok(event) = serde_json::from_str(line) {
            cx.emit(event);
        }
    }
    fn on_down(
        &mut self,
        down: &Down,
        _recovery: Recovery,
        cx: &mut Control<'_, NativeObservation>,
    ) {
        if !matches!(down, Down::Stopped) {
            cx.emit(NativeObservation {
                kind: "helper_ended".into(),
                ticket: 0,
                field: String::new(),
                value: None,
                start: 0,
                length: 0,
                composing: false,
                reason: Some(format!("helper_ended: {down:?}")),
            });
        }
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    fn prepared_observation_captures_a_correction_in_the_first_second() {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let monitor = TextEditMonitor::build(fake_hooks(&spawner), timers.clone());
        let reports = monitor.subscribe();
        let other = monitor.clone();
        let preparing = std::thread::spawn(move || other.prepare_paste("hello world", Some(42)));
        wait_for_spawn(&spawner);
        let deadline = Instant::now() + Duration::from_secs(2);
        while lock(&spawner.child(0)).writes.is_empty() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        spawner.emit(
            0,
            ChildEvent::Stdout(
                r#"{"kind":"prepared","ticket":1,"field":"a","value":"","start":0,"length":0}"#
                    .to_string()
                    + "\n",
            ),
        );
        let ticket = preparing.join().unwrap().expect("bound before paste");
        ticket.commit(true);
        spawner.emit(
            0,
            ChildEvent::Stdout(
                r#"{"kind":"value","field":"a","value":"hello World","composing":false}"#
                    .to_string()
                    + "\n",
            ),
        );
        timers.advance(1_000);
        let report = std::iter::from_fn(|| reports.try_recv().ok())
            .find(|report| report.edit.is_some())
            .unwrap();
        assert_eq!(
            report.edit.unwrap(),
            TextEdit {
                baseline: "hello world".into(),
                edited: "hello World".into(),
                pasted_ranges: std::iter::once(0..11).collect(),
            }
        );
        monitor.stop_monitoring();
    }

    struct ObservationFixture {
        monitor: TextEditMonitor,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
        reports: async_channel::Receiver<SessionReport>,
    }

    impl ObservationFixture {
        fn new() -> Self {
            let spawner = Arc::new(FakeSpawner::default());
            let timers = FakeTimers::new();
            let monitor = TextEditMonitor::build(fake_hooks(&spawner), timers.clone());
            let reports = monitor.subscribe();
            Self {
                monitor,
                spawner,
                timers,
                reports,
            }
        }

        fn paste(&self, field: &str, pre: &str, text: &str) {
            self.prepare(field, Some(pre), text).unwrap().commit(true);
        }

        fn prepare(&self, field: &str, pre: Option<&str>, text: &str) -> Option<PreparedPaste> {
            self.prepare_at(
                field,
                pre,
                text,
                pre.unwrap_or_default().encode_utf16().count(),
                0,
            )
        }

        fn prepare_at(
            &self,
            field: &str,
            pre: Option<&str>,
            text: &str,
            start: usize,
            length: usize,
        ) -> Option<PreparedPaste> {
            let previous_count = self.spawner.count();
            let previous = if previous_count > 0 {
                lock(&self.spawner.child(previous_count - 1)).writes.len()
            } else {
                0
            };
            let monitor = self.monitor.clone();
            let text = text.to_string();
            let preparing = std::thread::spawn(move || monitor.prepare_paste(&text, Some(42)));
            wait_for_spawn(&self.spawner);
            let deadline = Instant::now() + Duration::from_secs(2);
            let ticket = loop {
                let count = self.spawner.count();
                let offset = if count == previous_count { previous } else { 0 };
                if let Some(command) = lock(&self.spawner.child(count - 1)).writes.get(offset) {
                    let command: serde_json::Value = serde_json::from_str(command).unwrap();
                    break command["ticket"].as_u64().unwrap();
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            };
            self.emit(
                serde_json::json!({"kind":"prepared", "ticket":ticket, "field":field,
                "value":pre, "start":start, "length":length}),
            );
            preparing.join().unwrap()
        }

        fn emit(&self, event: serde_json::Value) {
            self.spawner.emit(
                self.spawner.count() - 1,
                ChildEvent::Stdout(event.to_string() + "\n"),
            );
        }

        fn edits(&self) -> Vec<SessionReport> {
            std::iter::from_fn(|| self.reports.try_recv().ok())
                .filter(|report| report.outcome == ObservationOutcome::CompletedEdit)
                .collect()
        }
    }

    impl Drop for ObservationFixture {
        fn drop(&mut self) {
            self.monitor.stop_monitoring();
        }
    }

    #[test]
    fn temporary_emptiness_interrupts_completion_until_replacement_is_stable() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello World"}));
        fixture.timers.advance(500);
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":""}));
        fixture.timers.advance(1_000);
        assert!(
            fixture.edits().is_empty(),
            "select-delete must not finalize the previous candidate"
        );
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello WORLD"}));
        fixture.timers.advance(1_000);
        assert_eq!(
            fixture.edits()[0].edit.as_ref().unwrap().edited,
            "hello WORLD"
        );
    }

    #[test]
    fn unreadable_observation_recovers_once_without_losing_its_field() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world");
        fixture.edits();
        for _ in 0..2 {
            fixture.emit(serde_json::json!({"kind":"unreadable", "field":"a", "reason":"ax_cannot_complete"}));
        }
        let failure = fixture.reports.try_recv().unwrap();
        assert_eq!(failure.outcome, ObservationOutcome::Unreadable);
        assert!(fixture.reports.try_recv().is_err());
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":""}));
        let recovered = fixture
            .reports
            .try_recv()
            .expect("even an empty field is a successful read");
        assert_eq!(recovered.outcome, ObservationOutcome::Recovered);
        assert_eq!(recovered.session_id, failure.session_id);
    }

    #[test]
    fn a_new_dictation_preserves_an_unfinished_correction_to_the_earlier_passage() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello World"}));
        fixture.timers.advance(500);
        fixture.paste("a", "hello World", " again");
        fixture.timers.advance(1_000);
        let edits = fixture.edits();
        assert_eq!(
            edits.len(),
            1,
            "the second dictation must not erase the pending earlier correction"
        );
        assert_eq!(
            edits[0].edit.as_ref().unwrap(),
            &TextEdit {
                baseline: "hello world again".into(),
                edited: "hello World again".into(),
                pasted_ranges: vec![0..11, 11..17],
            }
        );
    }

    #[test]
    fn synchronizing_flush_publishes_an_already_stable_edit_before_returning() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello World"}));
        fixture.timers.advance_without_running(1_000);
        let previous = lock(&fixture.spawner.child(0)).writes.len();
        let monitor = fixture.monitor.clone();
        let flushing = std::thread::spawn(move || monitor.flush_completed_edits());
        let deadline = Instant::now() + Duration::from_secs(2);
        let ticket = loop {
            if let Some(command) = lock(&fixture.spawner.child(0)).writes.get(previous) {
                let command: serde_json::Value = serde_json::from_str(command).unwrap();
                break command["ticket"].as_u64().unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        fixture.emit(serde_json::json!({"kind":"synced", "ticket":ticket}));
        flushing.join().unwrap();
        assert_eq!(
            fixture.edits().len(),
            1,
            "a delayed timer must not hide a completed edit at capture"
        );
    }

    #[test]
    fn initially_unreadable_field_recovers_without_learning_unknown_old_content() {
        let fixture = ObservationFixture::new();
        assert!(fixture.prepare("a", None, "hello world").is_none());
        assert_eq!(
            fixture.reports.try_recv().unwrap().outcome,
            ObservationOutcome::Unreadable
        );
        assert!(fixture.prepare("a", None, "hello world").is_none());
        assert!(
            fixture.reports.try_recv().is_err(),
            "one unreadable hint per episode"
        );
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"unrelated old draft hello World"}));
        assert_eq!(
            fixture.reports.try_recv().unwrap().outcome,
            ObservationOutcome::Recovered
        );
        fixture.timers.advance(1_000);
        assert!(
            fixture.edits().is_empty(),
            "unknown baseline cannot support attribution"
        );
        fixture.paste("a", "unrelated old draft hello World", " again");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"unrelated old draft hello World AGAIN"}));
        fixture.timers.advance(1_000);
        let edits = fixture.edits();
        assert_eq!(
            edits[0].edit.as_ref().unwrap().pasted_ranges,
            std::iter::once(31..37).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fields_keep_independent_completed_revisions_and_flush_an_immediate_send() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world");
        fixture.paste("b", "", "second field");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello World", "composing":true}));
        fixture.emit(serde_json::json!({"kind":"value", "field":"b", "value":"second Field"}));
        fixture.timers.advance(1_000);
        let first = fixture.edits();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].field.as_deref(), Some("b"));
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello World", "composing":false}));
        fixture.timers.advance(1_000);
        let a = fixture.edits().remove(0);
        assert_ne!(a.session_id, first[0].session_id);
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello world"}));
        fixture.timers.advance(1_000);
        let restored = fixture.edits().remove(0);
        assert_eq!(restored.session_id, a.session_id);
        assert_eq!(restored.revision, 2);
        assert_eq!(
            restored.edit.as_ref().unwrap().baseline,
            restored.edit.as_ref().unwrap().edited
        );
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"hello WORLD"}));
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"\n"}));
        fixture.emit(serde_json::json!({"kind":"sent", "field":"a"}));
        let sent = fixture.edits().remove(0);
        assert_eq!(sent.revision, 3);
        assert_eq!(sent.edit.unwrap().edited, "hello WORLD");
        fixture.emit(
            serde_json::json!({"kind":"value", "field":"a", "value":"unrelated next message"}),
        );
        fixture.timers.advance(1_000);
        assert!(fixture.edits().is_empty());
    }

    #[test]
    fn disabling_observation_invalidates_prepared_tickets_and_reenable_starts_fresh() {
        let fixture = ObservationFixture::new();
        let ticket = fixture.prepare("a", Some(""), "hello world").unwrap();
        fixture.monitor.set_enabled(false);
        ticket.commit(true);
        assert!(fixture.monitor.prepare_paste("ignored", Some(42)).is_none());
        fixture.timers.advance(1_000);
        assert!(fixture.edits().is_empty());
        fixture.monitor.set_enabled(true);
        fixture.paste("a", "", "new passage");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"new Passage"}));
        fixture.timers.advance(1_000);
        let edits = fixture.edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].edit.as_ref().unwrap().pasted_ranges,
            std::iter::once(0..11).collect::<Vec<_>>()
        );
    }

    #[test]
    fn canceled_delivery_does_not_create_a_learning_scope() {
        let fixture = ObservationFixture::new();
        fixture
            .prepare("a", Some("old draft"), "ignored")
            .unwrap()
            .commit(false);
        fixture
            .emit(serde_json::json!({"kind":"value", "field":"a", "value":"unrelated correction"}));
        fixture.timers.advance(1_000);
        assert!(fixture.edits().is_empty());
    }

    #[test]
    fn missing_helper_retries_and_reports_recovery_without_another_paste() {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let available = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let readable = available.clone();
        let mut hooks = fake_hooks(&spawner);
        hooks.resolve_helper = Arc::new(move || {
            readable.load(Ordering::SeqCst).then(|| HelperCommand {
                program: PathBuf::from("/fake/observer"),
                args: Vec::new(),
            })
        });
        let monitor = TextEditMonitor::build(hooks, timers.clone());
        let reports = monitor.subscribe();
        assert!(monitor.prepare_paste("hello world", Some(42)).is_none());
        assert_eq!(
            reports.try_recv().unwrap().diagnostic.as_deref(),
            Some("helper_unavailable")
        );
        assert!(monitor.prepare_paste("hello world", Some(42)).is_none());
        assert!(reports.try_recv().is_err());
        available.store(true, Ordering::SeqCst);
        timers.advance(1_000);
        spawner.emit(
            0,
            ChildEvent::Stdout("{\"kind\":\"health_recovered\"}\n".into()),
        );
        assert_eq!(
            reports.try_recv().unwrap().outcome,
            ObservationOutcome::Recovered
        );
        timers.advance(1_000);
        assert!(reports.try_recv().is_err());
        monitor.stop_monitoring();
    }

    #[test]
    fn disabling_during_preparation_cancels_the_wait_without_a_stale_scope() {
        let fixture = ObservationFixture::new();
        let monitor = fixture.monitor.clone();
        let preparing = std::thread::spawn(move || monitor.prepare_paste("hello world", Some(42)));
        wait_for_spawn(&fixture.spawner);
        fixture.monitor.set_enabled(false);
        assert!(preparing.join().unwrap().is_none());
        fixture.timers.advance(1_000);
        assert!(fixture.edits().is_empty());
    }

    #[test]
    fn a_correction_near_a_long_editor_tail_keeps_its_original_context() {
        let fixture = ObservationFixture::new();
        let context = "existing context 😀 ".repeat(1_024);
        fixture.paste("a", &context, "hello world");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":format!("{context}hello World")}));
        fixture.timers.advance(1_000);
        let edits = fixture.edits();
        let edit = edits[0].edit.as_ref().unwrap();
        assert_eq!(edit.baseline, format!("{context}hello world"));
        assert_eq!(edit.edited, format!("{context}hello World"));
        assert_eq!(
            edit.pasted_ranges,
            std::iter::once(22_528..22_539).collect::<Vec<_>>()
        );
    }

    #[test]
    fn repeated_identical_dictation_retains_each_delivered_region() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world ");
        fixture.paste("a", "hello world ", "hello world ");
        fixture.emit(
            serde_json::json!({"kind":"value", "field":"a", "value":"hello World hello world "}),
        );
        fixture.timers.advance(1_000);
        let report = fixture.edits().remove(0);
        let edit = report.edit.unwrap();
        assert_eq!(edit.baseline, "hello world hello world ");
        assert_eq!(edit.pasted_ranges, [0..12, 12..24]);
    }

    #[test]
    fn dictation_before_identical_existing_content_tracks_only_the_delivered_occurrence() {
        let fixture = ObservationFixture::new();
        fixture
            .prepare_at("a", Some("hello world"), "hello world ", 0, 0)
            .unwrap()
            .commit(true);
        fixture.emit(
            serde_json::json!({"kind":"value", "field":"a", "value":"hello World hello world"}),
        );
        fixture.timers.advance(1_000);
        let edit = fixture.edits().remove(0).edit.unwrap();
        assert_eq!(edit.baseline, "hello world hello world");
        assert_eq!(
            edit.pasted_ranges,
            std::iter::once(0..12).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dictation_between_multiple_corrections_preserves_original_baseline_and_regions() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "use cloud code with Shunade today");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"use Claude Code with Sinead today"}));
        fixture
            .prepare_at(
                "a",
                Some("use Claude Code with Sinead today"),
                "new ",
                21,
                0,
            )
            .unwrap()
            .commit(true);
        fixture.timers.advance(1_000);
        let edit = fixture.edits().remove(0).edit.unwrap();
        assert_eq!(edit.baseline, "use cloud code with new Shunade today");
        assert_eq!(edit.edited, "use Claude Code with new Sinead today");
        assert_eq!(edit.pasted_ranges, [0..20, 24..37, 20..24]);
    }

    #[test]
    fn replacement_dictation_splits_old_regions_and_keeps_unicode_byte_boundaries() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "hello world 😀 tail");
        fixture
            .prepare_at("a", Some("hello world 😀 tail"), "Claude Code", 6, 5)
            .unwrap()
            .commit(true);
        fixture.emit(
            serde_json::json!({"kind":"value", "field":"a", "value":"Hello Claude code 😀 tail"}),
        );
        fixture.timers.advance(1_000);
        let edit = fixture.edits().remove(0).edit.unwrap();
        assert_eq!(edit.baseline, "hello Claude Code 😀 tail");
        assert_eq!(edit.pasted_ranges, [0..6, 17..27, 6..17]);
    }

    #[test]
    fn alignment_budget_failure_preserves_prior_learning_without_claiming_the_new_paste() {
        let fixture = ObservationFixture::new();
        let original = format!(
            "use cloud code {} middle {}",
            "a".repeat(1_100),
            "b".repeat(1_100)
        );
        let corrected = format!(
            "use Claude Code {} middle {}",
            "a".repeat(1_100),
            "b".repeat(1_100)
        );
        fixture.paste("a", "", &original);
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":corrected}));
        fixture.timers.advance(1_000);
        let completed = fixture.edits().remove(0);
        let replaced = format!(
            "use Claude Code {} middle {}",
            "c".repeat(1_100),
            "d".repeat(1_100)
        );
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":replaced}));
        fixture
            .prepare_at("a", Some(&replaced), "new ", 1_117, 0)
            .unwrap()
            .commit(true);
        let unavailable = fixture.reports.try_recv().unwrap();
        assert_eq!(unavailable.session_id, completed.session_id);
        assert_eq!(
            unavailable.diagnostic.as_deref(),
            Some("text_alignment_budget_or_ambiguity")
        );
        fixture.emit(
            serde_json::json!({"kind":"value", "field":"a", "value":"unrelated new content"}),
        );
        fixture.timers.advance(1_000);
        assert!(
            fixture.edits().is_empty(),
            "uncertain insertion must not revoke or replace the prior completed correction"
        );
        fixture.emit(serde_json::json!({"kind":"sent", "field":"a"}));
        fixture.paste("a", "", "fresh passage");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"fresh Passage"}));
        fixture.timers.advance(1_000);
        let fresh = fixture.edits().remove(0);
        assert_ne!(fresh.session_id, completed.session_id);
        assert_eq!(
            fresh.edit.unwrap().pasted_ranges,
            std::iter::once(0..13).collect::<Vec<_>>()
        );
    }

    #[test]
    fn repeated_character_alignment_does_not_guess_which_original_position_survived() {
        let fixture = ObservationFixture::new();
        fixture.paste("a", "", "aa");
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":"aaa"}));
        fixture.edits();
        fixture
            .prepare_at("a", Some("aaa"), "B", 1, 0)
            .unwrap()
            .commit(true);
        let report = fixture.reports.try_recv().unwrap();
        assert_eq!(
            report.outcome,
            ObservationOutcome::Unreadable,
            "three equally optimal insertion locations cannot establish provenance"
        );
        fixture.timers.advance(1_000);
        assert!(fixture.edits().is_empty());
    }

    #[test]
    fn appending_to_a_long_field_maps_the_physical_end_without_an_alignment_budget() {
        let fixture = ObservationFixture::new();
        let context = "context ".repeat(30_000);
        let original = format!("start cloud {context} Shunade end");
        let corrected = format!("start Claude {context} Sinead end");
        fixture.paste("a", "", &original);
        fixture.emit(serde_json::json!({"kind":"value", "field":"a", "value":corrected}));
        fixture.paste("a", &corrected, " again");
        fixture.timers.advance(1_000);
        let edit = fixture.edits().remove(0).edit.unwrap();
        assert_eq!(edit.baseline, original.clone() + " again");
        assert_eq!(edit.edited, corrected + " again");
        assert_eq!(
            edit.pasted_ranges,
            [0..original.len(), original.len()..original.len() + 6]
        );
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
        assert_eq!(lock(&monitor.inner.capture).last_pid, Some(4242));
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
        assert_eq!(lock(&monitor.inner.capture).last_pid, Some(2222));
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
}

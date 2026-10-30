//! Supervises `macos-globe-listener`, the helper that reports Globe/Fn,
//! right-side modifiers, modifier releases and side mouse buttons, and that
//! holds the macOS Globe action at "Do Nothing" while the app owns the key.
//! Covers its restart policy (run by `supervised_child`), stdin JSON
//! reconfiguration and recovery of a preference left behind by a crash.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::super::helper::{arch_mismatch, resolve_helper};
use super::super::supervised_child::{
    BinaryResolver, ChildEvent, ChildOptions, ChildSink, Control, Down, HelperSpawner,
    ProcessSpawner, Protocol, Recovery, RestartPolicy, SupervisedChild,
};
use super::super::timer::{Timers, lock};
use super::slots::ListenerConfig;

/// Rapid automatic restarts before giving up.
const MAX_RESTART_ATTEMPTS: u32 = 3;
const RESTART_DELAY_MS: u64 = 1000;
/// Sustained uptime after which the restart counter resets (sleep/wake).
const RESTART_RESET_MS: u64 = 10_000;
/// The listener prints short event names; anything longer is garbage.
const MAX_LINE_BYTES: usize = 4096;
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

pub struct GlobeOptions {
    pub preference_state_path: Option<PathBuf>,
    pub spawner: Arc<dyn HelperSpawner>,
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

/// The listener's side of the supervised child: arguments from the current
/// configuration, stdout lines as [`GlobeEvent`]s, and errors reported once
/// until a listener starts again.
struct Listener {
    config: ListenerConfig,
    preference_state_path: Option<PathBuf>,
    has_reported_error: bool,
}

impl Listener {
    fn report_error(&mut self, message: String, cx: &mut Control<'_, GlobeEvent>) {
        if self.has_reported_error {
            return;
        }
        self.has_reported_error = true;
        log::error!("[globe] {message}");
        cx.emit(GlobeEvent::Error(message));
    }
}

impl Protocol for Listener {
    type Event = GlobeEvent;

    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if !self.config.mouse_buttons.is_empty() {
            args.push(self.config.mouse_buttons.join(","));
        }
        if let Some(path) = &self.preference_state_path {
            args.push("--globe-preference-state".into());
            args.push(path.display().to_string());
        }
        if self.config.suppress_globe_action {
            args.push("--suppress-system-globe-action".into());
        }
        args
    }

    fn on_start(&mut self, _cx: &mut Control<'_, GlobeEvent>) {
        self.has_reported_error = false;
        log::info!("[globe] listener spawned with {:?}", self.args());
    }

    fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, GlobeEvent>) {
        if let Some(event) = parse_line(line.trim()) {
            cx.emit(event);
        }
    }

    fn on_stderr(&mut self, text: &str, cx: &mut Control<'_, GlobeEvent>) {
        let message = text.trim();
        if message.contains("Failed to create event monitor") {
            cx.fail(message);
        } else if !message.is_empty() {
            log::warn!("[globe] listener stderr: {message}");
        }
    }

    fn on_down(&mut self, down: &Down, recovery: Recovery, cx: &mut Control<'_, GlobeEvent>) {
        match down {
            Down::Missing => self.report_error(
                "macOS Globe listener binary not found. Build the helpers (cargo build) before packaging."
                    .into(),
                cx,
            ),
            Down::NotStarted(message) => self.report_error(message.clone(), cx),
            // Keys held when the listener goes away never report their release.
            Down::Stopped => cx.emit(GlobeEvent::ListenerStopped),
            Down::Failed(message) => {
                cx.emit(GlobeEvent::ListenerStopped);
                self.report_error(message.clone(), cx);
            }
            Down::Exited { code, signal } => {
                log::info!("[globe] listener exited: code {code:?} signal {signal:?}");
                cx.emit(GlobeEvent::ListenerStopped);
                if matches!(signal, Some(libc::SIGINT | libc::SIGTERM)) {
                    return;
                }
                if *code != Some(0) {
                    let describe = |v: Option<i32>| v.map_or("null".to_string(), |v| v.to_string());
                    let message = format!(
                        "Globe key listener exited with code {} signal {}",
                        describe(*code),
                        describe(*signal)
                    );
                    return self.report_error(message, cx);
                }
                // Exit 0 without a stop request: the event tap was likely
                // invalidated (sleep/wake). Restart a few times.
                match recovery {
                    Recovery::Restarting { attempt } => log::warn!(
                        "[globe] unexpected exit, restarting (attempt {attempt}/{MAX_RESTART_ATTEMPTS})"
                    ),
                    Recovery::GaveUp => self.report_error(
                        format!(
                            "Globe key listener keeps exiting unexpectedly ({MAX_RESTART_ATTEMPTS} restarts failed). Try restarting Whisper."
                        ),
                        cx,
                    ),
                    Recovery::NotRestarting => {}
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct GlobeListener {
    child: SupervisedChild<Listener>,
    options: Arc<GlobeOptions>,
    recovery_blocked: Arc<AtomicBool>,
}

impl GlobeListener {
    pub fn new(options: GlobeOptions, sink: Arc<dyn Fn(GlobeEvent) + Send + Sync>) -> Self {
        let child = SupervisedChild::new(
            ChildOptions {
                label: "Globe listener",
                resolve_binary: Arc::clone(&options.resolve_binary),
                spawner: Arc::clone(&options.spawner),
                timers: Arc::clone(&options.timers),
                restart: RestartPolicy::CleanExit {
                    delay: Duration::from_millis(RESTART_DELAY_MS),
                    max_attempts: MAX_RESTART_ATTEMPTS,
                    reset_after: Duration::from_millis(RESTART_RESET_MS),
                },
                heartbeat: None,
                max_line: MAX_LINE_BYTES,
            },
            Listener {
                config: ListenerConfig::default(),
                preference_state_path: options.preference_state_path.clone(),
                has_reported_error: false,
            },
            sink,
        );
        Self {
            child,
            options: Arc::new(options),
            recovery_blocked: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.child.is_running()
    }

    pub fn start(&self) {
        if self.recovery_blocked.load(Ordering::SeqCst) {
            let message =
                "Globe listener blocked because preference recovery could not be terminated";
            self.child
                .update(|listener, cx| listener.report_error(message.into(), cx));
            return;
        }
        self.child.start();
    }

    pub fn stop(&self) {
        self.child.stop();
    }

    /// Replaces the listener's whole state (normalized as
    /// [`super::slots::SlotRegistry::native_listener_config`] builds it). A
    /// running listener is reconfigured over stdin rather than restarted: a
    /// restart drops events and briefly hands the Globe key back to macOS.
    pub fn set_configuration(&self, next: ListenerConfig) {
        let stdin_lost = self.child.update(|listener, cx| {
            if listener.config == next {
                return false;
            }
            listener.config = next;
            cx.is_running() && !cx.write_line(&wire_line(&listener.config))
        });
        // The old listener must be terminated so it restores the preference.
        if stdin_lost {
            self.child.stop();
            self.child.start();
        }
    }

    /// Restores a Globe preference marker left by a crashed run, using the
    /// helper's one-shot mode. Blocks for at most the recovery timeouts; a
    /// helper that cannot be terminated blocks the long-lived listener.
    pub fn restore_leftover_system_preference(&self) {
        let options = &self.options;
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
                        self.recovery_blocked.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
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

    pub use crate::native::supervised_child::tests::FakeSpawner;

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
            mouse_buttons: vec!["MouseButton4".into(), "MouseButton5".into()],
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

//! The laptop lid (clamshell) state, watched through
//! `macos-mic-listener --watch-lid-state`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::super::helper::resolve_helper;
use super::super::supervised_child::{
    BinaryResolver, ChildOptions, Control, Down, HelperSpawner, ProcessSpawner, Protocol, Recovery,
    RestartPolicy, SupervisedChild,
};
use super::super::timer::{RealTimers, Timers};

/// The helper samples every second; a sample this old (or a helper this
/// quiet) no longer counts.
const LID_STALE_STATE_MS: u64 = 5000;
const LID_RESTART_DELAY_MS: u64 = 2000;
const LID_MAX_LINE_BYTES: usize = 4096;
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
    pub spawner: Arc<dyn HelperSpawner>,
    pub resolve_binary: BinaryResolver,
    pub timers: Arc<dyn Timers>,
}

struct Sample {
    closed: Option<bool>,
    at_ms: u64,
}

/// The helper's side of the supervised child: every valid line is a sample
/// and a heartbeat; a helper that goes away takes its sample with it.
struct LidWatch {
    timers: Arc<dyn Timers>,
    sample: Option<Sample>,
}

impl Protocol for LidWatch {
    type Event = std::convert::Infallible;

    fn args(&self) -> Vec<String> {
        vec!["--watch-lid-state".into()]
    }

    fn on_stdout(&mut self, line: &str, cx: &mut Control<'_, Self::Event>) {
        if let Some(closed) = parse_lid_state(line.trim()) {
            self.sample = Some(Sample {
                closed,
                at_ms: self.timers.now_ms(),
            });
            cx.heartbeat();
        }
    }

    fn on_down(&mut self, down: &Down, _recovery: Recovery, _cx: &mut Control<'_, Self::Event>) {
        self.sample = None;
        match down {
            Down::NotStarted(error) => log::warn!("[lid] cannot start the lid monitor: {error}"),
            Down::Failed(error) => log::warn!("[lid] laptop lid monitor unavailable: {error}"),
            Down::Stopped | Down::Missing | Down::Exited { .. } => {}
        }
    }
}

/// Watches the clamshell state through the mic listener helper, which emits
/// a sample every second; a silent helper is replaced. Reads never block:
/// they return the latest sample.
#[derive(Clone)]
pub struct LaptopLidMonitor {
    child: SupervisedChild<LidWatch>,
}

impl LaptopLidMonitor {
    pub fn new(hooks: LidHooks) -> Self {
        let child = SupervisedChild::new(
            ChildOptions {
                label: "Lid monitor",
                resolve_binary: hooks.resolve_binary,
                spawner: hooks.spawner,
                timers: Arc::clone(&hooks.timers),
                restart: RestartPolicy::Always {
                    delay: Duration::from_millis(LID_RESTART_DELAY_MS),
                },
                heartbeat: Some(Duration::from_millis(LID_STALE_STATE_MS)),
                max_line: LID_MAX_LINE_BYTES,
            },
            LidWatch {
                timers: hooks.timers,
                sample: None,
            },
            Arc::new(|never| match never {}),
        );
        Self { child }
    }

    /// The process-wide monitor used by
    /// [`super::resolve_preferred_microphone`], started on first use.
    pub fn shared() -> LaptopLidMonitor {
        static SHARED: OnceLock<LaptopLidMonitor> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                let monitor = LaptopLidMonitor::new(LidHooks {
                    spawner: Arc::new(ProcessSpawner),
                    resolve_binary: Arc::new(|| resolve_helper(MIC_LISTENER_BINARY)),
                    timers: RealTimers::shared(),
                });
                monitor.start();
                monitor
            })
            .clone()
    }

    pub fn start(&self) {
        self.child.start();
    }

    /// The latest lid state, at once: `None` until the helper reports, while
    /// it restarts, after a sample went stale, or on a Mac without a lid.
    pub fn state(&self) -> Option<bool> {
        self.child.update(|lid, cx| {
            let sample = lid.sample.as_ref()?;
            if lid.timers.now_ms().saturating_sub(sample.at_ms) < LID_STALE_STATE_MS {
                return sample.closed;
            }
            // Timers can be delayed by sleep; never answer with a pre-sleep
            // value, and replace the helper that failed to refresh it.
            lid.sample = None;
            cx.fail("cached lid state expired");
            None
        })
    }

    #[cfg(test)]
    pub fn stop(&self) {
        self.child.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::supervised_child::ChildEvent;
    use crate::native::supervised_child::tests::FakeSpawner;
    use crate::native::timer::FakeTimers;
    use std::path::PathBuf;

    struct Lid {
        monitor: LaptopLidMonitor,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
    }

    fn helper() -> Lid {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::starting_at(100);
        let monitor = LaptopLidMonitor::new(LidHooks {
            spawner: spawner.clone(),
            resolve_binary: Arc::new(|| Some(PathBuf::from("/helper/macos-mic-listener"))),
            timers: timers.clone(),
        });
        monitor.start();
        Lid {
            monitor,
            spawner,
            timers,
        }
    }

    impl Lid {
        fn out(&self, index: usize, text: &str) {
            self.spawner.emit(index, ChildEvent::Stdout(text.into()));
        }
        fn kills(&self, index: usize) -> usize {
            self.spawner.kills(index).len()
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
    fn reads_return_the_latest_sample_at_once() {
        let h = helper();
        assert_eq!(h.spawner.args(0), vec!["--watch-lid-state".to_string()]);
        assert_eq!(h.monitor.state(), None, "unknown before the first sample");
        h.out(0, "{\"lidClosed\":false}\n");
        assert_eq!(h.monitor.state(), Some(false));
        assert_eq!(h.monitor.state(), Some(false));
        assert_eq!(h.spawner.count(), 1, "reads share the running helper");
        h.out(0, "{\"lidClosed\":true}\n");
        assert_eq!(h.monitor.state(), Some(true));
        h.out(0, "{\"lidClosed\":null}\n");
        assert_eq!(h.monitor.state(), None);
        h.monitor.stop();
    }

    #[test]
    fn unrecognized_output_is_neither_a_sample_nor_a_heartbeat() {
        let h = helper();
        h.out(0, "CAPABILITY PID\n{\"lidClosed\":1}\n");
        assert_eq!(h.monitor.state(), None);
        h.timers.advance(4000);
        h.out(0, "CAPABILITY PID\n");
        h.timers.advance(1000);
        assert_eq!(h.kills(0), 1);
        h.monitor.stop();
    }

    #[test]
    fn samples_keep_the_helper_alive_until_they_stop() {
        let h = helper();
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        assert_eq!(h.monitor.state(), Some(false));
        assert_eq!(h.kills(0), 0);
        h.timers.advance(1000);
        assert_eq!(h.kills(0), 1);
        assert_eq!(h.monitor.state(), None);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }

    #[test]
    fn helper_failure_clears_state_until_the_replacement_reports() {
        let h = helper();
        h.out(0, "{\"lidClosed\":true}\n");
        assert_eq!(h.monitor.state(), Some(true));
        h.spawner
            .emit(0, ChildEvent::Error("process failed".into()));
        assert_eq!(h.monitor.state(), None);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.out(1, "{\"lidClosed\":false}\n");
        assert_eq!(h.monitor.state(), Some(false));
        h.monitor.stop();
    }

    #[test]
    fn delayed_timer_after_sleep_cannot_expose_stale_state() {
        let h = helper();
        h.out(0, "{\"lidClosed\":true}\n");
        h.timers.advance_without_running(10_000);
        assert_eq!(h.monitor.state(), None);
        assert_eq!(h.kills(0), 1);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }
}

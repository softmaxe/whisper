//! Hold mode for key chords registered as global shortcuts
//! ("Control+Shift+Space"). The shortcut reports only its press, so the hold
//! ends when the native listener reports one of the chord's modifiers going
//! up.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::super::timer::{TimerId, Timers, lock};
use super::gesture::GestureEnv;
use super::keys::required_modifiers;
use super::{ForceStopReason, HotkeyCommand};

pub const MIN_HOLD_DURATION_MS: u64 = 150;
/// Safety ceiling: a push never records longer than five minutes.
pub const MAX_PUSH_DURATION_MS: u64 = 300_000;

struct PushState {
    seq: u64,
    is_recording: bool,
    required: Vec<&'static str>,
    safety_timer: TimerId,
    hold_timer: TimerId,
}

struct Inner {
    state: Mutex<(Option<PushState>, u64)>,
    env: Arc<dyn GestureEnv>,
    timers: Arc<dyn Timers>,
    this: Weak<Inner>,
}

#[derive(Clone)]
pub struct CompoundPush {
    inner: Arc<Inner>,
}

impl CompoundPush {
    pub fn new(env: Arc<dyn GestureEnv>, timers: Arc<dyn Timers>) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                state: Mutex::new((None, 0)),
                env,
                timers,
                this: this.clone(),
            }),
        }
    }

    pub fn is_active(&self) -> bool {
        lock(&self.inner.state).0.is_some()
    }

    /// A press of a Hold mode chord: prepare at once, start after the minimum
    /// hold, and give up at the safety ceiling.
    pub fn start(&self, hotkey: &str) {
        let inner = &self.inner;
        let mut guard = lock(&inner.state);
        if guard.0.is_some() || inner.env.is_processing() {
            return;
        }
        let required = required_modifiers(hotkey);
        if required.is_empty() {
            return;
        }
        guard.1 += 1;
        let seq = guard.1;

        inner.env.emit(HotkeyCommand::Prepare);

        let weak = inner.this.clone();
        let safety_timer = inner.timers.schedule(
            Duration::from_millis(MAX_PUSH_DURATION_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let active = lock(&inner.state).0.as_ref().is_some_and(|s| s.seq == seq);
                    if active {
                        log::warn!("[hotkeys] compound push safety timeout");
                        CompoundPush { inner }.force_stop(ForceStopReason::Timeout);
                    }
                }
            }),
        );
        let weak = inner.this.clone();
        let hold_timer = inner.timers.schedule(
            Duration::from_millis(MIN_HOLD_DURATION_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut guard = lock(&inner.state);
                    if let Some(state) = guard.0.as_mut().filter(|s| s.seq == seq)
                        && !state.is_recording
                    {
                        state.is_recording = true;
                        inner.env.emit(HotkeyCommand::Start);
                    }
                }
            }),
        );
        guard.0 = Some(PushState {
            seq,
            is_recording: false,
            required,
            safety_timer,
            hold_timer,
        });
    }

    /// The listener saw `modifier` ("command", "control", "option", "shift",
    /// "fn") go up: a physical release ends the push.
    pub fn handle_modifier_up(&self, modifier: &str) {
        let inner = &self.inner;
        let mut guard = lock(&inner.state);
        let releases = guard
            .0
            .as_ref()
            .is_some_and(|state| state.required.contains(&modifier));
        if !releases {
            return;
        }
        if let Some(state) = guard.0.take() {
            inner.timers.cancel(state.safety_timer);
            inner.timers.cancel(state.hold_timer);
            if state.is_recording {
                inner.env.emit(HotkeyCommand::Stop);
            } else {
                inner.env.emit(HotkeyCommand::CancelPreparation);
            }
        }
    }

    /// Ends a push without a physical release; the trigger keys may still be
    /// down, so the integrator must not paste into them.
    pub fn force_stop(&self, reason: ForceStopReason) {
        let inner = &self.inner;
        let mut guard = lock(&inner.state);
        let Some(state) = guard.0.take() else {
            return;
        };
        inner.timers.cancel(state.safety_timer);
        inner.timers.cancel(state.hold_timer);
        inner.env.emit(HotkeyCommand::ForceStop { reason });
        if state.is_recording {
            inner.env.emit(HotkeyCommand::Stop);
        } else {
            inner.env.emit(HotkeyCommand::CancelPreparation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::timer::FakeTimers;
    use crate::settings::ActivationMode;

    #[derive(Default)]
    struct Env {
        sent: Mutex<Vec<HotkeyCommand>>,
    }

    impl GestureEnv for Env {
        fn activation_mode(&self) -> ActivationMode {
            ActivationMode::Push
        }
        fn is_active(&self) -> bool {
            false
        }
        fn is_processing(&self) -> bool {
            false
        }
        fn emit(&self, command: HotkeyCommand) {
            lock(&self.sent).push(command);
        }
    }

    fn setup() -> (CompoundPush, Arc<Env>, Arc<FakeTimers>) {
        let env = Arc::new(Env::default());
        let timers = FakeTimers::new();
        (CompoundPush::new(env.clone(), timers.clone()), env, timers)
    }

    fn sent(env: &Env) -> Vec<HotkeyCommand> {
        lock(&env.sent).clone()
    }

    fn start_push() -> (CompoundPush, Arc<Env>, Arc<FakeTimers>) {
        let (push, env, timers) = setup();
        push.start("Control+Space");
        timers.advance(MIN_HOLD_DURATION_MS);
        assert_eq!(
            sent(&env),
            vec![HotkeyCommand::Prepare, HotkeyCommand::Start]
        );
        (push, env, timers)
    }

    #[test]
    fn physical_release_stops_without_forced_stop() {
        let (push, env, _timers) = start_push();
        push.handle_modifier_up("shift");
        push.handle_modifier_up("control");
        assert_eq!(
            sent(&env),
            vec![
                HotkeyCommand::Prepare,
                HotkeyCommand::Start,
                HotkeyCommand::Stop
            ]
        );
    }

    #[test]
    fn safety_ceiling_reports_forced_stop_before_stop() {
        let (_push, env, timers) = start_push();
        timers.advance(MAX_PUSH_DURATION_MS);
        assert_eq!(
            sent(&env),
            vec![
                HotkeyCommand::Prepare,
                HotkeyCommand::Start,
                HotkeyCommand::ForceStop {
                    reason: ForceStopReason::Timeout
                },
                HotkeyCommand::Stop,
            ]
        );
    }

    #[test]
    fn forced_stop_before_recording_cancels_preparation() {
        let (push, env, timers) = setup();
        push.start("Control+Option");
        timers.advance(100);
        push.force_stop(ForceStopReason::Timeout);
        timers.advance(1000);
        assert_eq!(
            sent(&env),
            vec![
                HotkeyCommand::Prepare,
                HotkeyCommand::ForceStop {
                    reason: ForceStopReason::Timeout
                },
                HotkeyCommand::CancelPreparation,
            ]
        );
    }

    #[test]
    fn settings_reset_reports_forced_stop() {
        let (push, env, _timers) = start_push();
        push.force_stop(ForceStopReason::Reset);
        assert_eq!(
            sent(&env)[2..],
            [
                HotkeyCommand::ForceStop {
                    reason: ForceStopReason::Reset
                },
                HotkeyCommand::Stop,
            ]
        );
        assert!(!push.is_active());
    }

    #[test]
    fn short_press_cancels_preparation_and_chord_without_modifier_is_ignored() {
        let (push, env, timers) = setup();
        push.start("F8");
        assert!(sent(&env).is_empty());
        push.start("Control+Space");
        timers.advance(50);
        push.handle_modifier_up("control");
        assert_eq!(
            sent(&env),
            vec![HotkeyCommand::Prepare, HotkeyCommand::CancelPreparation]
        );
        timers.advance(1000);
        assert_eq!(sent(&env).len(), 2);
    }
}

//! Decides what each press of a Dictation hotkey that reports its own release
//! does: Hold mode holds to talk, and Tap mode starts Hands-free dictation on
//! a Double tap and ends it on the next Clean press.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::settings::ActivationMode;
use crate::text::hotkeys::ListenerKey;

use super::super::timer::{TimerId, Timers, lock};
use super::{ForceStopReason, HotkeyCommand};

pub const HOLD_THRESHOLD_MS: u64 = 150;
pub const TAP_MAX_HOLD_MS: u64 = 300;
pub const DOUBLE_TAP_GAP_MS: u64 = 400;
/// Presses this soon after a Double tap are a third tap, not a stop.
pub const START_SETTLE_MS: u64 = 300;

/// What the gesture reads and where its commands go.
pub trait GestureEnv: Send + Sync {
    fn activation_mode(&self) -> ActivationMode;
    fn is_active(&self) -> bool;
    fn is_processing(&self) -> bool;
    fn emit(&self, command: HotkeyCommand);
}

/// What the release of the press currently held will do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressKind {
    /// May become the first half of a Double tap.
    Tap,
    /// The second half of a Double tap; Dictation already started.
    Start,
    /// A Hold mode press not yet held long enough.
    Pending,
    /// A Hold mode press that started Dictation.
    Held,
    /// Ends the active Dictation.
    Stop,
    /// Does nothing.
    Ignored,
}

#[derive(Debug, Clone)]
struct HeldPress {
    key: ListenerKey,
    kind: PressKind,
    at: u64,
    timer: Option<TimerId>,
    seq: u64,
}

#[derive(Debug, Clone)]
struct Tap {
    key: ListenerKey,
    at: u64,
    seq: u64,
}

#[derive(Default)]
struct State {
    held: Option<HeldPress>,
    // The last Clean tap that could open a Double tap; any later press uses it up.
    last_tap: Option<Tap>,
    double_tap_at: Option<u64>,
    // Reports a lone tap once no second tap can follow it.
    unused_tap_timer: Option<TimerId>,
    next_seq: u64,
}

struct Inner {
    state: Mutex<State>,
    env: Arc<dyn GestureEnv>,
    timers: Arc<dyn Timers>,
    this: Weak<Inner>,
}

#[derive(Clone)]
pub struct HotkeyGesture {
    inner: Arc<Inner>,
}

impl HotkeyGesture {
    pub fn new(env: Arc<dyn GestureEnv>, timers: Arc<dyn Timers>) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| Inner {
                state: Mutex::new(State::default()),
                env,
                timers,
                this: this.clone(),
            }),
        }
    }

    pub fn press(&self, key: ListenerKey) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        if state.held.as_ref().is_some_and(|held| held.key != key) {
            // Another Dictation hotkey pressed during a hold is just another key.
            inner.interrupt_locked(&mut state);
            return;
        }
        // Pressed again without a release: the release was lost, so start over.
        inner.clear_held_press(&mut state);
        inner.clear_unused_tap_timer(&mut state);

        let at = inner.timers.now_ms();
        let previous_tap = state.last_tap.take();
        state.next_seq += 1;
        let seq = state.next_seq;
        let press = |kind| HeldPress {
            key,
            kind,
            at,
            timer: None,
            seq,
        };

        let settling = state
            .double_tap_at
            .is_some_and(|t| at.saturating_sub(t) < START_SETTLE_MS);
        if inner.env.is_processing() || settling {
            state.held = Some(press(PressKind::Ignored));
        } else if inner.env.is_active() {
            state.held = Some(press(PressKind::Stop));
        } else if inner.env.activation_mode() == ActivationMode::Push {
            let weak = inner.this.clone();
            let timer = inner.timers.schedule(
                Duration::from_millis(HOLD_THRESHOLD_MS),
                Box::new(move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_hold_threshold(seq);
                    }
                }),
            );
            state.held = Some(HeldPress {
                timer: Some(timer),
                ..press(PressKind::Pending)
            });
            inner.env.emit(HotkeyCommand::Prepare);
        } else if previous_tap
            .as_ref()
            .is_some_and(|tap| tap.key == key && at.saturating_sub(tap.at) <= DOUBLE_TAP_GAP_MS)
        {
            state.double_tap_at = Some(at);
            state.held = Some(press(PressKind::Start));
            inner.env.emit(HotkeyCommand::Start);
        } else {
            state.held = Some(press(PressKind::Tap));
        }
    }

    pub fn release(&self, key: ListenerKey) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        if state.held.as_ref().is_none_or(|held| held.key != key) {
            return;
        }
        let Some(current) = inner.clear_held_press(&mut state) else {
            return;
        };
        let released_at = inner.timers.now_ms();
        match current.kind {
            PressKind::Stop | PressKind::Held => inner.env.emit(HotkeyCommand::Stop),
            PressKind::Pending => inner.env.emit(HotkeyCommand::UnusedPress {
                mode: ActivationMode::Push,
            }),
            PressKind::Tap if released_at.saturating_sub(current.at) <= TAP_MAX_HOLD_MS => {
                state.next_seq += 1;
                let seq = state.next_seq;
                state.last_tap = Some(Tap {
                    key,
                    at: released_at,
                    seq,
                });
                let weak = inner.this.clone();
                state.unused_tap_timer = Some(inner.timers.schedule(
                    Duration::from_millis(DOUBLE_TAP_GAP_MS),
                    Box::new(move || {
                        if let Some(inner) = weak.upgrade() {
                            inner.on_unused_tap(seq);
                        }
                    }),
                ));
            }
            PressKind::Tap => inner.env.emit(HotkeyCommand::UnusedPress {
                mode: ActivationMode::Tap,
            }),
            PressKind::Start | PressKind::Ignored => {}
        }
    }

    /// Another key or mouse button went down while the hotkey was held: the
    /// press was part of a shortcut, so it is void.
    pub fn interrupt(&self) {
        let mut state = lock(&self.inner.state);
        self.inner.interrupt_locked(&mut state);
    }

    /// The listener that reports releases went away, so the held press will
    /// never be released. A press that would have stopped Dictation stops it
    /// now, without knowing whether the hotkey is still down.
    pub fn release_lost(&self) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        let current = inner.clear_held_press(&mut state);
        state.last_tap = None;
        inner.clear_unused_tap_timer(&mut state);
        if matches!(
            current.map(|c| c.kind),
            Some(PressKind::Stop | PressKind::Held)
        ) {
            inner.env.emit(HotkeyCommand::ForceStop {
                reason: ForceStopReason::ListenerLost,
            });
            inner.env.emit(HotkeyCommand::Stop);
        }
    }

    pub fn reset(&self) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        inner.clear_held_press(&mut state);
        inner.clear_unused_tap_timer(&mut state);
        state.last_tap = None;
        state.double_tap_at = None;
    }
}

impl Inner {
    fn clear_unused_tap_timer(&self, state: &mut State) {
        if let Some(timer) = state.unused_tap_timer.take() {
            self.timers.cancel(timer);
        }
    }

    // Ends the press currently held. A Hold mode press that never started
    // Dictation gives back the microphone it prepared.
    fn clear_held_press(&self, state: &mut State) -> Option<HeldPress> {
        let current = state.held.take()?;
        if let Some(timer) = current.timer {
            self.timers.cancel(timer);
        }
        if current.kind == PressKind::Pending {
            self.env.emit(HotkeyCommand::CancelPreparation);
        }
        Some(current)
    }

    fn interrupt_locked(&self, state: &mut State) {
        let Some(current) = self.clear_held_press(state) else {
            return;
        };
        let kind = current.kind;
        state.held = Some(HeldPress {
            kind: PressKind::Ignored,
            timer: None,
            ..current
        });
        if matches!(kind, PressKind::Start | PressKind::Held) {
            self.env.emit(HotkeyCommand::Cancel);
        }
    }

    fn on_hold_threshold(&self, seq: u64) {
        let mut state = lock(&self.state);
        let Some(held) = state.held.as_mut() else {
            return;
        };
        if held.seq != seq || held.kind != PressKind::Pending {
            return;
        }
        held.kind = PressKind::Held;
        held.timer = None;
        self.env.emit(HotkeyCommand::Start);
    }

    fn on_unused_tap(&self, seq: u64) {
        let mut state = lock(&self.state);
        state.unused_tap_timer = None;
        if state.last_tap.as_ref().is_some_and(|tap| tap.seq == seq) {
            self.env.emit(HotkeyCommand::UnusedPress {
                mode: ActivationMode::Tap,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::timer::FakeTimers;
    use crate::text::hotkeys::Modifier;

    const GLOBE: ListenerKey = ListenerKey::Globe;
    const RIGHT_COMMAND: ListenerKey = ListenerKey::RightModifier(Modifier::Command);
    const MOUSE_BUTTON_4: ListenerKey = ListenerKey::MouseButton(4);

    struct Env {
        mode: Mutex<ActivationMode>,
        active: Mutex<bool>,
        processing: Mutex<bool>,
        commands: Mutex<Vec<&'static str>>,
        preparation: Mutex<Vec<&'static str>>,
        hints: Mutex<Vec<ActivationMode>>,
    }

    impl GestureEnv for Env {
        fn activation_mode(&self) -> ActivationMode {
            *lock(&self.mode)
        }
        fn is_active(&self) -> bool {
            *lock(&self.active)
        }
        fn is_processing(&self) -> bool {
            *lock(&self.processing)
        }
        fn emit(&self, command: HotkeyCommand) {
            match command {
                HotkeyCommand::Start => {
                    lock(&self.commands).push("start");
                    *lock(&self.active) = true;
                }
                HotkeyCommand::Stop => {
                    // A forced stop is recorded as one "forceStop" command.
                    let mut commands = lock(&self.commands);
                    if commands.last() != Some(&"forceStop") {
                        commands.push("stop");
                    }
                    *lock(&self.active) = false;
                }
                HotkeyCommand::Cancel => {
                    lock(&self.commands).push("cancel");
                    *lock(&self.active) = false;
                }
                HotkeyCommand::ForceStop { .. } => lock(&self.commands).push("forceStop"),
                HotkeyCommand::Prepare => lock(&self.preparation).push("prepare"),
                HotkeyCommand::CancelPreparation => lock(&self.preparation).push("cancel"),
                HotkeyCommand::UnusedPress { mode } => lock(&self.hints).push(mode),
                other => panic!("unexpected command {other:?}"),
            }
        }
    }

    struct Harness {
        gesture: HotkeyGesture,
        env: Arc<Env>,
        timers: Arc<FakeTimers>,
    }

    impl Harness {
        fn new(mode: ActivationMode) -> Self {
            let env = Arc::new(Env {
                mode: Mutex::new(mode),
                active: Mutex::new(false),
                processing: Mutex::new(false),
                commands: Mutex::new(Vec::new()),
                preparation: Mutex::new(Vec::new()),
                hints: Mutex::new(Vec::new()),
            });
            let timers = FakeTimers::new();
            let gesture = HotkeyGesture::new(env.clone(), timers.clone());
            Self {
                gesture,
                env,
                timers,
            }
        }
        fn tap_mode() -> Self {
            Self::new(ActivationMode::Tap)
        }
        fn push_mode() -> Self {
            Self::new(ActivationMode::Push)
        }
        fn advance(&self, ms: u64) {
            self.timers.advance(ms);
        }
        fn tap_key(&self, key: ListenerKey, held_ms: u64) {
            self.gesture.press(key);
            self.advance(held_ms);
            self.gesture.release(key);
        }
        fn tap(&self) {
            self.tap_key(RIGHT_COMMAND, 80);
        }
        fn commands(&self) -> Vec<&'static str> {
            lock(&self.env.commands).clone()
        }
        fn preparation(&self) -> Vec<&'static str> {
            lock(&self.env.preparation).clone()
        }
        fn hints(&self) -> Vec<ActivationMode> {
            lock(&self.env.hints).clone()
        }
        fn clear_commands(&self) {
            lock(&self.env.commands).clear();
        }
        fn start_hands_free(&self) {
            self.tap();
            self.advance(150);
            self.tap();
            self.advance(1000);
            self.clear_commands();
        }
    }

    const NONE: [&str; 0] = [];

    #[test]
    fn single_tap_does_not_start() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(2000);
        assert_eq!(h.commands(), NONE);
    }

    #[test]
    fn double_tap_starts_on_second_press() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(200);
        h.gesture.press(RIGHT_COMMAND);
        assert_eq!(h.commands(), ["start"]);
    }

    #[test]
    fn long_first_press_does_not_count() {
        let h = Harness::tap_mode();
        h.tap_key(RIGHT_COMMAND, 350);
        h.advance(100);
        h.gesture.press(RIGHT_COMMAND);
        assert_eq!(h.commands(), NONE);
    }

    #[test]
    fn taps_too_far_apart_are_two_single_taps() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(450);
        h.gesture.press(RIGHT_COMMAND);
        assert_eq!(h.commands(), NONE);
    }

    #[test]
    fn hands_free_ends_on_release_of_next_press() {
        let h = Harness::tap_mode();
        h.start_hands_free();
        assert_eq!(h.commands(), NONE);
        h.gesture.press(RIGHT_COMMAND);
        h.advance(2000);
        assert_eq!(h.commands(), NONE);
        h.gesture.release(RIGHT_COMMAND);
        assert_eq!(h.commands(), ["stop"]);
    }

    #[test]
    fn third_tap_right_after_double_tap_does_not_end() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(150);
        h.tap();
        h.advance(100);
        h.tap();
        h.advance(1000);
        assert_eq!(h.commands(), ["start"]);
    }

    #[test]
    fn press_used_as_modifier_is_not_a_tap() {
        let h = Harness::tap_mode();
        h.gesture.press(RIGHT_COMMAND);
        h.advance(60);
        h.gesture.interrupt();
        h.advance(40);
        h.gesture.release(RIGHT_COMMAND);
        h.advance(150);
        h.tap();
        h.advance(1000);
        assert_eq!(h.commands(), NONE);
    }

    #[test]
    fn interrupted_second_press_cancels() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(150);
        h.gesture.press(RIGHT_COMMAND);
        h.advance(60);
        h.gesture.interrupt();
        h.gesture.release(RIGHT_COMMAND);
        assert_eq!(h.commands(), ["start", "cancel"]);
    }

    #[test]
    fn shortcut_during_hands_free_does_not_end_it() {
        let h = Harness::tap_mode();
        h.start_hands_free();
        h.gesture.press(RIGHT_COMMAND);
        h.advance(60);
        h.gesture.interrupt();
        h.gesture.release(RIGHT_COMMAND);
        assert_eq!(h.commands(), NONE);
        h.advance(500);
        h.tap();
        assert_eq!(h.commands(), ["stop"]);
    }

    #[test]
    fn hold_starts_after_threshold_and_ends_on_release() {
        let h = Harness::push_mode();
        h.gesture.press(GLOBE);
        h.advance(149);
        assert_eq!(h.commands(), NONE);
        h.advance(1);
        assert_eq!(h.commands(), ["start"]);
        h.advance(3000);
        h.gesture.release(GLOBE);
        assert_eq!(h.commands(), ["start", "stop"]);
    }

    #[test]
    fn hold_short_press_or_shortcut_issues_nothing() {
        let h = Harness::push_mode();
        h.tap_key(RIGHT_COMMAND, 100);
        h.advance(500);
        h.gesture.press(RIGHT_COMMAND);
        h.advance(100);
        h.gesture.interrupt();
        h.advance(400);
        h.gesture.release(RIGHT_COMMAND);
        assert_eq!(h.commands(), NONE);
    }

    #[test]
    fn hold_shortcut_during_hold_cancels() {
        let h = Harness::push_mode();
        h.gesture.press(RIGHT_COMMAND);
        h.advance(800);
        h.gesture.interrupt();
        h.gesture.release(RIGHT_COMMAND);
        assert_eq!(h.commands(), ["start", "cancel"]);
    }

    #[test]
    fn presses_ignored_while_processing() {
        for mode in [ActivationMode::Tap, ActivationMode::Push] {
            let h = Harness::new(mode);
            *lock(&h.env.processing) = true;
            h.tap();
            h.advance(150);
            h.gesture.press(RIGHT_COMMAND);
            h.advance(1000);
            *lock(&h.env.processing) = false;
            h.gesture.release(RIGHT_COMMAND);
            assert_eq!(h.commands(), NONE, "{mode:?}");
        }
    }

    #[test]
    fn another_hotkey_during_hold_voids_it() {
        let h = Harness::push_mode();
        h.gesture.press(GLOBE);
        h.advance(500);
        h.gesture.press(RIGHT_COMMAND);
        h.advance(500);
        h.gesture.release(RIGHT_COMMAND);
        h.gesture.release(GLOBE);
        assert_eq!(h.commands(), ["start", "cancel"]);
    }

    #[test]
    fn reset_forgets_half_finished_gesture() {
        let tap = Harness::tap_mode();
        tap.tap();
        tap.gesture.reset();
        tap.advance(100);
        tap.tap();
        assert_eq!(tap.commands(), NONE);

        let hold = Harness::push_mode();
        hold.gesture.press(GLOBE);
        hold.gesture.reset();
        hold.advance(1000);
        hold.gesture.release(GLOBE);
        assert_eq!(hold.commands(), NONE);
    }

    #[test]
    fn only_two_taps_of_same_hotkey_make_double_tap() {
        let different = Harness::tap_mode();
        different.tap_key(GLOBE, 80);
        different.advance(150);
        different.gesture.press(RIGHT_COMMAND);
        assert_eq!(different.commands(), NONE);

        let voided = Harness::tap_mode();
        voided.tap();
        voided.advance(100);
        voided.gesture.press(GLOBE);
        voided.gesture.interrupt();
        voided.gesture.release(GLOBE);
        voided.advance(100);
        voided.gesture.press(RIGHT_COMMAND);
        assert_eq!(voided.commands(), NONE);
    }

    #[test]
    fn lost_release_does_not_block_next_press() {
        let h = Harness::push_mode();
        h.gesture.press(MOUSE_BUTTON_4);
        h.advance(500);
        h.gesture.interrupt();
        h.advance(5000);
        h.gesture.press(MOUSE_BUTTON_4);
        h.advance(200);
        h.gesture.release(MOUSE_BUTTON_4);
        assert_eq!(h.commands(), ["start", "cancel", "start", "stop"]);
    }

    #[test]
    fn next_hold_after_short_one_starts_normally() {
        let h = Harness::push_mode();
        h.gesture.press(GLOBE);
        h.advance(200);
        h.gesture.release(GLOBE);
        h.advance(100);
        h.gesture.press(GLOBE);
        h.advance(200);
        h.gesture.release(GLOBE);
        assert_eq!(h.commands(), ["start", "stop", "start", "stop"]);
    }

    #[test]
    fn microphone_prepared_at_press_and_kept() {
        let h = Harness::push_mode();
        h.gesture.press(GLOBE);
        assert_eq!(h.preparation(), ["prepare"]);
        h.advance(150);
        h.gesture.release(GLOBE);
        assert_eq!(h.commands(), ["start", "stop"]);
        assert_eq!(h.preparation(), ["prepare"]);
    }

    #[test]
    fn press_that_never_starts_gives_back_microphone() {
        let short = Harness::push_mode();
        short.tap_key(GLOBE, 100);
        assert_eq!(short.preparation(), ["prepare", "cancel"]);

        let shortcut = Harness::push_mode();
        shortcut.gesture.press(GLOBE);
        shortcut.advance(50);
        shortcut.gesture.interrupt();
        assert_eq!(shortcut.preparation(), ["prepare", "cancel"]);
        shortcut.advance(500);
        shortcut.gesture.release(GLOBE);
        assert_eq!(shortcut.commands(), NONE);
        assert_eq!(shortcut.preparation(), ["prepare", "cancel"]);

        let lost = Harness::push_mode();
        lost.gesture.press(GLOBE);
        lost.gesture.reset();
        assert_eq!(lost.preparation(), ["prepare", "cancel"]);
    }

    #[test]
    fn hold_released_too_soon_is_reported_unused() {
        let h = Harness::push_mode();
        h.tap_key(GLOBE, 100);
        assert_eq!(h.hints(), [ActivationMode::Push]);
        h.advance(500);
        h.gesture.press(GLOBE);
        h.gesture.interrupt();
        h.gesture.release(GLOBE);
        assert_eq!(h.hints(), [ActivationMode::Push]);
    }

    #[test]
    fn lone_tap_reported_once_no_second_tap_can_follow() {
        let h = Harness::tap_mode();
        h.tap();
        h.advance(399);
        assert!(h.hints().is_empty());
        h.advance(1);
        assert_eq!(h.hints(), [ActivationMode::Tap]);
    }

    #[test]
    fn tap_mode_long_press_reported_unused_on_release() {
        let h = Harness::tap_mode();
        h.tap_key(GLOBE, 1500);
        assert_eq!(h.hints(), [ActivationMode::Tap]);
    }

    #[test]
    fn double_tap_stop_and_shortcuts_not_reported_unused() {
        let h = Harness::tap_mode();
        h.start_hands_free();
        h.tap();
        h.gesture.press(RIGHT_COMMAND);
        h.gesture.interrupt();
        h.gesture.release(RIGHT_COMMAND);
        h.advance(1000);
        assert_eq!(h.commands(), ["stop"]);
        assert!(h.hints().is_empty());
    }

    #[test]
    fn lost_listener_force_stops_a_hold() {
        let held = Harness::push_mode();
        held.gesture.press(GLOBE);
        held.advance(500);
        held.gesture.release_lost();
        held.gesture.release(GLOBE);
        assert_eq!(held.commands(), ["start", "forceStop"]);

        let stopping = Harness::tap_mode();
        stopping.start_hands_free();
        stopping.gesture.press(RIGHT_COMMAND);
        stopping.gesture.release_lost();
        assert_eq!(stopping.commands(), ["forceStop"]);
    }

    #[test]
    fn lost_listener_leaves_hands_free_running() {
        let hands_free = Harness::tap_mode();
        hands_free.tap();
        hands_free.advance(150);
        hands_free.gesture.press(RIGHT_COMMAND);
        hands_free.gesture.release_lost();
        assert_eq!(hands_free.commands(), ["start"]);

        let pending = Harness::push_mode();
        pending.gesture.press(GLOBE);
        pending.gesture.release_lost();
        pending.advance(1000);
        assert_eq!(pending.commands(), NONE);
        assert_eq!(pending.preparation(), ["prepare", "cancel"]);
        assert!(pending.hints().is_empty());

        let first_tap = Harness::tap_mode();
        first_tap.tap();
        first_tap.gesture.release_lost();
        first_tap.advance(100);
        first_tap.gesture.press(RIGHT_COMMAND);
        first_tap.advance(1000);
        assert_eq!(first_tap.commands(), NONE);
        assert!(first_tap.hints().is_empty());
    }
}

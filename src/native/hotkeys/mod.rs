//! Global Dictation hotkeys.
//!
//! Two paths observe hotkeys:
//! - Globe/Fn, right-side modifiers and side mouse buttons come from the
//!   `macos-globe-listener` helper ([`globe`]), which reports presses *and*
//!   releases, so a [`gesture`] decides Tap (Double tap -> Hands-free) or Hold.
//! - Key chords ("Control+Shift+Space", "F13") are Carbon global shortcuts via
//!   `global-hotkey` ([`backend`]). They toggle in Tap mode; in Hold mode a
//!   chord holds until the listener reports one of its modifiers going up
//!   ([`push`]).
//!
//! Decisions are sent as [`HotkeyCommand`]s on an `async_channel`; the
//! integrator owns the Dictation lifecycle and reports it via
//! [`DictationStatus`].
//!
//! Threading: create [`HotkeyManager`] and call `register*`,
//! `set_listening_mode`, `register_cancel_hotkey*` and `shutdown` on the main
//! thread (Carbon registration). Events are produced on helper threads.

pub mod backend;
pub mod cancel;
pub mod gesture;
pub mod globe;
pub mod keys;
pub mod push;
pub mod repeat_gate;
pub mod slots;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::thread::JoinHandle;

use global_hotkey::HotKeyState;

use crate::paths;
use crate::settings::{ActivationMode, DEFAULT_HOTKEY};
use crate::text::hotkeys::{
    Hotkey, ListenerKey, Modifier, parse_hotkey_list, serialize_hotkey_list,
};

use super::timer::{RealTimers, Timers, lock};
use backend::CarbonBackend;
use cancel::CancelBinding;
pub use cancel::CancelHotkeyOwner;
use gesture::{GestureEnv, HotkeyGesture};
use globe::{GlobeEvent, GlobeListener, GlobeOptions};
use push::CompoundPush;
use repeat_gate::RepeatGate;
use slots::{
    CANCEL_SLOT, DICTATION_SLOT, ListenerConfig, ShortcutBackend, ShortcutTarget, SlotRegistry,
};

static CANCEL_KEY: LazyLock<Hotkey> =
    LazyLock::new(|| Hotkey::parse("Escape").expect("static hotkey"));

/// Why a Dictation was stopped without a physical release of its hotkey.
/// The trigger keys may still be down, so the transcript must be kept
/// (offered for copy) instead of pasted into the held modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceStopReason {
    /// The native listener went away while a hold was in progress.
    ListenerLost,
    /// The five-minute Hold mode safety ceiling.
    Timeout,
}

/// What the integrator should do with the Dictation and the hotkey UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyCommand {
    /// Start or stop depending on the current state (Tap mode key chords).
    Toggle,
    Start,
    Stop,
    /// Open the microphone ahead of a Hold mode start (and show the panel).
    Prepare,
    /// Give back a prepared microphone (and hide the panel if idle).
    CancelPreparation,
    /// The hold turned out to be a shortcut: discard the Dictation.
    Cancel,
    /// Notification that the following `Stop`/`CancelPreparation` happens
    /// without a physical release; do not paste. Always followed by one of
    /// those two.
    ForceStop {
        reason: ForceStopReason,
    },
    /// The cancel hotkey (Escape) was pressed.
    CancelKeyPressed,
    /// A clean press that did nothing; hint how `mode` starts Dictation.
    UnusedPress {
        mode: ActivationMode,
    },
    /// A hotkey failed to register; `message_key` is a locale key under
    /// `hotkey.errors.*` taking `{{hotkey}}` (and `{{slot}}`).
    RegistrationFailed {
        hotkey: String,
        message_key: String,
    },
    /// The comma-separated list of hotkeys now active.
    ActiveHotkeyChanged(String),
    /// The Globe listener failed (missing binary, crash loop, no event tap).
    ListenerError {
        message: String,
    },
}

/// The integrator's view of the current Dictation.
pub trait DictationStatus: Send + Sync {
    /// Requested, opening the microphone, or recording.
    fn is_active(&self) -> bool;
    /// Transcribing; hotkey presses are ignored.
    fn is_processing(&self) -> bool;
}

/// Raw listener events for the hotkey capture UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawKeyEvent {
    KeyDown(ListenerKey),
    KeyUp(ListenerKey),
    ModifierUp(Modifier),
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyFailureReason {
    AlreadyRegistered,
    RegistrationFailed,
    FnCombinationUnsupported,
    SlotConflict {
        slot: String,
    },
    /// Hold mode needs a key whose release macOS reports.
    HoldNeedsReleaseKey,
    /// The cancel binding is held by another owner with a different key.
    CancelKeyInUse,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.english_message())]
pub struct HotkeyError {
    pub hotkey: String,
    pub reason: HotkeyFailureReason,
    /// Alternatives to offer as a "Try: ..." suffix.
    pub suggestions: Vec<String>,
}

impl HotkeyError {
    /// Locale key (`locales/*.json`) for the message.
    pub fn message_key(&self) -> &'static str {
        match self.reason {
            HotkeyFailureReason::AlreadyRegistered => "hotkey.errors.alreadyRegistered",
            HotkeyFailureReason::RegistrationFailed | HotkeyFailureReason::CancelKeyInUse => {
                "hotkey.errors.registrationFailed"
            }
            HotkeyFailureReason::FnCombinationUnsupported => {
                "hotkey.errors.fnCombinationUnsupported"
            }
            HotkeyFailureReason::SlotConflict { .. } => "hotkey.errors.slotConflict",
            HotkeyFailureReason::HoldNeedsReleaseKey => "hotkey.errors.holdNeedsReleaseKey",
        }
    }

    fn english_message(&self) -> String {
        let hotkey = &self.hotkey;
        let base = match &self.reason {
            HotkeyFailureReason::AlreadyRegistered => {
                format!("\"{hotkey}\" is already registered by another application.")
            }
            HotkeyFailureReason::RegistrationFailed => format!(
                "Could not register \"{hotkey}\". It may be in use by another application."
            ),
            HotkeyFailureReason::FnCombinationUnsupported => {
                "The Globe/Fn key can only be used by itself.".to_string()
            }
            HotkeyFailureReason::SlotConflict { slot } => {
                format!("This hotkey is already used for {slot}.")
            }
            HotkeyFailureReason::HoldNeedsReleaseKey => "Hold-to-talk needs a key macOS can see you release: Globe, a right-side key like Right Option, or a shortcut with a modifier. Tap works with any key.".to_string(),
            HotkeyFailureReason::CancelKeyInUse => {
                "Cancel hotkey is in use with a different key".to_string()
            }
        };
        if self.suggestions.is_empty() {
            base
        } else {
            format!("{base} Try: {}", self.suggestions.join(", "))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyModeInfo {
    pub supports_push_to_talk: bool,
    /// Locale key explaining why Hold mode is unavailable.
    pub push_to_talk_unavailable_reason: Option<String>,
}

/// Result of [`HotkeyManager::register_saved`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRegistration {
    /// Active hotkey list (falls back to [`DEFAULT_HOTKEY`]).
    pub active: String,
    /// Effective activation mode; a saved Hold mode the key cannot support
    /// falls back to Tap and should be persisted by the caller.
    pub mode: ActivationMode,
}

fn mode_to_u8(mode: ActivationMode) -> u8 {
    match mode {
        ActivationMode::Tap => 0,
        ActivationMode::Push => 1,
    }
}

fn mode_from_u8(value: u8) -> ActivationMode {
    if value == 1 {
        ActivationMode::Push
    } else {
        ActivationMode::Tap
    }
}

#[derive(Default)]
struct State {
    slots: SlotRegistry,
    cancel: CancelBinding,
}

struct Inner {
    events: async_channel::Sender<HotkeyCommand>,
    status: Arc<dyn DictationStatus>,
    // Set when a start was requested and the integrator has not reported it
    // yet, so a quick second press already counts as a stop.
    requested: AtomicBool,
    listening: AtomicBool,
    mode: AtomicU8,
    state: Mutex<State>,
    backend: Arc<dyn ShortcutBackend>,
    timers: Arc<dyn Timers>,
    gesture: HotkeyGesture,
    push: CompoundPush,
    globe: GlobeListener,
    repeat_gate: Mutex<RepeatGate>,
    raw: Mutex<Vec<async_channel::Sender<RawKeyEvent>>>,
    native_started: AtomicBool,
    recovery: Mutex<Option<JoinHandle<()>>>,
}

/// Environment adapter for the gesture and push state machines.
struct Env(Weak<Inner>);

impl GestureEnv for Env {
    fn activation_mode(&self) -> ActivationMode {
        self.0
            .upgrade()
            .map_or(ActivationMode::Tap, |inner| inner.activation_mode())
    }
    fn is_active(&self) -> bool {
        self.0.upgrade().is_some_and(|inner| inner.is_active())
    }
    fn is_processing(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|inner| inner.status.is_processing())
    }
    fn emit(&self, command: HotkeyCommand) {
        if let Some(inner) = self.0.upgrade() {
            inner.emit(command);
        }
    }
}

pub(crate) type GlobeSink = Arc<dyn Fn(GlobeEvent) + Send + Sync>;

pub(crate) struct Parts {
    pub backend: Arc<dyn ShortcutBackend>,
    pub timers: Arc<dyn Timers>,
    pub globe: Box<dyn FnOnce(GlobeSink) -> GlobeListener>,
}

/// Owns every Dictation hotkey registration. Cheap to clone.
#[derive(Clone)]
pub struct HotkeyManager {
    inner: Arc<Inner>,
}

impl HotkeyManager {
    /// Creates the manager. Call on the main thread: the Carbon hotkey
    /// handler is installed on the application event target. Starts a
    /// background recovery of a Globe preference left by a crash; the Globe
    /// listener itself starts with [`HotkeyManager::start_native_listener`].
    pub fn new(
        status: Arc<dyn DictationStatus>,
        events: async_channel::Sender<HotkeyCommand>,
    ) -> Self {
        let backend = CarbonBackend::new();
        let timers = RealTimers::shared();
        let state_path = paths::data_dir().join("globe-preference-state.json");
        let globe_timers = Arc::clone(&timers);
        let manager = Self::with_parts(
            status,
            events,
            Parts {
                backend: backend.clone(),
                timers,
                globe: Box::new(move |sink| {
                    GlobeListener::new(
                        GlobeOptions::production(Some(state_path), globe_timers),
                        sink,
                    )
                }),
            },
        );
        let weak = Arc::downgrade(&manager.inner);
        backend.route_events(Arc::new(move |target, state| {
            if let Some(inner) = weak.upgrade() {
                inner.on_shortcut(target, state);
            }
        }));
        let globe = manager.inner.globe.clone();
        let recovery = std::thread::Builder::new()
            .name("whisper-globe-recovery".into())
            .spawn(move || globe.restore_leftover_system_preference())
            .ok();
        *lock(&manager.inner.recovery) = recovery;
        manager
    }

    pub(crate) fn with_parts(
        status: Arc<dyn DictationStatus>,
        events: async_channel::Sender<HotkeyCommand>,
        parts: Parts,
    ) -> Self {
        let inner = Arc::new_cyclic(|weak: &Weak<Inner>| {
            let env: Arc<dyn GestureEnv> = Arc::new(Env(weak.clone()));
            let sink_owner = weak.clone();
            let sink: Arc<dyn Fn(GlobeEvent) + Send + Sync> = Arc::new(move |event| {
                if let Some(inner) = sink_owner.upgrade() {
                    inner.on_globe_event(event);
                }
            });
            Inner {
                events,
                status,
                requested: AtomicBool::new(false),
                listening: AtomicBool::new(false),
                mode: AtomicU8::new(mode_to_u8(ActivationMode::Tap)),
                state: Mutex::new(State {
                    slots: SlotRegistry::new(),
                    cancel: CancelBinding::default(),
                }),
                backend: parts.backend,
                gesture: HotkeyGesture::new(Arc::clone(&env), Arc::clone(&parts.timers)),
                push: CompoundPush::new(env, Arc::clone(&parts.timers)),
                timers: parts.timers,
                globe: (parts.globe)(sink),
                repeat_gate: Mutex::new(RepeatGate::default()),
                raw: Mutex::new(Vec::new()),
                native_started: AtomicBool::new(false),
                recovery: Mutex::new(None),
            }
        });
        Self { inner }
    }

    /// Starts the Globe listener (it needs Accessibility). Idempotent and
    /// non-blocking.
    pub fn start_native_listener(&self) {
        if self.inner.native_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let recovery = lock(&self.inner.recovery).take();
        let inner = Arc::clone(&self.inner);
        let start = move || {
            if let Some(handle) = recovery {
                let _ = handle.join();
            }
            inner.sync_listener_config();
            inner.globe.start();
        };
        if std::thread::Builder::new()
            .name("whisper-globe-start".into())
            .spawn(start)
            .is_err()
        {
            log::error!("[hotkeys] failed to start the Globe listener thread");
        }
    }

    /// Replaces the Dictation hotkeys with `hotkey_list` atomically: on any
    /// failure the previous hotkeys stay registered. Also applies `mode`
    /// (Hold mode needs a primary key whose release is visible). Returns the
    /// active list.
    pub fn register(&self, hotkey_list: &str, mode: ActivationMode) -> Result<String, HotkeyError> {
        let hotkeys = parse_hotkey_list(hotkey_list);
        let Some(primary) = hotkeys.first() else {
            return Err(HotkeyError {
                hotkey: String::new(),
                reason: HotkeyFailureReason::RegistrationFailed,
                suggestions: Vec::new(),
            });
        };
        if mode == ActivationMode::Push && !primary.supports_hold() {
            return Err(HotkeyError {
                hotkey: primary.to_string(),
                reason: HotkeyFailureReason::HoldNeedsReleaseKey,
                suggestions: Vec::new(),
            });
        }
        {
            let mut state = lock(&self.inner.state);
            let backend = Arc::clone(&self.inner.backend);
            state
                .slots
                .setup_shortcuts(backend.as_ref(), &hotkeys, DICTATION_SLOT, true)?;
        }
        let joined = serialize_hotkey_list(&hotkeys);
        self.inner.mode.store(mode_to_u8(mode), Ordering::SeqCst);
        self.inner.after_hotkey_change();
        self.inner
            .send(HotkeyCommand::ActiveHotkeyChanged(joined.clone()));
        Ok(joined)
    }

    /// Startup registration of the saved list: best effort (individual
    /// failures are reported as [`HotkeyCommand::RegistrationFailed`]),
    /// falling back to [`DEFAULT_HOTKEY`] when nothing registers.
    pub fn register_saved(&self, hotkey_list: &str, mode: ActivationMode) -> StartupRegistration {
        let saved = parse_hotkey_list(hotkey_list);
        let mut active = None;
        if !saved.is_empty() {
            let result = {
                let mut state = lock(&self.inner.state);
                let backend = Arc::clone(&self.inner.backend);
                state
                    .slots
                    .setup_shortcuts(backend.as_ref(), &saved, DICTATION_SLOT, false)
            };
            match result {
                Ok(outcome) => {
                    for failure in &outcome.failures {
                        self.inner.notify_failure(failure);
                    }
                    active = Some(outcome.hotkeys);
                }
                Err(error) => {
                    self.inner.notify_failure(&HotkeyError {
                        hotkey: hotkey_list.trim().to_string(),
                        ..error
                    });
                }
            }
        }
        let active = active.unwrap_or_else(|| {
            let default = parse_hotkey_list(DEFAULT_HOTKEY);
            let mut state = lock(&self.inner.state);
            let backend = Arc::clone(&self.inner.backend);
            let _ = state
                .slots
                .setup_shortcuts(backend.as_ref(), &default, DICTATION_SLOT, false);
            default
        });
        let mode = if mode == ActivationMode::Push
            && active
                .first()
                .is_some_and(|primary| !primary.supports_hold())
        {
            ActivationMode::Tap
        } else {
            mode
        };
        let active = serialize_hotkey_list(&active);
        self.inner.mode.store(mode_to_u8(mode), Ordering::SeqCst);
        self.inner.after_hotkey_change();
        self.inner
            .send(HotkeyCommand::ActiveHotkeyChanged(active.clone()));
        StartupRegistration { active, mode }
    }

    /// Switches Tap/Hold. Returns false (and reports why) when the current
    /// primary hotkey cannot be held.
    pub fn set_activation_mode(&self, mode: ActivationMode) -> bool {
        if self.inner.activation_mode() == mode {
            return true;
        }
        if mode == ActivationMode::Push
            && let Some(primary) = self.primary_hotkey()
            && !primary.supports_hold()
        {
            self.inner.notify_failure(&HotkeyError {
                hotkey: primary.to_string(),
                reason: HotkeyFailureReason::HoldNeedsReleaseKey,
                suggestions: Vec::new(),
            });
            return false;
        }
        self.inner.mode.store(mode_to_u8(mode), Ordering::SeqCst);
        true
    }

    /// Suspends every hotkey while the settings UI captures a new one.
    /// Shortcuts are unregistered so they reach the capture field; the
    /// listener keeps reporting raw keys and keeps macOS's Globe action down.
    pub fn set_listening_mode(&self, enabled: bool) {
        if self.inner.listening.swap(enabled, Ordering::SeqCst) == enabled {
            return;
        }
        log::info!(
            "[hotkeys] listening mode {}",
            if enabled { "enabled" } else { "disabled" }
        );
        let accelerators = lock(&self.inner.state).slots.accelerators();
        if enabled {
            self.start_native_listener();
            self.inner.globe.set_configuration(ListenerConfig {
                mouse_buttons: Vec::new(),
                suppress_globe_action: true,
            });
            for (_, _, accelerator) in accelerators {
                self.inner.backend.unregister(&accelerator);
            }
        } else {
            for (slot, hotkey, accelerator) in accelerators {
                if !self.inner.backend.is_registered(&accelerator)
                    && !self
                        .inner
                        .backend
                        .register(&accelerator, ShortcutTarget { slot, hotkey })
                {
                    log::warn!(
                        "[hotkeys] failed to re-register \"{accelerator}\" after capture mode"
                    );
                }
            }
            self.inner.sync_listener_config();
        }
    }

    /// Binds Escape to cancel a recording (owner: recording).
    pub fn register_cancel_hotkey(&self) {
        if let Err(error) = self.register_cancel_hotkey_for(CancelHotkeyOwner::Recording) {
            log::warn!("[hotkeys] cancel hotkey: {error}");
        }
    }

    pub fn unregister_cancel_hotkey(&self) {
        self.unregister_cancel_hotkey_for(CancelHotkeyOwner::Recording);
    }

    /// The integrator settled a requested start (recording, idle again, or
    /// processing). Without this, a Dictation cancelled before any hotkey
    /// press queried its status would still look requested, and the next
    /// press would be taken as a stop.
    pub fn clear_start_request(&self) {
        self.inner.requested.store(false, Ordering::SeqCst);
    }

    /// Escape stays bound until every owner has released it.
    pub fn register_cancel_hotkey_for(&self, owner: CancelHotkeyOwner) -> Result<(), HotkeyError> {
        let mut state = lock(&self.inner.state);
        let State { slots, cancel } = &mut *state;
        cancel.register(slots, self.inner.backend.as_ref(), owner, &CANCEL_KEY)
    }

    pub fn unregister_cancel_hotkey_for(&self, owner: CancelHotkeyOwner) {
        let mut state = lock(&self.inner.state);
        let State { slots, cancel } = &mut *state;
        cancel.unregister(slots, self.inner.backend.as_ref(), owner);
    }

    /// Whether Hold mode works for the first hotkey of `hotkey_list` (or the
    /// active primary hotkey when the list is empty).
    pub fn mode_info(&self, hotkey_list: &str) -> HotkeyModeInfo {
        let hotkey = parse_hotkey_list(hotkey_list)
            .into_iter()
            .next()
            .or_else(|| self.primary_hotkey());
        let supports = hotkey.is_none_or(|hotkey| hotkey.supports_hold());
        HotkeyModeInfo {
            supports_push_to_talk: supports,
            push_to_talk_unavailable_reason: (!supports)
                .then(|| "hotkey.errors.holdNeedsReleaseKey".to_string()),
        }
    }

    /// The active Dictation hotkeys.
    pub fn active_hotkeys(&self) -> Vec<Hotkey> {
        lock(&self.inner.state).slots.slot_hotkeys(DICTATION_SLOT)
    }

    fn primary_hotkey(&self) -> Option<Hotkey> {
        self.active_hotkeys().into_iter().next()
    }

    /// Listener events for the hotkey capture UI. Each call adds a subscriber.
    pub fn subscribe_raw_keys(&self) -> async_channel::Receiver<RawKeyEvent> {
        let (tx, rx) = async_channel::unbounded();
        lock(&self.inner.raw).push(tx);
        rx
    }

    /// Unregisters everything and stops the listener, which puts the user's
    /// macOS Globe action back.
    pub fn shutdown(&self) {
        {
            let mut state = lock(&self.inner.state);
            let backend = Arc::clone(&self.inner.backend);
            state.slots.unregister_all(backend.as_ref());
        }
        self.inner.gesture.reset();
        self.inner.globe.stop();
    }
}

impl Inner {
    fn activation_mode(&self) -> ActivationMode {
        mode_from_u8(self.mode.load(Ordering::SeqCst))
    }

    fn is_active(&self) -> bool {
        if self.status.is_active() || self.status.is_processing() {
            self.requested.store(false, Ordering::SeqCst);
            return self.status.is_active();
        }
        self.requested.load(Ordering::SeqCst)
    }

    fn send(&self, command: HotkeyCommand) {
        if let Err(error) = self.events.try_send(command) {
            log::warn!("[hotkeys] dropped command: {error}");
        }
    }

    /// Drops starts while capturing a hotkey or transcribing and stops while
    /// capturing, tracks the requested Dictation, then sends.
    fn emit(&self, command: HotkeyCommand) {
        let listening = self.listening.load(Ordering::SeqCst);
        match &command {
            HotkeyCommand::Toggle | HotkeyCommand::Start | HotkeyCommand::Prepare => {
                if listening || self.status.is_processing() {
                    return;
                }
                let starting = !matches!(command, HotkeyCommand::Toggle) || !self.is_active();
                self.requested.store(starting, Ordering::SeqCst);
            }
            HotkeyCommand::Stop | HotkeyCommand::Cancel => {
                if listening {
                    return;
                }
                self.requested.store(false, Ordering::SeqCst);
            }
            HotkeyCommand::CancelPreparation | HotkeyCommand::ForceStop { .. } => {
                self.requested.store(false, Ordering::SeqCst);
            }
            _ => {}
        }
        self.send(command);
    }

    fn notify_failure(&self, error: &HotkeyError) {
        self.send(HotkeyCommand::RegistrationFailed {
            hotkey: error.hotkey.clone(),
            message_key: error.message_key().to_string(),
        });
    }

    fn after_hotkey_change(&self) {
        self.gesture.reset();
        self.sync_listener_config();
    }

    fn sync_listener_config(&self) {
        let config = if self.listening.load(Ordering::SeqCst) {
            ListenerConfig {
                mouse_buttons: Vec::new(),
                suppress_globe_action: true,
            }
        } else {
            lock(&self.state)
                .slots
                .native_listener_config(&[DICTATION_SLOT])
        };
        self.globe.set_configuration(config);
    }

    fn broadcast_raw(&self, event: RawKeyEvent) {
        lock(&self.raw).retain(|tx| tx.try_send(event.clone()).is_ok() || !tx.is_closed());
    }

    fn dictation_has_listener_key(&self, key: ListenerKey) -> bool {
        lock(&self.state)
            .slots
            .slot_hotkeys(DICTATION_SLOT)
            .iter()
            .any(|hotkey| hotkey.listener_key() == Some(key))
    }

    fn on_key_down(&self, key: ListenerKey) {
        self.broadcast_raw(RawKeyEvent::KeyDown(key));
        if self.dictation_has_listener_key(key) && !self.listening.load(Ordering::SeqCst) {
            self.gesture.press(key);
        }
    }

    /// The release ends a press of the key itself and, for Globe (Fn) and
    /// right-side modifiers, a Hold mode chord using that modifier.
    fn on_key_up(&self, key: ListenerKey) {
        self.broadcast_raw(RawKeyEvent::KeyUp(key));
        self.gesture.release(key);
        if let Some(modifier) = key.modifier() {
            self.push.handle_modifier_up(modifier);
        }
    }

    fn on_globe_event(&self, event: GlobeEvent) {
        match event {
            GlobeEvent::GlobeDown => self.on_key_down(ListenerKey::Globe),
            GlobeEvent::GlobeUp => self.on_key_up(ListenerKey::Globe),
            GlobeEvent::RightModifierDown(name) | GlobeEvent::MouseButtonDown(name) => {
                if let Some(key) = ListenerKey::parse(&name) {
                    self.on_key_down(key);
                }
            }
            GlobeEvent::RightModifierUp(name) | GlobeEvent::MouseButtonUp(name) => {
                if let Some(key) = ListenerKey::parse(&name) {
                    self.on_key_up(key);
                }
            }
            GlobeEvent::ModifierUp(name) => {
                if let Some(modifier) = Modifier::from_listener_name(&name) {
                    self.broadcast_raw(RawKeyEvent::ModifierUp(modifier));
                    self.push.handle_modifier_up(modifier);
                }
            }
            GlobeEvent::Interrupted => {
                self.broadcast_raw(RawKeyEvent::Interrupted);
                self.gesture.interrupt();
            }
            GlobeEvent::ListenerStopped => {
                self.gesture.release_lost();
                self.push.force_stop(ForceStopReason::ListenerLost);
            }
            GlobeEvent::Error(message) => self.send(HotkeyCommand::ListenerError { message }),
        }
    }

    /// A registered shortcut fired.
    fn on_shortcut(&self, target: ShortcutTarget, state: HotKeyState) {
        if state != HotKeyState::Pressed {
            return;
        }
        if target.slot == CANCEL_SLOT {
            self.send(HotkeyCommand::CancelKeyPressed);
            return;
        }
        if target.slot != DICTATION_SLOT
            || self.listening.load(Ordering::SeqCst)
            || self.status.is_processing()
        {
            return;
        }
        if self.activation_mode() == ActivationMode::Push && target.hotkey.supports_hold() {
            self.push.start(&target.hotkey);
            return;
        }
        let now = self.timers.now_ms();
        if !lock(&self.repeat_gate).is_press(now) {
            return;
        }
        self.emit(HotkeyCommand::Toggle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::hotkeys::globe::ChildEvent;
    use crate::native::hotkeys::globe::tests::FakeSpawner;
    use crate::native::hotkeys::slots::tests::FakeBackend;
    use crate::native::timer::FakeTimers;
    use std::path::PathBuf;
    use std::time::Duration;

    #[derive(Default)]
    struct Status {
        active: AtomicBool,
        processing: AtomicBool,
    }

    impl DictationStatus for Status {
        fn is_active(&self) -> bool {
            self.active.load(Ordering::SeqCst)
        }
        fn is_processing(&self) -> bool {
            self.processing.load(Ordering::SeqCst)
        }
    }

    struct Harness {
        manager: HotkeyManager,
        status: Arc<Status>,
        rx: async_channel::Receiver<HotkeyCommand>,
        backend: Arc<FakeBackend>,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
    }

    fn harness() -> Harness {
        let status = Arc::new(Status::default());
        let (tx, rx) = async_channel::unbounded();
        let backend = Arc::new(FakeBackend::default());
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::new();
        let globe_spawner = spawner.clone();
        let globe_timers = timers.clone();
        let manager = HotkeyManager::with_parts(
            status.clone(),
            tx,
            Parts {
                backend: backend.clone(),
                timers: timers.clone(),
                globe: Box::new(move |sink| {
                    GlobeListener::new(
                        GlobeOptions {
                            preference_state_path: None,
                            spawner: globe_spawner,
                            resolve_binary: Arc::new(|| Some(PathBuf::from("/fake/listener"))),
                            timers: globe_timers,
                            recovery_timeout: Duration::from_millis(10),
                            recovery_termination_timeout: Duration::from_millis(10),
                        },
                        sink,
                    )
                }),
            },
        );
        Harness {
            manager,
            status,
            rx,
            backend,
            spawner,
            timers,
        }
    }

    impl Harness {
        fn drain(&self) -> Vec<HotkeyCommand> {
            std::iter::from_fn(|| self.rx.try_recv().ok()).collect()
        }
        fn start_listener(&self) {
            self.manager.start_native_listener();
            for _ in 0..1000 {
                if self.spawner.count() > 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("listener never spawned");
        }
        fn line(&self, line: &str) {
            let index = self.spawner.count() - 1;
            self.spawner
                .emit(index, ChildEvent::Stdout(format!("{line}\n")));
        }
        fn fire(&self, accelerator: &str) {
            let target = self.backend.target(accelerator).expect("registered");
            self.manager.inner.on_shortcut(target, HotKeyState::Pressed);
        }
    }

    #[test]
    fn globe_double_tap_starts_and_next_press_stops() {
        let h = harness();
        h.manager.register("GLOBE", ActivationMode::Tap).unwrap();
        h.start_listener();
        assert_eq!(
            h.drain(),
            vec![HotkeyCommand::ActiveHotkeyChanged("GLOBE".into())]
        );
        // The listener was told to keep the macOS Globe action down.
        assert!(
            h.spawner
                .args(0)
                .contains(&"--suppress-system-globe-action".to_string())
        );

        h.line("FN_DOWN");
        h.timers.advance(50);
        h.line("FN_UP");
        h.timers.advance(100);
        h.line("FN_DOWN");
        assert_eq!(h.drain(), vec![HotkeyCommand::Start]);
        h.line("FN_UP");
        h.timers.advance(1000);
        // Not yet reported active by the integrator: the request still counts.
        h.line("FN_DOWN");
        h.line("FN_UP");
        assert_eq!(h.drain(), vec![HotkeyCommand::Stop]);
    }

    #[test]
    fn right_modifier_hold_and_listener_loss() {
        let h = harness();
        h.manager
            .register("RightAlt", ActivationMode::Push)
            .unwrap();
        h.start_listener();
        h.drain();
        h.line("RIGHT_MOD_DOWN:RightOption");
        h.timers.advance(200);
        h.status.active.store(true, Ordering::SeqCst);
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(1),
                signal: None,
            },
        );
        let commands = h.drain();
        assert_eq!(
            commands[..4],
            [
                HotkeyCommand::Prepare,
                HotkeyCommand::Start,
                HotkeyCommand::ForceStop {
                    reason: ForceStopReason::ListenerLost
                },
                HotkeyCommand::Stop,
            ]
        );
        assert!(matches!(commands[4], HotkeyCommand::ListenerError { .. }));
    }

    #[test]
    fn key_chord_toggles_in_tap_mode_with_repeat_gate() {
        let h = harness();
        h.manager
            .register("Control+Shift+Space", ActivationMode::Tap)
            .unwrap();
        h.drain();
        h.fire("Control+Shift+Space");
        h.timers.advance(100);
        h.fire("Control+Shift+Space");
        h.timers.advance(700);
        h.fire("Control+Shift+Space");
        assert_eq!(
            h.drain(),
            vec![HotkeyCommand::Toggle, HotkeyCommand::Toggle]
        );
    }

    #[test]
    fn key_chord_holds_until_a_modifier_goes_up() {
        let h = harness();
        h.manager
            .register("Control+Shift+Space", ActivationMode::Push)
            .unwrap();
        h.start_listener();
        h.drain();
        h.fire("Control+Shift+Space");
        h.timers.advance(150);
        h.line("MODIFIER_UP:shift");
        assert_eq!(
            h.drain(),
            vec![
                HotkeyCommand::Prepare,
                HotkeyCommand::Start,
                HotkeyCommand::Stop
            ]
        );
    }

    #[test]
    fn listening_mode_suspends_shortcuts_and_restores_them() {
        let h = harness();
        h.manager.register("F8,GLOBE", ActivationMode::Tap).unwrap();
        h.drain();
        assert!(h.backend.is_registered("F8"));
        h.manager.set_listening_mode(true);
        assert!(!h.backend.is_registered("F8"));
        h.start_listener();
        h.line("FN_DOWN");
        h.line("FN_UP");
        h.line("FN_DOWN");
        assert!(h.drain().is_empty());
        h.manager.set_listening_mode(false);
        assert!(h.backend.is_registered("F8"));
    }

    #[test]
    fn raw_events_reach_subscribers() {
        let h = harness();
        let raw = h.manager.subscribe_raw_keys();
        h.start_listener();
        h.line("FN_DOWN");
        h.line("RIGHT_MOD_DOWN:RightCommand");
        h.line("MOUSE_BUTTON_UP:MouseButton4");
        let events: Vec<_> = std::iter::from_fn(|| raw.try_recv().ok()).collect();
        assert_eq!(
            events,
            vec![
                RawKeyEvent::KeyDown(ListenerKey::Globe),
                RawKeyEvent::KeyDown(ListenerKey::RightModifier(Modifier::Command)),
                RawKeyEvent::KeyUp(ListenerKey::MouseButton(4)),
            ]
        );
    }

    #[test]
    fn hold_mode_needs_a_release_key_and_failures_roll_back() {
        let h = harness();
        let err = h.manager.register("F8", ActivationMode::Push).unwrap_err();
        assert_eq!(err.reason, HotkeyFailureReason::HoldNeedsReleaseKey);
        assert_eq!(err.message_key(), "hotkey.errors.holdNeedsReleaseKey");
        h.manager.register("F7", ActivationMode::Tap).unwrap();
        assert!(
            h.manager
                .register("F7,Control+BAD", ActivationMode::Tap)
                .is_err()
        );
        assert_eq!(serialize_hotkey_list(&h.manager.active_hotkeys()), "F7");
        assert!(!h.manager.set_activation_mode(ActivationMode::Push));
        assert!(h.drain().iter().any(|c| matches!(
            c,
            HotkeyCommand::RegistrationFailed { message_key, .. }
                if message_key == "hotkey.errors.holdNeedsReleaseKey"
        )));
        let info = h.manager.mode_info("");
        assert!(!info.supports_push_to_talk);
        assert!(h.manager.mode_info("GLOBE,F8").supports_push_to_talk);
    }

    #[test]
    fn startup_registration_falls_back_to_globe() {
        let h = harness();
        let result = h.manager.register_saved("Fn+A", ActivationMode::Push);
        assert_eq!(result.active, "GLOBE");
        assert_eq!(result.mode, ActivationMode::Push);
        let result = h.manager.register_saved("F9", ActivationMode::Push);
        assert_eq!(result.active, "F9");
        assert_eq!(result.mode, ActivationMode::Tap);
    }

    #[test]
    fn cancel_key_press_is_forwarded() {
        let h = harness();
        h.manager.register_cancel_hotkey();
        h.fire("Esc");
        assert_eq!(h.drain(), vec![HotkeyCommand::CancelKeyPressed]);
        h.manager.unregister_cancel_hotkey();
        assert!(!h.backend.is_registered("Esc"));
    }
}

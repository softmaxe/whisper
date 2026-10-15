//! Named slots ("dictation", "cancel") each holding a list of hotkeys, with
//! registration, rollback and conflict detection. The OS registrar is
//! abstracted as [`ShortcutBackend`].

use std::collections::BTreeMap;

use super::keys::{
    is_modifier_only_hotkey, is_right_side_modifier, is_unsupported_fn_combination,
    normalize_to_accelerator,
};
use super::{HotkeyError, HotkeyFailureReason};
use crate::text::hotkeys::{is_globe_like_hotkey, is_mouse_button_hotkey, parse_hotkey_list};

pub const DICTATION_SLOT: &str = "dictation";
pub const CANCEL_SLOT: &str = "cancel";

const SUGGESTED_SINGLE: [&str; 5] = ["F8", "F9", "F10", "Pause", "ScrollLock"];
const SUGGESTED_COMPOUND: [&str; 3] = ["Control+Alt", "Alt+Command", "Command+Shift+Space"];

/// Who a registered accelerator belongs to, for event dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutTarget {
    pub slot: String,
    pub hotkey: String,
}

/// The OS shortcut registrar (Carbon via `global-hotkey` in production).
pub trait ShortcutBackend: Send + Sync {
    /// Registers `accelerator`; false when the OS or another app refuses it.
    fn register(&self, accelerator: &str, target: ShortcutTarget) -> bool;
    fn unregister(&self, accelerator: &str);
    fn is_registered(&self, accelerator: &str) -> bool;
}

#[derive(Debug, Clone, Default)]
struct Slot {
    hotkeys: Vec<String>,
    // Mirrors `hotkeys`; `None` for hotkeys a native listener observes.
    accelerators: Vec<Option<String>>,
}

/// Successful registration: every hotkey that registered, plus best-effort
/// failures (only non-atomic registrations report these).
#[derive(Debug, Clone, Default)]
pub struct SetupOutcome {
    pub hotkeys: Vec<String>,
    pub failures: Vec<HotkeyError>,
}

/// Which mouse buttons the listener must swallow, and whether the app owns
/// Globe (macOS's own Globe action then has to stand down).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenerConfig {
    pub mouse_buttons: Vec<String>,
    pub suppress_globe_action: bool,
}

#[derive(Debug, Default)]
pub struct SlotRegistry {
    slots: BTreeMap<String, Slot>,
}

fn suggestions_for(failed: &str) -> Vec<String> {
    let pool: &[&str] = if failed.contains('+') {
        &SUGGESTED_COMPOUND
    } else {
        &SUGGESTED_SINGLE
    };
    pool.iter()
        .filter(|s| **s != failed)
        .take(3)
        .map(|s| s.to_string())
        .collect()
}

impl SlotRegistry {
    pub fn new() -> Self {
        let mut slots = BTreeMap::new();
        slots.insert(
            DICTATION_SLOT.to_string(),
            Slot {
                hotkeys: vec!["GLOBE".into()],
                accelerators: vec![None],
            },
        );
        Self { slots }
    }

    #[cfg(test)]
    pub fn with_slot(name: &str, hotkeys: &[&str]) -> Self {
        let mut registry = Self::default();
        registry.slots.insert(
            name.to_string(),
            Slot {
                hotkeys: hotkeys.iter().map(|s| s.to_string()).collect(),
                accelerators: vec![None; hotkeys.len()],
            },
        );
        registry
    }

    pub fn slot_hotkeys(&self, slot: &str) -> Vec<String> {
        self.slots
            .get(slot)
            .map(|s| s.hotkeys.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn slot_has_hotkey(&self, slot: &str, key: &str) -> bool {
        !key.is_empty()
            && self
                .slots
                .get(slot)
                .is_some_and(|s| s.hotkeys.iter().any(|h| h == key))
    }

    /// All accelerators registered with the backend, by slot.
    pub fn accelerators(&self) -> Vec<(String, String, String)> {
        self.slots
            .iter()
            .flat_map(|(name, slot)| {
                slot.hotkeys
                    .iter()
                    .zip(&slot.accelerators)
                    .filter_map(|(hotkey, accel)| {
                        accel
                            .as_ref()
                            .map(|a| (name.clone(), hotkey.clone(), a.clone()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn native_listener_config(&self, slots: &[&str]) -> ListenerConfig {
        let mut config = ListenerConfig::default();
        for slot in slots {
            for hotkey in self.slot_hotkeys(slot) {
                if is_mouse_button_hotkey(&hotkey) {
                    let canonical = format!("MouseButton{}", &hotkey[hotkey.len() - 1..]);
                    if !config.mouse_buttons.contains(&canonical) {
                        config.mouse_buttons.push(canonical);
                    }
                } else if is_globe_like_hotkey(&hotkey) {
                    config.suppress_globe_action = true;
                }
            }
        }
        config.mouse_buttons.sort();
        config
    }

    fn failure(hotkey: &str, reason: HotkeyFailureReason, suggestions: Vec<String>) -> HotkeyError {
        HotkeyError {
            hotkey: hotkey.to_string(),
            reason,
            suggestions,
        }
    }

    // Registers one hotkey without touching any slot. Ok(None) for hotkeys a
    // native listener observes.
    fn register_single(
        &self,
        backend: &dyn ShortcutBackend,
        slot: &str,
        hotkey: &str,
    ) -> Result<Option<String>, HotkeyError> {
        if is_mouse_button_hotkey(hotkey) || is_globe_like_hotkey(hotkey) {
            return Ok(None);
        }
        // Fn cannot be part of an accelerator; registering Fn+A as A would
        // claim an ordinary typing key globally.
        if is_unsupported_fn_combination(hotkey) {
            return Err(Self::failure(
                hotkey,
                HotkeyFailureReason::FnCombinationUnsupported,
                Vec::new(),
            ));
        }
        if is_right_side_modifier(hotkey) {
            return Ok(None);
        }
        let accelerator = normalize_to_accelerator(hotkey);
        let target = ShortcutTarget {
            slot: slot.to_string(),
            hotkey: hotkey.to_string(),
        };
        if backend.register(&accelerator, target) {
            return Ok(Some(accelerator));
        }
        let reason = if backend.is_registered(&accelerator) {
            HotkeyFailureReason::AlreadyRegistered
        } else {
            HotkeyFailureReason::RegistrationFailed
        };
        log::warn!("[hotkeys] failed to register \"{hotkey}\": {reason:?}");
        Err(Self::failure(hotkey, reason, suggestions_for(&accelerator)))
    }

    pub fn find_slot_conflict(&self, slot: &str, hotkey: &str) -> Option<HotkeyError> {
        let accelerator = if is_globe_like_hotkey(hotkey)
            || is_mouse_button_hotkey(hotkey)
            || is_right_side_modifier(hotkey)
            || is_modifier_only_hotkey(hotkey)
        {
            None
        } else {
            Some(normalize_to_accelerator(hotkey))
        };
        self.slots
            .iter()
            .filter(|(name, _)| name.as_str() != slot)
            .find(|(_, other)| {
                other.hotkeys.iter().any(|h| h == hotkey)
                    || accelerator
                        .as_ref()
                        .is_some_and(|a| other.accelerators.iter().flatten().any(|o| o == a))
            })
            .map(|(name, _)| {
                log::warn!("[hotkeys] \"{hotkey}\" conflicts with slot \"{name}\"");
                Self::failure(
                    hotkey,
                    HotkeyFailureReason::SlotConflict { slot: name.clone() },
                    Vec::new(),
                )
            })
    }

    /// Registers a slot's hotkey list. Best-effort by default: succeeds when at
    /// least one hotkey registers, reporting the rest. `atomic` rolls the slot
    /// back to its previous bindings on any failure.
    pub fn setup_shortcuts(
        &mut self,
        backend: &dyn ShortcutBackend,
        hotkey_list: &str,
        slot_name: &str,
        atomic: bool,
    ) -> Result<SetupOutcome, HotkeyError> {
        let desired = parse_hotkey_list(hotkey_list);
        if desired.is_empty() {
            return Err(Self::failure(
                "",
                HotkeyFailureReason::RegistrationFailed,
                Vec::new(),
            ));
        }
        // Reject conflicts before tearing down this slot's registration.
        for hotkey in &desired {
            if let Some(conflict) = self.find_slot_conflict(slot_name, hotkey) {
                return Err(conflict);
            }
        }

        let previous = self.slots.get(slot_name).cloned().unwrap_or_default();
        for accel in previous.accelerators.iter().flatten() {
            backend.unregister(accel);
        }

        let mut registered = Slot::default();
        let mut failures = Vec::new();
        for hotkey in &desired {
            match self.register_single(backend, slot_name, hotkey) {
                Ok(accelerator) => {
                    registered.hotkeys.push(hotkey.clone());
                    registered.accelerators.push(accelerator);
                }
                Err(error) => failures.push(error),
            }
        }

        if registered.hotkeys.is_empty() || (atomic && !failures.is_empty()) {
            // Roll back: drop what was just registered and restore the
            // previous bindings so the slot keeps working.
            for accel in registered.accelerators.iter().flatten() {
                backend.unregister(accel);
            }
            for (hotkey, accel) in previous.hotkeys.iter().zip(&previous.accelerators) {
                let Some(accel) = accel else { continue };
                let target = ShortcutTarget {
                    slot: slot_name.to_string(),
                    hotkey: hotkey.clone(),
                };
                if !backend.register(accel, target) {
                    log::warn!("[hotkeys] could not restore previous hotkey \"{hotkey}\"");
                }
            }
            self.slots.insert(slot_name.to_string(), previous);
            let error = failures.into_iter().next().unwrap_or_else(|| {
                Self::failure(
                    &desired[0],
                    HotkeyFailureReason::RegistrationFailed,
                    Vec::new(),
                )
            });
            return Err(error);
        }

        let hotkeys = registered.hotkeys.clone();
        self.slots.insert(slot_name.to_string(), registered);
        log::info!(
            "[hotkeys] slot \"{slot_name}\" registered: \"{}\"",
            hotkeys.join(", ")
        );
        Ok(SetupOutcome { hotkeys, failures })
    }

    /// Releases what the slot registered; native-listener entries need nothing.
    pub fn unregister_slot(&mut self, backend: &dyn ShortcutBackend, slot_name: &str) {
        let Some(slot) = self.slots.get_mut(slot_name) else {
            return;
        };
        for accel in slot.accelerators.iter().flatten() {
            backend.unregister(accel);
        }
        slot.hotkeys.clear();
        slot.accelerators.clear();
    }

    pub fn unregister_all(&mut self, backend: &dyn ShortcutBackend) {
        for slot in self.slots.values_mut() {
            for accel in slot.accelerators.iter().flatten() {
                backend.unregister(accel);
            }
            slot.hotkeys.clear();
            slot.accelerators.clear();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! Fn guard, slot rollback, slot unregistration and native listener keys.
    use super::*;
    use crate::native::timer::lock;
    use std::sync::Mutex;

    /// Stand-in for globalShortcut: accelerators containing "BAD" fail.
    #[derive(Default)]
    pub struct FakeBackend {
        pub registered: Mutex<Vec<(String, ShortcutTarget)>>,
        pub unregister_calls: Mutex<Vec<String>>,
        /// Refuses the next N registrations.
        pub refuse_next: Mutex<usize>,
    }

    impl FakeBackend {
        pub fn keys(&self) -> Vec<String> {
            lock(&self.registered)
                .iter()
                .map(|(a, _)| a.clone())
                .collect()
        }
        pub fn target(&self, accel: &str) -> Option<ShortcutTarget> {
            lock(&self.registered)
                .iter()
                .find(|(a, _)| a == accel)
                .map(|(_, t)| t.clone())
        }
    }

    impl ShortcutBackend for FakeBackend {
        fn register(&self, accelerator: &str, target: ShortcutTarget) -> bool {
            {
                let mut refuse = lock(&self.refuse_next);
                if *refuse > 0 {
                    *refuse -= 1;
                    return false;
                }
            }
            let mut registered = lock(&self.registered);
            if accelerator.contains("BAD") || registered.iter().any(|(a, _)| a == accelerator) {
                return false;
            }
            registered.push((accelerator.to_string(), target));
            true
        }
        fn unregister(&self, accelerator: &str) {
            lock(&self.unregister_calls).push(accelerator.to_string());
            lock(&self.registered).retain(|(a, _)| a != accelerator);
        }
        fn is_registered(&self, accelerator: &str) -> bool {
            lock(&self.registered).iter().any(|(a, _)| a == accelerator)
        }
    }

    #[test]
    fn fn_combination_rejected_without_registering_base_key() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        let err = registry
            .setup_shortcuts(&backend, "Fn+A", DICTATION_SLOT, true)
            .unwrap_err();
        assert_eq!(err.reason, HotkeyFailureReason::FnCombinationUnsupported);
        assert!(backend.keys().is_empty());
    }

    #[test]
    fn rejecting_fn_combination_preserves_previous_binding() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "Control+Shift+A", DICTATION_SLOT, true)
            .unwrap();
        assert!(
            registry
                .setup_shortcuts(&backend, "Fn+A", DICTATION_SLOT, true)
                .is_err()
        );
        assert_eq!(backend.keys(), ["Control+Shift+A"]);
        assert_eq!(
            backend.target("Control+Shift+A").map(|t| t.hotkey),
            Some("Control+Shift+A".to_string())
        );
    }

    #[test]
    fn atomic_failure_keeps_previous_working_hotkeys() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "F7", DICTATION_SLOT, true)
            .unwrap();
        assert_eq!(registry.slot_hotkeys(DICTATION_SLOT), ["F7"]);
        assert!(
            registry
                .setup_shortcuts(&backend, "F7,Control+BAD", DICTATION_SLOT, true)
                .is_err()
        );
        assert_eq!(registry.slot_hotkeys(DICTATION_SLOT), ["F7"]);
        assert!(backend.is_registered("F7"));
    }

    #[test]
    fn non_atomic_keeps_working_subset_and_reports_failures() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        let outcome = registry
            .setup_shortcuts(&backend, "F7,Control+BAD", DICTATION_SLOT, false)
            .unwrap();
        assert_eq!(outcome.hotkeys, ["F7"]);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].hotkey, "Control+BAD");
    }

    #[test]
    fn re_registering_replaces_previous_accelerators() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "F7", DICTATION_SLOT, true)
            .unwrap();
        registry
            .setup_shortcuts(&backend, "F6,F5", DICTATION_SLOT, true)
            .unwrap();
        assert_eq!(registry.slot_hotkeys(DICTATION_SLOT), ["F6", "F5"]);
        assert!(!backend.is_registered("F7"));
        assert!(backend.is_registered("F6"));
        assert!(backend.is_registered("F5"));
    }

    #[test]
    fn unregister_slot_releases_only_its_own_accelerators() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "F7,Control+Shift+K", DICTATION_SLOT, true)
            .unwrap();
        registry
            .setup_shortcuts(&backend, "F8", CANCEL_SLOT, true)
            .unwrap();
        lock(&backend.unregister_calls).clear();
        registry.unregister_slot(&backend, DICTATION_SLOT);
        assert_eq!(*lock(&backend.unregister_calls), ["F7", "Control+Shift+K"]);
        assert_eq!(backend.keys(), ["F8"]);
        assert!(registry.slot_hotkeys(DICTATION_SLOT).is_empty());
        assert_eq!(registry.slot_hotkeys(CANCEL_SLOT), ["F8"]);
    }

    #[test]
    fn freed_combo_can_be_registered_again() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "Control+Shift+K", DICTATION_SLOT, true)
            .unwrap();
        registry.unregister_slot(&backend, DICTATION_SLOT);
        assert!(
            registry
                .setup_shortcuts(&backend, "Control+Shift+K", DICTATION_SLOT, true)
                .is_ok()
        );
    }

    #[test]
    fn unregister_skips_native_listener_hotkeys_and_is_idempotent() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "F7,RightShift", DICTATION_SLOT, true)
            .unwrap();
        lock(&backend.unregister_calls).clear();
        registry.unregister_slot(&backend, DICTATION_SLOT);
        assert_eq!(*lock(&backend.unregister_calls), ["F7"]);
        lock(&backend.unregister_calls).clear();
        registry.unregister_slot(&backend, DICTATION_SLOT);
        assert!(lock(&backend.unregister_calls).is_empty());
    }

    #[test]
    fn conflicts_with_another_slot_are_rejected_before_teardown() {
        let backend = FakeBackend::default();
        let mut registry = SlotRegistry::new();
        registry
            .setup_shortcuts(&backend, "Escape", CANCEL_SLOT, true)
            .unwrap();
        registry
            .setup_shortcuts(&backend, "F7", DICTATION_SLOT, true)
            .unwrap();
        let err = registry
            .setup_shortcuts(&backend, "Escape", DICTATION_SLOT, true)
            .unwrap_err();
        assert_eq!(
            err.reason,
            HotkeyFailureReason::SlotConflict {
                slot: CANCEL_SLOT.into()
            }
        );
        assert!(backend.is_registered("F7"));
    }

    #[test]
    fn os_conflicts_report_already_registered_with_suggestions() {
        let backend = FakeBackend::default();
        backend.register(
            "Control+Shift+K",
            ShortcutTarget {
                slot: "other".into(),
                hotkey: "x".into(),
            },
        );
        let mut registry = SlotRegistry::new();
        let err = registry
            .setup_shortcuts(&backend, "F9", DICTATION_SLOT, true)
            .and_then(|_| registry.setup_shortcuts(&backend, "Control+Shift+K", "x", true))
            .unwrap_err();
        assert_eq!(err.reason, HotkeyFailureReason::AlreadyRegistered);
        assert_eq!(err.suggestions.len(), 3);
    }

    #[test]
    fn globe_and_fn_request_native_suppression() {
        for hotkey in ["GLOBE", "Fn"] {
            let registry = SlotRegistry::with_slot(DICTATION_SLOT, &["Control+Shift+R", hotkey]);
            assert_eq!(
                registry.native_listener_config(&[DICTATION_SLOT]),
                ListenerConfig {
                    mouse_buttons: vec![],
                    suppress_globe_action: true
                }
            );
        }
    }

    #[test]
    fn mouse_hotkeys_collected_once() {
        let registry = SlotRegistry::with_slot(
            DICTATION_SLOT,
            &["MouseButton4", "MouseButton5", "MouseButton4"],
        );
        assert_eq!(
            registry.native_listener_config(&[DICTATION_SLOT]),
            ListenerConfig {
                mouse_buttons: vec!["MouseButton4".into(), "MouseButton5".into()],
                suppress_globe_action: false
            }
        );
        let plain = SlotRegistry::with_slot(DICTATION_SLOT, &["Control+Shift+R"]);
        assert_eq!(
            plain.native_listener_config(&[DICTATION_SLOT]),
            ListenerConfig::default()
        );
    }

    #[test]
    fn dictation_lookup_handles_primary_and_secondary() {
        let registry = SlotRegistry::with_slot(DICTATION_SLOT, &["GLOBE", "Control+Shift+R"]);
        assert!(registry.slot_has_hotkey(DICTATION_SLOT, "Control+Shift+R"));
        assert!(!registry.slot_has_hotkey(DICTATION_SLOT, "F12"));
        assert_eq!(
            registry.slot_hotkeys(DICTATION_SLOT),
            ["GLOBE", "Control+Shift+R"]
        );
    }
}

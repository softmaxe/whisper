//! Shared ownership of the Escape (cancel) binding. Recording completion and
//! copy recovery can overlap, so the binding stays until both have released
//! it.

use super::slots::{CANCEL_SLOT, ShortcutBackend, SlotRegistry};
use super::{HotkeyError, HotkeyFailureReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelHotkeyOwner {
    Recording,
    CopyRecovery,
}

#[derive(Debug, Default)]
pub struct CancelBinding {
    owners: Vec<CancelHotkeyOwner>,
    key: Option<String>,
}

impl CancelBinding {
    pub fn register(
        &mut self,
        slots: &mut SlotRegistry,
        backend: &dyn ShortcutBackend,
        owner: CancelHotkeyOwner,
        key: &str,
    ) -> Result<(), HotkeyError> {
        if !self.owners.is_empty() && self.key.as_deref() == Some(key) {
            if !self.owners.contains(&owner) {
                self.owners.push(owner);
            }
            return Ok(());
        }
        if self.owners.iter().any(|active| *active != owner) {
            return Err(HotkeyError {
                hotkey: key.to_string(),
                reason: HotkeyFailureReason::CancelKeyInUse,
                suggestions: Vec::new(),
            });
        }
        slots.setup_shortcuts(backend, key, CANCEL_SLOT, false)?;
        if !self.owners.contains(&owner) {
            self.owners.push(owner);
        }
        self.key = Some(key.to_string());
        Ok(())
    }

    pub fn unregister(
        &mut self,
        slots: &mut SlotRegistry,
        backend: &dyn ShortcutBackend,
        owner: CancelHotkeyOwner,
    ) {
        if !self.owners.contains(&owner) {
            return;
        }
        if self.owners.len() == 1 {
            slots.unregister_slot(backend, CANCEL_SLOT);
            self.key = None;
        }
        self.owners.retain(|o| *o != owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::hotkeys::slots::tests::FakeBackend;
    use crate::native::timer::lock;
    use CancelHotkeyOwner::{CopyRecovery, Recording};

    struct Harness {
        slots: SlotRegistry,
        backend: FakeBackend,
        binding: CancelBinding,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                slots: SlotRegistry::new(),
                backend: FakeBackend::default(),
                binding: CancelBinding::default(),
            }
        }
        fn register(&mut self, owner: CancelHotkeyOwner, key: &str) -> Result<(), HotkeyError> {
            self.binding
                .register(&mut self.slots, &self.backend, owner, key)
        }
        fn unregister(&mut self, owner: CancelHotkeyOwner) {
            self.binding
                .unregister(&mut self.slots, &self.backend, owner);
        }
        fn escape_bound(&self) -> bool {
            self.backend.is_registered("Escape")
        }
        fn registrations(&self) -> usize {
            // Every successful registration adds one entry; unregisters remove it.
            lock(&self.backend.registered).len()
        }
    }

    #[test]
    fn registers_and_releases_escape() {
        let mut h = Harness::new();
        h.register(Recording, "Escape").unwrap();
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }

    #[test]
    fn late_recording_release_keeps_copy_recovery_binding() {
        let mut h = Harness::new();
        h.register(Recording, "Escape").unwrap();
        h.register(CopyRecovery, "Escape").unwrap();
        h.unregister(Recording);
        h.unregister(Recording);
        assert!(h.escape_bound());
        assert_eq!(h.registrations(), 1);
        h.unregister(CopyRecovery);
        assert!(!h.escape_bound());
    }

    #[test]
    fn copy_recovery_cleanup_keeps_new_recording_binding() {
        let mut h = Harness::new();
        h.register(CopyRecovery, "Escape").unwrap();
        h.register(Recording, "Escape").unwrap();
        h.unregister(CopyRecovery);
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }

    #[test]
    fn failed_registration_retains_no_owner() {
        let mut h = Harness::new();
        *lock(&h.backend.refuse_next) = 1;
        assert!(h.register(CopyRecovery, "Escape").is_err());
        let calls_before = lock(&h.backend.unregister_calls).len();
        h.unregister(CopyRecovery);
        assert_eq!(lock(&h.backend.unregister_calls).len(), calls_before);
        h.register(Recording, "Escape").unwrap();
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }

    #[test]
    fn conflicting_keys_cannot_replace_active_binding() {
        let mut h = Harness::new();
        h.register(CopyRecovery, "Escape").unwrap();
        let err = h.register(Recording, "F12").unwrap_err();
        assert_eq!(err.reason, HotkeyFailureReason::CancelKeyInUse);
        h.unregister(Recording);
        assert!(h.escape_bound());
        assert!(!h.backend.is_registered("F12"));
    }
}

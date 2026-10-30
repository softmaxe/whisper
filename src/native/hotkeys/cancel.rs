//! Shared ownership of the Escape (cancel) binding. Recording completion and
//! copy recovery can overlap, so the binding stays until both have released
//! it.

use std::sync::LazyLock;

use super::HotkeyError;
use super::slots::{CANCEL_SLOT, ShortcutBackend, SlotRegistry};
use crate::text::hotkeys::Hotkey;

static CANCEL_KEY: LazyLock<Hotkey> =
    LazyLock::new(|| Hotkey::parse("Escape").expect("static hotkey"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelHotkeyOwner {
    Recording,
    CopyRecovery,
}

#[derive(Debug, Default)]
pub struct CancelBinding {
    owners: Vec<CancelHotkeyOwner>,
}

impl CancelBinding {
    pub fn register(
        &mut self,
        slots: &mut SlotRegistry,
        backend: &dyn ShortcutBackend,
        owner: CancelHotkeyOwner,
    ) -> Result<(), HotkeyError> {
        if self.owners.is_empty() {
            slots.setup_shortcuts(
                backend,
                std::slice::from_ref(&*CANCEL_KEY),
                CANCEL_SLOT,
                false,
            )?;
        }
        if !self.owners.contains(&owner) {
            self.owners.push(owner);
        }
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
        fn register(&mut self, owner: CancelHotkeyOwner) -> Result<(), HotkeyError> {
            self.binding.register(&mut self.slots, &self.backend, owner)
        }
        fn unregister(&mut self, owner: CancelHotkeyOwner) {
            self.binding
                .unregister(&mut self.slots, &self.backend, owner);
        }
        fn escape_bound(&self) -> bool {
            self.backend.is_registered("Esc")
        }
        fn registrations(&self) -> usize {
            // Every successful registration adds one entry; unregisters remove it.
            lock(&self.backend.registered).len()
        }
    }

    #[test]
    fn registers_and_releases_escape() {
        let mut h = Harness::new();
        h.register(Recording).unwrap();
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }

    #[test]
    fn late_recording_release_keeps_copy_recovery_binding() {
        let mut h = Harness::new();
        h.register(Recording).unwrap();
        h.register(CopyRecovery).unwrap();
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
        h.register(CopyRecovery).unwrap();
        h.register(Recording).unwrap();
        h.unregister(CopyRecovery);
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }

    #[test]
    fn failed_registration_retains_no_owner() {
        let mut h = Harness::new();
        *lock(&h.backend.refuse_next) = 1;
        assert!(h.register(CopyRecovery).is_err());
        let calls_before = lock(&h.backend.unregister_calls).len();
        h.unregister(CopyRecovery);
        assert_eq!(lock(&h.backend.unregister_calls).len(), calls_before);
        h.register(Recording).unwrap();
        assert!(h.escape_bound());
        h.unregister(Recording);
        assert!(!h.escape_bound());
    }
}

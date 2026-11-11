//! Single owner of the Dock icon: it tracks the control panel's visibility,
//! which callers report explicitly.
//!
//! The icon follows the activation policy: Regular shows it, Accessory hides
//! it. Call on the main thread.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::NSOperationQueue;

use super::timer::{RealTimers, lock};

/// Hiding the icon this soon after showing it can leave duplicate Dock icons.
const HIDE_AFTER_SHOW_GUARD: Duration = Duration::from_millis(1000);

/// The process activation policy, abstracted for tests.
pub trait ActivationPolicy {
    fn is_regular(&self) -> bool;
    fn set_regular(&self, regular: bool);
}

struct AppKitPolicy(MainThreadMarker);

impl ActivationPolicy for AppKitPolicy {
    fn is_regular(&self) -> bool {
        NSApplication::sharedApplication(self.0).activationPolicy()
            == NSApplicationActivationPolicy::Regular
    }

    fn set_regular(&self, regular: bool) {
        let policy = if regular {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        NSApplication::sharedApplication(self.0).setActivationPolicy(policy);
    }
}

#[derive(Debug, Default)]
pub struct DockManager {
    control_panel_visible: bool,
    last_shown_at: Option<Instant>,
    generation: u64,
    pending_hide: Option<PendingHide>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingHide {
    generation: u64,
    deadline: Instant,
}

impl DockManager {
    /// Reported by every path that shows or hides the control panel, and once
    /// at startup before any window: a launch that opens the control panel
    /// keeps the icon macOS already shows (dropping to Accessory and back
    /// would hand focus to the previous app); tray-only launches hide it.
    pub fn set_control_panel_visible(
        &mut self,
        policy: &dyn ActivationPolicy,
        visible: bool,
        now: Instant,
    ) -> Option<PendingHide> {
        self.generation += 1;
        self.pending_hide = None;
        self.control_panel_visible = visible;
        self.apply(policy, now);
        self.pending_hide
    }

    fn apply(&mut self, policy: &dyn ActivationPolicy, now: Instant) {
        if self.control_panel_visible {
            if policy.is_regular() {
                return;
            }
            // Not a "dock show": while active it reactivates the app a moment
            // later, which cooperative activation refuses.
            policy.set_regular(true);
            self.last_shown_at = Some(now);
        } else {
            if !policy.is_regular() {
                return;
            }
            if let Some(shown) = self.last_shown_at
                && now < shown + HIDE_AFTER_SHOW_GUARD
            {
                self.pending_hide = Some(PendingHide {
                    generation: self.generation,
                    deadline: shown + HIDE_AFTER_SHOW_GUARD,
                });
                return;
            }
            policy.set_regular(false);
        }
    }

    fn finish_hide(
        &mut self,
        policy: &dyn ActivationPolicy,
        pending: PendingHide,
        now: Instant,
        any_window_visible: bool,
    ) -> Option<PendingHide> {
        if self.pending_hide != Some(pending) || self.control_panel_visible {
            return None;
        }
        self.pending_hide = None;
        if !any_window_visible {
            self.apply(policy, now);
        }
        self.pending_hide
    }
}

fn shared() -> &'static Mutex<DockManager> {
    static SHARED: OnceLock<Mutex<DockManager>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(DockManager::default()))
}

fn with_policy(f: impl FnOnce(&dyn ActivationPolicy)) {
    match MainThreadMarker::new() {
        Some(mtm) => f(&AppKitPolicy(mtm)),
        None => log::warn!("[dock] activation policy changes must run on the main thread"),
    }
}

/// Shows the Dock icon while the control panel is visible, hides it otherwise.
pub fn set_control_panel_visible(visible: bool) {
    with_policy(|policy| {
        let pending = lock(shared()).set_control_panel_visible(policy, visible, Instant::now());
        if let Some(pending) = pending {
            schedule_hide(pending);
        }
    });
}

fn schedule_hide(pending: PendingHide) {
    RealTimers::shared().schedule(
        pending.deadline.saturating_duration_since(Instant::now()),
        Box::new(move || {
            let block = block2::RcBlock::new(move || {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                let visible = NSApplication::sharedApplication(mtm)
                    .windows()
                    .iter()
                    .any(|window| window.isVisible());
                let retry = lock(shared()).finish_hide(
                    &AppKitPolicy(mtm),
                    pending,
                    Instant::now(),
                    visible,
                );
                if let Some(retry) = retry {
                    schedule_hide(retry);
                }
            });
            // AppKit visibility and activation policy must be checked together on the main thread.
            unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakePolicy {
        regular: RefCell<bool>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl FakePolicy {
        fn new(regular: bool) -> Self {
            Self {
                regular: RefCell::new(regular),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }
    }

    impl ActivationPolicy for FakePolicy {
        fn is_regular(&self) -> bool {
            *self.regular.borrow()
        }
        fn set_regular(&self, regular: bool) {
            *self.regular.borrow_mut() = regular;
            self.calls
                .borrow_mut()
                .push(if regular { "regular" } else { "accessory" });
        }
    }

    #[test]
    fn launch_with_control_panel_keeps_the_icon() {
        let policy = FakePolicy::new(true);
        let mut dock = DockManager::default();
        let now = Instant::now();
        dock.set_control_panel_visible(&policy, true, now);
        dock.set_control_panel_visible(&policy, true, now);
        assert!(policy.calls().is_empty());
        assert!(policy.is_regular());
    }

    #[test]
    fn tray_only_launch_hides_the_icon() {
        let policy = FakePolicy::new(true);
        DockManager::default().set_control_panel_visible(&policy, false, Instant::now());
        assert_eq!(policy.calls(), ["accessory"]);
    }

    #[test]
    fn closing_right_after_showing_schedules_hide_at_the_guard_deadline() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let start = Instant::now();
        dock.set_control_panel_visible(&policy, false, start);
        dock.set_control_panel_visible(&policy, true, start);
        // Opening the panel from the tray shows the icon.
        assert_eq!(policy.calls(), ["regular"]);
        policy.calls.borrow_mut().clear();
        let pending = dock
            .set_control_panel_visible(&policy, false, start)
            .unwrap();
        assert!(policy.calls().is_empty());
        assert_eq!(pending.deadline, start + HIDE_AFTER_SHOW_GUARD);
        assert!(
            dock.finish_hide(&policy, pending, pending.deadline, false)
                .is_none()
        );
        assert_eq!(policy.calls(), ["accessory"]);
    }

    #[test]
    fn reopening_invalidates_a_scheduled_hide_even_after_closing_again() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let start = Instant::now();
        dock.set_control_panel_visible(&policy, true, start);
        let old = dock
            .set_control_panel_visible(&policy, false, start)
            .unwrap();
        dock.set_control_panel_visible(&policy, true, start);
        assert!(
            dock.finish_hide(&policy, old, old.deadline, false)
                .is_none()
        );
        assert!(policy.is_regular());
        let new = dock
            .set_control_panel_visible(&policy, false, start)
            .unwrap();
        assert_ne!(old, new);
        dock.finish_hide(&policy, old, old.deadline, false);
        assert!(policy.is_regular());
        dock.finish_hide(&policy, new, new.deadline, false);
        assert!(!policy.is_regular());
    }

    #[test]
    fn scheduled_hide_rechecks_native_window_visibility() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let start = Instant::now();
        dock.set_control_panel_visible(&policy, true, start);
        let pending = dock
            .set_control_panel_visible(&policy, false, start)
            .unwrap();
        assert!(
            dock.finish_hide(&policy, pending, pending.deadline, true)
                .is_none()
        );
        assert!(policy.is_regular());
    }

    #[test]
    fn early_callback_keeps_the_original_deadline() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let start = Instant::now();
        dock.set_control_panel_visible(&policy, true, start);
        let pending = dock
            .set_control_panel_visible(&policy, false, start)
            .unwrap();
        assert_eq!(
            dock.finish_hide(&policy, pending, start, false),
            Some(pending)
        );
        assert!(policy.is_regular());
    }
}

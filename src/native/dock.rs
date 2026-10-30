//! Single owner of the Dock icon: it tracks the control panel's visibility,
//! which callers report explicitly.
//!
//! The icon follows the activation policy: Regular shows it, Accessory hides
//! it. Call on the main thread.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use super::timer::lock;

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
    ) {
        self.control_panel_visible = visible;
        self.apply(policy, now);
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
            if self
                .last_shown_at
                .is_some_and(|shown| now.duration_since(shown) < HIDE_AFTER_SHOW_GUARD)
            {
                return;
            }
            policy.set_regular(false);
        }
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
    with_policy(|policy| lock(shared()).set_control_panel_visible(policy, visible, Instant::now()));
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
    fn opening_the_panel_from_the_tray_shows_the_icon() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let now = Instant::now();
        dock.set_control_panel_visible(&policy, false, now);
        dock.set_control_panel_visible(&policy, true, now);
        assert_eq!(policy.calls(), ["regular"]);
    }

    #[test]
    fn reopening_a_visible_panel_changes_nothing() {
        let policy = FakePolicy::new(true);
        let mut dock = DockManager::default();
        let now = Instant::now();
        dock.set_control_panel_visible(&policy, true, now);
        dock.set_control_panel_visible(&policy, true, now);
        dock.set_control_panel_visible(&policy, true, now);
        assert!(policy.calls().is_empty());
    }

    #[test]
    fn closing_right_after_showing_keeps_the_icon_until_the_next_hide() {
        let policy = FakePolicy::new(false);
        let mut dock = DockManager::default();
        let start = Instant::now();
        dock.set_control_panel_visible(&policy, false, start);
        dock.set_control_panel_visible(&policy, true, start);
        policy.calls.borrow_mut().clear();
        dock.set_control_panel_visible(&policy, false, start);
        assert!(policy.calls().is_empty());
        dock.set_control_panel_visible(&policy, false, start + Duration::from_millis(1000));
        assert_eq!(policy.calls(), ["accessory"]);
    }
}

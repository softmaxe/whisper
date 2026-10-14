//! [`ShortcutBackend`] over `global-hotkey` (Carbon `RegisterEventHotKey`).
//! The manager must be created on the main thread; its events arrive on the
//! crate's global channel and are forwarded by one dispatch thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::super::timer::lock;
use super::keys::parse_accelerator;
use super::slots::{ShortcutBackend, ShortcutTarget};

pub type ShortcutHandler = Arc<dyn Fn(ShortcutTarget, HotKeyState) + Send + Sync>;
type Route = (Weak<CarbonBackend>, ShortcutHandler);

pub struct CarbonBackend {
    manager: Option<GlobalHotKeyManager>,
    registered: Mutex<HashMap<String, (HotKey, ShortcutTarget)>>,
}

impl CarbonBackend {
    /// Call on the main thread.
    pub fn new() -> Arc<Self> {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(error) => {
                log::error!("[hotkeys] global hotkey manager unavailable: {error}");
                None
            }
        };
        Arc::new(Self {
            manager,
            registered: Mutex::new(HashMap::new()),
        })
    }

    fn target_for_id(&self, id: u32) -> Option<ShortcutTarget> {
        lock(&self.registered)
            .values()
            .find(|(hotkey, _)| hotkey.id() == id)
            .map(|(_, target)| target.clone())
    }

    /// Routes this backend's shortcut events to `handler` from a dispatch
    /// thread. A later call replaces the previous route.
    pub fn route_events(self: &Arc<Self>, handler: ShortcutHandler) {
        static ROUTE: OnceLock<Mutex<Option<Route>>> = OnceLock::new();
        static THREAD: OnceLock<()> = OnceLock::new();
        let route = ROUTE.get_or_init(|| Mutex::new(None));
        *lock(route) = Some((Arc::downgrade(self), handler));
        THREAD.get_or_init(|| {
            let spawned = std::thread::Builder::new()
                .name("whisper-hotkey-events".into())
                .spawn(move || {
                    let receiver = GlobalHotKeyEvent::receiver();
                    while let Ok(event) = receiver.recv() {
                        let current = lock(route).clone();
                        let Some((backend, handler)) = current else {
                            continue;
                        };
                        let Some(backend) = backend.upgrade() else {
                            continue;
                        };
                        if let Some(target) = backend.target_for_id(event.id) {
                            handler(target, event.state);
                        }
                    }
                });
            if let Err(error) = spawned {
                log::error!("[hotkeys] failed to start event thread: {error}");
            }
        });
    }
}

impl ShortcutBackend for CarbonBackend {
    fn register(&self, accelerator: &str, target: ShortcutTarget) -> bool {
        let Some(manager) = &self.manager else {
            return false;
        };
        if lock(&self.registered).contains_key(accelerator) {
            return false;
        }
        let hotkey = match parse_accelerator(accelerator) {
            Ok(hotkey) => hotkey,
            Err(error) => {
                log::warn!("[hotkeys] cannot map \"{accelerator}\": {error}");
                return false;
            }
        };
        // Two spellings of one chord share a Carbon registration.
        let duplicate = lock(&self.registered)
            .values()
            .any(|(existing, _)| existing.id() == hotkey.id());
        if duplicate {
            return false;
        }
        match manager.register(hotkey) {
            Ok(()) => {
                lock(&self.registered).insert(accelerator.to_string(), (hotkey, target));
                true
            }
            Err(error) => {
                log::warn!("[hotkeys] RegisterEventHotKey failed for \"{accelerator}\": {error}");
                false
            }
        }
    }

    fn unregister(&self, accelerator: &str) {
        let removed = lock(&self.registered).remove(accelerator);
        if let (Some(manager), Some((hotkey, _))) = (&self.manager, removed)
            && let Err(error) = manager.unregister(hotkey)
        {
            log::warn!("[hotkeys] unregister failed for \"{accelerator}\": {error}");
        }
    }

    fn is_registered(&self, accelerator: &str) -> bool {
        lock(&self.registered).contains_key(accelerator)
    }
}

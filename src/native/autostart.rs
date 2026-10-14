//! Launch at login through `SMAppService.mainAppService` (macOS 13+), with
//! the "requires approval" state surfaced.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSError};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

/// `SMAppServiceStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

impl LoginItemStatus {
    fn from_raw(raw: isize) -> Self {
        match raw {
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            3 => Self::NotFound,
            _ => Self::NotRegistered,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoStartState {
    pub enabled: bool,
    /// Registered but waiting for approval in System Settings > Login Items;
    /// unsurfaced it looks like a toggle that will not stick.
    pub requires_approval: bool,
}

pub fn resolve_auto_start_state(status: LoginItemStatus) -> AutoStartState {
    AutoStartState {
        enabled: status == LoginItemStatus::Enabled,
        requires_approval: status == LoginItemStatus::RequiresApproval,
    }
}

/// SMAppService only manages a real app bundle.
fn running_from_bundle() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

fn main_app_service() -> Option<Retained<AnyObject>> {
    if !running_from_bundle() {
        return None;
    }
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: +[SMAppService mainAppService] returns an autoreleased object.
    unsafe { msg_send![class, mainAppService] }
}

pub fn login_item_status() -> LoginItemStatus {
    let Some(service) = main_app_service() else {
        return LoginItemStatus::NotRegistered;
    };
    // SAFETY: -[SMAppService status] returns an NSInteger enum.
    let raw: isize = unsafe { msg_send![&*service, status] };
    LoginItemStatus::from_raw(raw)
}

/// `{ enabled, requiresApproval }` for the settings toggle.
pub fn auto_start_state() -> AutoStartState {
    resolve_auto_start_state(login_item_status())
}

/// Registers or unregisters the app as a login item.
pub fn set_auto_start_enabled(enabled: bool) -> Result<(), String> {
    let service = main_app_service().ok_or_else(|| {
        "Launch at login requires running from the Whisper app bundle".to_string()
    })?;
    let result: Result<(), Retained<NSError>> = if enabled {
        // SAFETY: registerAndReturnError: follows the NSError out-parameter
        // convention that `msg_send!` maps to `Result`.
        unsafe { msg_send![&*service, registerAndReturnError: _] }
    } else {
        // SAFETY: as above.
        unsafe { msg_send![&*service, unregisterAndReturnError: _] }
    };
    result.map_err(|error| error.localizedDescription().to_string())
}

const fn fourcc(code: &[u8; 4]) -> u32 {
    ((code[0] as u32) << 24) | ((code[1] as u32) << 16) | ((code[2] as u32) << 8) | code[3] as u32
}

/// Whether the session started this process (a login item launch) rather
/// than the user. Reads the launch Apple Event, so call it on the main thread
/// during application launch.
pub fn was_launched_at_login() -> bool {
    const K_AE_OPEN_APPLICATION: u32 = fourcc(b"oapp");
    const KEY_AE_PROP_DATA: u32 = fourcc(b"prdt");
    const KEY_AE_LAUNCHED_AS_LOGIN_ITEM: u32 = fourcc(b"lgit");
    let manager = NSAppleEventManager::sharedAppleEventManager();
    let Some(event) = manager.currentAppleEvent() else {
        return false;
    };
    // SAFETY: -eventID returns an AEEventID (FourCharCode).
    let event_id: u32 = unsafe { msg_send![&*event, eventID] };
    if event_id != K_AE_OPEN_APPLICATION {
        return false;
    }
    // SAFETY: -paramDescriptorForKeyword: takes an AEKeyword and returns a
    // nullable descriptor.
    let param: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&*event, paramDescriptorForKeyword: KEY_AE_PROP_DATA] };
    param.is_some_and(|param| param.enumCodeValue() == KEY_AE_LAUNCHED_AS_LOGIN_ITEM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awaiting_approval_reads_as_off_with_reason() {
        assert_eq!(
            resolve_auto_start_state(LoginItemStatus::RequiresApproval),
            AutoStartState {
                enabled: false,
                requires_approval: true
            }
        );
    }

    #[test]
    fn approved_item_is_enabled() {
        assert_eq!(
            resolve_auto_start_state(LoginItemStatus::Enabled),
            AutoStartState {
                enabled: true,
                requires_approval: false
            }
        );
    }

    #[test]
    fn disabled_item_is_neither() {
        for status in [LoginItemStatus::NotRegistered, LoginItemStatus::NotFound] {
            assert_eq!(
                resolve_auto_start_state(status),
                AutoStartState {
                    enabled: false,
                    requires_approval: false
                }
            );
        }
    }

    #[test]
    fn unbundled_test_binary_degrades_gracefully() {
        assert_eq!(login_item_status(), LoginItemStatus::NotRegistered);
        assert!(set_auto_start_enabled(true).is_err());
        assert!(!was_launched_at_login());
    }
}

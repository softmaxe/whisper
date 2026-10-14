//! Microphone and Accessibility permissions, and the System Settings panes the
//! app links to.

use std::process::Command;

use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2::msg_send;
use objc2::runtime::{AnyClass, Bool};
use objc2_foundation::NSString;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef)
    -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

/// Whether this process may post keystrokes / read other apps' UI. With
/// `prompt`, macOS shows its "grant Accessibility" dialog when not trusted.
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    // SAFETY: the key is a valid CFString constant from ApplicationServices,
    // and the dictionary outlives the call.
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

/// `AVAuthorizationStatus` for audio capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneAccess {
    NotDetermined,
    Restricted,
    Denied,
    Granted,
}

impl MicrophoneAccess {
    /// Kebab-case name of the access status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDetermined => "not-determined",
            Self::Restricted => "restricted",
            Self::Denied => "denied",
            Self::Granted => "granted",
        }
    }
}

fn capture_device_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"AVCaptureDevice")
}

pub fn microphone_access() -> MicrophoneAccess {
    let Some(class) = capture_device_class() else {
        return MicrophoneAccess::NotDetermined;
    };
    // SAFETY: +[AVCaptureDevice authorizationStatusForMediaType:] takes an
    // AVMediaType (NSString) and returns an NSInteger.
    let status: isize =
        unsafe { msg_send![class, authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        1 => MicrophoneAccess::Restricted,
        2 => MicrophoneAccess::Denied,
        3 => MicrophoneAccess::Granted,
        _ => MicrophoneAccess::NotDetermined,
    }
}

/// Asks for microphone access (shows the system prompt once) and blocks until
/// the user answers. The app bundle must carry `NSMicrophoneUsageDescription`
/// or macOS terminates the process. Call off the main thread.
pub fn request_microphone_access() -> bool {
    match microphone_access() {
        MicrophoneAccess::Granted => return true,
        MicrophoneAccess::Denied | MicrophoneAccess::Restricted => return false,
        MicrophoneAccess::NotDetermined => {}
    }
    let Some(class) = capture_device_class() else {
        return false;
    };
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let tx = std::sync::Mutex::new(Some(tx));
    let handler = RcBlock::new(move |granted: Bool| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(granted.as_bool());
        }
    });
    // SAFETY: +[AVCaptureDevice requestAccessForMediaType:completionHandler:]
    // takes an AVMediaType and a `void (^)(BOOL)` block, which is copied.
    unsafe {
        let _: () = msg_send![
            class,
            requestAccessForMediaType: AVMediaTypeAudio,
            completionHandler: &*handler
        ];
    }
    rx.recv().unwrap_or(false)
}

/// System Settings panes and their URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPane {
    Microphone,
    SoundInput,
    Accessibility,
    LoginItems,
}

impl SettingsPane {
    pub fn url(self) -> &'static str {
        match self {
            Self::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            Self::SoundInput => "x-apple.systempreferences:com.apple.preference.sound?input",
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Self::LoginItems => "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
        }
    }
}

fn open_with(args: &[&str]) -> bool {
    Command::new("/usr/bin/open")
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

/// Opens a System Settings pane. For Accessibility, falls back to the
/// Settings app itself.
pub fn open_settings(pane: SettingsPane) -> Result<(), String> {
    if open_with(&[pane.url()]) {
        return Ok(());
    }
    if pane == SettingsPane::Accessibility {
        let fallbacks: [&[&str]; 4] = [
            &["-b", "com.apple.systempreferences"],
            &["/System/Library/PreferencePanes/Security.prefPane"],
            &["-a", "System Settings"],
            &["-a", "System Preferences"],
        ];
        if fallbacks.iter().any(|args| open_with(args)) {
            return Ok(());
        }
    }
    Err(format!("Failed to open {}", pane.url()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_urls_open_the_expected_panes() {
        assert_eq!(
            SettingsPane::Microphone.url(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        );
        assert_eq!(
            SettingsPane::LoginItems.url(),
            "x-apple.systempreferences:com.apple.LoginItems-Settings.extension"
        );
    }

    #[test]
    fn status_queries_do_not_prompt() {
        // Neither call shows UI; they only read TCC state.
        let _ = is_accessibility_trusted(false);
        let _ = microphone_access().as_str();
    }
}

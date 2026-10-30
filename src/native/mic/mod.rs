//! Microphone listing and the preferred-microphone policy.
//!
//! Devices come from CoreAudio in-process: `id` is the CoreAudio device UID,
//! which is exactly `cpal::Device::id()?.id()`, and `label` is the device
//! name (`cpal` `description().name()`), so the capture side can pick the
//! same device. Lid state comes from `macos-mic-listener --watch-lid-state`.

use unicode_normalization::UnicodeNormalization;

use crate::settings::{MicrophoneSelectionMode, Settings};

mod coreaudio;
mod lid;

pub use lid::LaptopLidMonitor;

/// An audio input device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicDevice {
    /// CoreAudio device UID (stable across reboots for most devices).
    pub id: String,
    /// CoreAudio device name, as shown in System Settings and by `cpal`.
    pub label: String,
    /// CoreAudio reports a built-in transport.
    pub is_built_in: bool,
    /// The current system default input.
    pub is_default: bool,
}

/// Every audio input device, with the system default flagged.
pub fn list_input_devices() -> Vec<MicDevice> {
    coreaudio::input_devices()
}

/// The system default input's name, read in-process.
pub fn system_default_microphone() -> Option<String> {
    let id = coreaudio::default_input()?;
    Some(coreaudio::device_name(id)?.trim().to_string())
}

// ---------------------------------------------------------------------------
// Selection policy

/// Lowercased, NFKC, without "Default - " / "(Built-in)" decorations.
pub fn normalize_microphone_label(label: &str) -> String {
    let mut text: String = label.nfkc().collect();
    let trimmed = text.trim_start();
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("default") {
        let rest = trimmed[7..].trim_start();
        if let Some(rest) = rest.strip_prefix('-') {
            text = rest.trim_start().to_string();
        }
    }
    let trimmed_end = text.trim_end();
    if let Some(open) = trimmed_end.rfind('(')
        && trimmed_end.ends_with(')')
    {
        let inner = trimmed_end[open + 1..trimmed_end.len() - 1].to_lowercase();
        let decoration = matches!(
            inner.as_str(),
            "builtin" | "built-in" | "built in" | "default"
        );
        if decoration {
            text = trimmed_end[..open].to_string();
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn comparable_label(label: &str) -> String {
    normalize_microphone_label(label)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn is_built_in_microphone(label: &str) -> bool {
    let lower = label.to_lowercase();
    if ["built-in", "internal", "macbook", "integrated"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return true;
    }
    if lower.contains("microphone") {
        let without = lower.replace("microphone", " ");
        const EXTERNAL: [&str; 12] = [
            "bluetooth",
            "airpods",
            "wireless",
            "usb",
            "external",
            "headset",
            "webcam",
            "iphone",
            "ipad",
            "cell",
            "phone",
            "continuity",
        ];
        return !EXTERNAL.iter().any(|k| without.contains(k));
    }
    false
}

fn auto_built_in_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    lower.contains("built-in")
        || lower.contains("built in")
        || lower.contains("builtin")
        || lower.contains("internal")
        || lower.contains("macbook")
        || lower.contains("integrated")
}

pub fn resolve_system_default_device(
    devices: &[MicDevice],
    system_default_name: Option<&str>,
) -> Option<MicDevice> {
    let native = system_default_name.map(str::trim).filter(|n| !n.is_empty());

    if let Some(name) = native {
        let target = comparable_label(name);
        let exact: Vec<_> = devices
            .iter()
            .filter(|d| !target.is_empty() && comparable_label(&d.label) == target)
            .collect();
        if exact.len() == 1 {
            return Some(exact[0].clone());
        }
        let contained: Vec<_> = devices
            .iter()
            .filter(|d| {
                let candidate = comparable_label(&d.label);
                target.chars().count() >= 8
                    && candidate.chars().count() >= 8
                    && (candidate.contains(&target) || target.contains(&candidate))
            })
            .collect();
        if contained.len() == 1 {
            return Some(contained[0].clone());
        }
    }

    // CoreAudio flags the default directly.
    devices.iter().find(|d| d.is_default).cloned()
}

/// The saved id while it exists, else a unique label match.
pub fn resolve_mic_device_selection(
    devices: &[MicDevice],
    selected_id: &str,
    selected_label: &str,
) -> Option<MicDevice> {
    if selected_id.is_empty() {
        return None;
    }
    if let Some(device) = devices.iter().find(|d| d.id == selected_id) {
        return Some(device.clone());
    }
    if selected_label.is_empty() {
        return None;
    }
    let matches: Vec<_> = devices
        .iter()
        .filter(|d| d.label == selected_label)
        .collect();
    // An ambiguous label picks nothing.
    match matches[..] {
        [device] => Some(device.clone()),
        _ => None,
    }
}

/// The microphone `settings` select; `None` means "use the system default
/// input". Auto prefers the built-in input, unless the lid is closed, where it
/// prefers a phone (Continuity), then any other external input; with neither
/// it follows the system default.
pub fn resolve_microphone_selection(
    devices: &[MicDevice],
    settings: &Settings,
    system_default_name: Option<&str>,
    lid_closed: Option<bool>,
) -> Option<MicDevice> {
    match settings.microphone_selection_mode {
        MicrophoneSelectionMode::Auto => {
            let physical: Vec<&MicDevice> = devices.iter().filter(|d| !d.id.is_empty()).collect();
            // macOS names its internal input explicitly; the generic
            // "Microphone" heuristic also matches custom-named external mics.
            let built_in: Vec<&MicDevice> = physical
                .iter()
                .copied()
                .filter(|d| auto_built_in_label(&d.label) || d.is_built_in)
                .collect();
            let external: Vec<&MicDevice> = physical
                .iter()
                .copied()
                .filter(|d| !d.label.is_empty() && !built_in.iter().any(|b| b.id == d.id))
                .collect();
            let phone = external.iter().find(|d| {
                let lower = d.label.to_lowercase();
                lower.contains("iphone") || lower.contains("continuity")
            });
            let device = if lid_closed == Some(true) {
                phone.or(external.first()).copied()
            } else {
                built_in.first().copied()
            };
            device
                .cloned()
                .or_else(|| resolve_system_default_device(devices, system_default_name))
        }
        MicrophoneSelectionMode::BuiltIn => devices
            .iter()
            .find(|d| is_built_in_microphone(&d.label))
            .cloned(),
        MicrophoneSelectionMode::Specific => resolve_mic_device_selection(
            devices,
            &settings.selected_mic_device_id,
            &settings.selected_mic_device_label,
        ),
    }
}

/// Starts watching the laptop lid so that microphone selection finds a
/// current lid state. Spawns the helper; call once at launch, off the main
/// thread.
pub fn watch_laptop_lid() {
    LaptopLidMonitor::shared();
}

/// Lists devices and the system default through CoreAudio, reads the lid
/// state (Auto) and applies the policy. The lid read returns the monitor's
/// latest sample without waiting; an unknown state (before the first sample,
/// while the helper restarts, or on a Mac without a lid) counts as open, so
/// Auto keeps the built-in microphone.
pub fn resolve_preferred_microphone(settings: &Settings) -> Option<MicDevice> {
    let devices = list_input_devices();
    let auto = settings.microphone_selection_mode == MicrophoneSelectionMode::Auto;
    let lid_closed = auto.then(|| LaptopLidMonitor::shared().state()).flatten();
    let system_default = auto.then(system_default_microphone).flatten();
    resolve_microphone_selection(&devices, settings, system_default.as_deref(), lid_closed)
}

#[cfg(test)]
mod tests {
    //! Microphone selection and device selection.
    use super::*;

    fn mic(id: &str, label: &str) -> MicDevice {
        MicDevice {
            id: id.into(),
            label: label.into(),
            is_built_in: false,
            is_default: false,
        }
    }

    fn pref(mode: MicrophoneSelectionMode) -> Settings {
        Settings {
            microphone_selection_mode: mode,
            ..Settings::default()
        }
    }

    fn specific(selected_id: &str, selected_label: &str) -> Settings {
        Settings {
            microphone_selection_mode: MicrophoneSelectionMode::Specific,
            selected_mic_device_id: selected_id.into(),
            selected_mic_device_label: selected_label.into(),
            ..Settings::default()
        }
    }

    #[test]
    fn auto_follows_lid_changes() {
        let built_in = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let phone = mic("iphone", "Rui's iPhone 16 Pro Max Microphone");
        let usb = mic("usb", "USB Microphone");
        let devices = vec![usb, phone.clone(), built_in.clone()];
        for lid in [false, true, false, true] {
            let result = resolve_microphone_selection(
                &devices,
                &pref(MicrophoneSelectionMode::Auto),
                Some(&phone.label),
                Some(lid),
            );
            assert_eq!(result.as_ref(), Some(if lid { &phone } else { &built_in }));
        }
    }

    #[test]
    fn auto_with_lid_closed_tracks_external_devices() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let phone = mic("iphone", "Rui's iPhone Microphone");
        let external = mic("usb", "Studio Microphone");
        let resolve = |devices: Vec<MicDevice>| {
            resolve_microphone_selection(
                &devices,
                &pref(MicrophoneSelectionMode::Auto),
                None,
                Some(true),
            )
        };
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone()]),
            Some(external.clone())
        );
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone(), phone.clone()]),
            Some(phone)
        );
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone()]),
            Some(external)
        );
        assert_eq!(resolve(vec![built_in]), None);
    }

    #[test]
    fn auto_with_lid_closed_falls_back_to_system_default() {
        let mut built_in = mic("macbook", "MacBook Pro Microphone");
        built_in.is_default = true;
        let result = resolve_microphone_selection(
            std::slice::from_ref(&built_in),
            &pref(MicrophoneSelectionMode::Auto),
            None,
            Some(true),
        );
        assert_eq!(result, Some(built_in));
    }

    #[test]
    fn auto_with_unknown_lid_prefers_built_in() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let result = resolve_microphone_selection(
            &[mic("continuity", "Joshua P Microphone"), built_in.clone()],
            &pref(MicrophoneSelectionMode::Auto),
            None,
            None,
        );
        assert_eq!(result, Some(built_in));
    }

    #[test]
    fn manual_input_ignores_the_lid() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let phone = mic("iphone", "Rui's iPhone Microphone");
        for lid in [true, false] {
            let result = resolve_microphone_selection(
                &[built_in.clone(), phone.clone()],
                &specific("iphone", ""),
                None,
                Some(lid),
            );
            assert_eq!(result, Some(phone.clone()));
        }
    }

    #[test]
    fn system_default_mapping() {
        let expected = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let device = resolve_system_default_device(
            &[mic("usb", "USB Microphone"), expected.clone()],
            Some("MacBook Pro Microphone"),
        );
        assert_eq!(device, Some(expected));

        let device = resolve_system_default_device(
            &[
                mic("continuity", "Joshua P Microphone"),
                mic("usb", "USB Microphone"),
            ],
            Some("USB Microphone"),
        );
        assert!(device.is_some_and(|d| !d.id.is_empty()));

        let device = resolve_system_default_device(
            &[
                mic("continuity", "Joshua P Microphone"),
                mic("usb", "USB Microphone"),
            ],
            Some("Missing System Microphone"),
        );
        assert_eq!(device, None);

        // A unique containing label matches.
        let expected = mic("7a1b", "Jabra Evolve2 65 Microphone");
        let device = resolve_system_default_device(
            &[
                mic("macbook", "MacBook Pro Microphone (Built-in)"),
                expected.clone(),
            ],
            Some("Jabra Evolve2 65"),
        );
        assert_eq!(device, Some(expected));
    }

    #[test]
    fn coreaudio_default_flag_is_used() {
        let mut default = mic("uid-1", "Desk Microphone");
        default.is_default = true;
        let device = resolve_system_default_device(&[mic("uid-2", "Other"), default.clone()], None);
        assert_eq!(device, Some(default));
    }

    #[test]
    fn built_in_mode_pins_the_physical_device() {
        let expected = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let result = resolve_microphone_selection(
            &[expected.clone(), mic("7a1b", "Jabra Evolve2 65")],
            &pref(MicrophoneSelectionMode::BuiltIn),
            None,
            None,
        );
        assert_eq!(result, Some(expected));
        assert_eq!(
            resolve_microphone_selection(
                &[mic("7a1b", "Jabra Evolve2 65")],
                &pref(MicrophoneSelectionMode::BuiltIn),
                None,
                None,
            ),
            None
        );
    }

    #[test]
    fn device_selection_by_id_then_unique_label() {
        let device = mic("saved-id", "Studio Mic");
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "saved-id", "Studio Mic"),
            Some(device)
        );
        let device = mic("new-id", "Studio Mic");
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "old-id", "Studio Mic"),
            Some(device.clone())
        );
        assert_eq!(
            resolve_mic_device_selection(
                &[mic("first", "USB Audio"), mic("second", "USB Audio")],
                "old-id",
                "USB Audio"
            ),
            None,
            "an ambiguous label picks nothing"
        );
        assert_eq!(
            resolve_mic_device_selection(&[], "old-id", "Studio Mic"),
            None
        );
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "old-id", ""),
            None
        );
        assert_eq!(resolve_mic_device_selection(&[device], "", ""), None);
    }

    #[test]
    fn labels_normalize() {
        assert_eq!(
            normalize_microphone_label("Default - MacBook Pro Microphone (Built-in)"),
            "macbook pro microphone"
        );
        assert_eq!(
            normalize_microphone_label("  USB   Mic (default) "),
            "usb mic"
        );
        assert!(is_built_in_microphone("MacBook Air Microphone"));
        assert!(!is_built_in_microphone("Galaxy Cell Microphone"));
        assert!(!is_built_in_microphone("AirPods Pro"));
    }

    #[test]
    fn stale_id_is_remapped_by_label() {
        let result = resolve_microphone_selection(
            &[mic("new-id", "Studio Mic")],
            &specific("old-id", "Studio Mic"),
            None,
            None,
        );
        assert_eq!(result.map(|d| d.id).as_deref(), Some("new-id"));
    }

    #[test]
    #[ignore = "reads real audio hardware"]
    fn lists_real_devices() {
        let devices = list_input_devices();
        assert!(devices.iter().all(|d| !d.id.is_empty()));
        println!("{devices:#?} default={:?}", system_default_microphone());
    }
}

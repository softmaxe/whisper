//! Generic input-device enumeration through cpal. macOS-specific policy
//! (built-in mic, lid state) lives elsewhere and builds on these ids.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use super::AudioError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputDeviceInfo {
    /// Stable cpal device id (`"coreaudio:<uid>"`); pass it back to open the device.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    pub is_default: bool,
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| device.to_string())
}

fn device_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

/// Lists input-capable devices on the default host.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, AudioError> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().as_ref().and_then(device_id);
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::Open(e.to_string()))?;
    Ok(devices
        .filter_map(|device| {
            let id = device_id(&device)?;
            Some(InputDeviceInfo {
                is_default: default_id.as_deref() == Some(id.as_str()),
                name: device_name(&device),
                id,
            })
        })
        .collect())
}

/// Opens the device named by `selector` (full id, backend id, or name), or
/// the system default when `None`/empty. An unknown selector is an error:
/// a recording never silently moves to a different microphone.
pub(crate) fn find_input_device(
    host: &cpal::Host,
    selector: Option<&str>,
) -> Result<cpal::Device, AudioError> {
    let Some(selector) = selector.map(str::trim).filter(|s| !s.is_empty()) else {
        return host
            .default_input_device()
            .ok_or(AudioError::NoDefaultDevice);
    };
    if let Ok(id) = selector.parse::<cpal::DeviceId>()
        && let Some(device) = host.device_by_id(&id)
    {
        return Ok(device);
    }
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::Open(e.to_string()))?
        .collect::<Vec<_>>();
    let by_id = devices.iter().find(|d| {
        d.id()
            .is_ok_and(|id| id.to_string() == selector || id.id() == selector)
    });
    by_id
        .or_else(|| devices.iter().find(|d| device_name(d) == selector))
        .cloned()
        .ok_or_else(|| AudioError::DeviceNotFound(selector.to_string()))
}

/// Display name of the device a selector resolves to (for logs and UI).
pub fn resolve_device_name(selector: Option<&str>) -> Option<String> {
    find_input_device(&cpal::default_host(), selector)
        .ok()
        .map(|d| device_name(&d))
}

pub(crate) fn describe(device: &cpal::Device) -> String {
    device_name(device)
}

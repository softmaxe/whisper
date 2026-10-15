//! Generic input-device enumeration through cpal. macOS-specific policy
//! (built-in mic, lid state) lives elsewhere and builds on these ids.

use super::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| device.to_string())
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

pub(crate) fn describe(device: &cpal::Device) -> String {
    device_name(device)
}
